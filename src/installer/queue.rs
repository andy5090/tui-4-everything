use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum QueueState {
    Idle,
    Queued,
    Installing,
    Success,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueueItem {
    pub tool_id: String,
    pub channel: String,
    pub state: QueueState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub attempts: u32,
}

impl QueueItem {
    pub fn new(tool_id: impl Into<String>, channel: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            tool_id: tool_id.into(),
            channel: channel.into(),
            state: QueueState::Queued,
            created_at: now,
            updated_at: now,
            attempts: 0,
        }
    }

    pub fn transition(&mut self, next: QueueState) {
        self.state = next;
        self.updated_at = Utc::now();
    }

    pub fn mark_attempt(&mut self) {
        self.attempts += 1;
        self.updated_at = Utc::now();
    }
}
