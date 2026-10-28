//! Routines in the core (theseus-pvm5c; the horizon report's §5.1, §5.4):
//! recurring work done right. A routine is a conversation's execution of its own
//! with one pending wake (`purpose: Routine`, `run: Task`) and a record, the
//! META key `routine:<session id>` (`RoutineSpec`). Each occurrence that fires
//! opens a fresh task under the routine's execution, so a run's context never
//! grows with the runs before it; its cost is the routine's month, its limit,
//! and each run carves its own budget from what the month has left.
//!
//! - **Shared by the owner's verbs and the model's tool** (`create`): nothing
//!   here needs the `Core`, only the kernel, the store and the outbox, so the
//!   tool the harness runs behind a card (`routine_tool.rs`) and the RPC
//!   (`rpc/routines.rs`) make a routine the same way.
//! - **The firing decision** (`plan`): the kernel (`Kernel::fire_routines`) gives
//!   what the wake and the budget say; this decides, in order: after downtime,
//!   `catch_up` (one run saying how many were missed, or none); a run still going
//!   (`overlap`: a routine never runs twice at once); the month's money (`budget`:
//!   skipped, said once a month); else the run, its brief headed with the run's
//!   number, its due time, when it started, what was missed and what the last run
//!   reported. The kernel applies it in one frame.
//! - **The month** is the routine execution's limit; the 1st of the local month
//!   starts it again at the first run of the month (`budget.reset { why: "month" }`).
//! - **`Book`** holds the records in memory (one META prefix scan, the first
//!   time something needs them), so the commitments block of a turn reads memory.

pub mod grant;
pub mod watch;

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use anyhow::{bail, Result};
use serde_json::json;
use theseus_kernel::cron::{describe, hm, hms_tenths, local, month_key, parse_schedule, when};
use theseus_kernel::TimeZone;
use theseus_kernel::{
    micros_to_usd, usd_to_micros, ExecState, Execution, Kernel, NewWake, Post, Purpose, RetryClass,
    RoutineDue, RoutinePlan, RunPlan, SessionKind, TaskRun, WakeRun,
};
use theseus_protocol::routines::{
    Carry, CatchUp, RoutineBlocked, RoutineInfo, RoutineSpec, RoutineStop, RunMark, RunRow, Speak,
    BLOCKED_PREFIX, KEY_PREFIX, NOTHING_NEW, RUN_PREFIX,
};
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord, Store as _};

use crate::config::RoutinesConfig;
use crate::node::{Node, Origin};
use crate::outbox::{Outbox, NONCE_KEY};
use crate::session::{SessionRecord, TaskOf};
use crate::store::Store;

/// A title is a line, and a brief is not a document.
const MAX_TITLE: usize = 120;
const MAX_BRIEF: usize = 8_000;

/// The most of the last run's report a run is given (theseus-97b8c). It is
/// there so run N reports the change, not so it reads a transcript: a report
/// longer than this is carried head and tail with the middle left out.
const MAX_CARRY: usize = 2_000;

/// The live routines by session id.
type Specs = BTreeMap<String, Arc<RoutineSpec>>;

/// The routines in memory, and the defaults a new one takes.
#[derive(Debug, Default)]
pub struct Book {
    pub cfg: RoutinesConfig,
    specs: RwLock<Option<Arc<Specs>>>,
    writes: std::sync::Mutex<()>,
    warmed: std::sync::atomic::AtomicBool,
    /// Each routine's grant by its session id, read from its wake once and
    /// kept (`grant_for`); a change to the routine drops its entry.
    grants: RwLock<BTreeMap<String, Arc<grant::Grant>>>,
    /// The runs not yet settled (`watch.rs`), by their task session.
    pub(crate) open: std::sync::Mutex<BTreeMap<String, String>>,
    /// Questions a run asked outside its grant that passed the run's deadline,
    /// for the tick to end their runs blocked (`rpc/routines_watch.rs`).
    pub(crate) blocked: std::sync::Mutex<Vec<theseus_kernel::Action>>,
    /// The open set was seeded from the last runs after the start.
    pub(crate) seeded: std::sync::atomic::AtomicBool,
}

impl Book {
    pub fn new(cfg: &RoutinesConfig) -> Self {
        Self {
            cfg: cfg.clone(),
            ..Default::default()
        }
    }

