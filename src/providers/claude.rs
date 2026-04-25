use crate::providers::*;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub struct ClaudeProvider {
    api_key: String,
    client: Client,
    base_url: String,
    default_model: String,
    log_interactions: bool,
}

impl ClaudeProvider {
    pub fn new(api_key: String, log_interactions: bool) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .expect("reqwest client");
        Self {
            api_key,
            client,
            base_url: "https://api.anthropic.com/v1".into(),
            default_model: CLAUDE_SONNET_46.into(),
            log_interactions,
        }
    }
}

#[derive(Serialize)]
struct MessagesRequest {
    model: String,
    max_tokens: u32,
    #[serde(skip_serializing_if = "String::is_empty")]
    system: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingConfig>,
    messages: Vec<Message>,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: Vec<ContentBlock>,
}

#[derive(Serialize, Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl>,
}

#[derive(Serialize, Deserialize)]
struct CacheControl {
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Serialize)]
struct ThinkingConfig {
    #[serde(rename = "type")]
    kind: String,
    budget_tokens: u32,
}

#[derive(Deserialize)]
struct MessagesResponse {
    #[serde(default)]
    content: Vec<ContentBlock>,
    #[serde(default)]
    model: String,
    #[serde(default)]
    usage: ApiUsage,
}

#[derive(Deserialize, Default)]
struct ApiUsage {
    input_tokens: u32,
    output_tokens: u32,
}

#[derive(Deserialize)]
struct ApiErrorResponse {
    #[serde(default)]
    error: ApiErrorDetail,
}

#[derive(Deserialize, Default)]
struct ApiErrorDetail {
    #[serde(default)]
    message: String,
}

#[async_trait]
impl TextProvider for ClaudeProvider {
    async fn generate_text(&self, req: &TextRequest) -> Result<TextResponse> {
        let max_tokens = if req.max_tokens == 0 {
            4096
        } else {
            req.max_tokens
        };
        let model = if req.model_id.is_empty() {
            &self.default_model
        } else {
            &req.model_id
        };

        let mut user_content = Vec::with_capacity(2);
        if !req.cacheable_prefix.is_empty() {
            user_content.push(ContentBlock {
                kind: "text".into(),
                text: req.cacheable_prefix.clone(),
                cache_control: Some(CacheControl {
                    kind: "ephemeral".into(),
                }),
            });
        }
        user_content.push(ContentBlock {
            kind: "text".into(),
            text: req.user_prompt.clone(),
            cache_control: None,
        });

        let body = MessagesRequest {
            model: model.clone(),
            max_tokens,
            system: req.system_prompt.clone(),
            thinking: if req.thinking_budget_tokens > 0 {
                Some(ThinkingConfig {
                    kind: "enabled".into(),
                    budget_tokens: req.thinking_budget_tokens,
                })
            } else {
                None
            },
            messages: vec![Message {
                role: "user".into(),
                content: user_content,
            }],
        };

        let url = format!("{}/messages", self.base_url);
        if self.log_interactions {
            tracing::info!(provider = "claude", %url, %model, "llm_request");
        }

        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "prompt-caching-2024-07-31")
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let raw = resp.text().await?;
        if !status.is_success() {
            let parsed: Option<ApiErrorResponse> = serde_json::from_str(&raw).ok();
            let msg = parsed
                .map(|p| p.error.message)
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| raw.clone());
            return Err(anyhow::Error::from(HttpError {
                status_code: status.as_u16(),
                message: msg,
            }));
        }

        let api_resp: MessagesResponse = serde_json::from_str(&raw)
            .map_err(|e| anyhow!("claude: decode response: {} (raw={})", e, raw))?;

        let mut content = String::new();
        for b in api_resp.content {
            if b.kind == "text" {
                content.push_str(&b.text);
            }
        }
        let text = if req.json_mode {
            strip_markdown_fences(&content)
        } else {
            content
        };

        let model_resp = if api_resp.model.is_empty() {
            model.clone()
        } else {
            api_resp.model
        };
        let total = api_resp.usage.input_tokens + api_resp.usage.output_tokens;

        Ok(TextResponse {
            content: text,
            tokens_used: TokenUsage {
                prompt_tokens: api_resp.usage.input_tokens,
                completion_tokens: api_resp.usage.output_tokens,
                total_tokens: total,
            },
            model_id: model_resp,
            provider_name: "claude".into(),
        })
    }
}

pub fn strip_markdown_fences(s: &str) -> String {
    let mut s = s.trim().to_string();
    if s.starts_with("```json") {
        s = s.trim_start_matches("```json").to_string();
    } else if s.starts_with("```") {
        s = s.trim_start_matches("```").to_string();
    }
    if let Some(idx) = s.rfind("```") {
        s.truncate(idx);
    }
    s.trim().to_string()
}
