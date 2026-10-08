//! Spend paid outside a turn's calls (rows 77 and 78, 45b): a speech call.
//!
//! A provider call reserves before it runs and settles at its real cost, as
//! an action inside a turn. A speech call is no turn's: an utterance is
//! transcribed before its turn starts, and a reply is synthesized after the
//! reply ended its turn, so there is no turn to reserve under. Its cost is
//! booked after it returns, in full and in one frame with its row, as a late
//! completion's real cost is: a real cost is never hidden, even past the
//! limit. Whether a call fits is asked before it is made
//! (`theseus_core::voice`), so a session at its limit makes none.

use anyhow::Result;
use serde_json::Value;
use theseus_protocol::LedgerKind;

use crate::kernel::{exec_record, Kernel, KernelError};
use crate::types::*;

impl Kernel {
    /// Book `cost` to `execution_id`'s spend, with its row (`kind`, `data`
    /// and the cost) in the same frame, and carry it to a task's parent as a
    /// task's own spend is. A terminal execution books it too: the money was
    /// spent.
    pub fn book_spend(
        &self,
        execution_id: &str,
        cost: Micros,
        kind: LedgerKind,
        mut data: Value,
    ) -> Result<Execution> {
        self.require_accepting()?;
        let _w = self.lock_family(execution_id)?;
        let mut e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.into()))?;
        let spent_before = e.budget.spent_micros;
        e.budget.spent_micros = e.budget.spent_micros.saturating_add(cost);
        e.updated_at_ms = self.now_ms();
        if let Value::Object(m) = &mut data {
            m.insert("execution_id".into(), e.id.clone().into());
            m.insert("cost_usd".into(), micros_to_usd(cost).into());
            m.insert(
                "spent_usd".into(),
                micros_to_usd(e.budget.spent_micros).into(),
            );
        }
        let mut frame = vec![exec_record(&e)?];
        self.carry_to_parent(&e, spent_before, &mut frame)?;
        frame.push(self.ledger(kind, Some(&e.session_id), data)?);
        self.commit(&frame)?;
        // Model spend, booked on the day in full (theseus-kp20).
        self.day_after([crate::day_ceiling::Effect::Book(cost)]);
        Ok(e)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use theseus_protocol::LedgerKind;

    use crate::tests::{auth, rows, world};
    use crate::types::*;

    /// A cost paid outside a turn is booked to its execution after it was
    /// paid, with its row in the same frame, in full even past the limit; a
    /// waiting execution stays waiting.
    #[test]
    fn spend_outside_a_turn_is_booked_with_its_row() {
        let w = world();
        let e = w
            .kernel
            .open_execution(
                &new_id("ses"),
                SessionKind::Conversation,
                auth(),
                Some(1_000),
                None,
            )
            .unwrap();
        let after = w
            .kernel
            .book_spend(
                &e.id,
                215,
                LedgerKind::SpeechTranscribed,
                json!({"model": "nova-3"}),
            )
            .unwrap();
        assert_eq!(after.budget.spent_micros, 215);
        assert_eq!(after.state, ExecState::Waiting);
        let row = &rows(&w, &e.session_id, "speech.transcribed")[0];
        assert_eq!(row["model"], "nova-3");
        assert_eq!(row["cost_usd"], 0.000215);
        assert_eq!(row["execution_id"], e.id.as_str());
        // Past the limit, in full: a real cost is never hidden.
        let over = w
            .kernel
            .book_spend(&e.id, 5_000, LedgerKind::SpeechSynthesized, json!({}))
            .unwrap();
        assert_eq!(over.budget.spent_micros, 5_215);
        assert_eq!(over.budget.available(), 0);
        let stored = w.kernel.execution(&e.id).unwrap().unwrap();
        assert_eq!(stored.budget.spent_micros, 5_215);
        assert_eq!(
            rows(&w, &e.session_id, "speech.synthesized")[0]["spent_usd"],
            0.005215
        );
        assert!(w
            .kernel
            .book_spend("exe_none", 1, LedgerKind::SpeechSynthesized, json!({}))
            .is_err());
    }
}
