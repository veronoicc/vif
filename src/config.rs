use std::{
    net::{IpAddr, Ipv6Addr},
    sync::Arc,
};

use figment::{
    Figment,
    providers::{self, Format},
};
use serde::Deserialize;
use serde_with::OneOrMany;

pub fn base_figment() -> Figment {
    Figment::new()
        .admerge(providers::Json::file(
            std::env::var("CONFIG_JSON").unwrap_or_else(|_| "config.json".into()),
        ))
        .admerge(providers::Yaml::file(
            std::env::var("CONFIG_YAML").unwrap_or_else(|_| "config.yaml".into()),
        ))
        .admerge(providers::Toml::file(
            std::env::var("CONFIG").unwrap_or_else(|_| "config.toml".into()),
        ))
        .admerge(providers::Env::prefixed("VIF_").split("_"))
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub database: DatabaseConfig,
    #[serde(default)]
    pub api: ApiConfig,
    pub embedders: EmbeddersConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    #[serde(default = "default_database_url")]
    pub url: String,
}

fn default_database_url() -> String {
    "http://localhost:6334".to_string()
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: default_database_url(),
        }
    }
}

#[serde_with::serde_as]
#[derive(Debug, Deserialize, Clone)]
pub struct ApiConfig {
    #[serde(default = "default_api_host")]
    #[serde_as(as = "OneOrMany<_>")]
    pub host: Vec<IpAddr>,
    #[serde(default = "default_api_port")]
    pub port: u16,
}

fn default_api_host() -> Vec<IpAddr> {
    vec![Ipv6Addr::UNSPECIFIED.into()]
}

fn default_api_port() -> u16 {
    6335
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            host: default_api_host(),
            port: default_api_port(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct EmbeddersConfig {
    pub gemini: Option<EmbeddersGeminiConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EmbeddersGeminiConfig {
    pub api_key: String,
}

impl Config {
    pub fn load() -> Result<Arc<Self>, figment::Error> {
        base_figment().extract().map(|c| Arc::new(c))
    }
}