    /// Read the records now (the harness's first tick; or the first reader).
    pub fn load(&self, store: &Store) -> Result<Arc<Specs>> {
        if let Some(b) = self.held() {
            return Ok(b);
        }
        theseus_store::blocking(|| {
            let _w = self.writes.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(b) = self.held() {
                return Ok(b);
            }
            let mut all = BTreeMap::new();
            for r in store.inner().latest_with_prefix(kinds::META, KEY_PREFIX)? {
                match r.decode::<RoutineSpec>() {
                    Ok(s) if s.deleted_ms.is_none() => {
                        all.insert(s.session_id.clone(), Arc::new(s));
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!(key = ?r.key, error = %e,
                        "a routine's record does not read: left out"),
                }
            }
            let all = Arc::new(all);
            *self.specs.write().unwrap_or_else(|e| e.into_inner()) = Some(all.clone());
            Ok(all)
        })
    }

    /// Read the records, once, on the driver's first tick after serving.
    pub fn warm_once(&self, store: &Store) {
        if !self.warmed.swap(true, std::sync::atomic::Ordering::Relaxed) {
            if let Err(e) = self.load(store) {
                tracing::warn!(error = %format!("{e:#}"), "the routines' records cannot be read");
            }
        }
    }

    /// The snapshot if it is loaded; a turn's reads use this and never load.
    pub fn held(&self) -> Option<Arc<Specs>> {
        self.specs.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The grant of the routine a task is the run of (theseus-52tut), or none:
    /// the book not loaded yet, a task whose parent is not a routine (a task a
    /// run starts has the run as its parent), or a routine with no grant. One
    /// map lookup; the first call of a routine's run reads its wake.
    pub fn grant_for(
        &self,
        kernel: &Kernel,
        _store: &Store,
        task: &TaskOf,
    ) -> Option<Arc<grant::Grant>> {
        let specs = self.held()?;
        let spec = specs.get(&task.parent_session)?;
        if spec.execution_id != task.parent_execution {
            return None;
        }
        let cached = self
            .grants
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&spec.session_id)
            .cloned();
        let g = match cached {
            Some(g) => g,
            None => {
                let tools = kernel
                    .execution(&spec.execution_id)
                    .ok()
                    .flatten()
                    .and_then(|e| written_grant(&e))
                    .unwrap_or_default();
                let g = Arc::new(grant::Grant::of(&tools));
                self.grants
                    .write()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(spec.session_id.clone(), g.clone());
                g
            }
        };
        (!g.is_empty()).then_some(g)
    }

    /// Keep `spec` after its record was written (a deleted one leaves).
    pub fn set(&self, spec: &RoutineSpec) {
        self.grants
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&spec.session_id);
        let _w = self.writes.lock().unwrap_or_else(|e| e.into_inner());
        let mut g = self.specs.write().unwrap_or_else(|e| e.into_inner());
        let Some(all) = g.as_ref() else {
            return;
        };
        let mut next = (**all).clone();
        match spec.deleted_ms {
            Some(_) => next.remove(&spec.session_id),
            None => next.insert(spec.session_id.clone(), Arc::new(spec.clone())),
        };
        *g = Some(Arc::new(next));
    }

    /// Every live routine, oldest first.
    pub fn all(&self, store: &Store) -> Result<Vec<Arc<RoutineSpec>>> {
        let mut v: Vec<Arc<RoutineSpec>> = self.load(store)?.values().cloned().collect();
        v.sort_by(|a, b| (a.created_ms, &a.session_id).cmp(&(b.created_ms, &b.session_id)));
        Ok(v)
    }

    /// The routine `name` names: its session id, or the end of it (four
    /// characters at least), or its title (whole or a part of it, any case),
    /// when exactly one matches.
    pub fn find(&self, store: &Store, name: &str) -> Result<Arc<RoutineSpec>, String> {
        let all = self.all(store).map_err(|e| format!("{e:#}"))?;
        let n = name.trim().trim_start_matches('…');
        let by_id: Vec<&Arc<RoutineSpec>> = all
            .iter()
            .filter(|s| n.len() >= 4 && (s.session_id == n || s.session_id.ends_with(n)))
            .collect();
        let lower = n.to_lowercase();
        let by_title: Vec<&Arc<RoutineSpec>> = all
            .iter()
            .filter(|s| !lower.is_empty() && s.title.to_lowercase().contains(&lower))
            .collect();
        let hits = if by_id.is_empty() { by_title } else { by_id };
        match hits[..] {
            [one] => Ok(one.clone()),
            [] if n.len() < 4 => Err(format!(
                "`{name}` names no routine: give at least four characters of its id, or its title"
            )),
            [] => Err(format!("no routine is named `{name}`")),
            _ => Err(format!(
                "`{name}` names {} routines: give more of its id or title",
                hits.len()
            )),
        }
    }

    pub fn by_execution(&self, store: &Store, execution_id: &str) -> Option<Arc<RoutineSpec>> {
        self.load(store)
            .ok()?
            .values()
            .find(|s| s.execution_id == execution_id)
            .cloned()
    }

    /// The routines a conversation made, from memory alone (a turn's
    /// commitments block): none while the book is not loaded.
    pub fn made_in(&self, session_id: &str) -> Vec<Arc<RoutineSpec>> {
        let Some(all) = self.held() else {
            return vec![];
        };
        let mut v: Vec<Arc<RoutineSpec>> = all
            .values()
            .filter(|s| s.created_in.as_deref() == Some(session_id))
            .cloned()
            .collect();
        v.sort_by_key(|s| s.created_ms);
        v
    }
}

/// How people name a routine: the last six characters of its id.
pub fn short(id: &str) -> String {
    crate::task::short(id)
}

/// The grant as written, from the routine's wake (its `TaskRun.tools`).
pub fn written_grant(e: &Execution) -> Option<Vec<String>> {
    e.wakes.iter().find_map(|w| match (&w.purpose, &w.run) {
        (Purpose::Routine, WakeRun::Task(t)) => Some(t.tools.clone()),
        _ => None,
    })
}

pub fn spec_record(spec: &RoutineSpec) -> Result<NewRecord> {
    NewRecord::json(
        kinds::META,
        Some(&format!("{KEY_PREFIX}{}", spec.session_id)),
        spec,
    )
}

