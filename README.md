# 🦀 CrabbyQ

CrabbyQ is a declarative, asynchronous Rust framework for building message-driven microservices.

Inspired by the ergonomics of **Axum** and the messaging mindset of **FastStream**, CrabbyQ aims to make broker-driven services feel modular, typed, and easy to compose.

## Overview

CrabbyQ abstracts away broker subscription loops and route dispatching so you can focus on handlers and domain logic. Internally it is built around **Tower**, so routes are modeled as `tower::Service`s and the framework can grow naturally toward layers and middleware.

## Current Features

- `Router::new()` for stateless apps and `set_state(...)` for typed shared state.
- `route(...)` for a single route key and `routes(...)` for many route keys on one handler.
- `include(...)` for composing several routers into one service without imposing HTTP-like path nesting.
- `route_service(...)` for raw `tower::Service` integration.
- Axum-like extractors such as `State<T>`, `Subject`, `Headers`, `Body`, `Json<T>`, and `Cbor<T>`.
- Framework-managed publishing through `Publish` and `Publisher`.
- RPC-style request-reply through handler return values.
- Always-on error logging with optional router-scoped and service-level error topics.
- Multi-argument handler support with the axum-style rule that body-consuming extractors go last.
- A working NATS broker backend.

## Quick Start

### Stateless Handler

```rust
use crabbyq::prelude::*;
use crabbyq::brokers::NatsBroker;
use tracing::info;

async fn handle_async_event(
    event: Event,
) -> CrabbyResult<()> {
    info!("🦀 Stateless handler got event: {}", event.subject());
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    info!("Stateless work done for: {}", event.subject());
    Ok(())
}

#[tokio::main]
async fn main() -> CrabbyResult<()> {
    tracing_subscriber::fmt::init();

    let nats_client = async_nats::connect("nats://localhost:4222").await?;
    let nats_broker = NatsBroker::new(nats_client);

    let app = Router::new()
        .route("test.simple", handle_async_event)
        .into_service(nats_broker);

    app.serve().await?;
    Ok(())
}
```

### State and Extractors

```rust
use crabbyq::prelude::*;
use serde::Deserialize;

#[derive(Clone)]
struct AppState {
    app_name: &'static str,
}

#[derive(Clone)]
struct AppName(&'static str);

impl FromRef<AppState> for AppName {
    fn from_ref(input: &AppState) -> Self {
        Self(input.app_name)
    }
}

#[derive(Deserialize)]
struct Payload {
    id: u32,
}

async fn handle(
    State(app_name): State<AppName>,
    Subject(subject): Subject,
    Json(payload): Json<Payload>,
) -> CrabbyResult<()> {
    println!("{} -> {} -> {}", app_name.0, subject, payload.id);
    Ok(())
}
```

Body-consuming extractors such as `Body`, `Json<T>`, and `Cbor<T>` should be the last handler argument.

### RPC Through Return Values

```rust
use crabbyq::prelude::*;
use serde::{Deserialize, Serialize};

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
```

## Examples

- `examples/basic.rs`: stateless handlers.
- `examples/basic_state.rs`: shared state in handlers.
- `examples/basic_rpc.rs`: RPC-style request-reply through return values.
- `examples/custom_error_rpc.rs`: custom error types that implement `IntoResponse`.
- `examples/multirouter.rs`: composing several routers with `include(...)`.
- `examples/extractors.rs`: `State`, `Headers`, `Subject`, `Body`, `Json`, and `Cbor`.
- `examples/publishers.rs`: publishing follow-up messages through `Publish`.

## Architecture Notes

- Route keys are broker-facing identifiers, not HTTP paths.
- `include(...)` composes routers without rewriting route keys.
- Duplicate route keys panic during router construction instead of failing later at runtime.
- Extractors are split into:
  - `FromEventParts<S>` for metadata-like values.
  - `FromEvent<S>` for full-event or payload-consuming values.
- Publishers are injected by the framework at `into_service(...)` time, so
  handlers can publish without manually wiring broker clients through state.
- RPC replies are sent automatically when the incoming message carries `reply_to`
  metadata and the handler returns a response value.

## Roadmap

The project is in its early stages (v0.1.0). Future development focuses on:

- Middleware and `tower::Layer` integration on the router level.
- Framework-managed publishers and publish helpers via extractors.
- More broker backends such as RabbitMQ, Kafka, and Redis.
- Richer extractor coverage, broker metadata, and better rejection/error types.
- Additional payload formats such as Protobuf and MsgPack.

## Contributing

If you are interested in building "Axum for message brokers" in Rust, contributions are welcome through:

- Feature requests and API design discussions.
- Pull requests for new broker implementations.
- Documentation improvements.
