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
    last_content_emit: Option<(i64, Instant)>,
    last_cursor_emit: Option<(i64, Instant, usize)>,
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
        match state.last_content_emit {
            Some((last_bufnr, last_emit))
                if last_bufnr == bufnr && now.duration_since(last_emit) < self.content_window =>
            {
                false
            }
            _ => {
                state.last_content_emit = Some((bufnr, now));
                true
            }
        }
    }

    pub async fn allow_cursor_emit(&self, bufnr: i64, line: usize) -> bool {
        let now = Instant::now();
        let mut state = self.state.lock().await;

        if state
            .last_cursor_emit
            .is_some_and(|(last_bufnr, _, last_line)| last_bufnr == bufnr && last_line == line)
        {
            return false;
        }

        let allow_time = match state.last_cursor_emit {
            Some((last_bufnr, last_emit, _)) if last_bufnr == bufnr => {
                now.duration_since(last_emit) >= self.cursor_window
            }
            _ => true,
        };

        if !allow_time {
            return false;
        }

        state.last_cursor_emit = Some((bufnr, now, line));
        true
    }

    pub async fn clear_buffer(&self, bufnr: i64) {
        let mut state = self.state.lock().await;
        if state
            .last_content_emit
            .is_some_and(|(last_bufnr, _)| last_bufnr == bufnr)
        {
            state.last_content_emit = None;
        }

        if state
            .last_cursor_emit
            .is_some_and(|(last_bufnr, _, _)| last_bufnr == bufnr)
        {
            state.last_cursor_emit = None;
        }
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
