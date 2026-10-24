//! A running job's output, on demand and live (theseus-e1hu4): `work.peek`
//! (one read: its size and last lines), `action.output` (a followed stream of
//! `action.output.chunk` to the asking connection, then `action.output.end`)
//! and `action.unfollow`. Types and the one wording of a job's time against
//! its usual: no clock, no I/O.
//!
//! A chunk names where its text begins (`offset`, a byte of the job's output
//! file), so a client that was told `events.lost` asks `work.peek` for what
//! the job printed since (its `bytes`) and follows again `from` there. A
//! follower never runs more than `FOLLOW_BEHIND` bytes behind the file: past
//! that it skips ahead, and the next chunk's offset shows the gap.

use serde::{Deserialize, Serialize};

/// The most lines a `work.peek` returns.
pub const PEEK_LINES: usize = 20;
/// The most a follower lags the job's output before it skips to the newest
/// `FOLLOW_BEHIND` bytes.
pub const FOLLOW_BEHIND: u64 = 4 * 1024 * 1024;
/// The longest a line of a peek is, in characters.
pub const PEEK_LINE_CHARS: usize = 1000;
/// The longest `doing.last_line` is, in characters.
pub const LAST_LINE_CHARS: usize = 120;

/// `work.peek`'s params: a job's id (`act_…`, as a work view names it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WorkPeekParams {
    pub id: String,
}

/// What a job has printed so far, as of `at_ms`: the size of its output, and
/// its last lines, scrubbed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WorkPeekResult {
    pub id: String,
    /// The output's length in bytes: where `action.output` follows from.
    pub bytes: u64,
    /// At most `PEEK_LINES`, oldest first, whole lines.
    pub lines: Vec<String>,
    pub at_ms: u64,
    /// The job has ended: `lines` are its last.
    pub complete: bool,
    /// When the job was dispatched, and the usual time of its program on this
    /// machine (the work board's `eta`), so a surface words its time as
    /// `job_words` does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub usual_ms: Option<u64>,
}

/// `job.stop`'s params: a job's id (`act_…`, as `work.peek` takes it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JobStopParams {
    pub id: String,
}

/// What `job.stop` did, as the model's `job_stop` tool tells it: whether the job has ended, whether its end was
/// verified (nothing of it is left), and the words for a toast. A job that had ended already answers `ended: true`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct JobStopResult {
    pub id: String,
    pub ended: bool,
    pub verified: bool,
    pub words: String,
}

/// `action.output`'s params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ActionOutputParams {
    pub id: String,
    /// The byte to follow from: absent, the output's end now (a peek's
    /// `bytes`); 0 reads what the job has printed, up to `FOLLOW_BEHIND`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub from: Option<u64>,
}

/// `action.output`'s answer: the follow has begun. Chunks and the end follow
/// as notifications to this connection alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ActionOutputResult {
    pub id: String,
    /// Where the first chunk begins.
    pub offset: u64,
    /// The output's length now.
    pub bytes: u64,
    /// The job had ended already: only what is left of its output follows,
    /// then the end.
    pub complete: bool,
}

/// `action.output.chunk`: text of the job's output from byte `offset`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ActionOutputChunk {
    pub id: String,
    pub offset: u64,
    pub text: String,
}

/// `action.output.end`: the job's end, after its last chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ActionOutputEnd {
    pub id: String,
    /// `succeeded`, `failed`, `cancelled` or `outcome_unknown`.
    pub outcome: String,
}

/// `action.unfollow`'s params and answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ActionUnfollowParams {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ActionUnfollowResult {
    /// This connection was following it.
    pub was_following: bool,
}

/// `2:14`, or `1:02:03` past an hour.
pub fn clock(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// How a running job's time reads on every surface: `job 0:05 of usual
/// 0:31`, or `job 0:05` when no usual time is known.
pub fn job_words(elapsed_ms: u64, usual_ms: Option<u64>) -> String {
    match usual_ms {
        Some(u) => format!("job {} of usual {}", clock(elapsed_ms), clock(u)),
        None => format!("job {}", clock(elapsed_ms)),
    }
}

/// A line cut to `max` characters, with an ellipsis where it was cut.
pub fn cut_line(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_jobs_time_reads_against_its_usual() {
        assert_eq!(job_words(5_400, Some(31_000)), "job 0:05 of usual 0:31");
        assert_eq!(job_words(65_000, None), "job 1:05");
        assert_eq!(
            job_words(3_725_000, Some(3_600_000)),
            "job 1:02:05 of usual 1:00:00"
        );
    }

    #[test]
    fn a_cut_line_says_it_was_cut_and_stays_within_its_bound() {
        assert_eq!(cut_line("short", 10), "short");
        let c = cut_line(&"é".repeat(300), LAST_LINE_CHARS);
        assert_eq!(c.chars().count(), LAST_LINE_CHARS);
        assert!(c.ends_with('…'));
    }
}
