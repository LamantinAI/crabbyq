use crabbyq::brokers::NatsBroker;
use crabbyq::prelude::*;
use tracing::info;

async fn handle_user_created(event: Event) -> CrabbyResult<()> {
    info!("User router got event: {}", event.subject());
    Ok(())
}

async fn handle_billing_event(event: Event) -> CrabbyResult<()> {
    info!("Billing router got event: {}", event.subject());
    Ok(())
}

fn user_router() -> Router {
    Router::new().route("user.created", handle_user_created)
}

fn billing_router() -> Router {
    Router::new().routes(["invoice.paid", "invoice.refunded"], handle_billing_event)
}

#[tokio::main]
async fn main() -> CrabbyResult<()> {
    // Initializing logging
    tracing_subscriber::fmt::init();

    // Connecting to NATS
    info!("Connecting to NATS...");
    let nats_client = async_nats::connect("nats://localhost:4222").await?;
    let nats_broker = NatsBroker::new(nats_client);

    // Creating modular routers
    let users = user_router();
    let billing = billing_router();

    // Creating our app from several routers
    let app = Router::new()
        .include(users)
        .include(billing)
        .into_service(nats_broker);

    info!("CrabbyQ starting...");
    info!("Press Ctrl+C to stop");

    // Running the application
    app.serve().await?;

    info!("CrabbyQ stopped");
    Ok(())
}
