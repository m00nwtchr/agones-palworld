//! Cross-module sanity test: build a Client against a wiremock Palworld server,
//! build a MockAgones, and run a single iteration of the poll loop's diff
//! machinery to confirm wiring across modules.

use agones_palworld::agones::{AgonesOps, AgonesState, MockAgones};
use agones_palworld::palworld::Client;
use agones_palworld::state::WorldState;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn poll_diff_drives_agones_counters_and_lists() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/players"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "players": [
                {"name": "alpha", "playerId": "P1", "level": 5}
            ]
        })))
        .mount(&server)
        .await;

    let client = Client::new(url::Url::parse(&server.uri()).unwrap(), "pw");
    let players = client.players().await.unwrap();
    let mut state = WorldState::new();
    let mock = MockAgones::new(AgonesState::Scheduled);
    let diff1 = state.observe(&players);
    for id in &diff1.joined {
        mock.counter_add("players", 1).await;
        mock.list_append("players", id).await;
    }
    assert_eq!(mock.counter("players"), 1);
    assert_eq!(mock.list("players"), vec!["P1".to_string()]);
}

#[tokio::test]
async fn shutdown_sequence_full_chain() {
    use wiremock::matchers::header;

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/save"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/announce"))
        .and(header("content-type", "application/json"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/shutdown"))
        .and(header("content-type", "application/json"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let client = Client::new(url::Url::parse(&server.uri()).unwrap(), "pw");
    let mock = MockAgones::new(AgonesState::Ready);
    client.save().await.unwrap();
    client.announce("bye").await.unwrap();
    client
        .shutdown(agones_palworld::palworld::ShutdownRequest {
            waittime: 5,
            message: "bye".into(),
        })
        .await
        .unwrap();
    mock.shutdown().await;
    assert_eq!(mock.current_state().await, AgonesState::Shutdown);
}
