use crabbyq::brokers::NatsBroker;
use crabbyq::prelude::*;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use tracing::info;

#[derive(Deserialize)]
struct DivideRequest {
    left: i32,
    right: i32,
}

#[derive(Serialize)]
struct DivideResponse {
    result: i32,
}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
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
    info!("🦀 Connecting to NATS...");
    let nats_client = async_nats::connect("nats://localhost:4222").await?;
    let nats_broker = NatsBroker::new(nats_client);

    // Creating our app with a custom error response
    let app = Router::new()
        .route("rpc.divide", handle_divide)
        .into_service(nats_broker);

    info!("🦀 CrabbyQ starting...");
    info!("🦀 Press Ctrl+C to stop");

    // Running the application
    app.serve().await?;

    info!("🦀 CrabbyQ stopped");
    Ok(())
}
