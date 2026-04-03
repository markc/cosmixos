//! Mix extension functions for dialog invocation.
//!
//! Registers dialog builtins that block the script until the user responds.
//! Returns structured Mix values (strings, bools, lists, maps) — not flat text.
//!
//! # Usage in Mix scripts
//!
//! ```mix
//! dialog_info("Build complete!")
//! $proceed = dialog_confirm("Deploy to production?")
//! $name = dialog_entry("What is your name?", "World")
//! $pass = dialog_password("Enter API key:")
//! $choice = dialog_combo("Pick one:", "alpha", "beta", "gamma")
//! $selected = dialog_checklist("Enable:", "logs:Logs:on", "metrics:Metrics:off")
//! $pick = dialog_radiolist("Mode:", "fast:Fast:on", "safe:Safe:off")
//! $body = dialog_text("Edit notes:", "default text here")
//! $fields = dialog_form("New user:", "name:Name:text", "email:Email:text", "admin:Admin:toggle")
//! ```

use std::future::Future;
use std::pin::Pin;

use indexmap::IndexMap;
use mix_core::error::MixResult;
use mix_core::evaluator::{Evaluator, ExtFn};
use mix_core::value::Value;

use cosmix_dialog::backend::blocking;
use cosmix_dialog::types::{FieldKind, FormField, ListItem, MessageLevel};
use cosmix_dialog::{DialogAction, DialogData, DialogKind, DialogRequest, DialogResult};

/// Register all dialog extension functions on a Mix evaluator.
pub fn register(eval: &mut Evaluator) {
    eval.register("dialog_info", make_dialog_info());
    eval.register("dialog_warning", make_dialog_warning());
    eval.register("dialog_error", make_dialog_error());
    eval.register("dialog_confirm", make_dialog_confirm());
    eval.register("dialog_entry", make_dialog_entry());
    eval.register("dialog_password", make_dialog_password());
    eval.register("dialog_combo", make_dialog_combo());
    eval.register("dialog_checklist", make_dialog_checklist());
    eval.register("dialog_radiolist", make_dialog_radiolist());
    eval.register("dialog_text", make_dialog_text());
    eval.register("dialog_form", make_dialog_form());
}

// ── dialog_info(text) ───���────────────────────────────────────────────

fn make_dialog_info() -> ExtFn {
    make_message_fn(MessageLevel::Info)
}

fn make_dialog_warning() -> ExtFn {
    make_message_fn(MessageLevel::Warning)
}

fn make_dialog_error() -> ExtFn {
    make_message_fn(MessageLevel::Error)
}

fn make_message_fn(level: MessageLevel) -> ExtFn {
    Box::new(move |args: Vec<Value>| -> Pin<Box<dyn Future<Output = MixResult<Value>>>> {
        let text = args.first().map(|v| v.to_string()).unwrap_or_default();
        let request = DialogRequest {
            kind: DialogKind::Message {
                text,
                level,
                detail: None,
            },
            title: None,
            size: None,
            timeout: 0,
            json_output: false,
            theme_dark: None,
        };
        Box::pin(async move {
            run_dialog(request);
            Ok(Value::Nil)
        })
    })
}

// ── dialog_confirm(text) → bool ────────────────────────────────────��─

fn make_dialog_confirm() -> ExtFn {
    Box::new(|args: Vec<Value>| -> Pin<Box<dyn Future<Output = MixResult<Value>>>> {
        let text = args.first().map(|v| v.to_string()).unwrap_or_default();
        let request = DialogRequest {
            kind: DialogKind::Question {
                text,
                yes_label: None,
                no_label: None,
                cancel: false,
            },
            title: None,
            size: None,
            timeout: 0,
            json_output: false,
            theme_dark: None,
        };
        Box::pin(async move {
            let result = run_dialog(request);
            Ok(Value::Bool(result.rc == 0))
        })
    })
}

// ── dialog_entry(text, [default]) → string ───────────────────────────

fn make_dialog_entry() -> ExtFn {
    Box::new(|args: Vec<Value>| -> Pin<Box<dyn Future<Output = MixResult<Value>>>> {
        let text = args.first().map(|v| v.to_string()).unwrap_or_default();
        let default = args.get(1).map(|v| v.to_string());
        let request = DialogRequest {
            kind: DialogKind::Entry {
                text,
                default,
                placeholder: None,
            },
            title: None,
            size: None,
            timeout: 0,
            json_output: false,
            theme_dark: None,
        };
        Box::pin(async move {
            let result = run_dialog(request);
            text_or_nil(&result)
        })
    })
}

