//! The JSON API over the notebook and the settings.

use super::{Event, Shared};
use crate::config;
use crate::gemini::{self, KeyCheck};
use crate::store::StoreError;
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::sse::{KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::Stream;
use serde::Deserialize;
use serde_json::{Value, json};
use std::convert::Infallible;
use std::time::Duration;

/// An API error: a status and a message for the person.
pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        match e.downcast_ref::<StoreError>() {
            Some(StoreError::NotFound(m)) => ApiError(StatusCode::NOT_FOUND, m.clone()),
            Some(StoreError::Invalid(m)) => ApiError(StatusCode::BAD_REQUEST, m.clone()),
            Some(StoreError::Conflict(m)) => ApiError(StatusCode::CONFLICT, m.clone()),
            None => ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
        }
    }
}

type ApiResult<T = Json<Value>> = Result<T, ApiError>;

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

pub async fn health() -> Json<Value> {
    Json(json!({ "app": "parrot", "version": crate::VERSION }))
}

pub async fn app_state(State(s): State<Shared>) -> Json<Value> {
    let cfg = s.config.read().unwrap().clone();
    Json(json!({
        "version": crate::VERSION,
        "data_dir": cfg.data_dir,
        "data_dir_display": crate::ui::tilde(&cfg.data_dir),
        "has_key": cfg.is_ready(),
        "masked_key": cfg.masked_key(),
        "voice": cfg.voice,
        "voices": gemini::VOICES,
        "language_codes": cfg.language_codes,
        "models": { "transcribe": gemini::TRANSCRIBE_MODEL, "voice": gemini::VOICE_MODEL },
        "keys_url": gemini::AI_STUDIO_KEYS,
    }))
}

#[derive(Deserialize)]
pub struct Settings {
    api_key: Option<String>,
    voice: Option<String>,
    language_codes: Option<Vec<String>>,
}

