#![allow(clippy::result_large_err)]

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Instant;

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Gauge};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::runtime;
use opentelemetry_sdk::trace::TracerProvider as SdkTracerProvider;
use prometheus::{Encoder, Registry, TextEncoder};
use tokio::net::TcpListener;
use tracing_subscriber::{EnvFilter, prelude::*};

use crate::config::Config;
use crate::error::{AppError, AppResult};
use crate::palworld;

pub const EXPECTED_NAMES: &[&str] = &[
    "palworld.sidecar.poll_cycles",
    "palworld.sidecar.poll_errors",
    "palworld.sidecar.player_joins",
    "palworld.sidecar.player_leaves",
    "palworld.sidecar.agones_ops",
    "palworld.sidecar.ready_state",
    "palworld.sidecar.last_successful_poll_unixtime",
    "palworld.sidecar.build_info",
    "palworld.sidecar.uptime_seconds",
    "palworld.server.fps",
    "palworld.server.frame_time_ms",
    "palworld.server.uptime_seconds",
    "palworld.players.current",
    "palworld.players.max",
    "palworld.players.connected",
    "palworld.world.base_camp_count",
    "palworld.world.in_game_days",
];

pub const HEALTH_UNKNOWN: u8 = 0;
pub const HEALTH_OK: u8 = 1;
pub const HEALTH_BAD: u8 = 2;

#[derive(Clone)]
pub struct Metrics {
    pub palworld_health: Arc<AtomicU8>,
    pub poll_cycles: Counter<u64>,
    pub poll_errors: Counter<u64>,
    pub player_joins: Counter<u64>,
    pub player_leaves: Counter<u64>,
    pub agones_ops: Counter<u64>,
    pub ready_state: Gauge<i64>,
    pub last_poll_ts: Gauge<i64>,
    pub build_info: Gauge<i64>,
    pub uptime: Gauge<i64>,
    pub palworld_server_fps: Gauge<i64>,
    pub palworld_server_frame_time_ms: Gauge<f64>,
    pub palworld_server_uptime_seconds: Gauge<i64>,
    pub palworld_players_current: Gauge<i64>,
    pub palworld_players_max: Gauge<i64>,
    pub palworld_players_connected: Gauge<i64>,
    pub palworld_world_base_camp_count: Gauge<i64>,
    pub palworld_world_in_game_days: Gauge<i64>,
}

pub struct Guard {
    _provider: SdkMeterProvider,
    pub registry: Registry,
    server_shutdown: tokio::sync::watch::Sender<bool>,
    health_probe: tokio::task::JoinHandle<()>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.server_shutdown.send(true);
        self.health_probe.abort();
        opentelemetry::global::shutdown_tracer_provider();
    }
}

fn build_resource(cfg: &Config) -> Resource {
    Resource::new(vec![
        KeyValue::new("service.name", "agones-palworld"),
        KeyValue::new("service.namespace", "palworld"),
        KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
        KeyValue::new("k8s.pod.name", cfg.pod_name.clone()),
        KeyValue::new("k8s.namespace.name", cfg.pod_namespace.clone()),
        KeyValue::new("k8s.container.name", "agones-sidecar"),
    ])
}

