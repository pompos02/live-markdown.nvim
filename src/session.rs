use crate::protocol::{ServerEvent, SessionEndReason, SnapshotResponse};
use crate::render::LiveMarkdownRenderer;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};

const EVENT_CHANNEL_CAPACITY: usize = 256;

#[derive(Debug, Clone)]
pub struct BufferSnapshot {
    pub bufnr: i64,
    pub changedtick: u64,
    pub markdown: String,
    pub cursor_line: usize,
    pub cursor_col: usize,
    pub source_path: Option<String>,
}

#[derive(Debug)]
struct Session {
    bufnr: i64,
    changedtick: u64,
    content_hash: u64,
    cursor_line: usize,
    cursor_col: usize,
    html: String,
    source_path: Option<PathBuf>,
    broadcaster: broadcast::Sender<ServerEvent>,
}

impl Session {
    fn new(snapshot: &BufferSnapshot, html: String, content_hash: u64) -> Self {
        let (broadcaster, _receiver) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self {
            bufnr: snapshot.bufnr,
            changedtick: snapshot.changedtick,
            content_hash,
            cursor_line: snapshot.cursor_line,
            cursor_col: snapshot.cursor_col,
            html,
            source_path: snapshot_source_path(snapshot.source_path.as_deref()),
            broadcaster,
        }
    }

    fn apply_snapshot(&mut self, snapshot: &BufferSnapshot, html: String, content_hash: u64) {
        self.changedtick = snapshot.changedtick;
        self.content_hash = content_hash;
        self.cursor_line = snapshot.cursor_line;
        self.cursor_col = snapshot.cursor_col;
        self.html = html;
        self.source_path = snapshot_source_path(snapshot.source_path.as_deref());
    }

    fn snapshot_response(&self) -> SnapshotResponse {
        let filename = self
            .source_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| String::from("buffer"));

