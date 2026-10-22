//! A turn that ran at home but served one thread becomes that thread's memory (theseus-98bz.6).
//!
//! When the router runs a message at a place's home (asked and answered `home`, an ask nobody
//! answered, its own home pick), the turn's nodes belong to no thread, and the lens reads them as
//! the place's own conversation: distant for every thread, so the thread the work was for later
//! disowns it. So after such a turn, `[threads] claim_home_turns` on:
//!
//! - **When.** Only when the turn's result names the files it wrote or its commands left, every one
//!   of them (`more` is zero on both lists: an unseen path could be another thread's), and the place
//!   has a live, unsealed thread. Those checks read memory alone, before any store read.
//! - **Which thread** ([`thread_of`], pure). A path is a thread's when it is one of the thread's
//!   `files` or lies under one of its `roots` (an absolute or `~/` entry as written, a relative one
//!   under the turn's directory, prefixes by components). Paths no thread holds are ignored; a path
//!   two threads hold, or paths of two threads, claim nothing.
//! - **What.** The turn's own nodes in the home session, read backwards a page at a time from the
//!   session's newest, at most `MOST`; a node a split moved already keeps its split's value.
//! - **The write, one frame:** each node's `moved.of.<node>` = `MovedTo { thread, split: "claim_…" }`,
//!   the claim's record (META `thread.claim.<id>`, for the undo) and its `thread.claimed` row. The
//!   home's compile reads `Body::Moved` nodes, not the key, so it is unchanged; nothing is posted.
//! - **`thread.unclaim`** writes each key back to null where it still holds this claim's value (a
//!   later split's is kept), marks the record undone and writes `thread.unclaimed`.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use theseus_protocol::threads::{ThreadInfo, ThreadUnclaimParams, ThreadUnclaimResult};
use theseus_protocol::{error_code, method, TurnSubmitResult};
use theseus_store::pages::ledger_session;
use theseus_store::{kinds, NewRecord, Page, Store as _};

use super::server::RpcFailure;
use super::thread_flow::Routing;
use super::Core;
use crate::approval::Answerer;
use crate::fact;
use crate::fact::thread::{ThreadClaimed, ThreadUnclaimed};
use crate::moved::{self, MovedTo};
use crate::node::Node;
use crate::places::PlaceClass;
use crate::thread_route::{entry_path, holds_path};

/// Nodes read a page at a time, back from the session's newest.
const PAGE: usize = 64;
/// The most nodes read for one turn: a turn longer than this is not claimed.
const MOST: usize = 1024;

/// The META key of a claim's record, which `thread.unclaim` reads.
pub fn claim_key(id: &str) -> String {
    format!("thread.claim.{id}")
}

/// What a claim wrote, for its undo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimRecord {
    pub id: String,
    pub thread: String,
    /// The home session the turn ran in, and the turn.
    pub session: String,
    pub turn: String,
    /// The nodes keyed to the thread.
    pub nodes: Vec<String>,
    /// The turn's paths the thread holds.
    pub paths: Vec<String>,
    pub at_ms: u64,
    #[serde(default)]
    pub undone: bool,
}

/// The one thread of `live` that holds every path of `paths` some thread holds, and those paths;
/// none when no thread holds one, or two threads do. `dir` is the turn's directory, which a
/// relative entry lies under; `home` expands `~`.
pub fn thread_of<'a>(
    paths: &[String],
    dir: Option<&str>,
    live: &[&'a ThreadInfo],
    home: Option<&Path>,
) -> Option<(&'a str, Vec<String>)> {
    let mut found: Option<&'a str> = None;
    let mut matched = Vec::new();
    for path in paths {
        let Some(p) = entry_path(path, dir, home) else {
            continue;
        };
        let mut of = live.iter().filter(|t| holds_path(t, &p, dir, home));
        let Some(t) = of.next() else { continue };
        if of.next().is_some() || found.is_some_and(|s| s != t.slug) {
            return None;
        }
        found = Some(&t.slug);
        matched.push(path.clone());
    }
    found.map(|s| (s, matched))
}

impl Core {
    /// The router's directory rule (`thread_route::apart_by_dir`, theseus-vw8k7), read where the
    /// message is placed: the turn's directory is the submit's `dir`, else the session's; a
    /// thread's relative roots are read under its own session's directory (where its working set
    /// grew them, `thread_grow.rs`). One point read per live thread with relative roots, and none
    /// when the turn's directory is unknown.
    pub(super) fn keep_apart_by_dir(
        &self,
        p: &theseus_protocol::TurnSubmitParams,
        session: &crate::session::SessionRecord,
        live: &[&ThreadInfo],
        pre: &mut [crate::thread_route::Pre],
    ) {
        let Some(dir) = p.dir.as_deref().or(session.dir.as_deref()) else {
            return;
        };
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let bases: Vec<(String, Option<String>)> = live
            .iter()
            .filter(|t| {
                t.working_set
                    .roots
                    .iter()
                    .any(|r| !r.trim().is_empty() && !r.starts_with('/') && !r.starts_with('~'))
            })
            .map(|t| {
                let dir = t.session_id.as_deref().and_then(|sid| {
                    let r: Option<crate::session::SessionRecord> =
                        self.store.get_session(sid).ok().flatten();
                    r.and_then(|r| r.dir)
                });
                (t.slug.clone(), dir)
            })
            .collect();
        let base = |t: &ThreadInfo| {
            bases
                .iter()
                .find(|(s, _)| *s == t.slug)
                .and_then(|(_, d)| d.as_deref())
        };
        let dropped = crate::thread_route::apart_by_dir(
            pre,
            live,
            &p.input,
            Some(dir),
            base,
            home.as_deref(),
        );
        if !dropped.is_empty() {
            tracing::debug!(dir, threads = ?dropped, "the turn's directory lies under none of these threads' roots");
        }
    }

