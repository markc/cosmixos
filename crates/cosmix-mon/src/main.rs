//! cosmix-mon — System monitor service for the cosmix appmesh.
//!
//! Registers as "mon" on the hub. Handles:
//! - `mon.status` — CPU, memory, disk, network, uptime
//! - `mon.processes` — top processes by CPU/memory
//!
//! UI shows local system stats with auto-refresh.
//! Query remote nodes via mesh: `mon.mko.amp`

use std::sync::Arc;

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use sysinfo::System;

fn main() {
    cosmix_ui::desktop::init_linux_env();

    #[cfg(feature = "desktop")]
    {
        use dioxus_desktop::{Config, LogicalSize, WindowBuilder};

        let cfg = Config::new().with_window(
            WindowBuilder::new()
                .with_title("cosmix-mon")
                .with_inner_size(LogicalSize::new(720.0, 520.0)),
        );

        LaunchBuilder::new().with_cfg(cfg).launch(app);
        return;
    }

    #[allow(unreachable_code)]
    {
        eprintln!("Desktop feature not enabled");
        std::process::exit(1);
    }
}

// ── System data ──

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SystemStatus {
    hostname: String,
    uptime_secs: u64,
    cpu_count: usize,
    cpu_usage: f32,
    mem_total_mb: u64,
    mem_used_mb: u64,
    mem_percent: f32,
    swap_total_mb: u64,
    swap_used_mb: u64,
    disks: Vec<DiskInfo>,
    load_avg: [f64; 3],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DiskInfo {
    mount: String,
    total_gb: f64,
    used_gb: f64,
    percent: f32,
}

#[derive(Clone, Debug, Serialize)]
struct ProcessInfo {
    pid: u32,
    name: String,
    cpu: f32,
    mem_mb: u64,
}

fn gather_status() -> SystemStatus {
    let mut sys = System::new_all();
    sys.refresh_all();

    let cpu_usage = sys.global_cpu_usage();
    let mem_total = sys.total_memory();
    let mem_used = sys.used_memory();
    let swap_total = sys.total_swap();
    let swap_used = sys.used_swap();

    let disks: Vec<DiskInfo> = sysinfo::Disks::new_with_refreshed_list()
        .iter()
        .filter(|d| {
            let mp = d.mount_point().to_string_lossy();
            mp == "/" || mp.starts_with("/home") || mp.starts_with("/data")
        })
        .map(|d| {
            let total = d.total_space() as f64 / 1_073_741_824.0;
            let used = (d.total_space() - d.available_space()) as f64 / 1_073_741_824.0;
            let pct = if total > 0.0 { (used / total * 100.0) as f32 } else { 0.0 };
            DiskInfo {
                mount: d.mount_point().to_string_lossy().to_string(),
                total_gb: (total * 10.0).round() / 10.0,
                used_gb: (used * 10.0).round() / 10.0,
                percent: pct,
            }
        })
        .collect();

    let load = System::load_average();

    SystemStatus {
        hostname: System::host_name().unwrap_or_else(|| "unknown".into()),
        uptime_secs: System::uptime(),
        cpu_count: sys.cpus().len(),
        cpu_usage,
        mem_total_mb: mem_total / 1_048_576,
        mem_used_mb: mem_used / 1_048_576,
        mem_percent: if mem_total > 0 { (mem_used as f32 / mem_total as f32) * 100.0 } else { 0.0 },
        swap_total_mb: swap_total / 1_048_576,
        swap_used_mb: swap_used / 1_048_576,
        disks,
        load_avg: [load.one, load.five, load.fifteen],
    }
}

fn gather_processes(limit: usize) -> Vec<ProcessInfo> {
    let mut sys = System::new_all();
    sys.refresh_all();

    let mut procs: Vec<ProcessInfo> = sys.processes().values().map(|p| {
        ProcessInfo {
            pid: p.pid().as_u32(),
            name: p.name().to_string_lossy().to_string(),
            cpu: p.cpu_usage(),
            mem_mb: p.memory() / 1_048_576,
        }
    }).collect();

    procs.sort_by(|a, b| b.cpu.partial_cmp(&a.cpu).unwrap_or(std::cmp::Ordering::Equal));
    procs.truncate(limit);
    procs
}

fn format_uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    if days > 0 {
        format!("{days}d {hours}h {mins}m")
    } else if hours > 0 {
        format!("{hours}h {mins}m")
    } else {
        format!("{mins}m")
    }
}

