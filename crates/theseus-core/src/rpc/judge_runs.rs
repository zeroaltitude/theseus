//! The owner's runs over the learning ledger (M5 25d; design §2.9):
//! `judge.replay`, `judge.audit`, and `judge.backfill`. Each is judged as
//! an approval is (`judge_act(Act::JudgeRun)`: the owner, from a private
//! place; the CLI refuses each inside a job), and runs on a thread of its
//! own at low priority (`learning::tender::on_low_thread`). The runs
//! themselves are `learning::replay`, `learning::audit`, and
//! `learning::backfill`. Consolidation (M6 31b, `memory.consolidate`) is
//! routed with them: it spends money and sends sessions' text to a profile,
//! so it is the owner's run too (`consolidate::run`).

use serde_json::{json, Value};
use theseus_protocol::error_code;

use super::server::{parse, Conn, RpcFailure};
use super::Core;
use crate::approval::Refusal;

/// The runs' methods, which `server.rs` routes to [`Core::rpc_judge_run`].
pub(super) const RUNS: [&str; 4] = [
    theseus_protocol::method::JUDGE_REPLAY,
    theseus_protocol::method::JUDGE_AUDIT,
    theseus_protocol::method::JUDGE_BACKFILL,
    theseus_protocol::method::MEMORY_CONSOLIDATE,
];

/// A run's error on the wire: a refusal is `REFUSED`, with who, through
/// what, and why; anything else the caller's to fix.
pub(super) fn failure(e: anyhow::Error) -> RpcFailure {
    match e.downcast::<Refusal>() {
        Ok(r) => RpcFailure {
            code: error_code::REFUSED,
            message: format!(
                "a run from {} does not count: {}. Nothing was sent or written.",
                r.who, r.why
            ),
            data: json!({"who": r.who, "via": r.via, "why": r.why}),
        },
        Err(e) => RpcFailure::invalid(e),
    }
}

impl Core {
    /// One of the owner's runs over the ledger (M5 25d), by its method.
    pub(super) async fn rpc_judge_run(
        self: &std::sync::Arc<Self>,
        method: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        let who = conn.answerer(None, None);
        let r = match method {
            theseus_protocol::method::JUDGE_REPLAY => self
                .judge_replay(parse(params)?, who)
                .await
                .map(serde_json::to_value),
            theseus_protocol::method::JUDGE_AUDIT => self
                .judge_audit(parse(params)?, who)
                .await
                .map(serde_json::to_value),
            theseus_protocol::method::MEMORY_CONSOLIDATE => {
                let params = if params.is_null() { json!({}) } else { params };
                self.memory_consolidate(parse(params)?, who)
                    .await
                    .map(serde_json::to_value)
            }
            _ => self
                .judge_backfill(parse(params)?, who)
                .await
                .map(serde_json::to_value),
        };
        Ok(r.map_err(failure)?.unwrap_or(Value::Null))
    }
}
