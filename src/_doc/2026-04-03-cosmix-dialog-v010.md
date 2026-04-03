# cosmix-dialog v0.1.0

**Date:** 2026-04-03

## Summary

First feature-complete release of the cosmix-dialog system. Dual-backend
(GTK layer-shell + Dioxus/WebKitGTK) with unified dark/light theming, full
Mix builtin coverage, and a kdialog/zenity-compatible CLI.

## What shipped

### Dual-backend rendering
- **Layer-shell backend** — compact native GTK3 dialogs via wlr-layer-shell.
  Bypasses cosmic-comp's 240px toplevel minimum. Handles: Message, Question,
  Entry, Password, ComboBox, Progress.
- **Dioxus backend** — full WebKitGTK rendering for complex types: CheckList,
  RadioList, TextInput, TextViewer, Form. Uses cosmix-lib-ui MenuBar + theme.
- **Auto-selection** — routes to layer-shell when Wayland + supported type +
  height < 240px. Falls back to Dioxus for everything else.

### Unified theming
- `--dark` / `--light` CLI flags (mutually exclusive, global).
- Layer-shell: split theme.rs into structure + dark/light colour blocks.
  Both palettes use the Tailwind gray scale matching cosmix-lib-ui.
- Dioxus: overrides THEME signal on startup from CLI flag.
- Default: dark for both backends (regardless of global cosmix config).

### Mix builtins (11 total)
Existing (6):
- `dialog_info(text)`, `dialog_warning(text)`, `dialog_error(text)` — fire-and-forget messages
- `dialog_confirm(text)` → bool
- `dialog_entry(text, [default])` → string or nil
- `dialog_password(text)` → string or nil

New (5):
- `dialog_combo(text, items...)` → string or nil
- `dialog_checklist(text, "key:label:on/off"...)` → list of selected keys or nil
- `dialog_radiolist(text, "key:label:on/off"...)` → string (selected key) or nil
- `dialog_text(text, [default])` → string or nil (multi-line editor)
- `dialog_form(text, "id:Label:kind[:extra]"...)` → map of id→value or nil

Form field spec supports: text, password/pw, number/num, toggle/bool,
select (comma-separated items), textarea/area.

### Bug fixes
- **gtk::init() before layer-shell detection** — `gtk_layer_is_supported()`
  requires GTK initialised; was being called too early in select_backend().
- **Backend type guard** — auto-selector now only routes the 6 types with
  actual layer-shell widget implementations; unsupported types go to Dioxus.
- **Signal borrow panic** — THEME.read() dropped before THEME.write() in
  Dioxus backend to avoid Dioxus signals AlreadyBorrowed error.

### Refactoring
- Extracted `run_dialog()` and `text_or_nil()` helpers in dialog_ext.rs,
  removing duplicated thread-spawn and result-matching from all builtins.
- Added `theme_dark: Option<bool>` to `DialogRequest` for theme propagation.

## Files changed

- `src/crates/cosmix-dialog/src/backend/mod.rs` — type guard + gtk::init fix
- `src/crates/cosmix-dialog/src/backend/dioxus_backend.rs` — theme override
- `src/crates/cosmix-dialog/src/backend/layer_backend.rs` — dark param
- `src/crates/cosmix-dialog/src/backend/blocking.rs` — dark default
- `src/crates/cosmix-dialog/src/layer/theme.rs` — dual dark/light CSS
- `src/crates/cosmix-dialog/src/cli.rs` — --dark/--light flags
- `src/crates/cosmix-dialog/src/lib.rs` — theme_dark field
- `src/crates/cosmix-lib-script/src/dialog_ext.rs` — 5 new builtins + refactor
- `src/crates/cosmix-lib-script/Cargo.toml` — indexmap dep
- `src/crates/cosmix-dialog/test_dialogs.sh` — CLI test script
