#![allow(unsafe_code)]
#![allow(clippy::result_large_err)]

use clap::Parser;
use url::Url;

use crate::error::AppResult;

pub struct SecretString(String);

impl SecretString {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretString(\"***\")")
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

#[derive(Parser, Debug)]
#[command(version, about = "Agones sidecar for Palworld dedicated server")]
pub struct Config {
    #[arg(
        long,
        env = "PALWORLD_API_URL",
        default_value = "http://127.0.0.1:8211"
    )]
    pub api_url: Url,
    #[arg(long, env = "PALWORLD_ADMIN_PASSWORD")]
    pub admin_password: SecretString,
    #[arg(long, env = "POLL_INTERVAL_SECS", default_value_t = 5)]
    pub poll_interval_secs: u64,
    #[arg(long, env = "HEALTH_INTERVAL_SECS", default_value_t = 2)]
    pub health_interval_secs: u64,
    #[arg(long, env = "SHUTDOWN_SAVE_TIMEOUT_SECS", default_value_t = 30)]
    pub shutdown_save_timeout_secs: u64,
    #[arg(long, env = "SHUTDOWN_WAITTIME_SECS", default_value_t = 30)]
    pub shutdown_waittime_secs: u32,
    #[arg(
        long,
        env = "SHUTDOWN_ANNOUNCE_MESSAGE",
        default_value = "Server shutting down"
    )]
    pub shutdown_announce: String,
    #[arg(long, env = "METRICS_PORT", default_value_t = 9090)]
    pub metrics_port: u16,
    #[arg(long, env = "METRICS_HOST", default_value = "::")]
    pub metrics_host: String,
    #[arg(long, env = "DISABLE_PROMETHEUS", default_value_t = false)]
    pub disable_prometheus: bool,
    #[arg(long, env = "OTEL_EXPORTER_OTLP_ENDPOINT")]
    pub otel_endpoint: Option<String>,
    #[arg(long, env = "POD_NAME", default_value = "unknown")]
    pub pod_name: String,
    #[arg(long, env = "POD_NAMESPACE", default_value = "default")]
    pub pod_namespace: String,
}

impl Config {
    pub fn load() -> AppResult<Self> {
        let cfg = <Self as Parser>::parse();
        if cfg.api_url.host_str() == Some("localhost") {
            tracing::warn!(
                "api_url uses localhost; prefer 127.0.0.1 to avoid IPv6 lookups on dualstack"
            );
        }
        Ok(cfg)
    }
}

impl Clone for SecretString {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl std::str::FromStr for SecretString {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn requires_admin_password_via_cli_or_env() {
        let err = Config::try_parse_from(["agones-palworld"]).unwrap_err();
        assert!(err.to_string().contains("admin-password"), "got: {err}");
    }

    #[test]
    #[serial]
    fn reads_all_values_from_cli() {
        let c = Config::try_parse_from([
            "agones-palworld",
            "--api-url",
            "http://127.0.0.1:8211",
            "--admin-password",
            "hunter2",
            "--poll-interval-secs",
            "5",
            "--metrics-port",
            "9090",
            "--pod-name",
            "palworld-0",
            "--pod-namespace",
            "games",
        ])
        .expect("config");
        assert_eq!(c.api_url.as_str(), "http://127.0.0.1:8211/");
        assert_eq!(c.poll_interval_secs, 5);
        assert_eq!(c.metrics_port, 9090);
        assert_eq!(c.pod_name, "palworld-0");
        assert_eq!(c.pod_namespace, "games");
        assert_eq!(c.metrics_host, "::");
        assert!(c.otel_endpoint.is_none());
        assert!(!c.disable_prometheus);
    }

    #[test]
    #[serial]
    fn env_vars_override_defaults() {
        unsafe {
            std::env::set_var("PALWORLD_API_URL", "http://127.0.0.1:8211");
            std::env::set_var("PALWORLD_ADMIN_PASSWORD", "hunter2");
        }
        let c = Config::parse_from(["agones-palworld"]);
        assert_eq!(c.api_url.as_str(), "http://127.0.0.1:8211/");
        assert_eq!(c.metrics_host, "::");
        unsafe {
            std::env::remove_var("PALWORLD_API_URL");
            std::env::remove_var("PALWORLD_ADMIN_PASSWORD");
        }
    }

    #[test]
    #[serial]
    fn cli_args_override_env_vars() {
        unsafe {
            std::env::set_var("PALWORLD_API_URL", "http://127.0.0.1:8211");
            std::env::set_var("PALWORLD_ADMIN_PASSWORD", "env-pw");
        }
        let c = Config::parse_from([
            "agones-palworld",
            "--admin-password",
            "cli-pw",
            "--api-url",
            "http://127.0.0.1:8211",
        ]);
        assert_eq!(c.admin_password.expose(), "cli-pw");
        unsafe {
            std::env::remove_var("PALWORLD_API_URL");
            std::env::remove_var("PALWORLD_ADMIN_PASSWORD");
        }
    }

    #[test]
    fn secret_string_debug_redacts_password() {
        let secret = SecretString::new("hunter2-supersecret");
        let dbg = format!("{:?}", secret);
        assert!(
            !dbg.contains("hunter2-supersecret"),
            "Debug leaked raw password: {dbg}"
        );
        assert!(dbg.contains("***"), "Debug missing redaction marker: {dbg}");
    }
}