// ── dialog_password(text) → string ───────────────────────────────────

fn make_dialog_password() -> ExtFn {
    Box::new(|args: Vec<Value>| -> Pin<Box<dyn Future<Output = MixResult<Value>>>> {
        let text = args.first().map(|v| v.to_string()).unwrap_or_default();
        let request = DialogRequest {
            kind: DialogKind::Password { text },
            title: None,
            size: None,
            timeout: 0,
            json_output: false,
            theme_dark: None,
        };
        Box::pin(async move {
            let result = run_dialog(request);
            text_or_nil(&result)
        })
    })
}

// ── dialog_combo(text, item1, item2, ...) → string ──────────────────

fn make_dialog_combo() -> ExtFn {
    Box::new(|args: Vec<Value>| -> Pin<Box<dyn Future<Output = MixResult<Value>>>> {
        let text = args.first().map(|v| v.to_string()).unwrap_or_default();
        let items: Vec<String> = args.iter().skip(1).map(|v| v.to_string()).collect();
        let request = DialogRequest {
            kind: DialogKind::ComboBox {
                text,
                items,
                default: None,
                editable: false,
            },
            title: None,
            size: None,
            timeout: 0,
            json_output: false,
            theme_dark: None,
        };
        Box::pin(async move {
            let result = run_dialog(request);
            text_or_nil(&result)
        })
    })
}

// ── dialog_checklist(text, "key:label:on/off", ...) → list ──────────

fn make_dialog_checklist() -> ExtFn {
    Box::new(|args: Vec<Value>| -> Pin<Box<dyn Future<Output = MixResult<Value>>>> {
        let text = args.first().map(|v| v.to_string()).unwrap_or_default();
        let items = parse_list_items(&args[1..]);
        let request = DialogRequest {
            kind: DialogKind::CheckList { text, items },
            title: None,
            size: None,
            timeout: 0,
            json_output: false,
            theme_dark: None,
        };
        Box::pin(async move {
            let result = run_dialog(request);
            if result.rc != 0 {
                return Ok(Value::Nil);
            }
            match result.data {
                DialogData::Selection(keys) => Ok(Value::List(
                    keys.into_iter().map(Value::String).collect(),
                )),
                _ => Ok(Value::List(vec![])),
            }
        })
    })
}

// ── dialog_radiolist(text, "key:label:on/off", ...) → string ────────

fn make_dialog_radiolist() -> ExtFn {
    Box::new(|args: Vec<Value>| -> Pin<Box<dyn Future<Output = MixResult<Value>>>> {
        let text = args.first().map(|v| v.to_string()).unwrap_or_default();
        let items = parse_list_items(&args[1..]);
        let request = DialogRequest {
            kind: DialogKind::RadioList { text, items },
            title: None,
            size: None,
            timeout: 0,
            json_output: false,
            theme_dark: None,
        };
        Box::pin(async move {
            let result = run_dialog(request);
            if result.rc != 0 {
                return Ok(Value::Nil);
            }
            match result.data {
                DialogData::Selection(keys) => Ok(keys
                    .into_iter()
                    .next()
                    .map(Value::String)
                    .unwrap_or(Value::Nil)),
                DialogData::Text(s) => Ok(Value::String(s)),
                _ => Ok(Value::Nil),
            }
        })
    })
}

// ── dialog_text(text, [default]) → string ───────────────────────────

fn make_dialog_text() -> ExtFn {
    Box::new(|args: Vec<Value>| -> Pin<Box<dyn Future<Output = MixResult<Value>>>> {
        let text = args.first().map(|v| v.to_string()).unwrap_or_default();
        let default = args.get(1).map(|v| v.to_string());
        let request = DialogRequest {
            kind: DialogKind::TextInput { text, default },
            title: None,
            size: None,
            timeout: 0,
            json_output: false,
            theme_dark: None,
        };
        Box::pin(async move {
            let result = run_dialog(request);
            text_or_nil(&result)
        })
    })
}

// ── dialog_form(text, "id:Label:kind", ...) → map ───────────────────
//
// Field spec format: "id:Label:kind[:extra]"
//   kind = text | password | number | toggle | select | textarea
//   select extra = comma-separated items, e.g. "role:Role:select:admin,user,guest"
//   number extra = "min,max,step", e.g. "age:Age:number:0,120,1"

