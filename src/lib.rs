pub mod brokers;
pub mod errors;
pub mod extract;
pub mod event;
pub mod handler;
pub mod publish;
pub mod router;
pub mod response;
pub mod service;

/// Common extractors and the main router API for most applications.
pub use extract::{Body, Cbor, FromRef, Headers, Json, Publish, State, Subject};
pub use brokers::base::HeaderMap;
pub use errors::{CrabbyError, CrabbyResult};
pub use publish::Publisher;
pub use router::Router;
pub use response::{HandlerResponse, IntoResponse};
pub use event::Event;

pub mod prelude {
    pub use crate::{
        Body,
        Cbor,
        CrabbyError,
        CrabbyResult,
        Router,
        Event,
        FromRef,
        HeaderMap,
        HandlerResponse,
        Headers,
        IntoResponse,
        Json,
        Publish,
        Publisher,
        State,
        Subject,
    };
}
