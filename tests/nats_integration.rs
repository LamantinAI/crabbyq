use crabbyq::brokers::NatsBroker;
use crabbyq::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Deserialize, Serialize)]
struct SumRequest {
    left: i32,
    right: i32,
}

#[derive(Deserialize, Serialize)]
struct SumResponse {
    result: i32,
}

#[derive(Clone)]
struct RecordedIds(Arc<Mutex<Vec<u32>>>);

#[derive(Deserialize, Serialize)]
struct CommandEvent {
    id: u32,
}

impl FromRef<Arc<Mutex<Vec<u32>>>> for RecordedIds {
    fn from_ref(input: &Arc<Mutex<Vec<u32>>>) -> Self {
        Self(input.clone())
    }
}

fn unique_subject(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{prefix}.{nanos}")
}

#[tokio::test]
#[ignore = "requires a local NATS server on nats://127.0.0.1:4222"]
async fn rpc_round_trip_works_with_real_nats()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = async_nats::connect("nats://127.0.0.1:4222").await?;
    let subject = unique_subject("crabbyq.test.rpc");

    let broker = NatsBroker::new(client.clone());
    let publisher = Publisher::new(broker.clone());
    let app = Router::new()
        .route(&subject, |Json(request): Json<SumRequest>| async move {
            Ok::<Json<SumResponse>, CrabbyError>(Json(SumResponse {
                result: request.left + request.right,
            }))
        })
        .into_service(broker);

    let handle = tokio::spawn(app.serve());
    tokio::time::sleep(Duration::from_millis(150)).await;

    let reply: SumResponse = tokio::time::timeout(
        Duration::from_secs(2),
        publisher.request(
            &subject,
            Json(SumRequest {
                left: 19,
                right: 23,
            }),
        ),
    )
    .await??
    .into_json()?;

    assert_eq!(reply.result, 42);

    handle.abort();
    let _ = handle.await;

    Ok(())
}

#[tokio::test]
#[ignore = "requires a local NATS server with JetStream on nats://127.0.0.1:4222"]
async fn jetstream_publish_reaches_durable_route()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = async_nats::connect("nats://127.0.0.1:4222").await?;
    let jetstream = async_nats::jetstream::new(client.clone());

    let stream_name = unique_subject("crabbyq_test_stream").replace('.', "_");
    let subject = unique_subject("crabbyq.test.js");
    let durable = unique_subject("crabbyq_test_durable").replace('.', "_");

    let stream = jetstream
        .get_or_create_stream(async_nats::jetstream::stream::Config {
            name: stream_name.clone(),
            subjects: vec![subject.clone()],
            retention: async_nats::jetstream::stream::RetentionPolicy::WorkQueue,
            storage: async_nats::jetstream::stream::StorageType::Memory,
            ..Default::default()
        })
        .await?;
    stream.purge().await?;

    let broker = NatsBroker::new(client);
    let publisher = NatsPublisher::new(broker.clone());
    let records = Arc::new(Mutex::new(Vec::new()));

    let app = NatsRouter::new()
        .set_state(records.clone())
        .jetstream(stream)
        .js_durable_route(
            &subject,
            &durable,
            |State(recorded): State<RecordedIds>, Json(payload): Json<CommandEvent>| async move {
                recorded.0.lock().unwrap().push(payload.id);
                Ok::<(), CrabbyError>(())
            },
        )
        .into_service(broker);

    let handle = tokio::spawn(app.serve());
    tokio::time::sleep(Duration::from_millis(150)).await;

    let ack = tokio::time::timeout(
        Duration::from_secs(2),
        publisher.js_publish(&subject, Json(CommandEvent { id: 7 })),
    )
    .await??;

    assert_eq!(ack.stream, stream_name);

    let started = tokio::time::Instant::now();
    loop {
        if records.lock().unwrap().as_slice() == [7] {
            break;
        }

        if started.elapsed() > Duration::from_secs(2) {
            panic!("timed out waiting for JetStream message to reach durable route");
        }

        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    handle.abort();
    let _ = handle.await;
    jetstream.delete_stream(&stream_name).await?;

    Ok(())
}