        SnapshotResponse {
            bufnr: self.bufnr,
            html: self.html.clone(),
            cursor_line: self.cursor_line,
            cursor_col: self.cursor_col,
            filename,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SessionManager {
    active: Arc<RwLock<Option<Session>>>,
}

impl SessionManager {
    pub async fn start_session(&self, snapshot: BufferSnapshot, renderer: &LiveMarkdownRenderer) {
        let rendered_html = renderer.render(&snapshot.markdown);
        let new_hash = content_hash(&snapshot.markdown);

        let mut active = self.active.write().await;
        if let Some(session) = active.as_mut()
            && session.bufnr == snapshot.bufnr
        {
            session.apply_snapshot(&snapshot, rendered_html.clone(), new_hash);
            let _ = session.broadcaster.send(ServerEvent::RenderFull {
                bufnr: snapshot.bufnr,
                html: rendered_html,
                cursor_line: snapshot.cursor_line,
            });
            return;
        }

        let session = Session::new(&snapshot, rendered_html.clone(), new_hash);
        let _ = session.broadcaster.send(ServerEvent::RenderFull {
            bufnr: snapshot.bufnr,
            html: rendered_html,
            cursor_line: snapshot.cursor_line,
        });
        *active = Some(session);
    }

    pub async fn stop_session(&self, bufnr: i64, reason: SessionEndReason) -> bool {
        let mut active = self.active.write().await;
        let Some(session) = active.take() else {
            return false;
        };

        if session.bufnr != bufnr {
            *active = Some(session);
            return false;
        }

        let _ = session
            .broadcaster
            .send(ServerEvent::SessionEnd { bufnr, reason });

        true
    }

    pub async fn stop_all(&self, reason: SessionEndReason) {
        let mut active = self.active.write().await;
        if let Some(session) = active.take() {
            let _ = session.broadcaster.send(ServerEvent::SessionEnd {
                bufnr: session.bufnr,
                reason,
            });
        }
    }

    pub async fn update_content(
        &self,
        snapshot: BufferSnapshot,
        renderer: &LiveMarkdownRenderer,
    ) -> bool {
        let new_hash = content_hash(&snapshot.markdown);

        {
            let active = self.active.read().await;
            let Some(session) = active.as_ref() else {
                return false;
            };

            if session.bufnr != snapshot.bufnr {
                return false;
            }

            if session.changedtick == snapshot.changedtick && session.content_hash == new_hash {
                return false;
            }
        }

        let rendered_html = renderer.render(&snapshot.markdown);

        let mut active = self.active.write().await;
        let Some(session) = active.as_mut() else {
            return false;
        };

        if session.bufnr != snapshot.bufnr {
            return false;
        }

        if session.changedtick == snapshot.changedtick && session.content_hash == new_hash {
            return false;
        }

        session.apply_snapshot(&snapshot, rendered_html.clone(), new_hash);

        let _ = session.broadcaster.send(ServerEvent::RenderFull {
            bufnr: snapshot.bufnr,
            html: rendered_html,
            cursor_line: snapshot.cursor_line,
        });

        true
    }

    pub async fn rerender_content(
        &self,
        snapshot: BufferSnapshot,
        renderer: &LiveMarkdownRenderer,
    ) -> bool {
        let rendered_html = renderer.render(&snapshot.markdown);
        let new_hash = content_hash(&snapshot.markdown);

        let mut active = self.active.write().await;
        let Some(session) = active.as_mut() else {
            return false;
        };

        if session.bufnr != snapshot.bufnr {
            return false;
        };

        session.apply_snapshot(&snapshot, rendered_html.clone(), new_hash);

        let _ = session.broadcaster.send(ServerEvent::RenderFull {
            bufnr: snapshot.bufnr,
            html: rendered_html,
            cursor_line: snapshot.cursor_line,
        });

        true
    }

    pub async fn update_cursor(&self, bufnr: i64, line: usize, col: usize) -> bool {
        let mut active = self.active.write().await;
        let Some(session) = active.as_mut() else {
            return false;
        };

        if session.bufnr != bufnr {
            return false;
        }

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

    pub async fn has_session(&self, bufnr: i64) -> bool {
        let active = self.active.read().await;
        active
            .as_ref()
            .is_some_and(|session| session.bufnr == bufnr)
    }

    pub async fn snapshot(&self, bufnr: i64) -> Option<SnapshotResponse> {
        let active = self.active.read().await;
        let session = active.as_ref()?;
        if session.bufnr != bufnr {
            return None;
        }

        Some(session.snapshot_response())
    }

    pub async fn resolve_local_asset_path(&self, bufnr: i64, raw_path: &str) -> Option<PathBuf> {
        let active = self.active.read().await;
        let session = active.as_ref()?;
        if session.bufnr != bufnr {
            return None;
        }

        let source_file = session.source_path.as_ref()?;
        let source_dir = source_file.parent()?.canonicalize().ok()?;

        let reference = parse_local_asset_reference(raw_path)?;
        let candidate = if reference.is_absolute() {
            reference
        } else {
            source_dir.join(reference)
        };

        let resolved = candidate.canonicalize().ok()?;
        if !resolved.starts_with(&source_dir) {
            return None;
        }
        if !resolved.is_file() {
            return None;
        }
        if !is_supported_image_path(&resolved) {
            return None;
        }

        Some(resolved)
    }

    pub async fn subscribe(&self, bufnr: i64) -> Option<broadcast::Receiver<ServerEvent>> {
        let active = self.active.read().await;
        let session = active.as_ref()?;
        if session.bufnr != bufnr {
            return None;
        }

        Some(session.broadcaster.subscribe())
    }

    pub async fn session_count(&self) -> usize {
        let active = self.active.read().await;
        if active.is_some() { 1 } else { 0 }
    }

    pub async fn active_bufnr(&self) -> Option<i64> {
        let active = self.active.read().await;
        active.as_ref().map(|session| session.bufnr)
    }
}

fn content_hash(input: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    hasher.finish()
}

fn snapshot_source_path(path: Option<&str>) -> Option<PathBuf> {
    let trimmed = path?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

fn parse_local_asset_reference(raw_path: &str) -> Option<PathBuf> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    let candidate = if has_url_scheme(trimmed) {
        if !lower.starts_with("file://") {
            return None;
        }
        &trimmed[7..]
    } else {
        trimmed
    };

    let end = candidate.find(['?', '#']).unwrap_or(candidate.len());
    if end == 0 {
        return None;
    }

    let without_suffix = &candidate[..end];
    let decoded = decode_percent_encoded(without_suffix)?;
    if decoded.trim().is_empty() {
        return None;
    }

    Some(PathBuf::from(decoded))
}

fn has_url_scheme(value: &str) -> bool {
    if value.len() >= 3 {
        let bytes = value.as_bytes();
        if bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes[2] == b'\\' || bytes[2] == b'/')
        {
            return false;
        }
    }

    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() {
        return false;
    }

    for ch in chars {
        if ch == ':' {
            return true;
        }
        if ch.is_ascii_alphanumeric() || ch == '+' || ch == '-' || ch == '.' {
            continue;
        }
        return false;
    }

    false
}

fn decode_percent_encoded(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }

        if index + 2 >= bytes.len() {
            return None;
        }

        let high = decode_hex_nibble(bytes[index + 1])?;
        let low = decode_hex_nibble(bytes[index + 2])?;
        decoded.push((high << 4) | low);
        index += 3;
    }