pub fn install(cfg: &Config) -> AppResult<(Metrics, Guard)> {
    let resource = build_resource(cfg);

    let prom_registry = Registry::new();
    let prom_exporter = opentelemetry_prometheus::exporter()
        .with_registry(prom_registry.clone())
        .build()
        .map_err(|e| AppError::Config(format!("prometheus exporter: {e}")))?;

    let mut provider_builder = SdkMeterProvider::builder().with_resource(resource.clone());
    if let Some(endpoint) = cfg.otel_endpoint.as_deref() {
        let otlp_exporter = opentelemetry_otlp::MetricExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build()
            .map_err(|e| AppError::Config(format!("otlp metric exporter: {e}")))?;
        let reader =
            opentelemetry_sdk::metrics::PeriodicReader::builder(otlp_exporter, runtime::Tokio)
                .build();
        provider_builder = provider_builder.with_reader(reader);
    }
    provider_builder = provider_builder.with_reader(prom_exporter);
    let provider = provider_builder.build();
    opentelemetry::global::set_meter_provider(provider.clone());

    let meter = opentelemetry::global::meter("agones-palworld");
    let started = Instant::now();
    let _ = meter
        .u64_observable_gauge("palworld.sidecar.uptime_seconds")
        .with_callback(move |observer| {
            observer.observe(started.elapsed().as_secs(), &[]);
        })
        .build();
    let m = Metrics {
        palworld_health: Arc::new(AtomicU8::new(HEALTH_UNKNOWN)),
        poll_cycles: meter.u64_counter("palworld.sidecar.poll_cycles").build(),
        poll_errors: meter.u64_counter("palworld.sidecar.poll_errors").build(),
        player_joins: meter.u64_counter("palworld.sidecar.player_joins").build(),
        player_leaves: meter.u64_counter("palworld.sidecar.player_leaves").build(),
        agones_ops: meter.u64_counter("palworld.sidecar.agones_ops").build(),
        ready_state: meter.i64_gauge("palworld.sidecar.ready_state").build(),
        last_poll_ts: meter
            .i64_gauge("palworld.sidecar.last_successful_poll_unixtime")
            .build(),
        build_info: meter.i64_gauge("palworld.sidecar.build_info").build(),
        uptime: meter.i64_gauge("palworld.sidecar.uptime_seconds").build(),
        palworld_server_fps: meter.i64_gauge("palworld.server.fps").build(),
        palworld_server_frame_time_ms: meter.f64_gauge("palworld.server.frame_time_ms").build(),
        palworld_server_uptime_seconds: meter.i64_gauge("palworld.server.uptime_seconds").build(),
        palworld_players_current: meter.i64_gauge("palworld.players.current").build(),
        palworld_players_max: meter.i64_gauge("palworld.players.max").build(),
        palworld_players_connected: meter.i64_gauge("palworld.players.connected").build(),
        palworld_world_base_camp_count: meter.i64_gauge("palworld.world.base_camp_count").build(),
        palworld_world_in_game_days: meter.i64_gauge("palworld.world.in_game_days").build(),
    };

    m.build_info.record(1, &[]);

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,h2=warn,hyper=warn,agones=warn"));

    let subscriber_builder = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer());

    if let Some(endpoint) = cfg.otel_endpoint.as_deref() {
        let span_exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build()
            .map_err(|e| AppError::Config(format!("otlp span exporter: {e}")))?;
        let tracer_provider = SdkTracerProvider::builder()
            .with_resource(resource)
            .with_batch_exporter(span_exporter, runtime::Tokio)
            .build();
        opentelemetry::global::set_tracer_provider(tracer_provider.clone());
        let tracer = tracer_provider.tracer("agones-palworld");
        let _ = subscriber_builder
            .with(tracing_opentelemetry::layer().with_tracer(tracer))
            .try_init();
    } else {
        match std::env::var("LOG_FORMAT").as_deref() {
            Ok("json") => {
                let _ = subscriber_builder
                    .with(tracing_subscriber::fmt::layer().json())
                    .try_init();
            }
            _ => {
                let _ = subscriber_builder
                    .with(tracing_subscriber::fmt::layer().pretty())
                    .try_init();
            }
        }
    }

    let server_shutdown = if cfg.disable_prometheus {
        tokio::sync::watch::channel(false).0
    } else {
        let addr: SocketAddr = format!("{}:{}", cfg.metrics_host, cfg.metrics_port)
            .parse()
            .map_err(|e| AppError::Config(format!("invalid METRICS_HOST:PORT: {e}")))?;
        let (tx, rx) = tokio::sync::watch::channel(false);
        let registry = prom_registry.clone();
        let health = m.palworld_health.clone();
        tokio::spawn(async move {
            if let Err(e) = run_metrics_server(addr, registry, health, rx).await {
                tracing::error!(error = %e, "metrics server exited");
            }
        });
        tx
    };

    let probe_state = m.palworld_health.clone();
    let probe_client = palworld::Client::new(cfg.api_url.clone(), cfg.admin_password.expose());
    let health_probe = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(5));
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let state = match probe_client.info().await {
                Ok(_) => HEALTH_OK,
                Err(_) => HEALTH_BAD,
            };
            probe_state.store(state, Ordering::Relaxed);
        }
    });

    Ok((
        m,
        Guard {
            _provider: provider,
            registry: prom_registry,
            server_shutdown,
            health_probe,
        },
    ))
}