/// A `routine.changed` row, for the frame that writes the record.
pub fn changed_row(spec: &RoutineSpec, what: &str, by: &str) -> Result<NewRecord> {
    let row = crate::ledger::LedgerRow::new(
        LedgerKind::RoutineChanged,
        Some(&spec.session_id),
        None,
        json!({"execution_id": spec.execution_id, "wake_id": spec.wake_id, "what": what,
               "by": by, "title": spec.title, "schedule": spec.schedule,
               "run_usd": micros_to_usd(spec.run_micros),
               "month_usd": micros_to_usd(spec.month_micros)}),
    );
    NewRecord::json(kinds::LEDGER, None, &row)
}

/// The first line of a text, clipped.
pub fn first_line(text: &str) -> String {
    let l = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let mut out: String = l.chars().take(160).collect();
    if l.chars().count() > 160 {
        out.push('…');
    }
    out
}

/// What the owner or the model asks for.
#[derive(Debug, Clone, Default)]
pub struct NewRoutine {
    pub title: String,
    pub schedule: String,
    pub brief: String,
    pub budget_usd: Option<f64>,
    pub month_usd: Option<f64>,
    pub deliver_to: Option<String>,
    pub catch_up: Option<String>,
    pub speak: Option<String>,
    /// The first occurrence is the first at or after this time, as a person
    /// writes one (`mon 9:00`, `in 2h`).
    pub start: Option<String>,
    /// Where the series ends: `after N runs`, or a time.
    pub stop: Option<String>,
    /// What each run is given of the run before it.
    pub carry: Option<String>,
    /// The tools its runs use without asking (`grant::check` has read them).
    pub grant: Vec<String>,
}

/// Read a stop: `after 3 runs` (or `3 runs`), else a time as `parse_when`
/// reads one. Nothing else: a condition to check on a timer is a `watch`
/// (`watch_tool.rs`), which already evaluates one with no model.
pub fn parse_stop(
    text: &str,
    now_ms: u64,
    zone: &TimeZone,
    max_ahead_ms: u64,
) -> Result<RoutineStop, String> {
    let lower = text.trim().to_ascii_lowercase();
    let body = lower.strip_prefix("after ").unwrap_or(&lower).trim();
    let count = body
        .strip_suffix("runs")
        .or_else(|| body.strip_suffix("run"))
        .unwrap_or(body)
        .trim();
    if let Ok(n) = count.parse::<u32>() {
        if n == 0 {
            return Err("a stop of `after N runs` needs at least one run".into());
        }
        return Ok(RoutineStop {
            text: format!("after {n} run{}", if n == 1 { "" } else { "s" }),
            after_runs: Some(n),
            at_ms: None,
        });
    }
    let at_ms = crate::wake_when::parse_when(text.trim(), now_ms, zone, max_ahead_ms)?;
    Ok(RoutineStop {
        text: local(zone, at_ms, "%Y-%m-%d %H:%M"),
        after_runs: None,
        at_ms: Some(at_ms),
    })
}

/// Whether a stop has been used up: its runs are done, or its time has come.
pub fn stop_spent(stop: &RoutineStop, runs: u32, now_ms: u64) -> bool {
    stop.spent_after(runs) || stop.at_ms.is_some_and(|t| now_ms >= t)
}

/// Where a request's series begins and ends (theseus-97b8c), read and checked
/// once so the owner's verb, the model's tool and the card all say the same.
#[derive(Debug, Clone, Default)]
pub struct Bounds {
    /// The start: the first occurrence is the first at or after it.
    pub start_ms: Option<u64>,
    pub stop: Option<RoutineStop>,
}

/// The start a request asks for, before its schedule is read: the schedule is
/// then read from there rather than from now.
pub fn start_of(
    n: &NewRoutine,
    zone: &TimeZone,
    now_ms: u64,
    max_ahead_ms: u64,
) -> Result<Option<u64>, String> {
    match n.start.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Ok(Some(crate::wake_when::parse_when(
            s,
            now_ms,
            zone,
            max_ahead_ms,
        )?)),
        None => Ok(None),
    }
}

/// The stop a request asks for, once its first occurrence is known: a stop
/// that comes before it would run nothing, and says so.
pub fn stop_of(
    n: &NewRoutine,
    zone: &TimeZone,
    now_ms: u64,
    max_ahead_ms: u64,
    first_ms: u64,
) -> Result<Option<RoutineStop>, String> {
    let Some(text) = n.stop.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let stop = parse_stop(text, now_ms, zone, max_ahead_ms)?;
    if stop.at_ms.is_some_and(|at| at < first_ms) {
        return Err(format!(
            "the stop ({}) comes before the first run ({}): nothing would run",
            stop.text,
            when(zone, first_ms)
        ));
    }
    Ok(Some(stop))
}

/// The stop as the card and the lists word it.
fn stop_words(stop: Option<&RoutineStop>) -> String {
    match stop {
        Some(s) => s.words(),
        None => "it runs until it is paused".to_string(),
    }
}

/// A request read and checked, nothing written: what the card says.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub spec: RoutineSpec,
    pub repeat: theseus_kernel::Repeat,
    pub first_ms: u64,
    /// The next occurrences, for the card.
    pub next: Vec<u64>,
    /// The grant, checked, as the wake will carry it.
    pub grant: Vec<String>,
    /// The series' `until`, when the stop is a time: the wake carries it.
    pub until_ms: Option<u64>,
}

/// A dollar figure a person may name: finite and above zero.
pub fn usd_ok(usd: f64) -> bool {
    usd.is_finite() && usd > 0.0
}

fn dollars(usd: f64) -> String {
    if (usd - usd.round()).abs() < 0.005 {
        format!("${}", usd.round() as i64)
    } else {
        format!("${usd:.2}")
    }
}

