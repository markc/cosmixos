//! cosmix-hub — Local WebSocket message broker for the cosmix appmesh.
//!
//! Apps connect via WebSocket at `ws://localhost:4200/ws`, register with a
//! service name, and the hub routes AMP messages between them by `to` header.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use clap::Parser;
use cosmix_port::amp::{self, AmpMessage};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{RwLock, mpsc};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// ── CLI ──

#[derive(Parser)]
#[command(name = "cosmix-hub", about = "Local WebSocket message broker for the cosmix appmesh")]
struct Cli {
    /// Port to listen on
    #[arg(long, default_value = "4200")]
    port: u16,
}

// ── App state ──

/// Maps service name → sender for that service's WebSocket.
type Registry = Arc<RwLock<HashMap<String, mpsc::UnboundedSender<String>>>>;

#[derive(Clone)]
struct AppState {
    registry: Registry,
}

// ── Main ──

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "cosmix_hub=info".into()),
        )
        .init();

    let cli = Cli::parse();

    let state = AppState {
        registry: Arc::new(RwLock::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/ws", axum::routing::get(ws_handler))
        .with_state(state);

    let addr = format!("127.0.0.1:{}", cli.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Hub listening on ws://localhost:{}", cli.port);

    axum::serve(listener, app).await?;

    Ok(())
}

// ── WebSocket handler ──

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut ws_sink, mut ws_stream) = socket.split();

    // Channel for sending messages back to this client
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // Spawn a task that forwards from the channel to the WebSocket sink
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_sink.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Track this connection's registered service name
    let mut service_name: Option<String> = None;

    // Read messages from the WebSocket
    while let Some(Ok(msg)) = ws_stream.next().await {
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Close(_) => break,
            _ => continue,
        };

        let amp_msg = match amp::parse(&text) {
            Ok(m) => m,
            Err(e) => {
                tracing::debug!("Invalid AMP message: {e}");
                let err = AmpMessage::new()
                    .with_header("rc", "10")
                    .with_header("error", &format!("Invalid AMP message: {e}"));
                let _ = tx.send(err.to_wire());
                continue;
            }
        };

        // Check if message is addressed to the hub itself
        let target = amp_msg.to_addr().unwrap_or("hub");
        if target == "hub" {
            handle_hub_command(&amp_msg, &tx, &state.registry, &mut service_name).await;
            continue;
        }

        // Route to target service
        let registry = state.registry.read().await;
        if let Some(target_tx) = registry.get(target) {
            if target_tx.send(text).is_err() {
                drop(registry);
                // Target disconnected, remove it
                state.registry.write().await.remove(target);
                let err = AmpMessage::new()
                    .with_header("rc", "10")
                    .with_header("error", &format!("Service '{target}' disconnected"));
                let _ = tx.send(err.to_wire());
            }
        } else {
            let err = AmpMessage::new()
                .with_header("rc", "10")
                .with_header("error", &format!("Service '{target}' not found"));
            let _ = tx.send(err.to_wire());
        }
    }

    // Cleanup: remove service from registry on disconnect
    if let Some(name) = &service_name {
        state.registry.write().await.remove(name);
        tracing::info!("Service '{}' disconnected", name);
    }

    send_task.abort();
}

// ── Hub internal commands ──

async fn handle_hub_command(
    msg: &AmpMessage,
    tx: &mpsc::UnboundedSender<String>,
    registry: &Registry,
    service_name: &mut Option<String>,
) {
    let command = msg.command_name().unwrap_or("");
    let msg_id = msg.get("id");

    // Helper: build a response that echoes the request id
    let respond = |rc: &str| -> AmpMessage {
        let mut resp = AmpMessage::new()
            .with_header("type", "response")
            .with_header("from", "hub")
            .with_header("rc", rc);
        if let Some(id) = msg_id {
            resp.set("id", id);
        }
        resp
    };

    match command {
        "hub.register" => {
            let from = match msg.from_addr() {
                Some(f) => f.to_string(),
                None => {
                    let mut resp = respond("10");
                    resp.set("error", "hub.register requires 'from' header");
                    let _ = tx.send(resp.to_wire());
                    return;
                }
            };

            if let Some(old_name) = service_name.take() {
                registry.write().await.remove(&old_name);
            }

            registry.write().await.insert(from.clone(), tx.clone());
            *service_name = Some(from.clone());
            tracing::info!("Service '{}' registered", from);

            let mut resp = respond("0");
            resp.set("command", "hub.register");
            resp.body = format!(r#"{{"registered": "{}"}}"#, from);
            let _ = tx.send(resp.to_wire());
        }

        "hub.list" => {
            let reg = registry.read().await;
            let services: Vec<&str> = reg.keys().map(|s| s.as_str()).collect();
            let body = serde_json::to_string(&services).unwrap_or_else(|_| "[]".to_string());

            let mut resp = respond("0");
            resp.set("command", "hub.list");
            resp.body = body;
            let _ = tx.send(resp.to_wire());
        }

        "hub.ping" => {
            let mut resp = respond("0");
            resp.set("command", "hub.ping");
            resp.body = r#"{"pong": true}"#.to_string();
            let _ = tx.send(resp.to_wire());
        }

        _ => {
            let mut resp = respond("10");
            resp.set("error", &format!("Unknown hub command: '{command}'"));
            let _ = tx.send(resp.to_wire());
        }
    }
}
