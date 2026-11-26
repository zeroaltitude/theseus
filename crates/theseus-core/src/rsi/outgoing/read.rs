//! What the sessions working on an item were shown from outside their own work ([`Heard::of`]; `super`'s doc).
//!
//! - **Who worked on it** ([`sessions`]): the sessions asked for; the item's task session and every session its
//!   evidence names (every start of its builder); the session of every row of its builds (`self.branch.built`) and
//!   of its fixes, at the gate and at review, so an earlier start the task no longer names is still read; then,
//!   until nothing new is found, what each was forked from, every fork of each, and the sessions of every task
//!   each started (a helper's, a task's whose report it received), and their tasks' tasks.
//! - **What each was shown** ([`Reader::session`]): a look back's result as it was rendered (a result kept by
//!   reference through its quotes, read at their recorded positions with the erase-aware reader; a result stored
//!   before quotes by its own words); every node recall laid; and every view of every one of its turns, through
//!   the whole of the turn's view index ([`crate::view::record::every_of_turn`]).
//! - **What a view laid** ([`Reader::piece`]): another session's exchange, each node as the renderer lays it (a
//!   result kept in parts through its parts) and its `did` line; a recalled node of another session; a fold, by
//!   its handle through the ladder's own reader, its title, account and the person's lines it keeps by node id,
//!   when it stands for any exchange of another session; a card's or a waiting line's source exchanges.
//! - **Not read**: what is the session's own input (its brief, the item's own text, a review's findings handed to
//!   a fixer, a file it read): `super`'s stated limits. A node erased since is nothing to match: its words are
//!   never read again.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use super::{Deferral, Heard, Source, Unread, Why};
use crate::node::{Body, Node};
use crate::quotes::{self, Part as QuotePart, Read};
use crate::store::Store;
use crate::view::{Piece, Space};

impl Heard {
    /// What the sessions working on `item` read, with the sessions in `also` among them (the one pushing), read
    /// by `until`.
    pub fn of(store: &Store, item: &str, also: &[&str], until: Instant) -> Result<Self, Unread> {
        #[cfg(test)]
        tests_hooks::READS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let own = sessions(store, item, also, until)?;
        let mut r = Reader {
            store,
            own: own.iter().cloned().collect(),
            until,
            heard: Heard::default(),
            exchanges: HashMap::new(),
        };
        for s in &own {
            r.session(s)?;
        }
        r.laid_exchanges()?;
        Ok(r.heard)
    }
}

#[cfg(test)]
pub(crate) mod tests_hooks {
    /// How many times this process gathered what an item's sessions read (a test runs in a process of its own).
    pub(crate) static READS: std::sync::atomic::AtomicUsize =
        std::sync::atomic::AtomicUsize::new(0);
}

fn late(until: Instant) -> Result<(), Unread> {
    match Instant::now() >= until {
        true => Err(Unread::Defer(Deferral::Deadline)),
        false => Ok(()),
    }
}

/// The sessions that worked on `item` (the module's doc).
pub(crate) fn sessions(
    store: &Store,
    item: &str,
    also: &[&str],
    until: Instant,
) -> Result<Vec<String>, Unread> {
    let mut out = Vec::new();
    also.iter().for_each(|s| add(&mut out, s));
    let task = crate::task_graph::get(store, item).map_err(|_| Why::Tasks)?;
    if let Some(t) = task {
        t.session.iter().for_each(|s| add(&mut out, s));
        (t.evidence.iter())
            .filter_map(|e| e.identity.strip_prefix("session:"))
            .for_each(|s| add(&mut out, s));
    }
    let mut kinds = vec!["self.branch.built"];
    kinds.extend(crate::rsi::join::fix::KINDS);
    let rows = match crate::rsi::join::rows_of(store, &kinds) {
        Ok(Some(rows)) => rows,
        Ok(None) => return Err(Unread::Defer(Deferral::NotReady)),
        Err(_) => return Err(Why::Rows.into()),
    };
    for r in rows.iter().filter(|r| r.data["item"] == item) {
        match r.kind.as_str() {
            "self.branch.built" => r.session_id.iter().for_each(|s| add(&mut out, s)),
            _ => r.data["session"]
                .as_str()
                .into_iter()
                .for_each(|s| add(&mut out, s)),
        }
    }
    let tasks = crate::task_graph::all(store).map_err(|_| Why::Tasks)?;
    let mut at = 0;
    while at < out.len() {
        late(until)?;
        if out.len() > super::SESSIONS_MOST {
            return Err(Why::Sessions.into());
        }
        let s = out[at].clone();
        if let Some(o) = crate::fork::origin_of(store, &s).map_err(|_| Why::Session)? {
            add(&mut out, &o.session);
        }
        for f in forks_of(store, &s)? {
            add(&mut out, &f);
        }
        // The tasks it started, and the tasks of a task it is the session of.
        let mine: HashSet<&str> = (tasks.iter())
            .filter(|t| t.session.as_deref() == Some(s.as_str()))
            .map(|t| t.id.as_str())
            .collect();
        for t in &tasks {
            let child =
                t.origin.session == s || t.parent.as_deref().is_some_and(|p| mine.contains(p));
            if let Some(ts) = t.session.as_deref().filter(|_| child) {
                add(&mut out, ts);
            }
        }
        at += 1;
    }
    Ok(out)
}

