//! `import.episodes`, `import.erase`, `import.list` and `import.topics`
//! (theseus-0lrr.6, theseus-anh3; `crate::import`): the operator's past
//! history, brought in a batch at a time, erased by tag, listed, and its
//! topic labels made the ontology's topics and memberships.
//!
//! - **The import, the erase and the topics are the owner's**, from a private place
//!   (`judge_act(Act::Import)`), and the CLI refuses them inside a job
//!   (`OPERATORS`): a job that could import could plant memories, and one
//!   that could erase could take the owner's away.
//! - **Off the serving workers.** Each batch's parse and frame, and the
//!   erase's walk, run on the blocking pool; the store's lock is the
//!   writer's alone, a frame at a time. The daemon's stop ends either at its
//!   next frame boundary (theseus-autz), answered with an error that names
//!   what was written and says a rerun finishes it.
//! - **The index catches up.** After an erase's frames, a running tender is
//!   asked to forget the nodes at once (`index.forget`); either way, its
//!   follower meets each tombstone, which has nothing to index, and drops
//!   the node, so a rebuild from the WAL leaves them out too.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use theseus_protocol::error_code;
use theseus_protocol::import::{
    ImportEpisodesParams, ImportEraseParams, ImportPeopleParams, ImportSessionsParams,
    ImportSessionsResult, ImportTopicsParams,
};
use theseus_protocol::index::{self, IndexForgetParams, IndexForgetResult};
use theseus_protocol::method;

use super::server::{parse, Conn, RpcFailure};
use super::{Act, Core};
use crate::approval::Refusal;
use crate::import::{catalog, people, summary_id_of, topics, write};
use crate::node::Body;

/// Why a place that is not private reads no imported text.
pub(crate) const WITHHELD: &str = "an imported session is the owner's own history, private \
     whatever place its episode names: its text goes only to a private place (the CLI, the web UI)";

/// How long an erase waits for the tender's forget: it rewrites vector
/// files, so longer than a query.
const FORGET_DEADLINE: Duration = Duration::from_secs(70);

/// Whether `rpc_prefixed` routes `name`: the ladder's, the import's, and
/// `context.explain` (theseus-7n3e), the books' (theseus-civ0), and the
/// people's two ontology methods (theseus-wy7y).
pub(super) fn prefixed(name: &str) -> bool {
    name.starts_with("pack.")
        || name.starts_with("import.")
        || name.starts_with("books.")
        || super::context::prefixed(name)
        || super::route_correct::ROUTE.contains(&name)
        || super::people::ROUTE.contains(&name)
}

