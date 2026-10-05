//! Runaway-train mode (step 40 part 2's review, theseus-ext.12): the
//! budget lines alert, and the alerts are the authority, unless the
//! observed spend runs `runaway_factor` (default 10) times past a line.
//!
//! - **The figure** is what the hour's meter counts (`watch`): what the AWS
//!   actions dispatched in the period reserve (those running) or spent
//!   (those settled), computed at the dispatch, never left to the poller's
//!   last pass. The hour is the clock hour; the day the local day, from
//!   local midnight.
//! - **The lines**: `runaway_factor` × `hourly_alert_usd` within the hour,
//!   and `runaway_factor` × `daily_budget_usd` within the day, when that is
//!   set.
//! - **Entering**: a new action the meters count (a hands group today)
//!   whose own reservation would bring the figure to a line is refused, and
//!   the account enters runaway mode until the hour or the day turns; a
//!   poller's pass that finds the figure at a line enters it too. Entering
//!   writes its mark (META `aws.runaway.<account>`) and its row
//!   (`aws.runaway`) in one frame, and one notice where approvals go; the
//!   mark keeps a restart in the same period from saying it again.
//! - **In runaway mode** every new action that reserves is refused, saying
//!   the spend, the line, the factor, when the period turns, and the keys
//!   that change it. Never refused: a cancel, a list or a status read, the
//!   reaper, and a running hand's settling; a group already admitted
//!   launches its later waves and finishes. Raising a line or the factor
//!   takes a restart, since the config is read at the start.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_kernel::Kernel;
use theseus_protocol::LedgerKind;
use theseus_store::kinds;

use super::group;
use super::watch::{self, hour_of, HOUR_MS};

use crate::aws::Account;
use crate::fact::{self, Fact, Rec, Say};
use crate::outbox::Outbox;
use crate::store::Store;

/// The META key's prefix of an account's runaway mode.
pub const MARK: &str = "aws.runaway.";

/// A day, in milliseconds.
const DAY_MS: u64 = 24 * HOUR_MS;

/// Which line the figure ran past.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Line {
    Hour,
    Day,
}

/// An account's runaway mode: its mark, and its row's fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Runaway {
    pub account: String,
    pub line: Line,
    /// The figure that reached it: reserved and spent in the period, with
    /// the refused action's own reservation when one brought it there.
    pub micros: u64,
    /// The line itself (`hourly_alert_usd`, or `daily_budget_usd`).
    pub line_micros: u64,
    pub factor: f64,
    /// The period: from its start until it turns.
    pub since_ms: u64,
    pub until_ms: u64,
    pub at_ms: u64,
}

impl Runaway {
    /// Whether it holds at `now_ms`.
    pub fn holds(&self, now_ms: u64) -> bool {
        self.since_ms <= now_ms && now_ms < self.until_ms
    }
}

impl Fact for Runaway {
    const KIND: Option<LedgerKind> = Some(LedgerKind::AwsRunaway);

    fn row(&self) -> Value {
        json!({"account": self.account, "line": self.line, "usd": self.micros as f64 / 1e6,
            "line_usd": self.line_micros as f64 / 1e6, "factor": self.factor,
            "since_ms": self.since_ms, "until_ms": self.until_ms})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(theseus_protocol::NarrativePart::Job, entered(self));
    }
}

/// The local day that holds `now_ms`: its start, from local midnight.
pub fn day_of(now_ms: u64) -> u64 {
    let l = crate::wake::local(now_ms);
    let into = (u64::from(l.hour) * 3600 + u64::from(l.minute) * 60 + u64::from(l.second)) * 1000
        + now_ms % 1000;
    now_ms.saturating_sub(into)
}

/// The keys that move a line, in words.
fn keys(r: &Runaway) -> &'static str {
    match r.line {
        Line::Hour => "hourly_alert_usd or runaway_factor",
        Line::Day => "daily_budget_usd or runaway_factor",
    }
}

fn period(r: &Runaway) -> &'static str {
    match r.line {
        Line::Hour => "this hour",
        Line::Day => "today",
    }
}

/// When the period turns, as local time.
fn turns(r: &Runaway) -> String {
    let l = crate::wake::local(r.until_ms);
    format!("{:02}:{:02}", l.hour, l.minute)
}

