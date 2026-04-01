//! cosmix-dialog — GUI dialog utility for the cosmix stack.
//!
//! Drop-in replacement for zenity/kdialog, built with Dioxus + dx-components.
//!
//! Usage:
//!   echo "hello" | cosmix-dialog text-info --title "Output"
//!   cosmix-dialog info --text "Done!" --title "Notice"
//!   cosmix-dialog error --text "Something failed"
//!   cosmix-dialog confirm --text "Delete this file?"    # exit 0=yes, 1=no
//!   cosmix-dialog input --text "Enter name:"            # prints input to stdout

use std::io::Read;
use std::sync::atomic::{AtomicI32, Ordering};

use clap::{Parser, Subcommand};
use cosmix_ui::app_init::use_theme_css;
use cosmix_ui::dx_components::button::{Button, ButtonVariant};
use cosmix_ui::dx_components::input::Input;
use dioxus::prelude::*;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// ── CLI ──

#[derive(Parser)]
#[command(name = "cosmix-dialog", about = "GUI dialog utility for cosmix")]
struct Cli {
    #[command(subcommand)]
    mode: Mode,
}

#[derive(Subcommand, Clone)]
enum Mode {
    /// Display scrollable text from stdin
    TextInfo {
        #[arg(long, default_value = "cosmix-dialog")]
        title: String,
        #[arg(long, default_value = "600")]
        width: u32,
        #[arg(long, default_value = "400")]
        height: u32,
    },
    /// Show an info message with OK button
    Info {
        #[arg(long)]
        text: String,
        #[arg(long, default_value = "Info")]
        title: String,
    },
    /// Show an error message with OK button
    Error {
        #[arg(long)]
        text: String,
        #[arg(long, default_value = "Error")]
        title: String,
    },
    /// Yes/No confirmation — exit 0 for yes, 1 for no
    Confirm {
        #[arg(long)]
        text: String,
        #[arg(long, default_value = "Confirm")]
        title: String,
    },
    /// Text input — prints entered text to stdout
    Input {
        #[arg(long, default_value = "")]
        text: String,
        #[arg(long, default_value = "Input")]
        title: String,
        /// Default value for the input field
        #[arg(long, default_value = "")]
        entry_text: String,
    },
}

// ── Shared state ──

static EXIT_CODE: AtomicI32 = AtomicI32::new(1);
static DIALOG_MODE: std::sync::OnceLock<Mode> = std::sync::OnceLock::new();
static DIALOG_TITLE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
static STDIN_TEXT: std::sync::OnceLock<String> = std::sync::OnceLock::new();

// ── Main ──

fn main() {
    cosmix_ui::desktop::init_linux_env();

    let cli = Cli::parse();

    if matches!(cli.mode, Mode::TextInfo { .. }) {
        let mut buf = String::new();
        let _ = std::io::stdin().read_to_string(&mut buf);
        STDIN_TEXT.set(buf).ok();
    }

    let (title, width, height) = match &cli.mode {
        Mode::TextInfo { title, width, height } => (title.clone(), *width, *height),
        Mode::Info { title, .. } => (title.clone(), 420, 170),
        Mode::Error { title, .. } => (title.clone(), 420, 170),
        Mode::Confirm { title, .. } => (title.clone(), 420, 170),
        Mode::Input { title, .. } => (title.clone(), 420, 170),
    };

    DIALOG_TITLE.set(title.clone()).ok();
    DIALOG_MODE.set(cli.mode).ok();

    #[cfg(feature = "desktop")]
    {
        let cfg = cosmix_ui::desktop::window_config(&title, width as f64, height as f64);
        LaunchBuilder::new().with_cfg(cfg).launch(app);
    }

    std::process::exit(EXIT_CODE.load(Ordering::Relaxed));
}

fn exit(code: i32) {
    EXIT_CODE.store(code, Ordering::Relaxed);
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::process::exit(code);
    });
}

// ── SVG Icons ──

