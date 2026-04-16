# Examples

This directory contains runnable CrabbyQ examples for the currently supported brokers.

Start the local broker stack first:

```bash
docker compose up -d
```

For local development and IDE indexing, enabling the full feature set is the
easiest option:

```bash
cargo check --features full
```

Then run any example with the features it needs:

```bash
cargo run --features <feature-list> --example <name>
```

## Available Examples

- `basic`: stateless event handler.
  Run with:
  `cargo run --features nats --example basic`
  Publish a message with:
  `nats pub test.simple 'hello'`

- `basic_state`: shared application state injected into handlers.
  Run with:
  `cargo run --features nats --example basic_state`
  Publish a message with:
  `nats pub test.state 'hello'`

- `basic_rpc`: RPC-style request-reply through handler return values and `Publisher::request(...)`.
  Run with:
  `cargo run --features nats,json --example basic_rpc`
  The example starts a local service task, performs a request through CrabbyQ,
  prints the response, and shuts down automatically.

- `custom_error_rpc`: custom error types that implement `IntoResponse`.
  Run with:
  `cargo run --features nats,json --example custom_error_rpc`
  The example performs one successful request and one failing request through
  CrabbyQ, then logs both replies.

- `multirouter`: several routers composed into one service with `include(...)`.
  Run with:
  `cargo run --features nats --example multirouter`
  Publish messages with:
  `nats pub user.created '{}'`
  `nats pub invoice.paid '{}'`

- `extractors`: `State`, `Headers`, `Subject`, `Body`, `Json`, and `Cbor`.
  Run with:
  `cargo run --features nats,json,cbor --example extractors`
  Useful for exploring handler argument extraction patterns.

- `publishers`: publish follow-up events from inside handlers using the `Publish` extractor.
  Run with:
  `cargo run --features nats,json --example publishers`
  Publish a trigger message with:
  `nats pub events.source '{}'`

- `basic_redis`: basic Redis pub/sub flow using `RedisBroker` and `RedisPublisher`.
  Run with:
  `cargo run --features redis --example basic_redis`

- `basic_mqtt`: basic MQTT flow using `MqttBroker` and `MqttPublisher`.
  Run with:
  `cargo run --features mqtt --example basic_mqtt`

- `jetstream`: mixes plain NATS routes with JetStream-backed routes through `NatsRouter`.
  Run with:
  `cargo run --features nats,json --example jetstream`
  The example creates a stream with `async-nats`, binds it with
  `jetstream(...)`, publishes through `NatsPublisher::js_publish(...)`,
  exercises both `js_route(...)` and `js_durable_route(...)`, then shuts down
  automatically.

- `middleware`: applies a custom `tower::Layer` to router routes.
  Run with:
  `cargo run --features nats --example middleware`
  Publish a message with:
  `nats pub events.logs 'hello'`

- `graceful_shutdown`: custom graceful shutdown signal and `on_shutdown(...)` hook.
  Run with:
  `cargo run --features nats,json --example graceful_shutdown`
  Publish a message with:
  `nats pub service.ping 'ping'`
  Then press `Ctrl+C` and watch the final `service.shutdown` event get published.