fn add(out: &mut Vec<String>, s: &str) {
    if !s.is_empty() && !out.iter().any(|o| o == s) {
        out.push(s.to_string());
    }
}

/// The sessions forked from `session`: each fork's first compilation includes it.
fn forks_of(store: &Store, session: &str) -> Result<Vec<String>, Unread> {
    use crate::graph::{Edge, EdgeKind};
    let mut out = Vec::new();
    let edges = store
        .scope_after(&Edge::scope_into(session), 0)
        .map_err(|_| Why::Session)?;
    for r in edges {
        if r.kind != theseus_store::kinds::EDGE {
            continue;
        }
        let e: Edge = r.decode().map_err(|_| Why::Session)?;
        if EdgeKind::named(&e.kind) != Some(EdgeKind::Includes) {
            continue;
        }
        let Some(c) = store.get_compilation(&e.from).map_err(|_| Why::Session)? else {
            return Err(Why::Session.into());
        };
        if c.session_id != session && !out.contains(&c.session_id) {
            out.push(c.session_id);
        }
    }
    Ok(out)
}

/// The text `parts` show, each quote read at its recorded position, field and range by the erase-aware reader: a
/// quote of a node erased since shows nothing; one that cannot be read as it was written is unresolved.
pub(crate) fn rendered(store: &Store, parts: &[QuotePart]) -> Result<String, Why> {
    let mut out = String::new();
    for p in parts {
        match p {
            QuotePart::Code(c) => out.push_str(c),
            QuotePart::Quote(q) => match quotes::read(store, q) {
                Read::Field(text) => {
                    let bytes = text.get(q.from as usize..q.to as usize).ok_or(Why::Quote)?;
                    out.push_str(&q.form.apply(bytes));
                }
                Read::Erased => {}
                Read::Unreadable => return Err(Why::Quote),
            },
        }
    }
    Ok(out)
}

/// What `n` showed whoever was laid it: a result kept by reference as its parts render; any other node its text
/// as the index reads it (a result stored before quotes, its own words; an erased node, nothing).
pub(crate) fn shown(store: &Store, n: &Node) -> Result<String, Why> {
    match &n.body {
        Body::ToolResult { parts, .. } if !parts.is_empty() => rendered(store, parts),
        _ => Ok(crate::recall::text_of(n)),
    }
}

/// The reader of what an item's sessions were shown.
struct Reader<'a> {
    store: &'a Store,
    own: HashSet<String>,
    until: Instant,
    heard: Heard,
    /// The exchanges views laid of other sessions, by session, so each such session is read once.
    exchanges: HashMap<String, HashSet<String>>,
}

