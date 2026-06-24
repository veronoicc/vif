use axum::Router;

use crate::AppState;

pub mod index;
pub mod links;
pub mod providers;
pub mod search;

pub fn router() -> Router<AppState> {
    Router::new()
        .nest("/index", index::router())
        .nest("/search", search::router())
        .nest("/providers", providers::router())
        .nest("/links", links::router())
}
