//! A tag's people (theseus-wy7y): its imported sessions' people as the
//! ontology's persons and memberships, and the erase's taking them back.
//!
//! - **Who is a person**: a message's author written `person:<name>` (the
//!   episode format's word for a person who is not the owner; the owner by
//!   name, `agent:`, `tool` and `outside` are not people), and a DM's other
//!   party, which the episode's place names with its id. A DM's id is the
//!   other party's handle: `discord:<id>` when it is all digits (a
//!   snowflake), else `slack:<id>`.
//! - **Never a person** (theseus-0p1r): an author or a DM's party the
//!   proposals' exclusions exclude (`judge::people::NotPeople`: the owner,
//!   his agents and the house's names the store knows, `[people]
//!   not_people`, a bot's or a UI's name), counted in `excluded`.
//! - **One person each**: a DM's party is found by its handle. An author is
//!   found by name, and a name that spoke in the DMs of exactly one party
//!   (the only person author there) is that party everywhere; any other
//!   name is a person of its own. Then each is found among the held people:
//!   by an exact handle (a DM's person the transport made holds its
//!   `discord:<id>`; the import never doubles one), or, for a name alone,
//!   among the people this tag's run made before (idempotent). A display
//!   name alone never joins a person someone else made.
//! - **Each session's memberships**, origin `import`: the people who spoke
//!   in it and its DM's party, the party first, then by messages, at most
//!   [`PER_SESSION`]; the operator's own kept and counted first. A list that
//!   holds the same people from the same origins is not written again.
//! - **The owner's act** (`import.people`, `theseus import people`): never
//!   at a start, a live session never touched, from the stored records,
//!   deterministic, no model call. Frames through the ontology's write, cut
//!   past [`super::write::FRAME_RECORDS`] records, the machine's quiet
//!   waited for between two and the daemon's stop looked for there.
//! - **The erase takes them back**: the erased sessions' person lists are
//!   emptied with the rest (`topics::unassign`), then [`unassign`] takes
//!   away each person this tag's import made that nothing uses: never
//!   another tag's, the operator's or the transport's.

use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

use anyhow::Result;
use theseus_ontology::{
    handle, person::KIND, Category, CategoryId, MemberList, Membership, Ontology, Origin, Record,
};
use theseus_protocol::import::ImportPeopleResult;

use super::tag_scope;
use super::topics::{made_by, write_frames, Writer, ERASER};
use super::write::{FRAME_RECORDS, ONE};
use crate::judge::people::NotPeople;
use crate::node::Body;
use crate::ontology::Board;
use crate::session::SessionRecord;
use crate::store::Store;

/// The most people the import gives one session: a channel's history may
/// name dozens, and a membership is guidance in a compile.
pub const PER_SESSION: usize = 12;

/// Whom an imported person is, before the ontology: a DM party's handle, or
/// a name (folded) alone.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Who {
    Handle(String),
    Name(String),
}

/// One imported session's people: its DM's party, and its authors by count.
#[derive(Debug, Default)]
struct Seen {
    party: Option<(String, Option<String>)>,
    authors: BTreeMap<String, (String, u64)>,
}

/// A DM's id as its party's handle.
pub fn party_handle(id: &str) -> Option<String> {
    let id = id.trim();
    if id.is_empty() {
        return None;
    }
    let kind = match id.bytes().all(|b| b.is_ascii_digit()) {
        true => "discord",
        false => "slack",
    };
    handle(&format!("{kind}:{id}")).ok()
}

/// A message author's name, when the author is a person.
pub fn author_person(author: &str) -> Option<&str> {
    author
        .strip_prefix("person:")
        .map(str::trim)
        .filter(|n| !n.is_empty())
}

/// The tag's sessions, each with its people, and how many authors and
/// parties the exclusions left out.
type Read = (Vec<(String, Seen)>, u64);

/// The tag's live imported sessions, each with its people, read from the
/// stored records; `None` when the stop came first.
fn read(
    store: &Store,
    tag: &str,
    not: &NotPeople,
    stopping: &impl Fn() -> bool,
) -> Result<Option<Read>> {
    let mut out = Vec::new();
    let mut excluded = std::collections::HashSet::new();
    for (i, r) in store
        .scope_after(&tag_scope(tag), 0)?
        .into_iter()
        .enumerate()
    {
        if i % 64 == 63 && stopping() {
            return Ok(None);
        }
        if r.kind != theseus_store::kinds::SESSION {
            continue;
        }
        let rec: SessionRecord = r.decode()?;
        let Some(imp) = rec.imported.as_ref().filter(|i| i.erased.is_none()) else {
            continue;
        };
        let mut seen = Seen::default();
        if imp.place.kind == "dm" {
            if let Some(h) = imp.place.id.as_deref().and_then(party_handle) {
                let name = imp.place.name.clone();
                let candidate = theseus_judge::builders::PersonCandidate {
                    name: name.clone().unwrap_or_default(),
                    handles: vec![h.clone()],
                    role_line: String::new(),
                    evidence: Vec::new(),
                };
                if not.excludes(&candidate) {
                    excluded.insert(h);
                } else {
                    seen.party = Some((h, name));
                }
            }
        }
        for (_, n) in store.session_nodes(&rec.session_id)? {
            if !matches!(n.body, Body::Imported { .. }) {
                continue;
            }
            if let Some(name) = n.author.as_deref().and_then(author_person) {
                if not.excludes_name(name) {
                    excluded.insert(name.to_lowercase());
                    continue;
                }
                let e = seen
                    .authors
                    .entry(name.to_lowercase())
                    .or_insert_with(|| (name.to_string(), 0));
                e.1 += 1;
            }
        }
        out.push((rec.session_id.clone(), seen));
    }
    Ok(Some((out, excluded.len() as u64)))
}

