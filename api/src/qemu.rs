use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::Command;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::config::AppConfig;
use crate::error::{ApiError, ApiResult};
use crate::models::{BootStage, TaskConfig};

/// Callback type for reporting VM creation progress
pub type ProgressCallback = Box<dyn Fn(BootStage) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct VmInfo {
    pub vm_id: String,
    pub task_id: Uuid,
    pub cid: u32,
    pub qmp_socket_path: PathBuf,
    pub volume_path: PathBuf,
    pub log_path: PathBuf,
    pub pid_file: PathBuf,
    pub pid: Option<u32>,
    // Network info
    pub tap_name: String,
    pub ip_address: String,
    pub gateway: String,
}

/// QMP (QEMU Machine Protocol) response types
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct QmpGreeting {
    #[serde(rename = "QMP")]
    qmp: QmpVersion,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct QmpVersion {
    version: QmpVersionInfo,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct QmpVersionInfo {
    qemu: QmpQemuVersion,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct QmpQemuVersion {
    major: u32,
    minor: u32,
}

#[derive(Debug, Serialize)]
struct QmpCommand {
    execute: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct QmpResponse {
    #[serde(rename = "return")]
    result: Option<serde_json::Value>,
    error: Option<QmpError>,
}

#[derive(Debug, Deserialize)]
struct QmpError {
    class: String,
    desc: String,
}

/// QMP Client for controlling QEMU VMs
pub struct QmpClient {
    socket_path: PathBuf,
}

impl QmpClient {
    pub fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }

    async fn connect(&self) -> ApiResult<UnixStream> {
        UnixStream::connect(&self.socket_path)
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to connect to QMP socket: {}", e)))
    }

    async fn send_command(
        &self,
        command: &str,
        arguments: Option<serde_json::Value>,
    ) -> ApiResult<serde_json::Value> {
        let mut stream = self.connect().await?;
        let (reader, mut writer) = stream.split();
        let mut reader = BufReader::new(reader);

        // Read QMP greeting
        let mut greeting_line = String::new();
        reader
            .read_line(&mut greeting_line)
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to read QMP greeting: {}", e)))?;

        // Parse greeting to verify it's QMP
        let _greeting: QmpGreeting = serde_json::from_str(&greeting_line)
            .map_err(|e| ApiError::VmError(format!("Failed to parse QMP greeting: {}", e)))?;

        // Send qmp_capabilities to enter command mode
        let caps_cmd = QmpCommand {
            execute: "qmp_capabilities".to_string(),
            arguments: None,
        };
        let caps_json = serde_json::to_string(&caps_cmd).unwrap() + "\n";
        writer
            .write_all(caps_json.as_bytes())
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to send qmp_capabilities: {}", e)))?;
        writer
            .flush()
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to flush qmp_capabilities: {}", e)))?;

        // Read capabilities response
        let mut caps_response = String::new();
        reader.read_line(&mut caps_response).await.map_err(|e| {
            ApiError::VmError(format!("Failed to read qmp_capabilities response: {}", e))
        })?;

        // Send the actual command
        let cmd = QmpCommand {
            execute: command.to_string(),
            arguments,
        };
        let cmd_json = serde_json::to_string(&cmd).unwrap() + "\n";
        writer
            .write_all(cmd_json.as_bytes())
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to send QMP command: {}", e)))?;
        writer
            .flush()
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to flush QMP command: {}", e)))?;

        // Read response
        let mut response_line = String::new();
        reader
            .read_line(&mut response_line)
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to read QMP response: {}", e)))?;

        let response: QmpResponse = serde_json::from_str(&response_line)
            .map_err(|e| ApiError::VmError(format!("Failed to parse QMP response: {}", e)))?;

        if let Some(error) = response.error {
            return Err(ApiError::VmError(format!(
                "QMP error ({}): {}",
                error.class, error.desc
            )));
        }

        Ok(response.result.unwrap_or(serde_json::Value::Null))
    }

    /// Pause the VM (QMP "stop" command)
    pub async fn pause(&self) -> ApiResult<()> {
        self.send_command("stop", None).await?;
        Ok(())
    }

    /// Resume the VM (QMP "cont" command)
    pub async fn resume(&self) -> ApiResult<()> {
        self.send_command("cont", None).await?;
        Ok(())
    }

    /// Graceful shutdown (QMP "system_powerdown" command)
    #[allow(dead_code)]
    pub async fn shutdown(&self) -> ApiResult<()> {
        self.send_command("system_powerdown", None).await?;
        Ok(())
    }

    /// Force quit (QMP "quit" command)
    pub async fn quit(&self) -> ApiResult<()> {
        self.send_command("quit", None).await?;
        Ok(())
    }

    /// Query VM status
    #[allow(dead_code)]
    pub async fn query_status(&self) -> ApiResult<String> {
        let result = self.send_command("query-status", None).await?;
        Ok(result
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("unknown")
            .to_string())
    }
}

pub struct VmManager {
    config: AppConfig,
    vms: Arc<RwLock<HashMap<String, VmInfo>>>,
    next_cid: AtomicU32,
    next_ip: AtomicU32,
}

impl VmManager {
    pub fn new(config: AppConfig) -> Self {
        Self {
            next_cid: AtomicU32::new(config.vm.vsock_cid_start),
            next_ip: AtomicU32::new(100), // Start from 172.16.0.100
            config,
            vms: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Allocate the next available IP address
    fn allocate_ip(&self) -> String {
        let last_octet = self.next_ip.fetch_add(1, Ordering::SeqCst);
        // Wrap around if we exceed 254
        let last_octet = if last_octet > 254 {
            self.next_ip.store(101, Ordering::SeqCst);
            100
        } else {
            last_octet
        };
        format!("172.16.0.{}", last_octet)
    }

    /// Generate a MAC address based on the last octet of the IP
    fn generate_mac(&self, ip: &str) -> String {
        let last_octet: u8 = ip.split('.').last().unwrap().parse().unwrap_or(100);
        format!("02:FC:00:00:00:{:02X}", last_octet)
    }

    /// Create a TAP device and attach it to the bridge
    async fn create_tap(&self, tap_name: &str) -> ApiResult<()> {
        let output = Command::new("lia-create-tap")
            .arg(tap_name)
            .arg(&self.config.network.bridge_name)
            .output()
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to create TAP device: {}", e)))?;

        if !output.status.success() {
            return Err(ApiError::VmError(format!(
                "Failed to create TAP device: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(())
    }

    /// Delete a TAP device
    async fn delete_tap(&self, tap_name: &str) -> ApiResult<()> {
        let output = Command::new("lia-delete-tap")
            .arg(tap_name)
            .output()
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to delete TAP device: {}", e)))?;

        if !output.status.success() {
            tracing::warn!(
                "Failed to delete TAP device {}: {}",
                tap_name,
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    pub async fn create_vm(
        &self,
        task_id: Uuid,
        task_config: Option<&TaskConfig>,
        ssh_public_key: Option<&str>,
    ) -> ApiResult<VmInfo> {
        self.create_vm_with_progress(task_id, task_config, ssh_public_key, None)
            .await
    }

    pub async fn create_vm_with_progress(
        &self,
        task_id: Uuid,
        task_config: Option<&TaskConfig>,
        ssh_public_key: Option<&str>,
        on_progress: Option<ProgressCallback>,
    ) -> ApiResult<VmInfo> {
        let report_progress = |stage: BootStage| {
            if let Some(ref callback) = on_progress {
                callback(stage);
            }
        };

        let vm_id = format!("vm-{}", task_id);
        let cid = self.next_cid.fetch_add(1, Ordering::SeqCst);

        // Allocate network resources
        let ip_address = self.allocate_ip();
        let gateway = self.config.network.bridge_ip.clone();
        let tap_name = format!("tap-{}", &task_id.to_string()[..8]);
        let mac_address = self.generate_mac(&ip_address);

        // Create paths
        let qmp_socket_path =
            PathBuf::from(&self.config.qemu.sockets_dir).join(format!("{}.qmp", vm_id));
        let volume_path =
            PathBuf::from(&self.config.qemu.volumes_dir).join(format!("{}.ext4", task_id));
        let log_path = PathBuf::from(&self.config.qemu.logs_dir).join(format!("{}.log", vm_id));
        let pid_file = PathBuf::from(&self.config.qemu.pids_dir).join(format!("{}.pid", vm_id));

        // Ensure directories exist
        tokio::fs::create_dir_all(&self.config.qemu.sockets_dir)
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to create sockets dir: {}", e)))?;
        tokio::fs::create_dir_all(&self.config.qemu.volumes_dir)
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to create volumes dir: {}", e)))?;
        tokio::fs::create_dir_all(&self.config.qemu.logs_dir)
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to create logs dir: {}", e)))?;
        tokio::fs::create_dir_all(&self.config.qemu.pids_dir)
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to create pids dir: {}", e)))?;

        // Create TAP device
        self.create_tap(&tap_name).await?;

        // Create sparse volume file
        let storage_gb = task_config
            .map(|c| c.storage_gb)
            .unwrap_or(self.config.vm.default_storage_gb);
        self.create_sparse_volume(&volume_path, storage_gb).await?;

        // Copy rootfs for this VM
        let vm_rootfs_path =
            PathBuf::from(&self.config.qemu.volumes_dir).join(format!("{}-rootfs.ext4", task_id));
        tokio::fs::copy(&self.config.qemu.rootfs_path, &vm_rootfs_path)
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to copy rootfs: {}", e)))?;

        // Report: configuring VM
        report_progress(BootStage::ConfiguringVm);

        // Get VM resource configuration
        let vcpu_count = task_config
            .map(|c| c.vcpu_count)
            .unwrap_or(self.config.vm.default_vcpu_count);
        let mem_size_mib = task_config
            .map(|c| c.max_memory_mb)
            .unwrap_or(self.config.vm.default_memory_mb);

        // Build kernel command line
        let ssh_key_arg = ssh_public_key
            .map(|k| format!(" lia.ssh_key={}", k.replace(' ', "^")))
            .unwrap_or_default();

        let kernel_cmdline = format!(
            "console=ttyS0 root=/dev/vda rw init=/sbin/init lia.ip={} lia.gateway={}{}",
            ip_address, gateway, ssh_key_arg
        );

        // Build QEMU command
        let mut qemu_cmd = Command::new(&self.config.qemu.bin_path);

        // Machine and CPU configuration
        qemu_cmd
            .arg("-M")
            .arg(&self.config.qemu.machine_type)
            .arg("-cpu")
            .arg("host")
            .arg("-enable-kvm")
            .arg("-m")
            .arg(format!("{}M", mem_size_mib))
            .arg("-smp")
            .arg(vcpu_count.to_string());

        // Display configuration (headless, use -display none for daemonize compatibility)
        qemu_cmd.arg("-display").arg("none").arg("-vga").arg("none");

        // Kernel configuration
        qemu_cmd
            .arg("-kernel")
            .arg(&self.config.qemu.kernel_path)
            .arg("-append")
            .arg(&kernel_cmdline);

        // Drives: rootfs and data volume
        qemu_cmd
            .arg("-drive")
            .arg(format!(
                "file={},format=raw,if=virtio,id=rootfs",
                vm_rootfs_path.display()
            ))
            .arg("-drive")
            .arg(format!(
                "file={},format=raw,if=virtio,id=data",
                volume_path.display()
            ));

        // Network configuration
        qemu_cmd
            .arg("-netdev")
            .arg(format!(
                "tap,id=net0,ifname={},script=no,downscript=no",
                tap_name
            ))
            .arg("-device")
            .arg(format!("virtio-net-pci,netdev=net0,mac={}", mac_address));

        // vsock device for host-guest communication
        qemu_cmd
            .arg("-device")
            .arg(format!("vhost-vsock-pci,guest-cid={}", cid));

        // QMP socket for runtime control
        qemu_cmd
            .arg("-qmp")
            .arg(format!("unix:{},server,nowait", qmp_socket_path.display()));

        // Serial output to log file
        qemu_cmd
            .arg("-serial")
            .arg(format!("file:{}", log_path.display()));

        // Daemonize and create PID file
        qemu_cmd.arg("-daemonize").arg("-pidfile").arg(&pid_file);

        // Configure stdio
        qemu_cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        tracing::info!("Starting QEMU VM {} with CID {}", vm_id, cid);
        tracing::debug!("QEMU command: {:?}", qemu_cmd);

        // Start QEMU
        let output = qemu_cmd
            .output()
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to start QEMU: {}", e)))?;

        if !output.status.success() {
            // Clean up TAP device on failure
            let _ = self.delete_tap(&tap_name).await;
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(ApiError::VmError(format!(
                "QEMU failed to start: {} {}",
                stderr, stdout
            )));
        }

        // Report: waiting for QMP socket
        report_progress(BootStage::WaitingForSocket);

        // Wait for QMP socket to be ready
        self.wait_for_socket(&qmp_socket_path).await?;

        // Read PID from pidfile
        let pid = self.read_pid_file(&pid_file).await.ok();

        // Report: VM is now booting
        report_progress(BootStage::BootingVm);

        let vm_info = VmInfo {
            vm_id: vm_id.clone(),
            task_id,
            cid,
            qmp_socket_path,
            volume_path,
            log_path,
            pid_file,
            pid,
            tap_name,
            ip_address,
            gateway,
        };

        // Store VM info
        self.vms
            .write()
            .await
            .insert(vm_id.clone(), vm_info.clone());

        Ok(vm_info)
    }

    async fn create_sparse_volume(&self, path: &PathBuf, size_gb: u32) -> ApiResult<()> {
        let file = tokio::fs::File::create(path)
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to create volume file: {}", e)))?;

        let size_bytes = (size_gb as u64) * 1024 * 1024 * 1024;
        file.set_len(size_bytes)
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to set volume size: {}", e)))?;

        // Format as ext4
        let output = Command::new("mkfs.ext4")
            .arg("-F")
            .arg(path)
            .output()
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to format volume: {}", e)))?;

        if !output.status.success() {
            return Err(ApiError::VmError(format!(
                "mkfs.ext4 failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(())
    }

    async fn wait_for_socket(&self, socket_path: &PathBuf) -> ApiResult<()> {
        for _ in 0..50 {
            if socket_path.exists() {
                // Additional delay to ensure socket is ready
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                return Ok(());
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
        Err(ApiError::VmError(
            "Timeout waiting for QMP socket".to_string(),
        ))
    }

    async fn read_pid_file(&self, pid_file: &PathBuf) -> ApiResult<u32> {
        // Wait a bit for the pidfile to be written
        for _ in 0..20 {
            if pid_file.exists() {
                let content = tokio::fs::read_to_string(pid_file)
                    .await
                    .map_err(|e| ApiError::VmError(format!("Failed to read PID file: {}", e)))?;
                let pid: u32 = content
                    .trim()
                    .parse()
                    .map_err(|e| ApiError::VmError(format!("Failed to parse PID: {}", e)))?;
                return Ok(pid);
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
        Err(ApiError::VmError("PID file not found".to_string()))
    }

    pub async fn start_vm(&self, vm_id: &str) -> ApiResult<()> {
        let vms = self.vms.read().await;
        let vm_info = vms
            .get(vm_id)
            .ok_or_else(|| ApiError::VmError(format!("VM not found: {}", vm_id)))?;

        let qmp = QmpClient::new(vm_info.qmp_socket_path.clone());
        qmp.resume().await
    }

    pub async fn pause_vm(&self, vm_id: &str) -> ApiResult<()> {
        let vms = self.vms.read().await;
        let vm_info = vms
            .get(vm_id)
            .ok_or_else(|| ApiError::VmError(format!("VM not found: {}", vm_id)))?;

        let qmp = QmpClient::new(vm_info.qmp_socket_path.clone());
        qmp.pause().await
    }

    pub async fn resume_vm(&self, vm_id: &str) -> ApiResult<()> {
        let vms = self.vms.read().await;
        let vm_info = vms
            .get(vm_id)
            .ok_or_else(|| ApiError::VmError(format!("VM not found: {}", vm_id)))?;

        let qmp = QmpClient::new(vm_info.qmp_socket_path.clone());
        qmp.resume().await
    }

    pub async fn stop_vm(&self, vm_id: &str) -> ApiResult<()> {
        // Remove from tracking
        let vm_info = self.vms.write().await.remove(vm_id);

        if let Some(info) = vm_info {
            // Try graceful shutdown via QMP first
            let qmp = QmpClient::new(info.qmp_socket_path.clone());
            if let Err(e) = qmp.quit().await {
                tracing::warn!("QMP quit failed: {}, falling back to SIGTERM", e);

                // Fallback: kill by PID
                if let Some(pid) = info.pid {
                    let _ = Command::new("kill")
                        .arg("-TERM")
                        .arg(pid.to_string())
                        .output()
                        .await;

                    // Wait a bit and force kill if needed
                    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                    let _ = Command::new("kill")
                        .arg("-KILL")
                        .arg(pid.to_string())
                        .output()
                        .await;
                }
            }

            // Delete TAP device
            let _ = self.delete_tap(&info.tap_name).await;

            // Cleanup files
            let _ = tokio::fs::remove_file(&info.qmp_socket_path).await;
            let _ = tokio::fs::remove_file(&info.volume_path).await;
            let _ = tokio::fs::remove_file(&info.log_path).await;
            let _ = tokio::fs::remove_file(&info.pid_file).await;

            // Also remove the copied rootfs
            let rootfs_copy = PathBuf::from(&self.config.qemu.volumes_dir)
                .join(format!("{}-rootfs.ext4", info.task_id));
            let _ = tokio::fs::remove_file(&rootfs_copy).await;
        }

        Ok(())
    }

    pub async fn get_vm_info(&self, vm_id: &str) -> Option<VmInfo> {
        self.vms.read().await.get(vm_id).cloned()
    }

    /// Get the CID for connecting to the VM via vsock
    #[allow(dead_code)]
    pub fn get_vm_cid(&self, _vm_id: &str) -> Option<u32> {
        // This is a sync version that returns CID from the vm_id
        // The actual VM info lookup happens async elsewhere
        None
    }

    /// Get VM CID from task_id (async version)
    pub async fn get_cid_for_task(&self, task_id: Uuid) -> Option<u32> {
        let vm_id = format!("vm-{}", task_id);
        self.vms.read().await.get(&vm_id).map(|info| info.cid)
    }
}
