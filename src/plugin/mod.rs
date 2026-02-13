pub mod autocmd;
pub mod commands;

use crate::protocol::SessionEndReason;
use crate::render::MarkdownRenderer;
use crate::server::{ServerConfig, ServerController};
use crate::session::{BufferSnapshot, SessionManager};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::process::Command;
use std::time::Duration;

#[derive(Debug)]
pub enum PluginError {
    Io(std::io::Error),
}

impl Display for PluginError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "io error: {err}"),
        }
    }
}

impl Error for PluginError {}

impl From<std::io::Error> for PluginError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug, Clone)]
pub struct MarkdownRenderPlugin {
    renderer: MarkdownRenderer,
    sessions: SessionManager,
    server: ServerController,
    autocmd: autocmd::AutocmdGate,
    config: ServerConfig,
}

impl Default for MarkdownRenderPlugin {
    fn default() -> Self {
        Self::new(ServerConfig::default())
    }
}

impl MarkdownRenderPlugin {
    pub fn new(config: ServerConfig) -> Self {
        let sessions = SessionManager::default();
        let server = ServerController::new(config.clone(), sessions.clone());
        let autocmd = autocmd::AutocmdGate::new(
            Duration::from_millis(config.debounce_ms_content),
            Duration::from_millis(config.throttle_ms_cursor),
        );

        Self {
            renderer: MarkdownRenderer::default(),
            sessions,
            server,
            autocmd,
            config,
        }
    }

    pub fn sessions(&self) -> SessionManager {
        self.sessions.clone()
    }

    pub async fn has_session(&self, bufnr: i64) -> bool {
        self.sessions.session_token(bufnr).await.is_some()
    }

    pub async fn start_preview(&self, snapshot: BufferSnapshot) -> Result<String, PluginError> {
        let addr = self.server.ensure_running().await?;
        let started = self.sessions.start_session(snapshot, &self.renderer).await;

        let url = format!(
            "http://{}:{}/?token={}&buf={}",
            addr.ip(),
            addr.port(),
            started.token,
            started.bufnr
        );

        if self.config.open_browser_on_start {
            open_browser(&url);
        }

        Ok(url)
    }

    pub async fn stop_preview(&self, bufnr: i64) -> Result<bool, PluginError> {
        let stopped = self
            .sessions
            .stop_session(bufnr, SessionEndReason::Stopped)
            .await;
        self.autocmd.clear_buffer(bufnr).await;

        if self.sessions.session_count().await == 0 {
            self.server.stop().await;
        }

        Ok(stopped)
    }

    pub async fn stop_all_previews(&self) {
        self.sessions.stop_all(SessionEndReason::Stopped).await;
        self.server.stop().await;
    }

    pub async fn toggle_preview(
        &self,
        snapshot: BufferSnapshot,
    ) -> Result<Option<String>, PluginError> {
        if self.sessions.session_token(snapshot.bufnr).await.is_some() {
            let _ = self.stop_preview(snapshot.bufnr).await?;
            return Ok(None);
        }

        self.start_preview(snapshot).await.map(Some)
    }

    pub async fn open_preview(&self, bufnr: i64) -> Result<Option<String>, PluginError> {
        let Some(token) = self.sessions.session_token(bufnr).await else {
            return Ok(None);
        };

        self.server.ensure_running().await?;
        Ok(self.server.preview_url_for(bufnr, &token).await)
    }

    pub async fn on_text_changed(&self, snapshot: BufferSnapshot) {
        if self.autocmd.allow_content_emit(snapshot.bufnr).await {
            let _ = self.sessions.update_content(snapshot, &self.renderer).await;
        }
    }

    pub async fn on_cursor_moved(&self, bufnr: i64, line: usize, col: usize) {
        if self.autocmd.allow_cursor_emit(bufnr, line).await {
            let _ = self.sessions.update_cursor(bufnr, line, col).await;
        }
    }

    pub async fn on_buf_enter(&self, bufnr: i64) {
        self.sessions.resume_session(bufnr).await;
    }

    pub async fn on_buf_leave(&self, bufnr: i64) {
        self.sessions.pause_session(bufnr).await;
    }

    pub async fn on_buf_wipeout(&self, bufnr: i64) -> Result<(), PluginError> {
        let _ = self
            .sessions
            .stop_session(bufnr, SessionEndReason::BufferClosed)
            .await;
        self.autocmd.clear_buffer(bufnr).await;

        if self.sessions.session_count().await == 0 {
            self.server.stop().await;
        }

        Ok(())
    }

    pub async fn shutdown(&self) {
        self.sessions.stop_all(SessionEndReason::Stopped).await;
        self.server.stop().await;
    }
}

pub fn launch_browser(url: &str) {
    open_browser(url);
}

fn open_browser(url: &str) {
    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("xdg-open").arg(url).spawn();
    }

    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg(url).spawn();
    }

    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("cmd").args(["/C", "start", "", url]).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::MarkdownRenderPlugin;
    use crate::server::ServerConfig;
    use crate::session::BufferSnapshot;

    #[tokio::test]
    async fn toggle_starts_and_stops_session() {
        let plugin = MarkdownRenderPlugin::new(ServerConfig {
            open_browser_on_start: false,
            ..ServerConfig::default()
        });

        let buffer = BufferSnapshot {
            bufnr: 5,
            changedtick: 1,
            markdown: String::from("# hello"),
            cursor_line: 1,
            cursor_col: 0,
        };

        let started = plugin
            .toggle_preview(buffer.clone())
            .await
            .expect("start preview");
        assert!(started.is_some());

        let stopped = plugin.toggle_preview(buffer).await.expect("stop preview");
        assert!(stopped.is_none());
    }
}
