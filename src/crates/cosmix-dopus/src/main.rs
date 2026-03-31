//! cosmix-dopus — Dual-pane file manager inspired by Directory Opus 4.
//!
//! Two file listers (source/dest) with a configurable button bank,
//! drive buttons, and full AMP integration. Registers as "dopus" on the hub.

use std::collections::HashSet;
use std::path::Path;

use dioxus::prelude::*;
use serde::Serialize;
use cosmix_ui::app_init::{THEME, use_theme_css, use_theme_poll, use_hub_client, use_hub_handler};
use cosmix_ui::menu::{menubar, standard_file_menu, MenuBar};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    cosmix_ui::app_init::launch_desktop("cosmix-dopus", 1100.0, 700.0, app);
}

// ── Data ──

#[derive(Clone, Debug, Serialize)]
struct FileEntry {
    name: String,
    path: String,
    is_dir: bool,
    size: u64,
    modified: String,
}

#[derive(Clone, Debug, PartialEq, Copy)]
enum Side {
    Left,
    Right,
}

impl Side {
    fn other(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }
}

#[derive(Clone, Debug)]
struct PaneState {
    path: String,
    entries: Vec<FileEntry>,
    selected: HashSet<usize>,
    last_clicked: Option<usize>,
}

impl PaneState {
    fn new(path: &str) -> Self {
        Self {
            entries: read_directory(path),
            path: path.to_string(),
            selected: HashSet::new(),
            last_clicked: None,
        }
    }

    fn navigate(&mut self, path: &str) {
        self.path = path.to_string();
        self.entries = read_directory(path);
        self.selected.clear();
        self.last_clicked = None;
    }

    fn refresh(&mut self) {
        self.entries = read_directory(&self.path);
        self.selected.retain(|&i| i < self.entries.len());
    }

    fn selected_paths(&self) -> Vec<String> {
        self.selected
            .iter()
            .filter_map(|&i| self.entries.get(i).map(|e| e.path.clone()))
            .collect()
    }

    fn selected_count(&self) -> usize {
        self.selected.len()
    }
}

fn read_directory(path: &str) -> Vec<FileEntry> {
    let show_hidden = *SHOW_HIDDEN.peek();
    let mut entries = Vec::new();
    if let Ok(rd) = std::fs::read_dir(path) {
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !show_hidden && name.starts_with('.') {
                continue;
            }
            let meta = entry.metadata().ok();
            entries.push(FileEntry {
                name: entry.file_name().to_string_lossy().to_string(),
                path: entry.path().to_string_lossy().to_string(),
                is_dir: meta.as_ref().map(|m| m.is_dir()).unwrap_or(false),
                size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                modified: meta
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .map(|t| {
                        let dt: chrono::DateTime<chrono::Local> = t.into();
                        dt.format("%Y-%m-%d %H:%M").to_string()
                    })
                    .unwrap_or_default(),
            });
        }
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes}");
    }
    let kb = bytes as f64 / 1024.0;
    if kb < 1024.0 {
        return format!("{kb:.1}K");
    }
    let mb = kb / 1024.0;
    if mb < 1024.0 {
        return format!("{mb:.1}M");
    }
    let gb = mb / 1024.0;
    format!("{gb:.1}G")
}

fn home_dir() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".into())
}

// ── File operations ──

