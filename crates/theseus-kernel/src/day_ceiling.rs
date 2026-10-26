//! The daemon's daily spend ceiling (theseus-kp20): one backstop over every
//! model call the daemon makes, by the owner's local day, `[kernel]
//! daily_spend_ceiling_usd` ($200 by default). It is not a budget: a
//! session's limit and its mode, a place's ceiling, an MCP client's limit,
//! AWS budgets, and the background day caps all keep their meaning, and this
//! sits above them, high enough never to touch normal work, low enough that
//! a loop spinning overnight stops.
//!
//! **Two modes** (theseus-9e02; the owner, 2026-10-11: tokens are less
//! cautious, AWS more): under `stop` (`[kernel] daily_spend_ceiling_mode`),
//! reaching the ceiling refuses calls, as above. Under `notify`, the default,
//! reaching it refuses nothing: the ceiling is a warning, at the ceiling and
//! at each `daily_notice_step_usd` after it, once each a day
//! ([`DayCeiling::take_warning`]), and what stops the day is the runaway line
//! (`daily_runaway_usd`, $2,000; 0 never). The refusal rules below then apply
//! to that line: [`DayCeiling::bar`] is the amount a call may not pass.
//!
//! **Sticky** (theseus-l9on, settled from a review): once a call is refused
//! for the day, every later call that day is refused too, even a smaller one
//! that would fit (a backstop does not let a smaller call slip past), until
//! local midnight or a higher bar (`[kernel] daily_spend_ceiling_usd` under
//! stop, `daily_runaway_usd` under notify, read at the daemon's next start). A refused turn or task ends; nothing
//! waits for the day to turn.
//!
//! - **One counter, in memory.** Today's day, what today settled and booked
//!   (`spent`), and what calls in flight hold (`held`), under one small lock,
//!   read and written by no store. A provider call's reservation holds its
//!   amount from its plan until its settle, as the kernel's own budget does,
//!   so two calls racing at the edge cannot both pass on the same headroom.
//!   The background calls (the judge, consolidation, the learning runs) hold
//!   through a [`Hold`] the same way, and a cost paid after its call (speech,
//!   a transcript) is booked as it lands, in full, even past the ceiling: a
//!   real cost is never hidden.
//! - **The day** is the kernel's zone's (`KernelConfig::zone`, the system's),
//!   from local midnight to the next. The first hold, booking, or read of a
//!   new day starts its total at zero. A hold made yesterday and settled
//!   today is counted today, at its settle: the day a cost lands in is the
//!   day its row is written in, which is what a restart reads back.
//! - **A restart** seeds the day once, before serving, from the ledger's
//!   rows of today (`seed`, read by the core), so it never resets the day.
//! - **Only model spend counts**: a provider call's reservation (never an AWS
//!   hand's), and what the core books for its own calls. A task's cost is its
//!   own call's, counted once in the day's total: carrying it to its
//!   parent's budget adds nothing to `spent`. The burn window is the other
//!   count (theseus-o1h3f.5): a settled cost fills the window of its
//!   execution and of each ancestor, so a fan-out's helpers, each a window
//!   of their own, still reach their parent's line together.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use jiff::tz::TimeZone;

use crate::kernel::{Kernel, KernelError};
use crate::types::*;

/// The default ceiling: $200 a day (the owner, 2026-10-08).
pub const DEFAULT_CEILING_MICROS: Micros = 200 * MICROS_PER_USD;

/// The default runaway line: $2,000 a day (the owner, 2026-10-11: "we've
/// occasionally spent upwards of 1k in a day on purpose [...] let's put
/// runaway at 2k but have continued notices every 200$").
pub const DEFAULT_RUNAWAY_MICROS: Micros = 2000 * MICROS_PER_USD;

/// The default warning step: a warning at the ceiling and every $200 after.
pub const DEFAULT_STEP_MICROS: Micros = 200 * MICROS_PER_USD;

/// The default burn-rate line: $200 a session in [`BURN_WINDOW_MS`].
pub const DEFAULT_BURN_MICROS: Micros = 200 * MICROS_PER_USD;

/// The burn-rate window: ten minutes.
pub const BURN_WINDOW_MS: u64 = 10 * 60 * 1000;