/// Entering, as the notice and the narrative say it.
fn entered(r: &Runaway) -> String {
    format!(
        "🚂 AWS runaway mode on account {}: {} reserved and spent {}, {} × its {} line ({}). \
         New AWS actions that reserve are refused until {}; running hands finish, and cancels, \
         lists and status reads still run. To change it, raise {} under [aws.accounts.{}] and \
         restart.",
        r.account,
        crate::narrative::dollars(r.micros),
        period(r),
        r.factor,
        crate::narrative::dollars(r.line_micros),
        match r.line {
            Line::Hour => "hourly_alert_usd",
            Line::Day => "daily_budget_usd",
        },
        turns(r),
        keys(r),
        r.account,
    )
}

/// A refusal in runaway mode, as the call's result says it.
fn refusal(r: &Runaway, figure: u64, reserve: u64) -> String {
    format!(
        "Not run: AWS runaway mode on account {}. Its observed spend {} is {} (reserved and \
         spent, with this call's {} worst case), past runaway_factor {} × its {} line {}. It \
         ends when the {} turns, at {} local time; to change it, the operator raises {} under \
         [aws.accounts.{}] and restarts the daemon. Running hands finish, and a cancel, a list \
         or a status read still runs.",
        r.account,
        period(r),
        crate::narrative::dollars(figure),
        crate::narrative::dollars(reserve),
        r.factor,
        match r.line {
            Line::Hour => "hourly_alert_usd",
            Line::Day => "daily_budget_usd",
        },
        crate::narrative::dollars(r.line_micros),
        match r.line {
            Line::Hour => "hour",
            Line::Day => "day",
        },
        turns(r),
        keys(r),
        r.account,
    )
}

/// What `account`'s AWS actions dispatched since `since_ms` reserve (those
/// running) or spent (those settled), as the hour's meter counts it.
pub fn spend_since(kernel: &Kernel, store: &Store, account: &str, since_ms: u64) -> Result<u64> {
    let mut sum = 0u64;
    for g in watch::groups_from(store, since_ms.saturating_sub(watch::LOOK_BACK_MS))? {
        if g.account != account {
            continue;
        }
        for a in group::hands(kernel, &g)? {
            if a.dispatched_at_ms.is_some_and(|at| at >= since_ms) {
                sum += group::completion(store, &a.correlation_id)
                    .and_then(|c| c.cost_micros)
                    .unwrap_or(a.reserved_micros);
            }
        }
    }
    Ok(sum)
}

/// `line_micros` × `factor`, in micro-dollars.
fn times(line_micros: u64, factor: f64) -> u64 {
    (line_micros as f64 * factor).round() as u64
}

/// The account's lines at `now_ms`, each with its period: the hour's, and
/// the day's when `daily_budget_usd` is set.
fn lines(account: &Account, now_ms: u64) -> Vec<(Line, u64, u64, u64)> {
    let hour = hour_of(now_ms);
    let mut out = vec![(
        Line::Hour,
        (account.cfg.hourly_alert_usd * 1e6).round() as u64,
        hour,
        hour + HOUR_MS,
    )];
    if let Some(d) = account.cfg.daily_budget_usd {
        let day = day_of(now_ms);
        out.push((Line::Day, u64::from(d) * 1_000_000, day, day + DAY_MS));
    }
    out
}

/// The account's runaway mode, when it holds at `now_ms` and the config still
/// gives the mark's line no more room than the mark saw: a line or a factor
/// raised since (a restart onto the new config), or a day's line removed,
/// ends it, and `reached` decides again. A lowered line keeps it. Read at
/// admission and at a poller's pass, never at the start.
pub fn current(store: &Store, account: &Account, now_ms: u64) -> Option<Runaway> {
    store
        .get_meta::<Runaway>(&format!("{MARK}{}", account.id))
        .ok()
        .flatten()
        .filter(|r| r.holds(now_ms))
        .filter(|r| {
            lines(account, now_ms)
                .into_iter()
                .find(|(line, ..)| *line == r.line)
                .is_some_and(|(_, line_micros, ..)| {
                    times(line_micros, account.cfg.runaway_factor) <= times(r.line_micros, r.factor)
                })
        })
}

