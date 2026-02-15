use crate::protocol::{ServerEvent, SessionQuery};
use crate::session::SessionManager;
use async_stream::stream;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::{Json, Router, routing::get};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, UNIX_EPOCH};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, oneshot};
use tokio::task::JoinHandle;

const PORT_FALLBACK_ATTEMPTS: u16 = 12;
const PREVIEW_HTML: &str = include_str!("assets/preview.html");

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub port: u16,
    pub bind_address: String,
    pub debounce_ms_content: u64,
    pub throttle_ms_cursor: u64,
    pub auto_scroll: bool,
    pub scroll_comfort_top: f64,
    pub scroll_comfort_bottom: f64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 6419,
            bind_address: String::from("127.0.0.1"),
            debounce_ms_content: 100,
            throttle_ms_cursor: 24,
            auto_scroll: true,
            scroll_comfort_top: 0.25,
            scroll_comfort_bottom: 0.65,
        }
    }
}

#[derive(Debug)]
struct RuntimeState {
    addr: Option<SocketAddr>,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl RuntimeState {
    fn empty() -> Self {
        Self {
            addr: None,
            shutdown: None,
            task: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServerController {
    config: ServerConfig,
    sessions: SessionManager,
    runtime: Arc<Mutex<RuntimeState>>,
    client_id_counter: Arc<AtomicU64>,
}

impl ServerController {
    pub fn new(config: ServerConfig, sessions: SessionManager) -> Self {
        Self {
            config,
            sessions,
            runtime: Arc::new(Mutex::new(RuntimeState::empty())),
            client_id_counter: Arc::new(AtomicU64::new(1)),
        }
    }

    pub async fn ensure_running(&self) -> Result<SocketAddr, std::io::Error> {
        let mut runtime = self.runtime.lock().await;
        if let Some(addr) = runtime.addr {
            return Ok(addr);
        }

        let (listener, addr) = bind_listener(&self.config).await?;
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let state = HttpState {
            sessions: self.sessions.clone(),
            config: self.config.clone(),
            client_id_counter: self.client_id_counter.clone(),
        };
        let app = build_router(state);

        let task = tokio::spawn(async move {
            let server = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await;

            if let Err(err) = server {
                eprintln!("live-markdown.nvim server stopped with error: {err}");
            }
        });

        runtime.addr = Some(addr);
        runtime.shutdown = Some(shutdown_tx);
        runtime.task = Some(task);

        Ok(addr)
    }

    pub async fn stop(&self) {
        let (shutdown, task) = {
            let mut runtime = self.runtime.lock().await;
            runtime.addr = None;
            (runtime.shutdown.take(), runtime.task.take())
        };

        if let Some(tx) = shutdown {
            let _ = tx.send(());
        }
        if let Some(task) = task {
            let _ = task.await;
        }
    }

    pub async fn bound_addr(&self) -> Option<SocketAddr> {
        self.runtime.lock().await.addr
    }

    pub async fn preview_url(&self) -> Option<String> {
        let addr = self.bound_addr().await?;
        Some(format!("http://{}:{}/", addr.ip(), addr.port()))
    }
}

#[derive(Clone)]
struct HttpState {
    sessions: SessionManager,
    config: ServerConfig,
    client_id_counter: Arc<AtomicU64>,
}

#[derive(Debug, Clone, Deserialize)]
struct AssetQuery {
    buf: i64,
    path: String,
}

#[derive(Debug, Clone, Serialize)]
struct ActiveResponse {
    bufnr: Option<i64>,
}

impl HttpState {
    fn next_client_id(&self) -> u64 {
        self.client_id_counter.fetch_add(1, Ordering::Relaxed)
    }
}

fn build_router(state: HttpState) -> Router {
    Router::new()
        .route("/", get(preview_shell))
        .route("/snapshot", get(snapshot))
        .route("/active", get(active))
        .route("/asset", get(asset))
        .route("/events", get(events))
        .with_state(state)
}

async fn preview_shell(State(state): State<HttpState>) -> impl IntoResponse {
    let html = PREVIEW_HTML
        .replace(
            "__AUTO_SCROLL__",
            if state.config.auto_scroll {
                "true"
            } else {
                "false"
            },
        )
        .replace(
            "__SCROLL_TOP__",
            &format!("{:.2}", state.config.scroll_comfort_top),
        )
        .replace(
            "__SCROLL_BOTTOM__",
            &format!("{:.2}", state.config.scroll_comfort_bottom),
        );

    let mut headers = HeaderMap::new();
    headers.insert(
        "content-security-policy",
        HeaderValue::from_static(
            "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'; img-src 'self' https: http: data:;",
        ),
    );

    (headers, Html(html))
}

async fn snapshot(State(state): State<HttpState>, Query(query): Query<SessionQuery>) -> Response {
    match state.sessions.snapshot(query.buf).await {
        Some(snapshot) => Json(snapshot).into_response(),
        None => json_error(StatusCode::NOT_FOUND, "preview session not found"),
    }
}

async fn active(State(state): State<HttpState>) -> Response {
    let bufnr = state.sessions.active_bufnr().await;
    Json(ActiveResponse { bufnr }).into_response()
}

async fn asset(
    State(state): State<HttpState>,
    request_headers: HeaderMap,
    Query(query): Query<AssetQuery>,
) -> Response {
    let Some(path) = state
        .sessions
        .resolve_local_asset_path(query.buf, &query.path)
        .await
    else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let metadata = match tokio::fs::metadata(&path).await {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return StatusCode::NOT_FOUND.into_response();
        }
        Err(_) => {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let etag = build_asset_etag(&metadata);
    if let Some(ref value) = etag {
        if if_none_match_matches(&request_headers, value) {
            let mut headers = HeaderMap::new();
            headers.insert(
                "cache-control",
                HeaderValue::from_static("private, max-age=60"),
            );
            if let Ok(header_value) = HeaderValue::from_str(value) {
                headers.insert("etag", header_value);
            }
            return (StatusCode::NOT_MODIFIED, headers).into_response();
        }
    }

    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return StatusCode::NOT_FOUND.into_response();
        }
        Err(_) => {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let mut headers = HeaderMap::new();
    headers.insert(
        "content-type",
        HeaderValue::from_static(image_content_type(&path)),
    );
    headers.insert(
        "cache-control",
        HeaderValue::from_static("private, max-age=60"),
    );
    if let Some(value) = etag {
        if let Ok(header_value) = HeaderValue::from_str(&value) {
            headers.insert("etag", header_value);
        }
    }

    (headers, bytes).into_response()
}

async fn events(State(state): State<HttpState>, Query(query): Query<SessionQuery>) -> Response {
    let client_id = state.next_client_id();
    let Some(mut rx) = state.sessions.subscribe(query.buf, client_id).await else {
        return json_error(StatusCode::NOT_FOUND, "preview session not found");
    };

    let sessions = state.sessions.clone();
    let bufnr = query.buf;
    let stream = stream! {
        let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(15));
        heartbeat_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = heartbeat_interval.tick() => {
                    let heartbeat = ServerEvent::Heartbeat { bufnr };
                    yield Ok::<Event, Infallible>(sse_event(&heartbeat));
                }
                recv = rx.recv() => {
                    match recv {
                        Ok(payload) => yield Ok::<Event, Infallible>(sse_event(&payload)),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }

        sessions.unsubscribe(bufnr, client_id).await;
    };

    Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(20))
                .text("keepalive"),
        )
        .into_response()
}

fn sse_event(payload: &ServerEvent) -> Event {
    let data = serde_json::to_string(payload).unwrap_or_else(|_| {
        String::from("{\"type\":\"error\",\"message\":\"serialization_error\"}")
    });

    Event::default().event(payload.event_name()).data(data)
}

fn json_error(status: StatusCode, message: &str) -> Response {
    #[derive(Serialize)]
    struct ErrorBody<'a> {
        error: &'a str,
    }

    (status, Json(ErrorBody { error: message })).into_response()
}

fn image_content_type(path: &Path) -> &'static str {
    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return "application/octet-stream";
    };

