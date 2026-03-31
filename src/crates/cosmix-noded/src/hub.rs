//! Hub module — WebSocket message broker for the cosmix appmesh.
//!
//! Routes AMP messages between local services and bridges to remote
//! mesh nodes over WireGuard.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use cosmix_mesh::{MeshConfig, MeshPeers};
use cosmix_amp::amp::{self, AmpAddress, AmpMessage};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{RwLock, mpsc, oneshot};

// ── State ──

type Registry = Arc<RwLock<HashMap<String, mpsc::UnboundedSender<String>>>>;
type PendingResponses = Arc<RwLock<HashMap<String, mpsc::UnboundedSender<String>>>>;
type TapSubscribers = Arc<RwLock<Vec<mpsc::UnboundedSender<String>>>>;

#[derive(Clone)]
struct AppState {
    registry: Registry,
    pending_responses: PendingResponses,
    tap_subscribers: TapSubscribers,
    mesh: Arc<MeshPeers>,
    node_name: String,
}

// ── Entry point ──

pub async fn run(
    port: u16,
    node: String,
    mesh_config_path: Option<String>,
    ready_tx: oneshot::Sender<()>,
) -> Result<()> {
    let mesh_config = if let Some(ref path) = mesh_config_path {
        MeshConfig::load(path)?
    } else {
        MeshConfig::load_default(&node)
    };

    tracing::info!(
        node = %mesh_config.node_name,
        peers = mesh_config.peers.len(),
        "Mesh config loaded"
    );

    let (mesh_incoming_tx, mut mesh_incoming_rx) = mpsc::unbounded_channel::<AmpMessage>();
    let mesh = Arc::new(MeshPeers::new(mesh_config, mesh_incoming_tx));
    let registry: Registry = Arc::new(RwLock::new(HashMap::new()));

    // Deliver messages from remote hubs to local services
    let registry_for_mesh = registry.clone();
    tokio::spawn(async move {
        while let Some(msg) = mesh_incoming_rx.recv().await {
            let target = msg.to_addr().unwrap_or("").to_string();
            let service = if let Some(amp_addr) = AmpAddress::parse(&target) {
                amp_addr.app.unwrap_or(target.clone())
            } else {
                target.clone()
            };

            let registry = registry_for_mesh.read().await;
            if let Some(tx) = registry.get(&service) {
                let _ = tx.send(msg.to_wire());
            } else {
                tracing::debug!(target = %service, "No local service for incoming mesh message");
            }
        }
    });

    let pending_responses: PendingResponses = Arc::new(RwLock::new(HashMap::new()));
    let tap_subscribers: TapSubscribers = Arc::new(RwLock::new(Vec::new()));

    let state = AppState {
        registry,
        pending_responses,
        tap_subscribers,
        mesh,
        node_name: node.clone(),
    };

    let app = Router::new()
        .route("/ws", axum::routing::get(ws_handler))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(node = %node, "Hub listening on ws://0.0.0.0:{}", port);

    // Signal that the listener is bound and ready
    let _ = ready_tx.send(());

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

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_sink.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    let mut service_name: Option<String> = None;

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

        // Check if this is a response to a pending request
        if amp_msg.message_type() == Some("response") {
            if let Some(id) = amp_msg.get("id") {
                let pending = state.pending_responses.read().await;
                if let Some(caller_tx) = pending.get(id) {
                    let _ = caller_tx.send(text.clone());
                    drop(pending);
                    state.pending_responses.write().await.remove(id);
                    continue;
                }
            }
        }

        let target = amp_msg.to_addr().unwrap_or("hub");
        if target == "hub" {
            handle_hub_command(&amp_msg, &tx, &state, &mut service_name).await;
            continue;
        }

        // Mesh address routing
        if let Some(amp_addr) = AmpAddress::parse(target) {
            if !amp_addr.is_for_node(&state.node_name) {
                let node = amp_addr.node.clone();
                if state.mesh.is_remote_peer(&node) {
                    let tx_clone = tx.clone();
                    let mesh = state.mesh.clone();
                    tokio::spawn(async move {
                        match mesh.call(&node, amp_msg).await {
                            Ok(resp) => {
                                let _ = tx_clone.send(resp.to_wire());
                            }
                            Err(e) => {
                                let err = AmpMessage::new()
                                    .with_header("rc", "10")
                                    .with_header("error", &format!("Mesh bridge error: {e}"));
                                let _ = tx_clone.send(err.to_wire());
                            }
                        }
                    });
                } else {
                    let err = AmpMessage::new()
                        .with_header("rc", "10")
                        .with_header("error", &format!("Unknown mesh node: '{}'", amp_addr.node));
                    let _ = tx.send(err.to_wire());
                }
                continue;
            }

            let local_service = amp_addr.app.as_deref().unwrap_or(target);
            if local_service == "hub" {
                handle_hub_command(&amp_msg, &tx, &state, &mut service_name).await;
            } else {
                route_local(&state.registry, &state.pending_responses, local_service, &text, &amp_msg, &tx).await;
                broadcast_tap(&state.tap_subscribers, &text).await;
            }
            continue;
        }

        // Plain service name — route locally
        route_local(&state.registry, &state.pending_responses, target, &text, &amp_msg, &tx).await;
        broadcast_tap(&state.tap_subscribers, &text).await;
    }

    if let Some(name) = &service_name {
        state.registry.write().await.remove(name);
        tracing::info!("Service '{}' disconnected", name);
    }

    send_task.abort();
}

