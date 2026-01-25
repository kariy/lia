use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket},
        Path, Query, State, WebSocketUpgrade,
    },
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    Json,
};
use futures::{SinkExt, Stream, StreamExt};
use tokio::io::{AsyncBufReadExt, AsyncSeekExt, BufReader};
use uuid::Uuid;

use crate::db;
use crate::error::{ApiError, ApiResult};
use crate::models::{
    is_valid_repo_format, BootStage, CreateTaskRequest, ListTasksQuery, LogsQuery, LogsResponse,
    StreamLogsQuery, TaskListResponse, TaskResponse, TaskStatus, WsMessage,
};
use crate::vsock::VsockRelay;
use crate::AppState;

pub async fn health_check() -> &'static str {
    "OK"
}

pub async fn create_task(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateTaskRequest>,
) -> ApiResult<Json<TaskResponse>> {
    // Validate request
    if req.prompt.is_empty() {
        return Err(ApiError::BadRequest("Prompt cannot be empty".to_string()));
    }

    // Validate repositories
    if req.repositories.is_empty() {
        return Err(ApiError::BadRequest(
            "At least one repository is required".to_string(),
        ));
    }

    for repo in &req.repositories {
        if !is_valid_repo_format(repo) {
            return Err(ApiError::BadRequest(format!(
                "Invalid repository format: '{}'. Expected 'owner/repo'",
                repo
            )));
        }
    }

    // Use a default user_id if not provided
    let user_id = req
        .user_id
        .clone()
        .unwrap_or_else(|| "anonymous".to_string());

    // Create task in database
    let task = db::create_task(
        &state.db,
        &user_id,
        req.source,
        &req.repositories,
        req.config.clone(),
    )
    .await?;

    let task_id = task.id;
    let vm_id = format!("vm-{}", task_id);

    // Create guild association if guild_id is provided
    if let Some(guild_id) = &req.guild_id {
        db::create_guild_task(&state.db, task_id, guild_id).await?;
    }

    // Update status to starting
    db::update_task_status(&state.db, task_id, TaskStatus::Starting, Some(&vm_id)).await?;

    // Get or create WebSocket channel for progress updates
    let channel = state.ws_registry.get_or_create(task_id).await;

    // Helper to send progress updates
    async fn send_progress(channel: &crate::ws::TaskChannel, stage: BootStage) {
        let msg = WsMessage::Progress {
            stage,
            message: stage.message().to_string(),
        };
        channel.send(msg).await;
    }

    // Spawn VM creation in background
    let state_clone = state.clone();
    let prompt = req.prompt.clone();
    let files = req.files.clone();
    let task_config = req.config.clone();
    let ssh_public_key = req.ssh_public_key.clone();
    let channel_clone = channel.clone();

    tokio::spawn(async move {
        // Send initial progress
        send_progress(&channel_clone, BootStage::CreatingVm).await;

        // Create a channel to receive progress updates from the sync callback
        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel::<BootStage>();

        // Create progress callback that sends to channel
        let progress_callback: crate::qemu::ProgressCallback = Box::new(move |stage| {
            let _ = progress_tx.send(stage);
        });

        // Spawn a task to forward progress updates to WebSocket
        let channel_for_progress = channel_clone.clone();
        let _progress_forwarder = tokio::spawn(async move {
            while let Some(stage) = progress_rx.recv().await {
                send_progress(&channel_for_progress, stage).await;
            }
        });

        match state_clone
            .vm_manager
            .create_vm_with_progress(
                task_id,
                task_config.as_ref(),
                ssh_public_key.as_deref(),
                Some(progress_callback),
            )
            .await
        {
            Ok(vm_info) => {
                tracing::info!("VM created: {:?}", vm_info);

                // Update task with VM ID and IP address
                if let Err(e) = db::update_task_status(
                    &state_clone.db,
                    task_id,
                    TaskStatus::Running,
                    Some(&vm_info.vm_id),
                )
                .await
                {
                    tracing::error!("Failed to update task status: {}", e);
                    return;
                }

                // Store the IP address
                if let Err(e) =
                    db::update_task_ip_address(&state_clone.db, task_id, &vm_info.ip_address).await
                {
                    tracing::error!("Failed to update task IP address: {}", e);
                }

                // Progress: connecting to agent
                send_progress(&channel_clone, BootStage::ConnectingAgent).await;

                // Start vsock relay using the VM's CID for direct AF_VSOCK connection
                let relay = VsockRelay::new(task_id, vm_info.cid, state_clone.ws_registry.clone());

                match relay
                    .start(state_clone.config.claude.api_key.clone(), prompt, files)
                    .await
                {
                    Ok(input_tx) => {
                        tracing::info!("vsock relay started for task {}", task_id);
                        // Store the input sender in the channel for forwarding WebSocket input to VM
                        channel_clone.set_input_sender(input_tx).await;
                        // Progress: initializing Claude
                        send_progress(&channel_clone, BootStage::InitializingClaude).await;
                        // Progress: ready (after a brief delay to allow Claude to start)
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                        send_progress(&channel_clone, BootStage::Ready).await;
                    }
                    Err(e) => {
                        tracing::error!("Failed to start vsock relay: {}", e);
                        // Send error message
                        channel_clone
                            .send(WsMessage::Error {
                                message: format!("Failed to connect to agent: {}", e),
                            })
                            .await;
                        let _ = db::complete_task(
                            &state_clone.db,
                            task_id,
                            1,
                            Some(&format!("vsock relay failed: {}", e)),
                        )
                        .await;
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to create VM: {}", e);
                // Send error message
                channel_clone
                    .send(WsMessage::Error {
                        message: format!("Failed to start VM: {}", e),
                    })
                    .await;
                let _ = db::complete_task(
                    &state_clone.db,
                    task_id,
                    1,
                    Some(&format!("VM creation failed: {}", e)),
                )
                .await;
            }
        }
    });

    // Return task response
    let task = db::get_task(&state.db, task_id).await?;
    let guild_id = db::get_guild_id_for_task(&state.db, task_id).await?;
    Ok(Json(TaskResponse::from_task(
        task,
        guild_id,
        &state.config.server.web_url,
    )))
}

pub async fn get_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TaskResponse>> {
    let task = db::get_task(&state.db, id).await?;
    let guild_id = db::get_guild_id_for_task(&state.db, id).await?;
    Ok(Json(TaskResponse::from_task(
        task,
        guild_id,
        &state.config.server.web_url,
    )))
}

pub async fn list_tasks(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListTasksQuery>,
) -> ApiResult<Json<TaskListResponse>> {
    let (tasks, total) = db::list_tasks(
        &state.db,
        query.user_id.as_deref(),
        query.status,
        query.page,
        query.per_page,
    )
    .await?;

    let mut task_responses = Vec::with_capacity(tasks.len());
    for task in tasks {
        let guild_id = db::get_guild_id_for_task(&state.db, task.id).await?;
        task_responses.push(TaskResponse::from_task(
            task,
            guild_id,
            &state.config.server.web_url,
        ));
    }

    Ok(Json(TaskListResponse {
        tasks: task_responses,
        total,
        page: query.page,
        per_page: query.per_page,
    }))
}

pub async fn delete_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<impl IntoResponse> {
    let task = db::get_task(&state.db, id).await?;

    // Stop VM if running
    if let Some(vm_id) = &task.vm_id {
        if let Err(e) = state.vm_manager.stop_vm(vm_id).await {
            tracing::warn!("Failed to stop VM: {}", e);
        }
    }

    // Update status to terminated
    db::update_task_status(&state.db, id, TaskStatus::Terminated, None).await?;

    // Remove WebSocket channel
    state.ws_registry.remove(id).await;

    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn resume_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TaskResponse>> {
    let task = db::get_task(&state.db, id).await?;

    // Check if task is in suspended state
    if task.status != TaskStatus::Suspended {
        return Err(ApiError::InvalidState(format!(
            "Task is not suspended, current status: {}",
            task.status
        )));
    }

    // Resume VM
    if let Some(vm_id) = &task.vm_id {
        state.vm_manager.resume_vm(vm_id).await?;
    } else {
        return Err(ApiError::InvalidState(
            "Task has no associated VM".to_string(),
        ));
    }

    // Update status to running
    let task = db::update_task_status(&state.db, id, TaskStatus::Running, None).await?;
    let guild_id = db::get_guild_id_for_task(&state.db, id).await?;

    Ok(Json(TaskResponse::from_task(
        task,
        guild_id,
        &state.config.server.web_url,
    )))
}

pub async fn get_task_output(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<WsMessage>>> {
    // Verify task exists
    let _ = db::get_task(&state.db, id).await?;

    // Get buffered output
    if let Some(channel) = state.ws_registry.get(id).await {
        Ok(Json(channel.get_buffered_output().await))
    } else {
        Ok(Json(vec![]))
    }
}

pub async fn ws_stream(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(state, id, socket))
}

async fn handle_ws(state: Arc<AppState>, task_id: Uuid, socket: WebSocket) {
    // Verify task exists
    if db::get_task(&state.db, task_id).await.is_err() {
        tracing::warn!("WebSocket connection for non-existent task: {}", task_id);
        return;
    }

    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Get or create channel
    let channel = state.ws_registry.get_or_create(task_id).await;

    // Send buffered output first
    for msg in channel.get_buffered_output().await {
        if let Ok(json) = serde_json::to_string(&msg) {
            if ws_sender.send(Message::Text(json)).await.is_err() {
                return;
            }
        }
    }

    // Subscribe to new messages
    let mut rx = channel.subscribe();

    // Spawn task to forward messages from channel to WebSocket
    let sender_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                msg = rx.recv() => {
                    match msg {
                        Ok(msg) => {
                            if let Ok(json) = serde_json::to_string(&msg) {
                                if ws_sender.send(Message::Text(json)).await.is_err() {
                                    break;
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            // Skip lagged messages
                            continue;
                        }
                        Err(_) => break,
                    }
                }
            }
        }
    });

    // Handle incoming messages from WebSocket
    while let Some(result) = ws_receiver.next().await {
        match result {
            Ok(Message::Text(text)) => {
                if let Ok(msg) = serde_json::from_str::<WsMessage>(&text) {
                    match msg {
                        WsMessage::Input { data } => {
                            tracing::debug!("Received input for task {}: {}", task_id, data);
                            // Forward input to the VM via vsock
                            if !channel.send_input(data).await {
                                tracing::warn!("Failed to forward input to VM for task {}", task_id);
                            }
                        }
                        WsMessage::Ping => {
                            channel.send(WsMessage::Pong).await;
                        }
                        _ => {}
                    }
                }
            }
            Ok(Message::Close(_)) => break,
            Err(_) => break,
            _ => {}
        }
    }

    sender_task.abort();
}

/// Get VM logs (snapshot) - last N lines
pub async fn get_vm_logs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Query(params): Query<LogsQuery>,
) -> ApiResult<Json<LogsResponse>> {
    // Verify task exists
    let _ = db::get_task(&state.db, id).await?;

    // Construct log path: {logs_dir}/vm-{task_id}.log
    let log_path = PathBuf::from(&state.config.qemu.logs_dir).join(format!("vm-{}.log", id));

    // Read last N lines
    let (lines, total_lines) = read_last_n_lines(&log_path, params.tail).await?;

    Ok(Json(LogsResponse {
        task_id: id,
        lines,
        total_lines,
    }))
}

