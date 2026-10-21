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

/// What a CLI answer from an agent's process is ledgered `by`
/// (theseus-5rvss): `agent:claude`, `agent:no terminal`. The owner's own is
/// the surface's name, a job's a `job:` label; a row from before this has
/// neither and reads as the owner's.
pub fn agent_by(name: &str) -> String {
    format!("agent:{name}")
}

/// The agent a `by` names, or none for the owner's own, a job's or an old
/// row's.
pub fn agent_of(by: &str) -> Option<&str> {
    by.strip_prefix("agent:")
}

/// A `by` in a sentence: an agent as `an agent (claude)`, anyone else as written.
pub fn by_words(by: &str) -> String {
    agent_of(by).map_or_else(|| by.to_string(), |a| format!("an agent ({a})"))
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
    /// For a task that reported: the first line of its report, which `words` carries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub report: Option<String>,
    /// For a question an agent answered: the agent, as `agent_by` names it
    /// less its prefix (`claude`, `no terminal`). None is the owner's own, and
    /// an old row's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub by: Option<String>,
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
    /// Set when the ledger read stopped at its row limit: the lines are complete up to this time
    /// and not after it (`digest_more` words it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub more: Option<DigestMore>,
    /// What the owner's Google account did in the window: mail sent, drafts
    /// waiting, replies waited on, documents written. None without `[google]`
    /// or when every count is 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub google: Option<crate::digest_google::DigestGoogle>,
}

/// Where a digest read from the ledger stopped (`WorkDigestResult.more`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DigestMore {
    /// The time the lines reach, unix ms: the rows after it were not read.
    pub since_ms: u64,
    /// The command that lists what ended since then, with the age that command takes.
    pub command: String,
}

/// The note under a digest the ledger read cut short, or none.
pub fn digest_more(res: &WorkDigestResult, hm: &dyn Fn(u64) -> String) -> Option<String> {
    let m = res.more.as_ref()?;
    Some(format!(
        "… and more since {}; {}",
        hm(m.since_ms),
        m.command
    ))
}

/// A line's words: `📋 Beta summary reported: the first line of its report`. `root` is the name of the
/// conversation a folded burst happened under. A task that reported is never folded without its title
/// (`fold_digest`), so the count words below are for the other kinds.
pub fn digest_words(
    kind: DigestKind,
    work: &WorkRef,
    count: u32,
    root: &str,
    report: Option<&str>,
    by: Option<&str>,
) -> String {
    let name = name_of(work);
    if count <= 1 {
        return match kind {
            DigestKind::Reported => match report.map(str::trim).filter(|r| !r.is_empty()) {
                Some(r) => format!("📋 {name} reported: {r}"),
                None => format!("📋 {name} reported"),
            },
            DigestKind::Failed => format!("✗ {name} failed"),
            DigestKind::Answered => match by {
                Some(agent) => format!("✓ answered by an agent ({agent}): {name}"),
                None => format!("✓ you answered: {name}"),
            },
            DigestKind::Cancelled => format!("⊘ {name} was cancelled"),
            DigestKind::Woke => format!("⏰ {name} woke"),
            DigestKind::Reminder => format!("⏰ reminder: {name}"),
        };
    }
    match kind {
        DigestKind::Reported => format!("📋 {count} reported to {root}"),
        DigestKind::Failed => format!("✗ {count} tasks failed under {root}"),
        DigestKind::Answered => match by {
            Some(agent) => format!("✓ an agent ({agent}) answered {count} questions under {root}"),
            None => format!("✓ you answered {count} questions under {root}"),
        },
        DigestKind::Cancelled => format!("⊘ {count} tasks were cancelled under {root}"),
        DigestKind::Woke => format!("⏰ {count} wakes fired under {root}"),
        DigestKind::Reminder => format!("⏰ {count} reminders fired"),
    }
}

/// A line for one thing.
pub fn digest_line(kind: DigestKind, at_ms: u64, work: WorkRef) -> DigestLine {
    digest_line_with(kind, at_ms, work, None, None)
}

/// A line for one thing, with the first line of its report when it is a task that reported, and
/// the agent that answered it when an agent did.
pub fn digest_line_with(
    kind: DigestKind,
    at_ms: u64,
    work: WorkRef,
    report: Option<String>,
    by: Option<String>,
) -> DigestLine {
    let report = report.filter(|r| !r.trim().is_empty());
    let words = digest_words(kind, &work, 1, "", report.as_deref(), by.as_deref());
    DigestLine {
        at_ms,
        kind,
        work,
        words,
        count: 1,
        report,
        by,
    }
}

/// Reported tasks a burst names one by one before it names the rest together.
pub const NAMED_MAX: usize = 5;
/// The titles the line for the rest lists before it ends `, …`.
const REST_NAMES: usize = 12;

