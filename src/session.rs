use crate::protocol::{ServerEvent, SessionEndReason, SnapshotResponse};
use crate::render::MarkdownRenderer;
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::{RwLock, broadcast};

const EVENT_CHANNEL_CAPACITY: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    Idle,
    Running,
    Paused,
    Stopped,
}

pub type ClientId = u64;

#[derive(Debug, Clone)]
pub struct BufferSnapshot {
    pub bufnr: i64,
    pub changedtick: u64,
    pub markdown: String,
    pub cursor_line: usize,
    pub cursor_col: usize,
}

#[derive(Debug, Clone)]
pub struct SessionStartInfo {
    pub bufnr: i64,
    pub token: String,
}

#[derive(Debug)]
struct Session {
    bufnr: i64,
    changedtick: u64,
    content_hash: u64,
    cursor_line: usize,
    cursor_col: usize,
    subscribers: HashSet<ClientId>,
    token: String,
    html: String,
    state: LifecycleState,
    broadcaster: broadcast::Sender<ServerEvent>,
}

impl Session {
    fn new(bufnr: i64, token: String) -> Self {
        let (broadcaster, _receiver) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self {
            bufnr,
            changedtick: 0,
            content_hash: 0,
            cursor_line: 1,
            cursor_col: 0,
            subscribers: HashSet::new(),
            token,
            html: String::new(),
            state: LifecycleState::Idle,
            broadcaster,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<i64, Session>>>,
    active_bufnr: Arc<RwLock<Option<i64>>>,
    token_counter: Arc<AtomicU64>,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            active_bufnr: Arc::new(RwLock::new(None)),
            token_counter: Arc::new(AtomicU64::new(1)),
        }
    }
}

impl SessionManager {
    pub async fn start_session(
        &self,
        snapshot: BufferSnapshot,
        renderer: &MarkdownRenderer,
    ) -> SessionStartInfo {
        let rendered_html = renderer.render(&snapshot.markdown);
        let content_hash = content_hash(&snapshot.markdown);

        let mut sessions = self.sessions.write().await;
        let session = sessions
            .entry(snapshot.bufnr)
            .or_insert_with(|| Session::new(snapshot.bufnr, self.generate_token()));

        session.state = LifecycleState::Running;
        session.changedtick = snapshot.changedtick;
        session.content_hash = content_hash;
        session.cursor_line = snapshot.cursor_line;
        session.cursor_col = snapshot.cursor_col;
        session.html = rendered_html.clone();

        let _ = session.broadcaster.send(ServerEvent::RenderFull {
            bufnr: snapshot.bufnr,
            html: rendered_html,
            cursor_line: snapshot.cursor_line,
        });

        let info = SessionStartInfo {
            bufnr: snapshot.bufnr,
            token: session.token.clone(),
        };

        drop(sessions);
        *self.active_bufnr.write().await = Some(snapshot.bufnr);

        info
    }

    pub async fn stop_session(&self, bufnr: i64, reason: SessionEndReason) -> bool {
        let mut sessions = self.sessions.write().await;
        let Some(mut session) = sessions.remove(&bufnr) else {
            return false;
        };

        session.state = LifecycleState::Stopped;
        let _ = session
            .broadcaster
            .send(ServerEvent::SessionEnd { bufnr, reason });

        drop(sessions);

        let mut active = self.active_bufnr.write().await;
        if active
            .as_ref()
            .is_some_and(|active_buf| *active_buf == bufnr)
        {
            *active = None;
        }

        true
    }

    pub async fn stop_all(&self, reason: SessionEndReason) {
        let mut sessions = self.sessions.write().await;
        let removed: Vec<_> = sessions.drain().collect();
        drop(sessions);

        for (bufnr, mut session) in removed {
            session.state = LifecycleState::Stopped;
            let _ = session.broadcaster.send(ServerEvent::SessionEnd {
                bufnr,
                reason: reason.clone(),
            });
        }

        *self.active_bufnr.write().await = None;
    }

