use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct AutocmdGate {
    content_window: Duration,
    cursor_window: Duration,
    state: Arc<Mutex<GateState>>,
}

#[derive(Debug, Default)]
struct GateState {
    last_content_emit: HashMap<i64, Instant>,
    last_cursor_emit: HashMap<i64, Instant>,
    last_cursor_line: HashMap<i64, usize>,
}

impl AutocmdGate {
    pub fn new(content_window: Duration, cursor_window: Duration) -> Self {
        Self {
            content_window,
            cursor_window,
            state: Arc::new(Mutex::new(GateState::default())),
        }
    }

    pub async fn allow_content_emit(&self, bufnr: i64) -> bool {
        let now = Instant::now();
        let mut state = self.state.lock().await;
        match state.last_content_emit.get(&bufnr) {
            Some(last) if now.duration_since(*last) < self.content_window => false,
            _ => {
                state.last_content_emit.insert(bufnr, now);
                true
            }
        }
    }

    pub async fn allow_cursor_emit(&self, bufnr: i64, line: usize) -> bool {
        let now = Instant::now();
        let mut state = self.state.lock().await;

        if state
            .last_cursor_line
            .get(&bufnr)
            .is_some_and(|last| *last == line)
        {
            return false;
        }

        let allow_time = match state.last_cursor_emit.get(&bufnr) {
            Some(last) => now.duration_since(*last) >= self.cursor_window,
            None => true,
        };

        if !allow_time {
            return false;
        }

        state.last_cursor_emit.insert(bufnr, now);
        state.last_cursor_line.insert(bufnr, line);
        true
    }

    pub async fn clear_buffer(&self, bufnr: i64) {
        let mut state = self.state.lock().await;
        state.last_content_emit.remove(&bufnr);
        state.last_cursor_emit.remove(&bufnr);
        state.last_cursor_line.remove(&bufnr);
    }
}

#[cfg(test)]
mod tests {
    use super::AutocmdGate;
    use std::time::Duration;

    #[tokio::test]
    async fn cursor_gate_rejects_duplicate_lines() {
        let gate = AutocmdGate::new(Duration::from_millis(100), Duration::from_millis(20));

        assert!(gate.allow_cursor_emit(1, 10).await);
        assert!(!gate.allow_cursor_emit(1, 10).await);
    }
}
