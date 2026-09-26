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

/// The continuation driver: takes a turn for every execution that is
/// runnable without human input — a job's result arrived, a confirm was
/// answered, a crash interrupted a turn. Parked on the admission notify and a
/// short timer; one continuation per execution at a time.
pub async fn drive(core: Arc<Core>) {
    use std::collections::HashSet;
    use std::sync::Mutex;
    let inflight: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let mut tick = tokio::time::interval(Duration::from_millis(500));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tracing::info!("continuation driver parked");
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            _ = core.admission.notified() => {}
            _ = core.shutdown.notified() => break,
        }
        let Ok(execs) = core.kernel.open_executions() else {
            continue;
        };
        for e in execs {
            if e.state != theseus_kernel::ExecState::Queued
                || !(e.resume_pending || !e.queued_results.is_empty())
            {
                continue;
            }
            if core.kernel.is_held(&e.id) || !inflight.lock().unwrap().insert(e.id.clone()) {
                continue;
            }
            let (c, inf, id) = (core.clone(), inflight.clone(), e.id.clone());
            tokio::spawn(async move {
                match c.continue_execution(&id).await {
                    Ok(Some(r)) => tracing::info!(
                        execution_id = %id,
                        session_id = %r.session_id,
                        loops = r.loops,
                        stop_reason = %r.stop_reason,
                        tool_calls = r.tool_calls,
                        "continuation turn"
                    ),
                    Ok(None) => {}
                    Err(err) => {
                        tracing::debug!(execution_id = %id, error = %err, "continuation not taken")
                    }
                }
                inf.lock().unwrap().remove(&id);
                c.admission.notify_waiters();
            });
        }
    }
}
