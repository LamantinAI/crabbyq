use async_trait::async_trait;
use crabbyq::brokers::base::{Broker, BrokerMessage, HeaderMap};
use futures_util::Stream;
use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct TestBrokerMessage {
    pub subject: String,
    pub payload: Vec<u8>,
    pub headers: Option<HeaderMap>,
    pub reply_to: Option<String>,
}

impl TestBrokerMessage {
    pub fn new(subject: impl Into<String>, payload: Vec<u8>) -> Self {
        Self {
            subject: subject.into(),
            payload,
            headers: None,
            reply_to: None,
        }
    }

    pub fn with_headers(mut self, headers: HeaderMap) -> Self {
        self.headers = Some(headers);
        self
    }

    pub fn with_reply_to(mut self, reply_to: impl Into<String>) -> Self {
        self.reply_to = Some(reply_to.into());
        self
    }
}

#[derive(Clone, Debug)]
pub struct PublishedMessage {
    pub subject: String,
    pub payload: Vec<u8>,
    pub headers: Option<HeaderMap>,
}

#[derive(Clone, Default)]
pub struct TestBroker {
    incoming: Arc<Mutex<VecDeque<TestBrokerMessage>>>,
    published: Arc<Mutex<Vec<PublishedMessage>>>,
    requested: Arc<Mutex<Vec<PublishedMessage>>>,
    request_replies: Arc<Mutex<VecDeque<TestBrokerMessage>>>,
}

impl TestBroker {
    pub fn new(messages: Vec<TestBrokerMessage>) -> Self {
        Self {
            incoming: Arc::new(Mutex::new(messages.into())),
            published: Arc::new(Mutex::new(Vec::new())),
            requested: Arc::new(Mutex::new(Vec::new())),
            request_replies: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub fn with_request_replies(mut self, messages: Vec<TestBrokerMessage>) -> Self {
        self.request_replies = Arc::new(Mutex::new(messages.into()));
        self
    }

    pub fn published_messages(&self) -> Vec<PublishedMessage> {
        self.published.lock().unwrap().clone()
    }

    pub fn requested_messages(&self) -> Vec<PublishedMessage> {
        self.requested.lock().unwrap().clone()
    }
}

#[async_trait]
impl Broker for TestBroker {
    type MessageStream = Pin<Box<dyn Stream<Item = BrokerMessage> + Send + Unpin>>;

    async fn subscribe(
        &self,
        _subjects: &[String],
    ) -> Result<Self::MessageStream, Box<dyn std::error::Error + Send + Sync>> {
        let messages: Vec<_> = self
            .incoming
            .lock()
            .unwrap()
            .drain(..)
            .map(|message| BrokerMessage {
                subject: message.subject,
                payload: message.payload,
                headers: message.headers,
                reply_to: message.reply_to,
                acknowledger: None,
            })
            .collect();

        Ok(Box::pin(futures_util::stream::iter(messages)))
    }

    async fn publish(
        &self,
        subject: &str,
        payload: &[u8],
        headers: Option<&HeaderMap>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.published.lock().unwrap().push(PublishedMessage {
            subject: subject.to_string(),
            payload: payload.to_vec(),
            headers: headers.cloned(),
        });
        Ok(())
    }

    async fn request(
        &self,
        subject: &str,
        payload: &[u8],
        headers: Option<&HeaderMap>,
    ) -> Result<BrokerMessage, Box<dyn std::error::Error + Send + Sync>> {
        self.requested.lock().unwrap().push(PublishedMessage {
            subject: subject.to_string(),
            payload: payload.to_vec(),
            headers: headers.cloned(),
        });

        let message = self
            .request_replies
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!("no queued request reply"))?;

        Ok(BrokerMessage {
            subject: message.subject,
            payload: message.payload,
            headers: message.headers,
            reply_to: message.reply_to,
            acknowledger: None,
        })
    }
}
