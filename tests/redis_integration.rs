#![cfg(feature = "redis")]

use crabbyq::brokers::RedisBroker;
use crabbyq::prelude::*;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn unique_channel(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{prefix}.{nanos}")
}

#[tokio::test]
#[ignore = "requires a local Redis server on redis://127.0.0.1:6379"]
async fn redis_pubsub_round_trip_works() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = redis::Client::open("redis://127.0.0.1:6379")?;
    let broker = RedisBroker::new(client);
    let publisher = Publisher::new(broker.clone());
    let records = Arc::new(Mutex::new(Vec::new()));
    let channel = unique_channel("crabbyq.test.redis");
    let recorded = records.clone();

    let app = Router::new()
        .route(&channel, move |event: Event| {
            let recorded = recorded.clone();
            async move {
                let payload = String::from_utf8(event.payload.to_vec()).unwrap();
                recorded.lock().unwrap().push(payload);
                Ok::<(), CrabbyError>(())
            }
        })
        .into_service(broker);

    let handle = tokio::spawn(app.serve());
    tokio::time::sleep(Duration::from_millis(150)).await;

    publisher.publish(&channel, "hello from redis").await?;

    let started = tokio::time::Instant::now();
    loop {
        if records.lock().unwrap().as_slice() == ["hello from redis"] {
            break;
        }

        if started.elapsed() > Duration::from_secs(2) {
            panic!("timed out waiting for Redis pub/sub message");
        }

        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    handle.abort();
    let _ = handle.await;

    Ok(())
}
