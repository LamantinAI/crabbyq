#![cfg(feature = "mqtt")]

use crabbyq::brokers::MqttBroker;
use crabbyq::prelude::*;
use rumqttc::MqttOptions;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn unique_topic(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{prefix}/{nanos}")
}

fn unique_client_id(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{prefix}-{nanos}")
}

#[tokio::test]
#[ignore = "requires a local MQTT broker on mqtt://127.0.0.1:1883"]
async fn mqtt_publish_reaches_router() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut options = MqttOptions::new(unique_client_id("crabbyq"), "127.0.0.1", 1883);
    options.set_keep_alive(Duration::from_secs(5));

    let broker = MqttBroker::new(options, 10);
    let publisher = Publisher::new(broker.clone());
    let records = Arc::new(Mutex::new(Vec::new()));
    let topic = unique_topic("crabbyq/test/mqtt");
    let recorded = records.clone();

    let app = Router::new()
        .route(&topic, move |event: Event| {
            let recorded = recorded.clone();
            async move {
                let payload = String::from_utf8(event.payload.to_vec()).unwrap();
                recorded.lock().unwrap().push(payload);
                Ok::<(), CrabbyError>(())
            }
        })
        .into_service(broker);

    let handle = tokio::spawn(app.serve());
    tokio::time::sleep(Duration::from_millis(250)).await;

    publisher.publish(&topic, "hello from mqtt").await?;

    let started = tokio::time::Instant::now();
    loop {
        if records.lock().unwrap().as_slice() == ["hello from mqtt"] {
            break;
        }

        if started.elapsed() > Duration::from_secs(3) {
            panic!("timed out waiting for MQTT message");
        }

        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    handle.abort();
    let _ = handle.await;

    Ok(())
}
