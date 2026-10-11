//! `bench concurrency` (theseus-4g0l): admission under a burst. M task turns
//! are submitted 5 ms apart (an order the bench knows), then one person's
//! typed turn 0.5 s after the first, on a stand-in that holds each call
//! `--hold-ms` (2 s): the scale report's row 7, where the person's turn
//! finished 67th of 81 after 19.3 s and 42% of the pairs finished out of
//! their submission order.
//!
//! Per M it reports the total wall, each turn's wall, its admission (the
//! turn's `admission.wait` span: arrival to admission), the order inversions
//! inside the tasks' class (pairs admitted, and pairs ended, against their
//! submission order), the person's rank by end and by admission, failures,
//! and the daemon's peak RSS and CPU. The targets it judges: no inversion
//! of admission inside the class; the person's turn admitted behind at most
//! the turns admitted before it arrived; and no failure, with
//! `--stuck-secs` (`[kernel] admission_stuck_secs`) set below the queue's
//! length too, so the tail runs while the queue moves. No budget is gated:
//! `--check` exits 1 when a target is missed.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Args;
use serde::Serialize;
use serde_json::{json, Value};

use super::{after_serving, emit, scratch_on, theseusd_or_beside, Output};
use crate::fake_model::{FakeModel, Rule};
use crate::lifecycle::{Rig, Summary};
use crate::procfs;
use crate::walcount::Tail;

/// The gap between two task turns' submissions: their order is the bench's.
const GAP: Duration = Duration::from_millis(5);

/// Admissions closer than this are one moment. The bench reads a turn's
/// admission as its submission by this clock plus the daemon's wait from
/// the turn's arrival, so each carries its trip to the daemon, a few ms
/// under the burst's threads; and a wave of released slots is handed on
/// within a few ms. A turn the queue put behind another is a hold (2 s)
/// later, so 50 ms tells the two apart.
const SAME_MOMENT_MS: f64 = 50.0;

#[derive(Args)]
pub struct ConcurrencyArgs {
    /// The daemon to measure (default: the `theseusd` beside this binary).
    #[arg(long)]
    theseusd: Option<PathBuf>,
    /// The task turns submitted at once, a run each (comma-separated).
    #[arg(long, value_delimiter = ',', default_value = "8,16,32,80")]
    turns: Vec<usize>,
    /// How long the stand-in holds each call, in ms.
    #[arg(long, default_value_t = 2000)]
    hold_ms: u64,
    /// When the person's turn is submitted, after the first task turn, in ms.
    #[arg(long, default_value_t = 500)]
    person_after_ms: u64,
    /// `[kernel] admission_stuck_secs` for the daemon (default: its own,
    /// 600): below the queue's length, the tail must still run.
    #[arg(long)]
    stuck_secs: Option<u64>,
    /// Exit 1 when a target is missed.
    #[arg(long)]
    check: bool,
    /// Also write the report as JSON here.
    #[arg(long)]
    json: Option<PathBuf>,
    /// Work in this directory and keep it (default: a temporary one).
    #[arg(long)]
    dir: Option<PathBuf>,
}

/// One turn of a run, as the bench saw it.
#[derive(Serialize, Clone)]
struct Turn {
    /// `task <i>` or `person`.
    name: String,
    /// When it was submitted, from the first submission, in ms.
    submitted_ms: f64,
    /// When it was admitted (its submission and its `admission.wait`).
    admitted_ms: Option<f64>,
    /// When its answer came, from the first submission.
    ended_ms: f64,
    wall_ms: f64,
    failed: Option<String>,
    /// Its class and ticket in the daemon's admission queue (its
    /// `admission.wait` span): the order it reached the queue.
    class: Option<String>,
    ticket: Option<u64>,
}

#[derive(Serialize)]
struct Run {
    tasks: usize,
    ceiling: u64,
    stuck_secs: Option<u64>,
    total_wall_ms: f64,
    task_wall_ms: Summary,
    /// Pairs of task turns admitted out of the order they reached the
    /// daemon's queue (their tickets), by more than one moment
    /// (`SAME_MOMENT_MS`): the queue's FIFO, judged.
    admitted_inversions: usize,
    /// The same against the bench's submission order: a turn that reached
    /// the queue late (its wait for the memory pass's writer, its secrets)
    /// shows here and not above. Not judged.
    submitted_inversions: usize,
    /// Pairs of task turns ended out of their submission order, of all pairs.
    ended_inversions: usize,
    pairs: usize,
    /// The person's turn: its place by end and by admission (1 is first), its
    /// wall, and the turns admitted before it arrived.
    person_rank_by_end: usize,
    person_rank_by_admission: usize,
    person_wall_ms: f64,
    admitted_before_person_arrived: usize,
    failures: usize,
    peak_rss_mb: f64,
    cpu_ms: f64,
    ok: bool,
    turns: Vec<Turn>,
}

