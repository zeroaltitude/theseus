//! The owner's edits of a task's own execution (theseus-jid7t.2): where its
//! report goes, and when a deferred task starts. One frame each: the
//! execution's record with the records the core's caller adds (the task's
//! session, its ledger row). A task's budget and its carve are never touched:
//! a task's parent never changes.

use anyhow::Result;
use theseus_store::NewRecord;

use crate::kernel::{exec_record, Kernel, KernelError};
use crate::types::Execution;

impl Kernel {
    /// Change the execution `id` under its lock and write it, with the
    /// records `f` returns, in one frame. `f` refuses by returning an error,
    /// and then nothing is written. Answers the execution as written and
    /// what `f` answered.
    pub fn edit_task<T>(
        &self,
        id: &str,
        f: impl FnOnce(&mut Execution) -> Result<(T, Vec<NewRecord>)>,
    ) -> Result<(Execution, T)> {
        self.require_accepting()?;
        let _w = self.lock(&[id]);
        let mut e = self
            .execution(id)?
            .ok_or_else(|| KernelError::UnknownExecution(id.into()))?;
        let (out, extra) = f(&mut e)?;
        e.updated_at_ms = self.now_ms();
        let mut frame = vec![exec_record(&e)?];
        frame.extend(extra);
        self.commit(&frame)?;
        Ok((e, out))
    }
}
