use crabbyq::brokers::NatsBroker;
use crabbyq::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Deserialize, Serialize)]
struct SumRequest {
    left: i32,
    right: i32,
}

#[derive(Deserialize, Serialize)]
struct SumResponse {
    result: i32,
}

fn unique_subject(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{prefix}.{nanos}")
}

#[tokio::test]
#[ignore = "requires a local NATS server on nats://127.0.0.1:4222"]
async fn rpc_round_trip_works_with_real_nats() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = async_nats::connect("nats://127.0.0.1:4222").await?;
    let subject = unique_subject("crabbyq.test.rpc");

    let broker = NatsBroker::new(client.clone());
    let publisher = Publisher::new(broker.clone());
    let app = Router::new()
        .route(&subject, |Json(request): Json<SumRequest>| async move {
            Ok::<Json<SumResponse>, CrabbyError>(Json(SumResponse {
                result: request.left + request.right,
            }))
        })
        .into_service(broker);

    let handle = tokio::spawn(app.serve());
    tokio::time::sleep(Duration::from_millis(150)).await;

    let reply: SumResponse = tokio::time::timeout(
        Duration::from_secs(2),
        publisher.request(&subject, Json(SumRequest { left: 19, right: 23 })),
    )
    .await??
    .into_json()?;

    assert_eq!(reply.result, 42);

    handle.abort();
    let _ = handle.await;

    Ok(())
}