#[derive(Serialize)]
struct Report {
    hold_ms: u64,
    person_after_ms: u64,
    runs: Vec<Run>,
}

/// The turn's `admission.wait` span.
fn admission_span(r: &Value) -> Option<&Value> {
    fn find(span: &Value) -> Option<&Value> {
        if span["name"] == "admission.wait" {
            return Some(span);
        }
        span["children"].as_array()?.iter().find_map(find)
    }
    find(&r["trace"])
}

/// The `admission.wait` span's end, in ms from the turn's arrival.
fn admission_ms(r: &Value) -> Option<f64> {
    admission_span(r)?["end_us"].as_f64().map(|us| us / 1000.0)
}

/// Pairs `(i, j)`, i submitted before j, whose `at` runs the other way by
/// more than `slack`.
fn inversions(at: &[f64], slack: f64) -> usize {
    let mut n = 0;
    for i in 0..at.len() {
        for j in i + 1..at.len() {
            n += usize::from(at[i] > at[j] + slack);
        }
    }
    n
}

/// One turn submitted at `at`, as the bench sees it, its times from `t0`.
fn submit(t0: Instant, name: String, sid: &str, at: Instant, rig: &Rig) -> Turn {
    if let Some(wait) = at.checked_duration_since(Instant::now()) {
        std::thread::sleep(wait);
    }
    let sent = Instant::now();
    let r = rig.call(
        "turn.submit",
        json!({"session_id": sid, "input": format!("{name}: one plain turn"),
               "author": "bench", "attachments": []}),
    );
    let ended = Instant::now();
    let ms = |i: Instant| i.duration_since(t0).as_secs_f64() * 1000.0;
    let submitted_ms = ms(sent);
    let (admitted_ms, failed) = match &r {
        Ok(v) => (admission_ms(v).map(|w| submitted_ms + w), None),
        Err(e) => (None, Some(format!("{e:#}"))),
    };
    let span = r.as_ref().ok().and_then(admission_span);
    let class = span.and_then(|s| s["attrs"]["class"].as_str().map(String::from));
    let ticket = span.and_then(|s| s["attrs"]["ticket"].as_u64());
    Turn {
        name,
        submitted_ms,
        admitted_ms,
        class,
        ticket,
        ended_ms: ms(ended),
        wall_ms: ended.duration_since(sent).as_secs_f64() * 1000.0,
        failed,
    }
}

