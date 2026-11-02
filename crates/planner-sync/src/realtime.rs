//! Supabase Realtime (Phoenix channels over a websocket). Any change to the
//! user's rows calls `on_change`, which triggers a pull; the payloads
//! themselves aren't used, so a missed event is caught by the next pull.

use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use planner_core::sync::TABLES;
use serde_json::{json, Value};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::client::Client;

const HEARTBEAT: Duration = Duration::from_secs(25);
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// Reconnect with a fresh token this long before the current one expires.
const TOKEN_MARGIN_SECS: i64 = 120;

/// Listen forever, reconnecting with backoff. Run on its own thread.
pub fn run(client: Arc<Mutex<Client>>, on_change: impl Fn()) {
    let mut backoff = Duration::from_secs(5);
    let mut last_err = String::new();
    loop {
        let started = Instant::now();
        match listen(&client, &on_change) {
            Ok(()) => last_err.clear(),
            Err(e) => {
                let msg = format!("{e:#}");
                if msg != last_err {
                    eprintln!("plannerd: realtime: {msg}");
                    last_err = msg;
                }
            }
        }
        if started.elapsed() > Duration::from_secs(60) {
            backoff = Duration::from_secs(5);
        }
        thread::sleep(backoff);
        backoff = (backoff * 2).min(Duration::from_secs(300));
    }
}

fn set_read_timeout(ws: &WebSocket<MaybeTlsStream<TcpStream>>) {
    let sock: Option<&TcpStream> = match ws.get_ref() {
        MaybeTlsStream::Plain(s) => Some(s),
        MaybeTlsStream::Rustls(s) => Some(s.get_ref()),
        _ => None,
    };
    if let Some(s) = sock {
        let _ = s.set_read_timeout(Some(READ_TIMEOUT));
    }
}

fn send(ws: &mut WebSocket<MaybeTlsStream<TcpStream>>, v: Value) -> Result<()> {
    ws.send(Message::Text(v.to_string()))?;
    Ok(())
}

/// One connection, until the token needs renewing (Ok) or something fails.
fn listen(client: &Arc<Mutex<Client>>, on_change: &impl Fn()) -> Result<()> {
    let (url, key, token, expires_at) = {
        let mut c = client.lock().map_err(|_| anyhow!("client lock poisoned"))?;
        let token = c.token_valid_for(TOKEN_MARGIN_SECS * 3)?;
        (c.cfg.url.clone(), c.cfg.anon_key.clone(), token, c.session.expires_at)
    };
    let ws_url = format!(
        "{}/realtime/v1/websocket?apikey={key}&vsn=1.0.0",
        url.replacen("https://", "wss://", 1).replacen("http://", "ws://", 1)
    );
    let (mut ws, _) = tungstenite::connect(ws_url.as_str())?;
    set_read_timeout(&ws);

    let changes: Vec<Value> =
        TABLES.iter().map(|t| json!({ "event": "*", "schema": "public", "table": t.name })).collect();
    send(
        &mut ws,
        json!({
            "topic": "realtime:timlo",
            "event": "phx_join",
            "payload": {
                "config": { "broadcast": { "self": false }, "presence": { "key": "" }, "postgres_changes": changes },
                "access_token": token,
            },
            "ref": "1",
            "join_ref": "1",
        }),
    )?;
    // Catch up on anything that happened while disconnected.
    on_change();

    let mut last_beat = Instant::now();
    let mut next_ref: u64 = 2;
    loop {
        if chrono::Utc::now().timestamp() + TOKEN_MARGIN_SECS >= expires_at {
            let _ = ws.close(None);
            return Ok(());
        }
        if last_beat.elapsed() >= HEARTBEAT {
            send(&mut ws, json!({ "topic": "phoenix", "event": "heartbeat", "payload": {}, "ref": next_ref.to_string() }))?;
            next_ref += 1;
            last_beat = Instant::now();
        }
        match ws.read() {
            Ok(Message::Text(text)) => {
                let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                match v.get("event").and_then(Value::as_str) {
                    Some("postgres_changes") => on_change(),
                    Some("phx_reply") if v["payload"]["status"] == "error" => {
                        bail!("join refused: {}", v["payload"]["response"])
                    }
                    Some("phx_error") | Some("phx_close") => bail!("channel closed by the server"),
                    Some("system") if v["payload"]["status"] == "error" => {
                        bail!("realtime error: {}", v["payload"]["message"])
                    }
                    _ => {}
                }
            }
            Ok(Message::Close(_)) => bail!("server closed the connection"),
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => return Err(e.into()),
        }
    }
}
