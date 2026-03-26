//! Hub integration for cosmix-mail.
//!
//! Connects to the local cosmix-hub as "mail" service.
//! Handles incoming commands: mail.status (unread count).
//! Provides helpers for delegating to other services (edit.compose, file.pick).

use std::sync::Arc;

/// Connect to the hub and start handling commands.
/// Fails silently if hub is unavailable.
pub async fn connect_to_hub() {
    match cosmix_client::HubClient::connect_default("mail").await {
        Ok(client) => {
            let client = Arc::new(client);
            tracing::info!("connected to cosmix-hub as 'mail'");
            tokio::spawn(handle_commands(client));
        }
        Err(_) => {
            tracing::debug!("hub not available, running standalone");
        }
    }
}

async fn handle_commands(client: Arc<cosmix_client::HubClient>) {
    let mut rx = match client.incoming_async().await {
        Some(rx) => rx,
        None => return,
    };

    while let Some(cmd) = rx.recv().await {
        let result = match cmd.command.as_str() {
            "mail.status" => {
                // Return basic status — unread count would come from JMAP state
                Ok(serde_json::json!({
                    "service": "cosmix-mail",
                    "status": "running",
                }).to_string())
            }
            _ => Err(format!("unknown command: {}", cmd.command)),
        };

        match result {
            Ok(body) => {
                if let Err(e) = client.respond(&cmd, 0, &body).await {
                    tracing::warn!("failed to send response: {e}");
                }
            }
            Err(msg) => {
                let err_body = serde_json::json!({"error": msg}).to_string();
                if let Err(e) = client.respond(&cmd, 10, &err_body).await {
                    tracing::warn!("failed to send error response: {e}");
                }
            }
        }
    }
}
