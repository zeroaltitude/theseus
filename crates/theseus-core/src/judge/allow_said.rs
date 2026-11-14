//! `allow_said.v1` (chat-acts-are-tools, S43-1): one special-case question
//! behind `chat.allow_held_call` — *does the operator's latest message say to
//! run this held call as it was first proposed?*
//!
//! The shape is [`JudgeService::judge_seen`]'s (`judge/people/seen.rs`), the
//! one other place in the tree that asks a single question outside a wired
//! decision point: one `Ask`, the day's budget reserved and settled, the
//! judgment scoped to this pack. Why it exists, what band it reads, and **why
//! it fails open** are `chat_acts::judged`, which owns the decision; this is
//! only the call.

use theseus_judge::builders::{AllowSaidInput, Input};
use theseus_judge::{Ask, DecisionPoint, Judge, Judgment, Outcome, Urgency};

use super::{spend, JudgeService, ScrubWith};
use crate::config::PackMode;

/// The pack.
pub const PACK: &str = "allow_said.v1";
/// Its judgments' scope, and their labels'.
pub const SCOPE: &str = "judge:allow_said";
/// The one question.
pub const SAYS_RUN: &str = "says_run";

/// How long the turn waits for it. The turn is blocked on this call, and a
/// hold released a second late is a hold released: past this the answer is no
/// answer and the model's own call stands (`chat_acts::judged`).
const DEADLINE_MS: u64 = 4_000;

impl JudgeService {
    /// The one call. `None` when none was asked — the pack off, the client not
    /// built, the day's budget spent, or the call shed — and **every `None` is
    /// a call that stands**, never one that is refused.
    pub(crate) async fn judge_allow_said(
        &self,
        session: &str,
        input: AllowSaidInput,
    ) -> Option<Judgment> {
        if self.mode_for(PACK, session).mode == PackMode::Off {
            return None;
        }
        let pack = theseus_judge::pack::by_name(PACK)?;
        let today = spend::local_day(theseus_protocol::now_unix_ms());
        let built = match self.built() {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"),
                    "judge: the Jev client was not built; a held call's second reading is skipped and the model's call stands");
                return None;
            }
        };
        let state = theseus_judge::prepare(
            &pack,
            &Input::AllowSaid(input),
            &ScrubWith(self.scrubber.clone()),
        )
        .ok()?;
        let blob = theseus_store::blocking(|| self.store.blobs().put(state.state.json.as_bytes()))
            .map_err(|e| tracing::warn!(error = %e, "judge: the allow check's state blob was not written"))
            .ok()?;
        let mut context = serde_json::json!({
            "session": session, "purpose": "live", "baseline": "none",
            "decision": "none", "blob": blob, "on_path_ms": 0,
        });
        let mode = self.ask_mode(&pack.name(), &mut context);
        let mut ask = Ask::new(pack.clone(), &state, mode, context);
        ask.id = Some(format!("jdg_{}", uuid::Uuid::now_v7().simple()));
        let asks = vec![ask];
        let need = built.judge.inner().reserve_micros(&asks).unwrap_or(0);
        if !self.reserve(&today, need, spend::Class::of_asks(&asks)) {
            return None;
        }
        let j = built
            .judge
            .judge(DecisionPoint {
                asks,
                // The turn is waiting on it, so it may take one of the permits
                // kept back for such calls, and the deadline bounds it.
                urgency: Urgency::Live {
                    deadline_ms: DEADLINE_MS,
                },
            })
            .await
            .into_iter()
            .next()?;
        let (called, failed, unknown) = match &j.outcome {
            Outcome::Answered => (true, false, false),
            Outcome::Failed { usage_unknown, .. } => (true, true, *usage_unknown),
            Outcome::Skipped { .. } => (false, false, false),
        };
        let spent = j.cost_micros.unwrap_or(if unknown { need } else { 0 });
        self.budget.settle(&today, need, spent, called, failed);
        Some(j)
    }
}
