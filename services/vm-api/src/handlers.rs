use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket},
        Path, Query, State, WebSocketUpgrade,
    },
    response::IntoResponse,
    Json,
};
use futures::{SinkExt, StreamExt};
use uuid::Uuid;

use crate::db;
use crate::error::{ApiError, ApiResult};
use crate::models::{
    is_valid_repo_format, CreateTaskRequest, ListTasksQuery, TaskListResponse, TaskResponse,
    TaskStatus, WsMessage,
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
    let user_id = req.user_id.clone().unwrap_or_else(|| "anonymous".to_string());

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

    // Spawn VM creation in background
    let state_clone = state.clone();
    let prompt = req.prompt.clone();
    let files = req.files.clone();
    let task_config = req.config.clone();
    let ssh_public_key = req.ssh_public_key.clone();

    tokio::spawn(async move {
        match state_clone
            .vm_manager
            .create_vm(task_id, task_config.as_ref(), ssh_public_key.as_deref())
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

                // Start vsock relay
                let vsock_path = state_clone.vm_manager.get_vsock_path(&vm_info.vm_id);
                let relay =
                    VsockRelay::new(task_id, vsock_path, state_clone.ws_registry.clone());

                match relay
                    .start(state_clone.config.claude.api_key.clone(), prompt, files)
                    .await
                {
                    Ok(_input_tx) => {
                        tracing::info!("vsock relay started for task {}", task_id);
                        // Store input_tx for later use with WebSocket input
                    }
                    Err(e) => {
                        tracing::error!("Failed to start vsock relay: {}", e);
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
    Ok(Json(TaskResponse::from_task(task, guild_id, &state.config.server.web_url)))
}

pub async fn get_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TaskResponse>> {
    let task = db::get_task(&state.db, id).await?;
    let guild_id = db::get_guild_id_for_task(&state.db, id).await?;
    Ok(Json(TaskResponse::from_task(task, guild_id, &state.config.server.web_url)))
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
        task_responses.push(TaskResponse::from_task(task, guild_id, &state.config.server.web_url));
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
        return Err(ApiError::InvalidState("Task has no associated VM".to_string()));
    }

    // Update status to running
    let task = db::update_task_status(&state.db, id, TaskStatus::Running, None).await?;
    let guild_id = db::get_guild_id_for_task(&state.db, id).await?;

    Ok(Json(TaskResponse::from_task(task, guild_id, &state.config.server.web_url)))
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
                            // Forward input to vsock relay
                            // This would need the input_tx stored somewhere
                            tracing::debug!("Received input for task {}: {}", task_id, data);
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
