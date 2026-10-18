//! `/ws/agent`: the voice assistant, Gemini Live with tools over the notebook.
//!
//! Browser → server: binary frames of 16 kHz mono PCM16 (the microphone), and JSON
//! `{"type":"view","note":..,"topic":..}`, `{"type":"text","text":..}`, `{"type":"stop"}`.
//! Server → browser: binary frames of 24 kHz mono PCM16 (the assistant's voice), and JSON
//! `ready`, `user_text`, `agent_text`, `turn_complete`, `interrupted`, `tool`, `changed`,
//! `status`, `error`, `stopped`.
//!
//! Tool calls run here, against the data folder, never in the browser. When Gemini asks the
//! session to move (`goAway`) or drops it, the server reconnects with the resumption handle so
//! the conversation carries on.

use super::Shared;
use crate::gemini;
use crate::tools::{self, View};
use anyhow::{Result, bail};
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::mpsc;

const MAX_FAILURES: u32 = 4;

enum Out {
    Json(Value),
    Audio(Vec<u8>),
}

enum In {
    Audio(Vec<u8>),
    Text(String),
    View(View),
}

pub async fn upgrade(ws: WebSocketUpgrade, State(state): State<Shared>) -> Response {
    ws.on_upgrade(move |socket| run(socket, state))
}

async fn run(socket: WebSocket, state: Shared) {
    let (mut to_browser, mut from_browser) = socket.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Out>();
    let writer = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            let frame = match msg {
                Out::Json(v) => Message::Text(v.to_string().into()),
                Out::Audio(b) => Message::Binary(b.into()),
            };
            if to_browser.send(frame).await.is_err() {
                break;
            }
        }
        let _ = to_browser.close().await;
    });

    let (in_tx, in_rx) = mpsc::unbounded_channel::<In>();
    let reader = tokio::spawn(async move {
        while let Some(Ok(msg)) = from_browser.next().await {
            let item = match msg {
                Message::Binary(b) => In::Audio(b.to_vec()),
                Message::Text(t) => {
                    let v: Value = serde_json::from_str(&t).unwrap_or(Value::Null);
                    match v["type"].as_str() {
                        Some("stop") => break,
                        Some("text") => In::Text(v["text"].as_str().unwrap_or("").to_string()),
                        Some("view") => In::View(serde_json::from_value(v).unwrap_or_default()),
                        _ => continue,
                    }
                }
                Message::Close(_) => break,
                _ => continue,
            };
            if in_tx.send(item).is_err() {
                break;
            }
        }
    });

    let send = |v: Value| {
        let _ = out_tx.send(Out::Json(v));
    };
    match state.api_key() {
        None => send(json!({ "type": "error", "message": "Add your Gemini API key in Settings first." })),
        Some(key) => {
            state.log("◆ Assistant session started");
            if let Err(e) = sessions(&state, &key, in_rx, &out_tx).await {
                send(json!({ "type": "error", "message": format!("{e:#}") }));
            }
            state.log("◇ Assistant session ended");
        }
    }
    send(json!({ "type": "stopped" }));
    drop(out_tx);
    reader.abort();
    let _ = writer.await;
}

fn system_prompt(state: &Shared) -> String {
    let today = chrono::Local::now().format("%A, %-d %B %Y").to_string();
    format!(
        "You are Parrot, a voice assistant for a team's meeting notebook. Today is {today}.\n\
         \n\
         The notebook is a folder of Markdown files. Topics are folders and can hold subtopics; notes are .md files. \
         Meeting transcripts are notes whose lines look like `[HH:MM:SS] what was said`, mixing Marathi, Hindi and English. \
         Paths are relative to the notebook, e.g. `Sales/Pune/Monday standup.md`.\n\
         \n\
         How to work:\n\
         - Use your tools to find and read notes before answering questions about them. Never invent what a meeting said.\n\
         - When the user says 'this note' or 'this meeting', call get_current_view first.\n\
         - Read a note before editing it. Use edit_note for small changes, append_to_note to add summaries or action items, \
         and replace_in_notes for changes across many notes (run it with dry_run first and tell the user how many places will change).\n\
         - Ask before deleting anything, and before rewriting a whole note.\n\
         - When you write summaries into notes, use clear Markdown headings and bullet lists, in English unless asked otherwise.\n\
         \n\
         How to speak:\n\
         - Answer in the language the user speaks (Marathi, Hindi or English; mixing is fine).\n\
         - Keep spoken answers short and natural. Don't read file paths, symbols or Markdown aloud; say what you found or changed.\n\
         - Quote numbers carefully (sales figures, lakhs and crores) and say which meeting they came from.\n\
         \n\
         The notebook lives at {}.",
        crate::ui::tilde(state.store.root())
    )
}