    match ext.to_ascii_lowercase().as_str() {
        "png" | "apng" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        "tif" | "tiff" => "image/tiff",
        _ => "application/octet-stream",
    }
}

fn build_asset_etag(metadata: &std::fs::Metadata) -> Option<String> {
    let modified = metadata.modified().ok()?;
    let modified_secs = modified.duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(format!("W/\"{:x}-{:x}\"", metadata.len(), modified_secs))
}

fn if_none_match_matches(headers: &HeaderMap, etag: &str) -> bool {
    let Some(raw_header) = headers.get("if-none-match") else {
        return false;
    };
    let Ok(raw_value) = raw_header.to_str() else {
        return false;
    };

    if raw_value.trim() == "*" {
        return true;
    }

    raw_value
        .split(',')
        .any(|candidate| candidate.trim() == etag)
}

async fn bind_listener(config: &ServerConfig) -> Result<(TcpListener, SocketAddr), std::io::Error> {
    let start_port = config.port;
    let end_port = config
        .port
        .saturating_add(PORT_FALLBACK_ATTEMPTS.saturating_sub(1));
    let mut last_error: Option<std::io::Error> = None;

    for port in start_port..=end_port {
        let addr = format!("{}:{port}", config.bind_address);
        match TcpListener::bind(&addr).await {
            Ok(listener) => {
                let bound = listener.local_addr()?;
                return Ok((listener, bound));
            }
            Err(err) => {
                last_error = Some(err);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| std::io::Error::other("failed to bind preview server")))
}

#[cfg(test)]
mod tests {
    use super::{ServerConfig, if_none_match_matches};
    use axum::http::{HeaderMap, HeaderValue};

    #[test]
    fn config_defaults_match_spec() {
        let cfg = ServerConfig::default();
        assert_eq!(cfg.port, 6419);
        assert_eq!(cfg.bind_address, "127.0.0.1");
        assert_eq!(cfg.debounce_ms_content, 100);
        assert_eq!(cfg.throttle_ms_cursor, 24);
        assert!(cfg.auto_scroll);
        assert!((cfg.scroll_comfort_top - 0.25).abs() < f64::EPSILON);
        assert!((cfg.scroll_comfort_bottom - 0.65).abs() < f64::EPSILON);
    }

    #[test]
    fn if_none_match_recognizes_exact_tag() {
        let mut headers = HeaderMap::new();
        headers.insert("if-none-match", HeaderValue::from_static("W/\"12-34\""));

        assert!(if_none_match_matches(&headers, "W/\"12-34\""));
        assert!(!if_none_match_matches(&headers, "W/\"ab-cd\""));
    }

    #[test]
    fn if_none_match_supports_multiple_and_wildcard_tags() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "if-none-match",
            HeaderValue::from_static("\"one\", W/\"22-33\", \"three\""),
        );
        assert!(if_none_match_matches(&headers, "W/\"22-33\""));

        headers.insert("if-none-match", HeaderValue::from_static("*"));
        assert!(if_none_match_matches(&headers, "W/\"whatever\""));
    }
}
