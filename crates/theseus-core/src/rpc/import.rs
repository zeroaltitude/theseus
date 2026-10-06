//! `import.episodes`, `import.erase` and `import.list` (theseus-0lrr.6;
//! `crate::import`): the operator's past history, brought in a batch at a
//! time, erased by tag, and listed.
//!
//! - **The import and the erase are the owner's**, from a private place
//!   (`judge_act(Act::Import)`), and the CLI refuses them inside a job
//!   (`OPERATORS`): a job that could import could plant memories, and one
//!   that could erase could take the owner's away.
//! - **Off the serving workers.** Each batch's parse and frame, and the
//!   erase's walk, run on the blocking pool; the store's lock is the
//!   writer's alone, a frame at a time.
//! - **The index catches up.** After an erase's frames, a running tender is
//!   asked to forget the nodes at once (`index.forget`); either way, its
//!   follower meets each tombstone, which has nothing to index, and drops
//!   the node, so a rebuild from the WAL leaves them out too.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use theseus_protocol::error_code;
use theseus_protocol::import::{ImportEpisodesParams, ImportEraseParams};
use theseus_protocol::index::{self, IndexForgetParams, IndexForgetResult};
use theseus_protocol::method;

use super::server::{parse, Conn, RpcFailure};
use super::{Act, Core};
use crate::approval::Refusal;
use crate::import::write;

/// How long an erase waits for the tender's forget: it rewrites vector
/// files, so longer than a query.
const FORGET_DEADLINE: Duration = Duration::from_secs(70);

/// Whether `rpc_prefixed` routes `name`: the ladder's and the import's.
pub(super) fn prefixed(name: &str) -> bool {
    name.starts_with("pack.") || name.starts_with("import.")
}

impl Core {
    /// The ladder's methods (`pack.*`) and the import's (`import.*`), from
    /// one arm of `dispatch`, which stays within clippy's length that way.
    pub(super) async fn rpc_prefixed(
        self: Arc<Self>,
        name: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        if name.starts_with("pack.") {
            return self.rpc_packs(name, params, conn);
        }
        self.rpc_import(name, params, conn).await
    }

    async fn rpc_import(
        self: Arc<Self>,
        name: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        let out = match name {
            method::IMPORT_LIST => {
                let core = self.clone();
                let r = blocking(move || write::list(&core.store)).await?;
                serde_json::to_value(r)
            }
            method::IMPORT_EPISODES => {
                let p: ImportEpisodesParams = parse(params)?;
                let what = format!("a batch of {} lines of {}", p.lines.len(), p.file);
                let by = self.import_judged(conn, method::IMPORT_EPISODES, &what)?;
                let core = self.clone();
                let r = blocking(move || write::import_batch(&core.store, &p, &by)).await?;
                serde_json::to_value(r)
            }
            method::IMPORT_ERASE => {
                let p: ImportEraseParams = parse(params)?;
                let what = format!("the erase of {}", p.tag);
                let by = self.import_judged(conn, method::IMPORT_ERASE, &what)?;
                let core = self.clone();
                let (tag, why) = (p.tag.clone(), p.why.clone());
                let e =
                    blocking(move || write::erase(&core.store, &tag, why.as_deref(), &by)).await?;
                let mut r = e.result;
                r.index = self.forget(e.nodes).await;
                serde_json::to_value(r)
            }
            other => {
                return Err(RpcFailure::new(
                    error_code::METHOD_NOT_FOUND,
                    format!("unknown method {other:?}"),
                ))
            }
        };
        out.map_err(|e| RpcFailure::invalid(e.into()))
    }

    /// The owner's act, from a private place, or its refusal: who it is.
    fn import_judged(
        &self,
        conn: Conn<'_>,
        method: &'static str,
        what: &str,
    ) -> Result<String, RpcFailure> {
        let who = conn.answerer(None, None);
        let by = who.label.clone();
        self.judge_act(&who, Act::Import { method, what })
            .map_err(|e| match e.downcast::<Refusal>() {
                Ok(r) => RpcFailure {
                    code: error_code::REFUSED,
                    message: format!(
                        "{what} from {} does not count: {}. Nothing was written.",
                        r.who, r.why
                    ),
                    data: serde_json::json!({"who": r.who, "via": r.via, "why": r.why}),
                },
                Err(e) => RpcFailure::invalid(e),
            })?;
        Ok(by)
    }

    /// Ask a running tender to forget `nodes` now, and say what it did. A
    /// tender that is off, down or slow is said, and its follower drops the
    /// nodes as it reads their tombstones.
    async fn forget(&self, nodes: Vec<String>) -> String {
        let later = "its follower drops them as it reads their tombstones";
        if nodes.is_empty() {
            return "nothing to forget".into();
        }
        let running = self.index.status().is_some_and(|t| t.state == "running");
        if !running {
            return format!("not asked (no tender runs): {later}");
        }
        let p = IndexForgetParams {
            nodes,
            texts: Vec::new(),
        };
        let socket = self.index.socket();
        match crate::tender::call::<IndexForgetResult>(
            &socket,
            index::method::FORGET,
            p,
            FORGET_DEADLINE,
        )
        .await
        {
            Ok(r) => format!(
                "forgot {} ({} chunks, {} vectors)",
                crate::narrative::count(r.nodes, "node", "nodes"),
                r.chunks,
                r.vectors_dropped
            ),
            Err(e) => format!("the tender's forget did not answer ({e}): {later}"),
        }
    }
}

/// Run `f` on the blocking pool; its error is the method's.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, RpcFailure> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| RpcFailure::new(error_code::INTERNAL, e.to_string()))?
        .map_err(|e| RpcFailure::new(error_code::INTERNAL, format!("{e:#}")))
}
