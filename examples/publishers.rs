use crabbyq::brokers::NatsBroker;
use crabbyq::prelude::*;
use serde::Serialize;
use tracing::info;

#[derive(Serialize)]
struct ProcessedEvent {
    source_subject: String,
}

async fn handle_publish(
    Publish(publisher): Publish,
    Subject(subject): Subject,
) -> CrabbyResult<()> {
    let mut headers = HeaderMap::new();
    headers.insert("x-source".to_string(), "crabbyq".to_string());

    publisher
        .publish(
            "events.processed",
            Json(ProcessedEvent {
                source_subject: subject,
            }),
        )
        .headers(headers)
        .await?;

    info!("Publish extractor emitted follow-up JSON event");
    Ok(())
}

#[tokio::main]
async fn main() -> CrabbyResult<()> {
    // Initializing logging
    tracing_subscriber::fmt::init();

    // Connecting to NATS
    info!("Connecting to NATS...");
    let nats_client = async_nats::connect("nats://localhost:4222").await?;
    let nats_broker = NatsBroker::new(nats_client);

    // Creating our app with a publisher extractor
    let app = Router::new()
        .route("events.publish", handle_publish)
        .into_service(nats_broker);

    info!("CrabbyQ starting...");
    info!("Press Ctrl+C to stop");

    // Running the application
    app.serve().await?;

    info!("CrabbyQ stopped");
    Ok(())
}
