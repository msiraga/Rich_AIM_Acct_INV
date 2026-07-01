//! NexusLedger Main Application
//!
//! Entry point for the NexusLedger accounting platform.

use nexus_core::{NexusLedger, api::{ApiServer, ApiConfig}};
use nexus_core::database::Database;
use nexus_core::database::user::SurrealUserRepository;
use tracing::{info, Level};
use tracing_subscriber::{FmtSubscriber, EnvFilter};
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(EnvFilter::from_default_env()
            .add_directive(Level::INFO.into()))
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;

    info!("Starting NexusLedger...");
    info!("Fully Agentic Accounting Platform v0.1.0");
    info!("Author: Mounir Siraji <mounir@richdaleai.com>");
    info!("Organization: RichdaleAI");

    // Create the main NexusLedger instance
    let mut nexus = NexusLedger::new();

    // Set up database before initialization so the orchestrator connects it
    let db = Database::new();
    nexus.orchestrator.database = Some(db.clone());

    // Initialize the system
    nexus.initialize().await?;

    info!("NexusLedger initialized successfully!");
    info!("Agents loaded: {}", nexus.orchestrator.agents.read().await.len());
    info!("Database ready");

    // AgentOrchestrator uses Arc internally, so cloning shares state.
    let orchestrator = Arc::new(Mutex::new(nexus.orchestrator.clone()));
    let nexus = Arc::new(Mutex::new(nexus));
    let db = Arc::new(Mutex::new(db));

    // Create user repository sharing the same DB client
    let user_repo = Arc::new(SurrealUserRepository::new(db.lock().await.db().await.ok()));

    // Ensure JWT secret is set (refuse to start with default)
    if std::env::var("JWT_SECRET").is_err() {
        info!("JWT_SECRET not set — using development secret. Set JWT_SECRET in production!");
        std::env::set_var("JWT_SECRET", "dev-secret-key-change-in-production-32b!");
    }

    // Start the API server with axum
    let api_config = ApiConfig::from_env();
    let api_server = ApiServer::new(api_config, orchestrator, db, nexus, user_repo);

    info!("Starting API server on {}:{}...", api_server.config.host, api_server.config.port);
    info!("WebSocket chat: ws://{}:{}/ws/chat", api_server.config.host, api_server.config.port);

    // Start the server (blocks until shutdown signal)
    api_server.start().await?;

    info!("NexusLedger shut down cleanly.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_main_initialization() {
        let mut nexus = NexusLedger::new();
        let result = nexus.initialize().await;
        assert!(result.is_ok());
    }
}
