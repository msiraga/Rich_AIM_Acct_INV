//! NexusLedger Main Application
//!
//! Entry point for the NexusLedger accounting platform.

use nexus_core::{NexusLedger, AgentOrchestrator};
use tracing::{info, error, Level};
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
    
    // Initialize the system
    nexus.initialize().await?;
    
    info!("NexusLedger initialized successfully!");
    info!("Agents loaded: {}", nexus.orchestrator.agents.len());

    // Start the agent orchestrator
    let orchestrator_arc = Arc::new(Mutex::new(nexus.orchestrator));
    
    // Here you would typically start the API server, CLI interface, or GUI
    // For now, we'll just keep the system running
    info!("System ready. Press Ctrl+C to exit.");

    // Keep the application running
    tokio::signal::ctrl_c().await?;
    
    info!("Shutting down NexusLedger...");
    
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
