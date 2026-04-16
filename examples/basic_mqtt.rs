use crabbyq::brokers::MqttBroker;
use crabbyq::prelude::*;
use rumqttc::MqttOptions;
use tracing::info;

async fn handle_event(event: Event) -> CrabbyResult<()> {
    let payload = String::from_utf8(event.payload.to_vec())?;
    info!("MQTT message on '{}': {}", event.subject(), payload);
    Ok(())
}

#[tokio::main]
async fn main() -> CrabbyResult<()> {
    tracing_subscriber::fmt::init();

    let mut options = MqttOptions::new("crabbyq-basic-mqtt", "127.0.0.1", 1883);
    options.set_keep_alive(std::time::Duration::from_secs(5));

    let broker = MqttBroker::new(options, 10);
    let publisher = MqttPublisher::new(broker.clone());

    let app = Router::new()
        .route("events/mqtt", handle_event)
        .into_service(broker)
        .with_graceful_shutdown(async {
            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        });

    info!("Starting MQTT example...");
    let handle = tokio::spawn(app.serve());
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;

    publisher.publish("events/mqtt", "hello from MQTT").await?;

    handle.await??;
    Ok(())
}
