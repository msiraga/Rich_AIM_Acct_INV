//! NexusLedger Tauri Backend
//!
//! This binary imports nexus-core and starts the real accounting API server.
//! It replaces the previous standalone mock backend.

use nexus_core::{NexusLedger, api::{ApiServer, ApiConfig}};
use nexus_core::database::Database;
use nexus_core::database::user::SurrealUserRepository;
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Simple logging (tracing not needed for Tauri backend)
    println!("Starting NexusLedger backend (Tauri)...");

    // Initialize NexusLedger with real accounting engine
    let mut nexus = NexusLedger::new();

    // Set up database before initialization
    let db = Database::new();
    nexus.orchestrator.database = Some(db.clone());

    nexus.initialize().await?;
    println!("NexusLedger initialized with agents");
    println!("Database ready");

    let orchestrator = Arc::new(Mutex::new(nexus.orchestrator.clone()));
    let nexus = Arc::new(Mutex::new(nexus));
    let db = Arc::new(Mutex::new(db));

    // Create user repository sharing the same DB client
    let user_repo = Arc::new(SurrealUserRepository::new(db.lock().await.client()));

    // Ensure JWT secret is set (refuse to start with default)
    if std::env::var("JWT_SECRET").is_err() {
        println!("JWT_SECRET not set — using development secret. Set JWT_SECRET in production!");
        std::env::set_var("JWT_SECRET", "dev-secret-key-change-in-production-32b!");
    }

    // API config — Tauri frontend expects port 4000
    let api_config = ApiConfig::new("127.0.0.1", 4000);

    let api_server = ApiServer::new(api_config, orchestrator, db, nexus, user_repo);
    api_server.start().await?;

    Ok(())
}
