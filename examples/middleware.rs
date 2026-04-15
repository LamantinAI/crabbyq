use crabbyq::brokers::NatsBroker;
use crabbyq::prelude::*;
use crabbyq::response::HandlerOutcome;
use std::task::{Context, Poll};
use tower::{Layer, Service};
use tracing::info;

#[derive(Clone)]
struct LoggingLayer;

impl<S> Layer<S> for LoggingLayer {
    type Service = LoggingService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        LoggingService { inner }
    }
}

#[derive(Clone)]
struct LoggingService<S> {
    inner: S,
}

impl<S> Service<Event> for LoggingService<S>
where
    S: Service<Event, Response = HandlerOutcome, Error = CrabbyError> + Send,
    S::Future: Send + 'static,
{
    type Response = HandlerOutcome;
    type Error = CrabbyError;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, event: Event) -> Self::Future {
        info!("Middleware saw event on '{}'", event.subject());
        self.inner.call(event)
    }
}

async fn handle(event: Event) -> CrabbyResult<()> {
    info!("Handler received '{}'", event.subject());
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

    // Creating our app with a custom Tower middleware layer.
    let app = Router::new()
        .route("events.logs", handle)
        .layer(LoggingLayer)
        .into_service(nats_broker);

    info!("CrabbyQ starting...");
    info!("Publish a message with: nats pub events.logs 'hello'");
    info!("Press Ctrl+C to stop");

    // Running the application
    app.serve().await?;

    info!("CrabbyQ stopped");
    Ok(())
}
