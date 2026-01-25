use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use futures::{Stream, StreamExt};
use serde::Deserialize;

pub struct ApiClient {
    base_url: String,
    client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
pub struct TaskListResponse {
    pub tasks: Vec<TaskResponse>,
    pub total: i64,
    pub page: u32,
    pub per_page: u32,
}

#[derive(Debug, Deserialize)]
pub struct TaskResponse {
    pub id: String,
    pub user_id: String,
    pub status: String,
    pub source: String,
    pub created_at: DateTime<Utc>,
    pub ip_address: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LogsResponse {
    pub task_id: String,
    pub lines: Vec<String>,
    pub total_lines: usize,
}

#[derive(Debug, Clone)]
pub struct SseEvent {
    pub event_type: String,
    pub task_id: Option<String>,
    pub line: Option<String>,
    pub error: Option<String>,
    pub timestamp: Option<i64>,
}

impl ApiClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
        }
    }

    pub async fn list_tasks(&self, status: Option<&str>) -> Result<TaskListResponse> {
        let mut url = format!("{}/api/v1/tasks", self.base_url);

        if let Some(s) = status {
            url.push_str(&format!("?status={}", s));
        }

        let response = self
            .client
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .json::<TaskListResponse>()
            .await?;

        Ok(response)
    }

    pub async fn get_logs(&self, task_id: &str, tail: usize) -> Result<LogsResponse> {
        let url = format!(
            "{}/api/v1/tasks/{}/logs?tail={}",
            self.base_url, task_id, tail
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .json::<LogsResponse>()
            .await?;

        Ok(response)
    }

    pub async fn stream_logs(
        &self,
        task_id: &str,
        tail: usize,
    ) -> Result<impl Stream<Item = Result<SseEvent>>> {
        let url = format!(
            "{}/api/v1/tasks/{}/logs/stream?tail={}",
            self.base_url, task_id, tail
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await?
            .error_for_status()?;

        let stream = response.bytes_stream();

        // Parse SSE events from the stream
        Ok(stream
            .map(|chunk| {
                chunk
                    .map_err(|e| anyhow!("Stream error: {}", e))
                    .and_then(|bytes| parse_sse_chunk(&bytes))
            })
            .filter_map(|result| async move {
                match result {
                    Ok(Some(event)) => Some(Ok(event)),
                    Ok(None) => None,
                    Err(e) => Some(Err(e)),
                }
            }))
    }
}

fn parse_sse_chunk(bytes: &[u8]) -> Result<Option<SseEvent>> {
    let text = String::from_utf8_lossy(bytes);

    // SSE format:
    // event: <type>
    // data: <json>
    //
    // (blank line)

    let mut event_type = String::new();
    let mut data = String::new();

    for line in text.lines() {
        if line.starts_with("event:") {
            event_type = line.trim_start_matches("event:").trim().to_string();
        } else if line.starts_with("data:") {
            data = line.trim_start_matches("data:").trim().to_string();
        }
    }

    if event_type.is_empty() && data.is_empty() {
        return Ok(None);
    }

    // Parse the data as JSON
    let parsed: serde_json::Value = if data.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(&data).unwrap_or(serde_json::Value::Null)
    };

    Ok(Some(SseEvent {
        event_type: if event_type.is_empty() {
            "message".to_string()
        } else {
            event_type
        },
        task_id: parsed
            .get("task_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        line: parsed
            .get("line")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        error: parsed
            .get("error")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        timestamp: parsed.get("timestamp").and_then(|v| v.as_i64()),
    }))
}
