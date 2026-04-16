use crate::errors::CrabbyError;
use crate::extract::Body;
use crate::publish::PreparedPublishPayload;
use bytes::Bytes;
use std::fmt::Display;

#[cfg(feature = "cbor")]
use crate::extract::Cbor;
#[cfg(feature = "json")]
use crate::extract::Json;
#[cfg(feature = "cbor")]
use crate::publish::cbor_payload;
#[cfg(feature = "json")]
use crate::publish::json_payload;

pub type HandlerResponse = Option<PreparedPublishPayload>;

pub struct HandlerOutcome {
    pub response: HandlerResponse,
    pub error_message: Option<String>,
}

pub trait IntoResponse {
    fn into_response(self) -> Result<HandlerResponse, CrabbyError>;
}

pub trait IntoHandlerResult {
    fn into_handler_result(self) -> Result<HandlerOutcome, CrabbyError>;
}

pub(crate) fn error_outcome<E>(error: E) -> Result<HandlerOutcome, CrabbyError>
where
    E: IntoResponse + Display,
{
    let error_message = error.to_string();
    Ok(HandlerOutcome {
        response: error.into_response()?,
        error_message: Some(error_message),
    })
}

impl IntoResponse for () {
    fn into_response(self) -> Result<HandlerResponse, CrabbyError> {
        Ok(None)
    }
}

impl IntoResponse for Body {
    fn into_response(self) -> Result<HandlerResponse, CrabbyError> {
        Ok(Some(PreparedPublishPayload {
            payload: self.0.to_vec(),
            headers: None,
        }))
    }
}

#[cfg(feature = "json")]
impl<T> IntoResponse for Json<T>
where
    T: serde::Serialize,
{
    fn into_response(self) -> Result<HandlerResponse, CrabbyError> {
        Ok(Some(json_payload(self.0)?))
    }
}

#[cfg(feature = "cbor")]
impl<T> IntoResponse for Cbor<T>
where
    T: serde::Serialize,
{
    fn into_response(self) -> Result<HandlerResponse, CrabbyError> {
        Ok(Some(cbor_payload(self.0)?))
    }
}

impl IntoResponse for Vec<u8> {
    fn into_response(self) -> Result<HandlerResponse, CrabbyError> {
        Ok(Some(PreparedPublishPayload {
            payload: self,
            headers: None,
        }))
    }
}

impl IntoResponse for Bytes {
    fn into_response(self) -> Result<HandlerResponse, CrabbyError> {
        Ok(Some(PreparedPublishPayload {
            payload: self.to_vec(),
            headers: None,
        }))
    }
}

impl IntoResponse for String {
    fn into_response(self) -> Result<HandlerResponse, CrabbyError> {
        Ok(Some(PreparedPublishPayload {
            payload: self.into_bytes(),
            headers: None,
        }))
    }
}

impl IntoResponse for &str {
    fn into_response(self) -> Result<HandlerResponse, CrabbyError> {
        Ok(Some(PreparedPublishPayload {
            payload: self.as_bytes().to_vec(),
            headers: None,
        }))
    }
}

impl IntoResponse for CrabbyError {
    fn into_response(self) -> Result<HandlerResponse, CrabbyError> {
        let message = self.to_string();
        Ok(Some(PreparedPublishPayload {
            payload: message.into_bytes(),
            headers: None,
        }))
    }
}

impl<T> IntoHandlerResult for T
where
    T: IntoResponse,
{
    fn into_handler_result(self) -> Result<HandlerOutcome, CrabbyError> {
        Ok(HandlerOutcome {
            response: self.into_response()?,
            error_message: None,
        })
    }
}

impl<T, E> IntoHandlerResult for Result<T, E>
where
    T: IntoResponse,
    E: IntoResponse + Display,
{
    fn into_handler_result(self) -> Result<HandlerOutcome, CrabbyError> {
        match self {
            Ok(value) => value.into_handler_result(),
            Err(error) => error_outcome(error),
        }
    }
}
