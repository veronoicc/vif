use std::{collections::HashMap, time::Duration};

use axum::{Json, Router, body::Body, extract::State, http::{HeaderValue, Response}, response::IntoResponse, routing::post};
use brest::Brest;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{AppState, embedding::EmbeddingError, media::{self, IndexError}};

pub fn router() -> Router<AppState> {
    Router::new().route("/", post(post_handler))
}

#[derive(Deserialize)]
struct PostPayload {
    link: String,
    #[serde(default = "default_timeout")]
    timeout: u64,
}

fn default_timeout() -> u64 {
    10
}

#[derive(Serialize)]
struct PostResponse {
    uuid: Uuid,
    providers: HashMap<String, Vec<f32>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Gif {
    //pub id: Uuid,
    pub link: String,
    pub score: f32,
}

async fn post_handler(
    State(state): State<AppState>,
    Json(payload): Json<PostPayload>,
) -> Response<Body> {
    let (uuid, providers) = match media::index(
        &payload.link,
        Duration::from_secs(payload.timeout),
        &state.qdrant,
        &state.embedders,
    )
    .await
    {
        Ok(results) => results,
        Err(IndexError::Embedding(EmbeddingError::Ratelimit(retry_after))) => {
            let mut response = Brest::<(), ()>::fail_status("Please try again later!", StatusCode::TOO_MANY_REQUESTS).into_response();
            response.headers_mut().insert(reqwest::header::RETRY_AFTER, HeaderValue::from_str(&retry_after).unwrap());
            return response
        }
        Err(e) => return  Brest::<(), ()>::error(format!("An error occured: {e:?}")).into_response(),
    };

     Brest::<PostResponse, ()>::success(PostResponse { uuid, providers }).into_response()
}