/// How many executions above one its settled cost reaches in their burn
/// windows: far more than the depth tasks allow (one), a bound for a cycle.
const BURN_ANCESTORS: usize = 8;

/// The counter. Shared by every view of a kernel.
pub struct DayCeiling {
    /// The ceiling, in micros. 0 turns it off (a test's kernel may).
    limit: AtomicU64,
    /// Under notify the ceiling only warns (`set_rules`).
    notify: AtomicBool,
    /// The runaway line, in micros: where a notifying day stops. 0 never.
    runaway: AtomicU64,
    /// The warning's step, in micros: a warning at the ceiling and at each
    /// step after it. 0 warns at the ceiling alone.
    step: AtomicU64,
    /// A session's burn-rate line over [`BURN_WINDOW_MS`]. 0 never.
    burn_line: AtomicU64,
    burn: Mutex<HashMap<String, VecDeque<(u64, Micros)>>>,
    zone: TimeZone,
    clock: Arc<dyn crate::clock::Clock>,
    st: Mutex<State>,
    next_hold: AtomicU64,
}

impl std::fmt::Debug for DayCeiling {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DayCeiling")
            .field("limit", &self.limit())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
struct State {
    /// `2026-10-08`; empty before the first read.
    day: String,
    start_ms: u64,
    end_ms: u64,
    spent: Micros,
    held: HashMap<String, Micros>,
    held_total: Micros,
    /// When the first call was refused today.
    reached_at_ms: Option<u64>,
    /// The ceiling it was refused under: every call is refused while the
    /// ceiling is no higher.
    reached_limit: Micros,
    /// Whether today's notice was taken (or read back at the start).
    noticed: bool,
    /// The highest warning level taken today (or read back): 1 at the
    /// ceiling, 2 at the ceiling and a step, and so on; 0 none.
    warned: u64,
}

/// A call the ceiling refused: nothing was made, and nothing written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reached {
    pub day: String,
    pub limit: Micros,
    pub spent: Micros,
    pub held: Micros,
    pub needed: Micros,
    /// When the day turns: the next local midnight.
    pub turns_at_ms: u64,
    /// Its local time, `2026-10-09 00:00`.
    pub turns_at: String,
    /// When an earlier call was refused today, which stopped this one
    /// whatever it needs (the stop is sticky): its local time.
    pub since: Option<String>,
    /// The line is the runaway line (the ceiling notifies), not the ceiling.
    pub runaway: bool,
}

/// What a refusal says of what comes next, for every refusal's words.
pub const REFUSAL_ENDS: &str = "a refused turn or task ends, it does not wait for the day to \
     turn; a higher `[kernel] daily_spend_ceiling_usd` applies at the daemon's next start";

/// What a runaway refusal says of what comes next.
pub const RUNAWAY_ENDS: &str = "a refused turn or task ends, it does not wait for the day to \
     turn; a higher `[kernel] daily_runaway_usd` applies at the daemon's next start";

impl std::fmt::Display for Reached {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.runaway {
            return self.runaway_words(f);
        }
        match &self.since {
            Some(since) => write!(
                f,
                "the {} daily ceiling stopped model calls at {} local time today ({} spent \
                 today), so this call was not made either, and none will be until the day \
                 turns at {} local time; {REFUSAL_ENDS}",
                usd(self.limit),
                since,
                usd(self.spent),
                self.turns_at
            ),
            None => write!(
                f,
                "today's spend reached the {} daily ceiling ({} spent today, {} held by calls \
                 in flight, and this call needs {}), so the call was not made, and no model \
                 call will be until the day turns at {} local time; {REFUSAL_ENDS}",
                usd(self.limit),
                usd(self.spent),
                usd(self.held),
                usd(self.needed),
                self.turns_at
            ),
        }
    }
}

impl Reached {
    /// A runaway stop's words (theseus-9e02): the day's spend is past the
    /// line where model spend stops, whatever the ceiling's warning said.
    fn runaway_words(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.since {
            Some(since) => write!(
                f,
                "the {} daily runaway line stopped model calls at {} local time today ({} spent \
                 today), so this call was not made either, and none will be until the day \
                 turns at {} local time; {RUNAWAY_ENDS}",
                usd(self.limit),
                since,
                usd(self.spent),
                self.turns_at
            ),
            None => write!(
                f,
                "today's spend reached the {} daily runaway line ({} spent today, {} held by \
                 calls in flight, and this call needs {}), so the call was not made, and no \
                 model call will be until the day turns at {} local time; {RUNAWAY_ENDS}",
                usd(self.limit),
                usd(self.spent),
                usd(self.held),
                usd(self.needed),
                self.turns_at
            ),
        }
    }
}

