use axum::Router;

use crate::AppState;

pub mod _user;
pub mod providers;

pub fn router() -> Router<AppState> {
    Router::new()
        .nest("/{users}", _user::router())
        .nest("/providers", providers::router())
}