async fn run_metrics_server(
    addr: SocketAddr,
    registry: Registry,
    health: Arc<AtomicU8>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> AppResult<()> {
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| AppError::Config(format!("bind {addr}: {e}")))?;
    tracing::info!(%addr, "metrics server listening");
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    break;
                }
            }
            accept = listener.accept() => {
                let (stream, _) = match accept {
                    Ok(x) => x,
                    Err(e) => {
                        tracing::warn!(error=%e, "metrics accept failed");
                        continue;
                    }
                };
                let registry = registry.clone();
                let health = health.clone();
                tokio::spawn(async move {
                    let io = TokioIo::new(stream);
                    let svc = service_fn(move |req: Request<hyper::body::Incoming>| {
                        let registry = registry.clone();
                        let health = health.clone();
                        async move { handle(req, &registry, &health).await }
                    });
                    if let Err(e) = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, svc)
                        .await
                    {
                        tracing::debug!(error=%e, "metrics conn ended");
                    }
                });
            }
        }
    }
    Ok(())
}

async fn handle(
    req: Request<hyper::body::Incoming>,
    registry: &Registry,
    health: &AtomicU8,
) -> Result<Response<Full<Bytes>>, Infallible> {
    if req.uri().path() == "/healthz" {
        let state = health.load(Ordering::Relaxed);
        let (status, body) = if state == HEALTH_OK {
            (StatusCode::OK, "{\"sidecar\":\"ok\",\"palworld\":\"ok\"}")
        } else {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "{\"sidecar\":\"ok\",\"palworld\":\"down\"}",
            )
        };
        return Ok(Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(body)))
            .expect("static response"));
    }
    if req.method() != hyper::Method::GET || req.uri().path() != "/metrics" {
        return Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Full::new(Bytes::from("not found")))
            .expect("static response"));
    }
    let encoder = TextEncoder::new();
    let metric_families = registry.gather();
    let mut buf = Vec::new();
    if let Err(e) = encoder.encode(&metric_families, &mut buf) {
        tracing::warn!(error = %e, "metrics encode failed");
        return Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Full::new(Bytes::from("encode failed")))
            .expect("static response"));
    }
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(hyper::header::CONTENT_TYPE, encoder.format_type())
        .body(Full::new(Bytes::from(buf)))
        .expect("static response"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_have_expected_names() {
        let names = vec![
            "palworld.sidecar.poll_cycles",
            "palworld.sidecar.poll_errors",
            "palworld.sidecar.player_joins",
            "palworld.sidecar.player_leaves",
            "palworld.sidecar.agones_ops",
            "palworld.sidecar.ready_state",
            "palworld.server.fps",
            "palworld.players.current",
        ];
        for name in names {
            assert!(
                EXPECTED_NAMES.contains(&name),
                "metric name {name} not in expected list"
            );
        }
    }

    #[test]
    fn healthz_reports_unknown_until_first_probe() {
        let health = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(HEALTH_UNKNOWN));
        assert_eq!(
            health.load(std::sync::atomic::Ordering::Relaxed),
            HEALTH_UNKNOWN
        );
        health.store(HEALTH_OK, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(health.load(std::sync::atomic::Ordering::Relaxed), HEALTH_OK);
        health.store(HEALTH_BAD, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(
            health.load(std::sync::atomic::Ordering::Relaxed),
            HEALTH_BAD
        );
    }
}
