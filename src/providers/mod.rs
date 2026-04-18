//! Provider interfaces and implementations for text, image, and video generation.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub mod claude;
pub mod flux;
pub mod mock;
pub mod ollama;
pub mod runway;

pub const CLAUDE_OPUS_46: &str = "claude-opus-4-6";
pub const CLAUDE_SONNET_46: &str = "claude-sonnet-4-6";
pub const CLAUDE_HAIKU_45: &str = "claude-haiku-4-5-20251001";

#[derive(Debug, Clone, thiserror::Error)]
#[error("HTTP {status_code}: {message}")]
pub struct HttpError {
    pub status_code: u16,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Default)]
pub struct TextRequest {
    pub system_prompt: String,
    pub user_prompt: String,
    pub cacheable_prefix: String,
    pub model_id: String,
    pub max_tokens: u32,
    pub thinking_budget_tokens: u32,
    pub temperature: f32,
    pub json_mode: bool,
}

#[derive(Debug, Clone, Default)]
pub struct TextResponse {
    pub content: String,
    pub tokens_used: TokenUsage,
    pub model_id: String,
    pub provider_name: String,
}

#[async_trait]
pub trait TextProvider: Send + Sync {
    async fn generate_text(&self, req: &TextRequest) -> Result<TextResponse>;
}

#[derive(Debug, Clone, Default)]
pub struct ImageRequest {
    pub prompt: String,
    pub model: String,
    pub reference_images: Vec<String>,
    pub aspect_ratio: String,
    pub seed: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct ImageResponse {
    pub image_data: Vec<u8>,
    pub mime_type: String,
    pub provider_name: String,
}

#[async_trait]
pub trait ImageProvider: Send + Sync {
    async fn generate_image(&self, req: &ImageRequest) -> Result<ImageResponse>;
}

#[derive(Debug, Clone, Default)]
pub struct FlashImageRequest {
    pub prompt: String,
    pub width: u32,
    pub height: u32,
    pub seed: Option<i64>,
    pub output_format: String,
}

#[derive(Debug, Clone, Default)]
pub struct FlashImageWithInputRequest {
    pub prompt: String,
    pub input_image: String,
    pub width: u32,
    pub height: u32,
    pub seed: Option<i64>,
    pub output_format: String,
}

#[derive(Debug, Clone, Default)]
pub struct FlashImageResponse {
    pub image_bytes: Vec<u8>,
    pub mime_type: String,
    pub seed_used: Option<i64>,
    pub provider_name: String,
}

#[async_trait]
pub trait FlashImageProvider: Send + Sync {
    async fn generate_image(&self, req: &FlashImageRequest) -> Result<FlashImageResponse>;
    async fn generate_image_with_input(
        &self,
        req: &FlashImageWithInputRequest,
    ) -> Result<FlashImageResponse>;
}

#[derive(Debug, Clone, Default)]
pub struct VideoRequest {
    pub prompt: String,
    pub duration_secs: u32,
    pub model: String,
    pub aspect_ratio: String,
    pub input_image_url: String,
    pub input_video_url: String,
    pub reference_images: Vec<String>,
    pub seed: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct VideoResponse {
    pub video_data: Vec<u8>,
    pub mime_type: String,
    pub duration_secs: u32,
    pub provider_name: String,
}

#[async_trait]
pub trait VideoProvider: Send + Sync {
    async fn generate_video(&self, req: &VideoRequest) -> Result<VideoResponse>;
}

pub type TextProviderRef = Arc<dyn TextProvider>;
pub type ImageProviderRef = Arc<dyn ImageProvider>;
pub type VideoProviderRef = Arc<dyn VideoProvider>;
pub type FlashImageProviderRef = Arc<dyn FlashImageProvider>;

/// Classify whether an error is retryable based on HTTP status or context.
/// 5xx, 429, or network errors => retryable; 4xx => permanent.
pub fn is_retryable_error(err: &anyhow::Error) -> bool {
    if let Some(http) = err.downcast_ref::<HttpError>() {
        let s = http.status_code;
        return s >= 500 || s == 429 || s == 408;
    }
    // network/timeout/connection failures are retryable by default
    true
}
