# 🦀 CrabbyQ

CrabbyQ is a declarative, asynchronous framework for building message-driven microservices in Rust.

Inspired by the elegance of **Axum** and the functional power of **Faststream**, CrabbyQ aims to make event routing as intuitive as HTTP routing while maintaining strict type safety and high performance.

## Overview

CrabbyQ abstracts away the complexity of message broker interactions, allowing you to focus on business logic. It leverages the **Tower** ecosystem, making it compatible with a wide range of middleware for logging, retries, and load shedding.

### Key Technical Pillars

* **Stateful Routing**: Uses type-level markers to ensure application state is correctly initialized before the service starts.
* **Declarative API**: Inspired by Axum, providing a familiar and clean syntax for defining message handlers and managing shared resources.
* **Tower Integration**: Every route is a `tower::Service`, enabling seamless middleware composition.

## Usage Example

```rust
use crabbyq::prelude::*;
use crabbyq::brokers::NatsBroker;
use tracing::info;

// Event handler
async fn handle_async_event(
    event: Event, 
    _state: ()
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!("🦀 Async handler got event: {}", event.subject);
    // Imitating some async work
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    info!("Async work done for: {}", event.subject);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initializing logging
    tracing_subscriber::fmt::init();

    // Connecting to NATS
    info!("🦀 Connecting to NATS...");
    let nats_client = async_nats::connect("nats://localhost:4222").await?;
    let nats_broker = NatsBroker::new(nats_client);

    // Creating our app
    let app = Router::new()
        .set_state(()) // Setting state
        .route("test.simple", handle_async_event) // add our handler
        .into_service(nats_broker);

    info!("🦀 CrabbyQ starting...");
    info!("🦀 Press Ctrl+C to stop");

    // Running the application
    app.serve().await?;

    info!("🦀 CrabbyQ stopped");
    Ok(())
}

```

## Run Locally

Start brokers:

```bash
docker compose up -d crabbyq-nats crabbyq-redpanda
```

Run NATS example:

```bash
cargo run --example nats_rpc
```

Run Kafka/Redpanda example:

```bash
cargo run --example kafka_rpc
```

## Architecture

CrabbyQ uses a specialized Builder pattern. The `Router` transitions through different states at compile-time (e.g., from `StateNotSet` to `StateSet`). This prevents common runtime errors where a handler attempts to access a state that hasn't been provided.

## Roadmap

The project is in its early stages (v0.1.0). Future development focuses on:

* **Unified Pub/Sub API**: Implementation of `.pub()` and `.sub()` methods to handle request-reply patterns automatically.
* **Multi-broker Support**: Adding RabbitMQ, Kafka, and Redis backends.
* **Extractor System**: Implementing an Axum-like extractor system for automatic payload deserialization (JSON, Protobuf, CBOR, MsgPack).
* **Advanced Middleware**: Built-in support for circuit breakers and dead-letter queues (DLQ).

## Contributing

We are looking for contributors to help shape the future of asynchronous messaging in Rust. If you are interested in "Axum for message brokers," we welcome your involvement through:

* Feature requests and API design discussions.
* Pull requests for new broker implementations.
* Documentation improvements.