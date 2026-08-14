use std::{path::PathBuf, sync::Arc};

use clap::{Parser, Subcommand};
use ring_intercom_bridge::{
    BridgeConfig, Runtime, research::write_synthetic_discovery_fixture,
    ring_client::RingReadOnlyClient, router,
};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    Serve,
    Healthcheck,
    ResearchDiscover {
        #[arg(long)]
        session_file: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    match cli.command.unwrap_or(Command::Serve) {
        Command::Serve => serve().await,
        Command::Healthcheck => healthcheck().await,
        Command::ResearchDiscover {
            session_file,
            output,
        } => research_discover(session_file, output).await,
    }
}

async fn healthcheck() -> Result<(), Box<dyn std::error::Error>> {
    let port = std::env::var("RING_INTERCOM_BIND_PORT").unwrap_or_else(|_| "8775".into());
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(4))
        .build()?
        .get(format!("http://127.0.0.1:{port}/healthz"))
        .send()
        .await?;
    if !response.status().is_success() || response.content_length().unwrap_or(0) > 4096 {
        return Err("Ring Intercom bridge health endpoint is not ready".into());
    }
    Ok(())
}

async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let config = BridgeConfig::from_env().await?;
    let address = config.socket_address()?;
    let runtime = Arc::new(Runtime::new(config)?);
    let listener = TcpListener::bind(address).await?;
    tracing::info!(%address, "Ring Intercom bridge control plane listening");
    axum::serve(listener, router(runtime))
        .with_graceful_shutdown(shutdown())
        .await?;
    Ok(())
}

async fn research_discover(
    session_file: PathBuf,
    output: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = RingReadOnlyClient::new(session_file)?;
    let devices = client.discover_intercoms().await?;
    write_synthetic_discovery_fixture(&output, devices.len())?;
    tracing::info!(
        intercom_count = devices.len(),
        fixture = %output.display(),
        "sanitized Ring Intercom discovery fixture written"
    );
    Ok(())
}

async fn shutdown() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to install shutdown signal handler");
    }
}
