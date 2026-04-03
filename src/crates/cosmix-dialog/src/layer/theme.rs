//! GTK CSS theme for layer-shell dialogs.
//!
//! Supports dark and light modes. Uses concrete values (no CSS variables)
//! since this is native GTK, not WebKitGTK. Palette matches the Tailwind
//! gray scale used by cosmix-lib-ui's theme system.

use gtk::prelude::*;

/// Shared structural CSS — layout, spacing, typography, border-radius.
/// Colour-neutral: all colours come from the dark/light block.
const STRUCTURE_CSS: &str = r#"
window {
    background-color: transparent;
    font-family: system-ui, -apple-system, sans-serif;
    font-size: 14px;
}

.dialog-frame {
    border-radius: 0.75rem;
    border: 1px solid rgba(128, 128, 128, 0.3);
}

.dialog-body {
    padding: 0.75rem 1rem;
}

.dialog-label {
    font-size: 0.9375rem;
}

.dialog-detail {
    font-size: 0.8125rem;
    margin-top: 0.25rem;
}

.dialog-footer {
    padding: 0.5rem 1rem;
    border-top: 1px solid rgba(128, 128, 128, 0.25);
    border-radius: 0 0 0.75rem 0.75rem;
}

.btn-primary, .btn-secondary, .btn-danger {
    padding: 0.375rem 1.25rem;
    border-radius: 0.375rem;
    font-weight: 500;
    font-size: 0.875rem;
    border: none;
    min-width: 4.5rem;
}

entry {
    border: 1px solid rgba(128, 128, 128, 0.4);
    border-radius: 0.375rem;
    padding: 0.375rem 0.5rem;
    font-size: 0.875rem;
    min-height: 1.75rem;
}

entry:focus {
    border-color: #3b82f6;
    outline: none;
}

combobox button {
    border: 1px solid rgba(128, 128, 128, 0.4);
    border-radius: 0.375rem;
    padding: 0.375rem 0.5rem;
    font-size: 0.875rem;
}

progressbar trough {
    border-radius: 0.25rem;
    min-height: 0.5rem;
}

progressbar progress {
    background-color: #3b82f6;
    border-radius: 0.25rem;
    min-height: 0.5rem;
}
"#;

/// Dark mode colours — Tailwind gray-950/900/800 palette.
const DARK_CSS: &str = r#"
window { color: #f3f4f6; }
.dialog-frame { background-color: #030712; }
.dialog-label { color: #f3f4f6; }
.dialog-detail { color: #9ca3af; }
.dialog-footer { background-color: #111827; }

.btn-primary { background-color: #3b82f6; color: #ffffff; }
.btn-primary:hover { background-color: #2563eb; }
.btn-secondary { background-color: #374151; color: #e5e7eb; }
.btn-secondary:hover { background-color: #4b5563; }
.btn-danger { background-color: #dc2626; color: #ffffff; }
.btn-danger:hover { background-color: #b91c1c; }

entry { background-color: #1f2937; color: #f3f4f6; }
combobox button { background-color: #1f2937; color: #f3f4f6; }
progressbar trough { background-color: #1f2937; }

.icon-info { color: #3b82f6; }
.icon-warning { color: #f59e0b; }
.icon-error { color: #ef4444; }
"#;

/// Light mode colours — Tailwind white/gray-50/gray-100 palette.
const LIGHT_CSS: &str = r#"
window { color: #111827; }
.dialog-frame { background-color: #ffffff; }
.dialog-label { color: #111827; }
.dialog-detail { color: #6b7280; }
.dialog-footer { background-color: #f9fafb; }

.btn-primary { background-color: #3b82f6; color: #ffffff; }
.btn-primary:hover { background-color: #2563eb; }
.btn-secondary { background-color: #e5e7eb; color: #1f2937; }
.btn-secondary:hover { background-color: #d1d5db; }
.btn-danger { background-color: #dc2626; color: #ffffff; }
.btn-danger:hover { background-color: #b91c1c; }

entry { background-color: #f9fafb; color: #111827; }
combobox button { background-color: #f9fafb; color: #111827; }
progressbar trough { background-color: #e5e7eb; }

.icon-info { color: #2563eb; }
.icon-warning { color: #d97706; }
.icon-error { color: #dc2626; }
"#;

/// Load the dialog CSS into a GtkCssProvider and apply it screen-wide.
pub fn apply_theme(dark: bool) {
    let css = if dark {
        format!("{STRUCTURE_CSS}\n{DARK_CSS}")
    } else {
        format!("{STRUCTURE_CSS}\n{LIGHT_CSS}")
    };
    let provider = gtk::CssProvider::new();
    provider
        .load_from_data(css.as_bytes())
        .expect("failed to load dialog CSS");
    gtk::StyleContext::add_provider_for_screen(
        &gdk::Screen::default().expect("no default screen"),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
