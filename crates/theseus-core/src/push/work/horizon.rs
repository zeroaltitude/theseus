//! The future on the work board (theseus-nuhsz; horizon HZ8): a reminder, a
//! routine, a watch and a deferred task are views with a time (`next_at_ms`),
//! a series, and, for a routine and a watch, what only they have. Nothing
//! here is stored and nothing waits on a timer: a view is built from the
//! wake (`PendingWake`, the execution's own record), a routine's record
//! (the META `routine:<session>` the frame carries), and the rows a frame
//! carries (`wake.fired`, `wake.skipped`, `watch.*`).
//!
//! - **A routine** is the wake of purpose `Routine`: `Ready` between runs,
//!   `Running` while a run's task is open (the run is its child), `NeedsYou`
//!   after a failed or blocked last run until it runs again, is paused, or is
//!   deleted, `Waiting` while paused.
//! - **A watch** is the wake of purpose `Watch`: it waits, and ends `Done`
//!   (met) or `Expired` by the rows that say so.
//! - **The agenda** expands every open series in a window by the kernel's own
//!   arithmetic (`Repeat::next_after`), and skips what is paused.
//!
//! Limits, said: a routine's 30-day counts read the ended runs the board
//! still holds (the 50 newest ended children of a root stay, `ENDED_KEPT`),
//! its skipped runs the `wake.skipped` rows this board has seen since it
//! began, and a watch's last value the rows since then (or the state the
//! seed read); a watch's next check lives in the checker's memory and is not
//! on the view (its `next_at_ms` is the time it gives up).

use std::collections::VecDeque;

use theseus_kernel::{PendingWake, Purpose, Repeat, TimeZone, WakeRun};
use theseus_protocol::routines::RoutineSpec;
use theseus_protocol::work::horizon::{
    AgendaLine, AgendaParams, AgendaResult, AgendaWhat, Month, RoutineLast, RoutineView, RunCounts,
    Series, WatchLast, WatchView, AGENDA_MAX,
};
use theseus_protocol::work::{Doing, DoingKind, WorkKind, WorkReport, WorkState, WorkView};
use theseus_protocol::WaitingOn;

use super::views::line;
use super::{ExecIn, Scope, WorkBoard};
use crate::ledger::LedgerRow;

/// The skipped runs kept for a routine's 30-day count.
const SKIPS_KEPT: usize = 64;

/// 30 days.
const WINDOW_30D: u64 = 30 * 86_400_000;

/// What the board holds of a routine besides its wake.
#[derive(Debug, Clone, Default)]
pub(super) struct RoutineIn {
    pub spec: Option<RoutineSpec>,
    /// How many occurrences the last firing passed over.
    pub missed_last: u32,
    /// When runs were skipped (newest last).
    pub skipped: VecDeque<u64>,
}

/// What the board holds of a watch.
#[derive(Debug, Clone, Default)]
pub(super) struct WatchIn {
    pub value: Option<String>,
    pub at_ms: u64,
    pub checks: u64,
    pub failing: Option<String>,
    /// `met` or `expired`, once a row said so.
    pub end: Option<&'static str>,
}

/// Whether a view belongs to the future: dated, or a routine, a watch or a
/// project.
pub(super) fn is_future(v: &WorkView) -> bool {
    matches!(
        v.kind,
        WorkKind::Routine | WorkKind::Watch | WorkKind::Project
    ) || v.next_at_ms.is_some()
}

/// A wake's view kind by its purpose.
pub(super) fn kind_of(w: &PendingWake) -> WorkKind {
    match w.purpose {
        Purpose::Routine => WorkKind::Routine,
        Purpose::Watch => WorkKind::Watch,
        _ => WorkKind::Wake,
    }
}

/// A repeating wake's series as a view shows it.
pub(super) fn series_of(w: &PendingWake, missed_last: u32) -> Option<Series> {
    let r = w.repeat.as_ref()?;
    Some(Series {
        every: r.cron.is_none().then(|| r.every.to_string()),
        cron: r.cron.clone(),
        days: r.days.iter().map(|d| d.as_str().to_string()).collect(),
        until_ms: r.until_ms,
        occurrence: w.occurrence,
        missed_last,
    })
}

