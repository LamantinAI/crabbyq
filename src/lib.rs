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
/// Common extractors and the main router API for most applications.
pub use extract::{Body, Cbor, FromRef, Headers, Json, Publish, State, Subject};
pub use publish::{NatsJsPublishRequest, NatsPublishAck, NatsPublisher, Publisher, Reply};
pub use response::{HandlerResponse, IntoResponse};
pub use routing::{NatsRouter, Router};

pub mod prelude {
    pub use crate::{
        Body, Cbor, CrabbyError, CrabbyResult, Event, FromRef, HandlerResponse, HeaderMap, Headers,
        IntoResponse, Json, NatsJsPublishRequest, NatsPublishAck, NatsPublisher, NatsRouter,
        Publish, Publisher, Reply, Router, State, Subject,
    };
}
