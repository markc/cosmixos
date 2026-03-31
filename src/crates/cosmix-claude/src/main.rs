//! cosmix-claude — Claude AI AMP port daemon.
//!
//! Listens on `/run/user/{uid}/cosmix/ports/claude.sock` and exposes
//! Claude CLI operations as AMP commands:
//!
//!   ask      — Send a prompt to Claude, return the response
//!   analyze  — Analyze code or errors with Claude
//!   generate — Generate code for a given task
//!
//! Usage from Mix:
//!   send "claude" "ask" prompt="What is 2+2?"
//!   address "claude"
//!       ask prompt="Explain recursion"
//!       analyze code=$src error=$err
//!   end

use cosmix_amp::Port;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn call_claude(prompt: &str) -> Result<String, String> {
    let output = std::process::Command::new("claude")
        .args(["-p", prompt, "--output-format", "text"])
        .output()
        .map_err(|e| format!("Failed to run claude CLI: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("Claude error: {}", stderr.trim()))
    }
}

fn handle_ask(args: serde_json::Value) -> anyhow::Result<serde_json::Value> {
    let prompt = args.get("prompt")
        .and_then(|v| v.as_str())
        .or_else(|| args.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'prompt' argument"))?;

    let context = args.get("context").and_then(|v| v.as_str());

    let full_prompt = if let Some(ctx) = context {
        format!("Context: {ctx}\n\n{prompt}")
    } else {
        prompt.to_string()
    };

    match call_claude(&full_prompt) {
        Ok(response) => Ok(serde_json::json!({
            "response": response,
        })),
        Err(e) => anyhow::bail!("{e}"),
    }
}

fn handle_analyze(args: serde_json::Value) -> anyhow::Result<serde_json::Value> {
    let code = args.get("code").and_then(|v| v.as_str()).unwrap_or("");
    let error = args.get("error").and_then(|v| v.as_str()).unwrap_or("");
    let language = args.get("language").and_then(|v| v.as_str()).unwrap_or("Mix");

    if code.is_empty() && error.is_empty() {
        anyhow::bail!("Provide 'code' and/or 'error' arguments");
    }

    let mut prompt = format!("Analyze this {language} code issue. Be concise.\n\n");
    if !code.is_empty() {
        prompt.push_str(&format!("Code:\n```\n{code}\n```\n\n"));
    }
    if !error.is_empty() {
        prompt.push_str(&format!("Error: {error}\n"));
    }

    match call_claude(&prompt) {
        Ok(response) => Ok(serde_json::json!({
            "analysis": response,
        })),
        Err(e) => anyhow::bail!("{e}"),
    }
}

fn handle_generate(args: serde_json::Value) -> anyhow::Result<serde_json::Value> {
    let task = args.get("task")
        .and_then(|v| v.as_str())
        .or_else(|| args.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'task' argument"))?;

    let language = args.get("language").and_then(|v| v.as_str()).unwrap_or("Mix");

    let prompt = format!(
        "Generate {language} code for this task. Return only the code, no explanation.\n\nTask: {task}"
    );

    match call_claude(&prompt) {
        Ok(response) => Ok(serde_json::json!({
            "code": response,
        })),
        Err(e) => anyhow::bail!("{e}"),
    }
}

#[tokio::main]
async fn main() {
    let _log = cosmix_daemon::init_tracing("cosmix_claude");

    tracing::info!("Starting cosmix-claude AMP port");

    // Check claude CLI is available
    let has_claude = std::process::Command::new("which")
        .arg("claude")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !has_claude {
        tracing::error!("Claude CLI not found on PATH");
        eprintln!("cosmix-claude: requires Claude Code CLI (https://claude.ai/code)");
        std::process::exit(1);
    }

    let port = Port::new("claude")
        .command("ask", "Send a prompt to Claude and return the response", handle_ask)
        .command("analyze", "Analyze code or errors with Claude", handle_analyze)
        .command("generate", "Generate code for a given task", handle_generate)
        .standard_help()
        .standard_info("cosmix-claude", env!("CARGO_PKG_VERSION"));

    let _handle = match port.start() {
        Ok(h) => {
            tracing::info!("Claude port listening on {}", h.socket_path.display());
            println!("cosmix-claude listening on {}", h.socket_path.display());
            h
        }
        Err(e) => {
            tracing::error!("Failed to start port: {e}");
            eprintln!("cosmix-claude: failed to start: {e}");
            std::process::exit(1);
        }
    };

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await.ok();
    tracing::info!("Shutting down");
}
