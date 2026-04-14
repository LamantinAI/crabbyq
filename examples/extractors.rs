use crabbyq::brokers::NatsBroker;
use crabbyq::prelude::*;
use serde::Deserialize;
use tracing::info;

#[derive(Clone)]
struct AppState {
    app_name: &'static str,
}

#[derive(Clone)]
struct AppName(&'static str);

#[derive(Deserialize)]
struct JsonPayload {
    id: u32,
    label: String,
}

#[derive(Deserialize)]
struct CborPayload {
    score: f32,
    active: bool,
}

impl FromRef<AppState> for AppName {
    fn from_ref(input: &AppState) -> Self {
        Self(input.app_name)
    }
}

async fn handle_state(
    State(app_name): State<AppName>,
) -> CrabbyResult<()> {
    info!("State extractor resolved app name: {}", app_name.0);
    Ok(())
}

async fn handle_headers(
    Headers(headers): Headers,
) -> CrabbyResult<()> {
    let count = headers.as_ref().map_or(0, |headers| headers.len());
    info!("Headers extractor got {} headers", count);
    Ok(())
}

async fn handle_subject_and_body(
    Subject(subject): Subject,
    Body(body): Body,
) -> CrabbyResult<()> {
    info!("Subject extractor got route key: {}", subject);
    info!("Body extractor got {} bytes", body.len());
    Ok(())
}

async fn handle_headers_subject_and_body(
    Headers(headers): Headers,
    Subject(subject): Subject,
    Body(body): Body,
) -> CrabbyResult<()> {
    let count = headers.as_ref().map_or(0, |headers| headers.len());
    info!("Combined extractors got {} headers", count);
    info!("Combined extractors got route key: {}", subject);
    info!("Combined extractors got {} bytes", body.len());
    Ok(())
}

async fn handle_json(
    Json(payload): Json<JsonPayload>,
) -> CrabbyResult<()> {
    info!(
        "Json extractor parsed payload: id={}, label={}",
        payload.id,
        payload.label
    );
    Ok(())
}

async fn handle_cbor(
    Cbor(payload): Cbor<CborPayload>,
) -> CrabbyResult<()> {
    info!(
        "Cbor extractor parsed payload: score={}, active={}",
        payload.score,
        payload.active
    );
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

    // Creating shared application state
    let app_state = AppState { app_name: "crabbyq" };

    // Creating our app with extractor-based handlers
    let app = Router::new()
        .set_state(app_state)
        .route("app.state", handle_state)
        .route("event.headers", handle_headers)
        .route("payload.echo", handle_subject_and_body)
        .route("payload.rich", handle_headers_subject_and_body)
        .route("payload.json", handle_json)
        .route("payload.cbor", handle_cbor)
        .into_service(nats_broker);

    info!("CrabbyQ starting...");
    info!("Press Ctrl+C to stop");

    // Running the application
    app.serve().await?;

    info!("CrabbyQ stopped");
    Ok(())
}
