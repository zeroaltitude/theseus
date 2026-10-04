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
//! - [`group`]: a group's record, its hands' actions, and its steps as they
//!   settle.
//! - [`poller`]: the completion queue's long poll, after serving and only
//!   while a hand is outstanding; each message checked, then settled on the
//!   kernel's path, or quarantined.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use theseus_tools::AwsBinding;
use tokio::sync::Notify;

use crate::aws::Account;

pub mod envelope;
pub mod group;
pub mod hand;
pub mod launch;
pub mod poller;
pub mod tool;

pub use launch::{HAND, RUN};

#[cfg(test)]
mod tests_hand;
#[cfg(test)]
mod tests_hands;

/// The hands' state in the daemon: where each account's hands run, read
/// once, and the poller's wake. Nothing in it is the record: the groups and
/// their hands are in the store.
#[derive(Default)]
pub struct Hands {
    envs: Mutex<BTreeMap<(String, String), launch::HandsEnv>>,
    /// Told when a group launches, so an idle poller polls.
    pub(crate) wake: Notify,
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

    /// A group launched: the poller polls.
    pub fn launched(&self) {
        self.wake.notify_one();
    }
}