async fn sessions(state: &Shared, key: &str, mut input: mpsc::UnboundedReceiver<In>, out: &mpsc::UnboundedSender<Out>) -> Result<()> {
    let send = |v: Value| {
        let _ = out.send(Out::Json(v));
    };
    let voice = state.config.read().unwrap().voice.clone();
    let mut handle: Option<String> = None;
    let mut failures = 0;
    let mut first = true;
    loop {
        let mut resumption = json!({});
        if let Some(h) = &handle {
            resumption["handle"] = json!(h);
        }
        let setup = json!({
            "setup": {
                "model": format!("models/{}", gemini::VOICE_MODEL),
                "generationConfig": {
                    "responseModalities": ["AUDIO"],
                    "speechConfig": { "voiceConfig": { "prebuiltVoiceConfig": { "voiceName": voice } } }
                },
                "systemInstruction": { "parts": [{ "text": system_prompt(state) }] },
                "tools": [{ "functionDeclarations": tools::declarations() }],
                "inputAudioTranscription": {},
                "outputAudioTranscription": {},
                "sessionResumption": resumption,
                "contextWindowCompression": { "slidingWindow": {} }
            }
        });
        send(json!({ "type": "status", "state": if first { "connecting" } else { "reconnecting" } }));
        let (mut sink, mut stream) = match gemini::connect(key, &setup).await {
            Ok(c) => c,
            Err(e) => {
                failures += 1;
                if first || failures >= MAX_FAILURES {
                    return Err(e);
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        if first {
            send(json!({ "type": "ready" }));
            first = false;
        }
        send(json!({ "type": "status", "state": "live" }));

        let ended: Result<bool> = async {
            loop {
                tokio::select! {
                    item = input.recv() => {
                        let Some(item) = item else { return Ok(true) };
                        match item {
                            In::Audio(chunk) => {
                                let msg = json!({ "realtimeInput": { "audio": {
                                    "data": base64::engine::general_purpose::STANDARD.encode(&chunk),
                                    "mimeType": "audio/pcm;rate=16000"
                                }}});
                                sink.send(gemini::text(&msg)).await?;
                            }
                            In::Text(text) if !text.trim().is_empty() => {
                                sink.send(gemini::text(&json!({ "realtimeInput": { "text": text } }))).await?;
                                send(json!({ "type": "user_text", "text": text, "typed": true }));
                            }
                            In::Text(_) => {}
                            In::View(view) => *state.view.lock().unwrap() = view,
                        }
                    }
                    msg = stream.next() => {
                        let Some(msg) = msg else { bail!("Gemini disconnected") };
                        let Some(v) = gemini::parse(msg?)? else { continue };
                        if let Some(err) = v.get("error") {
                            bail!("Gemini: {err}");
                        }
                        if let Some(update) = v.get("sessionResumptionUpdate")
                            && update["resumable"].as_bool().unwrap_or(false)
                                && let Some(h) = update["newHandle"].as_str() {
                                    handle = Some(h.to_string());
                                }
                        if v.get("goAway").is_some() {
                            return Ok(false);
                        }
                        if let Some(sc) = v.get("serverContent") {
                            server_content(sc, out);
                        }
                        if let Some(calls) = v.pointer("/toolCall/functionCalls").and_then(Value::as_array) {
                            let mut responses = Vec::new();
                            for call in calls {
                                responses.push(run_tool(state, call, out).await);
                            }
                            let msg = json!({ "toolResponse": { "functionResponses": responses } });
                            sink.send(gemini::text(&msg)).await?;
                        }
                    }
                }
            }
        }
        .await;
        let _ = tokio::time::timeout(Duration::from_millis(300), sink.close()).await;
        match ended {
            Ok(true) => return Ok(()),
            Ok(false) => failures = 0,
            Err(e) => {
                failures += 1;
                if failures >= MAX_FAILURES || handle.is_none() {
                    return Err(e);
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }
}

fn server_content(sc: &Value, out: &mpsc::UnboundedSender<Out>) {
    let send = |v: Value| {
        let _ = out.send(Out::Json(v));
    };
    if let Some(parts) = sc.pointer("/modelTurn/parts").and_then(Value::as_array) {
        for part in parts {
            let Some(data) = part.pointer("/inlineData/data").and_then(Value::as_str) else {
                continue;
            };
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) {
                let _ = out.send(Out::Audio(bytes));
            }
        }
    }
    if let Some(t) = sc.pointer("/inputTranscription/text").and_then(Value::as_str) {
        send(json!({ "type": "user_text", "text": t }));
    }
    if let Some(t) = sc.pointer("/outputTranscription/text").and_then(Value::as_str) {
        send(json!({ "type": "agent_text", "text": t }));
    }
    if sc["interrupted"].as_bool() == Some(true) {
        send(json!({ "type": "interrupted" }));
    }
    if sc["turnComplete"].as_bool() == Some(true) {
        send(json!({ "type": "turn_complete" }));
    }
}

/// Runs one function call off the async threads and returns its `functionResponses` entry.
async fn run_tool(state: &Shared, call: &Value, out: &mpsc::UnboundedSender<Out>) -> Value {
    let id = call["id"].as_str().unwrap_or("").to_string();
    let name = call["name"].as_str().unwrap_or("").to_string();
    let args = call.get("args").cloned().unwrap_or(json!({}));
    let _ = out.send(Out::Json(
        json!({ "type": "tool", "id": id, "name": name, "status": "running", "args": args }),
    ));
    let view = state.view.lock().unwrap().clone();
    let st = state.clone();
    let (n, a) = (name.clone(), args.clone());
    let outcome = tokio::task::spawn_blocking(move || tools::execute(&st.store, &view, &n, &a)).await;
    let outcome = match outcome {
        Ok(o) => o,
        Err(e) => tools::Outcome {
            ok: false,
            response: json!({ "error": e.to_string() }),
            summary: format!("{name} crashed"),
            changed: Vec::new(),
        },
    };
    state.log(format!("  ↳ {}", outcome.summary));
    let _ = out.send(Out::Json(json!({
        "type": "tool", "id": id, "name": name,
        "status": if outcome.ok { "done" } else { "error" },
        "summary": outcome.summary,
    })));
    if !outcome.changed.is_empty() {
        let _ = out.send(Out::Json(json!({ "type": "changed", "paths": outcome.changed })));
    }
    json!({ "id": id, "name": name, "response": outcome.response })
}
