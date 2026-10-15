//! The head-to-head's counted rows (theseus-7gir.13), judged in the gate's
//! turn bench beside the frames: counts, not times, since the gate runs debug
//! builds beside neighbour load (the owner's decision D-6). The times beside
//! them are recorded and never judged.
//!
//! - **Syncs before the first byte**: the frames the WAL gains from a turn's
//!   submit until the stand-in sends its answer's first byte (counted by the
//!   stand-in itself, from the WAL, as it is about to send), and of them,
//!   those before the request left. Each one is an `fdatasync` a person waits
//!   for before anything comes back. Budget: today's count; a step that
//!   writes fewer passes, and lowers it (turn-one-frame, tool-loop-frames).
//! - **The first request's tokens, as sent** (theseus-to0xw): what a fresh
//!   session's first request is for each kind of place on a daemon of the
//!   default config (beside the bench's), counted as the provider bills it
//!   (the deferred tools at their constant, the search tool at its size:
//!   `context.census`'s total, the number `theseus census` prints), at most
//!   `theseus_core::first_request::budget`: 8,000 in the cli and task
//!   places, 15,000 in the DM's, and the channel's and the headless run's own
//!   (today's and a tenth). Its bytes are recorded beside it, and the bench's
//!   own config (the template's every section: AWS, GitHub, MCP), not judged.
//! - **No timer between a delta and its write**: a stream of
//!   [`Pace::default`]'s chunks reaches the client as as many `model.delta`
//!   notifications (a timer that gathers deltas sends fewer), and in lockstep,
//!   the stand-in holding each next chunk until the client has the last (at
//!   most [`HOLD`]), every chunk reaches the client alone (a delta held for
//!   the next one, or for the stream's end, never does).

use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};

use super::{Driver, Summary};
use crate::fake_model::{mono_ns, Entry, Pace, Watch, STREAM_MARK};
use crate::lifecycle::Verdict;
use crate::walcount::Tail;

/// Syncs from a warm turn's submit to its first byte, as the stand-in counts
/// them: 3 when measured on 2026-10-10 (the turn's admission, its plan and
/// its call's plan), 2 since theseus-2uby put the admission in its input's
/// frame, so it is the plain turn's own budget (`PLAIN_SYNCS_BEFORE_CALL`,
/// read from the turn's trace): the two counts of one thing agree. It only
/// goes down.
pub const SYNCS_BEFORE_FIRST_BYTE: f64 = super::PLAIN_SYNCS_BEFORE_CALL;

/// The kinds of place a fresh session's first request is counted for: the
/// ones `theseus_core::first_request::budget` holds to a budget.
pub const PLACES: [&str; 5] = ["cli", "task", "dm", "channel", "headless"];

/// How long the lockstep stand-in holds a chunk for the client to see the
/// last: far past any delta's way to the client, so only a delta held for
/// something else waits it out.
pub const HOLD: Duration = Duration::from_secs(5);

/// Warm turns counted, after the fresh session's first.
const RUNS: usize = 3;

/// What the stand-in counts as it answers: armed for a counted turn, it reads
/// the WAL's new frames as the request arrives and again just before its
/// first byte; in lockstep it holds each next chunk until the client has seen
/// the last.
pub struct Counter {
    wal: PathBuf,
    state: Mutex<Armed>,
    seen: Mutex<Seen>,
    saw: Condvar,
}

#[derive(Default)]
struct Armed {
    tail: Option<Tail>,
    lockstep: bool,
    before_request: Option<usize>,
    before_first_byte: Option<usize>,
    /// Chunks the client had seen alone, each before the next was sent.
    alone: usize,
    /// When each chunk was written, `CLOCK_MONOTONIC` ns.
    written: Vec<u64>,
}

#[derive(Default)]
struct Seen {
    /// When each delta reached the client, `CLOCK_MONOTONIC` ns.
    at: Vec<u64>,
}

impl Counter {
    pub fn new(wal: PathBuf) -> Self {
        Self {
            wal,
            state: Mutex::default(),
            seen: Mutex::default(),
            saw: Condvar::new(),
        }
    }