impl WorkBoard {
    // ----------------------------------------------------------- inputs

    /// A routine's record, as a frame carried it (a META payload that reads
    /// as one; any other payload is not).
    pub(super) fn take_routine_spec(&mut self, spec: RoutineSpec, d: &mut super::Dirty) {
        let wake = spec.wake_id.clone();
        if spec.deleted_ms.is_some() {
            self.routines.remove(&wake);
        } else {
            self.routines.entry(wake.clone()).or_default().spec = Some(spec);
        }
        d.wakes.insert(wake);
    }

    /// The rows about routines and watches.
    pub(super) fn take_horizon_row(&mut self, r: &LedgerRow, d: &mut super::Dirty) {
        let Some(wake) = r.data["wake_id"].as_str().map(str::to_string) else {
            return;
        };
        match r.kind.as_str() {
            "wake.fired" if r.data["purpose"].as_str() == Some("routine") => {
                let missed = r.data["missed"].as_u64().unwrap_or(0) as u32;
                self.routines.entry(wake.clone()).or_default().missed_last = missed;
                d.wakes.insert(wake);
            }
            // An owner's own `routine.skip` opens the ledger row on the same kind a skip for
            // cause does (overlap, a spent month, catching up), but it is not one: it does not
            // belong in the 30-day tally that reads as the routine's reliability (theseus-0oldh,
            // review L-1).
            "wake.skipped"
                if r.data["purpose"].as_str() == Some("routine")
                    && r.data["why"].as_str() != Some("owner") =>
            {
                let ri = self.routines.entry(wake.clone()).or_default();
                if ri.skipped.len() == SKIPS_KEPT {
                    ri.skipped.pop_front();
                }
                ri.skipped.push_back(r.at_unix_ms);
                d.wakes.insert(wake);
            }
            "watch.checked" | "watch.met" | "watch.expired" | "watch.failing" => {
                let wi = self.watches.entry(wake.clone()).or_default();
                wi.at_ms = r.at_unix_ms;
                wi.checks = r.data["checks"].as_u64().unwrap_or(wi.checks);
                if let Some(v) = r.data["value"].as_str() {
                    wi.value = Some(v.to_string());
                }
                wi.failing = r.data["error"].as_str().map(str::to_string);
                wi.end = match r.kind.as_str() {
                    "watch.met" => Some("met"),
                    "watch.expired" => Some("expired"),
                    _ => wi.end,
                };
                d.wakes.insert(wake);
            }
            _ => {}
        }
    }

    /// The routine wake an execution's run belongs to: its parent's.
    pub(super) fn routine_wake_of(&self, ei: &ExecIn) -> Option<&PendingWake> {
        let parent = self.execs.get(ei.parent.as_ref()?)?;
        parent.wakes.iter().find(|w| w.purpose == Purpose::Routine)
    }

    /// Mark the routine a run belongs to for building again.
    pub(super) fn touch_routine(&self, exec: &str, d: &mut super::Dirty) {
        if let Some(w) = self.execs.get(exec).and_then(|e| self.routine_wake_of(e)) {
            d.wakes.insert(w.id.clone());
        }
    }

    // ------------------------------------------------------------ views

    /// What a pending wake adds to its view: its purpose, time, series, and
    /// for a routine or a watch their own parts. The state and the title of a
    /// routine are its, not the generic wake's.
    pub(super) fn decorate_wake(&self, v: &mut WorkView, ei: &ExecIn, w: &PendingWake) {
        v.kind = kind_of(w);
        v.purpose = Some(w.purpose.as_str().to_string());
        v.next_at_ms = Some(w.due_at_ms);
        let missed = self.routines.get(&w.id).map_or(0, |r| r.missed_last);
        v.series = series_of(w, missed);
        v.paused = w.paused_at_ms.is_some();
        match w.purpose {
            Purpose::Routine => self.routine_part(v, ei, w),
            Purpose::Watch => self.watch_part(v, w),
            _ => {}
        }
    }

