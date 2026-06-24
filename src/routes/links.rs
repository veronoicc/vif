use std::collections::HashMap;

use axum::{Router, extract::State, routing::get};
use brest::Brest;
use uuid::Uuid;

use crate::AppState;
use crate::vectors::get_links;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(get_handler))
}

async fn get_handler(State(state): State<AppState>) -> Brest<HashMap<Uuid, String>> {
    let links = match get_links(&state.qdrant).await {
        Ok(results) => results,
        Err(e) => return Brest::error(format!("An error occured: {e:?}")),
    };

    Brest::success(links)
}