impl Prepared {
    /// The card from its parts, before a routine is prepared (the tool's plan).
    pub fn words(
        n: &NewRoutine,
        repeat: &theseus_kernel::Repeat,
        next: &[u64],
        run_usd: f64,
        month_usd: f64,
        zone: &TimeZone,
        bounds: &Bounds,
    ) -> String {
        let speak = Speak::parse(n.speak.as_deref().unwrap_or("")).unwrap_or_default();
        let to = n.deliver_to.as_deref().unwrap_or("dm");
        let next: Vec<String> = next.iter().map(|m| when(zone, *m)).collect();
        format!(
            "Create routine \"{}\": {}{} (next: {}) · {} · up to {} a run, {} a month · {} · {}",
            n.title.trim(),
            at_words(&describe(repeat, zone)),
            bounds
                .start_ms
                .map_or(String::new(), |m| format!(" from {}", when(zone, m))),
            next.join(", "),
            stop_words(bounds.stop.as_ref()),
            dollars(run_usd),
            dollars(month_usd),
            deliver_words(to, speak),
            trust_words(&n.grant),
        )
    }

    /// `Create routine "Morning digest": weekdays at 08:00 (next: Fri 08:00,
    /// Mon 08:00, Tue 08:00) · up to $0.50 a run, $10 a month · posts to your DM`
    pub fn card(&self, zone: &TimeZone) -> String {
        let s = &self.spec;
        let next: Vec<String> = self.next.iter().map(|m| when(zone, *m)).collect();
        format!(
            "Create routine \"{}\": {}{} (next: {}) · {} · up to {} a run, {} a month · {} · {}",
            s.title,
            at_words(&describe(&self.repeat, zone)),
            s.start_ms
                .map_or(String::new(), |m| format!(" from {}", when(zone, m))),
            next.join(", "),
            stop_words(s.stop.as_ref()),
            dollars(micros_to_usd(s.run_micros)),
            dollars(micros_to_usd(s.month_micros)),
            deliver_words(&s.deliver_to, s.speak),
            trust_words(&self.grant),
        )
    }
}

/// What a run may do unasked, for the creation card: the grant, or the
/// plain rule. A call outside the grant asks, and with nobody there the run
/// ends blocked, naming it (theseus-52tut).
pub fn trust_words(grant: &[String]) -> String {
    if grant.is_empty() {
        "each run asks before it uses a tool, as any task does".to_string()
    } else {
        format!(
            "runs use without asking: {} · any other tool asks, and a run nobody answers ends \
             blocked, naming it",
            grant.join(", ")
        )
    }
}

/// `weekdays 08:00` as `weekdays at 08:00`.
fn at_words(series: &str) -> String {
    match series.rsplit_once(' ') {
        Some((a, clock)) if clock.contains(':') && !a.starts_with("every") => {
            format!("{a} at {clock}")
        }
        _ => series.to_string(),
    }
}

fn deliver_words(to: &str, speak: Speak) -> String {
    let base = match to {
        "dm" => "posts to your DM".to_string(),
        "none" => "posts nowhere, it only logs".to_string(),
        other => format!("posts to {other}"),
    };
    match speak {
        Speak::Always => base,
        Speak::WhenNotable => format!("{base} only when there is something new"),
    }
}