    /// After a routed turn: one that ran at home (no thread) has its nodes become the thread its
    /// files say it served, while `[threads] claim_home_turns` is on. A failure is logged and changes
    /// nothing: the turn has answered already.
    pub(super) fn claim_home_turn(&self, r: &Routing, res: &TurnSubmitResult) {
        if r.slug.is_some() || !self.cfg.threads.claim_home_turns {
            return;
        }
        if let Err(e) = self.try_claim(r, res) {
            tracing::warn!(error = %format!("{e:#}"), turn = %res.turn_id, "a home turn's claim failed");
        }
    }

    pub(super) fn try_claim(
        &self,
        r: &Routing,
        res: &TurnSubmitResult,
    ) -> Result<Option<ClaimRecord>> {
        let Some(files) = res.files.as_ref().filter(|f| !f.is_empty()) else {
            return Ok(None);
        };
        if files.changed.more > 0 || files.left.more > 0 {
            return Ok(None);
        }
        // A routed turn's place is private: the router runs in no other.
        let board = self.threads();
        let live: Vec<&ThreadInfo> = board
            .live(PlaceClass::Private)
            .into_iter()
            .filter(|t| t.sealed.is_none() && t.seal_pending.is_none())
            .collect();
        if live.is_empty() {
            return Ok(None);
        }
        let paths: Vec<String> = files
            .changed
            .paths
            .iter()
            .chain(&files.left.paths)
            .cloned()
            .collect();
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let Some((slug, matched)) = thread_of(&paths, files.dir.as_deref(), &live, home.as_deref())
        else {
            return Ok(None);
        };
        let mut nodes = Vec::new();
        for id in self.turn_nodes_back(&r.home, &res.turn_id)? {
            let to: Option<Option<MovedTo>> = self.store.get_meta(&moved::moved_of_key(&id))?;
            if !matches!(to, Some(Some(_))) {
                nodes.push(id);
            }
        }
        if nodes.is_empty() {
            return Ok(None);
        }
        let rec = ClaimRecord {
            id: crate::new_id(moved::CLAIM),
            thread: slug.to_string(),
            session: r.home.clone(),
            turn: res.turn_id.clone(),
            nodes,
            paths: matched,
            at_ms: theseus_protocol::now_unix_ms(),
            undone: false,
        };
        let to = MovedTo {
            thread: rec.thread.clone(),
            split: rec.id.clone(),
        };
        let fact = ThreadClaimed {
            claim: &rec.id,
            thread: &rec.thread,
            session: &rec.session,
            turn: &rec.turn,
            nodes: rec.nodes.len() as u32,
            paths: &rec.paths,
        };
        let mut records = vec![NewRecord::json(
            kinds::META,
            Some(&claim_key(&rec.id)),
            &rec,
        )?];
        for id in &rec.nodes {
            records.push(NewRecord::json(
                kinds::META,
                Some(&moved::moved_of_key(id)),
                &Some(&to),
            )?);
        }
        records.push(fact::row(&fact, Some(&rec.session), Some(&rec.turn))?);
        self.store.append(&records)?;
        self.rec(Some(&rec.session)).announce(&fact);
        Ok(Some(rec))
    }

    /// The ids of `turn`'s nodes in `sid`, read back from the session's newest node, a page at a
    /// time, to the first node of an older turn: never the whole transcript. A newer turn's nodes
    /// at the tail are passed over; none past `MOST` nodes read.
    fn turn_nodes_back(&self, sid: &str, turn: &str) -> Result<Vec<String>> {
        let (mut out, mut before, mut read) = (Vec::<String>::new(), None, 0);
        loop {
            let page = Page {
                kind: kinds::NODE,
                tags: vec![ledger_session(sid)],
                before,
                limit: PAGE,
                ..Page::default()
            };
            let Some(got) = self.store.inner().page(&page)? else {
                bail!("the index is still being built");
            };
            for r in got.records.iter().rev() {
                let n: Node = r.decode()?;
                match n.turn_id.as_deref() {
                    Some(t) if t == turn => {
                        if !out.contains(&n.id) {
                            out.push(n.id);
                        }
                    }
                    Some(_) if !out.is_empty() => return Ok(out),
                    _ => {}
                }
            }
            read += got.records.len();
            match (got.more, got.first) {
                (true, Some(first)) if read < MOST => before = Some(first),
                (true, _) => return Ok(Vec::new()),
                _ => return Ok(out),
            }
        }
    }

