use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::process::{Child, Command};
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
    pub socket_path: PathBuf,
    pub volume_path: PathBuf,
    pub log_path: PathBuf,
    pub pid: Option<u32>,
    // Network info
    pub tap_name: String,
    pub ip_address: String,
    pub gateway: String,
}

// Firecracker API request/response types
#[derive(Debug, Serialize)]
struct BootSource {
    kernel_image_path: String,
    boot_args: String,
}

#[derive(Debug, Serialize)]
struct Drive {
    drive_id: String,
    path_on_host: String,
    is_root_device: bool,
    is_read_only: bool,
}

#[derive(Debug, Serialize)]
struct MachineConfig {
    vcpu_count: u32,
    mem_size_mib: u32,
}

#[derive(Debug, Serialize)]
struct Vsock {
    guest_cid: u32,
    uds_path: String,
}

#[derive(Debug, Serialize)]
struct NetworkInterface {
    iface_id: String,
    guest_mac: String,
    host_dev_name: String,
}

#[derive(Debug, Serialize)]
struct InstanceActionInfo {
    action_type: String,
}

#[derive(Debug, Deserialize)]
struct FirecrackerError {
    fault_message: Option<String>,
}

pub struct VmManager {
    config: AppConfig,
    vms: Arc<RwLock<HashMap<String, VmInfo>>>,
    processes: Arc<RwLock<HashMap<String, Child>>>,
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
            processes: Arc::new(RwLock::new(HashMap::new())),
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
        let socket_path = PathBuf::from(&self.config.firecracker.sockets_dir)
            .join(format!("{}.sock", vm_id));
        let vsock_path = PathBuf::from(&self.config.firecracker.sockets_dir)
            .join(format!("{}.vsock", vm_id));
        let volume_path = PathBuf::from(&self.config.firecracker.volumes_dir)
            .join(format!("{}.ext4", task_id));
        let log_path =
            PathBuf::from(&self.config.firecracker.logs_dir).join(format!("{}.log", vm_id));

        // Ensure directories exist
        tokio::fs::create_dir_all(&self.config.firecracker.sockets_dir)
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to create sockets dir: {}", e)))?;
        tokio::fs::create_dir_all(&self.config.firecracker.volumes_dir)
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to create volumes dir: {}", e)))?;
        tokio::fs::create_dir_all(&self.config.firecracker.logs_dir)
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to create logs dir: {}", e)))?;

        // Create empty log file (Firecracker requires it to exist before starting)
        tokio::fs::write(&log_path, "")
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to create log file: {}", e)))?;

        // Create TAP device
        self.create_tap(&tap_name).await?;

        // Create sparse volume file
        let storage_gb = task_config
            .map(|c| c.storage_gb)
            .unwrap_or(self.config.vm.default_storage_gb);
        self.create_sparse_volume(&volume_path, storage_gb).await?;

        // Copy rootfs for this VM (copy-on-write would be better, but this works)
        let vm_rootfs_path = PathBuf::from(&self.config.firecracker.volumes_dir)
            .join(format!("{}-rootfs.ext4", task_id));
        tokio::fs::copy(&self.config.firecracker.rootfs_path, &vm_rootfs_path)
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to copy rootfs: {}", e)))?;

