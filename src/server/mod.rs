//! The local web app: a small HTTP server on 127.0.0.1 that serves the UI (embedded in the
//! binary), a JSON API over the notebook, server-sent events when files change, and two
//! WebSockets that relay audio to Gemini (live transcription and the voice assistant).
//!
//! Only this machine can reach it, and every request must name localhost as its Host (and, for
//! anything that changes state, come from the app's own origin), so other websites can't drive it.

mod agent;
mod api;
mod transcribe;
mod watch;

use crate::config::Config;
use crate::store::Store;
use crate::tools::View;
use anyhow::{Context, Result};
use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use rust_embed::RustEmbed;
use serde::Serialize;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::broadcast;

#[derive(RustEmbed)]
#[folder = "web/dist"]
struct Assets;

/// Something the browser should know about, sent over `/api/events`.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// Files under these paths changed (from any source: the app, the assistant, another editor).
    Changed { paths: Vec<String> },
}

/// A line for the terminal dashboard.
#[derive(Clone, Debug)]
pub struct Activity(pub String);

pub struct AppState {
    pub config: RwLock<Config>,
    pub store: Store,
    pub port: u16,
    pub events: broadcast::Sender<Event>,
    pub activity: broadcast::Sender<Activity>,
    /// What the browser shows, for the assistant's `get_current_view`.
    pub view: Mutex<View>,
}

pub type Shared = Arc<AppState>;

impl AppState {
    pub fn new(config: Config, port: u16) -> Result<Shared> {
        let store = Store::open(&config.data_dir)?;
        let (events, _) = broadcast::channel(256);
        let (activity, _) = broadcast::channel(256);
        Ok(Arc::new(Self {
            config: RwLock::new(config),
            store,
            port,
            events,
            activity,
            view: Mutex::new(View::default()),
        }))
    }

    pub fn log(&self, line: impl Into<String>) {
        let _ = self.activity.send(Activity(line.into()));
    }

    pub fn api_key(&self) -> Option<String> {
        self.config.read().ok().and_then(|c| c.api_key.clone()).filter(|k| !k.is_empty())
    }

    pub fn changed(&self, paths: Vec<String>) {
        if !paths.is_empty() {
            let _ = self.events.send(Event::Changed { paths });
        }
    }
}

pub fn router(state: Shared) -> Router {
    Router::new()
        .route("/api/health", get(api::health))
        .route("/api/state", get(api::app_state))
        .route("/api/settings", put(api::update_settings))
        .route("/api/tree", get(api::tree))
        .route("/api/note", get(api::read_note).put(api::write_note))
        .route("/api/notes", post(api::create_note))
        .route("/api/topics", post(api::create_topic))
        .route("/api/rename", post(api::rename))
        .route("/api/move", post(api::move_item))
        .route("/api/delete", post(api::delete))
        .route("/api/restore", post(api::restore))
        .route("/api/reveal", post(api::reveal))
        .route("/api/events", get(api::events))
        .route("/ws/transcribe", get(transcribe::upgrade))
        .route("/ws/agent", get(agent::upgrade))
        .fallback(static_file)
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

/// Refuses requests that don't come from this app on localhost (DNS rebinding, CSRF, other
/// sites' scripts). Also sets conservative security headers.
async fn guard(State(state): State<Shared>, req: Request, next: Next) -> Response {
    let port = state.port;
    let host_ok = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| allowed_host(h, port));
    if !host_ok {
        return (StatusCode::FORBIDDEN, "Parrot only answers on localhost").into_response();
    }
    let changes_state = req.method() != Method::GET && req.method() != Method::HEAD;
    let is_socket = req.uri().path().starts_with("/ws/") || req.uri().path() == "/api/events";
    if changes_state || is_socket {
        if let Some(origin) = req.headers().get(header::ORIGIN) {
            if !origin.to_str().is_ok_and(|o| allowed_origin(o, port)) {
                return (StatusCode::FORBIDDEN, "cross-origin request refused").into_response();
            }
        } else if req.uri().path().starts_with("/ws/") {
            return (StatusCode::FORBIDDEN, "missing origin").into_response();
        }
    }
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    resp
}

pub fn allowed_host(host: &str, port: u16) -> bool {
    [format!("localhost:{port}"), format!("127.0.0.1:{port}"), format!("[::1]:{port}")]
        .iter()
        .any(|h| h == host)
}

pub fn allowed_origin(origin: &str, port: u16) -> bool {
    [
        format!("http://localhost:{port}"),
        format!("http://127.0.0.1:{port}"),
        format!("http://[::1]:{port}"),
    ]
    .iter()
    .any(|o| o == origin)
}

/// Serves the embedded UI; unknown paths get index.html (client-side routing).
async fn static_file(uri: Uri, headers: HeaderMap) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") || path.starts_with("ws/") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    let (file, name) = match Assets::get(path).filter(|_| !path.is_empty()) {
        Some(f) => (f, path.to_string()),
        None => match Assets::get("index.html") {
            Some(f) => (f, "index.html".to_string()),
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    "The web app wasn't built into this binary (run `npm run build` in web/).",
                )
                    .into_response();
            }
        },
    };
    let etag = format!("\"{}\"", hex(&file.metadata.sha256_hash()[..8]));
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return StatusCode::NOT_MODIFIED.into_response();
    }
    let mime = mime_guess::from_path(&name).first_or_octet_stream();
    // Hashed asset names never change; index.html must always be revalidated.
    let cache = if name.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    Response::builder()
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CACHE_CONTROL, cache)
        .header(header::ETAG, etag)
        .body(Body::from(file.data.into_owned()))
        .unwrap()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Binds 127.0.0.1 on the configured port, or the next free one.
pub async fn bind(preferred: u16) -> Result<(tokio::net::TcpListener, u16)> {
    let mut last = None;
    for port in preferred..preferred.saturating_add(20) {
        match tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).await {
            Ok(l) => return Ok((l, port)),
            Err(e) => last = Some(e),
        }
    }
    Err(last.map(anyhow::Error::from).unwrap_or_else(|| anyhow::anyhow!("no port")))
        .with_context(|| format!("couldn't find a free port from {preferred}"))
}

/// Runs the server until `shutdown` resolves.
pub async fn serve(
    listener: tokio::net::TcpListener,
    state: Shared,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<()> {
    let _watcher = watch::start(state.clone())?;
    axum::serve(listener, router(state)).with_graceful_shutdown(shutdown).await?;
    Ok(())
}

/// Is a Parrot server already answering on this port? (Then we just open the browser.)
pub fn running_on(port: u16) -> bool {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_millis(600)))
        .http_status_as_error(false)
        .build()
        .into();
    agent
        .get(&format!("http://127.0.0.1:{port}/api/health"))
        .header("host", &format!("127.0.0.1:{port}"))
        .call()
        .ok()
        .and_then(|mut r| r.body_mut().read_json::<serde_json::Value>().ok())
        .is_some_and(|v| v["app"] == "parrot")
}

#[cfg(test)]
mod tests;
