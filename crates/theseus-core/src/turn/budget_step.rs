//! A task's limits that notify (theseus-usei; the owner, 2026-10-07: "for
//! tasks and tokens, we need to notify, not restrict"). With `[kernel]
//! spend_limit_mode = "notify"`, the default, a call past the session's
//! spend limit goes out (`Kernel::overdraws`), and the loop whose call took
//! the spend to the limit, or past a multiple of it, posts one notice to the
//! session's place; with `[profiles.*] on_max_loops = "notify"`, the loop
//! that reaches `max_loops`, or a multiple of it, does, and the turn goes on.
//! Each notice says what was reached, the spend or loops so far, and how to
//! stop the work, and is a ledger row and a narrative line. Its post and its
//! row ride in the turn's next frame, so a notice adds no frame, and a loop
//! that reaches nothing reads the execution twice and writes nothing more.
//!
//! What bounds a stuck loop under notify is the owner (each multiple's
//! notice, `/stop` or `theseus stop`, and the cockpit's Stop), and the hard
//! multiple (`[kernel] spend_limit_hard_multiple`, 10: the kernel refuses a
//! call whose reservation would pass it, theseus-jse4), which ends the turn.
//! A multiple is noticed at the check after the settle that crossed it: each
//! loop's start and its call's settle, whatever the call's outcome, against
//! the spend the last check saw (`SpendMarks`).

use super::*;
use crate::config::MaxLoopsMode;
use crate::fact::limits::{LoopsReached, SpendReached};

/// The failure's class when the hard multiple refused a call (theseus-jse4).
pub const HARD_STOP_CLASS: &str = "spend_hard_stop";

/// Each execution's spend at its last check, in memory (theseus-jse4): a
/// multiple crossed by a settle outside a loop's own call (a task's carry, a
/// call between two checks) is noticed at the next check, never skipped to
/// the next multiple. A start begins with none: an execution's first check
/// marks it.
#[derive(Debug, Default)]
pub struct SpendMarks(std::sync::Mutex<std::collections::HashMap<String, Micros>>);

impl SpendMarks {
    /// Mark `id` at `spent`: what it was marked at before, if it was.
    fn swap(&self, id: &str, spent: Micros) -> Option<Micros> {
        let mut m = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        m.insert(id.to_string(), spent)
    }
}

impl TurnRunner {
    /// Before a loop's call and at each settle, when the execution's limit
    /// notifies: when the spend passed the limit, or a multiple of it, since
    /// the last check, one notice for the highest multiple reached. Never
    /// one per call: a check that crosses no multiple says nothing, and one
    /// comparison is all it adds.
    pub(super) fn spend_checked(&self, t: &mut Turn<'_>, session: &SessionRecord) -> Result<()> {
        self.day_warned(t)?;
        let Some(e) = t.tc.kernel.execution(t.tc.execution_id)? else {
            return Ok(());
        };
        if !t.tc.kernel.overdraws(&e) {
            return Ok(());
        }
        let (spent, limit) = (e.budget.spent_micros, e.budget.limit_micros);
        let Some(before) = self.spend_marks.swap(&e.id, spent) else {
            return Ok(());
        };
        let Some(multiple) = crossed(before, spent, limit) else {
            return Ok(());
        };
        let who = Self::who(t);
        let past = match multiple {
            1 => format!("its {} limit", narrative::dollars(limit)),
            n => format!("{n} times its {} limit", narrative::dollars(limit)),
        };
        let hard = t.tc.kernel.config().spend_hard_multiple;
        let next = match hard {
            0 => String::new(),
            h => format!(
                " At {} times the limit, {}, its calls stop (`[kernel] spend_limit_hard_multiple`).",
                h,
                narrative::dollars(limit.saturating_mul(h))
            ),
        };
        let text = format!(
            "{who} has spent {}, past {past}, and goes on: the limit notifies \
             (`[kernel] spend_limit_mode = \"notify\"`). To stop the work: {}. \
             The next notice comes at {}.{next}",
            narrative::dollars(spent),
            stop_how(t.tc.session_id),
            narrative::dollars(limit.saturating_mul(multiple + 1)),
        );
        let posted = self.notice(t, &text)?;
        t.record(&SpendReached {
            spent,
            limit,
            multiple,
            lifetime_usd: session.cost_usd + t.cost.unwrap_or(0.0),
            posted,
            text: &text,
        });
        Ok(())
    }

