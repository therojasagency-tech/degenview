//! Local authenticated WebSocket, backed by OpenChart's native live client.
use super::LiveInbox;
use crate::model::Timeframe;
use crate::service::market::openchart::{ADJUSTMENT, Bar, decode_bar, resolution};
use crate::service::net::{url::encode_component, ws::Backoff};
use eframe::egui;
use serde::Deserialize;
use std::io::ErrorKind;
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::{Duration, Instant};
use tungstenite::{Message, client::IntoClientRequest};

#[derive(Deserialize)]
struct Event {
    #[serde(rename = "type")]
    kind: String,
    symbol: String,
    resolution: String,
    adjustment: String,
    #[serde(default)]
    bars: Vec<Bar>,
}

pub(super) fn run(symbol: String, base: Timeframe, inbox: Arc<LiveInbox>, shutdown: Arc<AtomicBool>, ctx: egui::Context) {
    let Ok(interval) = resolution(base) else { return };
    let mut backoff = Backoff::new();
    while !shutdown.load(Ordering::Acquire) {
        let Ok((port, token)) = crate::service::market::openchart_manager::endpoint() else {
            backoff.wait(&shutdown);
            continue;
        };
        let addr = SocketAddr::from(([127,0,0,1],port));
        let url = format!("ws://127.0.0.1:{port}/v1/live?symbol={}&resolution={interval}&adjustment={ADJUSTMENT}", encode_component(&symbol));
        if let Ok(mut request) = url.as_str().into_client_request() {
            if let Ok(header) = format!("Bearer {token}").parse() {
                request.headers_mut().insert("authorization", header);
                // Never use the system proxy for the loopback URL or local bearer token.
                if let Ok(stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
                    let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
                    if let Ok((mut socket, _)) = tungstenite::client(request, stream) {
                        let mut last_activity = Instant::now();
                        while !shutdown.load(Ordering::Acquire) {
                            match socket.read() {
                                Ok(Message::Text(text)) => {
                                    let Ok(event) = serde_json::from_str::<Event>(text.as_str()) else { break };
                                    if event.symbol != symbol || event.resolution != interval || event.adjustment != ADJUSTMENT { break; }
                                    last_activity = Instant::now();
                                    backoff.reset();
                                    if event.kind == "bars" {
                                        for bar in event.bars {
                                            if let Ok(kline) = decode_bar(bar, base) {
                                                inbox.push(symbol.clone(), kline);
                                            }
                                        }
                                        ctx.request_repaint();
                                    }
                                }
                                Ok(Message::Ping(_)) => { if socket.flush().is_err() { break; } }
                                Ok(Message::Close(_)) => break,
                                Ok(_) => {}
                                Err(tungstenite::Error::Io(error)) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                                Err(_) => break,
                            }
                            if last_activity.elapsed() > Duration::from_secs(45) { break; }
                        }
                        let _ = socket.close(None);
                    }
                }
            }
        }
        backoff.wait(&shutdown);
    }
}
