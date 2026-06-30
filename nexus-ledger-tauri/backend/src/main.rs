//! NexusLedger Tauri Backend
//!
//! This binary imports nexus-core and starts the real accounting API server.
//! It replaces the previous standalone mock backend.

use nexus_core::{NexusLedger, api::{ApiServer, ApiConfig}};
use nexus_core::database::Database;
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Simple logging (tracing not needed for Tauri backend)
    println!("Starting NexusLedger backend (Tauri)...");

    // Initialize NexusLedger with real accounting engine
    let mut nexus = NexusLedger::new();
    nexus.initialize().await?;
    println!("NexusLedger initialized with agents");

    let db = Database::new();
    println!("Database ready");

    let orchestrator = Arc::new(Mutex::new(nexus.orchestrator.clone()));
    let nexus = Arc::new(Mutex::new(nexus));
    let db = Arc::new(Mutex::new(db));

    // API config — Tauri frontend expects port 4000
    let api_config = ApiConfig::new("127.0.0.1", 4000);

    let api_server = ApiServer::new(api_config, orchestrator, db, nexus);
    api_server.start().await?;

    Ok(())
}