/// Stream VM logs via SSE (like tail -f)
pub async fn stream_vm_logs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
    Query(params): Query<StreamLogsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    // Verify task exists
    let _ = db::get_task(&state.db, id).await?;

    // Construct log path
    let log_path = PathBuf::from(&state.config.qemu.logs_dir).join(format!("vm-{}.log", id));

    let stream = async_stream::stream! {
        // Send init event
        let init_data = serde_json::json!({
            "task_id": id.to_string(),
            "tail": params.tail
        });
        yield Ok(Event::default().event("init").data(init_data.to_string()));

        // Check if file exists
        if !log_path.exists() {
            let error_data = serde_json::json!({
                "error": "Log file not found"
            });
            yield Ok(Event::default().event("error").data(error_data.to_string()));
            return;
        }

        // Read and send initial lines
        match read_last_n_lines(&log_path, params.tail).await {
            Ok((lines, _)) => {
                for line in lines {
                    let log_data = serde_json::json!({
                        "line": format!("{}\n", line)
                    });
                    yield Ok(Event::default().event("log").data(log_data.to_string()));
                }
            }
            Err(e) => {
                let error_data = serde_json::json!({
                    "error": format!("Failed to read log file: {}", e)
                });
                yield Ok(Event::default().event("error").data(error_data.to_string()));
                return;
            }
        }

        // Now tail the file for new content
        let file = match tokio::fs::File::open(&log_path).await {
            Ok(f) => f,
            Err(e) => {
                let error_data = serde_json::json!({
                    "error": format!("Failed to open log file: {}", e)
                });
                yield Ok(Event::default().event("error").data(error_data.to_string()));
                return;
            }
        };

        let mut reader = BufReader::new(file);

        // Seek to end of file
        if let Err(e) = reader.seek(std::io::SeekFrom::End(0)).await {
            let error_data = serde_json::json!({
                "error": format!("Failed to seek to end: {}", e)
            });
            yield Ok(Event::default().event("error").data(error_data.to_string()));
            return;
        }

        let mut last_size = match tokio::fs::metadata(&log_path).await {
            Ok(m) => m.len(),
            Err(_) => 0,
        };

        let mut heartbeat_interval = tokio::time::interval(std::time::Duration::from_secs(30));
        let mut poll_interval = tokio::time::interval(std::time::Duration::from_millis(500));

        loop {
            tokio::select! {
                _ = poll_interval.tick() => {
                    // Check for new content
                    let current_size = match tokio::fs::metadata(&log_path).await {
                        Ok(m) => m.len(),
                        Err(_) => continue,
                    };

                    if current_size < last_size {
                        // File was truncated, reopen and seek to beginning
                        let new_file = match tokio::fs::File::open(&log_path).await {
                            Ok(f) => f,
                            Err(_) => continue,
                        };
                        reader = BufReader::new(new_file);
                        last_size = 0;
                    }

                    // Read new lines
                    let mut line = String::new();
                    loop {
                        line.clear();
                        match reader.read_line(&mut line).await {
                            Ok(0) => break, // No more data
                            Ok(_) => {
                                let log_data = serde_json::json!({
                                    "line": line.clone()
                                });
                                yield Ok(Event::default().event("log").data(log_data.to_string()));
                            }
                            Err(_) => break,
                        }
                    }

                    last_size = current_size;
                }
                _ = heartbeat_interval.tick() => {
                    let heartbeat_data = serde_json::json!({
                        "timestamp": chrono::Utc::now().timestamp()
                    });
                    yield Ok(Event::default().event("heartbeat").data(heartbeat_data.to_string()));
                }
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// Helper function to read the last N lines from a file
async fn read_last_n_lines(path: &PathBuf, n: usize) -> ApiResult<(Vec<String>, usize)> {
    if !path.exists() {
        return Ok((vec![], 0));
    }

    let content = tokio::fs::read_to_string(path).await.map_err(|e| {
        ApiError::NotFound(format!("Failed to read log file: {}", e))
    })?;

    let all_lines: Vec<&str> = content.lines().collect();
    let total_lines = all_lines.len();

    let lines: Vec<String> = if all_lines.len() > n {
        all_lines[all_lines.len() - n..]
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        all_lines.iter().map(|s| s.to_string()).collect()
    };

    Ok((lines, total_lines))
}