    /// Count the next stream's syncs from now on.
    fn arm(&self, lockstep: bool) -> Result<()> {
        let tail = Tail::at_end(&self.wal)?;
        *self.state.lock().expect("counter") = Armed {
            tail: Some(tail),
            lockstep,
            ..Armed::default()
        };
        *self.seen.lock().expect("counter") = Seen::default();
        Ok(())
    }

    fn take(&self) -> (Armed, Seen) {
        let a = std::mem::take(&mut *self.state.lock().expect("counter"));
        let s = std::mem::take(&mut *self.seen.lock().expect("counter"));
        (a, s)
    }

    /// The client saw a delta.
    fn seen(&self) {
        self.seen.lock().expect("counter").at.push(mono_ns());
        self.saw.notify_all();
    }

    fn counting(e: &Entry) -> bool {
        e.opening.contains(STREAM_MARK)
    }

    fn frames(a: &mut Armed) -> usize {
        a.tail
            .as_mut()
            .and_then(|t| t.read().ok())
            .map_or(0, |f| f.len())
    }
}

impl Watch for Counter {
    fn arrived(&self, e: &Entry) {
        let mut a = self.state.lock().expect("counter");
        if Self::counting(e) && a.tail.is_some() && a.before_request.is_none() {
            a.before_request = Some(Self::frames(&mut a));
        }
    }

    fn first_byte(&self, e: &Entry) {
        let mut a = self.state.lock().expect("counter");
        if Self::counting(e) && a.tail.is_some() && a.before_first_byte.is_none() {
            let more = Self::frames(&mut a);
            a.before_first_byte = Some(a.before_request.unwrap_or(0) + more);
        }
    }

    fn after_chunk(&self, e: &Entry, k: usize) {
        if !Self::counting(e) {
            return;
        }
        let lockstep = {
            let mut a = self.state.lock().expect("counter");
            a.written.push(mono_ns());
            a.lockstep
        };
        if !lockstep {
            return;
        }
        let seen = self.seen.lock().expect("counter");
        let (seen, _) = self
            .saw
            .wait_timeout_while(seen, HOLD, |s| s.at.len() < k)
            .expect("counter");
        if seen.at.len() >= k {
            self.state.lock().expect("counter").alone += 1;
        }
    }
}

/// The counted rows, measured.
#[derive(Debug, Default, Serialize)]
pub struct Counted {
    pub chunks: usize,
    /// Each warm turn's syncs before its request left, and before its first
    /// byte.
    pub syncs_before_request: Vec<usize>,
    pub syncs_before_first_byte: Vec<usize>,
    /// A fresh session's first request on a daemon of the default config,
    /// in tokens as sent for each kind of place (`PLACES`): judged against
    /// `theseus_core::first_request::budget`.
    pub first_request_tokens: std::collections::BTreeMap<String, u64>,
    /// The same request's bytes with the default tools, and with the
    /// bench's (the template's every section: AWS, GitHub, MCP): recorded,
    /// never judged (a deferred tool's definition is bytes the provider
    /// does not bill as tokens).
    pub first_request_bytes: u64,
    pub first_request_bytes_bench: u64,
    /// The free stream: the deltas the client got, of `chunks`.
    pub deltas_seen: usize,
    /// The lockstep stream: the chunks the client saw alone, of `chunks`.
    pub deltas_alone: usize,
    /// Recorded, never judged: a delta's way from its write to the client,
    /// in ms (the free stream's).
    pub delta_ms: Option<Summary>,
}

impl Counted {
    pub fn verdicts(&self) -> Vec<Verdict> {
        let worst = self
            .syncs_before_first_byte
            .iter()
            .max()
            .copied()
            .unwrap_or(0);
        let rows = [
            ("syncs_first_byte", worst as f64, SYNCS_BEFORE_FIRST_BYTE),
            // A count of what went missing: 0 is the only pass.
            (
                "deltas_gathered",
                (self.chunks - self.deltas_seen.min(self.chunks)) as f64,
                0.0,
            ),
            (
                "deltas_held",
                (self.chunks - self.deltas_alone.min(self.chunks)) as f64,
                0.0,
            ),
        ];
        let tokens = self.first_request_tokens.iter().filter_map(|(place, n)| {
            let budget = theseus_core::first_request::budget(place)? as f64;
            Some((format!("first_request_tokens_{place}"), *n as f64, budget))
        });
        rows.into_iter()
            .map(|(phase, p95, budget)| (phase.to_string(), p95, budget))
            .chain(tokens)
            .map(|(phase, p95, budget)| Verdict {
                phase,
                p95,
                budget,
                margin: 0.0,
                ok: p95 <= budget,
            })
            .collect()
    }