    String::from_utf8(decoded).ok()
}

fn decode_hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn is_supported_image_path(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };

    matches!(
        ext.to_ascii_lowercase().as_str(),
        "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "svg"
            | "bmp"
            | "ico"
            | "avif"
            | "apng"
            | "tif"
            | "tiff"
    )
}

#[cfg(test)]
mod tests {
    use super::{BufferSnapshot, SessionManager};
    use crate::protocol::{ServerEvent, SessionEndReason};
    use crate::render::LiveMarkdownRenderer;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("live-markdown.nvim-{name}-{nanos}"))
    }

    #[tokio::test]
    async fn session_start_update_and_stop_lifecycle() {
        let sessions = SessionManager::default();
        let renderer = LiveMarkdownRenderer::default();

        sessions
            .start_session(
                BufferSnapshot {
                    bufnr: 1,
                    changedtick: 1,
                    markdown: String::from("# hello"),
                    cursor_line: 1,
                    cursor_col: 0,
                    source_path: None,
                },
                &renderer,
            )
            .await;

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
                    source_path: None,
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
    async fn starting_new_buffer_replaces_previous_session() {
        let sessions = SessionManager::default();
        let renderer = LiveMarkdownRenderer::default();

        sessions
            .start_session(
                BufferSnapshot {
                    bufnr: 11,
                    changedtick: 1,
                    markdown: String::from("# first"),
                    cursor_line: 1,
                    cursor_col: 0,
                    source_path: None,
                },
                &renderer,
            )
            .await;

        sessions
            .start_session(
                BufferSnapshot {
                    bufnr: 22,
                    changedtick: 1,
                    markdown: String::from("# second"),
                    cursor_line: 1,
                    cursor_col: 0,
                    source_path: None,
                },
                &renderer,
            )
            .await;

        assert_eq!(sessions.session_count().await, 1);
        assert_eq!(sessions.active_bufnr().await, Some(22));
        assert!(!sessions.has_session(11).await);
        assert!(sessions.snapshot(11).await.is_none());
        assert!(sessions.snapshot(22).await.is_some());
    }

    #[tokio::test]
    async fn cursor_updates_ignore_duplicates() {
        let sessions = SessionManager::default();
        let renderer = LiveMarkdownRenderer::default();

        sessions
            .start_session(
                BufferSnapshot {
                    bufnr: 2,
                    changedtick: 1,
                    markdown: String::from("line"),
                    cursor_line: 1,
                    cursor_col: 0,
                    source_path: None,
                },
                &renderer,
            )
            .await;

        assert!(!sessions.update_cursor(2, 1, 0).await);
        assert!(sessions.update_cursor(2, 2, 3).await);
        assert!(!sessions.update_cursor(2, 2, 3).await);
    }

    #[tokio::test]
    async fn subscription_requires_active_session() {
        let sessions = SessionManager::default();
        let renderer = LiveMarkdownRenderer::default();

        sessions
            .start_session(
                BufferSnapshot {
                    bufnr: 3,
                    changedtick: 1,
                    markdown: String::from("line"),
                    cursor_line: 1,
                    cursor_col: 0,
                    source_path: None,
                },
                &renderer,
            )
            .await;

        let mut rx = sessions.subscribe(3).await.expect("valid subscription");

        assert!(sessions.subscribe(99).await.is_none());
        assert!(sessions.update_cursor(3, 4, 0).await);

        let event = rx.recv().await.expect("event");
        match event {
            ServerEvent::CursorMove { bufnr, line, .. } => {
                assert_eq!(bufnr, 3);
                assert_eq!(line, 4);
            }
            _ => panic!("unexpected event"),
        }

        sessions
            .start_session(
                BufferSnapshot {
                    bufnr: 30,
                    changedtick: 1,
                    markdown: String::from("# switched"),
                    cursor_line: 1,
                    cursor_col: 0,
                    source_path: None,
                },
                &renderer,
            )
            .await;

        assert!(!sessions.has_session(3).await);
        assert!(sessions.has_session(30).await);
        assert_eq!(sessions.active_bufnr().await, Some(30));

        sessions.stop_all(SessionEndReason::Stopped).await;
        assert_eq!(sessions.session_count().await, 0);
        assert!(!sessions.has_session(3).await);
    }

    #[tokio::test]
    async fn rerender_content_forces_emit_without_text_changes() {
        let sessions = SessionManager::default();
        let renderer = LiveMarkdownRenderer::default();

        sessions
            .start_session(
                BufferSnapshot {
                    bufnr: 4,
                    changedtick: 10,
                    markdown: String::from("# title"),
                    cursor_line: 1,
                    cursor_col: 0,
                    source_path: None,
                },
                &renderer,
            )
            .await;

        let mut rx = sessions.subscribe(4).await.expect("valid subscription");

        assert!(
            sessions
                .rerender_content(
                    BufferSnapshot {
                        bufnr: 4,
                        changedtick: 10,
                        markdown: String::from("# title"),
                        cursor_line: 1,
                        cursor_col: 0,
                        source_path: None,
                    },
                    &renderer,
                )
                .await
        );

        let event = rx.recv().await.expect("render event");
        match event {
            ServerEvent::RenderFull {
                bufnr, cursor_line, ..
            } => {
                assert_eq!(bufnr, 4);
                assert_eq!(cursor_line, 1);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolves_image_asset_paths_from_buffer_directory() {
        let sessions = SessionManager::default();
        let renderer = LiveMarkdownRenderer::default();

        let root = temp_test_dir("assets");
        let image_dir = root.join("images");
        fs::create_dir_all(&image_dir).expect("create image dir");

        let markdown_path = root.join("note.md");
        fs::write(&markdown_path, "# note").expect("write markdown file");

        let image_path = image_dir.join("diagram.png");
        fs::write(&image_path, [137u8, 80, 78, 71]).expect("write image file");

        sessions
            .start_session(
                BufferSnapshot {
                    bufnr: 88,
                    changedtick: 1,
                    markdown: String::from("![diagram](images/diagram.png)"),
                    cursor_line: 1,
                    cursor_col: 0,
                    source_path: Some(markdown_path.to_string_lossy().to_string()),
                },
                &renderer,
            )
            .await;

        let resolved = sessions
            .resolve_local_asset_path(88, "images/diagram.png")
            .await
            .expect("resolve relative image");
        assert_eq!(resolved, image_path.canonicalize().expect("canonical path"));

        let encoded = sessions
            .resolve_local_asset_path(88, "images/diagram%2Epng")
            .await;
        assert!(encoded.is_some());

        let escaped = sessions.resolve_local_asset_path(88, "../secret.png").await;
        assert!(escaped.is_none());

        let remote = sessions
            .resolve_local_asset_path(88, "https://example.com/image.png")
            .await;
        assert!(remote.is_none());

        let missing_session = sessions
            .resolve_local_asset_path(99, "images/diagram.png")
            .await;
        assert!(missing_session.is_none());

        let _ = fs::remove_dir_all(root);
    }
}
