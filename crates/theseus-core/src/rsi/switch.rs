//! The kill switch (theseus-pw1q.2): `self.halt`, anyone's, and
//! `self.resume`, the owner's alone, from any place. Each move is one frame,
//! the switch's META record (`super::SWITCH_KEY`) and its row, and then the
//! cached switch, so the next [`super::gate`] hears it at once.

use anyhow::Result;
use theseus_protocol::rsi::{SelfHaltParams, SelfResumeParams, SelfState, SelfSwitchResult};

use super::{Ctx, SwitchRecord};
use crate::approval::Answerer;
use crate::fact::rsi::{SelfHalted, SelfResumed};
use crate::rpc::{Act, Core};

impl Core {
    /// What [`super::gate`] reads here.
    pub fn self_ctx(&self) -> Ctx<'_> {
        Ctx {
            mode: self.cfg.self_improve.mode,
            switch: &self.rsi,
            store: &self.store,
        }
    }

    /// The mode, the switch, and the gate, as every surface shows them.
    pub fn self_state(&self) -> SelfState {
        super::state(&self.self_ctx())
    }

    /// Halt every self step, by `who`, from wherever it came. Idempotent: a
    /// halted switch stays as it is and nothing is written; a store never
    /// resumed is written halted, with the words, so the log says who.
    pub fn self_halt(&self, p: &SelfHaltParams, who: &Answerer) -> Result<SelfSwitchResult> {
        let _move = self
            .rsi
            .writes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // A switch that cannot be read is halted anyway, by this halt.
        if matches!(self.rsi.current(&self.store), Ok(Some(r)) if r.halted) {
            return Ok(SelfSwitchResult {
                state: self.self_state(),
                changed: false,
            });
        }
        let rec = SwitchRecord {
            halted: true,
            at_ms: theseus_protocol::now_unix_ms(),
            by: who.who(),
            via: who.via(),
            why: p
                .why
                .as_deref()
                .map(str::trim)
                .filter(|w| !w.is_empty())
                .map(str::to_string),
        };
        let fact = SelfHalted {
            by: &rec.by,
            place: &rec.via,
            why: rec.why.as_deref(),
        };
        self.store
            .append(&[crate::fact::row(&fact, None, None)?, rec.record()?])?;
        self.rsi.set(rec.clone());
        self.rec(None).announce(&fact);
        Ok(SelfSwitchResult {
            state: self.self_state(),
            changed: true,
        })
    }

    /// Release the switch: only the owner (the CLI, the web UI, or an author
    /// holding an owner handle, from any place), and never from a job's shell (`judge_act(Act::SelfResume)`, whose refusal is an
    /// `approval.refused` row). A released switch stays as it is.
    pub fn self_resume(&self, p: &SelfResumeParams, who: &Answerer) -> Result<SelfSwitchResult> {
        let _move = self
            .rsi
            .writes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let from_job = p
            .from_job
            .as_deref()
            .map(str::trim)
            .filter(|j| !j.is_empty());
        self.judge_act(who, Act::SelfResume { from_job })?;
        if matches!(self.rsi.current(&self.store)?, Some(r) if !r.halted) {
            return Ok(SelfSwitchResult {
                state: self.self_state(),
                changed: false,
            });
        }
        let rec = SwitchRecord {
            halted: false,
            at_ms: theseus_protocol::now_unix_ms(),
            by: who.who(),
            via: who.via(),
            why: None,
        };
        let fact = SelfResumed {
            by: &rec.by,
            place: &rec.via,
        };
        self.store
            .append(&[crate::fact::row(&fact, None, None)?, rec.record()?])?;
        self.rsi.set(rec.clone());
        self.rec(None).announce(&fact);
        Ok(SelfSwitchResult {
            state: self.self_state(),
            changed: true,
        })
    }
}