    pub fn lines(&self) -> Vec<String> {
        let delta = self.delta_ms.map_or_else(
            || "not read".to_string(),
            |s| {
                format!(
                    "p50 {:.1} ms, max {:.1} ms (recorded, not judged)",
                    s.p50, s.max
                )
            },
        );
        vec![
            "  the head-to-head's counted rows (theseus-7gir.13):".to_string(),
            format!(
                "  syncs before the first byte: {:?} (before the request left: {:?}; budget {SYNCS_BEFORE_FIRST_BYTE})",
                self.syncs_before_first_byte, self.syncs_before_request
            ),
            format!(
                "  a fresh session's first request in tokens as sent, with the default tools: {} ({:.1} KB of \
                 bytes in the cli's, and {:.1} KB with the bench config's: recorded, not judged)",
                self.first_request_tokens
                    .iter()
                    .map(|(place, n)| {
                        let budget = theseus_core::first_request::budget(place);
                        format!("{place} {n}/{}", budget.map_or("-".to_string(), |b| b.to_string()))
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
                self.first_request_bytes as f64 / 1024.0,
                self.first_request_bytes_bench as f64 / 1024.0
            ),
            format!(
                "  deltas: {} of {} chunks reached the client; in lockstep {} of {} alone; a delta's way {delta}",
                self.deltas_seen, self.chunks, self.deltas_alone, self.chunks
            ),
        ]
    }
}

/// A verdict's unit, for its line: the frames' rows count frames.
pub fn unit(phase: &str) -> &'static str {
    match phase {
        p if p.starts_with("syncs_") => "sync(s)",
        p if p.starts_with("first_request_tokens_") => "tokens",
        p if p.starts_with("deltas_") => "delta(s) missing",
        _ => "frame(s)",
    }
}

/// `turn.submit` on a connection of its own, each line it reads handed to
/// `line` until the answer.
fn submit_on(sock: &Path, params: Value, line_seen: impl FnMut(&Value)) -> Result<Value> {
    call_on(sock, "turn.submit", params, line_seen)
}

/// One request on a connection of its own, each line it reads handed to
/// `line_seen` until the answer.
fn call_on(
    sock: &Path,
    method: &str,
    params: Value,
    mut line_seen: impl FnMut(&Value),
) -> Result<Value> {
    let s = UnixStream::connect(sock).context("connecting for a counted turn")?;
    s.set_read_timeout(Some(Duration::from_secs(60)))?;
    let req = theseus_protocol::Request::new(theseus_protocol::Id::Num(1), method, params);
    let mut line = serde_json::to_string(&req)?;
    line.push('\n');
    std::io::Write::write_all(&mut &s, line.as_bytes())?;
    let mut r = BufReader::new(&s);
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            bail!("theseusd closed the connection before answering a counted turn");
        }
        let v: Value = serde_json::from_str(&line)?;
        line_seen(&v);
        if v.get("id") == Some(&json!(1)) {
            if let Some(e) = v.get("error").filter(|e| !e.is_null()) {
                bail!("a counted turn: {e}");
            }
            return Ok(v["result"].clone());
        }
    }
}

/// One counted turn on `session`: armed, submitted on a connection that
/// reads its deltas, and what the stand-in counted.
fn counted_turn(
    d: &Driver<'_>,
    counter: &Counter,
    session: &str,
    lockstep: bool,
    n: usize,
) -> Result<(Armed, Seen)> {
    super::until_quiet(&mut Tail::at_end(&counter.wal)?)?;
    counter.arm(lockstep)?;
    let params = json!({"session_id": session, "input": format!("a counted turn {n}, {STREAM_MARK}"),
        "author": "bench", "attachments": []});
    submit_on(&d.rig.sock, params, |v| {
        if v["method"] == theseus_protocol::notify::MODEL_DELTA {
            counter.seen();
        }
    })?;
    Ok(counter.take())
}

