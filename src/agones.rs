use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use agones::Sdk;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgonesState {
    #[default]
    Scheduled,
    Ready,
    Allocated,
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum AgonesOp {
    Connect,
    Ready,
    Allocate,
    SetReady,
    Shutdown,
    HealthPing,
    CounterAdd { name: String, delta: i64 },
    CounterSet { name: String, value: i64 },
    ListAppend { name: String, value: String },
    ListDelete { name: String, value: String },
    ListClear { name: String },
}

#[derive(Debug, Default)]
struct MockState {
    current: AgonesState,
    ops: Vec<AgonesOp>,
    counters: HashMap<String, i64>,
    lists: HashMap<String, Vec<String>>,
}

#[derive(Clone)]
pub struct MockAgones {
    state: Arc<Mutex<MockState>>,
}

impl MockAgones {
    pub fn new(initial: AgonesState) -> Self {
        Self {
            state: Arc::new(Mutex::new(MockState {
                current: initial,
                ..Default::default()
            })),
        }
    }
    pub fn recorded(&self) -> Vec<AgonesOp> {
        self.state.lock().unwrap().ops.clone()
    }
    pub fn counter(&self, name: &str) -> i64 {
        *self.state.lock().unwrap().counters.get(name).unwrap_or(&0)
    }
    pub fn list(&self, name: &str) -> Vec<String> {
        self.state
            .lock()
            .unwrap()
            .lists
            .get(name)
            .cloned()
            .unwrap_or_default()
    }
}

impl AgonesOps for MockAgones {
    fn connect(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            self.state.lock().unwrap().ops.push(AgonesOp::Connect);
        })
    }
    fn ready(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let mut s = self.state.lock().unwrap();
            s.ops.push(AgonesOp::Ready);
            s.current = AgonesState::Ready;
        })
    }
    fn allocate(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let mut s = self.state.lock().unwrap();
            s.ops.push(AgonesOp::Allocate);
            s.current = AgonesState::Allocated;
        })
    }
    fn set_ready(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let mut s = self.state.lock().unwrap();
            s.ops.push(AgonesOp::SetReady);
            s.current = AgonesState::Ready;
        })
    }
    fn shutdown(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let mut s = self.state.lock().unwrap();
            s.ops.push(AgonesOp::Shutdown);
            s.current = AgonesState::Shutdown;
        })
    }
    fn health_ping(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            self.state.lock().unwrap().ops.push(AgonesOp::HealthPing);
        })
    }
    fn counter_add<'a>(&'a self, name: &'a str, delta: i64) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let mut s = self.state.lock().unwrap();
            s.ops.push(AgonesOp::CounterAdd {
                name: name.into(),
                delta,
            });
            *s.counters.entry(name.into()).or_default() += delta;
        })
    }
    fn counter_set<'a>(&'a self, name: &'a str, value: i64) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let mut s = self.state.lock().unwrap();
            s.ops.push(AgonesOp::CounterSet {
                name: name.into(),
                value,
            });
            s.counters.insert(name.into(), value);
        })
    }
    fn list_append<'a>(&'a self, name: &'a str, value: &'a str) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let mut s = self.state.lock().unwrap();
            s.ops.push(AgonesOp::ListAppend {
                name: name.into(),
                value: value.into(),
            });
            s.lists.entry(name.into()).or_default().push(value.into());
        })
    }
    fn list_delete<'a>(&'a self, name: &'a str, value: &'a str) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let mut s = self.state.lock().unwrap();
            s.ops.push(AgonesOp::ListDelete {
                name: name.into(),
                value: value.into(),
            });
            if let Some(v) = s.lists.get_mut(name) {
                v.retain(|x| x != value);
            }
        })
    }
    fn list_clear<'a>(&'a self, name: &'a str) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let mut s = self.state.lock().unwrap();
            s.ops.push(AgonesOp::ListClear { name: name.into() });
            s.lists.insert(name.into(), Vec::new());
        })
    }
    fn current_state(&self) -> BoxFuture<'_, AgonesState> {
        Box::pin(async move { self.state.lock().unwrap().current })
    }
}

pub trait AgonesOps: Send + Sync {
    fn connect(&self) -> BoxFuture<'_, ()>;
    fn ready(&self) -> BoxFuture<'_, ()>;
    fn allocate(&self) -> BoxFuture<'_, ()>;
    fn set_ready(&self) -> BoxFuture<'_, ()>;
    fn shutdown(&self) -> BoxFuture<'_, ()>;
    fn health_ping(&self) -> BoxFuture<'_, ()>;
    fn counter_add<'a>(&'a self, name: &'a str, delta: i64) -> BoxFuture<'a, ()>;
    fn counter_set<'a>(&'a self, name: &'a str, value: i64) -> BoxFuture<'a, ()>;
    fn list_append<'a>(&'a self, name: &'a str, value: &'a str) -> BoxFuture<'a, ()>;
    fn list_delete<'a>(&'a self, name: &'a str, value: &'a str) -> BoxFuture<'a, ()>;
    fn list_clear<'a>(&'a self, name: &'a str) -> BoxFuture<'a, ()>;
    fn current_state(&self) -> BoxFuture<'_, AgonesState>;
}

#[derive(Clone)]
pub struct Bridge {
    sdk: Sdk,
}

impl Bridge {
    pub fn new(sdk: Sdk) -> Self {
        Self { sdk }
    }
}

