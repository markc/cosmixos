//! Dioxus Desktop backend — the existing full-featured rendering path.
//!
//! Handles all dialog types via WebKitGTK. Subject to cosmic-comp's 240px minimum.

use std::sync::OnceLock;

use dioxus::prelude::*;

use crate::render::DialogView;
use crate::window::{self, RESULT};
use crate::DialogRequest;
use cosmix_ui::app_init::{use_theme_css, THEME};
use cosmix_ui::menu::{MenuBar, MenuBarDef};
use cosmix_ui::theme::ThemeParams;

/// The parsed dialog request, set once before Dioxus launches.
static REQUEST: OnceLock<DialogRequest> = OnceLock::new();

/// Theme override from CLI (--dark/--light). None = use config default.
static THEME_OVERRIDE: OnceLock<Option<bool>> = OnceLock::new();

/// Run a dialog using the Dioxus Desktop (WebKitGTK) backend.
/// This function takes ownership of the process — it calls Dioxus launch which
/// does not return until the window closes.
pub fn run(request: DialogRequest) {
    let title = request.effective_title().to_string();
    let (w, h) = request.default_size();
    let json_output = request.json_output;
    let theme_dark = request.theme_dark;

    REQUEST.set(request).expect("REQUEST already set");
    THEME_OVERRIDE.set(theme_dark).ok();
    window::JSON_OUTPUT.store(json_output, std::sync::atomic::Ordering::SeqCst);

    cosmix_ui::desktop::init_linux_env();
    let cfg = window::dialog_window_config(&title, w as f64, h as f64);
    LaunchBuilder::new().with_cfg(cfg).launch(app);

    // After window closes, output result to stdout
    if let Some(result) = RESULT.peek().as_ref() {
        let output = result.to_stdout(json_output);
        if !output.is_empty() {
            print!("{output}");
        }
    }
}

fn app() -> Element {
    // Apply theme: CLI flag overrides config. Default to dark for dialogs.
    {
        let dark = THEME_OVERRIDE
            .get()
            .and_then(|o| *o)
            .unwrap_or(true);
        let (needs_update, font_size) = {
            let current = THEME.read();
            (current.dark != dark, current.font_size)
        };
        if needs_update {
            *THEME.write() = ThemeParams {
                dark,
                font_size,
            };
        }
    }
    use_theme_css();

    let request = REQUEST.get().expect("REQUEST not set").clone();
    let menu = MenuBarDef::new();

    rsx! {
        document::Stylesheet { href: asset!("/assets/tailwind.css") }
        document::Stylesheet { href: asset!("/src/components/alert_dialog/style.css") }
        div { class: "flex flex-col w-full h-full",
            MenuBar { menu, on_action: move |_: String| {} }
            DialogView { request }
        }
    }
}
