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
/// The longest the driver waits for channel bindings at startup.
pub const BINDINGS_WAIT: Duration = Duration::from_secs(20);

pub async fn drive(core: Arc<Core>) {
    use std::collections::HashMap;
    use std::collections::HashSet;
    use std::sync::Mutex;
    let inflight: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    // A continuation that fails before it is admitted leaves the execution
    // queued; back off per execution instead of retrying every tick.
    let backoff: Arc<Mutex<HashMap<String, (std::time::Instant, u32)>>> =
        Arc::new(Mutex::new(HashMap::new()));
    // A turn resumed at startup must be seen by its channel from its first
    // event: wait (bounded) for channel bindings to be watching their sessions.
    let t0 = std::time::Instant::now();
    let ready = core.wait_for_bindings(BINDINGS_WAIT).await;
    let waited_ms = t0.elapsed().as_millis() as u64;
    if !ready {
        tracing::warn!(
            waited_ms,
            "channel bindings still starting; continuations run without them"
        );
    }
    if let Err(e) = core.store.append_ledger(&crate::ledger::LedgerRow::new(
        "driver.started",
        None,
        None,
        serde_json::json!({"waited_for_bindings_ms": waited_ms, "bindings_ready": ready}),
    )) {
        tracing::warn!(error = %e, "ledger append failed");
    }
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
            if backoff
                .lock()
                .unwrap()
                .get(&e.id)
                .is_some_and(|(until, _)| std::time::Instant::now() < *until)
            {
                continue;
            }
            if core.kernel.is_held(&e.id) || !inflight.lock().unwrap().insert(e.id.clone()) {
                continue;
            }
            let (c, inf, id, bo) = (
                core.clone(),
                inflight.clone(),
                e.id.clone(),
                backoff.clone(),
            );
            tokio::spawn(async move {
                let r = c.continue_execution(&id).await;
                if r.is_ok() {
                    bo.lock().unwrap().remove(&id);
                }
                match r {
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
                        let mut g = bo.lock().unwrap();
                        let n = g.get(&id).map(|(_, n)| n + 1).unwrap_or(1);
                        let wait = Duration::from_secs((1u64 << n.min(8)).min(300));
                        g.insert(id.clone(), (std::time::Instant::now() + wait, n));
                        tracing::warn!(execution_id = %id, error = %format!("{err:#}"), attempt = n, retry_in_s = wait.as_secs(), "continuation turn failed")
                    }
                }
                inf.lock().unwrap().remove(&id);
                c.admission.notify_waiters();
            });
        }
    }
}
