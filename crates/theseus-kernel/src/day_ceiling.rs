//! The daemon's daily spend ceiling (theseus-kp20): one backstop over every
//! model call the daemon makes, by the owner's local day, `[kernel]
//! daily_spend_ceiling_usd` ($200 by default). It is not a budget: a
//! session's limit and its mode, a place's ceiling, an MCP client's limit,
//! AWS budgets, and the background day caps all keep their meaning, and this
//! sits above them, high enough never to touch normal work, low enough that
//! a loop spinning overnight stops. There is no notify mode: at the ceiling a
//! call is not made.
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
//!   own call's, counted once: carrying it to its parent's budget adds
//!   nothing here.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use jiff::tz::TimeZone;

use crate::kernel::{Kernel, KernelError};
use crate::types::*;

/// The default ceiling: $200 a day (the owner, 2026-10-08).
pub const DEFAULT_CEILING_MICROS: Micros = 200 * MICROS_PER_USD;

/// The counter. Shared by every view of a kernel.
pub struct DayCeiling {
    /// The ceiling, in micros. 0 turns it off (a test's kernel may).
    limit: AtomicU64,
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
    /// Whether today's notice was taken (or read back at the start).
    noticed: bool,
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
}

impl std::fmt::Display for Reached {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "today's spend reached the {} daily ceiling ({} spent today, {} held by calls in \
             flight, and this call needs {}), so the call was not made; the owner can raise \
             `[kernel] daily_spend_ceiling_usd` or wait until midnight (the day turns at {} \
             local time)",
            usd(self.limit),
            usd(self.spent),
            usd(self.held),
            usd(self.needed),
            self.turns_at
        )
    }
}

/// Today, as `budget.list` and health show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Today {
    pub day: String,
    pub limit: Micros,
    pub spent: Micros,
    pub held: Micros,
    pub reached_at_ms: Option<u64>,
    pub turns_at_ms: u64,
    pub turns_at: String,
}

impl Today {
    /// Whether a call is refused now: one was today, or nothing is left.
    pub fn reached(&self) -> bool {
        self.reached_at_ms.is_some() || (self.limit > 0 && self.spent >= self.limit)
    }
}

/// What the start read back of today from the ledger.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seed {
    pub spent: Micros,
    /// Today's `spend.ceiling` row's time, if one was written.
    pub reached_at_ms: Option<u64>,
}

impl DayCeiling {
    pub fn new(limit: Micros, zone: TimeZone, clock: Arc<dyn crate::clock::Clock>) -> Self {
        Self {
            limit: AtomicU64::new(limit),
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
            st.noticed = false;
            st.day = day;
        }
        st.start_ms = start;
        st.end_ms = end;
    }

    fn reached(&self, st: &mut State, now_ms: u64, needed: Micros) -> Reached {
        st.reached_at_ms.get_or_insert(now_ms);
        Reached {
            day: st.day.clone(),
            limit: self.limit(),
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
        let limit = self.limit();
        let mut st = self.lock();
        self.roll(&mut st, now_ms);
        if limit > 0 && st.spent.saturating_add(st.held_total).saturating_add(need) > limit {
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
    pub fn settle(&self, now_ms: u64, id: &str, reserved: Micros, actual: Option<Micros>) {
        let mut st = self.lock();
        self.roll(&mut st, now_ms);
        let held = st.held.remove(id);
        if let Some(m) = held {
            st.held_total = st.held_total.saturating_sub(m);
        }
        let cost = actual.unwrap_or(held.unwrap_or(reserved));
        st.spent = st.spent.saturating_add(cost);
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
        let limit = self.limit();
        let mut st = self.lock();
        self.roll(&mut st, now_ms);
        if limit > 0 && st.spent.saturating_add(st.held_total).saturating_add(need) > limit {
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
            st.reached_at_ms.get_or_insert(at);
            st.noticed = true;
        }
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

    pub fn today(&self, now_ms: u64) -> Today {
        let mut st = self.lock();
        self.roll(&mut st, now_ms);
        Today {
            day: st.day.clone(),
            limit: self.limit(),
            spent: st.spent,
            held: st.held_total,
            reached_at_ms: st.reached_at_ms,
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

    pub(crate) fn apply_day(&self, effects: impl IntoIterator<Item = Effect>) {
        let now = self.now_ms();
        for e in effects {
            match e {
                Effect::Settle {
                    id,
                    reserved,
                    actual,
                } => self.ceiling.settle(now, &id, reserved, actual),
                Effect::Book(cost) => self.ceiling.book(now, cost),
            }
        }
    }
}
