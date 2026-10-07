//! Jev's connections, warm from serving on (theseus-ddbi). A fresh daemon's
//! first message paid Jev's connection setup (DNS, TCP and TLS) on
//! route.v1's request: two of a reviewer's nine verdicts missed the turn's
//! bound on a first message. So, with the judge on and a pack a message
//! waits on (route.v1, the inbound point's others, a rerank), the client is
//! built and two connections opened once the socket serves, never on the
//! start path (FAST: no network before serving), and kept: a keeper on
//! tokio's timer uses them again after every [`KEEP_WARM`] of silence, so
//! the pool's idle time ([`POOL_IDLE`]) never closes them. Each warm-up is a
//! `HEAD` of the judge's path: no key, nothing billed. Any answer of Jev's,
//! a call's included, resets the keeper's clock, so a conversation sends
//! no warm-up at all.
//!
//! [`POOL_IDLE`]: theseus_judge::client::POOL_IDLE

use std::sync::{Arc, Weak};
use std::time::Duration;

use theseus_judge::client::KEEP_WARM;

use super::{inbound, rerank, JudgeService};

/// The connections kept: route.v1's request and the inbound batch go out
/// at once.
pub const CONNECTIONS: usize = 2;

impl JudgeService {
    /// Whether a person's message waits on Jev, or rides beside a call that
    /// does: the judge on, and an inbound pack or the rerank on.
    fn message_asks_jev(&self) -> bool {
        self.cfg.enabled
            && (inbound::PACKS.iter().any(|p| self.pack_on(p)) || self.pack_on(rerank::RERANK_PACK))
    }

    /// The keeper, every `every` of Jev's silence, from now on: the daemon
    /// calls it after serving ([`crate::rpc::Core::warm_judge`]). Nothing
    /// with the judge off.
    pub fn keep_warm(&self, every: Duration) {
        if !self.cfg.enabled {
            return;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        rt.spawn(keep(self.me.clone(), every));
    }
}

impl crate::rpc::Core {
    /// Jev's connections opened and kept warm, once the socket serves
    /// (theseus-ddbi): never on the start path.
    pub fn warm_judge(&self) {
        self.runner.judge.keep_warm(KEEP_WARM);
    }
}

/// The keeper: while the service lives, a refresh of the connections when
/// Jev has been silent `every`, then a sleep until the silence would reach
/// `every` again. A refresh that reaches nothing waits `every` before the
/// next, whatever Jev said last.
async fn keep(me: Weak<JudgeService>, every: Duration) {
    loop {
        let Some(svc) = me.upgrade() else { return };
        let asks = {
            let svc = svc.clone();
            tokio::task::spawn_blocking(move || svc.message_asks_jev())
                .await
                .unwrap_or(false)
        };
        let built = asks.then(|| svc.built().ok()).flatten();
        drop(svc);
        let next = match built {
            None => every,
            Some(built) => refresh(&built, every).await,
        };
        tokio::time::sleep(next).await;
    }
}

/// One wake of the keeper: the refresh, if Jev has been silent `every`;
/// returns how long until it next has been.
async fn refresh(built: &Arc<super::Built>, every: Duration) -> Duration {
    let client = built.jev().client();
    if client.heard_ago().is_none_or(|a| a >= every) {
        if let Some((took, warm)) = client.refresh(CONNECTIONS).await {
            tracing::debug!(
                ms = took.as_millis() as u64,
                warm,
                "judge: Jev's connections kept warm"
            );
        }
    }
    match client.heard_ago() {
        Some(a) if a < every => every - a,
        _ => every,
    }
}

#[cfg(test)]
impl JudgeService {
    /// Whether the client holds connections Jev answered on lately (a
    /// test's wait for the warm-up to land).
    pub fn jev_warm(&self) -> bool {
        self.built.get().is_some_and(|b| b.jev().client().warm())
    }
}
