//! A tag's topics (theseus-anh3): its imported sessions' topic labels as
//! ontology topics and memberships, and the erase's taking them back.
//!
//! The pipeline labels each episode with topics (`labels.topic`), slash
//! paths such as `garden/beds/soil`, and the import keeps them on the
//! session's record. `import.topics` makes them the ontology's:
//!
//! - **A tree from the paths**: a topic for each label and each prefix of
//!   one, the prefix its parent. A part is found by its name among its
//!   parent's children (as the ontology compares names, in any case), so a
//!   topic the operator declared at that place is used, not doubled. A new
//!   one's id is its path's slug (`garden-beds-soil`), its name its last
//!   part, its description the label, and its `added_by` [`MADE_BY`] and
//!   the tag.
//! - **Each session's memberships**, origin `import`, at most the kind's
//!   per-session count ([`plan`]). A label whose topic is the ancestor of
//!   another of the session's is left out first, since the chain rule
//!   composes an ancestor's guidance with its descendant's; then the first
//!   ones in the pipeline's order are taken, which is its ranking (the
//!   name's signal, then the content's by count, then the programs it
//!   adds). The rest stay labels. A membership the operator gave the
//!   session is kept, and counts toward the limit first.
//! - **From the stored labels**, never a re-import, and **idempotent**: a
//!   topic held is not declared again, and a list that holds the same
//!   topics from the same origins is not written again.
//! - **The owner's act**, as the import's (`import.topics`, `theseus import
//!   topics`): never at a start, and a live session is never touched; it
//!   joins topics through `categorize.v1`'s proposals or the operator.
//! - **The erase takes them back** ([`unassign`]): each erased session's
//!   interpreted lists emptied, then each topic an import made that nothing
//!   uses (no child, membership, or guidance) taken away, the deepest
//!   first.
//!
//! Each frame is the ontology's write (`ontology::Board::write`: checked,
//! written with its ledger row, held), cut past
//! [`super::write::FRAME_RECORDS`], with the machine's quiet waited for
//! between two and the daemon's stop looked for there.

use std::collections::{BTreeSet, HashMap};
use std::time::Instant;

use anyhow::{bail, Result};
use theseus_ontology::{
    Category, CategoryId, MemberList, Membership, Ontology, Origin, PerSession, Record, MAX_DEPTH,
    NAME_MAX,
};
use theseus_protocol::import::ImportTopicsResult;

use super::write::{sessions_of_tag, FRAME_RECORDS, ONE};
use crate::fact;
use crate::ontology::Board;
use crate::store::Store;

/// The kind the labels are topics of.
pub const KIND: &str = "topic";

/// A topic an import declared: its `added_by` is this, a space, and the tag.
pub const MADE_BY: &str = "import";

/// A topic's `added_by` when `tag`'s import declares it.
pub fn made_by(tag: &str) -> String {
    let s = format!("{MADE_BY} {tag}");
    s.chars().take(theseus_ontology::ADDED_BY_MAX).collect()
}

/// One session's labels as topic paths: which become memberships, and what
/// stays a label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The paths taken, each its parts, in the pipeline's order.
    pub taken: Vec<Vec<String>>,
    /// More paths than `max`: the rest stay labels.
    pub capped: bool,
    /// Labels no topic is made of: an empty part, a part longer than a
    /// name, or deeper than the tree nests.
    pub unplaced: usize,
}

/// A label's parts, or none when no topic can be made of it.
pub fn parts(label: &str) -> Option<Vec<String>> {
    let parts: Vec<String> = label.split('/').map(|p| p.trim().to_string()).collect();
    let ok = (1..=MAX_DEPTH).contains(&parts.len())
        && parts.iter().all(|p| {
            !p.is_empty() && p.chars().count() <= NAME_MAX && !p.chars().any(char::is_control)
        });
    ok.then_some(parts)
}

/// A path as names compare: each part in lowercase.
fn folded(parts: &[String]) -> Vec<String> {
    parts.iter().map(|p| p.to_lowercase()).collect()
}

