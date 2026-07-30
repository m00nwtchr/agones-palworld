#![allow(clippy::result_large_err)]

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderValue};
use reqwest::{Client as Http, StatusCode};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{AppError, AppResult};
use crate::state::Player;

#[derive(Debug, Clone, Deserialize)]
pub struct ServerInfo {
    pub version: String,
    #[serde(default)]
    pub servername: String,
    pub worldguid: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerMetrics {
    pub serverfps: i64,
    pub currentplayernum: u32,
    pub serverframetime: f64,
    pub maxplayernum: u32,
    pub uptime: u64,
    pub basecampnum: u32,
    pub days: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShutdownRequest {
    pub waittime: u32,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
struct PlayersResponse {
    players: Vec<Player>,
}

#[derive(Clone)]
pub struct Client {
    http: Http,
    base_url: Url,
    auth_header: HeaderValue,
    timeout_secs: u64,
}

impl Client {
    pub fn new(base_url: Url, password: &str) -> Self {
        Self::with_timeout(base_url, password, 5)
    }

    pub fn with_timeout(base_url: Url, password: &str, timeout_secs: u64) -> Self {
        let auth = B64.encode(format!("admin:{password}"));
        let auth_header = HeaderValue::from_str(&format!("Basic {auth}")).expect("ascii");
        let http = Http::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .connect_timeout(std::time::Duration::from_secs(2))
            .build()
            .expect("reqwest builder");
        Self {
            http,
            base_url,
            auth_header,
            timeout_secs,
        }
    }

    fn url(&self, path: &str) -> AppResult<Url> {
        self.base_url
            .join(path.trim_start_matches('/'))
            .map_err(Into::into)
    }

    async fn handle(&self, resp: reqwest::Response) -> AppResult<reqwest::Response> {
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        let body = resp.text().await.unwrap_or_default();
        if status == StatusCode::REQUEST_TIMEOUT {
            Err(AppError::PalworldTimeout(self.timeout_secs))
        } else {
            Err(AppError::PalworldHttp(status, body))
        }
    }

    pub async fn info(&self) -> AppResult<ServerInfo> {
        let url = self.url("/info")?;
        let resp = self
            .http
            .get(url)
            .header(AUTHORIZATION, &self.auth_header)
            .send()
            .await?;
        let resp = self.handle(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn players(&self) -> AppResult<Vec<Player>> {
        let url = self.url("/players")?;
        let resp = self
            .http
            .get(url)
            .header(AUTHORIZATION, &self.auth_header)
            .send()
            .await?;
        let resp = self.handle(resp).await?;
        let parsed: PlayersResponse = resp.json().await?;
        Ok(parsed.players)
    }

    pub async fn metrics(&self) -> AppResult<ServerMetrics> {
        let url = self.url("/metrics")?;
        let resp = self
            .http
            .get(url)
            .header(AUTHORIZATION, &self.auth_header)
            .send()
            .await?;
        let resp = self.handle(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn save(&self) -> AppResult<()> {
        let url = self.url("/save")?;
        let resp = self
            .http
            .post(url)
            .header(AUTHORIZATION, &self.auth_header)
            .send()
            .await?;
        let _ = self.handle(resp).await?;
        Ok(())
    }

    pub async fn announce(&self, message: &str) -> AppResult<()> {
        let url = self.url("/announce")?;
        let resp = self
            .http
            .post(url)
            .header(AUTHORIZATION, &self.auth_header)
            .header(CONTENT_TYPE, "application/json")
            .json(&serde_json::json!({"message": message}))
            .send()
            .await?;
        let _ = self.handle(resp).await?;
        Ok(())
    }

    pub async fn settings(&self) -> AppResult<serde_json::Value> {
        let url = self.url("/settings")?;
        let resp = self
            .http
            .get(url)
            .header(AUTHORIZATION, &self.auth_header)
            .send()
            .await?;
        let resp = self.handle(resp).await?;
        Ok(resp.json().await?)
    }

    pub async fn stop(&self) -> AppResult<()> {
        let url = self.url("/stop")?;
        let resp = self
            .http
            .post(url)
            .header(AUTHORIZATION, &self.auth_header)
            .send()
            .await?;
        let _ = self.handle(resp).await?;
        Ok(())
    }

    pub async fn kick(&self, user_id: &str, message: &str) -> AppResult<()> {
        let url = self.url("/kick")?;
        let resp = self
            .http
            .post(url)
            .header(AUTHORIZATION, &self.auth_header)
            .header(CONTENT_TYPE, "application/json")
            .json(&serde_json::json!({"userId": user_id, "message": message}))
            .send()
            .await?;
        let _ = self.handle(resp).await?;
        Ok(())
    }

    pub async fn ban(&self, user_id: &str) -> AppResult<()> {
        let url = self.url("/ban")?;
        let resp = self
            .http
            .post(url)
            .header(AUTHORIZATION, &self.auth_header)
            .header(CONTENT_TYPE, "application/json")
            .json(&serde_json::json!({"userId": user_id}))
            .send()
            .await?;
        let _ = self.handle(resp).await?;
        Ok(())
    }

    pub async fn unban(&self, user_id: &str) -> AppResult<()> {
        let url = self.url("/unban")?;
        let resp = self
            .http
            .post(url)
            .header(AUTHORIZATION, &self.auth_header)
            .header(CONTENT_TYPE, "application/json")
            .json(&serde_json::json!({"userId": user_id}))
            .send()
            .await?;
        let _ = self.handle(resp).await?;
        Ok(())
    }

    pub async fn shutdown(&self, req: ShutdownRequest) -> AppResult<()> {
        let url = self.url("/shutdown")?;
        let resp = self
            .http
            .post(url)
            .header(AUTHORIZATION, &self.auth_header)
            .header(CONTENT_TYPE, "application/json")
            .json(&req)
            .send()
            .await?;
        let _ = self.handle(resp).await?;
        Ok(())
    }
}
