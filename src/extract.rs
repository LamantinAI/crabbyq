use crate::errors::CrabbyError;
use crate::event::{Event, EventParts};
use crate::publish::{PreparedPublishPayload, Publisher};
use bytes::Bytes;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

#[cfg(feature = "cbor")]
use crate::publish::cbor_payload;
#[cfg(feature = "json")]
use crate::publish::json_payload;
#[cfg(any(feature = "json", feature = "cbor"))]
use serde::de::DeserializeOwned;

pub type ExtractFuture<T> = Pin<Box<dyn Future<Output = Result<T, CrabbyError>> + Send>>;

#[derive(Clone)]
#[doc(hidden)]
pub struct RuntimeState<S> {
    pub(crate) app_state: S,
    pub(crate) publisher: Publisher,
}

impl<S> RuntimeState<S> {
    pub(crate) fn new(app_state: S, publisher: Publisher) -> Self {
        Self {
            app_state,
            publisher,
        }
    }
}

/// Builds a derived value from a shared state reference.
///
/// This makes it possible to extract specific sub-components from a larger
/// application state, similar to axum's `FromRef`.
pub trait FromRef<T> {
    fn from_ref(input: &T) -> Self;
}

impl<T: Clone> FromRef<T> for T {
    fn from_ref(input: &T) -> Self {
        input.clone()
    }
}

/// Extracts a value from event metadata without consuming the payload.
///
/// Typical examples are shared state, route keys, headers, and future broker
/// metadata.
pub trait FromEventParts<S>: Sized {
    fn from_event_parts(parts: &mut EventParts, state: &S) -> ExtractFuture<Self>;
}

/// Extracts a value from the full event.
///
/// Keep body-consuming extractors as the last handler argument. This mirrors
/// the axum-style rule that metadata can be extracted first, and the payload is
/// consumed only once at the end.
pub trait FromEvent<S>: Sized {
    fn from_event(event: Event, state: &S) -> ExtractFuture<Self>;
}

/// Extracts a typed value from shared application state.
pub struct State<T>(pub T);

/// Extracts the current route key.
pub struct Subject(pub String);

/// Extracts broker message headers.
pub struct Headers(pub Option<HashMap<String, String>>);

/// Extracts raw payload bytes.
pub struct Body(pub Bytes);

/// Extracts a framework-managed [`Publisher`].
///
/// This is the preferred way to publish follow-up messages from a handler
/// without manually storing broker clients inside application state.
pub struct Publish(pub Publisher);

/// Deserializes the payload as JSON.
#[cfg(feature = "json")]
pub struct Json<T>(pub T);

/// Deserializes the payload as CBOR.
#[cfg(feature = "cbor")]
pub struct Cbor<T>(pub T);

impl<S, T> FromEventParts<RuntimeState<S>> for State<T>
where
    T: FromRef<S> + Send + 'static,
    S: Send + Sync + 'static,
{
    fn from_event_parts(_parts: &mut EventParts, state: &RuntimeState<S>) -> ExtractFuture<Self> {
        let value = T::from_ref(&state.app_state);
        Box::pin(async move { Ok(State(value)) })
    }
}

impl<S> FromEventParts<S> for Subject
where
    S: Send + Sync + 'static,
{
    fn from_event_parts(parts: &mut EventParts, _state: &S) -> ExtractFuture<Self> {
        let subject = parts.subject.clone();
        Box::pin(async move { Ok(Subject(subject)) })
    }
}

impl<S> FromEventParts<S> for Headers
where
    S: Send + Sync + 'static,
{
    fn from_event_parts(parts: &mut EventParts, _state: &S) -> ExtractFuture<Self> {
        let headers = parts.headers.clone();
        Box::pin(async move { Ok(Headers(headers)) })
    }
}

impl<S> FromEventParts<RuntimeState<S>> for Publish
where
    S: Send + Sync + 'static,
{
    fn from_event_parts(_parts: &mut EventParts, state: &RuntimeState<S>) -> ExtractFuture<Self> {
        let publisher = state.publisher.clone();
        Box::pin(async move { Ok(Publish(publisher)) })
    }
}

impl<S> FromEvent<S> for Body
where
    S: Send + Sync + 'static,
{
    fn from_event(event: Event, _state: &S) -> ExtractFuture<Self> {
        Box::pin(async move { Ok(Body(event.payload)) })
    }
}

#[cfg(feature = "json")]
impl<S, T> FromEvent<S> for Json<T>
where
    S: Send + Sync + 'static,
    T: DeserializeOwned + Send + 'static,
{
    fn from_event(event: Event, _state: &S) -> ExtractFuture<Self> {
        Box::pin(async move {
            let value = serde_json::from_slice(&event.payload)?;
            Ok(Json(value))
        })
    }
}

#[cfg(feature = "cbor")]
impl<S, T> FromEvent<S> for Cbor<T>
where
    S: Send + Sync + 'static,
    T: DeserializeOwned + Send + 'static,
{
    fn from_event(event: Event, _state: &S) -> ExtractFuture<Self> {
        Box::pin(async move {
            let value = ciborium::from_reader(event.payload.as_ref())?;
            Ok(Cbor(value))
        })
    }
}

impl crate::publish::IntoPublishPayload for Body {
    fn into_publish_payload(self) -> Result<PreparedPublishPayload, CrabbyError> {
        Ok(PreparedPublishPayload {
            payload: self.0.to_vec(),
            headers: None,
        })
    }
}

#[cfg(feature = "json")]
impl<T> crate::publish::IntoPublishPayload for Json<T>
where
    T: serde::Serialize,
{
    fn into_publish_payload(self) -> Result<PreparedPublishPayload, CrabbyError> {
        json_payload(self.0)
    }
}

#[cfg(feature = "cbor")]
impl<T> crate::publish::IntoPublishPayload for Cbor<T>
where
    T: serde::Serialize,
{
    fn into_publish_payload(self) -> Result<PreparedPublishPayload, CrabbyError> {
        cbor_payload(self.0)
    }
}
