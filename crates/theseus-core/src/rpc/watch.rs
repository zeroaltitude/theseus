//! The owner's verbs on watches (theseus-f2r2b): `watch.create` and
//! `watch.list`. A cancel is `wake.cancel`'s, by the same id.
//!
//! `watch.create` is the owner's act (`judge_act(Act::Watch)`: the owner, from
//! a private place; the CLI refuses it inside a job). The owner typed the
//! command, so no approval card stands in front of it, as none stands before
//! `wake.create`; and it needs no model turn. It lives in `session_id`'s
//! conversation, or in the owner's Home when none is named.

use serde_json::Value;
use theseus_protocol::{
    method, WatchCreateParams, WatchCreateResult, WatchListParams, WatchListResult,
};

use super::confirms::Act;
use super::server::{or_empty, route, Conn, RpcFailure};
use super::Core;
use crate::watch::spec;

/// The owner's verbs on watches, routed by `rpc_prefixed`.
pub(super) const OWN: [&str; 2] = [method::WATCH_CREATE, method::WATCH_LIST];

impl Core {
    pub(super) fn rpc_watches(
        &self,
        name: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        match name {
            method::WATCH_CREATE => route(params, |p| self.watch_create(p, conn)),
            _ => route(or_empty(params), |p: WatchListParams| {
                Ok(WatchListResult {
                    watches: self.watch_list(p.session_id.as_deref())?,
                })
            }),
        }
    }

    fn watch_create(
        &self,
        p: WatchCreateParams,
        conn: Conn<'_>,
    ) -> Result<WatchCreateResult, RpcFailure> {
        let who = conn.answerer(p.author.clone(), p.discord.clone());
        let what = format!("the watch \"{}\"", p.title.trim());
        self.judge_act(
            &who,
            Act::Watch {
                method: method::WATCH_CREATE,
                what: &what,
            },
        )
        .map_err(failure)?;
        let by = conn.actor(p.author.as_deref());
        let (session_id, execution_id, home) = match &p.session_id {
            Some(s) => {
                let rec = self.session(s)?;
                let Some(ex) = rec.execution_id.clone() else {
                    return Err(invalid(format!(
                        "session {s} has no conversation to hold it"
                    )));
                };
                (rec.session_id, ex, false)
            }
            None => {
                let h = self.home().map_err(RpcFailure::from)?;
                (h.session_id, h.execution_id, true)
            }
        };
        let now = self.kernel.now_ms();
        let built = spec::build(&p, now, &self.cfg.watches, self.outbox.target(&session_id))
            .map_err(invalid)?;
        let set = self
            .kernel
            .create_wake(&execution_id, None, built.new, &by)
            .map_err(RpcFailure::invalid)?;
        let ex = self
            .kernel
            .execution(&execution_id)?
            .ok_or_else(|| invalid(format!("execution {execution_id} is gone")))?;
        if set.set {
            self.watch_seed(&ex, &set.wake, now, true);
        }
        let watch = self
            .watch_info(&ex, &set.wake)
            .ok_or_else(|| invalid("the watch does not read back"))?;
        Ok(WatchCreateResult { watch, home })
    }
}

fn invalid(e: impl std::fmt::Display) -> RpcFailure {
    RpcFailure::new(theseus_protocol::error_code::INVALID_PARAMS, e.to_string())
}

/// A refused act on the wire is `REFUSED`; anything else is invalid input.
fn failure(e: anyhow::Error) -> RpcFailure {
    use theseus_protocol::error_code;
    match e.downcast::<crate::approval::Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: format!(
                "a watch from {} does not count: {}. No watch was made.",
                r.who, r.why
            ),
            data: serde_json::json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}
