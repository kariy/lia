use std::path::PathBuf;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::error::ApiResult;
use crate::models::{TaskFile, VsockMessage, WsMessage};
use crate::ws::WsRegistry;

pub struct VsockRelay {
    task_id: Uuid,
    vsock_path: PathBuf,
    ws_registry: Arc<WsRegistry>,
}

impl VsockRelay {
    pub fn new(task_id: Uuid, vsock_path: PathBuf, ws_registry: Arc<WsRegistry>) -> Self {
        Self {
            task_id,
            vsock_path,
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
        let vsock_path = self.vsock_path.clone();
        let ws_registry = self.ws_registry.clone();

        // Wait for vsock to be ready and establish connection
        // Firecracker vsock protocol: connect to UDS, send "CONNECT <port>\n", read "OK <local_port>\n"
        // Debian takes ~30 seconds to boot, so we retry for up to 60 seconds
        const VSOCK_PORT: u32 = 5000;
        const MAX_ATTEMPTS: u32 = 600; // 600 * 100ms = 60 seconds
        let mut attempts = 0;
        let stream = loop {
            match UnixStream::connect(&vsock_path).await {
                Ok(mut stream) => {
                    // Send CONNECT command to accept guest-initiated connection
                    let connect_cmd = format!("CONNECT {}\n", VSOCK_PORT);
                    if let Err(e) = stream.write_all(connect_cmd.as_bytes()).await {
                        tracing::warn!("Failed to send CONNECT command: {}", e);
                        attempts += 1;
                        if attempts > MAX_ATTEMPTS {
                            return Err(crate::error::ApiError::VmError(format!(
                                "Failed to establish vsock connection after {} attempts ({}s)",
                                MAX_ATTEMPTS, MAX_ATTEMPTS / 10
                            )));
                        }
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        continue;
                    }

                    // Read response (should be "OK <local_port>\n")
                    let mut response = vec![0u8; 32];
                    match stream.read(&mut response).await {
                        Ok(n) if n > 0 => {
                            let response_str = String::from_utf8_lossy(&response[..n]);
                            if response_str.starts_with("OK ") {
                                tracing::info!("vsock connection established: {}", response_str.trim());
                                break stream;
                            } else {
                                tracing::warn!("Unexpected vsock response: {}", response_str.trim());
                                attempts += 1;
                                if attempts > MAX_ATTEMPTS {
                                    return Err(crate::error::ApiError::VmError(format!(
                                        "Failed to establish vsock connection: unexpected response"
                                    )));
                                }
                                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                                continue;
                            }
                        }
                        Ok(_) => {
                            tracing::warn!("Empty vsock response");
                        }
                        Err(e) => {
                            tracing::warn!("Failed to read vsock response: {}", e);
                        }
                    }
                    attempts += 1;
                    if attempts > MAX_ATTEMPTS {
                        return Err(crate::error::ApiError::VmError(format!(
                            "Failed to establish vsock connection after {}s",
                            MAX_ATTEMPTS / 10
                        )));
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
                Err(e) => {
                    attempts += 1;
                    if attempts > MAX_ATTEMPTS {
                        return Err(crate::error::ApiError::VmError(format!(
                            "Failed to connect to vsock after {}s: {}",
                            MAX_ATTEMPTS / 10, e
                        )));
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
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        // EOF - connection closed
                        tracing::info!("vsock connection closed for task {}", task_id);
                        break;
                    }
                    Ok(_) => {
                        if let Ok(msg) = serde_json::from_str::<VsockMessage>(&line) {
                            match msg {
                                VsockMessage::Output { data } => {
                                    let ws_msg = WsMessage::Output {
                                        data,
                                        timestamp: chrono::Utc::now().timestamp_millis(),
                                    };
                                    ws_registry_clone.broadcast(task_id, ws_msg).await;
                                }
                                VsockMessage::Exit { code } => {
                                    let ws_msg = WsMessage::Status {
                                        status: crate::models::TaskStatus::Terminated,
                                        exit_code: Some(code),
                                    };
                                    ws_registry_clone.broadcast(task_id, ws_msg).await;
                                    break;
                                }
                                VsockMessage::Error { message } => {
                                    tracing::error!("Sidecar error for task {}: {}", task_id, message);
                                    // Broadcast error to WebSocket clients
                                    let ws_msg = WsMessage::Error { message };
                                    ws_registry_clone.broadcast(task_id, ws_msg).await;
                                }
                                VsockMessage::Heartbeat => {
                                    // Heartbeat received, no action needed
                                }
                                _ => {}
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
