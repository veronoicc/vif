use std::sync::Arc;

use axum::{extract::FromRef, serve::Listener as _};
use eyre::eyre;
use futures::future::try_join_all;
use qdrant_client::Qdrant;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tracing::debug;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::{
    config::Config,
    embedding::{Embedder, GeminiEmbedder},
    listener::MultiListener,
};

pub mod config;
pub mod embedding;
pub mod listener;
pub mod media;
pub mod routes;
pub mod vectors;

#[derive(Clone, FromRef)]
pub struct AppState {
    qdrant: Qdrant,
    embedders: Vec<Arc<dyn Embedder + Send + Sync>>,
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("{}=debug", env!("CARGO_CRATE_NAME")).into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::load()?;
    debug!("config loaded successfully");

    let listener = MultiListener::from(
        try_join_all(
            config
                .api
                .host
                .iter()
                .map(|&host| TcpListener::bind((host, config.api.port))),
        )
        .await?,
    );

    let mut embedders: Vec<Arc<dyn Embedder + Send + Sync>> = Vec::new();

    if let Some(config) = config.embedders.gemini.clone() {
        let embedder = GeminiEmbedder::new(config);
        embedders.push(Arc::new(embedder));
    }

    if embedders.is_empty() {
        return Err(eyre!("No embedders configured"));
    }

    let qdrant = Qdrant::from_url(&config.database.url).build()?;
    vectors::initialize(&qdrant, &embedders).await?;

    if let Ok(addrs) = listener.local_addr() {
        debug!("listening on {addrs:?}");
    }

    let router = routes::router()
        .layer(CorsLayer::permissive())
        .with_state(AppState { qdrant, embedders });
    Ok(axum::serve(listener, router).await?)
}
