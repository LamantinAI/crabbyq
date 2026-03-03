use crabbyq::brokers::NatsBroker;
use crabbyq::prelude::*;
use tracing::info;

async fn handle_async_event(
    event: Event,
    _state: (),
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!("NATS handler got event: {}", event.subject);
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    info!("NATS async work done for: {}", event.subject);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt::init();

    info!("Connecting to NATS...");
    let nats_client = async_nats::connect("nats://localhost:4222").await?;
    let nats_broker = NatsBroker::new(nats_client);

    let app = Router::new()
        .set_state(())
        .route("test.simple", handle_async_event)
        .into_service(nats_broker);

    info!("CrabbyQ (NATS) starting...");
    info!("Press Ctrl+C to stop");
    app.serve().await?;
    info!("CrabbyQ (NATS) stopped");

    Ok(())
}