        // Start Firecracker process
        let child = Command::new(&self.config.firecracker.bin_path)
            .arg("--api-sock")
            .arg(&socket_path)
            .arg("--log-path")
            .arg(&log_path)
            .arg("--level")
            .arg("Debug")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| ApiError::VmError(format!("Failed to start Firecracker: {}", e)))?;

        let pid = child.id();

        // Report: waiting for Firecracker API socket
        report_progress(BootStage::WaitingForSocket);

        // Wait for socket to be ready
        self.wait_for_socket(&socket_path).await?;

        // Report: configuring VM via Firecracker API
        report_progress(BootStage::ConfiguringVm);

        // Configure the VM via Firecracker API
        let vcpu_count = task_config
            .map(|c| c.vcpu_count)
            .unwrap_or(self.config.vm.default_vcpu_count);
        let mem_size_mib = task_config
            .map(|c| c.max_memory_mb)
            .unwrap_or(self.config.vm.default_memory_mb);

        self.configure_vm(
            &socket_path,
            &vm_rootfs_path,
            &volume_path,
            &vsock_path,
            &tap_name,
            &mac_address,
            &ip_address,
            &gateway,
            ssh_public_key,
            cid,
            vcpu_count,
            mem_size_mib,
        )
        .await?;

        // Report: VM is now booting
        report_progress(BootStage::BootingVm);

        let vm_info = VmInfo {
            vm_id: vm_id.clone(),
            task_id,
            cid,
            socket_path,
            volume_path,
            log_path,
            pid,
            tap_name,
            ip_address,
            gateway,
        };

        // Store VM info
        self.vms.write().await.insert(vm_id.clone(), vm_info.clone());
        self.processes.write().await.insert(vm_id, child);

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
            "Timeout waiting for Firecracker socket".to_string(),
        ))
    }

    #[allow(clippy::too_many_arguments)]
    async fn configure_vm(
        &self,
        socket_path: &PathBuf,
        rootfs_path: &PathBuf,
        volume_path: &PathBuf,
        vsock_path: &PathBuf,
        tap_name: &str,
        mac_address: &str,
        ip_address: &str,
        gateway: &str,
        ssh_public_key: Option<&str>,
        cid: u32,
        vcpu_count: u32,
        mem_size_mib: u32,
    ) -> ApiResult<()> {
        // Build boot args with network config
        // Replace spaces in SSH key with + for kernel command line
        let ssh_key_arg = ssh_public_key
            .map(|k| format!(" lia.ssh_key={}", k.replace(' ', "+")))
            .unwrap_or_default();

        let boot_args = format!(
            "console=ttyS0 reboot=k panic=1 pci=off init=/sbin/init lia.ip={} lia.gateway={}{}",
            ip_address, gateway, ssh_key_arg
        );

        // Set boot source
        self.fc_put(
            socket_path,
            "/boot-source",
            &BootSource {
                kernel_image_path: self.config.firecracker.kernel_path.clone(),
                boot_args,
            },
        )
        .await?;

        // Set machine config
        self.fc_put(
            socket_path,
            "/machine-config",
            &MachineConfig {
                vcpu_count,
                mem_size_mib,
            },
        )
        .await?;

        // Add root drive
        self.fc_put(
            socket_path,
            "/drives/rootfs",
            &Drive {
                drive_id: "rootfs".to_string(),
                path_on_host: rootfs_path.to_string_lossy().to_string(),
                is_root_device: true,
                is_read_only: false,
            },
        )
        .await?;

        // Add data volume
        self.fc_put(
            socket_path,
            "/drives/data",
            &Drive {
                drive_id: "data".to_string(),
                path_on_host: volume_path.to_string_lossy().to_string(),
                is_root_device: false,
                is_read_only: false,
            },
        )
        .await?;

        // Add network interface
        self.fc_put(
            socket_path,
            "/network-interfaces/eth0",
            &NetworkInterface {
                iface_id: "eth0".to_string(),
                guest_mac: mac_address.to_string(),
                host_dev_name: tap_name.to_string(),
            },
        )
        .await?;

        // Add vsock device
        self.fc_put(
            socket_path,
            "/vsock",
            &Vsock {
                guest_cid: cid,
                uds_path: vsock_path.to_string_lossy().to_string(),
            },
        )
        .await?;

        // Start the VM
        self.fc_put(
            socket_path,
            "/actions",
            &InstanceActionInfo {
                action_type: "InstanceStart".to_string(),
            },
        )
        .await?;

        Ok(())
    }

    async fn fc_put<T: Serialize>(
        &self,
        socket_path: &PathBuf,
        endpoint: &str,
        body: &T,
    ) -> ApiResult<()> {
        // Use curl for Unix socket communication (simpler than hyperlocal setup)
        let body_json = serde_json::to_string(body)
            .map_err(|e| ApiError::VmError(format!("JSON serialization error: {}", e)))?;

        let output = Command::new("curl")
            .arg("--unix-socket")
            .arg(socket_path)
            .arg("-X")
            .arg("PUT")
            .arg("-H")
            .arg("Content-Type: application/json")
            .arg("-d")
            .arg(&body_json)
            .arg(format!("http://localhost{}", endpoint))
            .output()
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to call Firecracker API: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(ApiError::VmError(format!(
                "Firecracker API error: {} {}",
                stderr, stdout
            )));
        }

        // Check for Firecracker error in response
        let response = String::from_utf8_lossy(&output.stdout);
        if let Ok(error) = serde_json::from_str::<FirecrackerError>(&response) {
            if let Some(msg) = error.fault_message {
                return Err(ApiError::VmError(format!("Firecracker error: {}", msg)));
            }
        }

        Ok(())
    }

    pub async fn start_vm(&self, vm_id: &str) -> ApiResult<()> {
        let vms = self.vms.read().await;
        let vm_info = vms
            .get(vm_id)
            .ok_or_else(|| ApiError::VmError(format!("VM not found: {}", vm_id)))?;

        self.fc_put(
            &vm_info.socket_path,
            "/actions",
            &InstanceActionInfo {
                action_type: "InstanceStart".to_string(),
            },
        )
        .await
    }

    pub async fn pause_vm(&self, vm_id: &str) -> ApiResult<()> {
        let vms = self.vms.read().await;
        let vm_info = vms
            .get(vm_id)
            .ok_or_else(|| ApiError::VmError(format!("VM not found: {}", vm_id)))?;

        let output = Command::new("curl")
            .arg("--unix-socket")
            .arg(&vm_info.socket_path)
            .arg("-X")
            .arg("PATCH")
            .arg("-H")
            .arg("Content-Type: application/json")
            .arg("-d")
            .arg(r#"{"state": "Paused"}"#)
            .arg("http://localhost/vm")
            .output()
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to pause VM: {}", e)))?;

        if !output.status.success() {
            return Err(ApiError::VmError(format!(
                "Failed to pause VM: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(())
    }

    pub async fn resume_vm(&self, vm_id: &str) -> ApiResult<()> {
        let vms = self.vms.read().await;
        let vm_info = vms
            .get(vm_id)
            .ok_or_else(|| ApiError::VmError(format!("VM not found: {}", vm_id)))?;

        let output = Command::new("curl")
            .arg("--unix-socket")
            .arg(&vm_info.socket_path)
            .arg("-X")
            .arg("PATCH")
            .arg("-H")
            .arg("Content-Type: application/json")
            .arg("-d")
            .arg(r#"{"state": "Resumed"}"#)
            .arg("http://localhost/vm")
            .output()
            .await
            .map_err(|e| ApiError::VmError(format!("Failed to resume VM: {}", e)))?;

        if !output.status.success() {
            return Err(ApiError::VmError(format!(
                "Failed to resume VM: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(())
    }

    pub async fn stop_vm(&self, vm_id: &str) -> ApiResult<()> {
        // Remove from tracking
        let vm_info = self.vms.write().await.remove(vm_id);
        let child = self.processes.write().await.remove(vm_id);

        if let Some(mut child) = child {
            // Send SIGTERM
            let _ = child.kill().await;
        }

        // Cleanup files and TAP device
        if let Some(info) = vm_info {
            // Delete TAP device
            let _ = self.delete_tap(&info.tap_name).await;

            let _ = tokio::fs::remove_file(&info.socket_path).await;
            let _ = tokio::fs::remove_file(&info.volume_path).await;

            // Also remove the copied rootfs
            let rootfs_copy = PathBuf::from(&self.config.firecracker.volumes_dir)
                .join(format!("{}-rootfs.ext4", info.task_id));
            let _ = tokio::fs::remove_file(&rootfs_copy).await;

            // Remove vsock
            let vsock_path = PathBuf::from(&self.config.firecracker.sockets_dir)
                .join(format!("{}.vsock", vm_id));
            let _ = tokio::fs::remove_file(&vsock_path).await;
        }

        Ok(())
    }

    pub async fn get_vm_info(&self, vm_id: &str) -> Option<VmInfo> {
        self.vms.read().await.get(vm_id).cloned()
    }

    pub fn get_vsock_path(&self, vm_id: &str) -> PathBuf {
        PathBuf::from(&self.config.firecracker.sockets_dir).join(format!("{}.vsock", vm_id))
    }
}
