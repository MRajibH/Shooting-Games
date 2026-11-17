mod game;
mod rooms;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use futures_util::{SinkExt, StreamExt};
use riftline_sim::constants::DEFAULT_PORT;
use riftline_sim::protocol::ClientMessage;
use tokio::sync::mpsc::unbounded_channel;
use tower_http::services::{ServeDir, ServeFile};

use game::RoomEvent;
use rooms::Registry;

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    // Serves the built client (npm run build). In dev, Vite serves it and proxies /ws here.
    let dist = std::env::var("CLIENT_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../client/dist"))
        });
    let static_files = ServeDir::new(&dist).fallback(ServeFile::new(dist.join("index.html")));

    let app = Router::new()
        .route("/ws", get(upgrade))
        .fallback_service(static_files)
        .with_state(Registry::default());

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .expect("bind port");
    println!("RIFTLINE server on http://localhost:{port}");
    axum::serve(listener, app).await.expect("server");
}

async fn upgrade(ws: WebSocketUpgrade, State(reg): State<Registry>) -> Response {
    ws.max_message_size(16 * 1024)
        .on_upgrade(move |socket| session(socket, reg))
}

async fn session(socket: WebSocket, reg: Registry) {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let (mut sink, mut stream) = socket.split();
    let (out, mut outbox) = unbounded_channel::<String>();
    let writer = tokio::spawn(async move {
        while let Some(raw) = outbox.recv().await {
            if sink.send(Message::Text(raw.into())).await.is_err() {
                break;
            }
        }
    });

    let mut seat = None;
    while let Some(Ok(msg)) = stream.next().await {
        let Message::Text(text) = msg else { continue };
        let Ok(msg) = serde_json::from_str::<ClientMessage>(&text) else {
            continue;
        };
        match (msg, &seat) {
            (ClientMessage::Join { name, room, mode }, None) => {
                let name: String = name
                    .chars()
                    .filter(|c| c.is_alphanumeric() || " _-".contains(*c))
                    .take(16)
                    .collect();
                let name = if name.trim().is_empty() {
                    format!("Operator-{id}")
                } else {
                    name
                };
                let s = reg.take_seat(room.as_deref(), mode.unwrap_or_default());
                let _ = s.tx.send(RoomEvent::Join {
                    id,
                    name,
                    out: out.clone(),
                });
                seat = Some(s);
            }
            (ClientMessage::Input { mut frames }, Some(s)) => {
                frames.truncate(8);
                let _ = s.tx.send(RoomEvent::Input { id, frames });
            }
            _ => {}
        }
    }

    if let Some(s) = seat {
        reg.release(&s.room);
        let _ = s.tx.send(RoomEvent::Leave { id });
    }
    writer.abort();
}