/// A session's labels as at most `max` topic paths (the module's rule): the
/// usable ones, once each; less those another one descends from; then the
/// first `max`, in the pipeline's order.
pub fn plan(labels: &[String], max: usize) -> Plan {
    let mut unplaced = 0;
    let mut paths: Vec<Vec<String>> = Vec::new();
    for l in labels {
        match parts(l) {
            Some(p) if !paths.iter().any(|q| folded(q) == folded(&p)) => paths.push(p),
            Some(_) => {}
            None => unplaced += 1,
        }
    }
    let keys: Vec<Vec<String>> = paths.iter().map(|p| folded(p)).collect();
    let implied = |k: &Vec<String>| {
        keys.iter()
            .any(|o| o.len() > k.len() && o[..k.len()] == k[..])
    };
    let mut kept: Vec<Vec<String>> = paths
        .into_iter()
        .zip(&keys)
        .filter(|(_, k)| !implied(k))
        .map(|(p, _)| p)
        .collect();
    let capped = kept.len() > max;
    kept.truncate(max);
    Plan {
        taken: kept,
        capped,
        unplaced,
    }
}

/// Each folded path's topic.
type Ids = HashMap<Vec<String>, CategoryId>;

/// The topics of a set of paths: each path's (and prefix's) topic held in
/// `o`, or declared into it now (as the import's). Returns each folded path
/// with its topic's id, and the topics declared, parents first.
fn tree(o: &mut Ontology, paths: &[Vec<String>], tag: &str) -> Result<(Ids, Vec<Category>)> {
    let mut ids = Ids::new();
    let mut made = Vec::new();
    for p in paths {
        let mut parent: Option<CategoryId> = None;
        for depth in 1..=p.len() {
            let key = folded(&p[..depth]);
            if let Some(id) = ids.get(&key) {
                parent = Some(id.clone());
                continue;
            }
            let name = &p[depth - 1];
            let held = o
                .children(parent.as_ref())
                .into_iter()
                .find(|c| c.kind() == KIND && c.name.to_lowercase() == key[depth - 1])
                .map(|c| c.id.clone());
            let id = match held {
                Some(id) => id,
                None => {
                    let c = Category {
                        id: o.mint_id(KIND, &p[..depth].join(" "))?,
                        name: name.clone(),
                        parent: parent.clone(),
                        description: format!("imported label {}", p[..depth].join("/")),
                        added_by: made_by(tag),
                        retired_ms: None,
                        handles: Vec::new(),
                        merged_into: None,
                    };
                    o.put(Record::Category(c.clone()), Origin::Import)?;
                    made.push(c.clone());
                    c.id
                }
            };
            ids.insert(key, id.clone());
            parent = Some(id);
        }
    }
    Ok((ids, made))
}

/// The most of a kind one session may hold.
fn max_of(o: &Ontology) -> Result<usize> {
    let Some(kind) = o.kind(KIND) else {
        bail!("the ontology has no `{KIND}` kind: its seed row is gone");
    };
    if !kind.assigned_by.contains(&Origin::Import) {
        let names: Vec<&str> = kind.assigned_by.iter().map(|o| o.name()).collect();
        bail!(
            "the `{KIND}` kind's row (v{}) lets {} assign its memberships, not the import: \
             add `import` to its assigned_by, then run this again",
            kind.version,
            names.join(", ")
        );
    }
    Ok(match kind.per_session {
        PerSession::AtMost(n) => n as usize,
        PerSession::Many => usize::MAX,
    })
}

/// `import.topics`: `tag`'s sessions' labels as topics and memberships, as
/// `by` ordered it.
pub fn assign(store: &Store, board: &Board, tag: &str, by: &str) -> Result<ImportTopicsResult> {
    Ok(assign_unless(store, board, tag, by, || false)?.0)
}

/// [`assign`], ending at the first frame boundary that finds `stopping`;
/// true when it did. A rerun finishes it.
pub fn assign_unless(
    store: &Store,
    board: &Board,
    tag: &str,
    by: &str,
    stopping: impl Fn() -> bool,
) -> Result<(ImportTopicsResult, bool)> {
    assign_in(store, board, tag, by, stopping, FRAME_RECORDS)
}