fn op_copy(sources: &[String], dest_dir: &str) -> Result<usize, String> {
    let mut count = 0;
    for src in sources {
        let src_path = Path::new(src);
        let name = src_path.file_name().ok_or("invalid path")?;
        let dest = Path::new(dest_dir).join(name);
        if src_path.is_dir() {
            copy_dir_recursive(src_path, &dest)?;
        } else {
            std::fs::copy(src_path, &dest).map_err(|e| e.to_string())?;
        }
        count += 1;
    }
    Ok(count)
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())?.flatten() {
        let dest_child = dest.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir_recursive(&entry.path(), &dest_child)?;
        } else {
            std::fs::copy(entry.path(), &dest_child).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn op_move(sources: &[String], dest_dir: &str) -> Result<usize, String> {
    let mut count = 0;
    for src in sources {
        let src_path = Path::new(src);
        let name = src_path.file_name().ok_or("invalid path")?;
        let dest = Path::new(dest_dir).join(name);
        if std::fs::rename(src_path, &dest).is_err() {
            // Cross-filesystem: copy then delete
            if src_path.is_dir() {
                copy_dir_recursive(src_path, &dest)?;
                std::fs::remove_dir_all(src_path).map_err(|e| e.to_string())?;
            } else {
                std::fs::copy(src_path, &dest).map_err(|e| e.to_string())?;
                std::fs::remove_file(src_path).map_err(|e| e.to_string())?;
            }
        }
        count += 1;
    }
    Ok(count)
}

fn op_delete(sources: &[String]) -> Result<usize, String> {
    let mut count = 0;
    for src in sources {
        let path = Path::new(src);
        if path.is_dir() {
            std::fs::remove_dir_all(path).map_err(|e| e.to_string())?;
        } else {
            std::fs::remove_file(path).map_err(|e| e.to_string())?;
        }
        count += 1;
    }
    Ok(count)
}

// ── Button bank ──

#[derive(Clone, Copy, PartialEq)]
enum BankAction {
    SelectAll,
    SelectNone,
    Copy,
    MakeDir,
    Read,
    Edit,
    Refresh,
    Parent,
    Rename,
    Move,
    Pattern,
    GetSizes,
    Root,
    Delete,
    Hidden,
    // Placeholders (disabled in v1)
    Search,
    Hunt,
    HexRead,
    Show,
}

impl BankAction {
    fn label(self) -> &'static str {
        match self {
            Self::SelectAll => "All",
            Self::SelectNone => "None",
            Self::Copy => "Copy",
            Self::MakeDir => "MkDir",
            Self::Read => "Read",
            Self::Edit => "Edit",
            Self::Refresh => "Refresh",
            Self::Parent => "Parent",
            Self::Rename => "Rename",
            Self::Move => "Move",
            Self::Pattern => "Pattern",
            Self::GetSizes => "GetSiz",
            Self::Root => "Root",
            Self::Delete => "DELETE",
            Self::Hidden => if *SHOW_HIDDEN.peek() { "Hide .files" } else { "Show .files" },
            Self::Search => "Search",
            Self::Hunt => "Hunt",
            Self::HexRead => "HexRd",
            Self::Show => "Show",
        }
    }

    fn enabled(self) -> bool {
        !matches!(self, Self::Search | Self::Hunt | Self::HexRead | Self::Show)
    }
}

const BUTTON_BANK: &[&[BankAction]] = &[
    &[BankAction::SelectAll, BankAction::SelectNone, BankAction::Copy, BankAction::MakeDir, BankAction::Search, BankAction::Read, BankAction::Edit, BankAction::Refresh],
    &[BankAction::Parent, BankAction::Rename, BankAction::Move, BankAction::Pattern, BankAction::Hunt, BankAction::HexRead, BankAction::Show, BankAction::GetSizes],
    &[BankAction::Root, BankAction::Delete, BankAction::Hidden],
];

// ── Drive buttons ──

fn drive_buttons() -> Vec<(&'static str, String)> {
    let home = home_dir();
    vec![
        ("/", "/".into()),
        ("~", home.clone()),
        ("Desktop", format!("{home}/Desktop")),
        ("Down", format!("{home}/Downloads")),
        ("Docs", format!("{home}/Documents")),
    ]
}

// ── Hub command handling ──

/// Drag state: (source side, list of file paths being dragged)
static DRAG_STATE: GlobalSignal<Option<(Side, Vec<String>)>> = Signal::global(|| None);

/// Whether to show hidden (dot) files
static SHOW_HIDDEN: GlobalSignal<bool> = Signal::global(|| false);

static LEFT_PATH: GlobalSignal<String> = Signal::global(|| home_dir());
static RIGHT_PATH: GlobalSignal<String> = Signal::global(|| home_dir());
static SOURCE_SIDE: GlobalSignal<Side> = Signal::global(|| Side::Left);

