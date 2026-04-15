use crabbyq::brokers::NatsBroker;
use crabbyq::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::info;

#[derive(Debug, Deserialize, Serialize)]
struct CommandEvent {
    id: u32,
}

#[derive(Debug, Deserialize, Serialize)]
struct HelloEvent {
    name: &'static str,
}

async fn handle_add(Json(payload): Json<CommandEvent>) -> CrabbyResult<()> {
    info!(
        "JetStream route received add command with id={}",
        payload.id
    );
    Ok(())
}

async fn handle_remove(Json(payload): Json<CommandEvent>) -> CrabbyResult<()> {
    info!(
        "JetStream durable route received remove command with id={}",
        payload.id
    );
    Ok(())
}

async fn handle_plain(event: Event) -> CrabbyResult<()> {
    info!("Plain NATS route received '{}'", event.subject());
    Ok(())
}

#[tokio::main]
async fn main() -> CrabbyResult<()> {
    // Initializing logging
    tracing_subscriber::fmt::init();

    // Connecting to NATS
    info!("Connecting to NATS...");
    let nats_client = async_nats::connect("nats://localhost:4222").await?;
    let nats_broker = NatsBroker::new(nats_client.clone());
    let publisher = NatsPublisher::new(nats_broker.clone());

    // Building a JetStream stream with the native async-nats API.
    // CrabbyQ intentionally reuses that API instead of wrapping stream
    // management itself.
    let jetstream = async_nats::jetstream::new(nats_client);
    let commands_stream = jetstream
        .get_or_create_stream(async_nats::jetstream::stream::Config {
            name: "commands".to_string(),
            subjects: vec!["commands.*".to_string()],
            retention: async_nats::jetstream::stream::RetentionPolicy::WorkQueue,
            storage: async_nats::jetstream::stream::StorageType::Memory,
            ..Default::default()
        })
        .await?;

    // Clearing old messages keeps the example rerunnable, especially with
    // durable consumers.
    commands_stream.purge().await?;

    // Creating a mixed NATS router:
    // - js_route(...) uses JetStream with an ephemeral consumer
    // - js_durable_route(...) uses JetStream with an explicit durable
    // - route(...) stays a plain NATS subscription
    let app = NatsRouter::new()
        .jetstream(commands_stream)
        .js_route("commands.add", handle_add)
        .js_durable_route("commands.remove", "commands_remove", handle_remove)
        .route("hello.world", handle_plain)
        .into_service(nats_broker)
        .with_graceful_shutdown(async {
            tokio::time::sleep(Duration::from_millis(800)).await;
        });

    info!("Starting mixed NATS and JetStream service...");
    let handle = tokio::spawn(app.serve());

    // Give the subscriptions a moment to start before publishing.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Publishing messages into both JetStream-backed and plain NATS routes.
    // The NATS-specific publisher can speak both the core publish API and
    // JetStream-specific js_publish(...).
    let add_ack = publisher
        .js_publish("commands.add", Json(CommandEvent { id: 1 }))
        .await?;
    info!(
        "JetStream ack for commands.add -> stream={}, sequence={}",
        add_ack.stream, add_ack.sequence
    );

    publisher
        .js_publish("commands.remove", Json(CommandEvent { id: 2 }))
        .await?;
    publisher
        .publish("hello.world", Json(HelloEvent { name: "NATS" }))
        .await?;

    handle.await??;

    info!("CrabbyQ stopped");
    Ok(())
}
