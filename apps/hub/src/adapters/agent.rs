//! Agent HTTP client — Hub forwards chat messages to the intelligence layer.
//!
//! Graceful when Agent is down: callers get an error after retries; Hub stays up.

use serde_json::{json, Value};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("Agent URL is empty")]
    NotConfigured,
    #[error("Agent unreachable: {0}")]
    Unreachable(String),
    #[error("Agent API error ({status}): {body}")]
    Api { status: u16, body: String },
}

/// Optional P5 confirm + conversation context forwarded to Agent `/v1/chat`.
#[derive(Debug, Clone, Default)]
pub struct ChatOptions {
    pub confirm: bool,
    pub pending_action: Option<String>,
    pub conversation_history: Option<Value>,
}

#[derive(Clone)]
pub struct AgentClient {
    client: reqwest::Client,
    base_url: String,
    max_retries: u32,
}

impl AgentClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("reqwest client"),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            max_retries: 3,
        }
    }

    pub fn is_configured(&self) -> bool {
        !self.base_url.is_empty()
    }

    /// POST `{agent_url}/v1/chat` with retries on transport / 5xx.
    pub async fn chat(
        &self,
        message: &str,
        context: Option<Value>,
        options: ChatOptions,
    ) -> Result<Value, AgentError> {
        if !self.is_configured() {
            return Err(AgentError::NotConfigured);
        }

        let url = format!("{}/v1/chat", self.base_url);
        let mut ctx = context.unwrap_or_else(|| {
            json!({
                "devices": [],
                "scenes": [],
            })
        });
        if let Some(history) = options.conversation_history {
            if let Some(obj) = ctx.as_object_mut() {
                obj.insert("conversation_history".into(), history);
            }
        }

        let mut body = json!({
            "message": message,
            "context": ctx,
        });
        if options.confirm {
            body["confirm"] = json!(true);
        }
        if let Some(pending) = options.pending_action {
            body["pending_action"] = json!(pending);
        }

        let mut last_err = AgentError::Unreachable("unknown".into());
        for attempt in 1..=self.max_retries {
            match self
                .client
                .post(&url)
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await
            {
                Ok(resp) => {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    if status.is_server_error() && attempt < self.max_retries {
                        last_err = AgentError::Api {
                            status: status.as_u16(),
                            body: text,
                        };
                        tokio::time::sleep(backoff(attempt)).await;
                        continue;
                    }
                    if !status.is_success() {
                        return Err(AgentError::Api {
                            status: status.as_u16(),
                            body: text,
                        });
                    }
                    if text.trim().is_empty() {
                        return Ok(json!({ "reply": "", "status": "ok" }));
                    }
                    return serde_json::from_str(&text).or_else(|_| {
                        Ok(json!({ "reply": text, "status": "ok", "raw": true }))
                    });
                }
                Err(e) => {
                    last_err = AgentError::Unreachable(e.to_string());
                    tracing::warn!(
                        attempt,
                        max = self.max_retries,
                        error = %e,
                        "Agent chat request failed"
                    );
                    if attempt < self.max_retries {
                        tokio::time::sleep(backoff(attempt)).await;
                    }
                }
            }
        }
        Err(last_err)
    }
}

fn backoff(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_millis(200 * 2u64.pow(attempt.saturating_sub(1)))
}
