use axum::{Router, extract::State, routing::get};
use brest::Brest;

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(get_handler))
}

async fn get_handler(State(state): State<AppState>) -> Brest<Vec<String>> {
    Brest::success(
        state
            .embedders
            .iter()
            .map(|embedder| embedder.name())
            .collect(),
    )
}