/// The line for the reported tasks past `NAMED_MAX` in a burst: `📋 and 4 more: A, B, C, D`.
fn rest_line(rest: &[DigestLine], root: WorkRef) -> DigestLine {
    let last = rest.last().expect("a rest has members");
    let names: Vec<String> = rest
        .iter()
        .take(REST_NAMES)
        .map(|l| name_of(&l.work))
        .collect();
    let more = if rest.len() > REST_NAMES { ", …" } else { "" };
    DigestLine {
        at_ms: last.at_ms,
        kind: DigestKind::Reported,
        work: root,
        words: format!("📋 and {} more: {}{more}", rest.len(), names.join(", ")),
        count: rest.iter().map(|l| l.count).sum(),
        report: None,
        by: None,
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
pub fn fold_digest(entries: Vec<(DigestLine, WorkRef)>) -> Vec<DigestLine> {
    let mut open: Vec<Vec<(DigestLine, WorkRef)>> = Vec::new();
    let mut out: Vec<(u64, usize, DigestLine)> = Vec::new();
    let mut seq = 0usize;
    let mut close = |group: Vec<(DigestLine, WorkRef)>, out: &mut Vec<(u64, usize, DigestLine)>| {
        if group.len() >= BURST_MIN && group[0].0.kind == DigestKind::Reported {
            // Never folded without titles: the first few each told, the rest named together.
            let root = group
                .last()
                .map(|(_, r)| r.clone())
                .expect("a burst has members");
            let lines: Vec<DigestLine> = group.into_iter().map(|(l, _)| l).collect();
            let (named, rest) = lines.split_at(lines.len().min(NAMED_MAX));
            for l in named {
                out.push((l.at_ms, seq, l.clone()));
                seq += 1;
            }
            if !rest.is_empty() {
                let l = rest_line(rest, root);
                out.push((l.at_ms, seq, l));
                seq += 1;
            }
        } else if group.len() >= BURST_MIN && group[0].0.kind != DigestKind::Reminder {
            let (last, root) = group.last().cloned().expect("a burst has members");
            let n: u32 = group.iter().map(|(l, _)| l.count).sum();
            let words = digest_words(
                last.kind,
                &last.work,
                n,
                &name_of(&root),
                None,
                last.by.as_deref(),
            );
            out.push((
                last.at_ms,
                seq,
                DigestLine {
                    work: root,
                    words,
                    count: n,
                    report: None,
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
                && last.by == e.0.by
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
                    if last.kind == e.0.kind && last.by == e.0.by && root.id == e.1.id {
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
pub fn digest_summary(
    lines: &[DigestLine],
    since_ms: u64,
    hm: &dyn Fn(u64) -> String,
) -> Option<String> {
    let mut parts = Vec::new();
    let count = |kind: DigestKind, agent: bool| -> u32 {
        lines
            .iter()
            .filter(|l| l.kind == kind && l.by.is_some() == agent)
            .map(|l| l.count)
            .sum()
    };
    for kind in DigestKind::ALL {
        let n = count(kind, false);
        if n > 0 {
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
        // An agent's answers are told apart from the owner's (theseus-5rvss).
        let agents = count(kind, true);
        if kind == DigestKind::Answered && agents > 0 {
            parts.push(format!("{agents} answered by an agent"));
        }
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

/// A digest as one message: the summary, each line with its time, and what
/// waits, within `limit` characters (the oldest lines that do not fit are
/// counted, not shown). `since_ms` is when the surface last looked; `hm`
/// writes a time of day. The words of Discord's `/digest` and of its posted
/// digest (theseus-ss36w).
pub fn digest_text(
    res: &WorkDigestResult,
    since_ms: u64,
    hm: &dyn Fn(u64) -> String,
    limit: usize,
) -> String {
    let header = match digest_summary(&res.lines, since_ms, hm) {
        Some(s) => s,
        None if since_ms > 0 => format!("◆ nothing since {}", hm(since_ms)),
        None => "◆ nothing".to_string(),
    };
    let tail: Vec<String> = res
        .questions
        .iter()
        .map(|q| format!("● waiting: {}?", q.prompt.trim_end_matches('?')))
        .collect();
    let lines: Vec<String> = res
        .lines
        .iter()
        .map(|l| format!("• {} {}", hm(l.at_ms), l.words))
        .collect();
    let fixed = header.len() + tail.iter().map(|t| t.len() + 1).sum::<usize>() + 40;
    let mut used = fixed;
    let mut from = lines.len();
    for (k, l) in lines.iter().enumerate().rev() {
        if used + l.len() + 1 > limit {
            break;
        }
        used += l.len() + 1;
        from = k;
    }
    let mut out = vec![header];
    out.extend(res.google.and_then(|g| g.line()));
    if from > 0 {
        out.push(format!("… and {from} earlier"));
    }
    out.extend(lines[from..].iter().cloned());
    out.extend(digest_more(res, hm));
    out.extend(tail);
    out.join("\n")
}

#[cfg(test)]
#[path = "tests_work_digest.rs"]
mod tests;