/// A burn-rate stop (theseus-9e02): a session's settled spend in the last
/// [`BURN_WINDOW_MS`] reached its line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BurnStop {
    pub spent: Micros,
    pub line: Micros,
}

impl std::fmt::Display for BurnStop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the session spent {} in the last {} minutes, its {} burn-rate line \
             (`[kernel] runaway_usd_per_10min`), so the call was not made and the session \
             waits for the owner to resume it",
            usd(self.spent),
            BURN_WINDOW_MS / 60_000,
            usd(self.line),
        )
    }
}

/// Today, as `budget.list` and health show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Today {
    pub day: String,
    /// The ceiling (a warning under notify, a stop under stop).
    pub limit: Micros,
    /// Under notify, the runaway line where the day stops (0 never); 0 under
    /// stop, where the ceiling is the line.
    pub runaway: Micros,
    pub notify: bool,
    pub step: Micros,
    pub spent: Micros,
    pub held: Micros,
    pub reached_at_ms: Option<u64>,
    pub turns_at_ms: u64,
    pub turns_at: String,
}

impl Today {
    /// The amount a call may not pass: the ceiling under stop, the runaway
    /// line under notify.
    pub fn bar(&self) -> Micros {
        if self.notify {
            self.runaway
        } else {
            self.limit
        }
    }

    /// Whether a call is refused now: one was today, or nothing is left.
    pub fn reached(&self) -> bool {
        self.reached_at_ms.is_some() || (self.bar() > 0 && self.spent >= self.bar())
    }

    /// Whether today's spend is at or past the ceiling, whatever the mode: a
    /// notifying day's warning. Under stop it is [`Today::reached`]'s.
    pub fn past(&self) -> bool {
        self.limit > 0 && self.spent >= self.limit
    }

    /// The warning level today's spend is at: 1 at the ceiling, 2 a step
    /// after it, and so on; 0 below the ceiling.
    pub fn level(&self) -> u64 {
        level_of(self.spent, self.limit, self.step)
    }
}

/// The warning level of `spent`: 1 at `limit`, +1 for each `step` after it.
pub fn level_of(spent: Micros, limit: Micros, step: Micros) -> u64 {
    if limit == 0 || spent < limit {
        return 0;
    }
    match step {
        0 => 1,
        s => 1 + (spent - limit) / s,
    }
}

/// A warning taken (`DayCeiling::take_warning`): the day's spend is at a
/// level of the ceiling nobody has been told of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub day: String,
    pub level: u64,
    pub limit: Micros,
    pub step: Micros,
    pub spent: Micros,
    /// The runaway line, 0 for none.
    pub runaway: Micros,
    pub turns_at: String,
}

/// What the start read back of today from the ledger.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seed {
    pub spent: Micros,
    /// The highest warning level today's `spend.day_warning` rows say.
    pub warned: u64,
    /// Today's `spend.ceiling` row's time, if one was written.
    pub reached_at_ms: Option<u64>,
}

impl DayCeiling {
    pub fn new(limit: Micros, zone: TimeZone, clock: Arc<dyn crate::clock::Clock>) -> Self {
        Self {
            limit: AtomicU64::new(limit),
            notify: AtomicBool::new(false),
            runaway: AtomicU64::new(DEFAULT_RUNAWAY_MICROS),
            step: AtomicU64::new(DEFAULT_STEP_MICROS),
            burn_line: AtomicU64::new(0),
            burn: Mutex::default(),
            zone,
            clock,
            st: Mutex::default(),
            next_hold: AtomicU64::new(0),
        }
    }

