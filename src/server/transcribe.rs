//! `/ws/transcribe?path=<note>`: live transcription into a note.
//!
//! The browser sends 16 kHz mono PCM16 in binary frames (about 100 ms each) and `{"type":"stop"}`
//! when done. The server relays the audio to Gemini Transcribe Live, appends each finished
//! sentence to the note as `[HH:MM:SS] text`, and sends the browser what to show:
//! `ready`, `interim`, `final`, `status`, `error` and finally `stopped`.
//!
//! Gemini caps a live session at 10 minutes, so the server rotates to a fresh session a little
//! before that. Audio that arrives meanwhile waits in a queue, so nothing is lost.

use super::Shared;
use crate::gemini;
use crate::history::{Author, Change};
use anyhow::{Result, bail};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::Response;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::{mpsc, watch};

const SESSION_LIMIT: Duration = Duration::from_secs(9 * 60 + 30);
const MAX_FAILURES: u32 = 5;

#[derive(Deserialize)]
pub struct Params {
    path: String,
}

pub async fn upgrade(ws: WebSocketUpgrade, State(state): State<Shared>, Query(p): Query<Params>) -> Response {
    ws.on_upgrade(move |socket| run(socket, state, p.path))
}

async fn run(socket: WebSocket, state: Shared, path: String) {
    let (mut to_browser, mut from_browser) = socket.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Value>();
    let writer = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if to_browser.send(Message::Text(msg.to_string().into())).await.is_err() {
                break;
            }
        }
        let _ = to_browser.close().await;
    });

    let ready = match (state.api_key(), state.store.read(&path)) {
        (None, _) => Err("Add your Gemini API key in Settings first.".to_string()),
        (_, Err(e)) => Err(format!("{e:#}")),
        (Some(key), Ok(_)) => Ok(key),
    };
    let key = match ready {
        Ok(key) => key,
        Err(msg) => {
            let _ = out_tx.send(json!({ "type": "error", "message": msg }));
            let _ = out_tx.send(json!({ "type": "stopped" }));
            drop(out_tx);
            let _ = writer.await;
            return;
        }
    };

    let (audio_tx, audio_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (quit_tx, quit_rx) = watch::channel(false);
    let reader = tokio::spawn(async move {
        while let Some(Ok(msg)) = from_browser.next().await {
            match msg {
                Message::Binary(b) => {
                    let _ = audio_tx.send(b.to_vec());
                }
                Message::Text(t) if t.contains("\"stop\"") => break,
                Message::Close(_) => break,
                _ => {}
            }
        }
        let _ = quit_tx.send(true);
    });

    state.log(format!("● Recording into {path}"));
    // Keep what the note held before this recording (also catches edits made outside Parrot).
    let _ = state.store.baseline(&path);
    let lines_before = state.store.read(&path).map(|d| d.content.lines().count()).unwrap_or(0);
    let langs = state.config.read().unwrap().language_codes.clone();
    let result = sessions(&state, &key, &path, &langs, audio_rx, quit_rx, &out_tx).await;
    if let Err(e) = result {
        let _ = out_tx.send(json!({ "type": "error", "message": format!("{e:#}") }));
    }
    // One version per recording, not one per sentence.
    let added = state
        .store
        .read(&path)
        .map(|d| d.content.lines().count().saturating_sub(lines_before))
        .unwrap_or(0);
    if added > 0 {
        let summary = format!("Recorded {added} line{}", if added == 1 { "" } else { "s" });
        let _ = state.store.checkpoint(&path, Change::new(Author::Recording, &summary));
    }
    let _ = out_tx.send(json!({ "type": "stopped" }));
    state.log(format!("■ Stopped recording {path}"));
    drop(out_tx);
    reader.abort();
    let _ = writer.await;
}

