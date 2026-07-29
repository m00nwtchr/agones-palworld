use agones_palworld::palworld::{Client, ShutdownRequest};
use pretty_assertions::assert_eq;
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client_for(uri: &str) -> Client {
    Client::new(url::Url::parse(uri).unwrap(), "hunter2")
}

#[tokio::test]
async fn info_returns_server_info() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/info"))
        .and(header("authorization", "Basic Omh1bnRlcjI="))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "version": "v1.0.0", "servername": "Test", "description": "x",
            "worldguid": "GUID",
        })))
        .mount(&server)
        .await;
    let info = client_for(&server.uri()).info().await.unwrap();
    assert_eq!(info.version, "v1.0.0");
    assert_eq!(info.worldguid, "GUID");
}

#[tokio::test]
async fn players_returns_list() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/players"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "players": [
                {"name": "alpha", "playerId": "P1", "level": 5},
                {"name": "beta",  "playerId": "P2", "level": 7}
            ]
        })))
        .mount(&server)
        .await;
    let ps = client_for(&server.uri()).players().await.unwrap();
    assert_eq!(ps.len(), 2);
    let mut ids: Vec<_> = ps.iter().map(|p| p.player_id.clone()).collect();
    ids.sort();
    assert_eq!(ids, vec!["P1", "P2"]);
}

#[tokio::test]
async fn shutdown_sends_waittime_and_message() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/shutdown"))
        .and(header("content-type", "application/json"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    client_for(&server.uri())
        .shutdown(ShutdownRequest {
            waittime: 30,
            message: "bye".into(),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn save_posts_to_endpoint() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/save"))
        .and(header("authorization", "Basic Omh1bnRlcjI="))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    client_for(&server.uri()).save().await.unwrap();
}

#[tokio::test]
async fn announce_posts_message_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/announce"))
        .and(header("authorization", "Basic Omh1bnRlcjI="))
        .and(header("content-type", "application/json"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    client_for(&server.uri())
        .announce("maintenance in 5 minutes")
        .await
        .unwrap();
}
