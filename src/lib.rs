pub mod brokers;
pub mod errors;
pub mod event;
pub mod extract;
pub mod handler;
pub mod publish;
pub mod response;
pub mod routing;
pub mod service;

pub use brokers::base::HeaderMap;
pub use errors::{CrabbyError, CrabbyResult};
pub use event::Event;
#[cfg(feature = "cbor")]
pub use extract::Cbor;
#[cfg(feature = "json")]
pub use extract::Json;
/// Common extractors and the main router API for most applications.
pub use extract::{Body, FromRef, Headers, Publish, State, Subject};
#[cfg(feature = "mqtt")]
pub use publish::MqttPublisher;
#[cfg(feature = "redis")]
pub use publish::RedisPublisher;
#[cfg(feature = "nats")]
pub use publish::{NatsJsPublishRequest, NatsPublishAck, NatsPublisher};
pub use publish::{Publisher, Reply};
pub use response::{HandlerResponse, IntoResponse};
#[cfg(feature = "nats")]
pub use routing::NatsRouter;
pub use routing::Router;

pub mod prelude {
    pub use crate::{
        Body, CrabbyError, CrabbyResult, Event, FromRef, HandlerResponse, HeaderMap, Headers,
        IntoResponse, Publish, Publisher, Reply, Router, State, Subject,
    };

    #[cfg(feature = "cbor")]
    pub use crate::Cbor;
    #[cfg(feature = "json")]
    pub use crate::Json;
    #[cfg(feature = "mqtt")]
    pub use crate::MqttPublisher;
    #[cfg(feature = "redis")]
    pub use crate::RedisPublisher;
    #[cfg(feature = "nats")]
    pub use crate::{NatsJsPublishRequest, NatsPublishAck, NatsPublisher, NatsRouter};
}
