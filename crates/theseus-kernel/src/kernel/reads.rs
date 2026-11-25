//! The kernel's reads: executions and actions by range, open or settled, their counts, their positions in the log,
//! the quarantined completions, and the kernel's stats. None of them writes a frame.

use super::*;

impl Kernel {
    /// Every execution: O(all), for the listings that show them all.
    pub fn executions(&self) -> Result<Vec<Execution>> {
        self.store
            .latest_of_kind(kinds::EXECUTION)?
            .iter()
            .map(|r| self.read_execution(r))
            .collect()
    }

    /// Every execution that has not ended, in id order: parked
    /// conversations too. A reader that needs less asks by its own terms
    /// (`maybe_runnable`, `executions_by`).
    pub fn open_executions(&self) -> Result<Vec<Execution>> {
        self.executions_by(&OPEN_EXECUTIONS.map(|s| terms::one(&terms::state(s))))
    }

    /// What the driver's tick and the reconcile look at (theseus-lv2): the
    /// queued executions, and the waiting ones a due time or a task's report
    /// may wake (`wakes::due_now`'s cases), in id order. Never a parked one.
    pub fn maybe_runnable(&self) -> Result<Vec<Execution>> {
        self.executions_by(&[
            terms::one(&terms::state(ExecState::Queued)),
            terms::one("due"),
        ])
    }

    /// The executions with a term in any of `ranges` (`kernel::terms`), in
    /// id order (theseus-lv2).
    pub fn executions_by(&self, ranges: &[(String, String)]) -> Result<Vec<Execution>> {
        self.records_by(kinds::EXECUTION, ranges)?
            .iter()
            .map(|r| self.read_execution(r))
            .collect()
    }

    /// Every execution that decodes, and how many records did not (theseus-q6nt): for a listing,
    /// which one bad record must not fail. The count is the caller's to say.
    pub fn executions_readable(&self) -> Result<(Vec<Execution>, u64)> {
        let recs = self.store.latest_of_kind(kinds::EXECUTION)?;
        Ok(
            self.decode_executions_skipping(
                recs.iter().map(|r| (r.position, r.payload.as_slice())),
            ),
        )
    }

    /// `records` (a position and a payload each) decoded as `executions` decodes them, leaving
    /// out, and counting, each that does not.
    pub fn decode_executions_skipping<'a>(
        &self,
        records: impl IntoIterator<Item = (u64, &'a [u8])>,
    ) -> (Vec<Execution>, u64) {
        let (mut out, mut skipped) = (Vec::new(), 0u64);
        for (position, payload) in records {
            match self.decode_execution(payload) {
                Ok(e) => out.push(e),
                Err(_) => {
                    skipped += 1;
                    tracing::warn!(
                        position,
                        "an execution record did not decode; it is left out of the listing"
                    );
                }
            }
        }
        (out, skipped)
    }

    pub fn actions(&self) -> Result<Vec<Action>> {
        decode_all(&self.store.latest_of_kind(kinds::ACTION)?)
    }

    /// Every action not settled, in id order: O(open), from the store's
    /// terms (theseus-lv2).
    pub fn open_actions(&self) -> Result<Vec<Action>> {
        self.actions_by(&OPEN_ACTIONS.map(|s| terms::one(&terms::action_state(s))))
    }

    /// The actions with a term in any of `ranges`, in id order.
    pub fn actions_by(&self, ranges: &[(String, String)]) -> Result<Vec<Action>> {
        decode_all(&self.records_by(kinds::ACTION, ranges)?)
    }

    /// An execution's actions that are not settled (theseus-2qt), in id
    /// order: its own term, never a read of every action.
    pub fn unsettled_actions(&self, execution_id: &str) -> Result<Vec<Action>> {
        Ok(self
            .actions_by(&[terms::one(&terms::unsettled_of(execution_id))])?
            .into_iter()
            .filter(|a| a.execution_id == execution_id && !a.state.is_settled())
            .collect())
    }

