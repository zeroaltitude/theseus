//! `task_words.v1` (issue 44, decision 6): one question of one message of
//! the operator's, *what does it ask of the one task it addresses?* The
//! shape is `allow_said`'s: one `Ask`, the day's budget reserved and settled,
//! the judgment scoped to this pack. When it is asked, and what is made of
//! the answer, is `task_graph::judged`, which owns the decision; this is
//! only the call, and the read of the classify pack's `addressed_task` answer it
//! is conditioned on.

use theseus_judge::builders::{Input, TaskWordsInput};
use theseus_judge::{Ask, DecisionPoint, Judge, Judgment, Outcome, Urgency};

use super::{spend, JudgeService, ScrubWith};
use crate::config::PackMode;

/// The pack.
pub const PACK: &str = "task_words.v1";
/// The one question.
pub const CHANGE: &str = "change";

/// How long the call waits. The turn is blocked on it; past this there is no
/// answer, and no answer is the card.
const DEADLINE_MS: u64 = 4_000;

impl JudgeService {
    /// The one call, and whether the judge was shown less than the whole
    /// message or task (a field clipped or left out). `None` when none was
    /// asked (the pack off, the client not built, the day's budget spent, the
    /// call shed), and **every `None` is the card**: nothing applies without
    /// an answer.
    pub(crate) async fn judge_task_words(
        &self,
        session: &str,
        node: &str,
        task: (&str, u64),
        input: TaskWordsInput,
    ) -> Option<(Judgment, bool)> {
        if self.mode_for(PACK, session).mode == PackMode::Off {
            return None;
        }
        let pack = theseus_judge::pack::by_name(PACK)?;
        let today = spend::local_day(theseus_protocol::now_unix_ms());
        let built = self.built().ok()?;
        let short = input.clipped();
        let state = theseus_judge::prepare(
            &pack,
            &Input::TaskWords(input),
            &ScrubWith(self.scrubber.clone()),
        )
        .ok()?;
        let clipped = short || !state.state.truncated.is_empty() || !state.state.dropped.is_empty();
        let blob = theseus_store::blocking(|| self.store.blobs().put(state.state.json.as_bytes()))
            .map_err(
                |e| tracing::warn!(error = %e, "judge: the task words state blob was not written"),
            )
            .ok()?;
        let mut context = serde_json::json!({
            "session": session, "node": node, "task": task.0, "base_version": task.1,
            "purpose": "live", "baseline": "none", "decision": "none", "blob": blob,
            "on_path_ms": 0,
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
        Some((j, clipped))
    }

    /// The classify pack's `addressed_task` answer for the message `node`: from
    /// the judgments landed and not yet written, else from the ledger. None
    /// when neither holds one, which is the card.
    pub(crate) fn addressed_task(&self, session: &str, node: &str) -> Option<String> {
        let landed = self
            .queue
            .find(|j| super::inbound::reads_as_classify(&j.pack) && j.context["node"] == node);
        match landed {
            Some(j) => crate::task_graph::joined::addressed_of(&j),
            None => crate::task_graph::joined::judged_task(&self.store, session, node),
        }
    }
}
