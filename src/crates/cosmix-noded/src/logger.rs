//! Logger module — taps AMP traffic and writes to amp.log.
//!
//! Subscribes to hub.tap and writes each message in a human-readable,
//! grep-friendly format to ~/.local/log/cosmix/amp.log.

use std::io::Write;
use std::sync::Arc;

use anyhow::Result;
use chrono::Local;

pub async fn run(hub_url: &str) -> Result<()> {
    let log_dir = cosmix_daemon::log_dir();
    let _ = std::fs::create_dir_all(&log_dir);
    let amp_log_path = log_dir.join("amp.log");

    let client = Arc::new(
        cosmix_client::HubClient::connect("log", hub_url).await?
    );
    tracing::info!("Logger module connected to hub");

    // Subscribe to AMP traffic tap
    client.call("hub", "hub.tap", serde_json::Value::Null).await?;
    tracing::info!("Subscribed to AMP traffic tap");

    let Some(mut rx) = client.incoming_async().await else {
        anyhow::bail!("Failed to get incoming stream");
    };

    while let Some(cmd) = rx.recv().await {
        let now = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let from = &cmd.from;
        let command = &cmd.command;
        let body_len = cmd.body.len();

        let line = format!(
            "[{now}] {from} → {command}  body={body_len}B\n"
        );

        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&amp_log_path)
        {
            let _ = file.write_all(line.as_bytes());
        }

        tracing::debug!("{from} → {command} ({body_len}B)");
    }

    tracing::info!("Logger module stopped");
    Ok(())
}