    /// The latest records of `kind` with a term in any of `ranges`, each
    /// once, in key order (theseus-lv2): from the store's terms, or, from a
    /// store that keeps none, every record of the kind, by the same terms.
    fn records_by(
        &self,
        kind: theseus_store::RecordKind,
        ranges: &[(String, String)],
    ) -> Result<Vec<Record>> {
        let mut by_key: BTreeMap<String, Record> = BTreeMap::new();
        for (lo, hi) in ranges {
            let Some(found) = self.store.latest_by_terms(kind, lo, hi)? else {
                return Ok(self
                    .store
                    .latest_of_kind(kind)?
                    .into_iter()
                    .filter(|r| terms::any_in(&terms::of(kind, &r.payload), ranges))
                    .collect());
            };
            for r in found {
                if let Some(k) = r.key.clone() {
                    by_key.insert(k, r);
                }
            }
        }
        Ok(by_key.into_values().collect())
    }

    /// How many latest records of `kind` have each of `wanted`'s terms
    /// (theseus-lv2): counted in the store's terms, or, from a store that
    /// keeps none, over every record of the kind, once.
    pub(crate) fn counts_by(
        &self,
        kind: theseus_store::RecordKind,
        wanted: &[String],
    ) -> Result<Vec<u64>> {
        let mut out = Vec::with_capacity(wanted.len());
        for t in wanted {
            let (lo, hi) = terms::one(t);
            match self.store.count_by_terms(kind, &lo, &hi)? {
                Some(n) => out.push(n),
                None => {
                    let all: Vec<Vec<String>> = self
                        .store
                        .latest_of_kind(kind)?
                        .iter()
                        .map(|r| terms::of(kind, &r.payload))
                        .collect();
                    return Ok(wanted
                        .iter()
                        .map(|t| all.iter().filter(|ts| ts.contains(t)).count() as u64)
                        .collect());
                }
            }
        }
        Ok(out)
    }

    /// How many executions have not ended, counted by state.
    pub fn count_open_executions(&self) -> Result<u64> {
        let wanted = OPEN_EXECUTIONS.map(terms::state);
        Ok(self.counts_by(kinds::EXECUTION, &wanted)?.iter().sum())
    }

    /// How many executions there are, every state counted, an unreadable
    /// record's too (theseus-id8d): the push's total, from the store's terms.
    pub fn count_executions(&self) -> Result<u64> {
        let mut wanted: Vec<String> = EXEC_STATES.map(terms::state).to_vec();
        wanted.push(terms::UNREADABLE.to_string());
        Ok(self.counts_by(kinds::EXECUTION, &wanted)?.iter().sum())
    }

    /// The executions with a term in any of `ranges`, with the WAL position
    /// of each record, in id order (theseus-id8d: the push's seed).
    pub fn executions_by_at(&self, ranges: &[(String, String)]) -> Result<Vec<(u64, Execution)>> {
        self.records_by(kinds::EXECUTION, ranges)?
            .iter()
            .map(|r| Ok((r.position, self.read_execution(r)?)))
            .collect()
    }

    /// An execution with the WAL position of its record.
    pub fn execution_at(&self, id: &str) -> Result<Option<(u64, Execution)>> {
        match self.store.latest_by_key(kinds::EXECUTION, id)? {
            Some(r) => Ok(Some((r.position, self.read_execution(&r)?))),
            None => Ok(None),
        }
    }

