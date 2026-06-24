use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use chrono::{Datelike, Duration, TimeZone as _, Utc};
use chrono_tz::US::Pacific;
use qdrant_client::qdrant::{Distance, VectorParams, VectorParamsBuilder};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};
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
    Ratelimit(String),
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

#[derive(Debug, Deserialize)]
struct GeminiErrorEnvelope {
    error: GeminiErrorBody,
}

#[derive(Debug, Deserialize)]
struct GeminiErrorBody {
    details: Option<Vec<Value>>,
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
            let error_text = response.text().await.unwrap_or_default();

            let retry_after = parse_retry_after_from_429(
                &error_text,
            );

            return Err(EmbeddingError::Ratelimit(retry_after));
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

fn parse_retry_after_from_429(
    response_body: &str,
) -> String {
    if let Ok(envelope) = serde_json::from_str::<GeminiErrorEnvelope>(response_body) {
        if let Some(details) = envelope.error.details {
            let mut retry_delay: Option<String> = None;
            let mut hit_daily_quota = false;

            for detail in details {
                let detail_type = detail.get("@type").and_then(Value::as_str);

                if detail_type == Some("type.googleapis.com/google.rpc.QuotaFailure") {
                    if let Some(violations) = detail.get("violations").and_then(Value::as_array) {
                        for violation in violations {
                            if let Some(quota_id) = violation.get("quotaId").and_then(Value::as_str) {
                                if quota_id.contains("PerDay") {
                                    hit_daily_quota = true;
                                }
                            }
                        }
                    }
                }

                if detail_type == Some("type.googleapis.com/google.rpc.RetryInfo") {
                    if let Some(delay) = detail.get("retryDelay").and_then(Value::as_str) {
                        let delay = delay.trim();
                        if !delay.is_empty() {
                            retry_delay = Some(delay.to_string());
                        }
                    }
                }
            }

            if hit_daily_quota {
                return seconds_until_next_pacific_midnight();
            }

            if let Some(delay) = retry_delay {
                return delay;
            }
        }
    }

    "10".to_string()
}

fn seconds_until_next_pacific_midnight() -> String {
    let now_pt = Utc::now().with_timezone(&Pacific);
    let next_date = now_pt.date_naive() + Duration::days(1);
    let next_midnight_pt = Pacific
        .with_ymd_and_hms(next_date.year(), next_date.month(), next_date.day(), 0, 0, 0)
        .single()
        .expect("valid PT midnight");
    let secs = (next_midnight_pt.timestamp() - now_pt.timestamp()).max(1);
    secs.to_string()
}