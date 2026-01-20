use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

use crate::models::WsMessage;

const CHANNEL_CAPACITY: usize = 1024;

#[derive(Debug)]
pub struct TaskChannel {
    pub sender: broadcast::Sender<WsMessage>,
    pub output_buffer: Arc<RwLock<Vec<WsMessage>>>,
}

impl TaskChannel {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self {
            sender,
            output_buffer: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<WsMessage> {
        self.sender.subscribe()
    }

    pub async fn send(&self, msg: WsMessage) {
        // Buffer output messages
        if matches!(msg, WsMessage::Output { .. }) {
            self.output_buffer.write().await.push(msg.clone());
        }
        // Ignore send errors (no subscribers)
        let _ = self.sender.send(msg);
    }

    pub async fn get_buffered_output(&self) -> Vec<WsMessage> {
        self.output_buffer.read().await.clone()
    }
}

#[derive(Debug, Default)]
pub struct WsRegistry {
    channels: RwLock<HashMap<Uuid, Arc<TaskChannel>>>,
}

impl WsRegistry {
    pub fn new() -> Self {
        Self {
            channels: RwLock::new(HashMap::new()),
        }
    }

    pub async fn get_or_create(&self, task_id: Uuid) -> Arc<TaskChannel> {
        let mut channels = self.channels.write().await;
        channels
            .entry(task_id)
            .or_insert_with(|| Arc::new(TaskChannel::new()))
            .clone()
    }

    pub async fn get(&self, task_id: Uuid) -> Option<Arc<TaskChannel>> {
        self.channels.read().await.get(&task_id).cloned()
    }

    pub async fn remove(&self, task_id: Uuid) {
        self.channels.write().await.remove(&task_id);
    }

    pub async fn broadcast(&self, task_id: Uuid, msg: WsMessage) {
        if let Some(channel) = self.get(task_id).await {
            channel.send(msg).await;
        }
    }
}
