# 🦀 CrabbyQ

CrabbyQ is a declarative, asynchronous Rust framework for building message-driven microservices.

Inspired by the ergonomics of **Axum** and the messaging mindset of **FastStream**, CrabbyQ aims to make broker-driven services feel modular, typed, and easy to compose.

## Overview

CrabbyQ abstracts away broker subscription loops and route dispatching so you can focus on handlers and domain logic. Internally it is built around **Tower**, so routes are modeled as `tower::Service`s and the framework can grow naturally toward layers and middleware.

By default, CrabbyQ keeps the core lightweight and enables only the `json`
format feature. Broker backends such as NATS, Redis, and MQTT are enabled
explicitly through Cargo features.

## Current Features

- A broker-agnostic core `Router` with typed shared state through `set_state(...)`.
- Route registration through `route(...)`, `routes(...)`, `include(...)`, and `route_service(...)`.
- `tower` middleware support through `layer(...)`.
- Axum-like extractors such as `State<T>`, `Subject`, `Headers`, `Body`, and optional payload extractors like `Json<T>` and `Cbor<T>`.
- Framework-managed publishing through the `Publish` extractor and the core `Publisher`.
- Broker-specific publishers for NATS, Redis, and MQTT.
- RPC-style request-reply through handler return values and `Publisher::request(...)`.
- Graceful shutdown hooks and always-on error logging with optional router-scoped and service-level error topics.
- Multi-argument handler support with the axum-style rule that body-consuming extractors go last.
- Working broker backends for NATS, Redis pub/sub, and MQTT.
- `NatsRouter` and `NatsPublisher` for NATS-specific features, including JetStream routes and JetStream publishing.

## Features

- `json`
  Enabled by default. Adds `Json<T>` extractor/response support.
- `cbor`
  Enables `Cbor<T>` extractor/response support.
- `nats`
  Enables `NatsBroker`, `NatsRouter`, `NatsPublisher`, and JetStream integration.
- `redis`
  Enables `RedisBroker` and `RedisPublisher`.
- `mqtt`
  Enables `MqttBroker` and `MqttPublisher`.
- `full`
  Convenience feature for local development and IDE indexing. Enables `json`, `cbor`, `nats`, `redis`, and `mqtt`.

## Quick Start

### Stateless Handler

```rust
use crabbyq::prelude::*;
use crabbyq::brokers::NatsBroker;
use tracing::info;

async fn handle_async_event(
    event: Event,
) -> CrabbyResult<()> {
    info!("Stateless handler got event: {}", event.subject());
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

For the NATS examples above, enable the `nats` feature:

```toml
crabbyq = { version = "0.1.0", features = ["nats"] }
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

The repository includes runnable examples for stateless handlers, state,
extractors, publishers, RPC, multi-router composition, and graceful shutdown.
See [`examples/README.md`](examples/README.md) for the full list of runnable examples and example commands.

## Architecture Notes

- Route keys are broker-facing identifiers, not HTTP paths.
- `include(...)` composes routers without rewriting route keys.
- Duplicate route keys panic during router construction instead of failing later at runtime.
- Extractors are split into:
  - `FromEventParts<S>` for metadata-like values.
  - `FromEvent<S>` for full-event or payload-consuming values.
- Publishers are injected by the framework at `into_service(...)` time, so
  handlers can publish without manually wiring broker clients through state.
- `Publisher` stays broker-agnostic and is the injected handler capability.
  Broker-specific publishing features live in broker-specific publishers such
  as `NatsPublisher`, `RedisPublisher`, and `MqttPublisher`.
- RPC replies are sent automatically when the incoming message carries `reply_to`
  metadata and the handler returns a response value.
- Middleware is exposed through `Router::layer(...)` and currently composes
  routes through boxed `tower::Service`s for implementation simplicity.
  A more zero-cost internal pipeline is planned as the framework matures.
- Broker-specific features live in broker-specific routers and publishers. For
  example, `NatsRouter` and `NatsPublisher` extend the core API with
  JetStream-specific routing and publishing.
- Broker backends and message formats are feature-gated so embedded and other
  constrained targets can keep the dependency footprint smaller.
- CrabbyQ focuses on routing and service ergonomics. For NATS JetStream
  storage APIs such as Key-Value and Object Store, `async-nats` already
  provides a solid API, so CrabbyQ does not add extra wrapper layers there.

## Roadmap

Future development focuses on:

- A more zero-cost internal middleware pipeline on top of the current
  `tower::Layer` integration.
- Broker-specific extractors for transport metadata such as NATS and JetStream
  delivery context.
- Better typed rejection and error response handling.
- More broker backends such as RabbitMQ and Kafka.
- Additional payload formats such as Protobuf and MsgPack.

## Contributing

If you are interested in building "Axum for message brokers" in Rust, contributions are welcome through:

- Feature requests and API design discussions.
- Pull requests for new broker implementations.
- Documentation improvements.
