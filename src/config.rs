#![allow(unsafe_code)]
#![allow(clippy::result_large_err)]

use std::time::Duration;

use url::Url;

use crate::error::{AppError, AppResult};

#[derive(Debug)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        use std::ptr;
        unsafe {
            let bytes = self.0.as_mut_ptr();
            for i in 0..self.0.len() {
                ptr::write_volatile(bytes.add(i), 0);
            }
            self.0.clear();
        }
    }
}

#[derive(Debug)]
pub struct Config {
    pub api_url: Url,
    pub admin_password: SecretString,
    pub poll_interval: Duration,
    pub health_interval: Duration,
    pub shutdown_save_timeout: Duration,
    pub shutdown_waittime: u32,
    pub shutdown_announce: String,
    pub metrics_port: u16,
    pub metrics_host: String,
    pub disable_prometheus: bool,
    pub otel_endpoint: Option<String>,
    pub pod_name: String,
    pub pod_namespace: String,
}

fn env_required(key: &str) -> AppResult<String> {
    std::env::var(key).map_err(|_| AppError::Config(format!("missing env var {key}")))
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.into())
}

fn env_u64_or(key: &str, default: u64) -> AppResult<u64> {
    match std::env::var(key) {
        Ok(v) => v
            .parse()
            .map_err(|_| AppError::Config(format!("{key} must be u64"))),
        Err(_) => Ok(default),
    }
}

fn env_u32_or(key: &str, default: u32) -> AppResult<u32> {
    match std::env::var(key) {
        Ok(v) => v
            .parse()
            .map_err(|_| AppError::Config(format!("{key} must be u32"))),
        Err(_) => Ok(default),
    }
}

fn env_u16_or(key: &str, default: u16) -> AppResult<u16> {
    match std::env::var(key) {
        Ok(v) => v
            .parse()
            .map_err(|_| AppError::Config(format!("{key} must be u16"))),
        Err(_) => Ok(default),
    }
}

impl Config {
    pub fn from_env() -> AppResult<Self> {
        let api_url_raw = env_required("PALWORLD_API_URL")?;
        let api_url = Url::parse(&api_url_raw)
            .map_err(|_| AppError::Config(format!("invalid PALWORLD_API_URL: {api_url_raw}")))?;
        let admin_password = SecretString::new(env_required("PALWORLD_ADMIN_PASSWORD")?);
        let poll_interval = Duration::from_secs(env_u64_or("POLL_INTERVAL_SECS", 5)?);
        let health_interval = Duration::from_secs(env_u64_or("HEALTH_INTERVAL_SECS", 2)?);
        let shutdown_save_timeout =
            Duration::from_secs(env_u64_or("SHUTDOWN_SAVE_TIMEOUT_SECS", 30)?);
        let shutdown_waittime = env_u32_or("SHUTDOWN_WAITTIME_SECS", 30)?;
        let shutdown_announce = env_or("SHUTDOWN_ANNOUNCE_MESSAGE", "Server shutting down");
        let metrics_port = env_u16_or("METRICS_PORT", 9090)?;
        let metrics_host = env_or("METRICS_HOST", "0.0.0.0");
        let disable_prometheus = matches!(
            std::env::var("DISABLE_PROMETHEUS").as_deref(),
            Ok("1") | Ok("true") | Ok("TRUE")
        );
        let otel_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .ok()
            .filter(|s| !s.is_empty());
        let pod_name = env_or("POD_NAME", "unknown");
        let pod_namespace = env_or("POD_NAMESPACE", "default");
        Ok(Self {
            api_url,
            admin_password,
            poll_interval,
            health_interval,
            shutdown_save_timeout,
            shutdown_waittime,
            shutdown_announce,
            metrics_port,
            metrics_host,
            disable_prometheus,
            otel_endpoint,
            pod_name,
            pod_namespace,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    fn clear_env() {
        for k in [
            "PALWORLD_API_URL",
            "PALWORLD_ADMIN_PASSWORD",
            "POLL_INTERVAL_SECS",
            "HEALTH_INTERVAL_SECS",
            "SHUTDOWN_SAVE_TIMEOUT_SECS",
            "SHUTDOWN_WAITTIME_SECS",
            "SHUTDOWN_ANNOUNCE_MESSAGE",
            "METRICS_PORT",
            "METRICS_HOST",
            "DISABLE_PROMETHEUS",
            "OTEL_EXPORTER_OTLP_ENDPOINT",
            "POD_NAME",
            "POD_NAMESPACE",
        ] {
            unsafe {
                env::remove_var(k);
            }
        }
    }

    #[test]
    fn requires_api_url() {
        clear_env();
        unsafe {
            env::set_var("PALWORLD_ADMIN_PASSWORD", "x");
        }
        let err = Config::from_env().unwrap_err();
        assert!(matches!(err, AppError::Config(_)), "got: {err:?}");
    }

    #[test]
    fn reads_all_values() {
        clear_env();
        unsafe {
            env::set_var("PALWORLD_API_URL", "http://localhost:8211");
        }
        unsafe {
            env::set_var("PALWORLD_ADMIN_PASSWORD", "hunter2");
        }
        unsafe {
            env::set_var("POLL_INTERVAL_SECS", "5");
        }
        unsafe {
            env::set_var("METRICS_PORT", "9090");
        }
        unsafe {
            env::set_var("POD_NAME", "palworld-0");
        }
        unsafe {
            env::set_var("POD_NAMESPACE", "games");
        }
        let c = Config::from_env().expect("config");
        assert_eq!(c.api_url.as_str(), "http://localhost:8211/");
        assert_eq!(c.poll_interval, Duration::from_secs(5));
        assert_eq!(c.metrics_port, 9090);
        assert!(c.otel_endpoint.is_none());
    }
}