fn dispatch_command(cmd: &cosmix_client::IncomingCommand) -> Result<String, String> {
    match cmd.command.as_str() {
        "dopus.status" => {
            let left = LEFT_PATH.peek().clone();
            let right = RIGHT_PATH.peek().clone();
            let side = *SOURCE_SIDE.peek();
            Ok(serde_json::json!({
                "left_path": left,
                "right_path": right,
                "source": if side == Side::Left { "left" } else { "right" },
            }).to_string())
        }
        "dopus.navigate" => {
            let path = cmd.args.get("path")
                .and_then(|v| v.as_str())
                .ok_or("missing path")?;
            let pane = cmd.args.get("pane")
                .and_then(|v| v.as_str())
                .unwrap_or("source");
            let side = match pane {
                "left" => Side::Left,
                "right" => Side::Right,
                _ => *SOURCE_SIDE.peek(),
            };
            match side {
                Side::Left => *LEFT_PATH.write() = path.to_string(),
                Side::Right => *RIGHT_PATH.write() = path.to_string(),
            }
            Ok(serde_json::json!({"navigated": path}).to_string())
        }
        _ => Err(format!("unknown command: {}", cmd.command)),
    }
}

// ── UI ──

fn app() -> Element {
    let home = home_dir();
    let mut left = use_signal(|| PaneState::new(&home));
    let mut right = use_signal(|| PaneState::new(&home));
    let mut source_side = use_signal(|| Side::Left);
    let mut status_msg: Signal<String> = use_signal(String::new);
    let mut prompt: Signal<Option<(String, String)>> = use_signal(|| None); // (action, input_value)

    // Keep globals in sync for AMP queries
    use_effect(move || {
        *LEFT_PATH.write() = left().path.clone();
        *RIGHT_PATH.write() = right().path.clone();
        *SOURCE_SIDE.write() = source_side();
    });

    let hub = use_hub_client("dopus");
    use_hub_handler(hub, "dopus", dispatch_command);
    use_theme_poll(30);

    let app_menu = menubar(vec![standard_file_menu(vec![])]);

    // Navigation helper
    let mut navigate_pane = move |side: Side, path: String| {
        match side {
            Side::Left => left.write().navigate(&path),
            Side::Right => right.write().navigate(&path),
        }
        status_msg.write().clear();
    };

    let mut refresh_both = move || {
        left.write().refresh();
        right.write().refresh();
        status_msg.set("Refreshed".into());
    };

    // Get source/dest references
    // Button action handler
    let mut handle_bank_action = move |action: BankAction| {
        let ss = source_side();
        match action {
            BankAction::SelectAll => {
                let mut pane = match ss { Side::Left => left.write(), Side::Right => right.write() };
                let all: HashSet<usize> = (0..pane.entries.len()).collect();
                pane.selected = all;
            }
            BankAction::SelectNone => {
                match ss { Side::Left => left.write(), Side::Right => right.write() }.selected.clear();
            }
            BankAction::Parent => {
                let path = match ss { Side::Left => left().path.clone(), Side::Right => right().path.clone() };
                if let Some(parent) = Path::new(&path).parent() {
                    navigate_pane(ss, parent.to_string_lossy().to_string());
                }
            }
            BankAction::Root => {
                navigate_pane(ss, "/".into());
            }
            BankAction::Refresh => {
                refresh_both();
            }
            BankAction::Copy => {
                let src_paths = match ss { Side::Left => left().selected_paths(), Side::Right => right().selected_paths() };
                let dest_dir = match ss.other() { Side::Left => left().path.clone(), Side::Right => right().path.clone() };
                if src_paths.is_empty() {
                    status_msg.set("No files selected".into());
                } else {
                    match op_copy(&src_paths, &dest_dir) {
                        Ok(n) => {
                            status_msg.set(format!("Copied {n} items"));
                            match ss.other() { Side::Left => left.write().refresh(), Side::Right => right.write().refresh() };
                        }
                        Err(e) => status_msg.set(format!("Copy error: {e}")),
                    }
                }
            }
            BankAction::Move => {
                let src_paths = match ss { Side::Left => left().selected_paths(), Side::Right => right().selected_paths() };
                let dest_dir = match ss.other() { Side::Left => left().path.clone(), Side::Right => right().path.clone() };
                if src_paths.is_empty() {
                    status_msg.set("No files selected".into());
                } else {
                    match op_move(&src_paths, &dest_dir) {
                        Ok(n) => {
                            status_msg.set(format!("Moved {n} items"));
                            refresh_both();
                        }
                        Err(e) => status_msg.set(format!("Move error: {e}")),
                    }
                }
            }
            BankAction::Delete => {
                let src_paths = match ss { Side::Left => left().selected_paths(), Side::Right => right().selected_paths() };
                if src_paths.is_empty() {
                    status_msg.set("No files selected".into());
                } else {
                    match op_delete(&src_paths) {
                        Ok(n) => {
                            status_msg.set(format!("Deleted {n} items"));
                            match ss { Side::Left => left.write().refresh(), Side::Right => right.write().refresh() };
                        }
                        Err(e) => status_msg.set(format!("Delete error: {e}")),
                    }
                }
            }
            BankAction::MakeDir => {
                prompt.set(Some(("mkdir".into(), String::new())));
            }
            BankAction::Rename => {
                let sel = match ss { Side::Left => left().selected_paths(), Side::Right => right().selected_paths() };
                if sel.len() == 1 {
                    let name = Path::new(&sel[0]).file_name().unwrap_or_default().to_string_lossy().to_string();
                    prompt.set(Some(("rename".into(), name)));
                } else {
                    status_msg.set("Select exactly one file to rename".into());
                }
            }
            BankAction::Read => {
                let sel = match ss { Side::Left => left().selected_paths(), Side::Right => right().selected_paths() };
                if let Some(path) = sel.first() {
                    if let Some(hub) = hub() {
                        let path = path.clone();
                        spawn(async move {
                            let _ = hub.call("view", "view.open", serde_json::json!({"path": path})).await;
                        });
                    }
                }
            }
            BankAction::Edit => {
                let sel = match ss { Side::Left => left().selected_paths(), Side::Right => right().selected_paths() };
                if let Some(path) = sel.first() {
                    if let Some(hub) = hub() {
                        let path = path.clone();
                        spawn(async move {
                            let _ = hub.call("edit", "edit.open", serde_json::json!({"path": path})).await;
                        });
                    }
                }
            }
            BankAction::GetSizes => {
                let mut pane = match ss { Side::Left => left.write(), Side::Right => right.write() };
                for entry in &mut pane.entries {
                    if entry.is_dir {
                        entry.size = dir_size(&entry.path);
                    }
                }
                status_msg.set("Sizes calculated".into());
            }
            BankAction::Pattern => {
                prompt.set(Some(("pattern".into(), "*".into())));
            }
            BankAction::Hidden => {
                let cur = *SHOW_HIDDEN.peek();
                *SHOW_HIDDEN.write() = !cur;
                left.write().refresh();
                right.write().refresh();
                status_msg.set(if !cur { "Showing hidden files".into() } else { "Hiding hidden files".into() });
            }
            _ => {}
        }
    };

    // Keyboard handler
    let onkeydown = move |e: KeyboardEvent| {
        if e.modifiers().ctrl() {
            if let Key::Character(ref c) = e.key() {
                match c.as_str() {
                    "q" => std::process::exit(0),
                    "a" => handle_bank_action(BankAction::SelectAll),
                    "c" => handle_bank_action(BankAction::Copy),
                    "m" => handle_bank_action(BankAction::Move),
                    _ => {}
                }
            }
        } else {
            match e.key() {
                Key::Tab => source_side.set(source_side().other()),
                Key::Backspace => handle_bank_action(BankAction::Parent),
                Key::Delete => handle_bank_action(BankAction::Delete),
                Key::F5 => handle_bank_action(BankAction::Refresh),
                Key::F7 => handle_bank_action(BankAction::MakeDir),
                _ => {}
            }
        }
    };

    use_theme_css();
    let theme = THEME.read();
    let fs = theme.font_size;
    let fs_sm = fs.saturating_sub(2);
    let fs_xs = fs.saturating_sub(4);

    rsx! {
        div {
            tabindex: "0",
            onkeydown: onkeydown,
            style: "outline:none; width:100%; height:100vh; display:flex; flex-direction:column; background:var(--bg-primary); color:var(--fg-primary); font-family:var(--font-mono); font-size:{fs}px;",

            MenuBar {
                menu: app_menu,
                on_action: move |id: String| match id.as_str() {
                    "quit" => std::process::exit(0),
                    _ => {}
                },
            }

            // ── Dual panes ──
            div {
                style: "display:flex; flex:1; min-height:0;",

                // Left pane
                {render_pane(left, Side::Left, source_side(), fs, fs_sm, left, right, source_side)}

                // Divider
                div { style: "width:2px; background:var(--border); flex-shrink:0;" }

                // Right pane
                {render_pane(right, Side::Right, source_side(), fs, fs_sm, left, right, source_side)}
            }

            // ── Drive buttons ──
            div {
                style: "display:flex; gap:1px; padding:2px 4px; background:var(--bg-secondary); border-top:1px solid var(--border); flex-shrink:0;",
                for (label, path) in drive_buttons() {
                    {
                        let path = path.clone();
                        let ss = source_side();
                        rsx! {
                            div {
                                style: "padding:2px 8px; background:var(--bg-tertiary); color:var(--fg-secondary); border-right:1px solid var(--border); cursor:pointer; font-size:{fs_xs}px; font-family:var(--font-mono); user-select:none;",
                                onclick: move |_| navigate_pane(ss, path.clone()),
                                "{label}"
                            }
                        }
                    }
                }
            }

            // ── Button bank ──
            div {
                style: "display:grid; grid-template-columns:repeat(8,1fr); gap:1px; padding:2px; background:var(--bg-secondary); border-top:1px solid var(--border); flex-shrink:0;",
                for row in BUTTON_BANK {
                    for &action in *row {
                        {
                            let enabled = action.enabled();
                            let is_hidden_active = action == BankAction::Hidden && *SHOW_HIDDEN.peek();
                            let bg = if enabled { "var(--bg-tertiary)" } else { "var(--bg-secondary)" };
                            let fg = if enabled { "var(--fg-secondary)" } else { "var(--fg-muted)" };
                            let cursor = if enabled { "pointer" } else { "default" };
                            let is_delete = action == BankAction::Delete;
                            let fg = if is_delete { "var(--error, #c44)" } else { fg };
                            let border_style = if is_hidden_active { "inset" } else { "outset" };
                            rsx! {
                                button {
                                    style: "padding:4px 2px; background:{bg}; color:{fg}; border:1px {border_style} var(--border); cursor:{cursor}; font-size:{fs_xs}px; font-family:var(--font-mono); font-weight:600; text-align:center; user-select:none;",
                                    disabled: !enabled,
                                    onclick: move |_| if enabled { handle_bank_action(action) },
                                    "{action.label()}"
                                }
                            }
                        }
                    }
                }
            }

            // ── Prompt bar (for mkdir/rename/pattern) ──
            if let Some((action, value)) = prompt() {
                div {
                    style: "display:flex; align-items:center; gap:8px; padding:4px 8px; background:var(--bg-tertiary); border-top:1px solid var(--border); flex-shrink:0;",
                    span { style: "color:var(--fg-muted); font-size:{fs_sm}px;",
                        "{action}:"
                    }
                    input {
                        style: "flex:1; padding:2px 6px; background:var(--bg-primary); color:var(--fg-primary); border:1px solid var(--border); border-radius:var(--radius-sm); font-size:{fs_sm}px; font-family:var(--font-mono);",
                        value: "{value}",
                        autofocus: true,
                        oninput: move |e: FormEvent| {
                            if let Some((ref a, _)) = prompt() {
                                prompt.set(Some((a.clone(), e.value())));
                            }
                        },
                        onkeydown: move |e: KeyboardEvent| {
                            match e.key() {
                                Key::Enter => {
                                    if let Some((ref action, ref val)) = prompt() {
                                        let ss = source_side();
                                        match action.as_str() {
                                            "mkdir" => {
                                                let dir = match ss { Side::Left => left().path.clone(), Side::Right => right().path.clone() };
                                                let full = format!("{dir}/{val}");
                                                match std::fs::create_dir_all(&full) {
                                                    Ok(()) => {
                                                        status_msg.set(format!("Created {val}"));
                                                        match ss { Side::Left => left.write().refresh(), Side::Right => right.write().refresh() };
                                                    }
                                                    Err(e) => status_msg.set(format!("mkdir error: {e}")),
                                                }
                                            }
                                            "rename" => {
                                                let sel = match ss { Side::Left => left().selected_paths(), Side::Right => right().selected_paths() };
                                                if let Some(old) = sel.first() {
                                                    let parent = Path::new(old).parent().unwrap_or(Path::new("/"));
                                                    let new_path = parent.join(val);
                                                    match std::fs::rename(old, &new_path) {
                                                        Ok(()) => {
                                                            status_msg.set(format!("Renamed to {val}"));
                                                            match ss { Side::Left => left.write().refresh(), Side::Right => right.write().refresh() };
                                                        }
                                                        Err(e) => status_msg.set(format!("Rename error: {e}")),
                                                    }
                                                }
                                            }
                                            "pattern" => {
                                                let pane = match ss { Side::Left => left(), Side::Right => right() };
                                                let pat = val.to_lowercase();
                                                let mut matched: HashSet<usize> = HashSet::new();
                                                for (i, entry) in pane.entries.iter().enumerate() {
                                                    if glob_match(&pat, &entry.name.to_lowercase()) {
                                                        matched.insert(i);
                                                    }
                                                }
                                                let count = matched.len();
                                                match ss { Side::Left => left.write().selected = matched, Side::Right => right.write().selected = matched };
                                                status_msg.set(format!("{count} matched"));
                                            }
                                            _ => {}
                                        }
                                    }
                                    prompt.set(None);
                                }
                                Key::Escape => {
                                    prompt.set(None);
                                }
                                _ => {}
                            }
                        },
                    }
                    div {
                        style: "padding:2px 8px; background:var(--bg-secondary); color:var(--fg-muted); border:1px solid var(--border); cursor:pointer; font-size:{fs_xs}px; user-select:none;",
                        onclick: move |_| prompt.set(None),
                        "Esc"
                    }
                }
            }

            // ── Status bar ──
            div {
                style: "padding:3px 8px; background:var(--bg-secondary); border-top:1px solid var(--border); color:var(--fg-muted); font-size:{fs_xs}px; flex-shrink:0; display:flex; justify-content:space-between;",
                span {
                    {
                        let ss = source_side();
                        let pane = match ss { Side::Left => left(), Side::Right => right() };
                        let sel = pane.selected_count();
                        let total = pane.entries.len();
                        if sel > 0 {
                            format!("{sel} of {total} selected")
                        } else {
                            format!("{total} items")
                        }
                    }
                }
                span { "{status_msg}" }
            }
        }
    }
}