    fn routine_part(&self, v: &mut WorkView, ei: &ExecIn, w: &PendingWake) {
        let held = self.routines.get(&w.id);
        let spec = held.and_then(|r| r.spec.as_ref());
        if let Some(s) = spec {
            v.title = line(&s.title);
        }
        let runs: Vec<&WorkView> = self
            .kids
            .get(&w.id)
            .into_iter()
            .flatten()
            .filter_map(|k| self.views.get(k))
            .filter(|k| k.kind == WorkKind::Task)
            .collect();
        let running = runs.iter().any(|r| !r.state.ended());
        let last = runs.iter().max_by_key(|r| (r.started_at_ms, r.id.clone()));
        let failed_last = !running
            && last.is_some_and(|l| matches!(l.state, WorkState::Failed | WorkState::NeedsYou));
        let paused = w.paused_at_ms.is_some();
        v.state = if paused {
            WorkState::Waiting
        } else if running {
            WorkState::Running
        } else if failed_last {
            WorkState::NeedsYou
        } else {
            WorkState::Ready
        };
        let last_view = last.map(|l| RoutineLast {
            outcome: match l.state {
                WorkState::Done => "ok",
                WorkState::Failed => "failed",
                WorkState::NeedsYou => "blocked",
                WorkState::Cancelled | WorkState::Expired => "cancelled",
                _ => "running",
            }
            .to_string(),
            at_ms: l.ended_at_ms.unwrap_or(l.started_at_ms),
            duration_ms: l
                .ended_at_ms
                .unwrap_or(self.now_ms)
                .saturating_sub(l.started_at_ms),
            cost_usd: l.cost.map_or(0.0, |c| c.spent_usd),
            line: l
                .report
                .as_ref()
                .and_then(|r| r.first_line.clone().or_else(|| r.reason.clone())),
        });
        if failed_last {
            v.report = Some(WorkReport {
                outcome: "failed".into(),
                first_line: None,
                reason: last_view.as_ref().and_then(|l| l.line.clone()),
            });
        }
        let since = self.now_ms.saturating_sub(WINDOW_30D);
        let mut counts = RunCounts::default();
        for r in runs.iter().filter(|r| r.ended_at_ms.unwrap_or(0) >= since) {
            match r.state {
                WorkState::Done => counts.ok += 1,
                WorkState::Failed | WorkState::NeedsYou => counts.failed += 1,
                _ => {}
            }
        }
        counts.skipped =
            held.map_or(0, |h| h.skipped.iter().filter(|t| **t >= since).count()) as u32;
        v.doing = Some(if running {
            Doing {
                what: DoingKind::Job,
                label: "running".into(),
                since_ms: last.map_or(0, |l| l.started_at_ms),
                turn: 0,
                loop_index: 0,
                last_line: None,
            }
        } else {
            Doing {
                what: DoingKind::Sleeping,
                label: if paused {
                    "paused".into()
                } else {
                    "ready".into()
                },
                since_ms: w.set_at_ms,
                turn: 0,
                loop_index: 0,
                last_line: None,
            }
        });
        v.routine = Some(RoutineView {
            schedule: spec.map_or_else(|| line(&w.note), |s| s.schedule.clone()),
            last: last_view,
            runs_30d: counts,
            month: Month {
                spent_usd: ei.x.spent_usd,
                limit_usd: spec.map_or(ei.x.limit_usd, |s| s.month_micros as f64 / 1e6),
            },
            deliver_to: spec.map_or_else(|| "dm".to_string(), |s| s.deliver_to.clone()),
        });
    }