fn make_dialog_form() -> ExtFn {
    Box::new(|args: Vec<Value>| -> Pin<Box<dyn Future<Output = MixResult<Value>>>> {
        let text = args.first().map(|v| v.to_string()).unwrap_or_default();
        let fields = parse_form_fields(&args[1..]);
        let request = DialogRequest {
            kind: DialogKind::Form { text, fields },
            title: None,
            size: None,
            timeout: 0,
            json_output: false,
            theme_dark: None,
        };
        Box::pin(async move {
            let result = run_dialog(request);
            if result.rc != 0 {
                return Ok(Value::Nil);
            }
            match result.data {
                DialogData::Form(map) => {
                    let mut m = IndexMap::new();
                    for (k, v) in map {
                        m.insert(k, Value::String(v));
                    }
                    Ok(Value::Map(m))
                }
                _ => Ok(Value::Map(IndexMap::new())),
            }
        })
    })
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Run a dialog on a background thread and block until complete.
fn run_dialog(request: DialogRequest) -> DialogResult {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = blocking::run_blocking(request);
        let _ = tx.send(result);
    });
    rx.recv().unwrap_or(DialogResult {
        action: DialogAction::Error("dialog thread panicked".into()),
        data: DialogData::None,
        rc: 10,
    })
}

/// Return Text data as Value::String, or Nil on cancel/error.
fn text_or_nil(result: &DialogResult) -> MixResult<Value> {
    match &result.data {
        DialogData::Text(s) => Ok(Value::String(s.clone())),
        _ if result.rc == 0 => Ok(Value::String(String::new())),
        _ => Ok(Value::Nil),
    }
}

/// Parse "key:label:on/off" strings into ListItems.
fn parse_list_items(args: &[Value]) -> Vec<ListItem> {
    args.iter()
        .map(|v| {
            let s = v.to_string();
            let parts: Vec<&str> = s.splitn(3, ':').collect();
            match parts.as_slice() {
                [key, label, state] => ListItem {
                    key: key.to_string(),
                    label: label.to_string(),
                    checked: *state == "on" || *state == "true",
                },
                [key, label] => ListItem {
                    key: key.to_string(),
                    label: label.to_string(),
                    checked: false,
                },
                [key] => ListItem {
                    key: key.to_string(),
                    label: key.to_string(),
                    checked: false,
                },
                _ => ListItem {
                    key: s.clone(),
                    label: s,
                    checked: false,
                },
            }
        })
        .collect()
}

/// Parse "id:Label:kind[:extra]" strings into FormFields.
fn parse_form_fields(args: &[Value]) -> Vec<FormField> {
    args.iter()
        .map(|v| {
            let s = v.to_string();
            let parts: Vec<&str> = s.splitn(4, ':').collect();
            let (id, label, kind) = match parts.as_slice() {
                [id, label, kind, ..] => (id.to_string(), label.to_string(), *kind),
                [id, label] => (id.to_string(), label.to_string(), "text"),
                [id] => (id.to_string(), id.to_string(), "text"),
                _ => (s.clone(), s.clone(), "text"),
            };
            let extra = parts.get(3).copied().unwrap_or("");
            let field_kind = match kind {
                "password" | "pw" => FieldKind::Password,
                "number" | "num" => {
                    let nums: Vec<f64> =
                        extra.split(',').filter_map(|s| s.parse().ok()).collect();
                    FieldKind::Number {
                        default: None,
                        min: nums.first().copied(),
                        max: nums.get(1).copied(),
                        step: nums.get(2).copied(),
                    }
                }
                "toggle" | "bool" => FieldKind::Toggle {
                    default: extra == "on" || extra == "true",
                },
                "select" => FieldKind::Select {
                    items: extra.split(',').map(|s| s.to_string()).collect(),
                    default: None,
                },
                "textarea" | "area" => FieldKind::TextArea {
                    default: if extra.is_empty() {
                        None
                    } else {
                        Some(extra.to_string())
                    },
                    rows: 4,
                },
                _ => FieldKind::Text {
                    default: if extra.is_empty() {
                        None
                    } else {
                        Some(extra.to_string())
                    },
                    placeholder: None,
                },
            };
            FormField {
                id,
                label,
                kind: field_kind,
                required: false,
                help: None,
            }
        })
        .collect()
}
