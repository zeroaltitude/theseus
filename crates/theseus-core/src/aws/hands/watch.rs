//! Watching hands (AWS design §3.3, "Watching a hundred hands", and §3.7's
//! cascade; step 40 part 2, theseus-mgw.11): what health says of each
//! account's hands, and the hour's meter.
//!
//! - **Health's hands block** (`AwsHandsStatus`): the groups open, the hands
//!   running by backend, the oldest, and what they hold reserved; read from
//!   the store at each of the poller's passes, never on the start path.
//! - **The hour's meter.** AWS's billing lags by hours, so Theseus meters
//!   its own: what the AWS actions dispatched this clock hour reserve (those
//!   still running) or spent (those a completion settled, at its cost).
//!   Past `hourly_alert_usd` (default $1) it alerts once that hour: an
//!   `aws.hour.alert` row, a notice where approvals go, and health. The mark
//!   that it fired is a META record, `aws.hour.alerted.<account>`, so a
//!   restart in the same hour does not alert again. It alerts only: whether
//!   it should also refuse is the owner's open question.
//! - **The reaper's failures** (`aws.reaper.failed`): the TTL reaper's
//!   failed invocations come home on the completion queue (its Lambda's
//!   failure destination); each is a row and a count here, never left for
//!   the dead-letter queue.

use std::collections::BTreeMap;

use anyhow::Result;
use serde_json::{json, Value};
use theseus_kernel::ActionState;
use theseus_protocol::{AwsHandsStatus, LedgerKind};
use theseus_store::{kinds, Store as _};

use super::group::{self, GroupRecord, PREFIX};
use super::launch::Backend;
use crate::fact::{self, Fact, Say};
use crate::Core;

/// An hour, in milliseconds.
pub const HOUR_MS: u64 = 3_600_000;

/// The META key's prefix of the hour whose alert fired, by account.
pub const ALERTED: &str = "aws.hour.alerted.";

/// How long a group stays in the meter's view after it began: a hand runs
/// at most 12 hours.
const LOOK_BACK_MS: u64 = 13 * HOUR_MS;

/// The hour's alert: its row, and its line.
pub struct HourAlert<'a> {
    pub account: &'a str,
    pub hour_start_ms: u64,
    pub micros: u64,
    pub line_micros: u64,
    pub groups: usize,
}

impl Fact for HourAlert<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::AwsHourAlert);

    fn row(&self) -> Value {
        json!({"account": self.account, "hour_start_ms": self.hour_start_ms,
            "usd": self.micros as f64 / 1e6, "line_usd": self.line_micros as f64 / 1e6,
            "groups": self.groups})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(theseus_protocol::NarrativePart::Job, words(self));
    }
}

/// The alert in words, as the notice and the narrative say it.
fn words(a: &HourAlert<'_>) -> String {
    format!(
        "💸 AWS this hour: {} reserved and spent by hands on account {}, past its {} line \
         (hourly_alert_usd). Only an alert: nothing was refused.",
        crate::narrative::dollars(a.micros),
        a.account,
        crate::narrative::dollars(a.line_micros),
    )
}

/// A failure record of the TTL reaper's, read off the queue.
pub struct ReaperFailed<'a> {
    pub request_id: Option<&'a str>,
    pub condition: &'a str,
    pub error: &'a str,
}

impl Fact for ReaperFailed<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::AwsReaperFailed);

    fn row(&self) -> Value {
        json!({"request_id": self.request_id, "condition": self.condition, "error": self.error})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            theseus_protocol::NarrativePart::Job,
            format!(
                "⚠️ The hands' TTL reaper failed ({}): {}. Hands past their TTL may run until it next succeeds.",
                self.condition, self.error
            ),
        );
    }
}

/// What one account's hands are now.
#[derive(Default)]
struct Sums {
    groups: usize,
    status: AwsHandsStatus,
}

/// The hour that holds `now_ms`: its start.
pub fn hour_of(now_ms: u64) -> u64 {
    now_ms - now_ms % HOUR_MS
}

/// Every group in the meter's view: open, or begun within its look-back.
fn groups(core: &Core, now_ms: u64) -> Result<Vec<GroupRecord>> {
    let from = now_ms.saturating_sub(LOOK_BACK_MS);
    Ok(core
        .store
        .inner()
        .latest_with_prefix(kinds::META, PREFIX)?
        .into_iter()
        .filter_map(|r| r.decode::<GroupRecord>().ok())
        .filter(|g| g.settled.is_none() || g.created_at_ms >= from)
        .collect())
}