/// Read and check a request. `by` and `created_in` go in the record.
pub fn prepare(
    kernel: &Kernel,
    cfg: &RoutinesConfig,
    n: &NewRoutine,
    by: &str,
    created_in: Option<&str>,
) -> Result<Prepared, String> {
    let title = n.title.trim();
    if title.is_empty() || title.chars().count() > MAX_TITLE || title.contains('\n') {
        return Err(format!(
            "a routine's title is one line of at most {MAX_TITLE} characters"
        ));
    }
    let brief = n.brief.trim();
    if brief.is_empty() {
        return Err("a routine needs a brief: say what each run does".into());
    }
    if brief.chars().count() > MAX_BRIEF {
        return Err(format!("the brief is over {MAX_BRIEF} characters"));
    }
    let kc = kernel.config();
    let now = kernel.now_ms();
    // The start moves the first occurrence, so the series is read from there
    // rather than from now; a stop that is a time becomes the series' own
    // `until`, which the kernel already ends a series at (theseus-97b8c).
    let ahead = kc.wakes.max_ahead_ms;
    let start_ms = start_of(n, &kc.zone, now, ahead)?;
    let mut parsed = parse_schedule(
        &n.schedule,
        &kc.zone,
        start_ms.unwrap_or(now),
        kc.min_repeat_ms,
    )?;
    let stop = stop_of(n, &kc.zone, now, ahead, parsed.first_ms)?;
    parsed.repeat.until_ms = stop.as_ref().and_then(|s| s.at_ms);
    let carry = match &n.carry {
        Some(c) => Carry::parse(c).ok_or("carry is last_report or none")?,
        None => Carry::LastReport,
    };
    let run_usd = n.budget_usd.unwrap_or(cfg.default_run_usd);
    let month_usd = n.month_usd.unwrap_or(cfg.default_month_usd);
    if !(usd_ok(run_usd) && usd_ok(month_usd)) || run_usd > month_usd {
        return Err(format!(
            "a run's budget (${run_usd}) must be above 0 and no more than the month's (${month_usd})"
        ));
    }
    let catch_up = match &n.catch_up {
        Some(c) => CatchUp::parse(c).ok_or("catch_up is auto, one, or none")?,
        None => CatchUp::Auto,
    };
    let speak = match &n.speak {
        Some(s) => Speak::parse(s).ok_or("speak is always or when_notable")?,
        None => Speak::Always,
    };
    let deliver_to = match n.deliver_to.as_deref().map(str::trim) {
        None | Some("") | Some("dm") => "dm".to_string(),
        Some("none") => "none".to_string(),
        Some(p) if p.starts_with("discord:") => p.to_string(),
        Some(p) => {
            return Err(format!(
                "deliver_to is `dm`, `none`, or a place such as `discord:channel:<id>`, not `{p}`"
            ))
        }
    };
    let period_ms = {
        let mut gap = u64::MAX;
        let mut prev = parsed.first_ms;
        for _ in 0..8 {
            match parsed.repeat.next_after(&kc.zone, prev) {
                Some(next) => {
                    gap = gap.min(next - prev);
                    prev = next;
                }
                None => break,
            }
        }
        if gap == u64::MAX {
            parsed.repeat.every.nominal_ms()
        } else {
            gap
        }
    };
    let mut next = vec![parsed.first_ms];
    let mut prev = parsed.first_ms;
    while next.len() < 3 {
        match parsed.repeat.next_after(&kc.zone, prev) {
            Some(t) => {
                next.push(t);
                prev = t;
            }
            None => break,
        }
    }
    let spec = RoutineSpec {
        title: title.to_string(),
        schedule: n.schedule.trim().to_string(),
        brief: brief.to_string(),
        run_micros: usd_to_micros(run_usd),
        month_micros: usd_to_micros(month_usd),
        catch_up,
        speak,
        carry,
        start_ms,
        stop,
        deliver_to,
        period_ms,
        created_ms: now,
        created_by: by.to_string(),
        created_in: created_in.map(str::to_string),
        month: month_key(&kc.zone, now),
        ..Default::default()
    };
    Ok(Prepared {
        spec,
        first_ms: parsed.first_ms,
        until_ms: parsed.repeat.until_ms,
        repeat: parsed.repeat,
        next,
        grant: n.grant.clone(),
    })
}

/// Make the routine: its conversation (a session and an execution whose limit
/// is the month), then its wake and its record in one frame.
pub fn create(
    kernel: &Kernel,
    store: &Store,
    book: &Book,
    p: Prepared,
    by: &str,
) -> Result<RoutineSpec> {
    let live = book.all(store)?.len();
    if live >= book.cfg.max_routines {
        bail!(
            "the daemon already holds {live} routines, the most it may ({}): delete one first",
            book.cfg.max_routines
        );
    }
    let mut spec = p.spec;
    let mut rec = SessionRecord::new(
        SessionKind::Conversation,
        Some(format!("Routine: {}", spec.title)),
    );
    let authority = theseus_kernel::Authority {
        principal: crate::turn::OPERATOR.to_string(),
        ..Default::default()
    };
    let exec = kernel.frame(&[], |k| {
        let exec = k.open_execution(
            &rec.session_id,
            rec.kind,
            authority,
            Some(spec.month_micros),
            None,
        )?;
        rec.execution_id = Some(exec.id.clone());
        let opened = crate::ledger::LedgerRow::new(
            LedgerKind::SessionOpened,
            Some(&rec.session_id),
            None,
            json!({"execution_id": exec.id, "routine": true}),
        );
        k.stage(&[
            NewRecord::json(kinds::SESSION, Some(&rec.session_id), &rec)?,
            NewRecord::json(kinds::LEDGER, None, &opened)?,
        ])?;
        Ok(exec)
    })?;
    spec.session_id = rec.session_id;
    spec.execution_id = exec.id.clone();
    spec.wake_id = theseus_kernel::wake_id(&crate::new_id("act"));
    let row = changed_row(&spec, "created", by)?;
    let new = NewWake {
        due_at_ms: p.first_ms,
        note: spec.title.clone(),
        target: None,
        repeat: Some(p.repeat),
        purpose: Purpose::Routine,
        run: WakeRun::Task(TaskRun {
            brief: spec.brief.clone(),
            limit_micros: spec.run_micros,
            tools: p.grant,
        }),
    };
    kernel.create_wake_with(
        &exec.id,
        Some(spec.wake_id.clone()),
        new,
        by,
        vec![spec_record(&spec)?, row],
    )?;
    book.set(&spec);
    Ok(spec)
}

/// What a firing reads and writes beside the kernel.
pub struct Env<'a> {
    pub kernel: &'a Kernel,
    pub store: &'a Store,
    pub outbox: &'a Outbox,
    /// Where a run's report and the month's notice go.
    pub target: Option<String>,
}

fn run_state(e: &Execution) -> &'static str {
    match e.state {
        ExecState::Complete => "complete",
        ExecState::Failed => "failed",
        ExecState::Cancelled => "cancelled",
        ExecState::Queued => "queued",
        _ => "running",
    }
}

