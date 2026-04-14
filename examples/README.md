# Examples

This directory contains runnable CrabbyQ examples built around a local NATS server.

Start NATS first:

```bash
docker compose up -d
```

Then run any example with:

```bash
cargo run --example <name>
```

## Available Examples

- `basic`: stateless event handler.
  Publish a message with:
  `nats pub test.simple 'hello'`

- `basic_state`: shared application state injected into handlers.
  Publish a message with:
  `nats pub test.state 'hello'`

- `basic_rpc`: RPC-style request-reply through handler return values and `Publisher::request(...)`.
  The example starts a local service task, performs a request through CrabbyQ,
  prints the response, and shuts down automatically.

- `custom_error_rpc`: custom error types that implement `IntoResponse`.
  The example performs one successful request and one failing request through
  CrabbyQ, then logs both replies.

- `multirouter`: several routers composed into one service with `include(...)`.
  Publish messages with:
  `nats pub user.created '{}'`
  `nats pub invoice.paid '{}'`

- `extractors`: `State`, `Headers`, `Subject`, `Body`, `Json`, and `Cbor`.
  Useful for exploring handler argument extraction patterns.

- `publishers`: publish follow-up events from inside handlers using the `Publish` extractor.
  Publish a trigger message with:
  `nats pub events.source '{}'`

- `graceful_shutdown`: custom graceful shutdown signal and `on_shutdown(...)` hook.
  Publish a message with:
  `nats pub service.ping 'ping'`
  Then press `Ctrl+C` and watch the final `service.shutdown` event get published.
