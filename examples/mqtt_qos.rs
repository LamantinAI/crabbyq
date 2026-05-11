use crabbyq::brokers::MqttBroker;
use crabbyq::prelude::*;
use rumqttc::{MqttOptions, QoS};
use std::time::Duration;
use tracing::info;

async fn handle_critical(event: Event) -> CrabbyResult<()> {
    info!(
        "QoS::ExactlyOnce route got '{}': {}",
        event.subject(),
        String::from_utf8_lossy(&event.payload)
    );
    Ok(())
}

async fn handle_shared(event: Event) -> CrabbyResult<()> {
    info!(
        "Shared-subscription worker got '{}': {}",
        event.subject(),
        String::from_utf8_lossy(&event.payload)
    );
    Ok(())
}

async fn handle_status(event: Event) -> CrabbyResult<()> {
    info!(
        "Retained-aware status route got '{}': {}",
        event.subject(),
        String::from_utf8_lossy(&event.payload)
    );
    Ok(())
}

#[tokio::main]
async fn main() -> CrabbyResult<()> {
    tracing_subscriber::fmt::init();

    let mut options = MqttOptions::new("crabbyq-mqtt-qos", "127.0.0.1", 1883);
    options.set_keep_alive(Duration::from_secs(5));
    let broker = MqttBroker::new(options, 10);
    let publisher = MqttPublisher::new(broker.clone());

    // MqttRouter mixes plain MQTT subscriptions with per-route QoS, shared
    // ($share/group/topic) subscriptions, and retained-aware routes.
    let app = MqttRouter::new()
        .qos(QoS::AtLeastOnce)
        .qos_route("orders/critical", QoS::ExactlyOnce, handle_critical)
        .shared_route("workers", "orders/dispatch", handle_shared)
        .retained_route("devices/status", handle_status)
        .into_service(broker)
        .with_graceful_shutdown(async {
            tokio::time::sleep(Duration::from_millis(800)).await;
        });

    info!("Starting MQTT QoS / shared subs example...");
    let handle = tokio::spawn(app.serve());
    tokio::time::sleep(Duration::from_millis(250)).await;

    // QoS::ExactlyOnce publish into the critical-orders route.
    publisher
        .mqtt_publish("orders/critical", "transfer-42")
        .qos(QoS::ExactlyOnce)
        .await?;

    // Shared subscription delivery: workers belonging to "$share/workers/..."
    // share the message load. With one running consumer here the message
    // simply lands on this instance.
    publisher
        .mqtt_publish("orders/dispatch", "ship-7")
        .await?;

    // Retained status message: any new subscriber to devices/status receives
    // this last-known value on connect.
    publisher
        .mqtt_publish("devices/status", "online")
        .retain(true)
        .await?;

    handle.await??;
    Ok(())
}