/// One run, as `routine.runs` and the list show it.
pub fn run_row(env: &Env<'_>, execution_id: &str) -> Result<Option<RunRow>> {
    let Some(e) = env.kernel.execution(execution_id)? else {
        return Ok(None);
    };
    let zone = &env.kernel.config().zone;
    let mark: Option<RunMark> = env
        .store
        .get_meta(&format!("{RUN_PREFIX}{}", e.session_id))?;
    let blocked: Option<RoutineBlocked> = env
        .store
        .get_meta(&format!("{BLOCKED_PREFIX}{}", e.session_id))?;
    let report = if let Some(b) = &blocked {
        Some(format!("blocked: {} {}", b.tool, b.reason))
    } else if e.state.is_terminal() {
        match crate::task::load_report(env.store, env.kernel, &e.id)? {
            Some(r) if r.outcome == "complete" => r.text.as_deref().map(first_line),
            Some(r) => Some(format!(
                "{}: {}",
                r.outcome,
                r.reason.as_deref().unwrap_or("no reason given")
            )),
            None => None,
        }
    } else {
        None
    };
    let end = if e.state.is_terminal() {
        e.updated_at_ms
    } else {
        env.kernel.now_ms()
    };
    Ok(Some(RunRow {
        task: e.session_id.clone(),
        short: crate::task::short(&e.session_id),
        run: mark.map_or(0, |m| m.run),
        state: match &blocked {
            Some(_) => "blocked".to_string(),
            None => run_state(&e).to_string(),
        },
        started_ms: e.created_at_ms,
        started_local: local(zone, e.created_at_ms, "%Y-%m-%d %H:%M:%S"),
        duration_ms: end.saturating_sub(e.created_at_ms),
        cost_usd: micros_to_usd(e.budget.spent_micros),
        report,
        blocked,
    }))
}

/// A routine as the verbs show it.
pub fn info_of(env: &Env<'_>, spec: &RoutineSpec) -> Result<RoutineInfo> {
    let kc = env.kernel.config();
    let e = env.kernel.execution(&spec.execution_id)?;
    let wake = e
        .as_ref()
        .and_then(|e| e.wakes.iter().find(|w| w.purpose == Purpose::Routine));
    let spent = match (&e, spec.month == month_key(&kc.zone, env.kernel.now_ms())) {
        (Some(e), true) => e.budget.spent_micros,
        _ => 0,
    };
    let running = match &spec.last_task {
        Some(t) => env
            .kernel
            .execution(t)?
            .is_some_and(|t| !t.state.is_terminal()),
        None => false,
    };
    let last = match &spec.last_task {
        Some(t) => run_row(env, t)?,
        None => None,
    };
    let series = wake
        .and_then(|w| w.repeat.as_ref())
        .map(|r| describe(r, &kc.zone))
        .unwrap_or_else(|| spec.schedule.clone());
    Ok(RoutineInfo {
        id: spec.session_id.clone(),
        short: short(&spec.session_id),
        title: spec.title.clone(),
        schedule: spec.schedule.clone(),
        series,
        brief: spec.brief.clone(),
        paused: wake.is_some_and(|w| w.paused_at_ms.is_some()),
        next_ms: wake.map(|w| w.due_at_ms),
        next: wake.map(|w| when(&kc.zone, w.due_at_ms)),
        run_usd: micros_to_usd(spec.run_micros),
        month_usd: micros_to_usd(spec.month_micros),
        month_spent_usd: micros_to_usd(spent),
        deliver_to: spec.deliver_to.clone(),
        catch_up: spec.catch_up.as_str().to_string(),
        speak: spec.speak.as_str().to_string(),
        carry: spec.carry.as_str().to_string(),
        stop: spec.stop.as_ref().map(|s| s.text.clone()),
        stop_ms: spec.stop.as_ref().and_then(|s| s.at_ms),
        stopped: spec
            .stopped_ms
            .and(spec.stop.as_ref().map(|s| s.text.clone())),
        start_ms: spec.start_ms,
        runs: spec.runs,
        grant: e.as_ref().and_then(written_grant).unwrap_or_default(),
        streak: watch::streak(env, spec),
        running,
        last,
    })
}

/// Whether the daemon being down across occurrences runs one anyway.
pub fn catches_up(spec: &RoutineSpec) -> bool {
    match spec.catch_up {
        CatchUp::One => true,
        CatchUp::None => false,
        CatchUp::Auto => spec.period_ms >= 3_600_000,
    }
}

/// A fire's decision and the record as it will be once the kernel writes the frame.
type Planned = (RoutinePlan, RoutineSpec);

fn skipped(why: &'static str, post: Option<Post>, next: RoutineSpec) -> Result<Planned> {
    let extra = vec![spec_record(&next)?];
    Ok((RoutinePlan::Skip { why, post, extra }, next))
}

/// The run's budget: the wake's, else the routine's.
fn wanted(spec: &RoutineSpec, due: &RoutineDue) -> u64 {
    match due.task.limit_micros {
        0 => spec.run_micros,
        n => n,
    }
}

/// Decide a due routine's fire (see the module's doc).
pub fn plan(env: &Env<'_>, spec: &RoutineSpec, due: &RoutineDue) -> Result<Planned> {
    plan_as(env, spec, due, false)
}

/// `plan` for "Try it now" (theseus-52tut): occurrence 0, run while the
/// owner watches. It is run 0 in the log and does not count among the
/// routine's runs, so the first scheduled run is still run 1.
pub fn plan_trial(env: &Env<'_>, spec: &RoutineSpec, due: &RoutineDue) -> Result<Planned> {
    plan_as(env, spec, due, true)
}

