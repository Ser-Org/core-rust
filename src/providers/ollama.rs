use crate::providers::*;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub struct OllamaProvider {
    base_url: String,
    model_name: String,
    client: Client,
    log_interactions: bool,
}

impl OllamaProvider {
    pub fn new(base_url: String, model_name: String, log_interactions: bool) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(60 * 30))
            .build()
            .expect("reqwest client");
        Self { base_url, model_name, client, log_interactions }
    }
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    options: ChatOptions,
    #[serde(skip_serializing_if = "String::is_empty")]
    format: String,
}

#[derive(Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct ChatOptions {
    #[serde(skip_serializing_if = "is_zero_f32")]
    temperature: f32,
    #[serde(skip_serializing_if = "is_zero_u32")]
    num_predict: u32,
}

fn is_zero_f32(v: &f32) -> bool { *v == 0.0 }
fn is_zero_u32(v: &u32) -> bool { *v == 0 }

#[derive(Deserialize)]
struct ChatResponse {
    message: ChatMessage,
    #[serde(default)]
    prompt_eval_count: u32,
    #[serde(default)]
    eval_count: u32,
}

#[async_trait]
impl TextProvider for OllamaProvider {
    async fn generate_text(&self, req: &TextRequest) -> Result<TextResponse> {
        let user_content = if req.cacheable_prefix.is_empty() {
            req.user_prompt.clone()
        } else {
            format!("{}\n\n{}", req.cacheable_prefix, req.user_prompt)
        };
        let body = ChatRequest {
            model: self.model_name.clone(),
            messages: vec![
                ChatMessage { role: "system".into(), content: req.system_prompt.clone() },
                ChatMessage { role: "user".into(), content: user_content },
            ],
            stream: false,
            options: ChatOptions { temperature: req.temperature, num_predict: req.max_tokens },
            format: if req.json_mode { "json".into() } else { String::new() },
        };
        let url = format!("{}/api/chat", self.base_url);
        if self.log_interactions {
            tracing::info!(provider = "ollama", %url, model = %self.model_name, "llm_request");
        }
        let resp = self.client.post(&url).json(&body).send().await?;
        let status = resp.status();
        let raw = resp.text().await?;
        if !status.is_success() {
            return Err(anyhow::Error::from(HttpError {
                status_code: status.as_u16(),
                message: raw,
            }));
        }
        let r: ChatResponse = serde_json::from_str(&raw)
            .map_err(|e| anyhow!("ollama: decode: {} (raw={})", e, raw))?;
        let mut content = r.message.content;
        if req.json_mode {
            content = extract_json(&content);
        }
        Ok(TextResponse {
            content,
            tokens_used: TokenUsage {
                prompt_tokens: r.prompt_eval_count,
                completion_tokens: r.eval_count,
                total_tokens: r.prompt_eval_count + r.eval_count,
            },
            model_id: self.model_name.clone(),
            provider_name: "ollama".into(),
        })
    }
}

fn extract_json(content: &str) -> String {
    if serde_json::from_str::<serde_json::Value>(content).is_ok() {
        return content.to_string();
    }
    let re = Regex::new(r"(?s)(\{.+\}|\[.+\])").unwrap();
    if let Some(m) = re.find(content) {
        if serde_json::from_str::<serde_json::Value>(m.as_str()).is_ok() {
            return m.as_str().to_string();
        }
    }
    content.to_string()
}
