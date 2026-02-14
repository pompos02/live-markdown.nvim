use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct SessionQuery {
    pub buf: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionEndReason {
    Stopped,
    BufferClosed,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotResponse {
    pub bufnr: i64,
    pub html: String,
    pub cursor_line: usize,
    pub cursor_col: usize,
    pub filename: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    RenderFull {
        bufnr: i64,
        html: String,
        cursor_line: usize,
    },
    CursorMove {
        bufnr: i64,
        line: usize,
        col: usize,
    },
    SessionEnd {
        bufnr: i64,
        reason: SessionEndReason,
    },
    Heartbeat {
        bufnr: i64,
    },
}

impl ServerEvent {
    pub fn event_name(&self) -> &'static str {
        match self {
            Self::RenderFull { .. } => "render_full",
            Self::CursorMove { .. } => "cursor_move",
            Self::SessionEnd { .. } => "session_end",
            Self::Heartbeat { .. } => "heartbeat",
        }
    }

    pub fn bufnr(&self) -> i64 {
        match self {
            Self::RenderFull { bufnr, .. } => *bufnr,
            Self::CursorMove { bufnr, .. } => *bufnr,
            Self::SessionEnd { bufnr, .. } => *bufnr,
            Self::Heartbeat { bufnr } => *bufnr,
        }
    }
}