    /// What a budget question keeps of the call that asked it (`args.call`):
    /// its profile, model and output cap; and, for a burn-rate pause
    /// (theseus-9e02), the `brake` that makes it one: the window's spend, the
    /// line, and the window's length.
    pub(super) fn budget_call(t: &mut Turn<'_>) -> Value {
        let mut call = json!({"profile": t.target.profile, "model": t.target.model, "max_output_tokens": t.target.max_tokens, "turn_id": t.tc.turn_id});
        if let Some(b) = t.braked.take() {
            call["brake"] = json!({
                "spent_usd": micros_to_usd(b.spent),
                "line_usd": micros_to_usd(b.line),
                "window_minutes": theseus_kernel::day_ceiling::BURN_WINDOW_MS / 60_000,
            });
        }
        call
    }

    /// A loop's call the hard multiple refused (theseus-jse4): nothing was
    /// planned or sent, and the turn ends failed, class `spend_hard_stop`,
    /// which is lasting, so the driver retries nothing; its failure's notice
    /// carries the words to the session's place.
    pub(super) fn hard_stopped(t: &mut Turn<'_>, h: &theseus_kernel::HardStop) -> Failure {
        t.record(&fact::turn::LoopCut {
            decision: HARD_STOP_CLASS,
        });
        let words = format!("{}: {h}", Self::who(t));
        Failure {
            class: HARD_STOP_CLASS.into(),
            transient: false,
            usage_unknown: false,
            reason: HARD_STOP_CLASS.into(),
            source: anyhow::anyhow!("{words}"),
        }
    }

    /// After loop `i` ended and its turn goes on: at `max_loops` loops, and
    /// at each multiple of it, one notice, when the profile's cap notifies.
    pub(super) fn loops_noticed(&self, t: &mut Turn<'_>, i: u32, going_on: bool) -> Result<()> {
        let max = t.target.max_loops.max(1);
        let loops = i + 1;
        if !going_on || t.target.on_max_loops != MaxLoopsMode::Notify || !loops.is_multiple_of(max)
        {
            return Ok(());
        }
        let multiple = loops / max;
        let who = Self::who(t);
        let text =
            format!(
            "{who} has run {loops} loops in this turn, {}its profile's cap of {max}, and goes on: \
             the cap notifies (`[profiles.{}] on_max_loops = \"notify\"`). To stop the work: \
             {}. The next notice comes at {} loops.",
            if multiple == 1 { String::new() } else { format!("{multiple} times ") },
            t.target.profile,
            stop_how(t.tc.session_id),
            loops + max,
        );
        let posted = self.notice(t, &text)?;
        t.record(&LoopsReached {
            loops,
            max_loops: max,
            multiple,
            posted,
            text: &text,
        });
        Ok(())
    }

    fn who(t: &Turn<'_>) -> String {
        match t.tc.task {
            Some(_) => format!("Task {}", crate::task::short(t.tc.session_id)),
            None => "This session".to_string(),
        }
    }

    /// The notice's post to the session's place, staged for the turn's next
    /// frame, as a budget question's card is; whether it has a place.
    fn notice(&self, t: &mut Turn<'_>, text: &str) -> Result<bool> {
        let Some(target) = self.outbox.target(t.tc.session_id) else {
            return Ok(false);
        };
        let (post, records) = self.outbox.stage(
            t.tc.session_id,
            t.tc.execution_id,
            &target,
            json!({"kind": "notice", "text": text}),
        )?;
        for r in records {
            t.tc.store.defer(r)?;
        }
        t.tc.posts.lock().unwrap().push(post);
        Ok(true)
    }
}

/// The highest multiple of `limit` that a spend going from `before` to
/// `after` reached, when it reached one it had not: `Some(1)` as it reaches
/// the limit, `Some(2)` at twice it.
pub(crate) fn crossed(before: Micros, after: Micros, limit: Micros) -> Option<u64> {
    if limit == 0 {
        return None;
    }
    let (was, now) = (before / limit, after / limit);
    (now > was && now >= 1).then_some(now)
}

/// How the owner stops the work: the CLI, Discord, or the cockpit.
pub(super) fn stop_how(session_id: &str) -> String {
    format!("`theseus stop {session_id}`, `/stop` in its place, or Stop in the cockpit")
}

#[cfg(test)]
mod tests {
    use super::crossed;

    #[test]
    fn a_notice_comes_at_each_multiple_and_never_between() {
        let l = 1_000_000;
        assert_eq!(crossed(0, 999_999, l), None);
        assert_eq!(crossed(999_999, 1_000_000, l), Some(1));
        assert_eq!(crossed(1_000_000, 1_900_000, l), None);
        assert_eq!(crossed(1_900_000, 2_100_000, l), Some(2));
        // One call that passes two multiples says the higher, once.
        assert_eq!(crossed(900_000, 3_100_000, l), Some(3));
        assert_eq!(crossed(5, 10, 0), None);
    }
}
