//! Talking to Gemini: checking a key over REST, and opening Live API WebSockets.

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use futures_util::stream::{FuturesUnordered, SplitSink, SplitStream};
use serde_json::Value;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, client_async_tls, tungstenite::Message};

pub const TRANSCRIBE_MODEL: &str = "gemini-3.5-transcribe-live";
pub const VOICE_MODEL: &str = "gemini-3.8-live";
pub const AI_STUDIO_KEYS: &str = "https://aistudio.google.com/app/apikey";

const HOST: &str = "generativelanguage.googleapis.com";
const API: &str = "https://generativelanguage.googleapis.com/v1beta";
const LIVE: &str = "wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent";

/// How long one address gets before the next one is tried alongside it (RFC 8305 "Happy Eyeballs").
const NEXT_ADDRESS_AFTER: Duration = Duration::from_millis(250);

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
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(12)))
        // ureq tries addresses one by one, sharing this between them, so a dead IPv6 route
        // costs a few seconds instead of the whole budget.
        .timeout_connect(Some(Duration::from_secs(6)))
        .http_status_as_error(false)
        .user_agent(format!("parrot/{}", crate::VERSION))
        .build();
    let agent = ureq::Agent::with_parts(
        config,
        ureq::unversioned::transport::DefaultConnector::new(),
        Interleaved::default(),
    );
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
    // The key goes in a header rather than the URL, so it can't leak into logs or error messages.
    let mut request = LIVE.into_client_request()?;
    let key = HeaderValue::from_str(key.trim()).context("the API key has characters a key can't have")?;
    request.headers_mut().insert("x-goog-api-key", key);
    let (ws, _) = tokio::time::timeout(Duration::from_secs(15), async {
        let tcp = connect_tcp(HOST, 443).await?;
        anyhow::Ok(client_async_tls(request, tcp).await?)
    })
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

/// Opens a TCP connection, racing the host's addresses the way browsers do (RFC 8305): one
/// address at a time, starting the next if the last hasn't answered within
/// `NEXT_ADDRESS_AFTER`, alternating IPv6 and IPv4. A network that advertises IPv6 but drops it
/// then costs a quarter of a second instead of a timeout. Linux builds are static musl, which
/// ignores `/etc/gai.conf` and always lists IPv6 first, so this can't be left to the system.
async fn connect_tcp(host: &str, port: u16) -> std::io::Result<TcpStream> {
    race(interleave(tokio::net::lookup_host((host, port)).await?.collect())).await
}

/// Connects to the first of `addrs` that answers, giving each a head start of `NEXT_ADDRESS_AFTER`.
async fn race(addrs: Vec<SocketAddr>) -> std::io::Result<TcpStream> {
    let mut pending = addrs.into_iter();
    let mut attempts = FuturesUnordered::new();
    let mut last_err = None;
    loop {
        if let Some(addr) = pending.next() {
            attempts.push(TcpStream::connect(addr));
        }
        tokio::select! {
            Some(result) = attempts.next() => match result {
                Ok(tcp) => {
                    tcp.set_nodelay(true)?;
                    return Ok(tcp);
                }
                Err(e) => last_err = Some(e),
            },
            _ = tokio::time::sleep(NEXT_ADDRESS_AFTER), if pending.len() > 0 => {}
            else => break,
        }
    }
    Err(last_err.unwrap_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no addresses to connect to")))
}

/// Alternates address families, keeping the system's order within each and starting with the
/// family it listed first, so one broken family can't hold up the other.
fn interleave(addrs: Vec<SocketAddr>) -> Vec<SocketAddr> {
    let first_v6 = addrs.first().is_some_and(SocketAddr::is_ipv6);
    let mut out = Vec::with_capacity(addrs.len());
    let (a, b): (Vec<_>, Vec<_>) = addrs.into_iter().partition(|a| a.is_ipv6() == first_v6);
    let (mut a, mut b) = (a.into_iter(), b.into_iter());
    loop {
        match (a.next(), b.next()) {
            (None, None) => return out,
            (x, y) => out.extend(x.into_iter().chain(y)),
        }
    }
}

/// ureq's resolver with `interleave` applied, for the same reason as `connect_tcp`.
#[derive(Debug, Default)]
struct Interleaved(ureq::unversioned::resolver::DefaultResolver);

impl ureq::unversioned::resolver::Resolver for Interleaved {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<ureq::unversioned::resolver::ResolvedSocketAddrs, ureq::Error> {
        let found = self.0.resolve(uri, config, timeout)?;
        let mut out = self.empty();
        for addr in interleave(found.iter().copied().collect()) {
            out.push(addr);
        }
        Ok(out)
    }
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
    fn interleave_alternates_families() {
        let a = |s: &str| s.parse::<SocketAddr>().unwrap();
        let addrs = vec![
            a("[2001:db8::1]:443"),
            a("[2001:db8::2]:443"),
            a("[2001:db8::3]:443"),
            a("10.0.0.1:443"),
            a("10.0.0.2:443"),
        ];
        let want = vec![
            a("[2001:db8::1]:443"),
            a("10.0.0.1:443"),
            a("[2001:db8::2]:443"),
            a("10.0.0.2:443"),
            a("[2001:db8::3]:443"),
        ];
        assert_eq!(interleave(addrs), want);
        assert_eq!(
            interleave(vec![a("10.0.0.1:443"), a("[2001:db8::1]:443")]),
            vec![a("10.0.0.1:443"), a("[2001:db8::1]:443")]
        );
        assert!(interleave(vec![]).is_empty());
    }

    #[tokio::test]
    async fn race_skips_an_address_that_never_answers() {
        // 192.0.2.1 (TEST-NET-1) is unroutable, like a dropped IPv6 route: the connection must
        // still come through on the next address well before a timeout.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let live = listener.local_addr().unwrap();
        let dead: SocketAddr = format!("192.0.2.1:{}", live.port()).parse().unwrap();
        let start = std::time::Instant::now();
        let tcp = race(vec![dead, live]).await.unwrap();
        assert_eq!(tcp.peer_addr().unwrap(), live);
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn race_reports_the_error_when_nothing_answers() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let closed = listener.local_addr().unwrap();
        drop(listener);
        assert!(race(vec![closed]).await.is_err());
        assert!(race(vec![]).await.is_err());
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
