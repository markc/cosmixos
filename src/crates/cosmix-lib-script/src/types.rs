//! Core types for script definitions, execution context, and results.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A discovered Mix script with metadata and file path.
#[derive(Debug, Clone)]
pub struct Script {
    pub meta: ScriptMeta,
    pub path: PathBuf,
}

impl Script {
    pub fn meta(&self) -> &ScriptMeta {
        &self.meta
    }
}

/// Script metadata — name, optional shortcut, description.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptMeta {
    /// Display name in the User menu.
    pub name: String,
    /// Optional keyboard shortcut (e.g. "Ctrl+Shift+V").
    #[serde(default)]
    pub shortcut: Option<String>,
    /// Human-readable description (shown in tooltip or help).
    #[serde(default)]
    pub description: Option<String>,
}

/// Result of executing a script.
pub struct ScriptResult {
    /// AMP return code: 0=success, 5=warning, 10=error, 20=failure.
    pub rc: u8,
    /// Response body from the last successful step.
    pub body: Option<String>,
    /// Error message if execution failed.
    pub error: Option<String>,
}