pub async fn update_settings(State(s): State<Shared>, Json(body): Json<Settings>) -> ApiResult {
    let mut next = s.config.read().unwrap().clone();
    if let Some(key) = body.api_key {
        let key = key.trim().to_string();
        let check = {
            let key = key.clone();
            tokio::task::spawn_blocking(move || gemini::check_key(&key))
                .await
                .map_err(|e| bad(e.to_string()))?
        };
        if let KeyCheck::Invalid(why) = check {
            return Err(bad(why));
        }
        next.api_key = Some(key);
    }
    if let Some(voice) = body.voice {
        if !gemini::VOICES.contains(&voice.as_str()) {
            return Err(bad(format!("unknown voice {voice}")));
        }
        next.voice = voice;
    }
    if let Some(codes) = body.language_codes {
        let codes: Vec<String> = codes.into_iter().map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect();
        if codes
            .iter()
            .any(|c| c.len() > 12 || !c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-'))
        {
            return Err(bad("language codes look like mr-IN, hi-IN, en-IN"));
        }
        next.language_codes = codes;
    }
    next.save_to(&config::path()).map_err(ApiError::from)?;
    *s.config.write().unwrap() = next;
    s.log("Settings updated");
    Ok(app_state(State(s)).await)
}

pub async fn tree(State(s): State<Shared>) -> ApiResult {
    let tree = s.store.tree()?;
    Ok(Json(json!({ "tree": tree })))
}

#[derive(Deserialize)]
pub struct PathQuery {
    path: String,
}

pub async fn read_note(State(s): State<Shared>, Query(q): Query<PathQuery>) -> ApiResult {
    let doc = s.store.read(&q.path)?;
    Ok(Json(serde_json::to_value(doc).unwrap()))
}

#[derive(Deserialize)]
pub struct WriteNote {
    path: String,
    content: String,
    expected_modified: Option<i64>,
}

pub async fn write_note(State(s): State<Shared>, Json(b): Json<WriteNote>) -> ApiResult {
    let doc = s.store.write(&b.path, &b.content, b.expected_modified)?;
    Ok(Json(serde_json::to_value(doc).unwrap()))
}

#[derive(Deserialize)]
pub struct CreateNote {
    #[serde(default)]
    parent: String,
    title: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    content: Option<String>,
}

pub async fn create_note(State(s): State<Shared>, Json(b): Json<CreateNote>) -> ApiResult {
    let content = match (b.content, b.kind.as_deref()) {
        (Some(c), _) => c,
        (None, Some("transcript")) => transcript_header(),
        (None, _) => String::new(),
    };
    let path = s.store.create_note(&b.parent, &b.title, &content)?;
    s.log(format!("Created {path}"));
    Ok(Json(json!({ "path": path })))
}

/// The front matter a new recording starts with.
pub fn transcript_header() -> String {
    let now = chrono::Local::now();
    format!(
        "---\ntype: transcript\ncreated: {}\n---\n\n",
        now.to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
    )
}

#[derive(Deserialize)]
pub struct CreateTopic {
    #[serde(default)]
    parent: String,
    name: String,
}

pub async fn create_topic(State(s): State<Shared>, Json(b): Json<CreateTopic>) -> ApiResult {
    let path = s.store.create_topic(&b.parent, &b.name)?;
    s.log(format!("Created topic {path}"));
    Ok(Json(json!({ "path": path })))
}

#[derive(Deserialize)]
pub struct Rename {
    path: String,
    name: String,
}

pub async fn rename(State(s): State<Shared>, Json(b): Json<Rename>) -> ApiResult {
    let path = s.store.rename(&b.path, &b.name)?;
    Ok(Json(json!({ "path": path })))
}

#[derive(Deserialize)]
pub struct Move {
    path: String,
    #[serde(default)]
    to: String,
}

pub async fn move_item(State(s): State<Shared>, Json(b): Json<Move>) -> ApiResult {
    let path = s.store.move_to(&b.path, &b.to)?;
    Ok(Json(json!({ "path": path })))
}

pub async fn delete(State(s): State<Shared>, Json(b): Json<PathQuery>) -> ApiResult {
    let entry = s.store.delete(&b.path)?;
    s.log(format!("Moved {} to the trash", entry.path));
    Ok(Json(serde_json::to_value(entry).unwrap()))
}

#[derive(Deserialize)]
pub struct Restore {
    id: String,
}

pub async fn restore(State(s): State<Shared>, Json(b): Json<Restore>) -> ApiResult {
    let path = s.store.restore(&b.id)?;
    Ok(Json(json!({ "path": path })))
}

#[derive(Deserialize)]
pub struct Reveal {
    #[serde(default)]
    path: String,
}

/// Opens the folder (or the folder holding a note) in Finder / the file manager.
pub async fn reveal(State(s): State<Shared>, Json(b): Json<Reveal>) -> ApiResult {
    let target = s.store.resolve(&b.path)?;
    let dir = if target.is_dir() {
        target
    } else {
        target.parent().map(|p| p.to_path_buf()).unwrap_or(target)
    };
    open::that_detached(&dir).map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

/// Server-sent events: file changes, so every open tab stays current.
pub async fn events(State(s): State<Shared>) -> Sse<impl Stream<Item = Result<axum::response::sse::Event, Infallible>>> {
    let mut rx = s.events.subscribe();
    let stream = async_stream(move |tx| async move {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let data = serde_json::to_string(&ev).unwrap_or_default();
                    if tx.send(axum::response::sse::Event::default().data(data)).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    let ev = Event::Changed {
                        paths: vec![String::new()],
                    };
                    let _ = tx
                        .send(axum::response::sse::Event::default().data(serde_json::to_string(&ev).unwrap()))
                        .await;
                }
                Err(_) => break,
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

/// A tiny channel-backed stream, to avoid pulling in a stream macro crate.
fn async_stream<F, Fut, T>(f: F) -> impl Stream<Item = Result<T, Infallible>>
where
    F: FnOnce(tokio::sync::mpsc::Sender<T>) -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = tokio::sync::mpsc::channel(32);
    tokio::spawn(f(tx));
    futures_util::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|v| (Ok(v), rx)) })
}