async fn sessions(
    state: &Shared,
    key: &str,
    path: &str,
    langs: &[String],
    mut audio: mpsc::UnboundedReceiver<Vec<u8>>,
    mut quit: watch::Receiver<bool>,
    out: &mpsc::UnboundedSender<Value>,
) -> Result<()> {
    let setup = json!({
        "setup": {
            "model": format!("models/{}", gemini::TRANSCRIBE_MODEL),
            "generationConfig": { "responseModalities": ["TEXT"] },
            "inputAudioTranscription": { "languageCodes": langs, "mode": "SMART" }
        }
    });
    let mut failures = 0;
    let mut first = true;
    loop {
        if *quit.borrow() {
            return Ok(());
        }
        let _ = out.send(json!({ "type": "status", "state": if first { "connecting" } else { "reconnecting" } }));
        let (mut sink, mut stream) = match gemini::connect(key, &setup).await {
            Ok(c) => c,
            Err(e) => {
                failures += 1;
                if failures >= MAX_FAILURES || first {
                    bail!("{e:#}");
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };
        if first {
            let _ = out.send(json!({ "type": "ready" }));
            first = false;
        }
        let _ = out.send(json!({ "type": "status", "state": "listening" }));

        let deadline = tokio::time::sleep(SESSION_LIMIT);
        tokio::pin!(deadline);
        let ended: Result<bool> = async {
            loop {
                tokio::select! {
                    chunk = audio.recv() => {
                        let Some(chunk) = chunk else { return Ok(true) };
                        let msg = json!({ "realtimeInput": { "audio": {
                            "data": base64::engine::general_purpose::STANDARD.encode(&chunk),
                            "mimeType": "audio/pcm;rate=16000"
                        }}});
                        sink.send(gemini::text(&msg)).await?;
                    }
                    msg = stream.next() => {
                        let Some(msg) = msg else { bail!("Gemini disconnected") };
                        if let Some(v) = gemini::parse(msg?)? {
                            if v.get("goAway").is_some() {
                                return Ok(false);
                            }
                            handle(state, path, &v, out)?;
                        }
                    }
                    _ = &mut deadline => return Ok(false),
                    _ = quit.changed() => return Ok(true),
                }
            }
        }
        .await;

        // Flush: end the stream and collect the last finished sentences.
        let _ = sink
            .send(gemini::text(&json!({ "realtimeInput": { "audioStreamEnd": true } })))
            .await;
        let _ = tokio::time::timeout(Duration::from_millis(1500), async {
            while let Some(Ok(msg)) = stream.next().await {
                match gemini::parse(msg) {
                    Ok(Some(v)) => {
                        let _ = handle(state, path, &v, out);
                    }
                    Ok(None) => {}
                    Err(_) => break,
                }
            }
        })
        .await;
        let _ = tokio::time::timeout(Duration::from_millis(300), sink.close()).await;

        match ended {
            Ok(true) => return Ok(()),
            Ok(false) => failures = 0,
            Err(e) => {
                failures += 1;
                if failures >= MAX_FAILURES {
                    return Err(e);
                }
                let _ = out.send(json!({ "type": "status", "state": "reconnecting", "detail": format!("{e:#}") }));
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

fn handle(state: &Shared, path: &str, v: &Value, out: &mpsc::UnboundedSender<Value>) -> Result<()> {
    if let Some(err) = v.get("error") {
        bail!("Gemini: {err}");
    }
    let Some(sc) = v.get("serverContent") else { return Ok(()) };
    if let Some(text) = sc.pointer("/inputTranscription/text").and_then(Value::as_str) {
        let text = text.trim();
        if text.is_empty() {
            return Ok(());
        }
        let time = chrono::Local::now().format("%H:%M:%S").to_string();
        state.store.append(path, &format!("[{time}] {}\n", one_line(text)))?;
        let _ = out.send(json!({ "type": "final", "time": time, "text": text }));
    } else if let Some(text) = sc.pointer("/interimInputTranscription/text").and_then(Value::as_str) {
        let _ = out.send(json!({ "type": "interim", "text": text.trim() }));
    }
    Ok(())
}

/// One transcript line per sentence, so the file stays easy to grep and edit.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
