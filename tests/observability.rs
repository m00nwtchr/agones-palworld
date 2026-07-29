use std::time::Duration;

use agones_palworld::config::Config;
use agones_palworld::observability::{self, install};
use serial_test::serial;

fn unique_metrics_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral")
        .local_addr()
        .expect("local_addr")
        .port()
}

fn make_cfg(port: u16) -> Config {
    Config {
        api_url: url::Url::parse("http://localhost:8211/").unwrap(),
        admin_password: agones_palworld::config::SecretString::new("hunter2"),
        poll_interval: Duration::from_secs(5),
        health_interval: Duration::from_secs(2),
        shutdown_save_timeout: Duration::from_secs(30),
        shutdown_waittime: 30,
        shutdown_announce: "bye".into(),
        metrics_port: port,
        metrics_host: "127.0.0.1".into(),
        disable_prometheus: false,
        otel_endpoint: None,
        pod_name: "test-pod".into(),
        pod_namespace: "default".into(),
    }
}

#[tokio::test]
#[serial]
async fn http_scrape_returns_metric_text() {
    let port = unique_metrics_port();
    let cfg = make_cfg(port);
    let (_metrics, guard) = install(&cfg).expect("install");

    let meter = opentelemetry::global::meter("test");
    let counter = meter.u64_counter("palworld.sidecar.poll_cycles").build();
    counter.add(42, &[opentelemetry::KeyValue::new("k", "v")]);

    let url = format!("http://127.0.0.1:{port}/metrics");
    let mut body = String::new();
    for _ in 0..50 {
        match reqwest::get(&url).await {
            Ok(r) => {
                assert_eq!(r.status(), reqwest::StatusCode::OK);
                body = r.text().await.unwrap();
                break;
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    assert!(
        !body.is_empty(),
        "metrics server never came up on port {port}"
    );

    assert!(
        body.contains("palworld_sidecar_poll_cycles"),
        "metric missing from scrape; got:\n{body}"
    );
    assert!(
        body.contains("42"),
        "counter value missing from scrape; got:\n{body}"
    );

    drop(guard);
    let _ = observability::EXPECTED_NAMES;
}

#[tokio::test]
#[serial]
async fn install_skips_server_when_disabled() {
    let port = unique_metrics_port();
    let cfg = Config {
        disable_prometheus: true,
        metrics_port: port,
        ..make_cfg(port)
    };
    let (_metrics, guard) = install(&cfg).expect("install");
    tokio::time::sleep(Duration::from_millis(100)).await;
    let url = format!("http://127.0.0.1:{port}/metrics");
    let res = reqwest::get(&url).await;
    assert!(res.is_err(), "server should not be listening when disabled");
    drop(guard);
}