impl Core {
    /// The ladder's methods (`pack.*`), the import's (`import.*`), the
    /// books' (`books.*`), and the owner's corrections of routing
    /// (`route.correct`, `route.corrections`), from one arm of `dispatch`,
    /// which stays within clippy's length that way.
    pub(super) async fn rpc_prefixed(
        self: Arc<Self>,
        name: &str,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        if name.starts_with("pack.") {
            return self.rpc_packs(name, params, conn);
        }
        if super::context::prefixed(name) {
            return self.rpc_context(params, conn).await;
        }
        if name.starts_with("books.") {
            return self.rpc_books(name, params, conn).await;
        }
        if super::route_correct::ROUTE.contains(&name) {
            return self.rpc_route(name, params, conn);
        }
        if super::people::ROUTE.contains(&name) {
            return self.rpc_ontology(name, params, conn);
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
                let b = blocking(move || {
                    let stopping = || core.outbox.stopping();
                    write::import_batch_unless(&core.store, &p, &by, stopping)
                })
                .await?;
                if b.stopped {
                    let r = &b.result;
                    return Err(stopped(
                        format!(
                            "the daemon stopped this batch between two frames: {} of its lines were \
                             read and {} episodes imported ({} frames written); send the batch again \
                             to finish it (what was written is skipped)",
                            r.read, r.imported, r.frames
                        ),
                        &b.result,
                    ));
                }
                serde_json::to_value(b.result)
            }
            method::IMPORT_ERASE => {
                let p: ImportEraseParams = parse(params)?;
                let what = format!("the erase of {}", p.tag);
                let by = self.import_judged(conn, method::IMPORT_ERASE, &what)?;
                let core = self.clone();
                let (tag, why) = (p.tag.clone(), p.why.clone());
                let e = blocking(move || {
                    let stopping = || core.outbox.stopping();
                    let mut e =
                        write::erase_unless(&core.store, &tag, why.as_deref(), &by, stopping)?;
                    // Then the topics' half: the erased sessions' memberships,
                    // and the topics nothing else uses.
                    if !e.stopped {
                        let u = topics::unassign(
                            &core.store,
                            &core.runner.ontology,
                            &tag,
                            &by,
                            stopping,
                        )?;
                        e.result.memberships = u.memberships;
                        e.result.topics = u.topics;
                        e.stopped = u.stopped;
                    }
                    // And the people the import made that nothing uses now.
                    if !e.stopped {
                        let (n, stopped) = people::unassign(
                            &core.store,
                            &core.runner.ontology,
                            &tag,
                            &by,
                            stopping,
                        )?;
                        e.result.people = n;
                        e.stopped = stopped;
                    }
                    Ok(e)
                })
                .await?;
                let mut r = e.result;
                r.index = self.forget(e.nodes).await;
                if e.stopped {
                    return Err(stopped(
                        format!(
                            "the daemon stopped the erase of {} between two frames: {} sessions \
                             ({} nodes) were erased and counted ({} frames written); run the erase \
                             again to finish the tag (what was erased is skipped)",
                            r.tag, r.sessions, r.nodes, r.frames
                        ),
                        &r,
                    ));
                }
                serde_json::to_value(r)
            }
            method::IMPORT_SESSIONS => {
                let p: ImportSessionsParams = parse(params)?;
                let private = conn.surface.reads_private();
                let core = self.clone();
                let r = blocking(move || core.import_sessions(&p, private)).await?;
                serde_json::to_value(r)
            }
            method::IMPORT_TOPICS => return self.import_topics(params, conn).await,
            method::IMPORT_PEOPLE => return self.import_people(params, conn).await,
            other => {
                return Err(RpcFailure::new(
                    error_code::METHOD_NOT_FOUND,
                    format!("unknown method {other:?}"),
                ))
            }
        };
        out.map_err(|e| RpcFailure::invalid(e.into()))
    }

    /// `import.sessions` (theseus-7n3e): the projection at the import's
    /// counts now, queried. The page's summaries are read when asked, a node
    /// each. A place that is not private reads the labels and the counts
    /// alone: no title, no summary, no place name, and no search of words.
    pub(crate) fn import_sessions(
        &self,
        p: &ImportSessionsParams,
        private: bool,
    ) -> anyhow::Result<ImportSessionsResult> {
        let t0 = std::time::Instant::now();
        let list = write::list(&self.store)?;
        let (cat, built_ms) = self.episodes.at(&self.store, &list)?;
        let mut p = p.clone();
        if !private {
            p.q = None;
        }
        let a = catalog::query(&cat, &p);
        let mut episodes = Vec::with_capacity(a.page.len());
        for i in a.page {
            let mut e = cat.episode(i);
            if !private {
                e.title = None;
                e.place_name = None;
            } else if p.summaries && e.summary && !e.erased {
                if let Some((_, n)) = self.store.get_node(&summary_id_of(&e.session_id))? {
                    if let Body::ImportedSummary { text, cites, .. } = n.body {
                        e.summary_text = Some(text);
                        e.cites = Some(cites.len() as u32);
                    }
                }
            }
            episodes.push(e);
        }
        Ok(ImportSessionsResult {
            total: a.total,
            all: cat.rows.len() as u64,
            offset: p.offset.unwrap_or(0).min(a.total),
            episodes,
            facets: a.facets,
            withheld: (!private).then(|| WITHHELD.to_string()),
            version: cat.version.clone(),
            built_ms,
            ms: t0.elapsed().as_secs_f64() * 1e3,
        })
    }

    /// `import.topics` (theseus-anh3): a tag's labels as topics and
    /// memberships, on the blocking pool; a stop between two frames is an
    /// error naming what was written, and a rerun finishes it.
    async fn import_topics(
        self: Arc<Self>,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        let p: ImportTopicsParams = parse(params)?;
        let what = format!("the topics of {}", p.tag);
        let by = self.import_judged(conn, method::IMPORT_TOPICS, &what)?;
        let core = self.clone();
        let (r, stopped_early) = blocking(move || {
            let stopping = || core.outbox.stopping();
            topics::assign_unless(&core.store, &core.runner.ontology, &p.tag, &by, stopping)
        })
        .await?;
        if stopped_early {
            return Err(stopped(
                format!(
                    "the daemon stopped the topics of {} between two frames ({} frames written); \
                     run it again to finish (what was written is kept)",
                    r.tag, r.frames
                ),
                &r,
            ));
        }
        serde_json::to_value(r).map_err(|e| RpcFailure::invalid(e.into()))
    }

    /// `import.people` (theseus-wy7y): a tag's people, or their counts.
    async fn import_people(
        self: Arc<Self>,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        let p: ImportPeopleParams = parse(params)?;
        let what = format!("the people of {}", p.tag);
        let by = self.import_judged(conn, method::IMPORT_PEOPLE, &what)?;
        if p.propose {
            // Proposals from the sessions' text, under a cap (theseus-wy7y).
            let r = self
                .import_people_propose(&p)
                .await
                .map_err(RpcFailure::invalid)?;
            return serde_json::to_value(r).map_err(|e| RpcFailure::invalid(e.into()));
        }
        let core = self.clone();
        let (r, stopped_early) = blocking(move || {
            let stopping = || core.outbox.stopping();
            // The proposals' exclusions, so no agent or bot is declared (theseus-0p1r).
            let o = core.runner.ontology.snapshot(&core.store)?;
            let not = core.not_people(&[], &o);
            people::assign_unless(
                &core.store,
                &core.runner.ontology,
                (&p.tag, &by),
                p.dry_run,
                &not,
                stopping,
            )
        })
        .await?;
        if stopped_early {
            return Err(stopped(
                format!(
                    "the daemon stopped the people of {} between two frames ({} frames written); \
                     run it again to finish (what was written is kept)",
                    r.tag, r.frames
                ),
                &r,
            ));
        }
        serde_json::to_value(r).map_err(|e| RpcFailure::invalid(e.into()))
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
            // The follower may have met the tombstones first: then it
            // dropped the nodes itself, and nothing is left to forget.
            Ok(r) if r.nodes == 0 => {
                "nothing left to forget: its follower had dropped them at their tombstones".into()
            }
            Ok(r) => format!(
                "forgot {} its follower had not yet dropped ({} chunks, {} vectors)",
                crate::narrative::count(r.nodes, "node", "nodes"),
                r.chunks,
                r.vectors_dropped
            ),
            Err(e) => format!("the tender's forget did not answer ({e}): {later}"),
        }
    }
}

/// A batch or an erase the daemon's stop ended early (theseus-autz): an
/// error, so no client takes it for done, naming what was written and that
/// a rerun finishes it; `data` is the result for what was written. At a
/// stop the runtime's end usually cancels the connection's writer first,
/// and then no client hears it at all: the rerun is the same.
fn stopped(message: String, result: &impl serde::Serialize) -> RpcFailure {
    RpcFailure {
        code: error_code::INTERNAL,
        message,
        data: serde_json::to_value(result).unwrap_or_default(),
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