    pub async fn pause_session(&self, bufnr: i64) {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(&bufnr) {
            session.state = LifecycleState::Paused;
        }
    }

    pub async fn resume_session(&self, bufnr: i64) {
        {
            let mut sessions = self.sessions.write().await;
            if let Some(session) = sessions.get_mut(&bufnr) {
                session.state = LifecycleState::Running;
            }
        }
        *self.active_bufnr.write().await = Some(bufnr);
    }

    pub async fn update_content(
        &self,
        snapshot: BufferSnapshot,
        renderer: &MarkdownRenderer,
    ) -> bool {
        let new_hash = content_hash(&snapshot.markdown);

        {
            let sessions = self.sessions.read().await;
            let Some(session) = sessions.get(&snapshot.bufnr) else {
                return false;
            };
            if session.changedtick == snapshot.changedtick && session.content_hash == new_hash {
                return false;
            }
        }

        let rendered_html = renderer.render(&snapshot.markdown);

        let mut sessions = self.sessions.write().await;
        let Some(session) = sessions.get_mut(&snapshot.bufnr) else {
            return false;
        };

        if session.changedtick == snapshot.changedtick && session.content_hash == new_hash {
            return false;
        }

        session.changedtick = snapshot.changedtick;
        session.content_hash = new_hash;
        session.cursor_line = snapshot.cursor_line;
        session.cursor_col = snapshot.cursor_col;
        session.html = rendered_html.clone();

        let _ = session.broadcaster.send(ServerEvent::RenderFull {
            bufnr: snapshot.bufnr,
            html: rendered_html,
            cursor_line: snapshot.cursor_line,
        });

        true
    }

    pub async fn update_cursor(&self, bufnr: i64, line: usize, col: usize) -> bool {
        let mut sessions = self.sessions.write().await;
        let Some(session) = sessions.get_mut(&bufnr) else {
            return false;
        };

        if session.cursor_line == line && session.cursor_col == col {
            return false;
        }

        session.cursor_line = line;
        session.cursor_col = col;

        let _ = session
            .broadcaster
            .send(ServerEvent::CursorMove { bufnr, line, col });
        true
    }

    pub async fn session_token(&self, bufnr: i64) -> Option<String> {
        let sessions = self.sessions.read().await;
        sessions.get(&bufnr).map(|session| session.token.clone())
    }

    pub async fn verify_token(&self, bufnr: i64, token: &str) -> bool {
        let sessions = self.sessions.read().await;
        sessions.get(&bufnr).is_some_and(|session| {
            session.token == token && session.state != LifecycleState::Stopped
        })
    }

    pub async fn snapshot(&self, bufnr: i64, token: &str) -> Option<SnapshotResponse> {
        let sessions = self.sessions.read().await;
        let session = sessions.get(&bufnr)?;
        if session.token != token || session.state == LifecycleState::Stopped {
            return None;
        }

        Some(SnapshotResponse {
            bufnr: session.bufnr,
            html: session.html.clone(),
            cursor_line: session.cursor_line,
            cursor_col: session.cursor_col,
        })
    }

    pub async fn subscribe(
        &self,
        bufnr: i64,
        token: &str,
        client_id: ClientId,
    ) -> Option<broadcast::Receiver<ServerEvent>> {
        let mut sessions = self.sessions.write().await;
        let session = sessions.get_mut(&bufnr)?;
        if session.token != token || session.state == LifecycleState::Stopped {
            return None;
        }

        session.subscribers.insert(client_id);
        Some(session.broadcaster.subscribe())
    }

    pub async fn unsubscribe(&self, bufnr: i64, client_id: ClientId) {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(&bufnr) {
            session.subscribers.remove(&client_id);
        }
    }

    pub async fn session_count(&self) -> usize {
        self.sessions.read().await.len()
    }

    pub async fn active_bufnr(&self) -> Option<i64> {
        *self.active_bufnr.read().await
    }

