use chrono::{DateTime, Utc};
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "VARCHAR", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Pending,
    Starting,
    Running,
    Suspended,
    Terminated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "VARCHAR", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum TaskSource {
    Discord,
    Web,
}

impl Default for TaskSource {
    fn default() -> Self {
        TaskSource::Web
    }
}

impl std::fmt::Display for TaskSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskSource::Discord => write!(f, "discord"),
            TaskSource::Web => write!(f, "web"),
        }
    }
}

lazy_static! {
    static ref REPO_REGEX: Regex = Regex::new(r"^[a-zA-Z0-9._-]+/[a-zA-Z0-9._-]+$").unwrap();
}

pub fn is_valid_repo_format(repo: &str) -> bool {
    REPO_REGEX.is_match(repo)
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskStatus::Pending => write!(f, "pending"),
            TaskStatus::Starting => write!(f, "starting"),
            TaskStatus::Running => write!(f, "running"),
            TaskStatus::Suspended => write!(f, "suspended"),
            TaskStatus::Terminated => write!(f, "terminated"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskConfig {
    #[serde(default = "default_timeout")]
    pub timeout_minutes: u32,
    #[serde(default = "default_memory")]
    pub max_memory_mb: u32,
    #[serde(default = "default_vcpu")]
    pub vcpu_count: u32,
    #[serde(default = "default_storage")]
    pub storage_gb: u32,
}

fn default_timeout() -> u32 {
    30
}
fn default_memory() -> u32 {
    2048
}
fn default_vcpu() -> u32 {
    2
}
fn default_storage() -> u32 {
    50
}

impl Default for TaskConfig {
    fn default() -> Self {
        Self {
            timeout_minutes: default_timeout(),
            max_memory_mb: default_memory(),
            vcpu_count: default_vcpu(),
            storage_gb: default_storage(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Task {
    pub id: Uuid,
    pub user_id: String,
    pub status: TaskStatus,
    pub source: TaskSource,
    pub repositories: Vec<String>,
    pub vm_id: Option<String>,
    pub config: Option<sqlx::types::Json<TaskConfig>>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    pub error_message: Option<String>,
    pub ip_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct GuildTask {
    pub task_id: Uuid,
    pub guild_id: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskFile {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTaskRequest {
    pub prompt: String,
    /// GitHub repositories in "owner/repo" format
    pub repositories: Vec<String>,
    /// Task source: discord or web
    pub source: TaskSource,
    pub user_id: Option<String>,
    pub guild_id: Option<String>,
    pub config: Option<TaskConfig>,
    pub files: Option<Vec<TaskFile>>,
    /// SSH public key for accessing the VM (e.g., "ssh-rsa AAAA... user@host")
    pub ssh_public_key: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskResponse {
    pub id: Uuid,
    pub user_id: String,
    pub guild_id: Option<String>,
    pub status: TaskStatus,
    pub source: TaskSource,
    pub repositories: Vec<String>,
    pub vm_id: Option<String>,
    pub config: Option<TaskConfig>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub exit_code: Option<i32>,
    pub error_message: Option<String>,
    pub web_url: String,
    /// SSH connection info (e.g., "ssh root@172.16.0.100")
    pub ssh_command: Option<String>,
    /// IP address of the VM
    pub ip_address: Option<String>,
}

impl TaskResponse {
    pub fn from_task(task: Task, guild_id: Option<String>, web_base_url: &str) -> Self {
        let ssh_command = task
            .ip_address
            .as_ref()
            .map(|ip| format!("ssh root@{}", ip));

        Self {
            id: task.id,
            user_id: task.user_id,
            guild_id,
            status: task.status,
            source: task.source,
            repositories: task.repositories,
            vm_id: task.vm_id,
            config: task.config.map(|c| c.0),
            created_at: task.created_at,
            started_at: task.started_at,
            completed_at: task.completed_at,
            exit_code: task.exit_code,
            error_message: task.error_message,
            web_url: format!("{}/tasks/{}", web_base_url, task.id),
            ssh_command,
            ip_address: task.ip_address,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskListResponse {
    pub tasks: Vec<TaskResponse>,
    pub total: i64,
    pub page: u32,
    pub per_page: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ListTasksQuery {
    pub user_id: Option<String>,
    pub status: Option<TaskStatus>,
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_per_page")]
    pub per_page: u32,
}

fn default_page() -> u32 {
    1
}
fn default_per_page() -> u32 {
    20
}

// Boot progress stages for VM startup
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootStage {
    CreatingVm,
    WaitingForSocket,
    ConfiguringVm,
    BootingVm,
    ConnectingAgent,
    InitializingClaude,
    Ready,
}

impl BootStage {
    /// Human-readable message for UI display
    pub fn message(&self) -> &'static str {
        match self {
            BootStage::CreatingVm => "Starting VM...",
            BootStage::WaitingForSocket => "Starting VM...",
            BootStage::ConfiguringVm => "Configuring VM...",
            BootStage::BootingVm => "Booting...",
            BootStage::ConnectingAgent => "Connecting...",
            BootStage::InitializingClaude => "Initializing Claude...",
            BootStage::Ready => "Ready",
        }
    }
}

// WebSocket message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum WsMessage {
    Output { data: String, timestamp: i64 },
    Input { data: String },
    Status { status: TaskStatus, exit_code: Option<i32> },
    Progress { stage: BootStage, message: String },
    Error { message: String },
    Ping,
    Pong,
}

// vsock message types for sidecar communication
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum VsockMessage {
    Init {
        api_key: String,
        prompt: String,
        files: Option<Vec<TaskFile>>,
    },
    Output {
        data: String,
    },
    Input {
        data: String,
    },
    Exit {
        code: i32,
    },
    /// Error message from the sidecar (e.g., Claude Code failed to start)
    Error {
        message: String,
    },
    Heartbeat,
}

// Query params for log endpoints
#[derive(Debug, Clone, Deserialize)]
pub struct LogsQuery {
    #[serde(default = "default_logs_tail")]
    pub tail: usize,
}

fn default_logs_tail() -> usize {
    100
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamLogsQuery {
    #[serde(default = "default_stream_tail")]
    pub tail: usize,
}

fn default_stream_tail() -> usize {
    20
}

// Response for GET /logs
#[derive(Debug, Clone, Serialize)]
pub struct LogsResponse {
    pub task_id: Uuid,
    pub lines: Vec<String>,
    pub total_lines: usize,
}