const ICON_CLOSE: &str = r#"<svg style="width:0.75rem;height:0.75rem" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.5"><line x1="3" y1="3" x2="9" y2="9"/><line x1="9" y1="3" x2="3" y2="9"/></svg>"#;

const ICON_INFO: &str = r#"<svg style="width:1.5rem;height:1.5rem" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/></svg>"#;

const ICON_ERROR: &str = r#"<svg style="width:1.5rem;height:1.5rem" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="15" y1="9" x2="9" y2="15"/><line x1="9" y1="9" x2="15" y2="15"/></svg>"#;

const ICON_QUESTION: &str = r#"<svg style="width:1.5rem;height:1.5rem" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3"/><path d="M12 17h.01"/></svg>"#;

const ICON_EDIT: &str = r#"<svg style="width:1.5rem;height:1.5rem" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7"/><path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z"/></svg>"#;

// ── Dialog chrome CSS ──

const DIALOG_CSS: &str = r#"
/* Title bar — 1.75rem matches main app MenuBar height (28px) */
.dlg-titlebar {
    display: flex;
    align-items: center;
    height: 1.75rem;
    min-height: 1.75rem;
    background: var(--bg-secondary);
    border-bottom: 0.0625rem solid rgba(128,128,128,0.25);
    user-select: none;
}
.dlg-titlebar-drag {
    flex: 1;
    height: 100%;
    display: flex;
    align-items: center;
    padding-left: 1rem;
    cursor: grab;
}
.dlg-titlebar-drag:active { cursor: grabbing; }
.dlg-titlebar-title {
    font-size: 0.875rem;
    font-weight: 500;
    color: var(--fg-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
}
.dlg-close {
    width: 2.5rem;
    height: 1.75rem;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    color: var(--fg-muted);
    transition: background 0.15s, color 0.15s;
    border: none;
    background: transparent;
}
.dlg-close:hover {
    background: #ef4444;
    color: #fff;
}

/* Icon circles */
.dlg-icon-circle {
    flex-shrink: 0;
    width: 3rem;
    height: 3rem;
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
}
.dlg-icon-circle span {
    display: flex;
    align-items: center;
    justify-content: center;
    line-height: 0;
}
.dlg-icon-info {
    background: rgba(59, 130, 246, 0.12);
    color: #3b82f6;
}
.dlg-icon-error {
    background: rgba(239, 68, 68, 0.12);
    color: #ef4444;
}
.dlg-icon-warning {
    background: rgba(245, 158, 11, 0.12);
    color: #f59e0b;
}
.dlg-icon-input {
    background: rgba(139, 92, 246, 0.12);
    color: #8b5cf6;
}

/* Content */
.dlg-body {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 1.5rem;
    min-height: 0;
}
.dlg-message {
    font-size: 1rem;
    line-height: 1.5;
    color: var(--fg-primary);
}

/* Footer */
.dlg-footer {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 1rem;
    margin-bottom: 0.5rem;
    border-top: 0.0625rem solid rgba(128,128,128,0.25);
    background: var(--bg-secondary);
}
/* Smaller buttons for dialogs */
.dlg-footer .button {
    padding: 0.25rem 1rem;
    font-size: 0.75rem;
}

/* Input label */
.dlg-label {
    font-size: 0.875rem;
    font-weight: 500;
    color: var(--fg-secondary);
}

/* Scrollable text */
.dlg-pre {
    flex: 1;
    margin: 0;
    padding: 1rem 1.5rem;
    overflow: auto;
    font-family: 'JetBrains Mono', 'Fira Code', monospace;
    font-size: 1rem;
    line-height: 1.5;
    white-space: pre-wrap;
    word-wrap: break-word;
    min-height: 0;
    color: var(--fg-primary);
}
"#;

// ── Title bar component ──

#[component]
fn TitleBar() -> Element {
    let title = DIALOG_TITLE.get().map(|s| s.as_str()).unwrap_or("Dialog");

    rsx! {
        div { class: "dlg-titlebar",
            div {
                class: "dlg-titlebar-drag",
                onmousedown: move |_| {
                    let window = dioxus_desktop::use_window();
                    window.drag();
                },
                span { class: "dlg-titlebar-title", "{title}" }
            }
            div {
                class: "dlg-close",
                onclick: move |_| exit(1),
                title: "Close",
                span { dangerous_inner_html: ICON_CLOSE }
            }
        }
    }
}

// ── App root ──

fn app() -> Element {
    use_theme_css();
    let mode = DIALOG_MODE.get().unwrap();

    rsx! {
        document::Stylesheet { href: asset!("/assets/tailwind.css") }
        document::Style { {DIALOG_CSS} }

        div {
            class: "flex flex-col w-full h-screen bg-bg-primary text-fg-primary font-sans",
            TitleBar {}
            {match mode {
                Mode::TextInfo { .. } => rsx! { TextInfoView {} },
                Mode::Info { text, .. } => rsx! { MessageView { text: text.clone(), kind: "info" } },
                Mode::Error { text, .. } => rsx! { MessageView { text: text.clone(), kind: "error" } },
                Mode::Confirm { text, .. } => rsx! { ConfirmView { text: text.clone() } },
                Mode::Input { text, entry_text, .. } => rsx! { InputView { text: text.clone(), default: entry_text.clone() } },
            }}
        }
    }
}

// ── Text Info (scrollable stdin display) ──

#[component]
fn TextInfoView() -> Element {
    let text = STDIN_TEXT.get().map(|s| s.as_str()).unwrap_or("");

    rsx! {
        pre { class: "dlg-pre", "{text}" }
        div { class: "dlg-footer",
            Button {
                variant: ButtonVariant::Primary,
                onclick: move |_| exit(0),
                "OK"
            }
        }
    }
}

// ── Info / Error message ──

#[component]
fn MessageView(text: String, kind: String) -> Element {
    let (icon_svg, icon_class) = if kind == "error" {
        (ICON_ERROR, "dlg-icon-circle dlg-icon-error")
    } else {
        (ICON_INFO, "dlg-icon-circle dlg-icon-info")
    };

    rsx! {
        div { class: "dlg-body",
            div { class: "{icon_class}",
                span { dangerous_inner_html: icon_svg }
            }
            div { class: "dlg-message", "{text}" }
        }
        div { class: "dlg-footer",
            Button {
                variant: ButtonVariant::Primary,
                onclick: move |_| exit(0),
                "OK"
            }
        }
    }
}

// ── Confirm (Yes/No) ──

#[component]
fn ConfirmView(text: String) -> Element {
    rsx! {
        div { class: "dlg-body",
            div { class: "dlg-icon-circle dlg-icon-warning",
                span { dangerous_inner_html: ICON_QUESTION }
            }
            div { class: "dlg-message", "{text}" }
        }
        div { class: "dlg-footer",
            Button {
                variant: ButtonVariant::Outline,
                onclick: move |_| exit(1),
                "No"
            }
            Button {
                variant: ButtonVariant::Primary,
                onclick: move |_| exit(0),
                "Yes"
            }
        }
    }
}

// ── Input ──

#[component]
fn InputView(text: String, default: String) -> Element {
    let mut value = use_signal(|| default.clone());

    let submit = move |_| {
        print!("{}", value());
        exit(0);
    };

    rsx! {
        div { class: "dlg-body",
            div { class: "dlg-icon-circle dlg-icon-input",
                span { dangerous_inner_html: ICON_EDIT }
            }
            Input {
                class: "flex-1 min-w-0",
                value: "{value}",
                placeholder: "{text}",
                oninput: move |e: FormEvent| value.set(e.value()),
                onkeypress: move |e: KeyboardEvent| {
                    if e.key() == Key::Enter {
                        print!("{}", value());
                        exit(0);
                    }
                },
            }
        }
        div { class: "dlg-footer",
            Button {
                variant: ButtonVariant::Outline,
                onclick: move |_| exit(1),
                "Cancel"
            }
            Button {
                variant: ButtonVariant::Primary,
                onclick: submit,
                "OK"
            }
        }
    }
}