/// Each name that spoke in one party's DMs alone (the only person author
/// there), with that party's handles; and each person, by who they are, with
/// a display name and the names they went by.
type Linked = HashMap<String, Vec<String>>;
type People = BTreeMap<Who, (String, Vec<String>)>;

fn gather(sessions: &[(String, Seen)]) -> (Linked, People) {
    let mut linked: Linked = HashMap::new();
    for (_, s) in sessions {
        if let (Some((h, _)), [(folded, _)]) =
            (&s.party, s.authors.iter().collect::<Vec<_>>().as_slice())
        {
            let l = linked.entry((*folded).clone()).or_default();
            if !l.contains(h) {
                l.push(h.clone());
            }
        }
    }
    let mut people: People = BTreeMap::new();
    let mut add = |who: Who, name: &str| {
        let e = people
            .entry(who)
            .or_insert_with(|| (name.to_string(), Vec::new()));
        if !e.1.iter().any(|n| n.eq_ignore_ascii_case(name)) {
            e.1.push(name.to_string());
        }
    };
    for (_, s) in sessions {
        if let Some((h, name)) = &s.party {
            let name = name.clone().unwrap_or_else(|| h.clone());
            add(Who::Handle(h.clone()), &name);
        }
        for (folded, (name, _)) in &s.authors {
            add(who_of(&linked, folded), name);
        }
    }
    (linked, people)
}

/// Who a folded author name is: the one party it spoke to alone, else the
/// name itself.
fn who_of(linked: &Linked, folded: &str) -> Who {
    match linked.get(folded).map(Vec::as_slice) {
        Some([h]) => Who::Handle(h.clone()),
        _ => Who::Name(folded.to_string()),
    }
}

/// Each session's person list as the rule makes it, where it differs from
/// the one `work` holds: its DM's party first, then its authors by messages,
/// at most [`PER_SESSION`], the operator's own kept and counted first.
fn lists_of(
    work: &Ontology,
    sessions: &[(String, Seen)],
    ids: &BTreeMap<Who, CategoryId>,
    linked: &Linked,
    out: &mut ImportPeopleResult,
) -> Vec<Record> {
    let now = theseus_protocol::now_unix_ms();
    let mut lists = Vec::new();
    for (sid, s) in sessions {
        let mut wanted: Vec<(CategoryId, u64)> = Vec::new();
        if let Some((h, _)) = &s.party {
            wanted.push((ids[&Who::Handle(h.clone())].clone(), u64::MAX));
        }
        for (folded, (_, n)) in &s.authors {
            let id = ids[&who_of(linked, folded)].clone();
            match wanted.iter_mut().find(|(w, _)| *w == id) {
                Some(w) => w.1 = w.1.saturating_add(*n),
                None => wanted.push((id, *n)),
            }
        }
        wanted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let old = work
            .member_list(sid, KIND)
            .map(|l| l.members.clone())
            .unwrap_or_default();
        let wanted: Vec<CategoryId> = wanted.into_iter().map(|(id, _)| id).collect();
        let mut members: Vec<Membership> = old
            .iter()
            .filter(|m| m.origin != Origin::Import || wanted.contains(&m.category))
            .cloned()
            .collect();
        for id in &wanted {
            if members.iter().any(|m| &m.category == id) {
                continue;
            }
            if members.len() >= PER_SESSION {
                out.capped += 1;
                break;
            }
            members.push(Membership::import(id.clone(), now));
        }
        out.memberships += members
            .iter()
            .filter(|m| m.origin == Origin::Import)
            .count() as u64;
        let same = old.len() == members.len()
            && old
                .iter()
                .zip(&members)
                .all(|(x, y)| x.category == y.category && x.origin == y.origin);
        if same {
            continue;
        }
        lists.push(Record::Members(MemberList {
            session: sid.clone(),
            kind: KIND.to_string(),
            members,
        }));
    }
    lists
}