/// [`assign_unless`], its frames cut past `cap` records.
pub(super) fn assign_in(
    store: &Store,
    board: &Board,
    tag: &str,
    by: &str,
    stopping: impl Fn() -> bool,
    cap: usize,
) -> Result<(ImportTopicsResult, bool)> {
    let _one = ONE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let t0 = Instant::now();
    let mut out = ImportTopicsResult {
        tag: tag.to_string(),
        ..ImportTopicsResult::default()
    };
    let Some(sessions) = sessions_of_tag(store, tag, &stopping)? else {
        return Ok((out, true));
    };
    let snapshot = board.snapshot(store)?;
    let max = max_of(&snapshot)?;
    let mut plans: Vec<(String, Plan)> = Vec::new();
    let mut labels: BTreeSet<Vec<String>> = BTreeSet::new();
    let mut paths: Vec<Vec<String>> = Vec::new();
    for (sid, rec) in &sessions {
        let Some(imp) = rec.imported.as_ref().filter(|i| i.erased.is_none()) else {
            continue;
        };
        out.sessions += 1;
        let p = plan(&imp.labels.topic, max);
        out.unplaced += p.unplaced as u64;
        out.capped += u64::from(p.capped);
        for l in imp.labels.topic.iter().filter_map(|l| parts(l)) {
            if labels.insert(folded(&l)) {
                paths.push(l);
            }
        }
        plans.push((sid.clone(), p));
    }
    out.labels = labels.len() as u64;
    // Every label's topic is declared, a capped session's left-out ones too:
    // the tree is the labels', and categorize.v1 may offer any of them.
    let mut work = (*snapshot).clone();
    let (ids, made) = tree(&mut work, &paths, tag)?;
    out.topics = ids.len() as u64;
    out.made = made.len() as u64;
    let now = theseus_protocol::now_unix_ms();
    let mut lists = Vec::new();
    for (sid, p) in plans {
        let old = work.member_list(&sid, KIND).map(|l| l.members.clone());
        let old = old.unwrap_or_default();
        let wanted: Vec<&CategoryId> = p.taken.iter().map(|path| &ids[&folded(path)]).collect();
        // The list as it is, in its order, less an import membership no
        // longer wanted; then each wanted one it lacks, while there is room.
        let mut members: Vec<Membership> = old
            .iter()
            .filter(|m| m.origin != Origin::Import || wanted.contains(&&m.category))
            .cloned()
            .collect();
        for id in wanted {
            if members.iter().any(|m| &m.category == id) {
                continue;
            }
            if members.len() >= max {
                out.capped += u64::from(!p.capped);
                break;
            }
            members.push(Membership::import(id.clone(), now));
        }
        out.memberships += members
            .iter()
            .filter(|m| m.origin == Origin::Import)
            .count() as u64;
        let same = |a: &[Membership], b: &[Membership]| {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(x, y)| x.category == y.category && x.origin == y.origin)
        };
        if same(&old, &members) {
            continue;
        }
        lists.push(Record::Members(MemberList {
            session: sid,
            kind: KIND.to_string(),
            members,
        }));
    }
    out.joined = lists.len() as u64;
    let mut records: Vec<Record> = made.into_iter().map(Record::Category).collect();
    records.extend(lists);
    let to = Writer {
        tag,
        by,
        origin: Origin::Import,
        people: false,
    };
    let stopped = write_frames(store, board, records, cap, &stopping, &to, &mut out.frames)?;
    out.ms = t0.elapsed().as_secs_f64() * 1e3;
    Ok((out, stopped))
}

/// What the erase's half took back: sessions whose lists were emptied, and
/// topics taken away.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Unassigned {
    pub memberships: u64,
    pub topics: u64,
    pub stopped: bool,
}

/// Who the erase's half writes as: the erase is the operator's act, and an
/// operator may always empty a list or take a topic away, so a kinds row
/// changed since the import never stops an erase halfway.
pub(super) const ERASER: Origin = Origin::Operator;

/// The erase's half (`import.erase`, after its tombstones): every erased
/// session of `tag` loses its interpreted memberships, then each topic an
/// import made that nothing uses is taken away, the deepest first.
pub fn unassign(
    store: &Store,
    board: &Board,
    tag: &str,
    by: &str,
    stopping: impl Fn() -> bool,
) -> Result<Unassigned> {
    unassign_in(store, board, tag, by, stopping, FRAME_RECORDS)
}

