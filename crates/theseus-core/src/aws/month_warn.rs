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

/// The META key that holds the month an account's stop was last told.
fn stopped_key(account: &str) -> String {
    format!("aws.month_stopped.{account}")
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

    /// After a read of the account's budget: when it shows the month's actual
    /// spend at the line, tell the owner once a month. The tender reads every
    /// six hours, so this is seen at the next read, never before the stop.
    pub(crate) fn aws_month_stopped(&self, account: &Account, b: &AwsBudgetStatus) {
        if b.limit_cents == 0 || b.actual_cents < b.limit_cents {
            return;
        }
        let month = month_of(theseus_protocol::now_unix_ms());
        let key = stopped_key(&account.id);
        let told = self.store.get_meta::<String>(&key).ok().flatten();
        if told.as_deref() == Some(month.as_str()) {
            return;
        }
        let written = (|| -> anyhow::Result<()> {
            let Some((a, frame)) = self.stopped_frame(account, b, &month)? else {
                return Ok(());
            };
            self.store.append(&frame)?;
            self.outbox.posted(&a);
            Ok(())
        })();
        if let Err(e) = written {
            tracing::warn!(account = %account.id, error = %format!("{e:#}"), "aws: the month's stop was not told");
        }
    }

    /// The month's stop as one frame: the staged post to the owner and the
    /// META mark that says it was told, so a failed write leaves neither and
    /// a restart cannot post twice. `None` when this run has told it.
    fn stopped_frame(
        &self,
        account: &Account,
        b: &AwsBudgetStatus,
        month: &str,
    ) -> anyhow::Result<Option<(theseus_kernel::Action, Vec<NewRecord>)>> {
        let detail = format!(
            "A month's actual spend in account {} reached the budget line. Where this account \
             runs the foundation stack's stop action, AWS has now applied its deny-spend \
             policy; Theseus does not read whether it ran. The limit above is the budget line AWS holds, \
             which the reconcile sets from `monthly_budget_usd` at a start, so the two can \
             differ. This was seen at the next budget \
             read (every six hours), so it cannot be earlier than the stop itself.",
            account.id
        );
        let staged = self.outbox.stage_rule(
            None,
            crate::rule_notice::Notice {
                rule: crate::rule_notice::Rule::F1,
                outcome: crate::rule_notice::Outcome::Stopped,
                limit_usd: b.limit_cents as f64 / 100.0,
                figure_usd: b.actual_cents as f64 / 100.0,
                scope: &format!("{}:{month}", account.id),
                detail: &detail,
            },
        )?;
        let Some((a, mut frame)) = staged else {
            return Ok(None);
        };
        frame.push(NewRecord::json(
            kinds::META,
            Some(&stopped_key(&account.id)),
            &month,
        )?);
        Ok(Some((a, frame)))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::aws::tests::{core, Fake};

    fn stopped_posts(c: &crate::Core) -> Vec<String> {
        c.outbox
            .open_for(crate::outbox::OPERATOR_TARGET)
            .iter()
            .filter(|a| crate::outbox::kind_of(a) == "spend_ceiling")
            .map(|a| {
                crate::outbox::body_of(a)["text"]
                    .as_str()
                    .unwrap_or("")
                    .to_string()
            })
            .filter(|t| t.contains("Rule F1"))
            .collect()
    }

    fn at(actual_cents: u64) -> AwsBudgetStatus {
        AwsBudgetStatus {
            limit_cents: 10_000,
            actual_cents,
            forecast_cents: None,
            read_at_unix_ms: 0,
        }
    }

    fn this_month() -> String {
        month_of(theseus_protocol::now_unix_ms())
    }

    /// A core, its only account, and the stop's META key.
    fn rig(dir: &std::path::Path) -> (Arc<crate::Core>, Arc<Account>, String) {
        let fake = Fake::start(|_, _| (200, vec![], String::new()));
        let c = core(&fake, dir, vec![]);
        let a = c.tools.aws.clone().unwrap().account(None).unwrap().clone();
        let key = stopped_key(&a.id);
        (c, a, key)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_months_stop_tells_the_owner_once_a_month() {
        let dir = tempfile::tempdir().unwrap();
        let (c, a, key) = rig(dir.path());
        // a. one cent under the line: no post, no mark.
        c.aws_month_stopped(&a, &at(9_999));
        assert!(stopped_posts(&c).is_empty());
        assert_eq!(c.store.get_meta::<String>(&key).unwrap(), None);
        // b. at the line: exactly one notice, naming the rule and the figures.
        c.aws_month_stopped(&a, &at(10_000));
        let told = stopped_posts(&c);
        assert_eq!(told.len(), 1, "one notice: {told:?}");
        for w in [
            "Rule F1 stopped",
            "$100.00",
            "monthly_budget_usd",
            "does not read whether it ran",
            "budget line AWS holds",
        ] {
            assert!(told[0].contains(w), "{w}: {}", told[0]);
        }
        assert!(!told[0].contains("applies its stop action when"), "{}", told[0]);
        assert_eq!(
            c.store.get_meta::<String>(&key).unwrap().as_deref(),
            Some(this_month().as_str())
        );
        // c. a later read, same month: still one.
        c.aws_month_stopped(&a, &at(10_500));
        assert_eq!(stopped_posts(&c).len(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_stops_post_and_mark_are_one_frame() {
        let dir = tempfile::tempdir().unwrap();
        let (c, a, key) = rig(dir.path());
        let month = this_month();
        let (_, frame) = c.stopped_frame(&a, &at(10_000), &month).unwrap().unwrap();
        let marks: Vec<_> = frame
            .iter()
            .filter(|r| r.kind == kinds::META && r.key.as_deref() == Some(key.as_str()))
            .collect();
        assert_eq!(marks.len(), 1, "one mark in the frame");
        assert_eq!(serde_json::from_slice::<String>(&marks[0].payload).unwrap(), month);
        assert!(frame.len() >= 2, "the staged post is in it too: {}", frame.len());
        // nothing is written until the caller appends the frame.
        assert_eq!(c.store.get_meta::<String>(&key).unwrap(), None);
        assert!(stopped_posts(&c).is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_marked_month_stays_quiet_across_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let (c, a, key) = rig(dir.path());
        // d. the mark says this month: the in-memory set is empty, only the
        // mark suppresses.
        c.store
            .append(&[NewRecord::json(kinds::META, Some(&key), &this_month()).unwrap()])
            .unwrap();
        c.aws_month_stopped(&a, &at(12_000));
        assert!(stopped_posts(&c).is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn another_months_mark_does_not_silence_this_month() {
        let dir = tempfile::tempdir().unwrap();
        let (c, a, key) = rig(dir.path());
        // e. a mark from another month.
        c.store
            .append(&[NewRecord::json(kinds::META, Some(&key), &"1999-01").unwrap()])
            .unwrap();
        c.aws_month_stopped(&a, &at(10_000));
        assert_eq!(stopped_posts(&c).len(), 1);
    }
}
