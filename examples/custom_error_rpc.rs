use crabbyq::brokers::NatsBroker;
use crabbyq::prelude::*;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::time::Duration;
use tracing::info;

#[derive(Deserialize, Serialize)]
struct DivideRequest {
    left: i32,
    right: i32,
}

#[derive(Deserialize, Serialize)]
struct DivideResponse {
    result: i32,
}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
}

#[derive(Deserialize)]
struct ErrorReply {
    error: String,
}

enum DivideError {
    DivisionByZero,
}

impl Display for DivideError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DivisionByZero => write!(f, "division by zero"),
        }
    }
}

impl IntoResponse for DivideError {
    fn into_response(self) -> Result<HandlerResponse, CrabbyError> {
        match self {
            Self::DivisionByZero => Json(ErrorBody {
                error: "division_by_zero",
            })
            .into_response(),
        }
    }
}

async fn handle_divide(
    Json(payload): Json<DivideRequest>,
) -> Result<Json<DivideResponse>, DivideError> {
    if payload.right == 0 {
        return Err(DivideError::DivisionByZero);
    }

    Ok(Json(DivideResponse {
        result: payload.left / payload.right,
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

    // Creating our app with a custom error response
    let app = Router::new()
        .route("rpc.divide", handle_divide)
        .into_service(nats_broker)
        .with_graceful_shutdown(async {
            tokio::time::sleep(Duration::from_millis(700)).await;
        });

    info!("Starting RPC service...");
    let handle = tokio::spawn(app.serve());

    // Give the subscription a moment to start before sending the requests.
    tokio::time::sleep(Duration::from_millis(100)).await;

    let success = publisher
        .request("rpc.divide", Json(DivideRequest { left: 10, right: 2 }))
        .await?;
    let success: DivideResponse = success.into_json()?;
    info!("Successful division result: {}", success.result);

    let failure = publisher
        .request("rpc.divide", Json(DivideRequest { left: 10, right: 0 }))
        .await?;
    let failure: ErrorReply = failure.into_json()?;
    info!("Error reply: {}", failure.error);

    handle.await??;

    info!("CrabbyQ stopped");
    Ok(())
}
