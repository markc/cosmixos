use dioxus::prelude::*;

/// Set Linux-specific environment variables for WebKitGTK.
/// Call before Dioxus launch.
pub fn init_linux_env() {
    #[cfg(target_os = "linux")]
    unsafe {
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
    }
}

/// Build a File menu with Open and Quit items.
/// Shortcut hints shown in text (no muda accelerators — they cause GTK warnings).
pub fn build_file_menu() -> dioxus_desktop::muda::Menu {
    use dioxus_desktop::muda::*;

    let menu = Menu::new();
    let file_menu = Submenu::new("&File", true);
    file_menu.append(&MenuItem::with_id("open", "&Open...\tCtrl+O", true, None)).ok();
    file_menu.append(&PredefinedMenuItem::separator()).ok();
    file_menu.append(&MenuItem::with_id("quit", "&Quit\tCtrl+Q", true, None)).ok();
    menu.append(&file_menu).ok();
    menu
}

/// Open an async file picker dialog. Returns the selected path, if any.
pub async fn pick_file(filters: &[(&str, &[&str])]) -> Option<std::path::PathBuf> {
    let mut dialog = rfd::AsyncFileDialog::new().set_title("Open file");
    for (name, exts) in filters {
        dialog = dialog.add_filter(*name, exts);
    }
    dialog = dialog.add_filter("All files", &["*"]);
    dialog.pick_file().await.map(|h| h.path().to_path_buf())
}

/// Handle Ctrl+O / Ctrl+Q keyboard shortcuts.
/// Returns true if the event was handled.
pub fn handle_shortcut(e: &KeyboardEvent, on_open: impl FnOnce(), on_quit: impl FnOnce()) -> bool {
    if e.modifiers().ctrl() {
        match e.key() {
            Key::Character(ref c) if c == "o" => { on_open(); true }
            Key::Character(ref c) if c == "q" => { on_quit(); true }
            _ => false,
        }
    } else {
        false
    }
}
