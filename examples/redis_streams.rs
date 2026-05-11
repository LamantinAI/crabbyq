use crabbyq::brokers::RedisBroker;
use crabbyq::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::info;

#[derive(Debug, Deserialize, Serialize)]
struct Order {
    id: u32,
}

async fn handle_ephemeral(event: Event) -> CrabbyResult<()> {
    info!(
        "Ephemeral XREAD route received entry from '{}', payload size {} bytes",
        event.subject(),
        event.payload.len()
    );
    Ok(())
}

async fn handle_group(
    Headers(headers): Headers,
    Json(order): Json<Order>,
) -> CrabbyResult<()> {
    let redelivered = headers
        .as_ref()
        .and_then(|h| h.get("redis-redelivered"))
        .is_some();
    info!(
        "Consumer-group route received order id={} (redelivered={redelivered})",
        order.id
    );
    Ok(())
}

async fn handle_pubsub(event: Event) -> CrabbyResult<()> {
    let channel = event
        .headers()
        .and_then(|h| h.get("redis-channel"))
        .cloned()
        .unwrap_or_default();
    let body = String::from_utf8_lossy(&event.payload).to_string();
    info!("PSUBSCRIBE route caught channel='{channel}' body='{body}'");
    Ok(())
}

#[tokio::main]
async fn main() -> CrabbyResult<()> {
    tracing_subscriber::fmt::init();

    let client = redis::Client::open("redis://127.0.0.1:6379")?;
    let broker = RedisBroker::new(client);
    let publisher = RedisPublisher::new(broker.clone());

    // RedisRouter mixes plain pub/sub with Streams and PSUBSCRIBE routes.
    // Stream-backed routes need the broker to open their own connections, so
    // it must be bound with with_broker(...) before any x_* / psub_* route.
    let app = RedisRouter::new()
        .with_broker(broker.clone())
        .x_route("events.ephemeral", handle_ephemeral)
        // Consumer-group route with the full reliability stack:
        // - auto_claim re-delivers entries idle for more than 10s;
        // - max_deliveries caps the per-entry retry count at 5;
        // - dead_letter publishes a DeadLetterEvent stream entry to
        //   "events.orders.dead" when the cap is exceeded, then XACKs the
        //   original entry so it stops bouncing.
        // Re-deliveries carry the `redis-redelivered: 1` header and a
        // `redis-delivery-count: <n>` header so non-idempotent handlers can
        // react to repeated attempts.
        .x_group_route_with(
            RedisGroupRouteConfig::new("events.orders", "workers", "worker-1")
                .auto_claim(Duration::from_secs(10))
                .max_deliveries(5)
                .dead_letter("events.orders.dead"),
            handle_group,
        )
        .psub_route("events.pubsub.*", handle_pubsub)
        .into_service(broker)
        .with_graceful_shutdown(async {
            tokio::time::sleep(Duration::from_millis(1500)).await;
        });

    info!("Starting Redis Streams example...");
    let handle = tokio::spawn(app.serve());
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Ephemeral XREAD route: an entry written to "events.ephemeral".
    let id = publisher
        .xadd("events.ephemeral", "hello from ephemeral stream")
        .max_len_approx(1000)
        .await?;
    info!("Wrote ephemeral entry id={id}");

    // Consumer-group route: entries land in the pending list until XACK.
    publisher
        .xadd("events.orders", Json(Order { id: 7 }))
        .max_len_approx(1000)
        .await?;

    // PSUBSCRIBE route: any channel matching "events.pubsub.*" is dispatched
    // through the same handler, with the actual channel exposed as a header.
    publisher
        .publish("events.pubsub.alpha", "first pub/sub payload")
        .await?;
    publisher
        .publish("events.pubsub.beta", "second pub/sub payload")
        .await?;

    handle.await??;
    Ok(())
}
