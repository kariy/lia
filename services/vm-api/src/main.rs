use std::sync::Arc;

use axum::{
    routing::{delete, get, post},
    Router,
};
use sqlx::postgres::PgPoolOptions;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod config;
mod db;
mod error;
mod handlers;
mod models;
mod qemu;
mod vsock;
mod ws;

use config::AppConfig;

pub struct AppState {
    pub db: sqlx::PgPool,
    pub config: AppConfig,
    pub vm_manager: qemu::VmManager,
    pub ws_registry: Arc<ws::WsRegistry>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load environment variables
    dotenvy::dotenv().ok();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "vm_api=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load configuration
    let config = AppConfig::load()?;

    // Connect to database
    let db = PgPoolOptions::new()
        .max_connections(config.database.max_connections)
        .connect(&config.database.url)
        .await?;

    // Run migrations
    sqlx::migrate!("./migrations").run(&db).await?;

    tracing::info!("Database connected and migrations applied");

    // Initialize VM manager
    let vm_manager = qemu::VmManager::new(config.clone());

    // Initialize WebSocket registry
    let ws_registry = Arc::new(ws::WsRegistry::new());

    // Create app state
    let state = Arc::new(AppState {
        db,
        config: config.clone(),
        vm_manager,
        ws_registry,
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Build router
    let app = Router::new()
        .route("/health", get(handlers::health_check))
        .route("/api/v1/tasks", post(handlers::create_task))
        .route("/api/v1/tasks", get(handlers::list_tasks))
        .route("/api/v1/tasks/:id", get(handlers::get_task))
        .route("/api/v1/tasks/:id", delete(handlers::delete_task))
        .route("/api/v1/tasks/:id/resume", post(handlers::resume_task))
        .route("/api/v1/tasks/:id/output", get(handlers::get_task_output))
        .route("/api/v1/tasks/:id/stream", get(handlers::ws_stream))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // Start server
    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Server listening on {}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}
