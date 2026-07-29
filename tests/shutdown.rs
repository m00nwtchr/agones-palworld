use agones_palworld::agones::{AgonesState, MockAgones};
use agones_palworld::palworld::Client;
use agones_palworld::shutdown::run;
use std::time::Duration;

#[tokio::test]
async fn runs_save_then_announce_then_shutdown_then_sdk_shutdown() {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/save"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/announce"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/shutdown"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;

    let client = Client::new(url::Url::parse(&server.uri()).unwrap(), "pw");
    let mock = MockAgones::new(AgonesState::Ready);
    run(&client, &mock, Duration::from_secs(5), 10, "bye")
        .await
        .unwrap();
    let ops = mock.recorded();
    assert!(matches!(
        ops.last(),
        Some(agones_palworld::agones::AgonesOp::Shutdown)
    ));
}
