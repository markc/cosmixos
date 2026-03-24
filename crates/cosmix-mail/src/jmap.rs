use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};

// --- Wire types ---

#[derive(Serialize)]
pub struct JmapRequest {
    pub using: Vec<String>,
    #[serde(rename = "methodCalls")]
    pub method_calls: Vec<(String, serde_json::Value, String)>,
}

#[derive(Deserialize, Debug)]
pub struct JmapResponse {
    #[serde(rename = "methodResponses")]
    pub method_responses: Vec<(String, serde_json::Value, String)>,
}

// --- Data types ---

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Mailbox {
    pub id: String,
    pub name: String,
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    pub role: Option<String>,
    #[serde(rename = "sortOrder")]
    pub sort_order: i32,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct EmailAddress {
    pub name: Option<String>,
    pub email: String,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Email {
    pub id: String,
    pub subject: Option<String>,
    pub from: Option<Vec<EmailAddress>>,
    pub to: Option<Vec<EmailAddress>>,
    pub date: Option<String>,
    pub preview: Option<String>,
    #[serde(rename = "receivedAt")]
    pub received_at: Option<String>,
    #[serde(rename = "hasAttachment")]
    pub has_attachment: Option<bool>,
    pub keywords: Option<serde_json::Value>,
    #[serde(rename = "bodyValues")]
    pub body_values: Option<serde_json::Map<String, serde_json::Value>>,
    #[serde(rename = "textBody")]
    pub text_body: Option<Vec<BodyPart>>,
    #[serde(rename = "htmlBody")]
    pub html_body: Option<Vec<BodyPart>>,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct BodyPart {
    #[serde(rename = "partId")]
    pub part_id: String,
    #[serde(rename = "type")]
    pub content_type: Option<String>,
}

impl Email {
    pub fn text_body_value(&self) -> Option<&str> {
        let body_values = self.body_values.as_ref()?;
        let text_parts = self.text_body.as_ref()?;
        let part = text_parts.first()?;
        let val = body_values.get(&part.part_id)?;
        val.get("value")?.as_str()
    }

    pub fn html_body_value(&self) -> Option<&str> {
        let body_values = self.body_values.as_ref()?;
        let html_parts = self.html_body.as_ref()?;
        let part = html_parts.first()?;
        let val = body_values.get(&part.part_id)?;
        val.get("value")?.as_str()
    }

    pub fn from_display(&self) -> String {
        self.from
            .as_ref()
            .and_then(|addrs| addrs.first())
            .map(|a| {
                if let Some(name) = &a.name {
                    if !name.is_empty() {
                        return name.clone();
                    }
                }
                a.email.clone()
            })
            .unwrap_or_default()
    }

    pub fn date_short(&self) -> String {
        self.date
            .as_deref()
            .or(self.received_at.as_deref())
            .and_then(|d| {
                chrono::DateTime::parse_from_rfc3339(d)
                    .ok()
                    .map(|dt| dt.format("%b %d %H:%M").to_string())
            })
            .unwrap_or_default()
    }

    pub fn is_read(&self) -> bool {
        self.keywords
            .as_ref()
            .and_then(|k| k.get("$seen"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }
}

// --- GetResponse wrapper ---

#[derive(Deserialize, Debug)]
pub struct GetResponse<T> {
    #[serde(rename = "accountId")]
    pub account_id: String,
    pub state: String,
    pub list: Vec<T>,
}

#[derive(Deserialize, Debug)]
pub struct QueryResponse {
    pub ids: Vec<String>,
    pub total: Option<u64>,
}

// --- Client ---

#[derive(Clone)]
pub struct JmapClient {
    http: Client,
    base_url: String,
    email: String,
    password: String,
}

impl JmapClient {
    pub fn new(base_url: &str, email: &str, password: &str) -> Result<Self> {
        let http = Client::builder()
            .danger_accept_invalid_certs(true)
            .build()?;
        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            email: email.to_string(),
            password: password.to_string(),
        })
    }

    async fn call(&self, method_calls: Vec<(String, serde_json::Value, String)>) -> Result<JmapResponse> {
        let req = JmapRequest {
            using: vec![
                "urn:ietf:params:jmap:core".into(),
                "urn:ietf:params:jmap:mail".into(),
            ],
            method_calls,
        };
        let resp = self
            .http
            .post(format!("{}/jmap", self.base_url))
            .basic_auth(&self.email, Some(&self.password))
            .json(&req)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!("JMAP request failed: {}", resp.status()));
        }
        Ok(resp.json().await?)
    }

    pub async fn mailboxes(&self) -> Result<Vec<Mailbox>> {
        let resp = self
            .call(vec![("Mailbox/get".into(), serde_json::json!({}), "m0".into())])
            .await?;
        let (_, data, _) = resp
            .method_responses
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("empty response"))?;
        let get: GetResponse<Mailbox> = serde_json::from_value(data)?;
        Ok(get.list)
    }

    pub async fn email_ids(&self, mailbox_id: &str) -> Result<Vec<String>> {
        let resp = self
            .call(vec![(
                "Email/query".into(),
                serde_json::json!({
                    "filter": { "inMailbox": mailbox_id },
                    "sort": [{ "property": "receivedAt", "isAscending": false }],
                    "limit": 50
                }),
                "q0".into(),
            )])
            .await?;
        let (_, data, _) = resp
            .method_responses
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("empty response"))?;
        let qr: QueryResponse = serde_json::from_value(data)?;
        Ok(qr.ids)
    }

    pub async fn emails(&self, ids: &[String], fetch_body: bool) -> Result<Vec<Email>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let mut args = serde_json::json!({ "ids": ids });
        if fetch_body {
            args["fetchTextBodyValues"] = serde_json::json!(true);
            args["fetchHTMLBodyValues"] = serde_json::json!(true);
        }
        let resp = self
            .call(vec![("Email/get".into(), args, "e0".into())])
            .await?;
        let (_, data, _) = resp
            .method_responses
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("empty response"))?;
        let get: GetResponse<Email> = serde_json::from_value(data)?;
        Ok(get.list)
    }
}