fn one_run(theseusd: &std::path::Path, a: &ConcurrencyArgs, tasks: usize) -> Result<Run> {
    let model = FakeModel::start_rules_on(
        "127.0.0.1:0",
        vec![Rule {
            when: String::new(),
            calls: vec![],
            text: Some("Done, after the stand-in's hold.".into()),
            hold_ms: a.hold_ms,
            repeat: 0,
            steps: vec![],
            ttfb_ms: None,
            chunks: None,
            chunk_ms: None,
        }],
    )?;
    let dir = a.dir.as_ref().map(|d| d.join(format!("m{tasks}")));
    let stuck = a.stuck_secs;
    let s = scratch_on(theseusd, dir.as_deref(), model, |t| {
        if let Some(secs) = stuck {
            let k = t
                .entry("kernel")
                .or_insert_with(|| toml::Value::Table(Default::default()));
            let k = k.as_table_mut().context("[kernel] is a table")?;
            k.insert("admission_stuck_secs".into(), (secs as i64).into());
        }
        Ok(())
    })?;
    let (mut daemon, _) = s.rig.start()?;
    let pid = daemon.0.id();
    let mut tail = Tail::at_start(&s.wal());
    after_serving(&mut tail)?;
    let rig = &s.rig;
    let open = |kind: &str, label: String| -> Result<String> {
        let r = rig.call("session.open", json!({"kind": kind, "label": label}))?;
        Ok(r["session_id"]
            .as_str()
            .context("no session id")?
            .to_string())
    };
    let sessions: Vec<String> = (0..tasks)
        .map(|i| open("task", format!("burst task {i}")))
        .collect::<Result<_>>()?;
    let person = open("conversation", "the person".into())?;
    let ceiling = rig.call("health", Value::Null)?["kernel"]["admission_ceiling"]
        .as_u64()
        .unwrap_or(0);
    let cpu0 = procfs::sample(pid)?.cpu_ns;
    let t0 = Instant::now();
    let person_at = t0 + Duration::from_millis(a.person_after_ms);
    let turns: Vec<Turn> = std::thread::scope(|sc| {
        let submit =
            |name: String, sid: String, at: Instant, rig: &Rig| submit(t0, name, &sid, at, rig);
        let mut each: Vec<_> = sessions
            .iter()
            .enumerate()
            .map(|(i, sid)| {
                let sid = sid.clone();
                let at = t0 + GAP * i as u32;
                sc.spawn(move || submit(format!("task {i}"), sid, at, rig))
            })
            .collect();
        let p = person.clone();
        each.push(sc.spawn(move || submit("person".into(), p, person_at, rig)));
        each.into_iter()
            .map(|h| h.join().expect("a turn's thread"))
            .collect()
    });
    let total_wall_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let end = procfs::sample(pid)?;
    s.rig.stop(&mut daemon)?;
    summarize(
        Measured {
            tasks,
            ceiling,
            stuck,
            total_wall_ms,
            peak_rss_mb: end.hwm_mb(),
            cpu_ms: end.cpu_ns.saturating_sub(cpu0) as f64 / 1e6,
        },
        turns,
    )
}

/// A run's measures besides its turns.
struct Measured {
    tasks: usize,
    ceiling: u64,
    stuck: Option<u64>,
    total_wall_ms: f64,
    peak_rss_mb: f64,
    cpu_ms: f64,
}

/// A run's turns read against its targets.
fn summarize(m: Measured, turns: Vec<Turn>) -> Result<Run> {
    let Measured {
        tasks,
        ceiling,
        stuck,
        total_wall_ms,
        peak_rss_mb,
        cpu_ms,
    } = m;
    let (task_turns, person_turn) = turns.split_at(tasks);
    let me = &person_turn[0];
    let ok_tasks: Vec<&Turn> = task_turns.iter().filter(|t| t.failed.is_none()).collect();
    let admitted: Vec<f64> = ok_tasks
        .iter()
        .map(|t| t.admitted_ms.unwrap_or(f64::MAX))
        .collect();
    let ended: Vec<f64> = ok_tasks.iter().map(|t| t.ended_ms).collect();
    let rank = |of: f64, slack: f64, all: &mut dyn Iterator<Item = f64>| {
        1 + all.filter(|x| *x + slack < of).count()
    };
    let person_admitted = me.admitted_ms.unwrap_or(f64::MAX);
    let person_rank_by_end = rank(me.ended_ms, 0.0, &mut task_turns.iter().map(|t| t.ended_ms));
    // Admitted in the same moment as the person's turn is not ahead of it.
    let person_rank_by_admission = rank(
        person_admitted,
        SAME_MOMENT_MS,
        &mut task_turns.iter().filter_map(|t| t.admitted_ms),
    );
    let admitted_before_person_arrived = task_turns
        .iter()
        .filter_map(|t| t.admitted_ms)
        .filter(|at| *at < me.submitted_ms)
        .count();
    let failures = turns.iter().filter(|t| t.failed.is_some()).count();
    // The queue's own order: by ticket, the order the turns reached it.
    let mut by_ticket: Vec<&&Turn> = ok_tasks.iter().collect();
    by_ticket.sort_by_key(|t| t.ticket.unwrap_or(u64::MAX));
    let queued: Vec<f64> = by_ticket
        .iter()
        .map(|t| t.admitted_ms.unwrap_or(f64::MAX))
        .collect();
    let admitted_inversions = inversions(&queued, SAME_MOMENT_MS);
    let submitted_inversions = inversions(&admitted, SAME_MOMENT_MS);
    let walls: Vec<f64> = task_turns.iter().map(|t| t.wall_ms).collect();
    // Each turn queued in its class: the tasks as tasks, the person's first.
    let classed = task_turns
        .iter()
        .all(|t| t.failed.is_some() || t.class.as_deref() == Some("task"))
        && me.class.as_deref() == Some("person");
    let ok = failures == 0
        && classed
        && admitted_inversions == 0
        && person_rank_by_admission <= admitted_before_person_arrived + 1;
    Ok(Run {
        tasks,
        ceiling,
        stuck_secs: stuck,
        total_wall_ms,
        task_wall_ms: Summary::of(&walls).context("no task turns")?,
        admitted_inversions,
        submitted_inversions,
        ended_inversions: inversions(&ended, 0.0),
        pairs: ok_tasks.len() * ok_tasks.len().saturating_sub(1) / 2,
        person_rank_by_end,
        person_rank_by_admission,
        person_wall_ms: me.wall_ms,
        admitted_before_person_arrived,
        failures,
        peak_rss_mb,
        cpu_ms,
        ok,
        turns,
    })
}

