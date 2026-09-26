//! The harness loop (spec §3.3): parked on `select()` over the heartbeat
//! timer, the spool notify socket, and shutdown. It never calls a model and
//! has no busy loop; between events it costs nothing. "Tending" is the set of
//! open action records in the WAL, not anything held here.

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixListener;

use crate::Core;

/// Where job wrappers poke after spooling a result.
pub fn notify_socket_path(core: &Core) -> std::path::PathBuf {
    core.spool.dir().join("notify.sock")
}

pub async fn run(core: Arc<Core>) {
    let path = notify_socket_path(&core);
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(l) => Some(l),
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "notify socket unavailable; heartbeat only");
            None
        }
    };
    let period = Duration::from_millis(core.kernel.config().heartbeat_ms.max(1000));
    let mut tick = tokio::time::interval(period);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tick.tick().await; // the first tick fires immediately; startup already reconciled
    tracing::info!(heartbeat_secs = period.as_secs(), notify = %path.display(), "harness loop parked");
    loop {
        tokio::select! {
            _ = tick.tick() => {
                let c = core.clone();
                // Store work is blocking; keep it off the reactor.
                let _ = tokio::task::spawn_blocking(move || c.heartbeat("timer")).await;
            }
            accepted = async {
                match &listener {
                    Some(l) => l.accept().await.map(|(s, _)| s),
                    None => std::future::pending().await,
                }
            } => {
                if let Ok(stream) = accepted {
                    let mut lines = BufReader::new(stream).lines();
                    let mut ids = Vec::new();
                    while let Ok(Some(line)) = tokio::time::timeout(Duration::from_millis(500), lines.next_line()).await.unwrap_or(Ok(None)) {
                        ids.push(line);
                    }
                    let c = core.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        tracing::debug!(ids = ?ids, "wrapper notify");
                        c.heartbeat("notify");
                    })
                    .await;
                }
            }
            _ = core.shutdown.notified() => break,
        }
    }
    let _ = std::fs::remove_file(&path);
}