impl Reader<'_> {
    /// What `session` read: its look backs' and recalls' sources, and what every view of its turns laid.
    fn session(&mut self, session: &str) -> Result<(), Unread> {
        late(self.until)?;
        let nodes = self
            .store
            .session_nodes(session)
            .map_err(|_| Why::Session)?;
        let mut turns = Vec::new();
        for (_, n) in &nodes {
            if let Some(t) = n.turn_id.as_ref().filter(|t| !turns.contains(*t)) {
                turns.push(t.clone());
            }
            match &n.body {
                // A look back's result, as it was shown: its own rendering, never its sources' text now.
                Body::ToolResult { tool, .. } if crate::copies::copier(tool) => {
                    let text = shown(self.store, n)?;
                    self.heard.add(&text, Source::History);
                }
                Body::Recall { items, .. } => {
                    for i in items {
                        self.node(&i.node_id, Source::Recall)?;
                    }
                }
                _ => {}
            }
        }
        for turn in turns {
            late(self.until)?;
            let views =
                crate::view::record::every_of_turn(self.store, &turn).map_err(|e| match e {
                    crate::view::record::Every::Index => Why::ViewIndex,
                    crate::view::record::Every::Missing => Why::ViewMissing,
                    crate::view::record::Every::Unreadable => Why::ViewUnreadable,
                })?;
            for v in views {
                for s in &v.shown {
                    self.piece(&s.piece)?;
                }
            }
        }
        Ok(())
    }

    /// The words of node `id`, unless it is of one of the item's own sessions.
    fn node(&mut self, id: &str, source: Source) -> Result<(), Why> {
        let Some((_, n)) = self.store.get_node(id).map_err(|_| Why::Node)? else {
            return Err(Why::Node);
        };
        if !self.own.contains(&n.session_id) {
            let text = shown(self.store, &n)?;
            self.heard.add(&text, source);
        }
        Ok(())
    }

    /// Exchange `turn`, laid by a view: kept to be read with its session's others when it is another session's.
    fn exchange(&mut self, turn: &str) -> Result<(), Why> {
        let Some(e) = crate::exchange::exchange(self.store, turn).map_err(|_| Why::Exchange)?
        else {
            return Err(Why::Exchange);
        };
        if !self.own.contains(&e.session_id) {
            (self.exchanges.entry(e.session_id))
                .or_default()
                .insert(e.turn_id);
        }
        Ok(())
    }

    /// What one shown piece laid of another session.
    fn piece(&mut self, p: &Piece) -> Result<(), Why> {
        match &p.range.space {
            // The renderer lays the range's first exchange and, when it is another, its last.
            Space::Exchanges { .. } => {
                self.exchange(&p.range.first.id)?;
                if p.range.last.id != p.range.first.id {
                    self.exchange(&p.range.last.id)?;
                }
            }
            Space::Recalled { node, .. } => self.node(node, Source::Recall)?,
            Space::Folds { .. } => self.fold(&p.handle.0)?,
            Space::Card {
                from: Some(from), ..
            }
            | Space::Waiting {
                from: Some(from), ..
            } => {
                for turn in &from.exchanges {
                    self.exchange(turn)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// A fold a view laid, by its handle through the ladder's own reader: when it stands for an exchange of
    /// another session, its title and account, and the person's lines it keeps by node id.
    fn fold(&mut self, handle: &str) -> Result<(), Why> {
        let Some(f) = crate::ladder::fold::node(self.store, handle).map_err(|_| Why::Fold)? else {
            return Err(Why::Fold);
        };
        let mut foreign = false;
        for turn in &f.exchanges {
            match crate::exchange::exchange(self.store, turn).map_err(|_| Why::Exchange)? {
                Some(e) if self.own.contains(&e.session_id) => {}
                // An exchange no longer there may have been anyone's.
                _ => foreign = true,
            }
        }
        if !foreign {
            return Ok(());
        }
        self.heard.add(&f.title, Source::View);
        self.heard.add(&f.account, Source::View);
        for h in &f.his {
            self.node(&h.node, Source::View)?;
        }
        Ok(())
    }

    /// The words of every exchange a view laid of another session: each node as it was laid, and its `did` line,
    /// its session read once.
    fn laid_exchanges(&mut self) -> Result<(), Unread> {
        let laid = std::mem::take(&mut self.exchanges);
        for (session, turns) in &laid {
            late(self.until)?;
            let nodes = self
                .store
                .session_nodes(session)
                .map_err(|_| Why::Session)?;
            for (_, n) in &nodes {
                if (n.turn_id.as_ref()).is_some_and(|t| turns.contains(t)) {
                    let text = shown(self.store, n)?;
                    self.heard.add(&text, Source::View);
                }
            }
            for turn in turns {
                if let Some(d) =
                    crate::ladder::did::did(self.store, turn).map_err(|_| Why::Exchange)?
                {
                    let line = crate::ladder::did::shown_line(self.store, &d);
                    self.heard.add(&line, Source::View);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests_read.rs"]
mod tests;
