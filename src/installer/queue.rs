use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

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

#[derive(Debug, Error, PartialEq, Eq)]
pub enum QueueTransitionError {
    #[error("invalid transition: {from:?} -> {to:?}")]
    Invalid { from: QueueState, to: QueueState },
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

    pub fn transition(&mut self, next: QueueState) -> Result<(), QueueTransitionError> {
        if !is_valid_transition(&self.state, &next) {
            return Err(QueueTransitionError::Invalid {
                from: self.state.clone(),
                to: next,
            });
        }
        self.state = next;
        self.updated_at = Utc::now();
        Ok(())
    }

    pub fn mark_attempt(&mut self) {
        self.attempts += 1;
        self.updated_at = Utc::now();
    }
}

fn is_valid_transition(from: &QueueState, to: &QueueState) -> bool {
    match (from, to) {
        (QueueState::Idle, QueueState::Queued) => true,
        (QueueState::Queued, QueueState::Installing) => true,
        (QueueState::Installing, QueueState::Success) => true,
        (QueueState::Installing, QueueState::Failed) => true,
        (QueueState::Failed, QueueState::Queued) => true,
        (a, b) if a == b => true,
        _ => false,
    }
}
