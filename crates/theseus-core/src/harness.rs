//! The harness loop (spec §3.3): parked on `select()` over the heartbeat
//! timer, the spool notify socket, and shutdown. It never calls a model and
//! has no busy loop; between events it costs nothing. "Tending" is the set of
//! open action records in the WAL, not anything held here.

use std::sync::Arc;
use std::time::Duration;
use theseus_protocol::LedgerKind;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixListener;

use crate::Core;

/// Where job wrappers poke after spooling a result.
pub fn notify_socket_path(core: &Core) -> std::path::PathBuf {
    core.spool.dir().join("notify.sock")
}

/// Bind the notify socket, take what the spool holds once, then park: a
/// heartbeat every `heartbeat_ms`, and one for each wrapper's notify.
#[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
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
    // The first tick fires immediately: the bind's heartbeat below is it. The
    // startup's spool drain ran before serving, and nothing listened here
    // until now: a job's result spooled between the two had its one notify
    // refused. Taken here, it waits for no heartbeat (theseus-74lt).
    tick.tick().await;
    let c = core.clone();
    let _ = tokio::task::spawn_blocking(move || c.heartbeat("bind")).await;
    tracing::info!(heartbeat_secs = period.as_secs(), notify = %path.display(), "harness loop parked");
    loop {
        tokio::select! {
            _ = tick.tick() => {
                let c = core.clone();
                // Store work is blocking; keep it off the reactor.
                let _ = tokio::task::spawn_blocking(move || c.heartbeat("timer")).await;
                // The disk's floor holds for a job that already runs, as it
                // does for the next one (theseus-ht82).
                core.stop_jobs_below_floor().await;
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
#[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
pub async fn drive(core: Arc<Core>) {
    use std::collections::HashMap;
    use std::collections::HashSet;
    use std::sync::Mutex;
    let inflight: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    // A continuation that fails before it is admitted leaves the execution
    // queued; back off per execution instead of retrying every tick.
    let backoff: Arc<Mutex<HashMap<String, (std::time::Instant, u32)>>> =
        Arc::new(Mutex::new(HashMap::new()));
    // Nothing waits for a channel binding (theseus-q4v): what a continuation
    // says reaches its channel through the outbox, whenever the binding
    // delivers it. The driver's start is a phase of its own, which the
    // lifecycle bench reads.
    let t0 = std::time::Instant::now();
    core.startup_log
        .record("driver", true, t0, serde_json::Value::Null);
    // Dropped once the stop's last checkpoint is written: a signal just after
    // serving can stop the daemon before the driver starts (theseus-6mxq).
    core.ledger_unless_closed(&crate::ledger::LedgerRow::new(
        LedgerKind::DriverStarted,
        None,
        None,
        serde_json::json!({"after_start_ms": core.startup_log.us(t0) / 1000}),
    ));
    let mut tick = tokio::time::interval(Duration::from_millis(500));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tracing::info!("continuation driver parked");
    let mut unlisted = Unlisted::default();
    // An earlier process's provider calls, unknown at the first tick (theseus-m9iy).
    core.mark_earlier_calls();
    // The stop's wake reaches only a parked driver; a busy one reads its mark (theseus-jtrc).
    while !core.outbox.stopping() {
        tokio::select! {
            _ = tick.tick() => {}
            _ = core.admission.notified() => {}
            _ = core.shutdown.notified() => break,
        }
        // A call's question nobody answered expires within a tick of the time
        // its card gives (theseus-830), whatever executions the tick reads,
        // which leave out a parked one; the read below takes up its wake.
        core.expire_questions_if_due();
        // A task claim whose lease lapsed is freed, from the claims kept in
        // memory (39b).
        core.free_expired_leases_if_due();
        // The queued executions and those a due time may wake, by their
        // terms: a tick reads none of the parked ones (theseus-lv2).
        let Some(execs) = unlisted.runnable(&core) else {
            continue;
        };
        let now = core.kernel.now_ms();
        for e in execs {
            // A due time that has come, or a wake of its own that is due
            // while it is free (DD8): queue it here, so a wake runs within a
            // tick of its time, not a heartbeat. The heartbeat's reconciler
            // catches any this misses. It writes only when one is due.
            let e = if theseus_kernel::wakes::due_now(&e, now) {
                match core.kernel.fire_due(&e.id) {
                    Ok(Some(queued)) => queued,
                    Ok(None) => continue,
                    Err(err) => {
                        tracing::warn!(execution_id = %e.id, error = %format!("{err:#}"), "a due wake could not be queued");
                        continue;
                    }
                }
            } else {
                e
            };
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

/// The driver's failure to list the executions it may run (R4): each
/// distinct failure is news once, and so is the tick that lists them again,
/// so the log says when the driver stopped and started without a line a tick.
#[derive(Default)]
struct Unlisted(Option<String>);

impl Unlisted {
    /// The executions a tick may run, or `None` when they cannot be read:
    /// the tick then runs nothing, and says so once for each failure. It
    /// once skipped them without a word.
    fn runnable(&mut self, core: &Core) -> Option<Vec<theseus_kernel::Execution>> {
        match core.kernel.maybe_runnable() {
            Ok(execs) => {
                if let Some(was) = self.listed() {
                    tracing::info!(was = %was, "continuation driver: the runnable executions read again");
                }
                Some(execs)
            }
            Err(e) => {
                if self.failed(&e) {
                    tracing::error!(error = %format!("{e:#}"),
                        "continuation driver: the runnable executions could not be read; no continuation or wake runs until they can");
                }
                None
            }
        }
    }

    /// A tick failed with `e`: true when it is news, to be logged.
    fn failed(&mut self, e: &anyhow::Error) -> bool {
        let e = format!("{e:#}");
        let news = self.0.as_deref() != Some(e.as_str());
        self.0 = Some(e);
        news
    }

    /// A tick listed them: the failure that had stopped the driver, if one
    /// had.
    fn listed(&mut self) -> Option<String> {
        self.0.take()
    }
}

#[cfg(test)]
mod tests {
    use super::Unlisted;

    /// A failure is logged once however many ticks it lasts, a different one
    /// is logged too, and the tick that lists again says what had stopped it.
    #[test]
    fn the_driver_says_each_failure_once_and_when_it_reads_again() {
        let mut u = Unlisted::default();
        assert_eq!(u.listed(), None, "nothing had failed");
        let refused = anyhow::anyhow!("corrupt frame in segment 1 at offset 0");
        assert!(u.failed(&refused), "the first tick that fails is news");
        assert!(
            !u.failed(&refused),
            "the same failure, a tick later, is not"
        );
        assert!(u.failed(&anyhow::anyhow!("io: no space left on device")));
        assert_eq!(
            u.listed().as_deref(),
            Some("io: no space left on device"),
            "the tick that lists again names what had stopped it"
        );
        assert_eq!(u.listed(), None);
        assert!(
            u.failed(&refused),
            "a failure after a recovery is news again"
        );
    }
}
