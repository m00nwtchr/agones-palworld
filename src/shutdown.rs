use std::time::Duration;

use tokio::time::timeout;

use crate::agones::AgonesOps;
use crate::error::AppResult;
use crate::palworld::{Client, ShutdownRequest};

pub async fn run(
    client: &Client,
    bridge: &dyn AgonesOps,
    save_timeout: Duration,
    waittime: u32,
    message: &str,
) -> AppResult<()> {
    let save = timeout(save_timeout, client.save()).await;
    match save {
        Ok(Ok(())) => tracing::info!("world saved"),
        Ok(Err(e)) => tracing::warn!(error=%e, "save failed; continuing"),
        Err(_) => tracing::warn!(?save_timeout, "save timed out; continuing"),
    }
    if let Err(e) = client.announce(message).await {
        tracing::warn!(error=%e, "announce failed; continuing");
    }
    if let Err(e) = client
        .shutdown(ShutdownRequest {
            waittime,
            message: message.into(),
        })
        .await
    {
        tracing::warn!(error=%e, "shutdown POST failed; continuing");
    }
    bridge.shutdown().await;
    Ok(())
}
