use crabbyq::brokers::KafkaBroker;
use crabbyq::prelude::*;
use tracing::info;

async fn handle_async_event(
    event: Event,
    _state: (),
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!("Kafka handler got event: {}", event.subject);
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    info!("Kafka async work done for: {}", event.subject);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt::init();

    info!("Connecting to Kafka/Redpanda...");
    let kafka_broker = KafkaBroker::new("localhost:9092", "crabbyq-kafka-rpc")?;

    let app = Router::new()
        .set_state(())
        .route("test.simple", handle_async_event)
        .into_service(kafka_broker);

    info!("CrabbyQ (Kafka) starting...");
    info!("Press Ctrl+C to stop");
    app.serve().await?;
    info!("CrabbyQ (Kafka) stopped");

    Ok(())
}
