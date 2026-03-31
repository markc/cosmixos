//! Script executor — runs Mix scripts via the hub.

use std::collections::HashMap;
use std::sync::Arc;

use crate::types::{Script, ScriptResult};

/// Execute a discovered Mix script.
pub async fn execute_script(
    script: &Script,
    service_name: &str,
    app_vars: &HashMap<String, String>,
    hub: Arc<cosmix_client::HubClient>,
) -> ScriptResult {
    match std::fs::read_to_string(&script.path) {
        Ok(source) => {
            crate::mix_runtime::execute_mix(&source, hub, service_name, app_vars).await
        }
        Err(e) => ScriptResult {
            rc: 10,
            body: None,
            error: Some(format!("Failed to read {}: {e}", script.path.display())),
        },
    }
}
