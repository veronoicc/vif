use std::{collections::HashMap, sync::Arc, time::Duration};

use qdrant_client::qdrant::value::Kind;
use qdrant_client::qdrant::{SearchBatchPointsBuilder, SearchPointsBuilder};
use qdrant_client::{Qdrant, QdrantError};
use thiserror::Error;
use tokio::time::error::Elapsed;
use url::Url;
use uuid::Uuid;

use crate::embedding::{Embedder, EmbeddingError};
use crate::vectors::{get_media, get_query, insert_media, insert_query};

const UUID_NAMESPACE: Uuid = Uuid::from_u128(0x6ba7b811_9dad_11d1_80b4_00c04fd430c8);

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("Qdrant error: {0}")]
    Qdrant(#[from] QdrantError),
    #[error("Embedding error: {0}")]
    Embedding(#[from] EmbeddingError),
    #[error("Timeout error")]
    Timeout,
    #[error("ratelimited, retry after: {0:?}")]
    Ratelimit(Duration),
}

impl From<Elapsed> for SearchError {
    fn from(_: Elapsed) -> Self {
        SearchError::Timeout
    }
}

pub async fn search(
    text: String,
    timeout: Duration,
    limit: u64,
    qdrant: &Qdrant,
    embedders: &Vec<Arc<dyn Embedder + Send + Sync>>,
) -> Result<HashMap<String, Vec<(String, f32)>>, SearchError> {
    let uuid = Uuid::new_v5(&UUID_NAMESPACE, &text.as_bytes());

    let vectors = if let Some((vectors, _)) = get_query(&qdrant, &uuid).await? {
        vectors
    } else {
        let mut vectors = HashMap::new();
        for embedder in embedders {
            let vector = tokio::time::timeout(timeout, embedder.embed_text(&text)).await??;
            vectors.insert(embedder.name(), vector);
        }
        insert_query(&qdrant, &uuid, &vectors, &text).await?;
        vectors
    };

    let (names, search_points): (Vec<String>, Vec<_>) = vectors
        .into_iter()
        .map(|(name, vector)| {
            let point = SearchPointsBuilder::new("media", vector, limit)
                .vector_name(&name)
                .with_payload(true)
                .build();
            (name, point)
        })
        .unzip();

    let search = qdrant
        .search_batch_points(SearchBatchPointsBuilder::new("media", search_points))
        .await?;

    let results = names
        .into_iter()
        .zip(search.result)
        .map(|(name, response)| {
            let extracted_points = response
                .result
                .into_iter()
                .map(|point| {
                    let link = point
                        .payload
                        .get("link")
                        .and_then(|val| val.kind.as_ref())
                        .and_then(|kind| match kind {
                            Kind::StringValue(s) => Some(s.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();

                    (link, point.score)
                })
                .collect();

            (name, extracted_points)
        })
        .collect();

    Ok(results)
}

#[derive(Debug, Error)]
pub enum IndexError {
    #[error("Qdrant error: {0}")]
    Qdrant(#[from] QdrantError),
    #[error("Url parsing error: {0}")]
    Url(#[from] url::ParseError),
    #[error("Embedding error: {0}")]
    Embedding(#[from] EmbeddingError),
    #[error("Timeout error")]
    Timeout,
}

impl From<Elapsed> for IndexError {
    fn from(_: Elapsed) -> Self {
        IndexError::Timeout
    }
}

pub async fn index(
    media: &str,
    timeout: Duration,
    qdrant: &Qdrant,
    embedders: &Vec<Arc<dyn Embedder + Send + Sync>>,
) -> Result<(Uuid, HashMap<String, Vec<f32>>), IndexError> {
    let url = Url::parse(media)?;
    let uuid = Uuid::new_v5(
        &UUID_NAMESPACE,
        &strip_url_parameters(url.clone()).into_bytes(),
    );

    if let Some((vectors, _)) = get_media(&qdrant, &uuid).await? {
        Ok((uuid, vectors))
    } else {
        let mut vectors = HashMap::new();
        for embedder in embedders {
            let vector = tokio::time::timeout(timeout, embedder.embed_media(media)).await??;
            vectors.insert(embedder.name(), vector);
        }
        insert_media(&qdrant, &uuid, &vectors, &url.to_string()).await?;
        Ok((uuid, vectors))
    }
}

fn strip_url_parameters(mut url: Url) -> String {
    url.set_query(None);
    url.to_string()
}
