//! Hands (AWS design §3.3; step 40 part 1, theseus-mgw.6): jobs that run in
//! AWS and keep the job wrapper's contract, detached, durable, and
//! cancellable.
//!
//! - [`envelope`]: the per-dispatch key, the spec a hand is given, and the
//!   signed completion it sends home.
//! - [`hand`]: the `hand` role of the daemon's binary (`theseusd hand`), the
//!   wrapper inside the hand image.
//! - [`launch`] and [`tool`]: `aws.hands.run`, its backend, and each hand's
//!   launch on Lambda or Fargate.
//! - [`network`]: the hands' network, the stack's own VPC or an existing
//!   one the config names, which Theseus uses and never changes.
//! - [`grid`]: a group as its surfaces show it: `hands.list`'s cells and
//!   money, and Discord's one line per group, edited in place.
//! - [`group`]: a group's record, its hands' actions, and its steps as they
//!   settle.
//! - [`cancel`]: a running hand's stop, by its backend (Fargate's
//!   `StopTask`, verified STOPPED; Lambda's none), and a group's.
//! - [`overdue`]: a hand past its deadline, asked about before it is
//!   called unknown, and a hand the TTL reaper stopped.
//! - [`quota`]: the room Fargate's vCPU quota or Lambda's concurrency
//!   leaves a group, read once an hour; a bigger group launches in waves.
//! - [`watch`]: health's hands block, the hour's meter and its alert, and
//!   the TTL reaper's failures.
//! - [`poller`]: the completion queue's long poll, after serving and only
//!   while a hand is outstanding; each message checked, then settled on the
//!   kernel's path, or quarantined.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use theseus_tools::AwsBinding;
use tokio::sync::Notify;

use crate::aws::Account;

pub mod cancel;
pub mod envelope;
pub mod grid;
pub mod group;
pub mod hand;
pub mod launch;
pub mod network;
pub mod overdue;
pub mod poller;
pub mod quota;
pub mod tool;
pub mod watch;

pub use launch::{HAND, RUN};

#[cfg(test)]
mod tests_hand;
#[cfg(test)]
mod tests_hands;
#[cfg(test)]
mod tests_part2;

/// The hands' state in the daemon: where each account's hands run, read
/// once, and the poller's wake. Nothing in it is the record: the groups and
/// their hands are in the store.
#[derive(Default)]
pub struct Hands {
    envs: Mutex<BTreeMap<(String, String), launch::HandsEnv>>,
    /// Told when a group launches, so an idle poller polls.
    pub(crate) wake: Notify,
    /// A group that did not fit its session's budget, by execution, until
    /// its turn asks the budget question (`take_over_budget`): the turn's
    /// own hand-off, never the record.
    over_budget: Mutex<BTreeMap<String, OverBudget>>,
    /// The TTL reaper's failures read off the queue, for health.
    pub reaper: watch::Reaper,
    /// Each account's quotas, read before a wave and kept an hour.
    pub quotas: quota::Quotas,
    /// The line each group last posted to its place.
    pub lines: grid::Lines,
}

/// What a group needed of its session's budget, and what it had.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverBudget {
    pub needed: u64,
    pub available: u64,
    pub spent: u64,
    pub limit: u64,
}

impl Hands {
    /// Where `account`'s hands run in `region`: its stacks' outputs, read at
    /// the first call and kept.
    pub async fn env(
        &self,
        account: &Arc<Account>,
        region: &str,
        binding: Option<&AwsBinding>,
    ) -> Result<launch::HandsEnv, String> {
        let k = (account.id.clone(), region.to_string());
        if let Some(e) = self.envs.lock().unwrap().get(&k) {
            return Ok(e.clone());
        }
        let e = launch::discover(account, binding, region).await?;
        self.envs.lock().unwrap().insert(k, e.clone());
        Ok(e)
    }

    /// A group of `execution_id`'s did not fit its budget: its turn asks.
    pub fn over_budget(&self, execution_id: &str, o: OverBudget) {
        self.over_budget
            .lock()
            .unwrap()
            .insert(execution_id.to_string(), o);
    }

    /// The budget question a group of `execution_id`'s left its turn to ask
    /// (step 40 part 2: as a model call's would).
    pub fn take_over_budget(&self, execution_id: &str) -> Option<OverBudget> {
        self.over_budget.lock().unwrap().remove(execution_id)
    }

    /// A group launched: the poller polls.
    pub fn launched(&self) {
        self.wake.notify_one();
    }
}
