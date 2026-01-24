use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub qemu: QemuConfig,
    pub vm: VmConfig,
    pub network: NetworkConfig,
    pub claude: ClaudeConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_web_url")]
    pub web_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QemuConfig {
    #[serde(default = "default_qemu_bin")]
    pub bin_path: String,
    #[serde(default = "default_kernel_path")]
    pub kernel_path: String,
    #[serde(default = "default_rootfs_path")]
    pub rootfs_path: String,
    #[serde(default = "default_volumes_dir")]
    pub volumes_dir: String,
    #[serde(default = "default_sockets_dir")]
    pub sockets_dir: String,
    #[serde(default = "default_logs_dir")]
    pub logs_dir: String,
    #[serde(default = "default_pids_dir")]
    pub pids_dir: String,
    #[serde(default = "default_machine_type")]
    pub machine_type: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VmConfig {
    #[serde(default = "default_vcpu_count")]
    pub default_vcpu_count: u32,
    #[serde(default = "default_memory_mb")]
    pub default_memory_mb: u32,
    #[serde(default = "default_storage_gb")]
    pub default_storage_gb: u32,
    #[serde(default = "default_idle_timeout_minutes")]
    pub idle_timeout_minutes: u32,
    #[serde(default = "default_vsock_cid_start")]
    pub vsock_cid_start: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NetworkConfig {
    #[serde(default = "default_bridge_name")]
    pub bridge_name: String,
    #[serde(default = "default_bridge_ip")]
    pub bridge_ip: String,
    #[serde(default = "default_subnet")]
    pub subnet: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeConfig {
    pub api_key: String,
}

// Default value functions
fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    8811
}

fn default_web_url() -> String {
    "http://localhost:5173".to_string()
}

fn default_max_connections() -> u32 {
    10
}

fn default_qemu_bin() -> String {
    "/usr/bin/qemu-system-x86_64".to_string()
}

fn default_kernel_path() -> String {
    "/var/lib/lia/kernel/vmlinuz".to_string()
}

fn default_pids_dir() -> String {
    "/var/run/lia".to_string()
}

fn default_machine_type() -> String {
    "q35".to_string()
}

fn default_rootfs_path() -> String {
    "/var/lib/lia/rootfs/rootfs.ext4".to_string()
}

fn default_volumes_dir() -> String {
    "/var/lib/lia/volumes".to_string()
}

fn default_sockets_dir() -> String {
    "/var/lib/lia/sockets".to_string()
}

fn default_logs_dir() -> String {
    "/var/lib/lia/logs".to_string()
}

fn default_vcpu_count() -> u32 {
    2
}

fn default_memory_mb() -> u32 {
    2048
}

fn default_storage_gb() -> u32 {
    50
}

fn default_idle_timeout_minutes() -> u32 {
    30
}

fn default_vsock_cid_start() -> u32 {
    100
}

fn default_bridge_name() -> String {
    "lia-br0".to_string()
}

fn default_bridge_ip() -> String {
    "172.16.0.1".to_string()
}

fn default_subnet() -> String {
    "172.16.0.0/24".to_string()
}

impl AppConfig {
    pub fn load() -> anyhow::Result<Self> {
        let config = config::Config::builder()
            .add_source(config::File::with_name("config/default").required(true))
            .add_source(config::File::with_name("config/local").required(false))
            .build()?;

        Ok(config.try_deserialize()?)
    }
}