/// `import.people`: `tag`'s sessions' people as persons and memberships, as
/// `by` ordered it, none `not` excludes; with `dry_run`, only the counts.
pub fn assign_unless(
    store: &Store,
    board: &Board,
    (tag, by): (&str, &str),
    dry_run: bool,
    not: &NotPeople,
    stopping: impl Fn() -> bool,
) -> Result<(ImportPeopleResult, bool)> {
    assign_in(
        store,
        board,
        (tag, by),
        (dry_run, not),
        stopping,
        FRAME_RECORDS,
    )
}

pub(super) fn assign_in(
    store: &Store,
    board: &Board,
    (tag, by): (&str, &str),
    (dry_run, not): (bool, &NotPeople),
    stopping: impl Fn() -> bool,
    cap: usize,
) -> Result<(ImportPeopleResult, bool)> {
    let _one = ONE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let t0 = Instant::now();
    let mut out = ImportPeopleResult {
        tag: tag.to_string(),
        dry_run,
        ..ImportPeopleResult::default()
    };
    let Some((sessions, excluded)) = read(store, tag, not, &stopping)? else {
        return Ok((out, true));
    };
    out.excluded = excluded;
    out.sessions = sessions.len() as u64;

    let (linked, people) = gather(&sessions);
    out.people = people.len() as u64;

    // Each person found among the held people, or declared now.
    let snapshot = board.snapshot(store)?;
    let mut work: Ontology = (*snapshot).clone();
    let mine = made_by(tag);
    let mut ids: BTreeMap<Who, CategoryId> = BTreeMap::new();
    let mut made: Vec<Category> = Vec::new();
    for (who, (name, names)) in &people {
        let held = match who {
            Who::Handle(h) => work.person_by_handle(h).map(|c| c.id.clone()),
            Who::Name(folded) => work
                .categories()
                .find(|c| {
                    c.kind() == KIND
                        && c.added_by == mine
                        && c.handles.iter().any(|h| {
                            h.strip_prefix("name:")
                                .is_some_and(|n| n.to_lowercase() == *folded)
                        })
                })
                .map(|c| c.id.clone()),
        };
        if let Some(id) = held {
            out.held += 1;
            ids.insert(who.clone(), id);
            continue;
        }
        let mut handles: Vec<String> = Vec::new();
        if let Who::Handle(h) = who {
            handles.push(h.clone());
        }
        for n in names {
            if let Ok(h) = handle(&format!("name:{n}")) {
                if !handles.contains(&h) {
                    handles.push(h);
                }
            }
        }
        let display: String = name
            .chars()
            .filter(|c| !c.is_control())
            .take(theseus_ontology::NAME_MAX)
            .collect();
        let c = Category {
            handles,
            description: format!("From the import {tag}."),
            ..Category::new(work.mint_id(KIND, &display)?, display.trim(), mine.clone())
        };
        work.put(Record::Category(c.clone()), Origin::Import)?;
        ids.insert(who.clone(), c.id.clone());
        made.push(c);
    }
    out.made = made.len() as u64;

    let lists = lists_of(&work, &sessions, &ids, &linked, &mut out);
    out.joined = lists.len() as u64;
    if dry_run {
        out.ms = t0.elapsed().as_secs_f64() * 1e3;
        return Ok((out, false));
    }
    let mut records: Vec<Record> = made.into_iter().map(Record::Category).collect();
    records.extend(lists);
    let to = Writer {
        tag,
        by,
        origin: Origin::Import,
        people: true,
    };
    let stopped = write_frames(store, board, records, cap, &stopping, &to, &mut out.frames)?;
    out.ms = t0.elapsed().as_secs_f64() * 1e3;
    Ok((out, stopped))
}

/// The erase's last half (`import.erase`, after `topics::unassign` emptied
/// the erased sessions' lists): each person `tag`'s import made that
/// nothing uses (no membership, no guidance) taken away; another tag's
/// unused person stays (one held past [`PER_SESSION`], say). How many, and
/// whether the stop came first.
pub fn unassign(
    store: &Store,
    board: &Board,
    tag: &str,
    by: &str,
    stopping: impl Fn() -> bool,
) -> Result<(u64, bool)> {
    let _one = ONE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let snapshot = board.snapshot(store)?;
    let mut work = (*snapshot).clone();
    let now = theseus_protocol::now_unix_ms();
    let mine = made_by(tag);
    let made: Vec<Category> = work
        .categories()
        .filter(|c| c.kind() == KIND && c.added_by == mine)
        .cloned()
        .collect();
    let mut records = Vec::new();
    for c in made {
        let r = Record::Category(c.retired(now));
        if work.put(r.clone(), ERASER).is_ok() {
            records.push(r);
        }
    }
    let n = records.len() as u64;
    let to = Writer {
        tag,
        by,
        origin: ERASER,
        people: true,
    };
    let mut frames = 0;
    let stopped = write_frames(
        store,
        board,
        records,
        FRAME_RECORDS,
        &stopping,
        &to,
        &mut frames,
    )?;
    Ok((n, stopped))
}
