use crabbyq::brokers::RedisBroker;
use crabbyq::prelude::*;
use tracing::info;

async fn handle_event(event: Event) -> CrabbyResult<()> {
    let payload = String::from_utf8(event.payload.to_vec())?;
    info!("Redis message on '{}': {}", event.subject(), payload);
    Ok(())
}

#[tokio::main]
async fn main() -> CrabbyResult<()> {
    tracing_subscriber::fmt::init();

    let client = redis::Client::open("redis://127.0.0.1:6379")?;
    let broker = RedisBroker::new(client);
    let publisher = RedisPublisher::new(broker.clone());

    let app = Router::new()
        .route("events.redis", handle_event)
        .into_service(broker)
        .with_graceful_shutdown(async {
            tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        });

    info!("Starting Redis pub/sub example...");
    let handle = tokio::spawn(app.serve());
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    publisher
        .publish("events.redis", "hello from Redis")
        .await?;

    handle.await??;
    Ok(())
}
