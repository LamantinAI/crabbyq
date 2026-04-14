use crabbyq::brokers::NatsBroker;
use crabbyq::prelude::*;
use serde::Serialize;
use tracing::info;

#[derive(Serialize)]
struct ShutdownEvent {
    service: &'static str,
    status: &'static str,
}

async fn handle_ping(event: Event) -> CrabbyResult<()> {
    info!("Received event on '{}'", event.subject());
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

    // Creating our app with a shutdown hook. Press Ctrl+C to trigger graceful
    // shutdown: the service will stop taking new messages, publish the final
    // event, and only then finish.
    let app = Router::new()
        .route("service.ping", handle_ping)
        .into_service(nats_broker)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to listen for Ctrl+C");
        })
        .on_shutdown(|publisher| async move {
            info!("Publishing final shutdown event...");
            publisher
                .publish(
                    "service.shutdown",
                    Json(ShutdownEvent {
                        service: "graceful_shutdown",
                        status: "stopping",
                    }),
                )
                .await?;
            Ok(())
        });

    info!("CrabbyQ starting...");
    info!("Send messages to 'service.ping' and press Ctrl+C to stop");
    info!("Example request: nats pub service.ping 'ping'");

    // Running the application
    app.serve().await?;

    info!("CrabbyQ stopped");
    Ok(())
}
