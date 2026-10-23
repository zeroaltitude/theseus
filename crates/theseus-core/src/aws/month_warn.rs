//! The AWS month's warning (theseus-9e02; the owner, 2026-10-11: tokens are
//! less cautious, AWS more, so only AWS keeps its stops, plus one warning).
//! The budget's line, read every six hours (`tend::read_budget`), says what
//! AWS Budgets counts this month; once it passes `[aws.accounts.<id>]
//! monthly_warn_usd` (default half of `monthly_budget_usd`), Theseus warns
//! once that month: a `aws.month_warning` row, a narrative line, and a post
//! to the owner's DM when one is bound (the kind the day ceiling's warning
//! uses). It warns only: the hourly line, the daily budget, the runaway
//! brake and the month's stop keep refusing exactly as before.

use serde_json::{json, Value};
use theseus_protocol::{AwsBudgetStatus, LedgerKind};
use theseus_store::kinds;
use theseus_store::NewRecord;

use super::Account;
use crate::fact::{Fact, Say};

/// `aws.month_warning`: the month's AWS spend passed its warning line. Once a
/// month an account; nothing was refused.
pub struct MonthWarned<'a> {
    pub account: &'a str,
    pub month: &'a str,
    pub actual_cents: u64,
    pub warn_cents: u64,
    pub limit_cents: u64,
    pub posted: bool,
    pub text: &'a str,
}

impl Fact for MonthWarned<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::AwsMonthWarning);

    fn row(&self) -> Value {
        json!({"account": self.account, "month": self.month,
               "actual_usd": self.actual_cents as f64 / 100.0,
               "warn_usd": self.warn_cents as f64 / 100.0,
               "limit_usd": self.limit_cents as f64 / 100.0, "posted": self.posted})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(theseus_protocol::NarrativePart::Job, self.text.to_string());
    }
}

/// The META key that holds the month an account was last warned in.
fn meta_key(account: &str) -> String {
    format!("aws.month_warned.{account}")
}

/// `2026-10`, on the daemon's clock.
fn month_of(now_ms: u64) -> String {
    let l = crate::wake::local(now_ms);
    format!("{:04}-{:02}", l.year, l.month)
}

/// The warning's words: the spend, the line it passed, the month's stop.
pub fn warning_text(account: &str, actual_cents: u64, warn_cents: u64, limit_cents: u64) -> String {
    let d = |c: u64| format!("${}.{:02}", c / 100, c % 100);
    crate::owner_note::note(
        crate::owner_note::Kind::Alert,
        &format!("AWS spend in account {account} is past its warning line"),
        &format!(
            "this month's spend is {}, past {} (`[aws.accounts.{account}] monthly_warn_usd`)",
            d(actual_cents),
            d(warn_cents),
        ),
        &format!(
            "The month's budget is {}: AWS spend still stops at its lines, so this only warns.",
            d(limit_cents)
        ),
    )
}

impl crate::Core {
    /// After a read of the account's budget: the month's warning, once a
    /// month. Called from the tender, off every turn's path.
    pub(crate) fn aws_month_warned(&self, account: &Account, b: &AwsBudgetStatus) {
        let warn = account.cfg.monthly_warn_cents(b.limit_cents);
        if warn == 0 || b.actual_cents < warn {
            return;
        }
        let now = theseus_protocol::now_unix_ms();
        let month = month_of(now);
        let key = meta_key(&account.id);
        if self
            .store
            .get_meta::<String>(&key)
            .ok()
            .flatten()
            .as_deref()
            == Some(month.as_str())
        {
            return;
        }
        let text = warning_text(&account.id, b.actual_cents, warn, b.limit_cents);
        let written = (|| -> anyhow::Result<()> {
            let mut frame = Vec::new();
            let post = match self.runner.place_rule.owner_dm_bound() {
                true => {
                    let (a, records) = self
                        .outbox
                        .stage_to_operator(None, json!({"kind": "spend_ceiling", "text": text}))?;
                    frame.extend(records);
                    Some(a)
                }
                false => None,
            };
            let f = MonthWarned {
                account: &account.id,
                month: &month,
                actual_cents: b.actual_cents,
                warn_cents: warn,
                limit_cents: b.limit_cents,
                posted: post.is_some(),
                text: &text,
            };
            frame.push(crate::fact::row(&f, None, None)?);
            frame.push(NewRecord::json(kinds::META, Some(&key), &month)?);
            self.store.append(&frame)?;
            if let Some(a) = post {
                self.outbox.posted(&a);
            }
            Ok(())
        })();
        if let Err(e) = written {
            tracing::warn!(account = %account.id, error = %format!("{e:#}"), "aws: the month's warning was not written");
        }
    }
}