    /// The kernel's clock's now.
    pub fn now(&self) -> u64 {
        self.clock.now_ms()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn limit(&self) -> Micros {
        self.limit.load(Ordering::Relaxed)
    }

    /// The mode and its lines (`[kernel] daily_spend_ceiling_mode`,
    /// `daily_runaway_usd`, `daily_notice_step_usd`, `runaway_usd_per_10min`).
    pub fn set_rules(&self, notify: bool, runaway: Micros, step: Micros, burn_line: Micros) {
        self.notify.store(notify, Ordering::Relaxed);
        self.runaway.store(runaway, Ordering::Relaxed);
        self.step.store(step, Ordering::Relaxed);
        self.burn_line.store(burn_line, Ordering::Relaxed);
    }

    pub fn notifies(&self) -> bool {
        self.notify.load(Ordering::Relaxed)
    }

    pub fn runaway(&self) -> Micros {
        self.runaway.load(Ordering::Relaxed)
    }

    pub fn step(&self) -> Micros {
        self.step.load(Ordering::Relaxed)
    }

    /// The amount a call may not pass: the ceiling under stop, the runaway
    /// line under notify (0: no bar).
    pub fn bar(&self) -> Micros {
        if self.notifies() {
            self.runaway()
        } else {
            self.limit()
        }
    }

    /// A new ceiling (a config reload): what is spent stays.
    pub fn set_limit(&self, limit: Micros) {
        self.limit.store(limit, Ordering::Relaxed);
    }

    /// `now_ms`'s local day: its date, its first millisecond, and the next
    /// day's.
    pub fn day_of(&self, now_ms: u64) -> (String, u64, u64) {
        span(&self.zone, now_ms)
    }

    /// Move to `now_ms`'s day: a new one starts at zero, with the holds of
    /// the calls still in flight carried into it.
    fn roll(&self, st: &mut State, now_ms: u64) {
        if !st.day.is_empty() && st.start_ms <= now_ms && now_ms < st.end_ms {
            return;
        }
        let (day, start, end) = self.day_of(now_ms);
        if day != st.day {
            st.spent = 0;
            st.reached_at_ms = None;
            st.reached_limit = 0;
            st.noticed = false;
            st.warned = 0;
            st.day = day;
        }
        st.start_ms = start;
        st.end_ms = end;
    }

    /// Whether the day is stopped under `limit`: a call was refused today
    /// under a ceiling no lower.
    fn stopped(st: &State, limit: Micros) -> bool {
        limit > 0 && st.reached_at_ms.is_some() && limit <= st.reached_limit
    }

    /// Whether a call of `need` is refused now: the day is stopped, or it
    /// would pass the ceiling.
    fn refuses(st: &State, limit: Micros, need: Micros) -> bool {
        Self::stopped(st, limit)
            || (limit > 0 && st.spent.saturating_add(st.held_total).saturating_add(need) > limit)
    }

    fn reached(&self, st: &mut State, now_ms: u64, needed: Micros) -> Reached {
        let limit = self.bar();
        let since = match Self::stopped(st, limit) {
            true => st.reached_at_ms.map(|at| local_time(&self.zone, at)),
            false => {
                st.reached_at_ms = Some(now_ms);
                st.reached_limit = limit;
                None
            }
        };
        Reached {
            since,
            runaway: self.notifies(),
            day: st.day.clone(),
            limit,
            spent: st.spent,
            held: st.held_total,
            needed,
            turns_at_ms: st.end_ms,
            turns_at: local_time(&self.zone, st.end_ms),
        }
    }

    /// Hold `need` for the call `id` makes, or refuse it: today's spend, what
    /// is held, and `need` together may not pass the ceiling.
    pub fn hold(&self, now_ms: u64, id: &str, need: Micros) -> Result<(), Reached> {
        let limit = self.bar();
        let mut st = self.lock();
        self.roll(&mut st, now_ms);
        if Self::refuses(&st, limit, need) {
            return Err(self.reached(&mut st, now_ms, need));
        }
        st.held_total = st.held_total.saturating_add(need);
        *st.held.entry(id.to_string()).or_default() += need;
        Ok(())
    }

    /// A hold of the core's own (the judge, consolidation, a learning run),
    /// settled at the call's real cost, or released if dropped unsettled.
    pub fn hold_call(self: &Arc<Self>, now_ms: u64, need: Micros) -> Result<Hold, Reached> {
        let id = format!("call:{}", self.next_hold.fetch_add(1, Ordering::Relaxed));
        self.hold(now_ms, &id, need)?;
        Ok(Hold {
            ceiling: self.clone(),
            id,
            done: false,
        })
    }

    /// Let go of `id`'s hold, spending nothing: its call was never made.
    pub fn release(&self, id: &str) {
        let mut st = self.lock();
        if let Some(m) = st.held.remove(id) {
            st.held_total = st.held_total.saturating_sub(m);
        }
    }

    /// `id`'s call settled: its hold goes, and `actual` is spent (its hold's
    /// amount, or `reserved` for a hold this process never made, when no
    /// cost is known).
    pub fn settle(
        &self,
        now_ms: u64,
        id: &str,
        reserved: Micros,
        actual: Option<Micros>,
    ) -> Micros {
        let mut st = self.lock();
        self.roll(&mut st, now_ms);
        let held = st.held.remove(id);
        if let Some(m) = held {
            st.held_total = st.held_total.saturating_sub(m);
        }
        let cost = actual.unwrap_or(held.unwrap_or(reserved));
        st.spent = st.spent.saturating_add(cost);
        cost
    }

    /// Part of `key`'s hold settled: `reserved` of it goes, and `cost` is
    /// spent. For a caller that holds by amount under one key (the judge's
    /// day budget settles by amount, not by call).
    pub fn settle_part(&self, now_ms: u64, key: &str, reserved: Micros, cost: Micros) {
        let mut st = self.lock();
        self.roll(&mut st, now_ms);
        if let Some(m) = st.held.get_mut(key) {
            let gone = reserved.min(*m);
            *m -= gone;
            if *m == 0 {
                st.held.remove(key);
            }
            st.held_total = st.held_total.saturating_sub(gone);
        }
        st.spent = st.spent.saturating_add(cost);
    }

    /// A cost paid with no hold: booked in full, even past the ceiling.
    pub fn book(&self, now_ms: u64, cost: Micros) {
        let mut st = self.lock();
        self.roll(&mut st, now_ms);
        st.spent = st.spent.saturating_add(cost);
    }

    /// Whether a call of `need` would fit now, holding nothing: for a cost
    /// that is booked after its call (speech), asked before it is made.
    pub fn fits(&self, now_ms: u64, need: Micros) -> Result<(), Reached> {
        let limit = self.bar();
        let mut st = self.lock();
        self.roll(&mut st, now_ms);
        if Self::refuses(&st, limit, need) {
            return Err(self.reached(&mut st, now_ms, need));
        }
        Ok(())
    }

    /// The start's read of today from the ledger, once, before serving.
    pub fn seed(&self, now_ms: u64, seed: &Seed) {
        let mut st = self.lock();
        self.roll(&mut st, now_ms);
        st.spent = st.spent.saturating_add(seed.spent);
        if let Some(at) = seed.reached_at_ms {
            // The start read a row under a ceiling no lower than this one.
            st.reached_at_ms.get_or_insert(at);
            st.reached_limit = st.reached_limit.max(self.bar());
            st.noticed = true;
        }
        st.warned = st.warned.max(seed.warned);
    }

    /// Whether this refusal is the day's first unannounced one: true once a
    /// day, and never on a day whose notice the start read back.
    pub fn take_notice(&self, now_ms: u64) -> bool {
        let mut st = self.lock();
        self.roll(&mut st, now_ms);
        if st.reached_at_ms.is_none() || st.noticed {
            return false;
        }
        st.noticed = true;
        true
    }

    /// The day's warning, once per level (theseus-9e02): under notify, when
    /// the spend is at the ceiling or a step past it and nobody was told of
    /// that level, the highest unannounced one. Nothing under stop (its
    /// refusal has its own notice, `take_notice`), and nothing below the
    /// ceiling.
    pub fn take_warning(&self, now_ms: u64) -> Option<Warning> {
        if !self.notifies() {
            return None;
        }
        let mut st = self.lock();
        self.roll(&mut st, now_ms);
        let (limit, step) = (self.limit(), self.step());
        let level = level_of(st.spent, limit, step);
        if level <= st.warned {
            return None;
        }
        st.warned = level;
        Some(Warning {
            day: st.day.clone(),
            level,
            limit,
            step,
            spent: st.spent,
            runaway: self.runaway(),
            turns_at: local_time(&self.zone, st.end_ms),
        })
    }

    /// Record `cost`, settled for `execution` at `now_ms`, in its burn
    /// window.
    pub fn burn_record(&self, now_ms: u64, execution: &str, cost: Micros) {
        if self.burn_line.load(Ordering::Relaxed) == 0 || cost == 0 {
            return;
        }
        let mut m = self.burn.lock().unwrap_or_else(PoisonError::into_inner);
        if m.len() > 512 {
            m.retain(|_, r| {
                r.back()
                    .is_some_and(|(t, _)| now_ms.saturating_sub(*t) < BURN_WINDOW_MS)
            });
        }
        let ring = m.entry(execution.to_string()).or_default();
        while ring
            .front()
            .is_some_and(|(t, _)| now_ms.saturating_sub(*t) >= BURN_WINDOW_MS)
        {
            ring.pop_front();
        }
        ring.push_back((now_ms, cost));
    }

    /// `execution`'s settled spend in the last [`BURN_WINDOW_MS`], when it
    /// has reached the line.
    pub fn burn_stop(&self, now_ms: u64, execution: &str) -> Option<BurnStop> {
        let line = self.burn_line.load(Ordering::Relaxed);
        if line == 0 {
            return None;
        }
        let m = self.burn.lock().unwrap_or_else(PoisonError::into_inner);
        let spent: Micros = m
            .get(execution)?
            .iter()
            .filter(|(t, _)| now_ms.saturating_sub(*t) < BURN_WINDOW_MS)
            .map(|(_, c)| *c)
            .sum();
        (spent >= line).then_some(BurnStop { spent, line })
    }

    /// How many settles `execution`'s ring holds (a test's view).
    #[cfg(test)]
    pub(crate) fn burn_len(&self, execution: &str) -> usize {
        let m = self.burn.lock().unwrap_or_else(PoisonError::into_inner);
        m.get(execution).map_or(0, VecDeque::len)
    }

    /// How many executions have a ring (a test's view).
    #[cfg(test)]
    pub(crate) fn burn_rings(&self) -> usize {
        self.burn
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// The owner resumed `execution`: its window starts empty.
    pub fn burn_clear(&self, execution: &str) {
        self.burn
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(execution);
    }

    pub fn today(&self, now_ms: u64) -> Today {
        let mut st = self.lock();
        self.roll(&mut st, now_ms);
        let limit = self.limit();
        let bar = self.bar();
        Today {
            day: st.day.clone(),
            limit,
            runaway: if self.notifies() { self.runaway() } else { 0 },
            notify: self.notifies(),
            step: self.step(),
            spent: st.spent,
            held: st.held_total,
            reached_at_ms: st.reached_at_ms.filter(|_| Self::stopped(&st, bar)),
            turns_at_ms: st.end_ms,
            turns_at: local_time(&self.zone, st.end_ms),
        }
    }
}

/// A background call's hold on the day ([`DayCeiling::hold_call`]).
#[derive(Debug)]
pub struct Hold {
    ceiling: Arc<DayCeiling>,
    id: String,
    done: bool,
}

impl Hold {
    /// The call cost `cost`: the hold becomes spend.
    pub fn settle(mut self, cost: Micros) {
        self.done = true;
        let now = self.ceiling.now();
        self.ceiling.settle(now, &self.id, 0, Some(cost));
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        if !self.done {
            self.ceiling.release(&self.id);
        }
    }
}

/// `now_ms`'s local day in `zone`: `(date, start, next day's start)`.
fn span(zone: &TimeZone, now_ms: u64) -> (String, u64, u64) {
    let ms = i64::try_from(now_ms).unwrap_or(i64::MAX);
    let zoned = jiff::Timestamp::from_millisecond(ms)
        .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
        .to_zoned(zone.clone());
    let day = zoned.date().to_string();
    let start = zoned.start_of_day().ok();
    let end = start
        .as_ref()
        .and_then(|s| s.tomorrow().ok())
        .and_then(|t| t.start_of_day().ok());
    let as_ms = |z: &jiff::Zoned| u64::try_from(z.timestamp().as_millisecond()).unwrap_or(0);
    match (start, end) {
        (Some(s), Some(e)) => (day, as_ms(&s), as_ms(&e)),
        _ => (day, now_ms, now_ms.saturating_add(1)),
    }
}

/// `ms` as local time in `zone`: `2026-10-09 00:00`.
fn local_time(zone: &TimeZone, ms: u64) -> String {
    let ms = i64::try_from(ms).unwrap_or(i64::MAX);
    jiff::Timestamp::from_millisecond(ms)
        .map(|t| {
            t.to_zoned(zone.clone())
                .strftime("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// What a settle does to the day, applied once its frame is written.
#[derive(Debug, Clone)]
pub(crate) enum Effect {
    /// A provider call's reservation `id` settled, at `actual` (none known:
    /// its reservation, `reserved`, counts).
    Settle {
        id: String,
        execution_id: String,
        reserved: Micros,
        actual: Option<Micros>,
    },
    /// A cost with no hold.
    Book(Micros),
}

impl Effect {
    /// A provider call's settle, from its action; nothing for any other
    /// tool's (an AWS hand's is not model spend).
    pub(crate) fn of(a: &Action, actual: Option<Micros>) -> Option<Effect> {
        let id = a.reservation_id.as_ref()?;
        (a.tool == PROVIDER_TOOL).then(|| Effect::Settle {
            id: id.clone(),
            execution_id: a.execution_id.clone(),
            reserved: a.reserved_micros,
            actual,
        })
    }

    /// An unknown call's outcome learned later: its reservation already
    /// counted, so only a cost above it is booked more.
    pub(crate) fn resolved(a: &Action, cost: Option<Micros>) -> Option<Effect> {
        (a.tool == PROVIDER_TOOL)
            .then(|| Effect::Book(cost.unwrap_or(0).saturating_sub(a.reserved_micros)))
    }
}

impl Kernel {
    /// The daemon's day ceiling.
    pub fn day_ceiling(&self) -> &Arc<DayCeiling> {
        &self.ceiling
    }

    /// Hold a provider call's reservation `id` on the day, or refuse it with
    /// `KernelError::DayCeiling`. Inside a transaction the hold is let go if
    /// the transaction fails.
    pub(crate) fn day_hold(&self, id: &str, need: Micros) -> anyhow::Result<()> {
        self.ceiling
            .hold(self.now_ms(), id, need)
            .map_err(|r| KernelError::DayCeiling(Box::new(r)))?;
        if let Some(tx) = self.tx() {
            tx.day_held(id);
        }
        Ok(())
    }

    /// `effects`, once their frame is written: now outside a transaction (the
    /// caller's commit returned), at the commit inside one.
    pub(crate) fn day_after(&self, effects: impl IntoIterator<Item = Effect>) {
        match self.tx() {
            Some(tx) => tx.day_effects(effects),
            None => self.apply_day(effects),
        }
    }

    /// The executions above `execution_id`, nearest first, whose burn window
    /// its settled cost also fills (theseus-o1h3f.5): a helper's spend is
    /// its parent's, and so on up to the conversation. Plain store reads,
    /// which take no execution's lock (an execution's parent never changes),
    /// so this is sound under the locks a transaction's commit still holds.
    /// At most [`BURN_ANCESTORS`] steps and never one execution twice, so a
    /// corrupt store's cycle ends.
    fn burn_ancestors(&self, execution_id: &str) -> Vec<String> {
        let mut seen = vec![execution_id.to_string()];
        let mut at = execution_id.to_string();
        while seen.len() <= BURN_ANCESTORS {
            let Some(up) = self.execution(&at).ok().flatten().and_then(|e| e.parent) else {
                break;
            };
            if seen.contains(&up) {
                break;
            }
            seen.push(up.clone());
            at = up;
        }
        seen.split_off(1)
    }

    pub(crate) fn apply_day(&self, effects: impl IntoIterator<Item = Effect>) {
        let now = self.now_ms();
        for e in effects {
            match e {
                Effect::Settle {
                    id,
                    execution_id,
                    reserved,
                    actual,
                } => {
                    let cost = self.ceiling.settle(now, &id, reserved, actual);
                    self.ceiling.burn_record(now, &execution_id, cost);
                    for up in self.burn_ancestors(&execution_id) {
                        self.ceiling.burn_record(now, &up, cost);
                    }
                }
                Effect::Book(cost) => self.ceiling.book(now, cost),
            }
        }
    }
}
