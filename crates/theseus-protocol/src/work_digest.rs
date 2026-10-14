//! `work.digest` (theseus-qkoi3): "since you last looked", in a few lines. What
//! finished, failed, was answered, was cancelled or woke on the work board
//! since a position (or a time), worded once here so every surface says the
//! same thing: the CLI, the terminal UI, the cockpit and Discord. The daemon
//! builds the lines from the board's notice ring when it reaches back far
//! enough and from the ledger by kind when it does not; the caller says which
//! it got (`from_ledger`). A surface keeps its own position (its seen file, its
//! storage, its last read): the daemon keeps none.
//!
//! Types and words only: no clock. A time of day is written by the caller's
//! `hm`, as `notices`' is.

use serde::{Deserialize, Serialize};

use crate::notices::name_of;
use crate::work::{Question, Rollup, WorkRef};

/// What a line says happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum DigestKind {
    /// A task reported, or a job or a conversation finished.
    Reported,
    Failed,
    /// A question was answered (or expired).
    Answered,
    Cancelled,
    /// A wake fired.
    Woke,
    /// A reminder fired.
    Reminder,
}

impl DigestKind {
    /// The kinds in the order a summary lists them.
    pub const ALL: [DigestKind; 6] = [
        DigestKind::Reported,
        DigestKind::Failed,
        DigestKind::Answered,
        DigestKind::Cancelled,
        DigestKind::Woke,
        DigestKind::Reminder,
    ];
}

/// One line of the digest: one thing that happened, or a burst of them
/// folded (`count` above 1, `work` then the root they happened under).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DigestLine {
    pub at_ms: u64,
    pub kind: DigestKind,
    pub work: WorkRef,
    pub words: String,
    /// How many things the line stands for: 1 unless a burst was folded.
    #[serde(default = "one")]
    pub count: u32,
}

fn one() -> u32 {
    1
}

/// `work.digest`'s params. One of `since` and `since_ms`; neither reads from
/// the start of what the board holds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WorkDigestParams {
    /// A WAL position: the lines after it. A surface keeps the `to` of its
    /// last digest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub since: Option<u64>,
    /// A time, unix milliseconds: the lines at or after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub since_ms: Option<u64>,
    /// Only these conversations or tasks (session ids): a task's own timeline.
    /// A connection that reads no private text must name them, and sees those
    /// of a shared place alone, scrubbed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub roots: Option<Vec<String>>,
}

/// `work.digest`'s answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WorkDigestResult {
    /// The position the digest starts after (the `since` asked, else 0).
    pub from: u64,
    /// The board's position now: what a surface passes next time.
    pub to: u64,
    /// Oldest first, bursts folded.
    pub lines: Vec<DigestLine>,
    /// True when the notice ring did not reach back to `from` and the lines
    /// were read from the ledger.
    pub from_ledger: bool,
    /// What the board holds now, over the roots asked.
    pub now: Rollup,
    /// What waits for an answer now, the longest waiting first.
    pub questions: Vec<Question>,
}

/// A line's words: `📋 Beta summary reported`. `root` is the name of the
/// conversation a folded burst happened under.
pub fn digest_words(kind: DigestKind, work: &WorkRef, count: u32, root: &str) -> String {
    let name = name_of(work);
    if count <= 1 {
        return match kind {
            DigestKind::Reported => format!("📋 {name} reported"),
            DigestKind::Failed => format!("✗ {name} failed"),
            DigestKind::Answered => format!("✓ you answered: {name}"),
            DigestKind::Cancelled => format!("⊘ {name} was cancelled"),
            DigestKind::Woke => format!("⏰ {name} woke"),
            DigestKind::Reminder => format!("⏰ reminder: {name}"),
        };
    }
    match kind {
        DigestKind::Reported => format!("📋 {count} tasks reported to {root}"),
        DigestKind::Failed => format!("✗ {count} tasks failed under {root}"),
        DigestKind::Answered => format!("✓ you answered {count} questions under {root}"),
        DigestKind::Cancelled => format!("⊘ {count} tasks were cancelled under {root}"),
        DigestKind::Woke => format!("⏰ {count} wakes fired under {root}"),
        DigestKind::Reminder => format!("⏰ {count} reminders fired"),
    }
}

