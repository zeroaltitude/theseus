//! The spool's sweep (theseus-2ij): raw job output that no result will absorb.
//!
//! Since H3 (theseus-wz2) a job's raw output, `spool/results/<id>.out`, which
//! is what it printed before the scrubber saw it, is deleted once the node
//! that holds its result is written. Three kinds were left behind: the output
//! of a job whose execution was cancelled or ended while it ran, which no turn
//! absorbs; the output whose node was written when the daemon died before the
//! delete; and every file from before H3. The sweep deletes them by what the
//! store says, and never what a turn may still read:
//! - a job that runs keeps its file: its wrapper is alive, or its action has
//!   not settled;
//! - a settled job keeps it while its result may still be absorbed: its call
//!   has no answer but a background placeholder, and its execution goes on,
//!   however long it waits (behind a budget question for days, say);
//! - a settled job's file goes once its result is written (an answer that is
//!   not a placeholder), or once its execution has ended (cancelled, failed,
//!   complete), since no turn will read it then;
//! - a file the store knows nothing about goes once it is a day old. A job's
//!   action is durable before its wrapper makes the file, so such a file is
//!   never a live job's; the day is margin for what that cannot see, a store
//!   restored from a copy older than the spool.
//!
//! It runs after serving, as a tender, never on the start path (§2): once as
//! the daemon starts, then every hour (`Core::sweep_spool_after_serving`).

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime};

use theseus_kernel::{ActionState, Kernel, Spool};
use theseus_protocol::SpoolSweep;

use crate::node::{Body, ResultStatus};
use crate::store::Store;

/// A file the store knows nothing about is swept once it is this old.
pub const UNKNOWN_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

/// How often a daemon sweeps after its first sweep.
pub const EVERY: Duration = Duration::from_secs(60 * 60);

/// What becomes of one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fate {
    /// Removed: its result was written.
    Absorbed,
    /// Removed: its execution has ended, so no turn will absorb it.
    Ended,
    /// Removed: the store knows no such job, and the file is a day old.
    Unknown,
    /// Kept: the job still runs.
    Running,
    /// Kept: its result may still be absorbed.
    Pending,
    /// Kept: the store knows no such job, and the file is younger than a day.
    Young,
    /// Kept: the store could not be read for it.
    Unread,
}

impl Fate {
    pub fn removes(self) -> bool {
        matches!(self, Fate::Absorbed | Fate::Ended | Fate::Unknown)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Fate::Absorbed => "absorbed",
            Fate::Ended => "ended",
            Fate::Unknown => "unknown",
            Fate::Running => "running",
            Fate::Pending => "pending",
            Fate::Young => "young",
            Fate::Unread => "unread",
        }
    }
}

/// What the store says of the job a file is named for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Known {
    /// No action has the file's id.
    Nothing,
    /// Its action, whether it has settled, whether its call has an answer
    /// that is not a background placeholder, and whether its execution has
    /// ended.
    Job {
        settled: bool,
        answered: bool,
        execution_ended: bool,
    },
}

/// The rule: what becomes of a file, from whether its wrapper is alive, what
/// the store knows (`None`: it could not be read), and the file's age.
pub fn fate(wrapper_alive: bool, known: Option<Known>, age: Duration) -> Fate {
    match known {
        _ if wrapper_alive => Fate::Running,
        None => Fate::Unread,
        Some(Known::Nothing) if age >= UNKNOWN_AFTER => Fate::Unknown,
        Some(Known::Nothing) => Fate::Young,
        Some(Known::Job { settled: false, .. }) => Fate::Running,
        Some(Known::Job { answered: true, .. }) => Fate::Absorbed,
        Some(Known::Job {
            execution_ended: true,
            ..
        }) => Fate::Ended,
        Some(Known::Job { .. }) => Fate::Pending,
    }
}