    pub(super) fn watch_part(&self, v: &mut WorkView, w: &PendingWake) {
        let held = self.watches.get(&w.id);
        let spec = crate::watch::spec::Spec::of(w).ok();
        let (check, until, expires) = match &spec {
            Some(s) => (s.check.target(), s.until.clone(), Some(s.expires_ms)),
            None => (String::new(), String::new(), Some(w.due_at_ms)),
        };
        v.state = match held.and_then(|h| h.end) {
            Some("met") => WorkState::Done,
            Some(_) => WorkState::Expired,
            None => WorkState::Waiting,
        };
        let failing = held.and_then(|h| h.failing.clone());
        v.doing = Some(Doing {
            what: DoingKind::Sleeping,
            label: match (&failing, &spec) {
                (Some(f), _) => format!("failing: {}", line(f)),
                (None, Some(s)) if matches!(s.check, crate::watch::spec::Check::Mail { .. }) => {
                    let day = s.follow_up.as_ref().map(|_| {
                        let d = crate::recall::stamp::day_in(
                            s.expires_ms,
                            &crate::recall::stamp::system,
                        );
                        d.chars().take(3).collect::<String>()
                    });
                    theseus_protocol::work::horizon::reply_wait(
                        &s.title,
                        theseus_protocol::now_unix_ms().saturating_sub(w.set_at_ms),
                        day.as_deref(),
                    )
                }
                (None, Some(s)) => format!(
                    "checking every {}",
                    crate::watch::spec::every_word(s.every_ms)
                ),
                (None, None) => "watching".into(),
            },
            since_ms: w.set_at_ms,
            turn: 0,
            loop_index: 0,
            last_line: None,
        });
        v.watch = Some(WatchView {
            check,
            until,
            last: held.and_then(|h| {
                h.value.clone().map(|value| WatchLast {
                    value,
                    at_ms: h.at_ms,
                })
            }),
            checks: held.map_or(0, |h| h.checks),
            expires_at_ms: expires,
            failing,
        });
        if v.state.ended() {
            v.ended_at_ms = held.map(|h| h.at_ms);
        }
    }

    /// A deferred task asleep until its start: its time.
    pub(super) fn decorate_exec(v: &mut WorkView, ei: &ExecIn) {
        if let Some(WaitingOn::DueAt { at_ms }) = &ei.x.waiting_on {
            if *at_ms < theseus_protocol::task_pause::PAUSED_AT_MS {
                v.next_at_ms = Some(*at_ms);
            }
        }
    }

    // ------------------------------------------------------- the future

    /// The next reminder due, and the routines whose last run failed: from
    /// the small set of future views, never the whole board.
    pub(in crate::push) fn future_stats(&self) -> (Option<u64>, u32) {
        let mut next: Option<u64> = None;
        let mut failed = 0;
        for v in self.futures.iter().filter_map(|id| self.views.get(id)) {
            if v.kind == WorkKind::Routine && v.state == WorkState::NeedsYou {
                failed += 1;
            }
            if v.purpose.as_deref() == Some("remind") && !v.paused && !v.state.ended() {
                next = match (next, v.next_at_ms) {
                    (Some(n), Some(t)) => Some(n.min(t)),
                    (n, t) => n.or(t),
                };
            }
        }
        (next, failed)
    }

    /// The views a client keeps beside the tree: what is dated, a series, a
    /// routine, a watch or a project, not over, whatever root it is under.
    pub(super) fn future(&self, scope: &Scope) -> Vec<&WorkView> {
        let mut out: Vec<&WorkView> = self
            .futures
            .iter()
            .filter_map(|id| self.views.get(id))
            .filter(|v| !v.state.ended() || v.attention.level == theseus_protocol::Level::NeedsYou)
            .filter(|v| scope.sees(&v.root))
            .collect();
        out.sort_by_key(|v| (v.next_at_ms.unwrap_or(u64::MAX), v.id.clone()));
        out
    }

