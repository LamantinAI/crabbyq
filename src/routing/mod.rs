//! Routing APIs for broker-agnostic and broker-specific application builders.

pub mod base;
#[cfg(feature = "mqtt")]
pub mod mqtt;
#[cfg(feature = "nats")]
pub mod nats;
#[cfg(feature = "redis")]
pub mod redis;

pub use base::Router;
#[cfg(feature = "mqtt")]
pub use mqtt::MqttRouter;
#[cfg(feature = "nats")]
pub use nats::NatsRouter;
#[cfg(feature = "redis")]
pub use redis::{
    AutoClaimConfig, RedisGroupRouteConfig, RedisRouter, RedisStreamRouteConfig,
};
