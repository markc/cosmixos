//! cosmix-noded — Consolidated node daemon.
//!
//! Single binary running the hub (WebSocket broker), config service,
//! system monitor, and AMP traffic logger as async tasks.

use anyhow::Result;
use clap::Parser;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

mod hub;
mod config;
mod monitor;
mod logger;

#[derive(Parser)]
#[command(name = "cosmix-noded", about = "Consolidated cosmix node daemon")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Start the node daemon (default)
    Serve {
        /// Port for the hub WebSocket listener
        #[arg(long, default_value = "4200")]
        port: u16,

        /// This node's name on the mesh
        #[arg(long, default_value = "localhost")]
        node: String,

        /// Path to mesh config file
        #[arg(long)]
        mesh_config: Option<String>,

        /// Disable the system monitor module
        #[arg(long)]
        no_monitor: bool,

        /// Disable the AMP traffic logger module
        #[arg(long)]
        no_log: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let _log = cosmix_daemon::init_tracing("cosmix_noded");

    let cli = Cli::parse();

    // Default to Serve if no subcommand given
    let (port, node, mesh_config, no_monitor, no_log) = match cli.command {
        Some(Command::Serve { port, node, mesh_config, no_monitor, no_log }) => {
            (port, node, mesh_config, no_monitor, no_log)
        }
        None => (4200, "localhost".to_string(), None, false, false),
    };

    let hub_url = format!("ws://127.0.0.1:{port}/ws");

    tracing::info!(node = %node, port = port, "Starting cosmix-noded");

    // Start the hub with a readiness signal
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let hub_node = node.clone();
    let hub_handle = tokio::spawn(async move {
        if let Err(e) = hub::run(port, hub_node, mesh_config, ready_tx).await {
            tracing::error!("Hub module failed: {e}");
        }
    });

    // Wait for the hub listener to be bound
    if ready_rx.await.is_err() {
        anyhow::bail!("Hub failed to start");
    }

    // Spawn client modules
    let config_url = hub_url.clone();
    let config_handle = tokio::spawn(async move {
        if let Err(e) = config::run(&config_url).await {
            tracing::error!("Config module failed: {e}");
        }
    });

    let monitor_handle = if !no_monitor {
        let mon_url = hub_url.clone();
        Some(tokio::spawn(async move {
            if let Err(e) = monitor::run(&mon_url).await {
                tracing::error!("Monitor module failed: {e}");
            }
        }))
    } else {
        None
    };

    let logger_handle = if !no_log {
        let log_url = hub_url.clone();
        Some(tokio::spawn(async move {
            if let Err(e) = logger::run(&log_url).await {
                tracing::error!("Logger module failed: {e}");
            }
        }))
    } else {
        None
    };

    // Wait for shutdown signal or hub failure
    tokio::select! {
        _ = cosmix_daemon::shutdown_signal() => {
            tracing::info!("Shutdown signal received");
        }
        _ = hub_handle => {
            tracing::error!("Hub exited unexpectedly");
        }
    }

    // Cleanup — abort remaining tasks
    config_handle.abort();
    if let Some(h) = monitor_handle { h.abort(); }
    if let Some(h) = logger_handle { h.abort(); }

    tracing::info!("cosmix-noded stopped");
    Ok(())
}