    /// `thread.unclaim`: the claim `p.claim` undone.
    pub fn thread_unclaim(
        &self,
        p: &ThreadUnclaimParams,
        who: impl Into<Answerer>,
    ) -> Result<ThreadUnclaimResult, RpcFailure> {
        let who = who.into();
        let id = p.claim.trim();
        let rec: ClaimRecord = self
            .store
            .get_meta(&claim_key(id))
            .map_err(RpcFailure::invalid)?
            .ok_or_else(|| {
                RpcFailure::new(
                    error_code::NOT_FOUND,
                    format!(
                        "no claim {id}: `thread.unclaim`'s `claim` is a claim's id (`claim_…`), \
                         as `theseus history` and the `thread.claimed` row name it"
                    ),
                )
            })?;
        self.judge_thread(&who, method::THREAD_UNCLAIM, &rec.thread)?;
        if rec.undone {
            return Err(RpcFailure::invalid(anyhow::anyhow!(
                "the claim {id} was undone already: `thread.unclaim`'s `claim` names one that holds"
            )));
        }
        let mine = MovedTo {
            thread: rec.thread.clone(),
            split: rec.id.clone(),
        };
        let mut records = Vec::new();
        let (mut restored, mut kept) = (0, 0);
        for node in &rec.nodes {
            let key = moved::moved_of_key(node);
            let to: Option<Option<MovedTo>> =
                self.store.get_meta(&key).map_err(RpcFailure::invalid)?;
            if to.flatten().as_ref() == Some(&mine) {
                records.push(NewRecord::json(kinds::META, Some(&key), &None::<MovedTo>));
                restored += 1;
            } else {
                kept += 1;
            }
        }
        let (who_s, via) = (who.who(), who.via());
        let fact = ThreadUnclaimed {
            claim: &rec.id,
            thread: &rec.thread,
            session: &rec.session,
            restored,
            kept,
            who: &who_s,
            via: &via,
        };
        let done = ClaimRecord {
            undone: true,
            ..rec.clone()
        };
        records.push(NewRecord::json(
            kinds::META,
            Some(&claim_key(&rec.id)),
            &done,
        ));
        records.push(fact::row(&fact, Some(&rec.session), None));
        let records = records
            .into_iter()
            .collect::<Result<Vec<_>>>()
            .map_err(RpcFailure::invalid)?;
        self.store.append(&records).map_err(RpcFailure::invalid)?;
        self.rec(Some(&rec.session)).announce(&fact);
        Ok(ThreadUnclaimResult {
            claim: rec.id,
            thread: rec.thread,
            session: rec.session,
            restored,
            kept,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(slug: &str, roots: &[&str], files: &[&str]) -> ThreadInfo {
        serde_json::from_value(serde_json::json!({
            "slug": slug, "title": slug,
            "working_set": {"roots": roots, "files": files},
        }))
        .unwrap()
    }

    fn of(paths: &[&str], live: &[&ThreadInfo]) -> Option<(String, Vec<String>)> {
        let paths: Vec<String> = paths.iter().map(|s| s.to_string()).collect();
        thread_of(&paths, Some("/w/home"), live, Some(Path::new("/h")))
            .map(|(s, m)| (s.to_string(), m))
    }

    #[test]
    fn a_path_is_the_one_thread_whose_root_or_file_holds_it() {
        let pr = t("pr-round", &["tarnlib"], &[]);
        let sec = t("sec-fix", &["~/svc/secsvc"], &["/w/home/notes/PRR-4142.md"]);
        let live = [&pr, &sec];
        // A bare root is a first component under the turn's directory.
        assert_eq!(
            of(
                &["/w/home/tarnlib/tarn/retry.py", "/tmp/scratch.txt"],
                &live
            ),
            Some((
                "pr-round".into(),
                vec!["/w/home/tarnlib/tarn/retry.py".into()]
            ))
        );
        assert_eq!(of(&["/w/home/tarnlibx/a.py"], &live), None, "by components");
        assert_eq!(of(&["/w/elsewhere/tarnlib/a.py"], &live), None);
        // `~/` expands; a file matches itself; `..` folds.
        assert_eq!(of(&["/h/svc/secsvc/main.go"], &live).unwrap().0, "sec-fix");
        assert_eq!(
            of(&["/w/home/notes/../notes/PRR-4142.md"], &live)
                .unwrap()
                .0,
            "sec-fix"
        );
        // Nothing threaded, or two threads: no claim.
        assert_eq!(of(&["/tmp/scratch.txt"], &live), None);
        assert_eq!(
            of(&["/w/home/tarnlib/a.py", "/h/svc/secsvc/b.go"], &live),
            None
        );
        let both = t("both", &["/w/home"], &[]);
        assert_eq!(of(&["/w/home/tarnlib/a.py"], &[&pr, &both]), None);
        assert_eq!(of(&[], &live), None);
    }
}
