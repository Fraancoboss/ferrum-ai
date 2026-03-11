pub mod agent_mode;
pub mod config;
pub mod curated_skills;
pub mod db;
pub mod error;
pub mod local_models;
pub mod process;
pub mod routes;
pub mod state;

use std::sync::Arc;

use axum::Router;
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

use crate::{
    config::Config,
    curated_skills::ensure_curated_skill_catalog,
    db::Database,
    process::build_provider_map,
    state::{AppState, EventHub, default_provider_prefs},
};

pub async fn build_state(config: Config) -> anyhow::Result<AppState> {
    let config = Arc::new(config);
    let db = Database::connect(&config.database_url).await?;
    db.migrate().await?;
    db.ensure_default_workflow_templates().await?;
    ensure_curated_skill_catalog(&db).await?;

    Ok(AppState {
        config,
        db,
        providers: Arc::new(build_provider_map()),
        hub: Arc::new(EventHub::default()),
        provider_prefs: Arc::new(tokio::sync::RwLock::new(default_provider_prefs())),
    })
}

pub fn app(state: AppState) -> Router {
    let frontend_dir = state.config.frontend_dir.clone();
    let frontend_index = frontend_dir.join("index.html");

    Router::new()
        .nest("/api", routes::router(state))
        .fallback_service(
            ServeDir::new(frontend_dir).not_found_service(ServeFile::new(frontend_index)),
        )
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}

pub fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "gateway=info,tower_http=info".into()),
        )
        .init();
}
