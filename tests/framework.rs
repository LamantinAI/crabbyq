mod common;

use common::{TestBroker, TestBrokerMessage};
use crabbyq::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct AppState {
    records: Arc<Mutex<Vec<String>>>,
}

#[derive(Clone)]
struct SharedRecords(Arc<Mutex<Vec<String>>>);

impl FromRef<AppState> for SharedRecords {
    fn from_ref(input: &AppState) -> Self {
        Self(input.records.clone())
    }
}

#[derive(Deserialize, Serialize)]
struct Payload {
    id: u32,
}

#[derive(Deserialize, Serialize)]
struct SumRequest {
    left: i32,
    right: i32,
}

#[derive(Deserialize, Serialize)]
struct SumResponse {
    result: i32,
}

#[derive(Debug)]
struct TestError(&'static str);

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for TestError {}

impl IntoResponse for TestError {
    fn into_response(self) -> Result<HandlerResponse, CrabbyError> {
        Ok(None)
    }
}

#[derive(Deserialize)]
struct ErrorEnvelope {
    subject: String,
    reply_to: Option<String>,
    headers: Option<HeaderMap>,
    payload: Vec<u8>,
    error: String,
}

async fn noop_handler(_event: Event) -> CrabbyResult<()> {
    Ok(())
}

#[test]
#[should_panic(expected = "duplicate route key 'dup' is already registered")]
fn duplicate_route_keys_panic() {
    let _ = Router::new()
        .route("dup", noop_handler)
        .route("dup", noop_handler);
}

#[tokio::test]
async fn state_subject_and_json_extractors_work_together() {
    let records = Arc::new(Mutex::new(Vec::new()));
    let state = AppState {
        records: records.clone(),
    };

    let message = TestBrokerMessage::new(
        "orders.created",
        serde_json::to_vec(&Payload { id: 7 }).unwrap(),
    );
    let broker = TestBroker::new(vec![message]);

    let app = Router::new()
        .set_state(state)
        .route(
            "orders.created",
            |State(shared): State<SharedRecords>, Subject(subject): Subject, Json(payload): Json<Payload>| async move {
                shared
                    .0
                    .lock()
                    .unwrap()
                    .push(format!("{subject}:{}", payload.id));
                Ok::<(), CrabbyError>(())
            },
        )
        .into_service(broker);

    app.serve().await.unwrap();

    assert_eq!(records.lock().unwrap().as_slice(), ["orders.created:7"]);
}

#[tokio::test]
async fn routes_registers_one_handler_for_multiple_subjects() {
    let records = Arc::new(Mutex::new(Vec::new()));
    let broker = TestBroker::new(vec![
        TestBrokerMessage::new("alpha", Vec::new()),
        TestBrokerMessage::new("beta", Vec::new()),
    ]);

    let app = Router::new()
        .set_state(records.clone())
        .routes(["alpha", "beta"], |event: Event, records: Arc<Mutex<Vec<String>>>| async move {
            records.lock().unwrap().push(event.subject().to_string());
            Ok::<(), CrabbyError>(())
        })
        .into_service(broker);

    app.serve().await.unwrap();

    assert_eq!(records.lock().unwrap().as_slice(), ["alpha", "beta"]);
}

#[tokio::test]
async fn publisher_extractor_emits_follow_up_message() {
    let broker = TestBroker::new(vec![TestBrokerMessage::new("source", Vec::new())]);
    let published_broker = broker.clone();

    let app = Router::new()
        .route(
            "source",
            |Publish(publisher): Publish, Subject(subject): Subject| async move {
                publisher
                    .publish("follow-up", Json(Payload { id: subject.len() as u32 }))
                    .await?;
                Ok::<(), CrabbyError>(())
            },
        )
        .into_service(broker);

    app.serve().await.unwrap();

    let published = published_broker.published_messages();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].subject, "follow-up");
    assert_eq!(
        published[0].headers.as_ref().unwrap().get("content-type"),
        Some(&"application/json".to_string())
    );

    let payload: Payload = serde_json::from_slice(&published[0].payload).unwrap();
    assert_eq!(payload.id, "source".len() as u32);
}

#[tokio::test]
async fn rpc_reply_is_published_to_reply_subject() {
    let broker = TestBroker::new(vec![
        TestBrokerMessage::new(
            "sum",
            serde_json::to_vec(&SumRequest { left: 20, right: 22 }).unwrap(),
        )
        .with_reply_to("_reply.sum"),
    ]);
    let published_broker = broker.clone();

    let app = Router::new()
        .route("sum", |Json(request): Json<SumRequest>| async move {
            Ok::<Json<SumResponse>, CrabbyError>(Json(SumResponse {
                result: request.left + request.right,
            }))
        })
        .into_service(broker);

    app.serve().await.unwrap();

    let published = published_broker.published_messages();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].subject, "_reply.sum");
    assert_eq!(
        published[0].headers.as_ref().unwrap().get("content-type"),
        Some(&"application/json".to_string())
    );

    let reply: SumResponse = serde_json::from_slice(&published[0].payload).unwrap();
    assert_eq!(reply.result, 42);
}