    /// The newest `n` execution records, settled into their latest: each
    /// execution once, by its newest record among them, newest first, with
    /// its position (theseus-id8d: the push's seed reads the recent ones);
    /// and how many records were read, fewer than `n` once the kind is read
    /// whole.
    pub fn newest_executions_at(&self, n: usize) -> Result<(usize, Vec<(u64, Execution)>)> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        let tail = self.store.tail_of_kind(kinds::EXECUTION, n)?;
        for r in tail.iter().rev() {
            if let Some(k) = &r.key {
                if seen.insert(k.clone()) {
                    out.push((r.position, self.read_execution(r)?));
                }
            }
        }
        Ok((tail.len(), out))
    }

    /// How many actions are not settled, counted by state.
    pub fn count_open_actions(&self) -> Result<u64> {
        let wanted = OPEN_ACTIONS.map(terms::action_state);
        Ok(self.counts_by(kinds::ACTION, &wanted)?.iter().sum())
    }

    /// Completions that matched no action, in key order.
    pub fn quarantined(&self) -> Result<Vec<Completion>> {
        decode_all(
            &self
                .store
                .latest_with_prefix(kinds::COMPLETION, QUARANTINE_PREFIX)?,
        )
    }

    /// The kernel's counts, for health: each from the store's terms, so a
    /// health answer reads no execution and no action (theseus-lv2).
    pub fn stats(&self) -> Result<KernelStats> {
        let mut s = KernelStats {
            turns_held: self.held.lock().unwrap().len() as u32,
            admission_ceiling: self.cfg.admission_ceiling,
            accepting: self.is_accepting(),
            ..Default::default()
        };
        let wanted = EXEC_STATES.map(terms::state);
        for (st, n) in EXEC_STATES
            .iter()
            .zip(self.counts_by(kinds::EXECUTION, &wanted)?)
        {
            if n > 0 {
                s.executions_by_state.insert(st.as_str().to_string(), n);
            }
        }
        let waiting = s.executions_by_state.get("waiting").copied().unwrap_or(0);
        s.waiting_by_wake = self.waiting_by_wake(waiting)?;
        let wanted = ACTION_STATES.map(terms::action_state);
        for (st, n) in ACTION_STATES
            .iter()
            .zip(self.counts_by(kinds::ACTION, &wanted)?)
        {
            if n > 0 {
                s.actions_by_state.insert(st.as_str().to_string(), n);
            }
        }
        s.quarantined_completions = self
            .store
            .latest_with_prefix(kinds::COMPLETION, QUARANTINE_PREFIX)?
            .len() as u64;
        Ok(s)
    }

    /// Every execution with the WAL position of its record (theseus-in3):
    /// the push's seed reads these first, then the actions.
    pub fn executions_at(&self) -> Result<Vec<(u64, Execution)>> {
        self.store
            .latest_of_kind(kinds::EXECUTION)?
            .iter()
            .map(|r| Ok((r.position, self.read_execution(r)?)))
            .collect()
    }

    /// Every action with the WAL position of its record (theseus-in3).
    pub fn actions_at(&self) -> Result<Vec<(u64, Action)>> {
        self.store
            .latest_of_kind(kinds::ACTION)?
            .iter()
            .map(|r| Ok((r.position, r.decode()?)))
            .collect()
    }

    /// Every action not settled, with the WAL position of its record, from
    /// the store's terms (theseus-id8d): the push's seed, which reads no
    /// settled action.
    pub fn open_actions_at(&self) -> Result<Vec<(u64, Action)>> {
        let open = OPEN_ACTIONS.map(|s| terms::one(&terms::action_state(s)));
        self.records_by(kinds::ACTION, &open)?
            .iter()
            .map(|r| Ok((r.position, r.decode()?)))
            .collect()
    }

    /// The position of the newest action record, settled or not; 0 with none.
    pub fn newest_action_position(&self) -> Result<u64> {
        Ok(self
            .store
            .tail_of_kind(kinds::ACTION, 1)?
            .first()
            .map_or(0, |r| r.position))
    }

    /// An execution as stored, decoded as `executions` decodes it (a unit
    /// budget read in dollars): what the push's observer hands on. Its
    /// callers pass over a record that does not read, so a payload that is no
    /// JSON object is one more such record (`Execution::from_stored_listed`).
    pub fn decode_execution(&self, payload: &[u8]) -> Result<Execution> {
        Execution::from_stored_listed(payload, self.cfg.spend_limit_micros)
    }
}
