use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub firecracker: FirecrackerConfig,
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
pub struct FirecrackerConfig {
    #[serde(default = "default_firecracker_bin")]
    pub bin_path: String,
    #[serde(default = "default_jailer_bin")]
    pub jailer_bin_path: String,
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

fn default_firecracker_bin() -> String {
    "/usr/local/bin/firecracker".to_string()
}

fn default_jailer_bin() -> String {
    "/usr/local/bin/jailer".to_string()
}

fn default_kernel_path() -> String {
    "/var/lib/lia/kernel/vmlinux".to_string()
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
            .add_source(config::File::with_name("config/default").required(false))
            .add_source(config::File::with_name("config/local").required(false))
            .add_source(
                config::Environment::with_prefix("LIA")
                    .separator("__")
                    .try_parsing(true),
            )
            .set_default("server.host", default_host())?
            .set_default("server.port", default_port() as i64)?
            .set_default("server.web_url", default_web_url())?
            .set_default("database.max_connections", default_max_connections() as i64)?
            .set_default("firecracker.bin_path", default_firecracker_bin())?
            .set_default("firecracker.jailer_bin_path", default_jailer_bin())?
            .set_default("firecracker.kernel_path", default_kernel_path())?
            .set_default("firecracker.rootfs_path", default_rootfs_path())?
            .set_default("firecracker.volumes_dir", default_volumes_dir())?
            .set_default("firecracker.sockets_dir", default_sockets_dir())?
            .set_default("firecracker.logs_dir", default_logs_dir())?
            .set_default("vm.default_vcpu_count", default_vcpu_count() as i64)?
            .set_default("vm.default_memory_mb", default_memory_mb() as i64)?
            .set_default("vm.default_storage_gb", default_storage_gb() as i64)?
            .set_default(
                "vm.idle_timeout_minutes",
                default_idle_timeout_minutes() as i64,
            )?
            .set_default("vm.vsock_cid_start", default_vsock_cid_start() as i64)?
            .set_default("network.bridge_name", default_bridge_name())?
            .set_default("network.bridge_ip", default_bridge_ip())?
            .set_default("network.subnet", default_subnet())?
            .build()?;

        Ok(config.try_deserialize()?)
    }
}