fn print_run(r: &Run) {
    println!(
        "M={} (ceiling {}{}): all done in {:.1} s; task walls p50 {:.0} ms, p95 {:.0}, max {:.0}",
        r.tasks,
        r.ceiling,
        r.stuck_secs
            .map_or(String::new(), |s| format!(", stuck window {s} s")),
        r.total_wall_ms / 1000.0,
        r.task_wall_ms.p50,
        r.task_wall_ms.p95,
        r.task_wall_ms.max,
    );
    println!(
        "  inversions inside the tasks' class: {} admitted against the queue's order ({} against \
         the submissions'), {} ended, of {} pairs ({:.1}% ended out of order)",
        r.admitted_inversions,
        r.submitted_inversions,
        r.ended_inversions,
        r.pairs,
        100.0 * r.ended_inversions as f64 / r.pairs.max(1) as f64,
    );
    println!(
        "  the person's turn: admitted {} of {} (behind {}, {} admitted before it arrived), \
         ended {} of {}, wall {:.0} ms",
        r.person_rank_by_admission,
        r.tasks + 1,
        r.person_rank_by_admission - 1,
        r.admitted_before_person_arrived,
        r.person_rank_by_end,
        r.tasks + 1,
        r.person_wall_ms,
    );
    println!(
        "  failures {}; the daemon's peak RSS {:.0} MB, CPU {:.0} ms: {}",
        r.failures,
        r.peak_rss_mb,
        r.cpu_ms,
        if r.ok { "ok" } else { "MISSED A TARGET" }
    );
    for t in r.turns.iter().filter(|t| t.failed.is_some()) {
        println!("  {} failed: {}", t.name, t.failed.as_deref().unwrap_or(""));
    }
}

pub fn concurrency_cmd(a: ConcurrencyArgs) -> Result<()> {
    let theseusd = theseusd_or_beside(a.theseusd.clone())?;
    let mut runs = Vec::new();
    for &m in &a.turns {
        let r = one_run(&theseusd, &a, m.max(1))?;
        print_run(&r);
        runs.push(r);
    }
    let ok = runs.iter().all(|r| r.ok);
    let report = Report {
        hold_ms: a.hold_ms,
        person_after_ms: a.person_after_ms,
        runs,
    };
    let out = Output {
        json: a.json.as_deref(),
        record: None,
        label: None,
    };
    emit("concurrency", &out, &report, &[], &[], ok)?;
    if a.check && !ok {
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inversions_count_pairs_out_of_order() {
        assert_eq!(inversions(&[1.0, 2.0, 3.0], 0.0), 0);
        assert_eq!(inversions(&[3.0, 2.0, 1.0], 0.0), 3);
        assert_eq!(inversions(&[1.0, 3.0, 2.0, 4.0], 0.0), 1);
        // One moment's jitter is no inversion; a wave's is.
        assert_eq!(inversions(&[2000.0, 2005.0, 2001.0, 4000.0], 50.0), 0);
        assert_eq!(inversions(&[4000.0, 2000.0, 2001.0], 50.0), 2);
    }

    #[test]
    fn the_admission_wait_is_read_from_the_turns_trace() {
        let r = json!({"trace": {"name": "turn", "children": [
            {"name": "secrets.wait", "end_us": 10},
            {"name": "admission.wait", "start_us": 10, "end_us": 2500}
        ]}});
        assert_eq!(admission_ms(&r), Some(2.5));
        assert_eq!(admission_ms(&json!({"trace": {"name": "turn"}})), None);
    }
}