#[tokio::test]
async fn service_level_error_topic_is_used_as_fallback() {
    let message_headers = HeaderMap::from([
        ("trace-id".to_string(), "abc".to_string()),
    ]);

    let broker = TestBroker::new(vec![
        TestBrokerMessage::new("jobs.run", b"payload".to_vec()).with_headers(message_headers.clone()),
    ]);
    let published_broker = broker.clone();

    let app = Router::new()
        .route("jobs.run", |_event: Event| async move { Err::<(), _>(TestError("job failed")) })
        .into_service(broker)
        .on_error("errors.default");

    app.serve().await.unwrap();

    let published = published_broker.published_messages();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].subject, "errors.default");
    assert_eq!(
        published[0].headers.as_ref().unwrap().get("content-type"),
        Some(&"application/json".to_string())
    );

    let envelope: ErrorEnvelope = serde_json::from_slice(&published[0].payload).unwrap();
    assert_eq!(envelope.subject, "jobs.run");
    assert_eq!(envelope.reply_to, None);
    assert_eq!(envelope.headers.unwrap(), message_headers);
    assert_eq!(envelope.payload, b"payload".to_vec());
    assert_eq!(envelope.error, "job failed");
}

#[tokio::test]
async fn router_error_topic_overrides_service_fallback_and_merges_headers() {
    let broker = TestBroker::new(vec![
        TestBrokerMessage::new("camera.sync", b"frame".to_vec()),
    ]);
    let published_broker = broker.clone();

    let route_headers = HeaderMap::from([
        ("x-router".to_string(), "camera".to_string()),
        ("x-shared".to_string(), "route".to_string()),
    ]);
    let service_headers = HeaderMap::from([
        ("x-service".to_string(), "default".to_string()),
        ("x-shared".to_string(), "service".to_string()),
    ]);

    let app = Router::new()
        .on_error("errors.camera")
        .error_headers(route_headers)
        .route("camera.sync", |_event: Event| async move { Err::<(), _>(TestError("camera failed")) })
        .into_service(broker)
        .dlq("errors.default")
        .error_headers(service_headers);

    app.serve().await.unwrap();

    let published = published_broker.published_messages();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].subject, "errors.camera");

    let headers = published[0].headers.as_ref().unwrap();
    assert_eq!(headers.get("content-type"), Some(&"application/json".to_string()));
    assert_eq!(headers.get("x-service"), Some(&"default".to_string()));
    assert_eq!(headers.get("x-router"), Some(&"camera".to_string()));
    assert_eq!(headers.get("x-shared"), Some(&"route".to_string()));
}

#[tokio::test]
async fn shutdown_hook_can_publish_final_message() {
    let broker = TestBroker::new(Vec::new());
    let published_broker = broker.clone();

    let app = Router::new()
        .into_service(broker)
        .with_graceful_shutdown(async {})
        .on_shutdown(|publisher| async move {
            publisher
                .publish("service.stopped", Json(Payload { id: 1 }))
                .await?;
            Ok(())
        });

    app.serve().await.unwrap();

    let published = published_broker.published_messages();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].subject, "service.stopped");
    assert_eq!(
        published[0].headers.as_ref().unwrap().get("content-type"),
        Some(&"application/json".to_string())
    );
}

#[tokio::test]
async fn publisher_request_returns_decodable_reply() {
    let broker = TestBroker::new(Vec::new()).with_request_replies(vec![
        TestBrokerMessage::new(
            "_reply.sum",
            serde_json::to_vec(&SumResponse { result: 42 }).unwrap(),
        )
        .with_headers(HeaderMap::from([(
            "content-type".to_string(),
            "application/json".to_string(),
        )])),
    ]);
    let recorded_broker = broker.clone();
    let publisher = Publisher::new(broker);

    let reply = publisher
        .request("rpc.sum", Json(SumRequest { left: 19, right: 23 }))
        .await
        .unwrap();

    assert_eq!(reply.subject(), "_reply.sum");
    assert_eq!(
        reply.headers().unwrap().get("content-type"),
        Some(&"application/json".to_string())
    );

    let body: SumResponse = reply.into_json().unwrap();
    assert_eq!(body.result, 42);

    let requests = recorded_broker.requested_messages();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].subject, "rpc.sum");
}