    fn generate_token(&self) -> String {
        use std::time::{SystemTime, UNIX_EPOCH};

        let unix_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let counter = self.token_counter.fetch_add(1, Ordering::Relaxed);

        format!("{unix_ns:x}{counter:x}")
    }
}

fn content_hash(input: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::{BufferSnapshot, LifecycleState, SessionManager};
    use crate::protocol::{ServerEvent, SessionEndReason};
    use crate::render::MarkdownRenderer;

    #[tokio::test]
    async fn session_start_update_and_stop_lifecycle() {
        let sessions = SessionManager::default();
        let renderer = MarkdownRenderer::default();

        let start = sessions
            .start_session(
                BufferSnapshot {
                    bufnr: 1,
                    changedtick: 1,
                    markdown: String::from("# hello"),
                    cursor_line: 1,
                    cursor_col: 0,
                },
                &renderer,
            )
            .await;

        assert_eq!(start.bufnr, 1);
        assert_eq!(sessions.session_count().await, 1);
        assert_eq!(sessions.active_bufnr().await, Some(1));

        let updated = sessions
            .update_content(
                BufferSnapshot {
                    bufnr: 1,
                    changedtick: 2,
                    markdown: String::from("# hello\n\nmore"),
                    cursor_line: 2,
                    cursor_col: 0,
                },
                &renderer,
            )
            .await;
        assert!(updated);

        let stopped = sessions.stop_session(1, SessionEndReason::Stopped).await;
        assert!(stopped);
        assert_eq!(sessions.session_count().await, 0);
        assert_eq!(sessions.active_bufnr().await, None);
    }

    #[tokio::test]
    async fn cursor_updates_ignore_duplicates() {
        let sessions = SessionManager::default();
        let renderer = MarkdownRenderer::default();

        sessions
            .start_session(
                BufferSnapshot {
                    bufnr: 2,
                    changedtick: 1,
                    markdown: String::from("line"),
                    cursor_line: 1,
                    cursor_col: 0,
                },
                &renderer,
            )
            .await;

        assert!(!sessions.update_cursor(2, 1, 0).await);
        assert!(sessions.update_cursor(2, 2, 3).await);
        assert!(!sessions.update_cursor(2, 2, 3).await);
    }

    #[tokio::test]
    async fn subscription_requires_valid_token() {
        let sessions = SessionManager::default();
        let renderer = MarkdownRenderer::default();

        let start = sessions
            .start_session(
                BufferSnapshot {
                    bufnr: 3,
                    changedtick: 1,
                    markdown: String::from("line"),
                    cursor_line: 1,
                    cursor_col: 0,
                },
                &renderer,
            )
            .await;

        let mut rx = sessions
            .subscribe(3, &start.token, 99)
            .await
            .expect("valid subscription");

        assert!(sessions.subscribe(3, "bad-token", 100).await.is_none());
        assert!(sessions.update_cursor(3, 4, 0).await);

        let event = rx.recv().await.expect("event");
        match event {
            ServerEvent::CursorMove { bufnr, line, .. } => {
                assert_eq!(bufnr, 3);
                assert_eq!(line, 4);
            }
            _ => panic!("unexpected event"),
        }

        sessions.pause_session(3).await;
        assert!(sessions.verify_token(3, &start.token).await);

        sessions.resume_session(3).await;
        assert_eq!(sessions.active_bufnr().await, Some(3));

        sessions.stop_all(SessionEndReason::Stopped).await;
        assert_eq!(sessions.session_count().await, 0);
        assert!(!sessions.verify_token(3, &start.token).await);
    }

    #[test]
    fn lifecycle_states_exist_for_transitions() {
        assert_eq!(LifecycleState::Idle as u8, 0);
        assert_eq!(LifecycleState::Running as u8, 1);
        assert_eq!(LifecycleState::Paused as u8, 2);
        assert_eq!(LifecycleState::Stopped as u8, 3);
    }
}
