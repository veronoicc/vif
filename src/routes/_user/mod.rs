use axum::Router;

use crate::AppState;

pub mod index;
pub mod links;
pub mod search;

pub fn router() -> Router<AppState> {
    Router::new()
        .nest("/index", index::router())
        .nest("/search", search::router())
        .nest("/links", links::router())
}
