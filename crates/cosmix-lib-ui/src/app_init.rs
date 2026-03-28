//! Shared app initialization for all cosmix Dioxus apps.
//!
//! Provides the global THEME signal, theme polling/hub-watch hooks,
//! and the `launch_desktop()` helper that replaces boilerplate in every app's `main()`.

use dioxus::prelude::*;
use crate::theme::{ThemeParams, generate_css};

// ── Global theme signal ──

/// Global theme signal loaded from cosmix config on startup.
/// All apps should use this instead of declaring their own static THEME.
pub static THEME: GlobalSignal<ThemeParams> = Signal::global(|| {
    #[cfg(all(not(target_arch = "wasm32"), feature = "config"))]
    {
        cosmix_config::store::load()
            .map(|s| ThemeParams {
                hue: s.global.theme_hue,
                dark: s.global.theme_dark,
                font_size: s.global.font_size,
            })
            .unwrap_or_default()
    }
    #[cfg(any(target_arch = "wasm32", not(feature = "config")))]
    {
        ThemeParams::default()
    }
});

/// Reload THEME from config file. Call from any context that can write signals.
#[cfg(all(not(target_arch = "wasm32"), feature = "config"))]
pub fn reload_theme() {
    if let Ok(settings) = cosmix_config::store::load() {
        *THEME.write() = ThemeParams {
            hue: settings.global.theme_hue,
            dark: settings.global.theme_dark,
            font_size: settings.global.font_size,
        };
    }
}

// ── Theme CSS hook ──

/// Returns the current theme CSS string for injection via `document::Style`.
///
/// Usage in any app's `app()`:
/// ```ignore
/// let css = use_theme_css();
/// rsx! { document::Style { "{css}" } ... }
/// ```
pub fn use_theme_css() -> String {
    let theme = THEME.read();
    generate_css(&theme)
}

// ── Theme polling hook ──

/// Spawns a background task that reloads the theme from config every `interval_secs`.
/// Call once in your app's root component. Requires `config` feature.
///
/// For apps connected to the hub, prefer `use_theme_hub_watch()` instead.
#[cfg(all(not(target_arch = "wasm32"), feature = "config"))]
pub fn use_theme_poll(interval_secs: u64) {
    use_effect(move || {
        spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
                reload_theme();
            }
        });
    });
}

// ── Theme hub watch hook ──

/// Registers with configd for `config.changed` notifications and updates THEME.
/// Also handles the `config.changed` command in the incoming command stream.
///
/// This is the preferred theme refresh method for hub-connected apps.
/// Requires both `hub` and `config` features.
#[cfg(all(not(target_arch = "wasm32"), feature = "hub", feature = "config"))]
pub fn use_theme_hub_watch(
    client: Signal<Option<std::sync::Arc<cosmix_client::HubClient>>>,
    service_name: &'static str,
) {
    use_effect(move || {
        spawn(async move {
            // Wait for hub client to connect
            loop {
                if client().is_some() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }

            if let Some(c) = client() {
                // Register as config watcher
                let _ = c.call(
                    "configd",
                    "config.watch",
                    serde_json::json!({ "watcher": service_name }),
                ).await;
            }
        });
    });
}

/// Handle a `config.changed` command by reloading the theme.
/// Call this from your hub command handler's match arms:
///
/// ```ignore
/// "config.changed" => { cosmix_ui::app_init::handle_config_changed(); Ok("ok".into()) }
/// ```
#[cfg(all(not(target_arch = "wasm32"), feature = "config"))]
pub fn handle_config_changed() {
    reload_theme();
}

// ── Desktop launch helper ──

/// Standard desktop app launcher. Replaces the boilerplate in every app's `main()`.
///
/// Handles:
/// - `init_linux_env()` (WebKit workaround)
/// - Window config (frameless, CSD)
/// - `LaunchBuilder` with desktop config
/// - WASM fallback with `dioxus::launch()`
///
/// Usage:
/// ```ignore
/// fn main() {
///     cosmix_ui::app_init::launch_desktop("Cosmix Files", 900.0, 640.0, app);
/// }
/// ```
pub fn launch_desktop(title: &str, width: f64, height: f64, app_fn: fn() -> Element) {
    #[cfg(all(not(target_arch = "wasm32"), feature = "desktop"))]
    {
        crate::desktop::init_linux_env();
        let cfg = crate::desktop::window_config(title, width, height);
        LaunchBuilder::new().with_cfg(cfg).launch(app_fn);
        return;
    }

    #[allow(unreachable_code)]
    {
        dioxus::launch(app_fn);
    }
}
