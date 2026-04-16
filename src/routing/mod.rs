//! Routing APIs for broker-agnostic and broker-specific application builders.

pub mod base;
#[cfg(feature = "nats")]
pub mod nats;

pub use base::Router;
#[cfg(feature = "nats")]
pub use nats::NatsRouter;
