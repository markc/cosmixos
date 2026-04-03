use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const DEFAULT_BASE_URL: &str = "http://localhost:11434";
const DEFAULT_MODEL: &str = "qwen3:30b-a3b-nt";

pub struct LlmClient {
    base_url: String,
    model: String,
    http: reqwest::Client,
}

impl LlmClient {
    pub fn new(base_url: Option<&str>, model: Option<&str>) -> Self {
        Self {
            base_url: base_url.unwrap_or(DEFAULT_BASE_URL).trim_end_matches('/').to_string(),
            model: model.unwrap_or(DEFAULT_MODEL).to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// Create from cosmix settings.toml [skills] section.
    pub fn from_config() -> Self {
        let cfg = cosmix_config::store::load().unwrap_or_default().skills;
        Self {
            base_url: cfg.llm_url.trim_end_matches('/').to_string(),
            model: cfg.llm_model,
            http: reqwest::Client::new(),
        }
    }

    /// Send a system + user prompt, get a text response back.
    /// Uses Ollama's /api/chat endpoint (OpenAI-compatible structure).
    pub async fn complete(&self, system: &str, user: &str) -> Result<String> {
        let body = ChatRequest {
            model: &self.model,
            messages: vec![
                ChatMessage {
                    role: "system",
                    content: system,
                },
                ChatMessage {
                    role: "user",
                    content: user,
                },
            ],
            stream: false,
        };

        let resp = self
            .http
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await
            .context("sending request to LLM")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("LLM returned {status}: {text}");
        }

        let chat_resp: ChatResponse = resp.json().await.context("parsing LLM response")?;
        Ok(chat_resp.message.content)
    }

    pub fn model(&self) -> &str {
        &self.model
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    stream: bool,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ChatResponse {
    message: ResponseMessage,
}

#[derive(Deserialize)]
struct ResponseMessage {
    content: String,
}