fn plan_as(env: &Env<'_>, spec: &RoutineSpec, due: &RoutineDue, trial: bool) -> Result<Planned> {
    // The stop is a fact the daemon keeps, not a memory: once it is used up
    // the series runs nothing, whether or not anything is awake to notice
    // (theseus-97b8c). The occurrence that finds it spent records the time and
    // is skipped `stopped`; the core then holds the series, so there is one
    // such skip and not one an occurrence for ever. A run asked for by hand
    // still runs: the stop ends the schedule, not the owner's own verb.
    if let Some(stop) = spec.stop.as_ref().filter(|_| !trial && !due.manual) {
        if stop_spent(stop, spec.runs, due.now_ms) {
            let mut next = spec.clone();
            next.stopped_ms = Some(next.stopped_ms.unwrap_or(due.now_ms));
            return skipped("stopped", None, next);
        }
    }
    let month = month_key(&env.kernel.config().zone, due.now_ms);
    let reset = spec.month != month;
    match held_back(env, spec, due, reset)? {
        Some(skip) => Ok(skip),
        None => run_plan(env, spec, due, reset.then_some(month), trial),
    }
}

/// The reasons not to run: after downtime with no catch-up, a run still going,
/// and a spent month. `None`: run it. A run by hand is refused in words instead.
fn held_back(
    env: &Env<'_>,
    spec: &RoutineSpec,
    due: &RoutineDue,
    reset: bool,
) -> Result<Option<Planned>> {
    let f = &due.fired;
    // After downtime (or a long wait for the scan): one catch-up run, or none.
    if !due.manual && (f.while_down || f.missed > 0) && !catches_up(spec) {
        return skipped("catch_up", None, spec.clone()).map(Some);
    }
    // A routine never runs twice at once.
    if let Some(t) = match &spec.last_task {
        Some(last) => env.kernel.execution(last)?,
        None => None,
    }
    .filter(|t| !t.state.is_terminal())
    {
        if due.manual {
            bail!(
                "{} has a run going ({}): a routine never runs twice at once",
                spec.title,
                crate::task::short(&t.session_id)
            );
        }
        return skipped("overlap", None, spec.clone()).map(Some);
    }
    // The month's money: a run is carved whole from what the month has left.
    let want = wanted(spec, due);
    let avail = match reset {
        true => due.available_after_reset_micros,
        false => due.available_micros,
    };
    if avail >= want {
        return Ok(None);
    }
    if due.manual {
        bail!(
            "{}'s month has ${:.2} left and a run is carved at ${:.2}: raise the month with \
             `routine update`",
            spec.title,
            micros_to_usd(avail),
            micros_to_usd(want)
        );
    }
    let mut next = spec.clone();
    let tell = !std::mem::replace(&mut next.month_told, true);
    let post = env.target.clone().filter(|_| tell).map(|target| Post {
        session_id: spec.session_id.clone(),
        execution_id: spec.execution_id.clone(),
        target,
        body: json!({"kind": "notice", "text": month_spent_text(
            &spec.title, micros_to_usd(spec.month_micros), &short(&spec.session_id))}),
        retry_class: RetryClass::IdempotentWithKey {
            key: NONCE_KEY.into(),
        },
    });
    skipped("budget", post, next).map(Some)
}

/// A routine's month spent (theseus-x8b9k).
pub(crate) fn month_spent_text(title: &str, month_usd: f64, id: &str) -> String {
    crate::owner_note::note(
        crate::owner_note::Kind::Action,
        &format!("{title}: runs are skipped until the 1st"),
        &format!("this month's ${month_usd:.2} is spent"),
        &format!("Raise it with `theseus routine update {id} --month <usd>`."),
    )
}

/// The run: its task's session, brief, binding and mark, and the routine's record.
/// `new_month` is the month the spend starts again in, when it turned.
fn run_plan(
    env: &Env<'_>,
    spec: &RoutineSpec,
    due: &RoutineDue,
    new_month: Option<String>,
    trial: bool,
) -> Result<Planned> {
    let mut next = spec.clone();
    let reset_month = new_month.is_some();
    if let Some(month) = new_month {
        next.month = month;
        next.month_told = false;
    }
    if !trial {
        next.runs += 1;
    }
    next.last_task = Some(due.task_id.clone());
    let run_no = if trial { 0 } else { next.runs };
    let mut brief = spec.brief.clone();
    if spec.speak == Speak::WhenNotable {
        brief.push_str(&format!(
            "\n\nIf there is nothing new to report, end your report with exactly {NOTHING_NEW} on \
             a line of its own."
        ));
    }
    let text = format!(
        "{}\n\n{}",
        header(env, spec, due, run_no)?,
        crate::task::brief_text(&due.session_id, &spec.session_id, &brief)
    );
    let mut rec = SessionRecord::with_id(
        due.session_id.clone(),
        SessionKind::Task,
        Some("task".into()),
    );
    rec.title = Some(format!("{} #{run_no}", spec.title));
    rec.execution_id = Some(due.task_id.clone());
    // A run carries the labels of the session that made the routine
    // (`created_in`; `spec.session_id` is the routine's own, which reads nothing),
    // as that session has them now (theseus-gyjyo.5).
    rec.data = spec
        .created_in
        .as_deref()
        .map(|s| env.store.get_session::<SessionRecord>(s))
        .transpose()?
        .flatten()
        .and_then(|r| r.data)
        .and_then(|f| f.inherited());
    rec.task = Some(TaskOf {
        parent_session: spec.session_id.clone(),
        parent_execution: spec.execution_id.clone(),
        by: due.fired.wake.id.clone(),
        target: env.target.clone(),
        arrangement: None,
        check: None,
        notify_done: false,
    });
    let author = format!("session:{}", spec.session_id);
    let node = Node::relayed(&due.session_id, None, Origin::Agent, &author, &text);
    let mut records = vec![
        NewRecord::json(kinds::SESSION, Some(&rec.session_id), &rec)?,
        node.record()?,
        NewRecord::json(
            kinds::META,
            Some(&format!("{RUN_PREFIX}{}", due.session_id)),
            &RunMark {
                routine: spec.session_id.clone(),
                run: run_no,
                speak: spec.speak,
            },
        )?,
    ];
    if let Some(t) = &env.target {
        records.push(env.outbox.task_record(&due.session_id, t)?);
    }
    let plan = RoutinePlan::Run(RunPlan {
        want_micros: wanted(spec, due),
        reset_month,
        reports_to: env.target.clone(),
        records,
        extra: vec![spec_record(&next)?],
    });
    Ok((plan, next))
}

