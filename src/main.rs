use std::sync::Arc;
use std::time::Duration;

use agones::Sdk;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::Notify;
use tokio::time::interval;

use agones_palworld::agones::{AgonesOps, Bridge};
use agones_palworld::config::Config;
use agones_palworld::observability::{Metrics, install as install_obs};
use agones_palworld::palworld::Client;
use agones_palworld::shutdown as do_shutdown;
use agones_palworld::state::WorldState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = Config::from_env()?;
    let (metrics, _guard) = install_obs(&cfg)?;

    let client = Client::new(cfg.api_url.clone(), cfg.admin_password.expose());
    let sdk = Sdk::new(None, None).await?;
    let bridge: Arc<dyn AgonesOps> = Arc::new(Bridge::new(sdk));

    wait_for_game(&client, &metrics).await?;
    bridge.ready().await;

    let stop = Arc::new(Notify::new());
    spawn_signal_listener(stop.clone());

    let poll_metrics = metrics.clone();
    let poll_client = client.clone();
    let poll_bridge = bridge.clone();
    let poll_stop = stop.clone();
    let poll_handle = tokio::spawn(async move {
        run_poll_loop(
            poll_client,
            poll_bridge,
            poll_metrics,
            cfg.poll_interval,
            poll_stop,
        )
        .await;
    });

    let health_stop = stop.clone();
    let health_bridge = bridge.clone();
    let health_metrics = metrics.clone();
    let health_interval = cfg.health_interval;
    let health_handle = tokio::spawn(async move {
        let mut t = interval(health_interval);
        loop {
            tokio::select! {
                _ = t.tick() => {
                    health_bridge.health_ping().await;
                    health_metrics.agones_ops.add(1, &[]);
                }
                _ = health_stop.notified() => break,
            }
        }
    });

    stop.notified().await;
    tracing::info!("SIGTERM received; running shutdown sequence");
    do_shutdown::run(
        &client,
        bridge.as_ref(),
        cfg.shutdown_save_timeout,
        cfg.shutdown_waittime,
        &cfg.shutdown_announce,
    )
    .await?;
    poll_handle.abort();
    health_handle.abort();
    Ok(())
}

async fn wait_for_game(
    client: &Client,
    metrics: &Metrics,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut backoff = Duration::from_millis(500);
    loop {
        match client.info().await {
            Ok(_) => return Ok(()),
            Err(e) => {
                metrics.poll_errors.add(1, &[]);
                tracing::warn!(error=%e, ?backoff, "palworld not ready");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(10));
            }
        }
    }
}

async fn run_poll_loop(
    client: Client,
    bridge: Arc<dyn AgonesOps>,
    metrics: Metrics,
    every: Duration,
    stop: Arc<Notify>,
) {
    let mut state = WorldState::new();
    let mut t = interval(every);
    loop {
        tokio::select! {
            _ = t.tick() => {
                metrics.poll_cycles.add(1, &[]);
                let snapshot = tokio::time::timeout(Duration::from_secs(5), async {
                    let players = client.players().await?;
                    let metrics_json = client.metrics().await?;
                    Ok::<_, agones_palworld::error::AppError>((players, metrics_json))
                }).await;
                let snapshot = match snapshot {
                    Ok(Ok(s)) => s,
                    Ok(Err(e)) => {
                        metrics.poll_errors.add(1, &[]);
                        tracing::debug!(error=%e, "poll failed");
                        continue;
                    }
                    Err(_) => {
                        metrics.poll_errors.add(1, &[]);
                        tracing::debug!("poll timeout");
                        continue;
                    }
                };
                let (players, m) = snapshot;
                let diff = state.observe(&players);
                for id in &diff.joined {
                    bridge.counter_add("players", 1).await;
                    metrics.agones_ops.add(1, &[]);
                    bridge.list_append("players", id).await;
                    metrics.agones_ops.add(1, &[]);
                    metrics.player_joins.add(1, &[]);
                }
                for id in &diff.left {
                    bridge.counter_add("players", -1).await;
                    metrics.agones_ops.add(1, &[]);
                    bridge.list_delete("players", id).await;
                    metrics.agones_ops.add(1, &[]);
                    metrics.player_leaves.add(1, &[]);
                }
                let cur = (m.currentplayernum as i64, m.maxplayernum as i64);
                let gs = bridge.current_state().await;
                if cur.0 > 0 && gs == agones_palworld::agones::AgonesState::Ready {
                    bridge.allocate().await;
                    metrics.agones_ops.add(1, &[]);
                }
                if cur.0 == 0 && gs == agones_palworld::agones::AgonesState::Allocated {
                    bridge.set_ready().await;
                    metrics.agones_ops.add(1, &[]);
                }
                metrics.palworld_server_fps.record(m.serverfps, &[]);
                metrics.palworld_server_frame_time_ms.record(m.serverframetime, &[]);
                metrics.palworld_server_uptime_seconds.record(m.uptime as i64, &[]);
                metrics.palworld_players_current.record(m.currentplayernum as i64, &[]);
                metrics.palworld_players_max.record(m.maxplayernum as i64, &[]);
                metrics.palworld_players_connected.record(state.players.len() as i64, &[]);
                metrics.palworld_world_base_camp_count.record(m.basecampnum as i64, &[]);
                metrics.palworld_world_in_game_days.record(m.days as i64, &[]);
                metrics.ready_state.record(gs as i64, &[]);
                metrics.last_poll_ts.record(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as i64,
                    &[],
                );
            }
            _ = stop.notified() => break,
        }
    }
}

fn spawn_signal_listener(stop: Arc<Notify>) {
    tokio::spawn(async move {
        let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM");
        let mut sigint = signal(SignalKind::interrupt()).expect("install SIGINT");
        tokio::select! {
            _ = sigterm.recv() => tracing::info!("SIGTERM"),
            _ = sigint.recv() => tracing::info!("SIGINT"),
        }
        stop.notify_waiters();
    });
}
