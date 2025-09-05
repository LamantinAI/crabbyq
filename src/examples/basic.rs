use crabbyq::prelude::*;
use crabbyq::brokers::NatsBroker;
use tracing::info;

// Event handler
async fn handle_async_event(
    event: Event, 
    _state: ()
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!("🦀 Async handler got event: {}", event.subject);
    // Imitating some async work
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    info!("Async work done for: {}", event.subject);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initializing logging
    tracing_subscriber::fmt::init();

    // Connecting to NATS
    info!("🦀 Connecting to NATS...");
    let nats_client = async_nats::connect("nats://localhost:4222").await?;
    let nats_broker = NatsBroker::new(nats_client);

    // Creating our app
    let app = Router::new()
        .set_state(()) // Setting state
        .route("test.simple", handle_async_event) // add our handler
        .into_service(nats_broker);

    info!("🦀 CrabbyQ starting...");
    info!("🦀 Press Ctrl+C to stop");

    // Running the application
    app.serve().await?;

    info!("🦀 CrabbyQ stopped");
    Ok(())
}