//! `ledger.verify` (theseus-bmkxk): the ledger's hash chain, walked on the
//! blocking pool, a page of rows at a time. A read: it writes nothing and
//! takes no session or execution lock, so it is anyone's to ask, inside a job
//! too. The walk is `theseus_store::chain_walk::verify`, which recomputes
//! every link from the stored bytes and trusts none.

use std::time::Instant;

use serde_json::Value;
use theseus_protocol::{
    error_code, LedgerBreak, LedgerHead, LedgerVerifyParams, LedgerVerifyResult,
};
use theseus_store::chain::{self, Head};
use theseus_store::chain_walk::{self, Anchor, Report};
use theseus_store::Store;

use super::server::RpcFailure;
use super::Core;

pub(crate) fn head_of(h: &Head) -> LedgerHead {
    LedgerHead {
        seq: h.link.seq,
        digest: chain::hex(&h.link.digest),
        position: h.position,
    }
}

/// `SEQ:DIGEST`, the digest in hex (8 characters at least).
pub fn parse_anchor(text: &str) -> Result<Anchor, String> {
    let bad = || {
        format!("an anchor is SEQ:DIGEST (the digest in hex, 8 characters or more), not {text:?}")
    };
    let (seq, digest) = text.split_once(':').ok_or_else(bad)?;
    let seq = seq.trim().parse::<u64>().map_err(|_| bad())?;
    let digest = digest.trim().to_ascii_lowercase();
    if digest.len() < 8 || digest.len() > 64 || !digest.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(bad());
    }
    Ok(Anchor { seq, digest })
}

fn result_of(r: Report, took_ms: u64) -> LedgerVerifyResult {
    let broken = r.broken.as_ref().map(|b| LedgerBreak {
        position: b.position,
        at_unix_ms: b.at_unix_ms,
        kind: b.kind.clone(),
        seq: b.seq,
        fault: b.fault.word().to_string(),
        missing: match b.fault {
            chain_walk::Fault::Removed { missing } => Some(missing),
            _ => None,
        },
        detail: b.detail.clone(),
    });
    LedgerVerifyResult {
        ok: broken.is_none(),
        rows: r.rows,
        since_ms: r.since_ms,
        head: r.head.as_ref().map(head_of),
        unchained: r.unchained,
        history_checked: r.history_checked,
        broken,
        took_ms,
        building: false,
    }
}

impl Core {
    pub(crate) async fn ledger_verify(
        self: &std::sync::Arc<Self>,
        params: Value,
    ) -> Result<LedgerVerifyResult, RpcFailure> {
        // Its params are optional: none walks the whole ledger.
        let p: LedgerVerifyParams = match params {
            Value::Null => LedgerVerifyParams::default(),
            v => serde_json::from_value(v).map_err(|e| RpcFailure::invalid(e.into()))?,
        };
        let anchor = match p.anchor.as_deref() {
            Some(a) => {
                Some(parse_anchor(a).map_err(|e| RpcFailure::new(error_code::INVALID_PARAMS, e))?)
            }
            None => None,
        };
        if p.head_only {
            let head = self
                .store
                .inner()
                .ledger_head()
                .map_err(RpcFailure::invalid)?;
            return Ok(LedgerVerifyResult {
                ok: true,
                head: head.as_ref().map(head_of),
                ..Default::default()
            });
        }
        let core = self.clone();
        let began = Instant::now();
        let report = tokio::task::spawn_blocking(move || {
            chain_walk::verify(core.store.inner().as_ref(), p.since, anchor)
        })
        .await
        .map_err(|e| RpcFailure::new(error_code::INTERNAL, e.to_string()))?
        .map_err(RpcFailure::invalid)?;
        Ok(result_of(report, began.elapsed().as_millis() as u64))
    }

    /// The chain's head, short, for health: a claim read from the writer's
    /// own state, never a walk.
    pub(crate) fn ledger_head_short(&self) -> Option<String> {
        let h = self.store.inner().ledger_head().ok().flatten()?;
        Some(head_of(&h).short())
    }
}