// ── Pane rendering ──

fn render_pane(
    pane_sig: Signal<PaneState>,
    side: Side,
    source_side: Side,
    fs: u16,
    fs_sm: u16,
    mut left: Signal<PaneState>,
    mut right: Signal<PaneState>,
    source_signal: Signal<Side>,
) -> Element {
    let pane = pane_sig();
    let is_source = side == source_side;
    let role_label = if is_source { "S" } else { "D" };
    let role_color = if is_source { "var(--accent)" } else { "var(--fg-muted)" };
    let border_left = if is_source { "3px solid var(--accent)" } else { "3px solid transparent" };
    let fs_xs = fs.saturating_sub(4);

    let dir = pane.path.clone();
    let segments: Vec<(String, String)> = {
        let parts: Vec<&str> = dir.split('/').collect();
        let mut segs = vec![("/".to_string(), "/".to_string())];
        let mut accum = String::new();
        for part in &parts[1..] {
            if part.is_empty() { continue; }
            accum.push('/');
            accum.push_str(part);
            segs.push((part.to_string(), accum.clone()));
        }
        segs
    };

    let mut pw = pane_sig;
    let mut ss = source_signal;

    // Navigate via signal directly
    let mut nav = move |s: Side, path: String| {
        match s {
            Side::Left => left.write().navigate(&path),
            Side::Right => right.write().navigate(&path),
        }
    };

    rsx! {
        div {
            style: "flex:1; display:flex; flex-direction:column; min-width:0; border-left:{border_left};",
            onclick: move |_| ss.set(side),

            // Path bar
            div {
                style: "display:flex; align-items:center; padding:3px 6px; background:var(--bg-secondary); border-bottom:1px solid var(--border); flex-shrink:0; overflow-x:auto; white-space:nowrap;",
                span {
                    style: "font-weight:700; color:{role_color}; margin-right:6px; font-size:{fs_sm}px;",
                    "{role_label}"
                }
                for (i, (label, path)) in segments.iter().enumerate() {
                    if i > 1 {
                        span { style: "color:var(--fg-muted); margin:0 1px; font-size:{fs_xs}px;", "/" }
                    }
                    {
                        let path = path.clone();
                        rsx! {
                            span {
                                style: "color:var(--fg-secondary); cursor:pointer; padding:1px 2px; font-size:{fs_xs}px;",
                                onclick: move |e| {
                                    e.stop_propagation();
                                    nav(side,path.clone());
                                },
                                "{label}"
                            }
                        }
                    }
                }
            }

            // Column headers
            div {
                style: "display:flex; align-items:center; padding:2px 6px; background:var(--bg-tertiary); border-bottom:1px solid var(--border); color:var(--fg-muted); font-size:{fs_xs}px; font-weight:600; text-transform:uppercase; flex-shrink:0;",
                span { style: "flex:1; min-width:0;", "Name" }
                span { style: "width:60px; text-align:right; margin-right:8px;", "Size" }
                span { style: "width:140px;", "Modified" }
            }

            // File list
            div {
                style: "flex:1; overflow-y:auto; min-height:0;",

                // Parent entry
                if dir != "/" {
                    {
                        let parent = Path::new(&*dir)
                            .parent()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|| "/".to_string());
                        rsx! {
                            div {
                                style: "display:flex; align-items:center; padding:2px 6px; cursor:pointer; border-bottom:1px solid rgba(128,128,128,0.4);",
                                onclick: move |e| {
                                    e.stop_propagation();
                                    nav(side,parent.clone());
                                },
                                span { style: "margin-right:6px; color:var(--fg-muted);", dangerous_inner_html: "{ICON_FOLDER}" }
                                span { style: "flex:1; color:var(--fg-secondary);", ".." }
                            }
                        }
                    }
                }

                // Entries
                for (idx, entry) in pane.entries.iter().enumerate() {
                    {
                        let entry_path = entry.path.clone();
                        let entry_is_dir = entry.is_dir;
                        let is_selected = pane.selected.contains(&idx);
                        let bg = if is_selected { "rgba(80,140,220,0.3)" } else { "transparent" };
                        let icon = if entry.is_dir { ICON_FOLDER } else { ICON_FILE };
                        let icon_color = if entry.is_dir { "var(--accent)" } else { "var(--fg-muted)" };
                        let name_color = if is_selected { "var(--accent)" } else if entry.is_dir { "var(--fg-primary)" } else { "var(--fg-secondary)" };
                        let size_str = if entry.is_dir && entry.size > 0 { format_size(entry.size) } else if entry.is_dir { String::new() } else { format_size(entry.size) };
                        let name = entry.name.clone();
                        let modified = entry.modified.clone();
                        let drag_path = entry.path.clone();

                        rsx! {
                            div {
                                style: "display:flex; align-items:center; padding:2px 6px; cursor:pointer; border-bottom:1px solid rgba(128,128,128,0.4); background:{bg}; user-select:none;",
                                // Single click: toggle selection
                                onclick: move |e| {
                                    e.stop_propagation();
                                    ss.set(side);
                                    let mut p = pw.write();
                                    if p.selected.contains(&idx) {
                                        p.selected.remove(&idx);
                                    } else {
                                        p.selected.insert(idx);
                                    }
                                    p.last_clicked = Some(idx);
                                },
                                // Double click: navigate into dirs
                                ondoubleclick: move |e| {
                                    e.stop_propagation();
                                    if entry_is_dir {
                                        nav(side, entry_path.clone());
                                    }
                                },
                                span { style: "margin-right:6px; color:{icon_color}; flex-shrink:0;", dangerous_inner_html: "{icon}" }
                                span { style: "flex:1; min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; color:{name_color}; font-size:{fs_sm}px;", "{name}" }
                                span { style: "width:60px; text-align:right; margin-right:8px; color:var(--fg-muted); font-size:{fs_xs}px;", "{size_str}" }
                                span { style: "width:140px; color:var(--fg-muted); font-size:{fs_xs}px; white-space:nowrap;", "{modified}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ── Helpers ──

fn dir_size(path: &str) -> u64 {
    let mut total = 0u64;
    if let Ok(rd) = std::fs::read_dir(path) {
        for entry in rd.flatten() {
            let meta = entry.metadata().ok();
            if let Some(m) = meta {
                if m.is_dir() {
                    total += dir_size(&entry.path().to_string_lossy());
                } else {
                    total += m.len();
                }
            }
        }
    }
    total
}

fn glob_match(pattern: &str, name: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return name.ends_with(suffix);
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return name.starts_with(prefix);
    }
    if pattern.contains('*') {
        let parts: Vec<&str> = pattern.split('*').collect();
        if parts.len() == 2 {
            return name.starts_with(parts[0]) && name.ends_with(parts[1]);
        }
    }
    name == pattern
}

// ── Icons ──

const ICON_FOLDER: &str = cosmix_ui::icons::ICON_FOLDER;
const ICON_FILE: &str = cosmix_ui::icons::ICON_FILE_EDIT;
