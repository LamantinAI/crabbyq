use crabbyq::brokers::NatsBroker;
use crabbyq::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::info;

#[derive(Deserialize, Serialize)]
struct SumRequest {
    left: i32,
    right: i32,
}

#[derive(Deserialize, Serialize)]
struct SumResponse {
    result: i32,
}

async fn handle_sum(
    Json(payload): Json<SumRequest>,
) -> CrabbyResult<Json<SumResponse>> {
    Ok(Json(SumResponse {
        result: payload.left + payload.right,
    }))
}

#[tokio::main]
async fn main() -> CrabbyResult<()> {
    // Initializing logging
    tracing_subscriber::fmt::init();

    // Connecting to NATS
    info!("Connecting to NATS...");
    let nats_client = async_nats::connect("nats://localhost:4222").await?;
    let nats_broker = NatsBroker::new(nats_client);
    let publisher = Publisher::new(nats_broker.clone());

    // Creating our app with an RPC-style handler
    let app = Router::new()
        .route("rpc.sum", handle_sum)
        .into_service(nats_broker)
        .with_graceful_shutdown(async {
            tokio::time::sleep(Duration::from_millis(500)).await;
        });

    info!("Starting RPC service...");
    let handle = tokio::spawn(app.serve());

    // Give the subscription a moment to start before sending the request.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Sending a request through the framework-managed RPC API.
    let reply = publisher
        .request("rpc.sum", Json(SumRequest { left: 20, right: 22 }))
        .await?;
    let response: SumResponse = reply.into_json()?;

    info!("RPC result: {}", response.result);

    handle.await??;

    info!("CrabbyQ stopped");
    Ok(())
}
