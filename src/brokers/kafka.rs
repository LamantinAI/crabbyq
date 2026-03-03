use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::Stream;
use futures_util::stream;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::error::KafkaError;
use rdkafka::message::{Headers, Message};
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::util::Timeout;

use crate::brokers::base::{Broker, BrokerMessage};

pub struct KafkaBroker {
    producer: FutureProducer,
    consumer: Arc<StreamConsumer>,
}

impl KafkaBroker {
    pub fn new(bootstrap_servers: &str, group_id: &str) -> Result<Self, KafkaError> {
        let producer = ClientConfig::new()
            .set("bootstrap.servers", bootstrap_servers)
            .create()?;

        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", bootstrap_servers)
            .set("group.id", group_id)
            .set("enable.auto.commit", "true")
            .set("auto.offset.reset", "earliest")
            .create()?;

        Ok(Self {
            producer,
            consumer: Arc::new(consumer),
        })
    }
}

#[async_trait]
impl Broker for KafkaBroker {
    type MessageStream = Pin<Box<dyn Stream<Item = BrokerMessage> + Send>>;

    async fn subscribe(
        &self,
        subjects: &[String],
    ) -> Result<Self::MessageStream, Box<dyn std::error::Error + Send + Sync>> {
        let topic_refs: Vec<&str> = subjects.iter().map(String::as_str).collect();
        self.consumer
            .subscribe(&topic_refs)
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

        let consumer = self.consumer.clone();
        let message_stream = stream::unfold(consumer, |consumer: Arc<StreamConsumer>| async move {
            loop {
                match consumer.recv().await {
                    Ok(message) => {
                        let headers = extract_headers(&message);

                        let broker_message = BrokerMessage {
                            subject: message.topic().to_string(),
                            payload: message.payload().map_or_else(Vec::new, ToOwned::to_owned),
                            headers,
                        };
                        return Some((broker_message, consumer));
                    }
                    Err(error) => {
                        tracing::error!("Kafka receive error: {}", error);
                    }
                }
            }
        });

        Ok(Box::pin(message_stream))
    }

    async fn publish(
        &self,
        subject: &str,
        payload: &[u8],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.producer
            .send(
                FutureRecord::<(), _>::to(subject).payload(payload),
                Timeout::After(Duration::from_secs(5)),
            )
            .await
            .map_err(|(error, _)| Box::new(error) as Box<dyn std::error::Error + Send + Sync>)?;

        Ok(())
    }
}

fn extract_headers(message: &impl Message) -> Option<HashMap<String, String>> {
    let headers = message.headers()?;
    let mut map = HashMap::new();

    for idx in 0..headers.count() {
        let header = headers.get(idx);
        let value = header
            .value
            .map(|v| String::from_utf8_lossy(v).to_string())
            .unwrap_or_default();
        map.insert(header.key.to_string(), value);
    }

    if map.is_empty() { None } else { Some(map) }
}
