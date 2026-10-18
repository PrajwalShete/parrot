//! Talking to Gemini: checking a key over REST, and opening Live API WebSockets.

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use futures_util::stream::{SplitSink, SplitStream};
use serde_json::Value;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

pub const TRANSCRIBE_MODEL: &str = "gemini-3.5-transcribe-live";
pub const VOICE_MODEL: &str = "gemini-3.8-live";
pub const AI_STUDIO_KEYS: &str = "https://aistudio.google.com/app/apikey";

const API: &str = "https://generativelanguage.googleapis.com/v1beta";
const LIVE: &str = "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";

/// The prebuilt voices the assistant can use.
pub const VOICES: [&str; 8] = ["Kore", "Puck", "Charon", "Aoede", "Fenrir", "Leda", "Orus", "Zephyr"];

pub type Sink = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, Message>;
pub type Stream = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

/// Why a key was rejected, in words for a person.
#[derive(Debug, PartialEq)]
pub enum KeyCheck {
    Valid,
    Invalid(String),
    /// Couldn't tell (offline, Google unreachable): accept it and let the app retry later.
    Unknown(String),
}

/// Checks a key by listing one model. Blocking; used during setup.
pub fn check_key(key: &str) -> KeyCheck {
    let key = key.trim();
    if key.is_empty() {
        return KeyCheck::Invalid("The key is empty.".into());
    }
    if key.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return KeyCheck::Invalid("The key has spaces in it; copy it again from AI Studio.".into());
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(12)))
        .http_status_as_error(false)
        .user_agent(format!("parrot/{}", crate::VERSION))
        .build()
        .into();
    let resp = agent.get(&format!("{API}/models?pageSize=1")).header("x-goog-api-key", key).call();
    match resp {
        Ok(mut r) => {
            let status = r.status().as_u16();
            if status == 200 {
                return KeyCheck::Valid;
            }
            let body: Value = r.body_mut().read_json().unwrap_or(Value::Null);
            let msg = body.pointer("/error/message").and_then(Value::as_str).unwrap_or("").to_string();
            match status {
                400 | 401 | 403 => KeyCheck::Invalid(if msg.is_empty() {
                    "Google didn't accept this key.".into()
                } else {
                    msg
                }),
                429 => KeyCheck::Valid,
                _ => KeyCheck::Unknown(format!("Google answered {status}. {msg}")),
            }
        }
        Err(e) => KeyCheck::Unknown(format!("Couldn't reach Google ({e}).")),
    }
}

/// Opens a Live API session: connects, sends `setup`, and waits for `setupComplete`.
pub async fn connect(key: &str, setup: &Value) -> Result<(Sink, Stream)> {
    use futures_util::SinkExt;
    let url = format!("{LIVE}?key={key}");
    let (ws, _) = tokio::time::timeout(Duration::from_secs(15), connect_async(url))
        .await
        .context("timed out connecting to Gemini")?
        .context("couldn't connect to Gemini")?;
    let (mut sink, mut stream) = ws.split();
    sink.send(Message::Text(setup.to_string().into())).await?;
    let ready = tokio::time::timeout(Duration::from_secs(15), async {
        while let Some(msg) = stream.next().await {
            match parse(msg?)? {
                Some(v) if v.get("setupComplete").is_some() => return Ok(()),
                Some(v) if v.get("error").is_some() => bail!("Gemini: {}", v["error"]),
                _ => {}
            }
        }
        bail!("Gemini closed the connection during setup")
    })
    .await;
    match ready {
        Ok(r) => r?,
        Err(_) => bail!("Gemini didn't finish setting up in time"),
    }
    Ok((sink, stream))
}

/// A server frame as JSON. `Ok(None)` for pings and other non-data frames; an error on close,
/// with Google's reason (it explains bad keys, unknown models and quota problems).
pub fn parse(msg: Message) -> Result<Option<Value>> {
    let text = match msg {
        Message::Text(t) => t.to_string(),
        Message::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
        Message::Close(frame) => {
            let reason = frame.map(|f| format!("{} {}", u16::from(f.code), f.reason)).unwrap_or_default();
            bail!("Gemini closed the session: {}", friendly_close(&reason));
        }
        _ => return Ok(None),
    };
    Ok(serde_json::from_str(&text).ok())
}

fn friendly_close(reason: &str) -> String {
    let lower = reason.to_lowercase();
    if lower.contains("api key") || lower.contains("permission") || lower.contains("unauthenticated") {
        format!("{reason} — check your API key in Settings")
    } else if lower.contains("quota") || lower.contains("exhausted") || lower.contains("rate") {
        format!("{reason} — you've hit a usage limit; try again in a minute")
    } else {
        reason.trim().to_string()
    }
}

pub fn text(msg: &Value) -> Message {
    Message::Text(msg.to_string().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_obviously_bad_keys_offline() {
        assert!(matches!(check_key(""), KeyCheck::Invalid(_)));
        assert!(matches!(check_key("abc def"), KeyCheck::Invalid(_)));
    }

    #[test]
    fn close_reasons_are_friendly() {
        let err = parse(Message::Close(Some(tokio_tungstenite::tungstenite::protocol::CloseFrame {
            code: 1008u16.into(),
            reason: "API key not valid".into(),
        })))
        .unwrap_err();
        assert!(err.to_string().contains("check your API key"));
    }
}