    /// `work.agenda`: every occurrence in `[from, to)`, soonest first, at
    /// most `AGENDA_MAX`. A series is expanded by the kernel's own
    /// arithmetic in `zone`, lazily: the streams are merged by time and only
    /// as far as the cap, so a minute series costs 200 steps, not a window's
    /// worth. A paused wake has no occurrences.
    pub(super) fn agenda(&self, p: &AgendaParams, scope: &Scope, zone: &TimeZone) -> AgendaResult {
        let mut streams: Vec<(AgendaLine, Occurrences)> = Vec::new();
        let mut starts: Vec<AgendaLine> = Vec::new();
        for ei in self.execs.values() {
            if !scope.sees(&self.root_of(&ei.x.execution_id)) {
                continue;
            }
            for w in ei.wakes.iter().filter(|w| w.paused_at_ms.is_none()) {
                let title = match w.purpose {
                    Purpose::Routine => self
                        .routines
                        .get(&w.id)
                        .and_then(|r| r.spec.as_ref())
                        .map_or_else(|| line(&w.note), |s| line(&s.title)),
                    _ => line(&w.note),
                };
                let end_n = (w.purpose == Purpose::Routine)
                    .then(|| self.routines.get(&w.id).and_then(|r| r.spec.as_ref()))
                    .flatten()
                    .and_then(runs_left)
                    .map(|left| w.occurrence.max(1).saturating_add(left));
                let mut occ = Occurrences::of(w, p, zone, end_n);
                if let Some((at_ms, occurrence)) = occ.next() {
                    streams.push((head_of(w, &ei.x.session_id, title, at_ms, occurrence), occ));
                }
            }
            if let Some(WaitingOn::DueAt { at_ms }) = &ei.x.waiting_on {
                if (p.from_ms..p.to_ms).contains(at_ms)
                    && *at_ms < theseus_protocol::task_pause::PAUSED_AT_MS
                    && ei.kind == WorkKind::Task
                {
                    starts.push(AgendaLine {
                        at_ms: *at_ms,
                        id: ei.x.execution_id.clone(),
                        kind: WorkKind::Task,
                        what: AgendaWhat::Starts,
                        title: ei.title.clone(),
                        session_id: Some(ei.x.session_id.clone()),
                        purpose: None,
                        occurrence: None,
                    });
                }
            }
        }
        let (lines, more) = merged(streams, starts);
        AgendaResult {
            from_ms: p.from_ms,
            to_ms: p.to_ms,
            lines,
            more,
        }
    }
}

/// How many runs a routine's run-count stop has left, when it has one: the
/// daemon counts the runs it opened and skips the occurrence past the count, so
/// the agenda serves no more than this many (theseus-97b8c, review H-4).
fn runs_left(spec: &RoutineSpec) -> Option<u32> {
    let after = spec.stop.as_ref()?.after_runs?;
    Some(after.saturating_sub(spec.runs))
}

/// A stream's line at one of its times.
fn head_of(
    w: &PendingWake,
    session: &str,
    title: String,
    at_ms: u64,
    n: Option<u32>,
) -> AgendaLine {
    AgendaLine {
        at_ms,
        id: w.id.clone(),
        kind: kind_of(w),
        what: match w.purpose {
            Purpose::Routine => AgendaWhat::Run,
            Purpose::Watch => AgendaWhat::Expires,
            _ => AgendaWhat::Due,
        },
        title,
        session_id: Some(session.to_string()),
        purpose: Some(w.purpose.as_str().to_string()),
        occurrence: n,
    }
}

/// The first `AGENDA_MAX` lines of several time-ordered streams, soonest
/// first (ties by id), and whether more were left.
fn merged(
    mut streams: Vec<(AgendaLine, Occurrences)>,
    starts: Vec<AgendaLine>,
) -> (Vec<AgendaLine>, bool) {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    // Each stream's current line is its head; the heap orders the heads.
    let mut heap: BinaryHeap<Reverse<(u64, String, u32, usize)>> = BinaryHeap::new();
    for (i, (h, _)) in streams.iter().enumerate() {
        heap.push(Reverse((
            h.at_ms,
            h.id.clone(),
            h.occurrence.unwrap_or(0),
            i,
        )));
    }
    let mut starts: Vec<AgendaLine> = starts;
    starts.sort_by(|a, b| (b.at_ms, &b.id).cmp(&(a.at_ms, &a.id)));
    let mut out: Vec<AgendaLine> = Vec::new();
    loop {
        let from_streams = heap.peek().map(|Reverse((t, id, _, _))| (*t, id.clone()));
        let from_starts = starts.last().map(|l| (l.at_ms, l.id.clone()));
        let take_start = match (&from_streams, &from_starts) {
            (None, None) => break,
            (Some(a), Some(b)) => b < a,
            (None, Some(_)) => true,
            (Some(_), None) => false,
        };
        if out.len() == AGENDA_MAX {
            return (out, true);
        }
        if take_start {
            out.extend(starts.pop());
            continue;
        }
        let Some(Reverse((_, _, _, i))) = heap.pop() else {
            break;
        };
        let (head, occ) = &mut streams[i];
        out.push(head.clone());
        if let Some((at_ms, n)) = occ.next() {
            head.at_ms = at_ms;
            head.occurrence = n;
            heap.push(Reverse((at_ms, head.id.clone(), n.unwrap_or(0), i)));
        }
    }
    (out, false)
}

