//! `execution.list` (theseus-0jet): every execution as it always answered,
//! the newest `n` by birth through the index with a cursor, or only the
//! executions `ids` names. The paged forms cost the page; the parent of an
//! execution that the page does not hold is read by its id.

use theseus_kernel::Execution;
use theseus_protocol::{ExecutionListParams, ExecutionListResult};
use theseus_store::{kinds, Store as _};

use super::server::RpcFailure;
use super::Core;

/// The most executions one page holds.
const PAGE_MAX: usize = 1000;

impl Core {
    pub(crate) fn execution_list(
        &self,
        p: ExecutionListParams,
    ) -> Result<ExecutionListResult, RpcFailure> {
        let (execs, older) = if let Some(ids) = &p.ids {
            let mut found = Vec::new();
            for id in ids {
                found.extend(self.kernel.execution(id)?);
            }
            (found, None)
        } else if let Some(n) = p.n {
            self.executions_page(n.min(PAGE_MAX), p.before)?
        } else {
            (self.kernel.executions()?, None)
        };
        let pending = self.pending_by_execution(&self.kernel.pending_confirms()?, None);
        let mut sessions: std::collections::HashMap<String, Option<String>> = execs
            .iter()
            .map(|e| (e.id.clone(), Some(e.session_id.clone())))
            .collect();
        let mut executions = Vec::with_capacity(execs.len());
        for e in &execs {
            let parent = match e.parent.as_deref() {
                None => None,
                Some(p) => match sessions.get(p) {
                    Some(s) => s.clone(),
                    None => {
                        let s = self.kernel.execution(p)?.map(|x| x.session_id);
                        sessions.insert(p.to_string(), s.clone());
                        s
                    }
                },
            };
            let asks = pending.get(&e.id).cloned().unwrap_or_default();
            let mut info = Self::execution_info(e);
            info.attention = Some(crate::push::view(e, asks, parent, 0, e.updated_at_ms).attention);
            executions.push(info);
        }
        Ok(ExecutionListResult { executions, older })
    }

    /// The newest `n` executions by birth, born before `before` when given,
    /// and the cursor for the page after while older ones remain. While the
    /// index's shape is built, every execution is read, the newest `n` by
    /// creation kept, and no cursor is given.
    fn executions_page(
        &self,
        n: usize,
        before: Option<u64>,
    ) -> Result<(Vec<Execution>, Option<u64>), RpcFailure> {
        let Some((born, more)) = self
            .store
            .inner()
            .newest_keys(kinds::EXECUTION, before, n)?
        else {
            let mut all = self.kernel.executions()?;
            all.sort_by_key(|e| std::cmp::Reverse(e.created_at_ms));
            all.truncate(n);
            return Ok((all, None));
        };
        let older = born.last().map(|(b, _)| *b).filter(|_| more);
        let execs = born
            .iter()
            .map(|(_, r)| self.kernel.decode_execution(&r.payload))
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok((execs, older))
    }
}