/// A line for one thing.
pub fn digest_line(kind: DigestKind, at_ms: u64, work: WorkRef) -> DigestLine {
    let words = digest_words(kind, &work, 1, "");
    DigestLine {
        at_ms,
        kind,
        work,
        words,
        count: 1,
    }
}

/// A burst is this many or more lines of one kind under one root, each within
/// `BURST_GAP_MS` of the one before.
pub const BURST_MIN: usize = 3;
pub const BURST_GAP_MS: u64 = 10 * 60_000;

/// Fold bursts: three or more lines of one kind under one root, each within
/// ten minutes of the last, become one line at the burst's end ("3 tasks
/// reported to harbour survey"). `entries` are oldest first, each a line with
/// the root it happened under and the root's name; the result keeps the order
/// of each line's (or burst's) last member.
pub fn fold(entries: Vec<(DigestLine, WorkRef)>) -> Vec<DigestLine> {
    let mut open: Vec<Vec<(DigestLine, WorkRef)>> = Vec::new();
    let mut out: Vec<(u64, usize, DigestLine)> = Vec::new();
    let mut seq = 0usize;
    let mut close = |group: Vec<(DigestLine, WorkRef)>, out: &mut Vec<(u64, usize, DigestLine)>| {
        if group.len() >= BURST_MIN && group[0].0.kind != DigestKind::Reminder {
            let (last, root) = group.last().cloned().expect("a burst has members");
            let n: u32 = group.iter().map(|(l, _)| l.count).sum();
            let words = digest_words(last.kind, &last.work, n, &name_of(&root));
            out.push((
                last.at_ms,
                seq,
                DigestLine {
                    work: root,
                    words,
                    count: n,
                    ..last
                },
            ));
            seq += 1;
        } else {
            for (l, _) in group {
                out.push((l.at_ms, seq, l));
                seq += 1;
            }
        }
    };
    for e in entries {
        let slot = open.iter().position(|g| {
            let (last, root) = g.last().expect("a group has a member");
            last.kind == e.0.kind
                && root.id == e.1.id
                && e.0.at_ms.saturating_sub(last.at_ms) <= BURST_GAP_MS
        });
        match slot {
            Some(i) => open[i].push(e),
            None => {
                // Close the groups of this kind and root that the gap ended.
                let mut i = 0;
                while i < open.len() {
                    let (last, root) = open[i].last().expect("a group has a member");
                    if last.kind == e.0.kind && root.id == e.1.id {
                        let g = open.remove(i);
                        close(g, &mut out);
                    } else {
                        i += 1;
                    }
                }
                open.push(vec![e]);
            }
        }
    }
    for g in open {
        close(g, &mut out);
    }
    out.sort_by_key(|(at, seq, _)| (*at, *seq));
    out.into_iter().map(|(_, _, l)| l).collect()
}

/// The digest's one line: `◆ 2 reported, 1 answered by you since 18:14`. None
/// when there is nothing to say. `since_ms` is when the surface last looked;
/// `hm` writes a time of day.
pub fn summary(lines: &[DigestLine], since_ms: u64, hm: &dyn Fn(u64) -> String) -> Option<String> {
    let mut parts = Vec::new();
    for kind in DigestKind::ALL {
        let n: u32 = lines
            .iter()
            .filter(|l| l.kind == kind)
            .map(|l| l.count)
            .sum();
        if n == 0 {
            continue;
        }
        parts.push(match kind {
            DigestKind::Reported => format!("{n} reported"),
            DigestKind::Failed => format!("{n} failed"),
            DigestKind::Answered => format!("{n} answered by you"),
            DigestKind::Cancelled => format!("{n} cancelled"),
            DigestKind::Woke => format!("{n} woke"),
            DigestKind::Reminder => {
                format!("{n} reminder{}", if n == 1 { "" } else { "s" })
            }
        });
    }
    if parts.is_empty() {
        return None;
    }
    let since = if since_ms > 0 {
        format!(" since {}", hm(since_ms))
    } else {
        String::new()
    };
    Some(format!("◆ {}{since}", parts.join(", ")))
}

#[cfg(test)]
#[path = "tests_work_digest.rs"]
mod tests;