// ── Hub command handling ──

async fn handle_hub_commands(client: Arc<cosmix_client::HubClient>) {
    let mut rx = match client.incoming_async().await {
        Some(rx) => rx,
        None => return,
    };

    while let Some(cmd) = rx.recv().await {
        let result = match cmd.command.as_str() {
            "mon.status" => {
                let status = gather_status();
                serde_json::to_string(&status).map_err(|e| e.to_string())
            }
            "mon.processes" => {
                let limit = cmd.args.get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(15) as usize;
                let procs = gather_processes(limit);
                serde_json::to_string(&procs).map_err(|e| e.to_string())
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

// ── UI ──

fn app() -> Element {
    let mut status = use_signal(gather_status);
    let mut remote_status: Signal<Option<SystemStatus>> = use_signal(|| None);
    let mut remote_node = use_signal(|| String::new());
    let mut hub_client: Signal<Option<Arc<cosmix_client::HubClient>>> = use_signal(|| None);

    // Connect to hub + auto-refresh every 5 seconds
    use_effect(move || {
        spawn(async move {
            match cosmix_client::HubClient::connect_default("mon").await {
                Ok(client) => {
                    let client = Arc::new(client);
                    hub_client.set(Some(client.clone()));
                    tracing::info!("connected to cosmix-hub as 'mon'");
                    tokio::spawn(handle_hub_commands(client));
                }
                Err(_) => {
                    tracing::debug!("hub not available, running standalone");
                }
            }
        });

        // Periodic refresh
        spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                status.set(gather_status());
            }
        });
    });

    let fetch_remote = move |_| {
        let node = remote_node();
        if node.is_empty() {
            return;
        }
        spawn(async move {
            if let Some(client) = hub_client() {
                let target = format!("mon.{node}.amp");
                match client.call(&target, "mon.status", serde_json::Value::Null).await {
                    Ok(val) => {
                        if let Ok(s) = serde_json::from_value::<SystemStatus>(val) {
                            remote_status.set(Some(s));
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to fetch remote status");
                        remote_status.set(None);
                    }
                }
            }
        });
    };

    let s = status();

    rsx! {
        document::Style { "{CSS}" }
        div {
            style: "width:100%; height:100vh; display:flex; flex-direction:column; background:{BG_BASE}; color:{TEXT_PRIMARY}; font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Helvetica,Arial,sans-serif; font-size:13px; overflow-y:auto;",

            // Header
            div {
                style: "padding:12px 16px; background:{BG_SURFACE}; border-bottom:1px solid {BORDER}; display:flex; align-items:center; gap:12px;",
                span { style: "font-weight:600; font-size:15px;", "{s.hostname}" }
                span { style: "color:{TEXT_DIM}; font-size:12px;", "up {format_uptime(s.uptime_secs)}" }
                span { style: "color:{TEXT_DIM}; font-size:12px;", "load {s.load_avg[0]:.2} {s.load_avg[1]:.2} {s.load_avg[2]:.2}" }

                // Remote query
                div { style: "margin-left:auto; display:flex; align-items:center; gap:6px;",
                    input {
                        style: "background:{BG_ELEVATED}; border:1px solid {BORDER}; color:{TEXT_PRIMARY}; padding:4px 8px; border-radius:4px; width:100px; font-size:12px;",
                        placeholder: "node name",
                        value: "{remote_node}",
                        oninput: move |e| remote_node.set(e.value()),
                    }
                    button {
                        style: "background:{BG_ELEVATED}; border:1px solid {BORDER}; color:{TEXT_MUTED}; padding:4px 10px; border-radius:4px; cursor:pointer; font-size:12px;",
                        onclick: fetch_remote,
                        "Query"
                    }
                }
            }

            // Main content
            div { style: "padding:16px; display:flex; flex-direction:column; gap:16px;",

                // CPU + Memory row
                div { style: "display:flex; gap:16px;",
                    {stat_card("CPU", &format!("{:.1}%", s.cpu_usage), &format!("{} cores", s.cpu_count), pct_color(s.cpu_usage))}
                    {stat_card("Memory", &format!("{} / {} MB", s.mem_used_mb, s.mem_total_mb), &format!("{:.1}%", s.mem_percent), pct_color(s.mem_percent))}
                    if s.swap_total_mb > 0 {
                        {stat_card("Swap", &format!("{} / {} MB", s.swap_used_mb, s.swap_total_mb), "", TEXT_MUTED)}
                    }
                }

                // Disks
                if !s.disks.is_empty() {
                    div { style: "background:{BG_SURFACE}; border-radius:6px; padding:12px;",
                        div { style: "font-weight:600; margin-bottom:8px; color:{TEXT_MUTED};", "Disks" }
                        for disk in s.disks.iter() {
                            div { style: "display:flex; align-items:center; gap:12px; margin-bottom:6px;",
                                span { style: "width:120px; color:{TEXT_SECONDARY}; font-size:12px;", "{disk.mount}" }
                                div { style: "flex:1; height:8px; background:{BG_ELEVATED}; border-radius:4px; overflow:hidden;",
                                    div { style: "height:100%; width:{disk.percent}%; background:{pct_color(disk.percent)}; border-radius:4px;" }
                                }
                                span { style: "width:120px; text-align:right; color:{TEXT_DIM}; font-size:12px;",
                                    "{disk.used_gb:.1} / {disk.total_gb:.1} GB"
                                }
                            }
                        }
                    }
                }

                // Remote node status (if queried)
                if let Some(ref rs) = remote_status() {
                    div { style: "background:{BG_SURFACE}; border-radius:6px; padding:12px; border:1px solid #2563eb44;",
                        div { style: "font-weight:600; margin-bottom:8px; color:#60a5fa;", "Remote: {rs.hostname}" }
                        div { style: "display:flex; gap:16px;",
                            {stat_card("CPU", &format!("{:.1}%", rs.cpu_usage), &format!("{} cores", rs.cpu_count), pct_color(rs.cpu_usage))}
                            {stat_card("Memory", &format!("{} / {} MB", rs.mem_used_mb, rs.mem_total_mb), &format!("{:.1}%", rs.mem_percent), pct_color(rs.mem_percent))}
                        }
                    }
                }
            }
        }
    }
}

fn stat_card(title: &str, value: &str, subtitle: &str, accent: &str) -> Element {
    rsx! {
        div { style: "flex:1; background:{BG_SURFACE}; border-radius:6px; padding:12px;",
            div { style: "color:{TEXT_DIM}; font-size:11px; text-transform:uppercase; letter-spacing:0.05em; margin-bottom:4px;", "{title}" }
            div { style: "font-size:18px; font-weight:600; color:{accent};", "{value}" }
            if !subtitle.is_empty() {
                div { style: "color:{TEXT_DIM}; font-size:11px; margin-top:2px;", "{subtitle}" }
            }
        }
    }
}

fn pct_color(pct: f32) -> &'static str {
    if pct > 90.0 { "#ef4444" }
    else if pct > 70.0 { "#f59e0b" }
    else { "#22c55e" }
}

// ── Theme ──

const BG_BASE: &str = cosmix_ui::theme::BG_BASE;
const BG_SURFACE: &str = cosmix_ui::theme::BG_SURFACE;
const BG_ELEVATED: &str = cosmix_ui::theme::BG_ELEVATED;
const BORDER: &str = cosmix_ui::theme::BORDER_DEFAULT;
const TEXT_PRIMARY: &str = cosmix_ui::theme::TEXT_PRIMARY;
const TEXT_SECONDARY: &str = cosmix_ui::theme::TEXT_SECONDARY;
const TEXT_MUTED: &str = cosmix_ui::theme::TEXT_MUTED;
const TEXT_DIM: &str = cosmix_ui::theme::TEXT_DIM;

const CSS: &str = r#"
html, body, #main {
    margin: 0; padding: 0;
    width: 100%; height: 100%;
    overflow: hidden;
}
::-webkit-scrollbar { width: 8px; }
::-webkit-scrollbar-track { background: transparent; }
::-webkit-scrollbar-thumb { background: #374151; border-radius: 4px; }
::-webkit-scrollbar-thumb:hover { background: #4b5563; }
button:hover { background: #374151 !important; }
"#;