/// A wake's times in a window, soonest first: one for a wake that does not
/// repeat, a watch's expiry, or a series' occurrences, by the kernel's
/// `next_after` in the zone.
struct Occurrences {
    kind: OccKind,
}

enum OccKind {
    None,
    One(Option<(u64, Option<u32>)>),
    Series {
        repeat: Repeat,
        zone: TimeZone,
        at: u64,
        n: u32,
        to: u64,
        /// The occurrence number a run-count stop ends the series before: a
        /// routine's occurrences past it will never run (theseus-97b8c).
        end_n: Option<u32>,
    },
}

impl Occurrences {
    /// `end_n`: for a routine whose stop is a run count, the number of the
    /// occurrence the stop ends the series before (see [`runs_left`]).
    fn of(w: &PendingWake, p: &AgendaParams, zone: &TimeZone, end_n: Option<u32>) -> Self {
        let inside = |t: u64| (p.from_ms..p.to_ms).contains(&t);
        let kind = if w.purpose == Purpose::Watch {
            // A watch is not a series of events: it gives up once.
            let at = w
                .repeat
                .as_ref()
                .and_then(|r| r.until_ms)
                .unwrap_or(w.due_at_ms);
            OccKind::One(inside(at).then_some((at, None)))
        } else if !matches!(w.run, WakeRun::Turn | WakeRun::Notice | WakeRun::Task(_)) {
            OccKind::None
        } else if let Some(r) = &w.repeat {
            let mut at = w.due_at_ms;
            let mut n = w.occurrence.max(1);
            let mut live = true;
            // Jump to the window instead of walking up to it.
            if at < p.from_ms && p.from_ms > 0 {
                n = n.saturating_add(r.between(zone, at, p.from_ms - 1) as u32 + 1);
                match r.next_after(zone, p.from_ms - 1) {
                    Some(t) => at = t,
                    None => live = false,
                }
            }
            if live && at < p.to_ms {
                OccKind::Series {
                    repeat: r.clone(),
                    zone: zone.clone(),
                    at,
                    n,
                    to: p.to_ms,
                    end_n,
                }
            } else {
                OccKind::None
            }
        } else {
            OccKind::One(inside(w.due_at_ms).then_some((w.due_at_ms, None)))
        };
        Self { kind }
    }
}

impl Iterator for Occurrences {
    type Item = (u64, Option<u32>);

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.kind {
            OccKind::None => None,
            OccKind::One(o) => o.take(),
            OccKind::Series {
                repeat,
                zone,
                at,
                n,
                to,
                end_n,
            } => {
                if *at >= *to || end_n.is_some_and(|e| *n >= e) {
                    return None;
                }
                let out = (*at, Some(*n));
                match repeat.next_after(zone, *at) {
                    Some(next) if next > *at => {
                        *at = next;
                        *n += 1;
                    }
                    _ => *at = u64::MAX,
                }
                Some(out)
            }
        }
    }
}

/// A wake's agenda lines in a window as (time, occurrence), and whether the
/// cap cut them, for the tests beside this module. `end_n` is the run-count
/// stop's end, as the production call passes it (`None`: no run-count stop).
#[cfg(test)]
pub(super) fn lines_for_tests(
    w: &PendingWake,
    p: &AgendaParams,
    zone: &TimeZone,
    end_n: Option<u32>,
) -> (Vec<(u64, u32)>, bool) {
    let occ = Occurrences::of(w, p, zone, end_n);
    let (mut out, mut more) = (Vec::new(), false);
    for (at, n) in occ {
        if out.len() == AGENDA_MAX {
            more = true;
            break;
        }
        out.push((at, n.unwrap_or(0)));
    }
    (out, more)
}