/// What a fresh session's first request is on a daemon of the default
/// config: its bytes as the stand-in received them, and its tokens as sent
/// for each kind of place (`PLACES`) by `context.census` (nothing is sent to
/// a model). A scratch daemon of its own (its config only the stand-in, a
/// key from the environment, and its paths), one turn, the census, then its
/// clean stop.
fn default_first_request(
    theseusd: &Path,
    base: &str,
    model_entries: &impl Fn(usize) -> Vec<Entry>,
) -> Result<(u64, std::collections::BTreeMap<String, u64>)> {
    const OPENING: &str = "the default tools' first request";
    let dir = tempfile::tempdir()?;
    let (state, sock, projects) = (
        dir.path().join("state"),
        dir.path().join("sock"),
        dir.path().join("projects"),
    );
    std::fs::create_dir_all(&projects)?;
    let config = dir.path().join("config.toml");
    let mut t = toml::Table::new();
    let table = |pairs: &[(&str, toml::Value)]| {
        toml::Value::Table(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
        )
    };
    let s = |p: &Path| toml::Value::String(p.display().to_string());
    t.insert("model".into(), table(&[("api_base", base.into())]));
    t.insert(
        "secrets".into(),
        table(&[("anthropic_api_key", "env:THESEUS_H2H_KEY".into())]),
    );
    t.insert(
        "server".into(),
        table(&[("socket", s(&sock)), ("state_dir", s(&state))]),
    );
    t.insert("tools".into(), table(&[("projects_dir", s(&projects))]));
    for off in ["web", "discord", "index"] {
        t.insert(off.into(), table(&[("enabled", false.into())]));
    }
    std::fs::write(&config, toml::to_string(&t)?)?;
    let mut child = std::process::Command::new(theseusd)
        .arg("--config")
        .arg(&config)
        .env("THESEUS_H2H_KEY", "a-stand-in-key")
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(dir.path().join("theseusd.log"))?)
        .spawn()
        .context("starting a daemon of the default config")?;
    let answer = (|| {
        let until = std::time::Instant::now() + Duration::from_secs(20);
        while UnixStream::connect(&sock).is_err() {
            if std::time::Instant::now() > until {
                bail!("a daemon of the default config did not answer in 20 s");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let before = model_entries(0).len();
        submit_on(
            &sock,
            json!({"input": OPENING, "author": "bench", "attachments": []}),
            |_| {},
        )?;
        let bytes = model_entries(before + 1)
            .iter()
            .skip(before)
            .find(|e| e.opening.contains(OPENING))
            .map(|e| e.req_bytes)
            .context("the stand-in logged no request from the daemon of the default config")?;
        let mut tokens = std::collections::BTreeMap::new();
        for place in PLACES {
            let census = call_on(&sock, "context.census", json!({"place": place}), |_| {})?;
            let total = census["total_tokens"]
                .as_u64()
                .with_context(|| format!("the census of the {place} place has no total"))?;
            tokens.insert(place.to_string(), total);
        }
        Ok((bytes, tokens))
    })();
    let stop = UnixStream::connect(&sock).and_then(|s| {
        let req =
            theseus_protocol::Request::new(theseus_protocol::Id::Num(1), "shutdown", Value::Null);
        std::io::Write::write_all(
            &mut &s,
            format!("{}\n", serde_json::to_string(&req).unwrap_or_default()).as_bytes(),
        )
    });
    if stop.is_err() || !matches!(wait_for(&mut child, Duration::from_secs(10)), Some(true)) {
        let _ = child.kill();
        let _ = child.wait();
    }
    answer
}

/// The child's exit within `limit`: whether it exited, or none.
fn wait_for(child: &mut std::process::Child, limit: Duration) -> Option<bool> {
    let until = std::time::Instant::now() + limit;
    loop {
        if let Ok(Some(_)) = child.try_wait() {
            return Some(true);
        }
        if std::time::Instant::now() > until {
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The counted rows, on the bench's daemon: a fresh session's first turn
/// (its request's bytes), then [`RUNS`] warm turns free and one in lockstep.
pub fn run(
    d: &mut Driver<'_>,
    counter: &Counter,
    base: &str,
    model_entries: impl Fn(usize) -> Vec<Entry>,
) -> Result<Counted> {
    let chunks = Pace::default().chunks;
    let (first_request_bytes, first_request_tokens) =
        default_first_request(&d.rig.theseusd, base, &model_entries)?;
    let fresh = d.open_session("bench counted")?;
    let before = model_entries(0).len();
    counted_turn(d, counter, &fresh, false, 0)?;
    let first_request_bytes_bench = model_entries(before + 1)
        .get(before)
        .filter(|e| Counter::counting(e))
        .map(|e| e.req_bytes)
        .context("the stand-in logged no request for the fresh session's first turn")?;
    let (mut before_req, mut before_byte) = (Vec::new(), Vec::new());
    let (mut deltas_seen, mut delta_ms) = (chunks, Vec::new());
    for n in 1..=RUNS {
        let (a, s) = counted_turn(d, counter, &fresh, false, n)?;
        before_req.push(a.before_request.context("the counter saw no request")?);
        before_byte.push(
            a.before_first_byte
                .context("the counter saw no first byte")?,
        );
        deltas_seen = deltas_seen.min(s.at.len());
        delta_ms.extend(
            a.written
                .iter()
                .zip(&s.at)
                .map(|(w, c)| c.saturating_sub(*w) as f64 / 1e6),
        );
    }
    let (a, _) = counted_turn(d, counter, &fresh, true, RUNS + 1)?;
    Ok(Counted {
        chunks,
        syncs_before_request: before_req,
        syncs_before_first_byte: before_byte,
        first_request_tokens,
        first_request_bytes,
        first_request_bytes_bench,
        deltas_seen,
        deltas_alone: a.alone,
        delta_ms: Summary::of(&delta_ms),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counted(tokens: &[(&str, u64)]) -> Counted {
        Counted {
            chunks: 8,
            deltas_seen: 8,
            deltas_alone: 8,
            syncs_before_first_byte: vec![2],
            first_request_tokens: tokens.iter().map(|(p, n)| (p.to_string(), *n)).collect(),
            // The bytes are information: a request of any size in bytes passes on its tokens.
            first_request_bytes: 400 * 1024,
            ..Counted::default()
        }
    }

    fn verdict(c: &Counted, place: &str) -> Verdict {
        c.verdicts()
            .into_iter()
            .find(|v| v.phase == format!("first_request_tokens_{place}"))
            .unwrap_or_else(|| panic!("no verdict for {place}"))
    }

    /// The owner's budgets: 8,000 tokens in the cli and task places, 15,000 in the DM's; the channel's
    /// and the headless run's are their own, and none is above the DM's.
    #[test]
    fn a_first_request_over_its_places_budget_fails_and_one_at_it_passes() {
        for (place, budget) in [("cli", 8_000), ("task", 8_000), ("dm", 15_000)] {
            let at = counted(&[(place, budget)]);
            assert!(verdict(&at, place).ok, "{place} at {budget}");
            assert_eq!(verdict(&at, place).budget, budget as f64);
            let over = counted(&[(place, budget + 1)]);
            assert!(!verdict(&over, place).ok, "{place} at {}", budget + 1);
        }
        for place in ["channel", "headless"] {
            let budget = theseus_core::first_request::budget(place).unwrap();
            assert!(budget <= 15_000, "{place}");
            assert!(verdict(&counted(&[(place, budget)]), place).ok);
            assert!(!verdict(&counted(&[(place, budget + 1)]), place).ok);
        }
    }

    /// The byte line is gone: a request big in bytes (deferred tools' definitions) is judged on its tokens.
    #[test]
    fn the_byte_line_is_gone() {
        let c = counted(&[("cli", 7_000)]);
        assert!(c.verdicts().iter().all(|v| v.ok), "{:?}", c.verdicts());
        assert!(c.verdicts().iter().all(|v| v.phase != "first_request_kb"));
        assert_eq!(unit("first_request_tokens_dm"), "tokens");
    }
}
