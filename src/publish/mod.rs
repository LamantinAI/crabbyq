//! Broker-agnostic and broker-specific publishing APIs.

pub mod base;
#[cfg(feature = "mqtt")]
pub mod mqtt;
#[cfg(feature = "nats")]
pub mod nats;
#[cfg(feature = "redis")]
pub mod redis;

#[cfg(feature = "cbor")]
pub use base::cbor_payload;
#[cfg(feature = "json")]
pub use base::json_payload;
pub use base::{
    IntoPublishPayload, PreparedPublishPayload, PublishRequest, Publisher, Reply, Request,
    merge_headers,
};
#[cfg(feature = "mqtt")]
pub use mqtt::MqttPublisher;
#[cfg(feature = "nats")]
pub use nats::{NatsJsPublishRequest, NatsPublishAck, NatsPublisher};
#[cfg(feature = "redis")]
pub use redis::RedisPublisher;
