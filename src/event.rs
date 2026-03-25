use bytes::Bytes;
use std::collections::HashMap;

#[derive(Clone)]
pub struct EventParts {
    pub subject: String,
    pub headers: Option<HashMap<String, String>>,
    pub reply_to: Option<String>,
}

#[derive(Clone)]
pub struct Event {
    pub parts: EventParts,
    pub payload: Bytes,
}

impl Event {
    pub fn new(
        subject: String,
        payload: Bytes,
        headers: Option<HashMap<String, String>>,
        reply_to: Option<String>,
    ) -> Self {
        Self {
            parts: EventParts {
                subject,
                headers,
                reply_to,
            },
            payload,
        }
    }

    pub fn subject(&self) -> &str {
        &self.parts.subject
    }

    pub fn headers(&self) -> Option<&HashMap<String, String>> {
        self.parts.headers.as_ref()
    }

    pub fn reply_to(&self) -> Option<&str> {
        self.parts.reply_to.as_deref()
    }

    pub fn into_parts(self) -> (EventParts, Bytes) {
        (self.parts, self.payload)
    }
}
