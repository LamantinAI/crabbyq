use crabbyq::brokers::NatsBroker;
use crabbyq::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Deserialize)]
struct SumRequest {
    left: i32,
    right: i32,
}

#[derive(Serialize)]
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
    info!("🦀 Connecting to NATS...");
    let nats_client = async_nats::connect("nats://localhost:4222").await?;
    let nats_broker = NatsBroker::new(nats_client);

    // Creating our app with an RPC-style handler
    let app = Router::new()
        .route("rpc.sum", handle_sum)
        .into_service(nats_broker);

    info!("🦀 CrabbyQ starting...");
    info!("🦀 Press Ctrl+C to stop");

    // Running the application
    app.serve().await?;

    info!("🦀 CrabbyQ stopped");
    Ok(())
}
