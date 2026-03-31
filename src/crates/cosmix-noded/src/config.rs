//! Config module — serves settings over AMP and watches for file changes.
//!
//! Handles config.get, config.set, config.list, config.sections,
//! config.watch, config.reload.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::RwLock;

struct AppState {
    settings: RwLock<cosmix_config::CosmixSettings>,
    watchers: RwLock<Vec<String>>,
    client: cosmix_client::HubClient,
}

pub async fn run(hub_url: &str) -> Result<()> {
    let settings = cosmix_config::store::load()?;
    tracing::info!(
        path = %cosmix_config::store::config_path().display(),
        "Loaded settings"
    );

    let client = cosmix_client::HubClient::connect("config", hub_url).await?;
    tracing::info!("Config module registered on hub");

    let state = Arc::new(AppState {
        settings: RwLock::new(settings),
        watchers: RwLock::new(Vec::new()),
        client,
    });

    // Spawn file watcher
    let watcher_state = state.clone();
    tokio::spawn(async move {
        watch_config_file(watcher_state).await;
    });

    handle_hub_commands(state).await;

    tracing::info!("Config module stopped");
    Ok(())
}

async fn handle_hub_commands(state: Arc<AppState>) {
    let mut rx = match state.client.incoming_async().await {
        Some(rx) => rx,
        None => return,
    };

    while let Some(cmd) = rx.recv().await {
        let result = match cmd.command.as_str() {
            "config.get" => handle_get(&state, &cmd).await,
            "config.set" => handle_set(&state, &cmd).await,
            "config.list" => handle_list(&state, &cmd).await,
            "config.sections" => handle_sections(&state).await,
            "config.watch" => handle_watch(&state, &cmd).await,
            "config.reload" => handle_reload(&state).await,
            _ => Err(format!("unknown command: {}", cmd.command)),
        };

        match result {
            Ok(body) => {
                if let Err(e) = state.client.respond(&cmd, 0, &body).await {
                    tracing::warn!("failed to send response: {e}");
                }
            }
            Err(msg) => {
                let err_body = serde_json::json!({"error": msg}).to_string();
                if let Err(e) = state.client.respond(&cmd, 10, &err_body).await {
                    tracing::warn!("failed to send error response: {e}");
                }
            }
        }
    }
}

async fn handle_get(state: &AppState, cmd: &cosmix_client::IncomingCommand) -> Result<String, String> {
    let key = cmd.args.get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'key' argument".to_string())?;

    let settings = state.settings.read().await;
    let value = cosmix_config::store::get_value(&settings, key)
        .map_err(|e| e.to_string())?;

    Ok(serde_json::json!({"key": key, "value": value}).to_string())
}

async fn handle_set(state: &AppState, cmd: &cosmix_client::IncomingCommand) -> Result<String, String> {
    let key = cmd.args.get("key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'key' argument".to_string())?;

    let value = cmd.args.get("value")
        .ok_or_else(|| "missing 'value' argument".to_string())?;

    let key_owned = key.to_string();

    {
        let mut settings = state.settings.write().await;
        cosmix_config::store::set_value(&mut settings, &key_owned, value.clone())
            .map_err(|e| e.to_string())?;
        cosmix_config::store::save(&settings)
            .map_err(|e| e.to_string())?;
    }

    notify_watchers(state, &serde_json::json!({
        "key": key_owned,
        "value": value,
    })).await;

    Ok(serde_json::json!({"ok": true}).to_string())
}

async fn handle_list(state: &AppState, cmd: &cosmix_client::IncomingCommand) -> Result<String, String> {
    let settings = state.settings.read().await;

    let section = cmd.args.get("section").and_then(|v| v.as_str());

    if let Some(section) = section {
        let data = cosmix_config::store::list_section(&settings, section)
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({section: data}).to_string())
    } else {
        let all = cosmix_config::store::list_all(&settings)
            .map_err(|e| e.to_string())?;
        Ok(serde_json::to_string(&all).map_err(|e| e.to_string())?)
    }
}

async fn handle_sections(state: &AppState) -> Result<String, String> {
    let settings = state.settings.read().await;
    let sections = cosmix_config::store::list_sections(&settings)
        .map_err(|e| e.to_string())?;
    Ok(serde_json::to_string(&sections).map_err(|e| e.to_string())?)
}

async fn handle_watch(state: &AppState, cmd: &cosmix_client::IncomingCommand) -> Result<String, String> {
    let from = cmd.from.clone();
    let mut watchers = state.watchers.write().await;
    if !watchers.contains(&from) {
        watchers.push(from.clone());
        tracing::info!(watcher = %from, "Added config watcher");
    }
    Ok(serde_json::json!({"watching": true}).to_string())
}

async fn handle_reload(state: &AppState) -> Result<String, String> {
    let new_settings = cosmix_config::store::load()
        .map_err(|e| e.to_string())?;
    *state.settings.write().await = new_settings;
    tracing::info!("Reloaded settings from disk");

    notify_watchers(state, &serde_json::json!({"reload": true})).await;

    Ok(serde_json::json!({"ok": true}).to_string())
}

async fn notify_watchers(state: &AppState, payload: &serde_json::Value) {
    let watchers = state.watchers.read().await;
    let body = payload.to_string();

    for watcher in watchers.iter() {
        if let Err(e) = state.client.send(watcher, "config.changed", serde_json::Value::Null).await {
            tracing::debug!(watcher = %watcher, error = %e, "Failed to notify watcher (may have disconnected)");
        }
    }

    if !watchers.is_empty() {
        tracing::debug!(count = watchers.len(), body = %body, "Notified watchers");
    }
}

async fn watch_config_file(state: Arc<AppState>) {
    use notify::{Watcher, RecursiveMode, Event, EventKind};

    let config_path = cosmix_config::store::config_path();

    let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(1);

    let mut watcher = match notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        if let Ok(event) = res {
            if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                let _ = tx.try_send(());
            }
        }
    }) {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!("Failed to create file watcher: {e}");
            return;
        }
    };

    let watch_dir = config_path.parent().unwrap_or(&config_path);
    if let Err(e) = watcher.watch(watch_dir, RecursiveMode::NonRecursive) {
        tracing::warn!("Failed to watch {}: {e}", watch_dir.display());
        return;
    }

    tracing::info!(path = %config_path.display(), "Watching settings file for changes");

    loop {
        if rx.recv().await.is_none() {
            break;
        }

        // Debounce — drain events within 500ms
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        while rx.try_recv().is_ok() {}

        tracing::info!("Settings file changed, reloading");
        match cosmix_config::store::load() {
            Ok(new_settings) => {
                *state.settings.write().await = new_settings;
                notify_watchers(&state, &serde_json::json!({"reload": true})).await;
            }
            Err(e) => {
                tracing::warn!("Failed to reload settings: {e}");
            }
        }
    }
}
