use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tokio_vsock::{VsockAddr, VsockStream};
use uuid::Uuid;

use crate::error::ApiResult;
use crate::models::{TaskFile, VsockMessage, WsMessage};
use crate::ws::WsRegistry;

/// vsock port used by the agent sidecar in the VM
const VSOCK_PORT: u32 = 5000;

/// Maximum connection attempts (600 * 100ms = 60 seconds)
const MAX_ATTEMPTS: u32 = 600;

pub struct VsockRelay {
    task_id: Uuid,
    guest_cid: u32,
    ws_registry: Arc<WsRegistry>,
}

impl VsockRelay {
    pub fn new(task_id: Uuid, guest_cid: u32, ws_registry: Arc<WsRegistry>) -> Self {
        Self {
            task_id,
            guest_cid,
            ws_registry,
        }
    }

    pub async fn start(
        &self,
        api_key: String,
        prompt: String,
        files: Option<Vec<TaskFile>>,
    ) -> ApiResult<mpsc::Sender<String>> {
        // Create channel for sending input to the VM
        let (input_tx, mut input_rx) = mpsc::channel::<String>(100);

        let task_id = self.task_id;
        let guest_cid = self.guest_cid;
        let ws_registry = self.ws_registry.clone();

        // Connect to the VM via vsock
        // QEMU's vhost-vsock-pci device allows direct AF_VSOCK connections
        // The guest CID is assigned when creating the VM
        let vsock_addr = VsockAddr::new(guest_cid, VSOCK_PORT);
        let mut attempts = 0;
        let stream = loop {
            match VsockStream::connect(vsock_addr).await {
                Ok(stream) => {
                    tracing::info!(
                        "vsock connection established to CID {} port {}",
                        guest_cid,
                        VSOCK_PORT
                    );
                    break stream;
                }
                Err(e) => {
                    attempts += 1;
                    if attempts > MAX_ATTEMPTS {
                        return Err(crate::error::ApiError::VmError(format!(
                            "Failed to connect to vsock (CID {}, port {}) after {}s: {}",
                            guest_cid,
                            VSOCK_PORT,
                            MAX_ATTEMPTS / 10,
                            e
                        )));
                    }
                    // Log every 50 attempts (5 seconds)
                    if attempts % 50 == 0 {
                        tracing::debug!(
                            "Waiting for vsock connection to CID {} (attempt {}/{}): {}",
                            guest_cid,
                            attempts,
                            MAX_ATTEMPTS,
                            e
                        );
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            }
        };

        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        // Send init message
        let init_msg = VsockMessage::Init {
            api_key,
            prompt,
            files,
        };
        let init_json = serde_json::to_string(&init_msg).unwrap() + "\n";
        writer.write_all(init_json.as_bytes()).await.map_err(|e| {
            crate::error::ApiError::VmError(format!("Failed to send init message: {}", e))
        })?;
        writer.flush().await.map_err(|e| {
            crate::error::ApiError::VmError(format!("Failed to flush init message: {}", e))
        })?;

        // Spawn reader task
        let ws_registry_clone = ws_registry.clone();
        tokio::spawn(async move {
            tracing::info!("vsock reader task started for task {}", task_id);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        // EOF - connection closed
                        tracing::info!("vsock connection closed for task {}", task_id);
                        break;
                    }
                    Ok(n) => {
                        tracing::debug!("vsock received {} bytes for task {}: {}", n, task_id, line.trim());
                        match serde_json::from_str::<VsockMessage>(&line) {
                            Ok(msg) => {
                                match msg {
                                    VsockMessage::Output { data } => {
                                        tracing::debug!("Broadcasting output for task {}", task_id);
                                        let ws_msg = WsMessage::Output {
                                            data,
                                            timestamp: chrono::Utc::now().timestamp_millis(),
                                        };
                                        ws_registry_clone.broadcast(task_id, ws_msg).await;
                                    }
                                    VsockMessage::Exit { code } => {
                                        tracing::info!("Task {} exited with code {}", task_id, code);
                                        let ws_msg = WsMessage::Status {
                                            status: crate::models::TaskStatus::Terminated,
                                            exit_code: Some(code),
                                        };
                                        ws_registry_clone.broadcast(task_id, ws_msg).await;
                                        break;
                                    }
                                    VsockMessage::Error { message } => {
                                        tracing::error!("Sidecar error for task {}: {}", task_id, message);
                                        let ws_msg = WsMessage::Error { message };
                                        ws_registry_clone.broadcast(task_id, ws_msg).await;
                                    }
                                    VsockMessage::Heartbeat => {
                                        tracing::debug!("Heartbeat received for task {}", task_id);
                                    }
                                    _ => {
                                        tracing::debug!("Unknown message type for task {}", task_id);
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Failed to parse vsock message for task {}: {} (raw: {})", task_id, e, line.trim());
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Error reading from vsock: {}", e);
                        break;
                    }
                }
            }
        });

        // Spawn writer task for input
        tokio::spawn(async move {
            while let Some(input) = input_rx.recv().await {
                let input_msg = VsockMessage::Input { data: input };
                let json = serde_json::to_string(&input_msg).unwrap() + "\n";
                if writer.write_all(json.as_bytes()).await.is_err() {
                    break;
                }
                if writer.flush().await.is_err() {
                    break;
                }
            }
        });

        Ok(input_tx)
    }
}
