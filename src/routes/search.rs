use std::{collections::HashMap, time::Duration};

use axum::{
    Router, body::Body, extract::{Query, State}, http::{HeaderValue, Response}, response::IntoResponse, routing::get,
};
use brest::Brest;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use crate::{AppState, embedding::EmbeddingError, media::{self, SearchError}};

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(get_handler))
}

#[derive(Deserialize)]
struct GetPayload {
    query: String,
    #[serde(default = "default_amount")]
    amount: u64,
    #[serde(default = "default_timeout")]
    timeout: u64,
}

fn default_amount() -> u64 {
    100
}

fn default_timeout() -> u64 {
    10
}

#[derive(Serialize)]
struct GetResponse {
    providers: HashMap<String, Vec<Gif>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Gif {
    //pub id: Uuid,
    pub link: String,
    pub score: f32,
}

async fn get_handler(
    State(state): State<AppState>,
    Query(payload): Query<GetPayload>,
) -> Response<Body> {
    let results = match media::search(
        payload.query,
        Duration::from_secs(payload.timeout),
        payload.amount,
        &state.qdrant,
        &state.embedders,
    )
    .await
    {
        Ok(results) => results,
        Err(SearchError::Embedding(EmbeddingError::Ratelimit(retry_after))) => {
            let mut response = Brest::<(), ()>::fail_status("Please try again later!", StatusCode::TOO_MANY_REQUESTS).into_response();
            response.headers_mut().insert(reqwest::header::RETRY_AFTER, HeaderValue::from_str(&retry_after).unwrap());
            return response
        }
        Err(e) => return Brest::<(), ()>::error(format!("An error occured: {e:?}")).into_response(),
    };

    Brest::<GetResponse, ()>::success(GetResponse {
        providers: results
            .into_iter()
            .map(|(provider, value)| {
                (
                    provider,
                    value
                        .into_iter()
                        .map(|(link, score)| Gif { link, score })
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<HashMap<_, _>>(),
    }).into_response()
}
