use std::time::Duration;

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use qdrant_client::qdrant::{Distance, VectorParams, VectorParamsBuilder};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use serde_json::json;
use thiserror::Error;

use crate::config::EmbeddersGeminiConfig;

#[derive(Debug, Error)]
pub enum EmbeddingError {
    #[error("reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("api error: {0}")]
    Api(String),
    #[error("download media error: {0}")]
    DownloadMedia(String),
    #[error("ratelimited, retry after: {0:?}")]
    Ratelimit(Duration),
}

#[async_trait]
pub trait Embedder {
    fn name(&self) -> String;
    fn params(&self) -> VectorParams;

    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, EmbeddingError>;
    async fn embed_media(&self, media: &str) -> Result<Vec<f32>, EmbeddingError>;
}

pub struct GeminiEmbedder {
    client: Client,
    config: EmbeddersGeminiConfig,
}

#[derive(Deserialize)]
struct GeminiEmbedResponse {
    embedding: GeminiEmbedding,
}

#[derive(Deserialize)]
struct GeminiEmbedding {
    values: Vec<f32>,
}

#[async_trait]
impl Embedder for GeminiEmbedder {
    fn name(&self) -> String {
        "gemini-embedding-2".to_string()
    }

    fn params(&self) -> VectorParams {
        VectorParamsBuilder::new(3072, Distance::Cosine).build()
    }

    async fn embed_text(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        let api_url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-embedding-2:embedContent?key={}",
            self.config.api_key
        );

        let request_body = json!({
            "model": "models/gemini-embedding-2",
            "content": {
                "parts": [{ "text": text }]
            },
            "taskType": "RETRIEVAL_DOCUMENT",
            "outputDimensionality": 3072
        });

        Self::send_request(&self.client, &api_url, &request_body).await
    }

    async fn embed_media(&self, media: &str) -> Result<Vec<f32>, EmbeddingError> {
        let media_response = self.client.get(media).send().await?;

        if !media_response.status().is_success() {
            return Err(EmbeddingError::DownloadMedia(format!(
                "Failed to download media. HTTP Status: {}",
                media_response.status()
            )));
        }

        let media_bytes = media_response.bytes().await?;

        let mime_type = infer::get(&media_bytes)
            .map(|info| info.mime_type())
            .ok_or(EmbeddingError::DownloadMedia("Could not determine mime-type".to_string()))?;

        let base64_data = BASE64.encode(&media_bytes);

        let api_url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-embedding-2:embedContent?key={}",
            self.config.api_key
        );

        let request_body = json!({
            "model": "models/gemini-embedding-2",
            "content": {
                "parts": [{
                    "inlineData": {
                        "mimeType": mime_type, 
                        "data": base64_data
                    }
                }]
            },
            "taskType": "RETRIEVAL_DOCUMENT",
            "outputDimensionality": 3072
        });

        Self::send_request(&self.client, &api_url, &request_body).await
    }
}

impl GeminiEmbedder {
    pub fn new(config: EmbeddersGeminiConfig) -> Self {
        Self {
            client: Client::new(),
            config,
        }
    }

    async fn send_request(
        client: &Client,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<Vec<f32>, EmbeddingError> {
        let response = client.post(url).json(body).send().await?;

        if response.status() == StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|str_value| str_value.parse::<u64>().ok())
                .map(Duration::from_secs)
                .unwrap_or(Duration::from_secs(10));
            return Err(EmbeddingError::Ratelimit(retry_after))
        }

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(EmbeddingError::Api(format!(
                "Gemini API Error: {}",
                error_text
            )));
        }

        let resp_data: GeminiEmbedResponse = response.json().await?;

        Ok(resp_data.embedding.values)
    }
}