/// Sweep `spool`'s raw job output, by what `kernel` and `store` say at `now`:
/// remove what no result will absorb, and count what goes and what stays,
/// with their bytes. Each session's transcript is read at most once.
pub fn sweep(kernel: &Kernel, store: &Store, spool: &Spool, now: SystemTime) -> SpoolSweep {
    let t0 = Instant::now();
    let mut out = SpoolSweep {
        at_unix_ms: now
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64),
        ..Default::default()
    };
    let mut answered = HashMap::new();
    let Ok(dir) = std::fs::read_dir(spool.dir().join("results")) else {
        return out;
    };
    for e in dir.flatten() {
        let name = e.file_name();
        let Some(id) = name.to_str().and_then(|n| n.strip_suffix(".out")) else {
            continue;
        };
        // Not followed: a link or a directory here is no job's output.
        let Ok(meta) = e.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let age = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .unwrap_or_default();
        // A wrapper that lingers (its command exited, and a process it
        // started holds the output open) is alive, though its pid file is
        // gone: it still writes the file (theseus-5wgd).
        let alive = spool.wrapper_lives(id);
        let known = if alive {
            None
        } else {
            known(kernel, store, id, &mut answered)
        };
        let fate = fate(alive, known, age);
        let gone = fate.removes()
            && match spool.remove_result(&e.path()) {
                Ok(_) => true,
                Err(err) => {
                    tracing::warn!(id, error = %format!("{err:#}"), "the sweep could not remove a job's raw output");
                    *out.kept_by.entry("failed".into()).or_default() += 1;
                    false
                }
            };
        if gone {
            out.removed += 1;
            out.removed_bytes += meta.len();
            *out.removed_by.entry(fate.as_str().into()).or_default() += 1;
        } else {
            out.kept += 1;
            out.kept_bytes += meta.len();
            if !fate.removes() {
                *out.kept_by.entry(fate.as_str().into()).or_default() += 1;
            }
        }
    }
    out.took_ms = t0.elapsed().as_millis() as u64;
    out
}

/// What the store knows of job `id`; `None` when it could not be read.
fn known(
    kernel: &Kernel,
    store: &Store,
    id: &str,
    answered: &mut HashMap<String, Option<HashSet<String>>>,
) -> Option<Known> {
    let a = match kernel.action(id) {
        Ok(Some(a)) => a,
        Ok(None) => return Some(Known::Nothing),
        Err(_) => return None,
    };
    let settled = matches!(
        a.state,
        ActionState::Succeeded
            | ActionState::Failed
            | ActionState::Cancelled
            | ActionState::OutcomeUnknown
    );
    if !settled {
        return Some(Known::Job {
            settled,
            answered: false,
            execution_ended: false,
        });
    }
    let ids = answered
        .entry(a.session_id.clone())
        .or_insert_with(|| answered_in(store, &a.session_id))
        .as_ref()?;
    if ids.contains(id) {
        return Some(Known::Job {
            settled,
            answered: true,
            execution_ended: false,
        });
    }
    let e = kernel.execution(&a.execution_id).ok()??;
    Some(Known::Job {
        settled,
        answered: false,
        execution_ended: e.state.is_terminal(),
    })
}

/// The jobs a session's transcript has answered: each tool result that names
/// its job and is not a background placeholder. A late result is one.
fn answered_in(store: &Store, session_id: &str) -> Option<HashSet<String>> {
    let nodes = store.session_nodes(session_id).ok()?;
    Some(
        nodes
            .into_iter()
            .filter_map(|(_, n)| match n.body {
                Body::ToolResult {
                    correlation_id: Some(c),
                    status,
                    ..
                } if status != ResultStatus::Background => Some(c),
                _ => None,
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule, case by case (theseus-2ij): a live wrapper or an unsettled
    /// action keeps the file; a written result, or an ended execution, sweeps
    /// it; a settled job whose result may still be absorbed keeps it however
    /// old it is; a file the store does not know goes only once a day old; a
    /// store that cannot be read keeps it.
    #[test]
    fn what_a_sweep_keeps_and_what_it_removes() {
        let day = UNKNOWN_AFTER;
        let old = day * 30;
        let job = |settled, answered, execution_ended| {
            Some(Known::Job {
                settled,
                answered,
                execution_ended,
            })
        };
        let cases = [
            (true, job(true, true, true), old, Fate::Running),
            (false, job(false, false, true), old, Fate::Running),
            (
                false,
                job(true, true, false),
                Duration::ZERO,
                Fate::Absorbed,
            ),
            (false, job(true, false, true), Duration::ZERO, Fate::Ended),
            (false, job(true, false, false), old, Fate::Pending),
            (false, Some(Known::Nothing), day, Fate::Unknown),
            (
                false,
                Some(Known::Nothing),
                day - Duration::from_secs(1),
                Fate::Young,
            ),
            (false, None, old, Fate::Unread),
        ];
        for (alive, known, age, want) in cases {
            assert_eq!(fate(alive, known, age), want, "{alive} {known:?} {age:?}");
        }
        let removes: Vec<&str> = [
            Fate::Absorbed,
            Fate::Ended,
            Fate::Unknown,
            Fate::Running,
            Fate::Pending,
            Fate::Young,
            Fate::Unread,
        ]
        .into_iter()
        .filter(|f| f.removes())
        .map(Fate::as_str)
        .collect();
        assert_eq!(removes, ["absorbed", "ended", "unknown"]);
    }
}
