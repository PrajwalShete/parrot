//! End-to-end check against a running Parrot and the real Gemini API.
//!
//!   cargo run --example live_check -- <port> transcribe <note-path> <speech.wav>
//!   cargo run --example live_check -- <port> agent <speech.wav>
//!
//! Streams a 16 kHz mono 16-bit WAV in real time (100 ms chunks), then silence, and prints every
//! event the server sends back. Make the WAV with:
//!   say -o /tmp/s.aiff "Hello" && afconvert -f WAVE -d LEI16@16000 -c 1 /tmp/s.aiff /tmp/s.wav

use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

fn pcm_from_wav(path: &str) -> Vec<u8> {
    let bytes = std::fs::read(path).expect("reading wav");
    let mut i = 12;
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let len = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().unwrap()) as usize;
        if id == b"data" {
            return bytes[i + 8..(i + 8 + len).min(bytes.len())].to_vec();
        }
        i += 8 + len + (len & 1);
    }
    panic!("no data chunk in {path}");
}

#[tokio::main]
async fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let args: Vec<String> = std::env::args().collect();
    let port = &args[1];
    let (url, wav) = match args[2].as_str() {
        "transcribe" => (
            format!("ws://127.0.0.1:{port}/ws/transcribe?path={}", args[3].replace(' ', "%20")),
            args[4].clone(),
        ),
        _ => (format!("ws://127.0.0.1:{port}/ws/agent"), args[3].clone()),
    };
    let mut req = url.into_client_request().unwrap();
    req.headers_mut()
        .insert("origin", format!("http://127.0.0.1:{port}").parse().unwrap());
    let (ws, _) = tokio_tungstenite::connect_async(req).await.expect("connect");
    let (mut tx, mut rx) = ws.split();

    let pcm = pcm_from_wav(&wav);
    let mut audio_bytes = 0usize;
    let reader = tokio::spawn(async move {
        while let Some(Ok(msg)) = rx.next().await {
            match msg {
                Message::Text(t) => {
                    println!("← {t}");
                    if t.contains("\"stopped\"") {
                        break;
                    }
                }
                Message::Binary(b) => {
                    audio_bytes += b.len();
                    if audio_bytes == b.len() {
                        println!("← (assistant audio started)");
                    }
                }
                _ => {}
            }
        }
        println!("== assistant audio: {:.1} s at 24 kHz", audio_bytes as f64 / 48_000.0);
    });

    tokio::time::sleep(Duration::from_secs(3)).await;
    if args[2] == "agent" {
        tx.send(Message::Text(
            r#"{"type":"view","note":"Sales/Monday standup.md","topic":"Sales"}"#.into(),
        ))
        .await
        .unwrap();
    }
    for chunk in pcm.chunks(3200) {
        tx.send(Message::Binary(chunk.to_vec().into())).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // Silence so voice activity detection sees the end of the turn, and time for the answer.
    let secs = if args[2] == "agent" { 25 } else { 4 };
    for _ in 0..secs * 10 {
        tx.send(Message::Binary(vec![0u8; 3200].into())).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    tx.send(Message::Text(r#"{"type":"stop"}"#.into())).await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(8), reader).await;
}
