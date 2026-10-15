//! The work board's read for `work.digest` (theseus-qkoi3): the notices after
//! a position and what the board holds now, cut to one connection's scope, in
//! one pass over memory under the board's lock. It writes nothing and reads no
//! store; the ledger fallback is the rpc's (`rpc/digest.rs`).

use std::collections::{HashMap, HashSet};

use theseus_protocol::notices::Notice;
use theseus_protocol::work::{Question, Rollup, WorkKind, WorkState, WorkView};
use theseus_protocol::Level;

use super::{Scope, WorkBoard};
use crate::ledger::LedgerRow;
use crate::scrub::Scrubber;

/// A view's ancestry is read this deep at most.
const DEPTH: usize = 16;

/// What the board gives `work.digest`.
#[derive(Debug, Default)]
pub struct DigestRead {
    /// The board's position now.
    pub position: u64,
    /// The notices after the position asked, oldest first, in scope.
    pub notices: Vec<Notice>,
    /// Whether the ring holds every notice since the position asked.
    pub complete: bool,
    /// Tasks that were cancelled after it: the policy gives a cancel no
    /// notice, so the digest reads the views.
    pub cancelled: Vec<WorkView>,
    /// The title of each view's root and of each work a notice names, by id.
    pub titles: HashMap<String, String>,
    /// The ids of every view under a root the caller named, the root included.
    pub under: HashSet<String>,
    pub now: Rollup,
    pub questions: Vec<Question>,
    /// The `wake.fired` rows after the position, with their positions,
    /// unscoped: a wake is told from this one fact on both paths.
    pub fired: Vec<(u64, LedgerRow)>,
}

impl WorkBoard {
    /// The board as `work.digest` reads it. `scope` cuts to the connection's
    /// roots; `named` are the execution ids the caller asked for (empty: all),
    /// whose descendants `under` lists.
    pub(in crate::push) fn digest_read(
        &self,
        since: u64,
        scope: &Scope,
        named: &HashSet<String>,
        scrubber: &Scrubber,
    ) -> DigestRead {
        let (notices, complete) = self.notices_since(since);
        let under = self.under(named);
        let notices: Vec<Notice> = notices
            .iter()
            .filter(|n| scope.sees(&n.root.id))
            .filter(|n| wanted_here(&n.work.id, &n.root.id, named, &under))
            .map(|n| scope.notice(n, scrubber))
            .collect();
        let mut titles = HashMap::new();
        let mut cancelled = Vec::new();
        for v in self.views.values() {
            if !scope.sees(&v.root) {
                continue;
            }
            if v.parent.is_none() && !v.title.is_empty() {
                titles.insert(v.id.clone(), scope.show(v, scrubber).title);
            }
            if v.kind == WorkKind::Task
                && v.state == WorkState::Cancelled
                && v.position > since
                && wanted_here(&v.id, &v.root, named, &under)
            {
                cancelled.push(scope.show(v, scrubber));
            }
        }
        cancelled.sort_by_key(|v| (v.at_ms, v.id.clone()));
        let mut now = Rollup::default();
        let mut questions: Vec<WorkView> = Vec::new();
        for v in self.views.values() {
            if !scope.sees(&v.root) || !wanted_here(&v.id, &v.root, named, &under) {
                continue;
            }
            if v.parent.is_none() {
                now.total += 1;
                match v.attention.level {
                    Level::NeedsYou => now.needs_you += 1,
                    Level::Working => now.working += 1,
                    _ if v.state == WorkState::Failed => now.failed += 1,
                    _ => now.done += 1,
                }
                if let Some(b) = v.below {
                    now.needs_you += b.needs_you;
                    now.working += b.working;
                    now.done += b.done;
                    now.failed += b.failed;
                    now.total += b.total;
                }
            }
            if v.kind == WorkKind::Question && !v.state.ended() && v.question.is_some() {
                questions.push(scope.show(v, scrubber));
            }
        }
        let mut qs: Vec<Question> = questions.into_iter().filter_map(|v| v.question).collect();
        qs.sort_by(|a, b| (a.asked_at_ms, &a.id).cmp(&(b.asked_at_ms, &b.id)));
        DigestRead {
            position: self.position,
            notices,
            complete,
            cancelled,
            titles,
            under,
            now,
            questions: qs,
            fired: self.fired_since(since),
        }
    }

    /// Every view id under a named root, the roots included.
    fn under(&self, named: &HashSet<String>) -> HashSet<String> {
        let mut out: HashSet<String> = named.clone();
        if named.is_empty() {
            return out;
        }
        for v in self.views.values() {
            let mut at = v.parent.as_deref();
            for _ in 0..DEPTH {
                let Some(p) = at else { break };
                if named.contains(p) {
                    out.insert(v.id.clone());
                    break;
                }
                at = self.views.get(p).and_then(|x| x.parent.as_deref());
            }
        }
        out
    }
}

/// Whether a work under `root` is one the caller asked for.
fn wanted_here(id: &str, root: &str, named: &HashSet<String>, under: &HashSet<String>) -> bool {
    named.is_empty() || named.contains(root) || under.contains(id)
}
