use crabbyq::brokers::NatsBroker;
use crabbyq::prelude::*;
use tracing::info;

#[derive(Clone)]
struct AppState {
    app_name: &'static str,
}

impl AppState {
    fn new() -> Self {
        Self { app_name: "crabbyq" }
    }
}

async fn handle_async_event(
    event: Event,
    state: AppState,
) -> CrabbyResult<()> {
    info!(
        "🦀 Stateful handler in '{}' got event: {}",
        state.app_name,
        event.subject()
    );
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    info!("Stateful work done for: {}", event.subject());
    Ok(())
}

#[tokio::main]
async fn main() -> CrabbyResult<()> {
    // Initializing logging
    tracing_subscriber::fmt::init();

    // Connecting to NATS
    info!("🦀 Connecting to NATS...");
    let nats_client = async_nats::connect("nats://localhost:4222").await?;
    let nats_broker = NatsBroker::new(nats_client);

    // Creating shared application state
    let app_state = AppState::new();

    // Creating our app
    let app = Router::new()
        .set_state(app_state)
        .route("test.simple", handle_async_event)
        .into_service(nats_broker);

    info!("🦀 CrabbyQ starting...");
    info!("🦀 Press Ctrl+C to stop");

    // Running the application
    app.serve().await?;

    info!("🦀 CrabbyQ stopped");
    Ok(())
}