/// The first line the figure, with `reserve` added, reaches at `now_ms`.
fn reached(
    kernel: &Kernel,
    store: &Store,
    account: &Account,
    reserve: u64,
    now_ms: u64,
) -> Result<Option<Runaway>> {
    let factor = account.cfg.runaway_factor;
    for (line, line_micros, since_ms, until_ms) in lines(account, now_ms) {
        let micros = spend_since(kernel, store, &account.id, since_ms)? + reserve;
        if micros >= times(line_micros, factor) {
            return Ok(Some(Runaway {
                account: account.id.clone(),
                line,
                micros,
                line_micros,
                factor,
                since_ms,
                until_ms,
                at_ms: now_ms,
            }));
        }
    }
    Ok(None)
}

/// Where entering writes: the store, the operator's notices, and the
/// fact's channels.
pub struct Sink<'a> {
    pub kernel: &'a Kernel,
    pub store: &'a Store,
    pub outbox: &'a Outbox,
    pub rec: Rec<'a>,
}

impl Sink<'_> {
    /// Enter runaway mode: its mark and its row in one frame, then its
    /// notice where approvals go.
    fn enter(&self, r: &Runaway) -> Result<()> {
        self.store.append(&[
            theseus_store::NewRecord::json(kinds::META, Some(&format!("{MARK}{}", r.account)), r)?,
            fact::row(r, None, None)?,
        ])?;
        self.rec.announce(r);
        if let Err(e) = self
            .outbox
            .to_operator(None, json!({"kind": "notice", "text": entered(r)}))
        {
            tracing::warn!(error = %format!("{e:#}"), "hands: runaway mode's notice was not written");
        }
        Ok(())
    }

    /// Whether a new action of `account` that reserves `reserve` may run at
    /// `now_ms`: `None`, or the refusal's words. One whose reservation
    /// brings the figure to a line enters runaway mode.
    pub fn admit(&self, account: &Account, reserve: u64, now_ms: u64) -> Result<Option<String>> {
        if reserve == 0 {
            return Ok(None);
        }
        if let Some(r) = current(self.store, account, now_ms) {
            let figure = spend_since(self.kernel, self.store, &account.id, r.since_ms)? + reserve;
            return Ok(Some(refusal(&r, figure, reserve)));
        }
        match reached(self.kernel, self.store, account, reserve, now_ms)? {
            Some(r) => {
                self.enter(&r)?;
                Ok(Some(refusal(&r, r.micros, reserve)))
            }
            None => Ok(None),
        }
    }

    /// A poller's pass: runaway mode as it holds at `now_ms`, entered now
    /// when the figure is at a line already.
    pub fn observe(&self, account: &Account, now_ms: u64) -> Result<Option<Runaway>> {
        if let Some(r) = current(self.store, account, now_ms) {
            return Ok(Some(r));
        }
        let r = reached(self.kernel, self.store, account, 0, now_ms)?;
        if let Some(r) = &r {
            self.enter(r)?;
        }
        Ok(r)
    }
}

/// Health's words for runaway mode.
pub fn health(r: &Runaway) -> String {
    format!(
        "runaway mode: {} {}, {} × its {} line; new AWS actions refused until {}",
        crate::narrative::dollars(r.micros),
        period(r),
        r.factor,
        crate::narrative::dollars(r.line_micros),
        turns(r),
    )
}

/// A poller's pass for health's hands block: runaway mode, entered when
/// the figure is at a line already, as it holds now.
pub fn into_health(
    core: &crate::Core,
    account: &Account,
    now_ms: u64,
    st: &mut theseus_protocol::AwsHandsStatus,
) {
    let sink = Sink {
        kernel: &core.kernel,
        store: &core.store,
        outbox: &core.outbox,
        rec: core.rec(None),
    };
    match sink.observe(account, now_ms) {
        Ok(Some(r)) => {
            st.runaway_until_unix_ms = Some(r.until_ms);
            st.runaway = Some(health(&r));
        }
        Ok(None) => {}
        Err(e) => tracing::warn!(error = %format!("{e:#}"), "hands: runaway mode was not read"),
    }
}