impl AgonesOps for Bridge {
    fn connect(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            tracing::debug!(
                "sdk.connect no-op; Bridge holds Sdk already connected at construction"
            );
        })
    }
    fn ready(&self) -> BoxFuture<'_, ()> {
        let mut sdk = self.sdk.clone();
        Box::pin(async move {
            sdk.ready()
                .await
                .map_err(|e| tracing::error!(error = %e, "sdk.ready failed"))
                .ok();
        })
    }
    fn allocate(&self) -> BoxFuture<'_, ()> {
        let mut sdk = self.sdk.clone();
        Box::pin(async move {
            sdk.allocate()
                .await
                .map_err(|e| tracing::error!(error = %e, "sdk.allocate failed"))
                .ok();
        })
    }
    fn set_ready(&self) -> BoxFuture<'_, ()> {
        let mut sdk = self.sdk.clone();
        Box::pin(async move {
            sdk.ready()
                .await
                .map_err(|e| tracing::error!(error = %e, "sdk.ready (returning to Ready) failed"))
                .ok();
        })
    }
    fn shutdown(&self) -> BoxFuture<'_, ()> {
        let mut sdk = self.sdk.clone();
        Box::pin(async move {
            sdk.shutdown()
                .await
                .map_err(|e| tracing::error!(error = %e, "sdk.shutdown failed"))
                .ok();
        })
    }
    fn health_ping(&self) -> BoxFuture<'_, ()> {
        let sdk = self.sdk.clone();
        Box::pin(async move {
            sdk.health_check()
                .send(())
                .await
                .map_err(|e| tracing::error!(error = %e, "sdk.health failed"))
                .ok();
        })
    }
    fn counter_add<'a>(&'a self, name: &'a str, delta: i64) -> BoxFuture<'a, ()> {
        let mut beta = self.sdk.beta().clone();
        Box::pin(async move {
            if delta >= 0 {
                beta.increment_counter(name, delta)
                    .await
                    .map_err(|e| tracing::error!(error = %e, name, "increment failed"))
                    .ok();
            } else {
                beta.decrement_counter(name, -delta)
                    .await
                    .map_err(|e| tracing::error!(error = %e, name, "decrement failed"))
                    .ok();
            }
        })
    }
    fn counter_set<'a>(&'a self, name: &'a str, value: i64) -> BoxFuture<'a, ()> {
        let mut beta = self.sdk.beta().clone();
        Box::pin(async move {
            beta.set_counter_count(name, value)
                .await
                .map_err(|e| tracing::error!(error = %e, name, value, "set_counter_count failed"))
                .ok();
        })
    }
    fn list_append<'a>(&'a self, name: &'a str, value: &'a str) -> BoxFuture<'a, ()> {
        let mut beta = self.sdk.beta().clone();
        Box::pin(async move {
            beta.append_list_value(name, value)
                .await
                .map_err(|e| tracing::error!(error = %e, name, value, "append failed"))
                .ok();
        })
    }
    fn list_delete<'a>(&'a self, name: &'a str, value: &'a str) -> BoxFuture<'a, ()> {
        let mut beta = self.sdk.beta().clone();
        Box::pin(async move {
            beta.delete_list_value(name, value)
                .await
                .map_err(|e| tracing::error!(error = %e, name, value, "delete failed"))
                .ok();
        })
    }
    fn list_clear<'a>(&'a self, name: &'a str) -> BoxFuture<'a, ()> {
        let mut beta = self.sdk.beta().clone();
        Box::pin(async move {
            beta.set_list_capacity(name, 0)
                .await
                .map_err(|e| tracing::error!(error = %e, name, "set_list_capacity(0) failed"))
                .ok();
        })
    }
    fn current_state(&self) -> BoxFuture<'_, AgonesState> {
        let mut sdk = self.sdk.clone();
        Box::pin(async move {
            let state = match sdk.get_gameserver().await {
                Ok(gs) => gs
                    .status
                    .as_ref()
                    .map(|s| s.state.clone())
                    .unwrap_or_default(),
                Err(_) => String::new(),
            };
            parse_state(&state)
        })
    }
}

fn parse_state(s: &str) -> AgonesState {
    match s {
        "Scheduled" => AgonesState::Scheduled,
        "Ready" => AgonesState::Ready,
        "Allocated" => AgonesState::Allocated,
        "Shutdown" => AgonesState::Shutdown,
        _ => AgonesState::Scheduled,
    }
}

#[cfg(test)]
mod bridge_tests {
    use super::*;
    #[test]
    fn bridge_is_agones_ops() {
        fn assert_ops<T: AgonesOps>() {}
        assert_ops::<Bridge>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mock_records_state_transitions() {
        let m = MockAgones::new(AgonesState::Ready);
        assert_eq!(m.current_state().await, AgonesState::Ready);
        m.allocate().await;
        assert_eq!(m.current_state().await, AgonesState::Allocated);
        m.set_ready().await;
        assert_eq!(m.current_state().await, AgonesState::Ready);
        m.shutdown().await;
        assert_eq!(m.current_state().await, AgonesState::Shutdown);
    }

    #[tokio::test]
    async fn mock_records_counter_and_list_ops() {
        let m = MockAgones::new(AgonesState::Ready);
        m.counter_add("players", 1).await;
        m.counter_add("players", 1).await;
        m.counter_add("players", -1).await;
        m.list_append("players", "p1").await;
        m.list_append("players", "p2").await;
        m.list_delete("players", "p1").await;
        assert_eq!(m.counter("players"), 1);
        assert_eq!(m.list("players"), vec!["p2".to_string()]);
    }
}