/// Read each account's hands and the hour's meter, put them in health, and
/// alert once an hour past an account's line.
pub fn refresh(core: &Core, reaper: &Reaper) -> Result<()> {
    let Some(aws) = core.tools.aws.as_ref() else {
        return Ok(());
    };
    let now = theseus_protocol::now_unix_ms();
    let hour = hour_of(now);
    let mut by: BTreeMap<String, Sums> = BTreeMap::new();
    for g in groups(core, now)? {
        let sums = by.entry(g.account.clone()).or_default();
        let open = core
            .kernel
            .action(&g.group)?
            .is_some_and(|a| !a.state.is_settled());
        sums.status.groups_open += u32::from(open);
        let mut counted = false;
        for a in group::hands(&core.kernel, &g)? {
            let Some(at) = a.dispatched_at_ms else {
                continue;
            };
            if a.state == ActionState::Dispatched {
                match g.backend {
                    Backend::Lambda => sums.status.running_lambda += 1,
                    Backend::Fargate => sums.status.running_fargate += 1,
                }
                sums.status.oldest_unix_ms =
                    Some(sums.status.oldest_unix_ms.map_or(at, |o| o.min(at)));
                sums.status.reserved_micros += a.reserved_micros;
            }
            if at >= hour {
                let cost = group::completion(&core.store, &a.correlation_id)
                    .and_then(|c| c.cost_micros)
                    .unwrap_or(a.reserved_micros);
                sums.status.hour_micros += cost;
                counted = true;
            }
        }
        sums.groups += usize::from(counted);
    }
    for account in aws.accounts() {
        let sums = by.remove(&account.id).unwrap_or_default();
        let mut st = sums.status;
        st.hour_line_micros = (account.cfg.hourly_alert_usd * 1e6).round() as u64;
        st.read_at_unix_ms = now;
        let key = format!("{ALERTED}{}", account.id);
        let mut alerted: Option<u64> = core.store.get_meta(&key)?;
        if st.hour_micros > st.hour_line_micros && alerted != Some(hour) {
            let alert = HourAlert {
                account: &account.id,
                hour_start_ms: hour,
                micros: st.hour_micros,
                line_micros: st.hour_line_micros,
                groups: sums.groups,
            };
            // The mark and the row in one frame: a restart in this hour
            // reads the mark and says nothing more.
            core.store.append(&[
                theseus_store::NewRecord::json(kinds::META, Some(&key), &hour)?,
                fact::row(&alert, None, None)?,
            ])?;
            core.rec(None).announce(&alert);
            if let Err(e) = core
                .outbox
                .to_operator(None, json!({"kind": "notice", "text": words(&alert)}))
            {
                tracing::warn!(error = %format!("{e:#}"), "hands: the hour's notice was not written");
            }
            alerted = Some(hour);
        }
        st.alerted_hour_unix_ms = alerted.filter(|h| *h == hour);
        let (n, last) = reaper.of(&account.id);
        st.reaper_failures = n;
        st.reaper_last_failure = last;
        account.tended.lock().unwrap().hands = Some(st);
    }
    Ok(())
}

/// The reaper's failures since the start, by account, for health.
#[derive(Default)]
pub struct Reaper {
    seen: std::sync::Mutex<BTreeMap<String, (u32, Option<String>)>>,
}

impl Reaper {
    pub fn failed(&self, account: &str, why: String) {
        let mut s = self.seen.lock().unwrap();
        let e = s.entry(account.to_string()).or_default();
        e.0 += 1;
        e.1 = Some(why);
    }

    fn of(&self, account: &str) -> (u32, Option<String>) {
        self.seen
            .lock()
            .unwrap()
            .get(account)
            .cloned()
            .unwrap_or_default()
    }
}

/// Whether a queue message is the reaper's failure record: Lambda's
/// failure destination, for the `theseus-reaper` function.
pub fn is_reaper_failure(body: &Value) -> bool {
    body.get("requestContext").is_some()
        && body["requestContext"]["functionArn"]
            .as_str()
            .is_some_and(|f| f.contains(":function:theseus-reaper"))
}

/// Take a reaper's failure record: its row, its line, and health's count.
pub fn reaper_failed(core: &Core, account: &str, reaper: &Reaper, body: &Value) -> Result<()> {
    let ctx = &body["requestContext"];
    let error = body["responsePayload"]["errorMessage"]
        .as_str()
        .unwrap_or("no message");
    let f = ReaperFailed {
        request_id: ctx["requestId"].as_str(),
        condition: ctx["condition"].as_str().unwrap_or("an error"),
        error,
    };
    core.rec(None).record(&f);
    reaper.failed(account, format!("{}: {error}", f.condition));
    Ok(())
}