/// `Run 22 of Morning digest, due 08:00, started 08:00:00.3 (2 missed while
/// the daemon was down); the last run reported: <first line>`
fn header(env: &Env<'_>, spec: &RoutineSpec, due: &RoutineDue, run_no: u32) -> Result<String> {
    let zone = &env.kernel.config().zone;
    let f = &due.fired;
    let mut out = format!(
        "Run {run_no} of {}, due {}, started {}",
        spec.title,
        hm(zone, f.wake.due_at_ms),
        hms_tenths(zone, due.now_ms)
    );
    if run_no == 0 {
        out.push_str(" (a trial: the owner is watching)");
    }
    out.push_str(&match (f.missed, f.while_down, due.manual) {
        (0, _, true) => " (run by hand)".to_string(),
        (0, true, _) => " (late: the daemon was down at the due time)".to_string(),
        (n, true, _) => format!(" ({n} missed while the daemon was down)"),
        (n, false, _) if n > 0 => format!(" ({n} missed)"),
        _ => String::new(),
    });
    if let Some(last) = &spec.last_task {
        if let Some(r) = run_row(env, last)? {
            out.push_str(&match r.report {
                Some(line) if r.state == "complete" => {
                    format!("; the last run reported: {line}")
                }
                Some(line) => format!("; the last run ended: {line}"),
                None => format!("; the last run is {}", r.state),
            });
            if r.state == "complete" {
                out.push_str(&carried(env, spec, last)?);
            }
        }
    }
    Ok(out)
}

/// What the last run saw, for the run that follows it (theseus-97b8c): the
/// whole of its report, clipped, and the one line that says what to do with it.
/// This is what makes a routine that watches something different from one that
/// runs the same brief on a timer: run N reports the change, not the state.
/// The daemon reads it from the run's own report; nothing is re-derived, and a
/// report whose whole text is the first line the clause already carries adds
/// nothing, so it is left out.
fn carried(env: &Env<'_>, spec: &RoutineSpec, last_task: &str) -> Result<String> {
    if spec.carry == Carry::None {
        return Ok(String::new());
    }
    let Some(text) = crate::task::load_report(env.store, env.kernel, last_task)?
        .filter(|r| r.outcome == "complete")
        .and_then(|r| r.text)
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty() && *t != first_line(t))
    else {
        return Ok(String::new());
    };
    Ok(format!(
        "\n\nWhat the last run reported, in full, so that you can report what has changed \
         since rather than saying it again:\n---\n{}\n---",
        clip(&text, MAX_CARRY)
    ))
}

/// A text at most `max` characters, its head and its tail kept.
fn clip(text: &str, max: usize) -> String {
    let n = text.chars().count();
    if n <= max {
        return text.to_string();
    }
    let head = max * 2 / 3;
    let from = (n - max / 3).max(head);
    let out: String = text.chars().take(head).collect();
    let tail: String = text.chars().skip(from).collect();
    format!("{out}\n[… {} characters left out …]\n{tail}", from - head)
}

/// Whether a finished run's report is not posted: its routine speaks only when
/// notable and the report ends with the marker.
pub fn quiet(store: &Store, task_session: &str, report: Option<&str>) -> bool {
    let Some(text) = report else {
        return false;
    };
    let mark: Option<RunMark> = store
        .get_meta(&format!("{RUN_PREFIX}{task_session}"))
        .ok()
        .flatten();
    mark.is_some_and(|m| m.speak == Speak::WhenNotable && text.trim_end().ends_with(NOTHING_NEW))
}

/// The commitments block's lines for the routines a conversation made:
/// `🔁 a41d04 weekdays 08:00 Morning digest (next Fri)`.
pub fn lines(kernel: &Kernel, specs: &[Arc<RoutineSpec>]) -> Vec<String> {
    let zone = &kernel.config().zone;
    specs
        .iter()
        .filter_map(|s| {
            let e = kernel.execution(&s.execution_id).ok().flatten()?;
            if e.state.is_terminal() {
                return None;
            }
            let w = e.wakes.iter().find(|w| w.purpose == Purpose::Routine)?;
            let series = w
                .repeat
                .as_ref()
                .map(|r| describe(r, zone))
                .unwrap_or_else(|| s.schedule.clone());
            let day = local(zone, w.due_at_ms, "%a");
            let paused = if w.paused_at_ms.is_some() {
                " (paused)"
            } else {
                ""
            };
            Some(format!(
                "🔁 {} {series} {} (next {day}){paused}",
                short(&s.session_id),
                s.title
            ))
        })
        .collect()
}
