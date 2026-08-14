use std::sync::Arc;

use clap::Parser;
use ring_intercom_bridge::{BridgeConfig, Runtime, router};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(version, about)]
struct Cli {}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    Cli::parse();
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = BridgeConfig::from_env().await?;
    let address = config.socket_address()?;
    let runtime = Arc::new(Runtime::new(config));
    let listener = TcpListener::bind(address).await?;
    tracing::info!(%address, "Ring Intercom bridge control plane listening");
    axum::serve(listener, router(runtime))
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn shutdown() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to install shutdown signal handler");
    }
}