async fn route_local(
    registry: &Registry,
    pending_responses: &PendingResponses,
    service: &str,
    raw: &str,
    msg: &AmpMessage,
    caller_tx: &mpsc::UnboundedSender<String>,
) {
    let reg = registry.read().await;
    if let Some(target_tx) = reg.get(service) {
        if let Some(id) = msg.get("id") {
            pending_responses.write().await.insert(id.to_string(), caller_tx.clone());
        }
        if target_tx.send(raw.to_string()).is_err() {
            drop(reg);
            registry.write().await.remove(service);
            if let Some(id) = msg.get("id") {
                pending_responses.write().await.remove(id);
            }
            let err = AmpMessage::new()
                .with_header("rc", "10")
                .with_header("error", &format!("Service '{service}' disconnected"));
            let _ = caller_tx.send(err.to_wire());
        }
    } else {
        let err = AmpMessage::new()
            .with_header("rc", "10")
            .with_header("error", &format!("Service '{service}' not found"));
        let _ = caller_tx.send(err.to_wire());
    }
}

async fn broadcast_tap(tap_subscribers: &TapSubscribers, raw: &str) {
    let taps = tap_subscribers.read().await;
    if taps.is_empty() {
        return;
    }
    let mut disconnected = Vec::new();
    for (i, tap_tx) in taps.iter().enumerate() {
        if tap_tx.send(raw.to_string()).is_err() {
            disconnected.push(i);
        }
    }
    drop(taps);
    if !disconnected.is_empty() {
        let mut taps = tap_subscribers.write().await;
        for i in disconnected.into_iter().rev() {
            taps.remove(i);
        }
    }
}

// ── Hub internal commands ──

async fn handle_hub_command(
    msg: &AmpMessage,
    tx: &mpsc::UnboundedSender<String>,
    state: &AppState,
    service_name: &mut Option<String>,
) {
    let command = msg.command_name().unwrap_or("");
    let msg_id = msg.get("id");

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
                state.registry.write().await.remove(&old_name);
            }

            state.registry.write().await.insert(from.clone(), tx.clone());
            *service_name = Some(from.clone());
            tracing::info!("Service '{}' registered", from);

            let mut resp = respond("0");
            resp.set("command", "hub.register");
            resp.body = format!(r#"{{"registered": "{}"}}"#, from);
            let _ = tx.send(resp.to_wire());
        }

        "hub.list" => {
            let reg = state.registry.read().await;
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

        "hub.peers" => {
            let peer_names = state.mesh.peer_names();
            let body = serde_json::json!({
                "node": state.node_name,
                "peers": peer_names,
            });

            let mut resp = respond("0");
            resp.set("command", "hub.peers");
            resp.body = serde_json::to_string(&body).unwrap_or_else(|_| "{}".to_string());
            let _ = tx.send(resp.to_wire());
        }

        "hub.tap" => {
            state.tap_subscribers.write().await.push(tx.clone());
            tracing::info!("Tap subscriber added");

            let mut resp = respond("0");
            resp.set("command", "hub.tap");
            resp.body = r#"{"tapping": true}"#.to_string();
            let _ = tx.send(resp.to_wire());
        }

        _ => {
            let mut resp = respond("10");
            resp.set("error", &format!("Unknown hub command: '{command}'"));
            let _ = tx.send(resp.to_wire());
        }
    }
}
