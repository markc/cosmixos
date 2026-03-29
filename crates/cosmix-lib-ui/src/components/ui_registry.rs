//! AMP-addressable UI element registry and wrapper components.
//!
//! Apps register interactive UI elements by using wrapper components like
//! `AmpButton` instead of raw HTML buttons. These auto-register into a
//! per-app `UiRegistry` on mount and deregister on unmount.
//!
//! External AMP commands (`ui.invoke`, `ui.highlight`, `ui.list`, `ui.set`)
//! can then address elements by their semantic ID.

use std::collections::HashMap;
use dioxus::prelude::*;

// ── Types ────────────────────────────────────────────────────────────────

/// What kind of UI element this is (for discovery via `ui.list`).
#[derive(Clone, Debug, PartialEq)]
pub enum ElementKind {
    Button,
    Toggle,
    Input,
}

impl ElementKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Button => "button",
            Self::Toggle => "toggle",
            Self::Input => "input",
        }
    }
}

/// Current state of a registered UI element.
#[derive(Clone, Debug, PartialEq)]
pub enum ElementState {
    Button { disabled: bool },
    Toggle { checked: bool, disabled: bool },
    Input { value: String, disabled: bool },
}

/// Command that can be sent to a UI element via AMP.
#[derive(Clone, Debug, PartialEq)]
pub enum UiCommand {
    /// Activate the element (click a button, toggle a toggle).
    Invoke,
    /// Visual pulse highlight.
    Highlight { duration_ms: u32 },
    /// Set a value (for inputs/toggles).
    SetValue(String),
}

/// A registered UI element in the registry.
#[derive(Clone, Debug)]
pub struct UiElement {
    pub id: String,
    pub kind: ElementKind,
    pub label: String,
    pub disabled: bool,
}

/// Per-app registry of AMP-addressable UI elements.
///
/// Provided via `use_context_provider` at the app root, retrieved via
/// `use_context` in wrapper components.
#[derive(Clone, Debug, Default)]
pub struct UiRegistry {
    pub elements: HashMap<String, UiElement>,
}

impl UiRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, element: UiElement) {
        self.elements.insert(element.id.clone(), element);
    }

    pub fn deregister(&mut self, id: &str) {
        self.elements.remove(id);
    }

    /// List all elements, optionally filtered by ID prefix.
    pub fn list(&self, prefix: Option<&str>) -> Vec<serde_json::Value> {
        self.elements.values()
            .filter(|e| prefix.map_or(true, |p| e.id.starts_with(p)))
            .map(|e| serde_json::json!({
                "id": e.id,
                "kind": e.kind.as_str(),
                "label": e.label,
                "disabled": e.disabled,
            }))
            .collect()
    }
}

// ── Global signal for incoming UI commands ───────────────────────────────

/// Write to this signal to send a command to a specific UI element.
/// The element ID is included so the correct wrapper component reacts.
pub static UI_CMD: GlobalSignal<Option<(String, UiCommand)>> = Signal::global(|| None);

/// Global UI registry — all AmpButton/AmpToggle/AmpInput components
/// register themselves here on mount.
pub static UI_REGISTRY: GlobalSignal<UiRegistry> = Signal::global(UiRegistry::new);

// ── CSS ──────────────────────────────────────────────────────────────────

const AMP_BUTTON_CSS: &str = r#"
.cmx-amp-btn {
    padding: 4px 12px;
    border: 1px solid var(--border, #374151);
    border-radius: 4px;
    background: var(--bg-secondary, #111827);
    color: var(--fg-primary, #f3f4f6);
    cursor: pointer;
    font-size: var(--font-size-sm, 12px);
    font-family: system-ui, sans-serif;
    transition: background 0.15s;
}
.cmx-amp-btn:hover {
    background: var(--bg-tertiary, #1f2937);
}
.cmx-amp-btn:disabled {
    opacity: 0.4;
    cursor: default;
    pointer-events: none;
}
.cmx-amp-btn.cmx-amp-highlight {
    animation: amp-btn-pulse 400ms ease-out;
}
@keyframes amp-btn-pulse {
    0%   { box-shadow: 0 0 0 2px var(--accent, #3b82f6); }
    100% { box-shadow: 0 0 0 0 transparent; }
}
"#;

// ── AmpButton component ─────────────────────────────────────────────────

/// Drop-in AMP-addressable button that auto-registers with `UiRegistry`.
///
/// ```ignore
/// AmpButton {
///     id: "file.save",
///     label: "Save",
///     on_click: move |_| do_save(),
/// }
/// ```
///
/// External AMP commands can then invoke it:
/// ```text
/// command: ui.invoke
/// to: edit
/// ---
/// {"id": "file.save"}
/// ```
#[component]
pub fn AmpButton(
    /// Semantic ID for AMP addressing (e.g. "file.save"). Stable API surface.
    id: String,
    /// Display label for the button.
    label: String,
    /// Click handler — called for both real clicks and AMP `ui.invoke`.
    on_click: EventHandler<()>,
    /// Optional: disable the button.
    #[props(default = false)]
    disabled: bool,
    /// Optional: extra CSS class.
    #[props(default = String::new())]
    class: String,
) -> Element {
    let label_clone = label.clone();

    // Register on mount, deregister on unmount
    use_effect({
        let id = id.clone();
        let label = label.clone();
        move || {
            UI_REGISTRY.write().register(UiElement {
                id: id.clone(),
                kind: ElementKind::Button,
                label: label.clone(),
                disabled,
            });
        }
    });

    use_drop({
        let id = id.clone();
        move || {
            UI_REGISTRY.write().deregister(&id);
        }
    });

    // Watch for incoming AMP commands targeting this element
    #[allow(unused_mut)]
    let mut is_highlighted = use_signal(|| false);

    #[cfg(all(not(target_arch = "wasm32"), feature = "hub", feature = "config"))]
    {
        let id_watch = id.clone();
        use_effect(move || {
            let cmd = UI_CMD.read().clone();
            if let Some((target_id, ref command)) = cmd {
                if target_id == id_watch {
                    // Consume the command
                    *UI_CMD.write() = None;

                    match command {
                        UiCommand::Invoke => {
                            is_highlighted.set(true);
                            on_click.call(());
                            spawn(async move {
                                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                                is_highlighted.set(false);
                            });
                        }
                        UiCommand::Highlight { duration_ms } => {
                            is_highlighted.set(true);
                            let ms = *duration_ms;
                            spawn(async move {
                                tokio::time::sleep(std::time::Duration::from_millis(ms as u64)).await;
                                is_highlighted.set(false);
                            });
                        }
                        UiCommand::SetValue(_) => {
                            // Buttons don't have values — invoke instead
                            on_click.call(());
                        }
                    }
                }
            }
        });
    }

    let highlight_class = if *is_highlighted.read() { " cmx-amp-highlight" } else { "" };
    let extra = if class.is_empty() { String::new() } else { format!(" {class}") };
    let full_class = format!("cmx-amp-btn{highlight_class}{extra}");

    rsx! {
        document::Style { {AMP_BUTTON_CSS} }
        button {
            class: "{full_class}",
            disabled,
            onclick: move |_| on_click.call(()),
            "{label_clone}"
        }
    }
}
