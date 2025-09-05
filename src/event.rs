use bytes::Bytes;
use std::collections::HashMap;

// Public event struct for de/serialization
#[derive(Clone)]
pub struct Event {
    pub subject: String,
    pub payload: Bytes,
    pub headers: Option<HashMap<String, String>>,
}

impl Event {
    pub fn new(subject: String, payload: Bytes, headers: Option<HashMap<String, String>>) -> Self {
        Self {
            subject,
            payload,
            headers,
        }
    }
}