pub(super) fn unassign_in(
    store: &Store,
    board: &Board,
    tag: &str,
    by: &str,
    stopping: impl Fn() -> bool,
    cap: usize,
) -> Result<Unassigned> {
    let _one = ONE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut out = Unassigned::default();
    let Some(sessions) = sessions_of_tag(store, tag, &stopping)? else {
        out.stopped = true;
        return Ok(out);
    };
    let snapshot = board.snapshot(store)?;
    let mut work = (*snapshot).clone();
    let kinds: Vec<String> = work
        .kinds()
        .into_iter()
        .filter(|k| k.stores())
        .map(|k| k.name.clone())
        .collect();
    let mut records = Vec::new();
    for (sid, rec) in &sessions {
        if !rec.imported.as_ref().is_some_and(|i| i.erased.is_some()) {
            continue;
        }
        let mut emptied = false;
        for kind in &kinds {
            if work
                .member_list(sid, kind)
                .is_some_and(|l| !l.members.is_empty())
            {
                let r = Record::Members(MemberList {
                    session: sid.clone(),
                    kind: kind.clone(),
                    members: Vec::new(),
                });
                work.put(r.clone(), ERASER)?;
                records.push(r);
                emptied = true;
            }
        }
        out.memberships += u64::from(emptied);
    }
    // The deepest first: a parent is unused only once its children are gone.
    let now = theseus_protocol::now_unix_ms();
    let mut made: Vec<(usize, Category)> = work
        .categories()
        .filter(|c| c.kind() == KIND && c.added_by.split(' ').next() == Some(MADE_BY))
        .map(|c| (work.path(&c.id).len(), c.clone()))
        .collect();
    made.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
    for (_, c) in made {
        let r = Record::Category(c.retired(now));
        if work.put(r.clone(), ERASER).is_ok() {
            records.push(r);
            out.topics += 1;
        }
    }
    let mut frames = 0;
    let to = Writer {
        tag,
        by,
        origin: ERASER,
        people: false,
    };
    out.stopped = write_frames(store, board, records, cap, &stopping, &to, &mut frames)?;
    Ok(out)
}

/// Who writes a run's frames: its tag, who ordered it, the origin the
/// ontology judges the records as, and whose row each frame carries
/// (`import.people`'s, theseus-wy7y, or `import.topics`').
pub(super) struct Writer<'a> {
    pub(super) tag: &'a str,
    pub(super) by: &'a str,
    pub(super) origin: Origin,
    pub(super) people: bool,
}

/// Write `records` through the ontology, `cap` a frame, each frame with its
/// `import.topics` row counting what it holds; the machine's quiet waited
/// for between two frames, and the stop looked for there. True when a stop
/// ended it early.
pub(super) fn write_frames(
    store: &Store,
    board: &Board,
    records: Vec<Record>,
    cap: usize,
    stopping: &impl Fn() -> bool,
    to: &Writer<'_>,
    frames: &mut u64,
) -> Result<bool> {
    let mut left = records.into_iter().peekable();
    while left.peek().is_some() {
        let frame: Vec<Record> = left.by_ref().take(cap.max(1)).collect();
        let mut f = fact::import::ImportTopics {
            tag: to.tag,
            by: to.by,
            made: 0,
            joined: 0,
            taken: 0,
            retired: 0,
        };
        for r in &frame {
            match r {
                Record::Category(c) if c.retired_ms.is_some() => f.retired += 1,
                Record::Category(_) => f.made += 1,
                Record::Members(l) if l.members.is_empty() => f.taken += 1,
                Record::Members(_) => f.joined += 1,
                _ => {}
            }
        }
        let p = fact::import::ImportPeople {
            tag: to.tag,
            by: to.by,
            made: f.made,
            joined: f.joined,
            retired: f.retired,
        };
        board.write(store, frame, to.origin, |_| {
            let row = match to.people {
                true => fact::row(&p, None, None)?,
                false => fact::row(&f, None, None)?,
            };
            Ok(vec![row.scoped(&fact::import::scope(to.tag))])
        })?;
        *frames += 1;
        if left.peek().is_some() {
            theseus_store::pressure::quiet_blocking_unless(
                theseus_store::pressure::BOUND,
                stopping,
            );
            if stopping() {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
