#![allow(clippy::result_large_err)]

use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Gauge};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use prometheus::Registry;
use tracing_subscriber::{EnvFilter, prelude::*};

use crate::config::Config;
use crate::error::{AppError, AppResult};

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

#[derive(Clone)]
pub struct Metrics {
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
}

impl Drop for Guard {
    fn drop(&mut self) {
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

    let exporter = opentelemetry_prometheus::exporter()
        .with_registry(prom_registry.clone())
        .build()
        .map_err(|e| AppError::Config(format!("prometheus exporter: {e}")))?;

    let provider = SdkMeterProvider::builder()
        .with_resource(resource)
        .with_reader(exporter)
        .build();
    opentelemetry::global::set_meter_provider(provider.clone());

    let meter = opentelemetry::global::meter("agones-palworld");
    let m = Metrics {
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
    let subscriber = tracing_subscriber::registry().with(filter);
    match std::env::var("LOG_FORMAT").as_deref() {
        Ok("json") => subscriber
            .with(tracing_subscriber::fmt::layer().json())
            .init(),
        _ => subscriber
            .with(tracing_subscriber::fmt::layer().pretty())
            .init(),
    }

    Ok((
        m,
        Guard {
            _provider: provider,
            registry: prom_registry,
        },
    ))
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
}
