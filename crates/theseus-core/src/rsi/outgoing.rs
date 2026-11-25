//! The outgoing check (issue 53): no push of the self loop, a branch's or main's, carries a run of words that
//! a session working on that branch read from outside its own work.
//!
//! - **What was read** ([`Heard::of`]): for every session that worked on the item (its builder, across every
//!   start; every fixer of its branches; the forks of any of them, and what they were forked from), the words
//!   of what it was shown from other sessions, found through the references those reads left in the store,
//!   never through a copy of their words: a look back's result (`context.history`, `look.work`) by its copy
//!   edges (`copies`); a recall by its recalled nodes; a view by what its record says it laid of another
//!   session (an exchange, a recalled node, a fold and the owner's words it keeps by reference). Its own
//!   sessions' words are not counted: they are the work. What is read is held as hashes of its runs, for one
//!   push, and kept nowhere.
//! - **What leaves** ([`commits`]): every commit the push sends, each with its message and the lines it adds
//!   against its first parent, so a run added in one commit and removed in a later one is still seen; a join's
//!   merge carries its generated message.
//! - **The test** ([`Heard::check`]): a run of [`RUN`] words found in both, words compared with whitespace and
//!   case folded and punctuation dropped as a separator ([`words`]). A run that is also in the repository's tree at the base is
//!   not a match: quoting the code is not quoting a person.
//! - **The answer** ([`Refusal`]) names the commit and the kind of read, never a word of what matched; and a
//!   read that cannot be resolved refuses the push the same way.
//!
//! It does not catch a paraphrase.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use anyhow::{bail, Context, Result};

use super::tree::Place;
use crate::node::{Body, Node};
use crate::store::Store;
use crate::view::{Pos, Space};

/// How many words in a row are a match. Ten: a run that long, word for word, is a quotation; nine of the
/// commonest phrases in a commit message or a comment ("a test that fails without the fix passes with it")
/// are not words of a conversation, and the tests hold both sides of the line.
pub const RUN: usize = 10;

/// What kind of read showed the words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// A look back: the history tool's page, open or search, or `look.work`.
    History,
    /// What recall laid.
    Recall,
    /// What a view laid of another session.
    View,
}

impl Source {
    pub fn words(self) -> &'static str {
        match self {
            Source::History => "a look back through history",
            Source::Recall => "recall",
            Source::View => "the view of another session",
        }
    }
}

/// One commit about to leave: its id, and its message and then the lines it adds against its first parent.
#[derive(Debug, Clone)]
pub struct Commit {
    pub id: String,
    pub text: String,
}

/// Why a push is refused. It never holds a word of what matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// A commit holds a run of words a session working on the branch read from `source`.
    Words { commit: String, source: Source },
    /// What a session working on the branch read cannot be resolved, so nothing can be cleared.
    Unresolved(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Words { commit, source } => write!(
                f,
                "commit {} holds {RUN} or more words in a row that a session working on this branch read \
                 from {}: words of one of the owner's conversations are not pushed",
                &commit[..commit.len().min(12)],
                source.words()
            ),
            Refusal::Unresolved(why) => write!(
                f,
                "what a session working on this branch read cannot be resolved ({why}), so nothing is pushed"
            ),
        }
    }
}

/// What the sessions working on an item read from outside their own work, as the hashes of its runs.
#[derive(Debug, Default)]
pub struct Heard {
    runs: HashMap<u64, Source>,
}

impl Heard {
    /// What the sessions working on `item` read, with the sessions in `also` among them (the one pushing).
    pub fn of(store: &Store, item: &str, also: &[&str]) -> Result<Self> {
        let own = sessions(store, item, also)?;
        let mut heard = Heard::default();
        let mut laid = Laid::default();
        for s in &own {
            laid.read(store, s, &own, &mut heard)
                .with_context(|| format!("what {s} read"))?;
        }
        laid.exchanges(store, &own, &mut heard)?;
        Ok(heard)
    }

    fn add(&mut self, text: &str, source: Source) {
        for h in runs(&words(text)) {
            self.runs.entry(h).or_insert(source);
        }
    }

    /// Whether `commits` may leave: none holds a run of what was read that the tree at the base does not
    /// hold too (`base`, its text, read only when a commit matches).
    pub fn check(
        &self,
        commits: &[Commit],
        base: impl FnOnce() -> Result<String>,
    ) -> std::result::Result<(), Refusal> {
        let hits: Vec<(&str, Source, u64)> = (commits.iter())
            .flat_map(|c| runs(&words(&c.text)).into_iter().map(move |h| (c, h)))
            .filter_map(|(c, h)| self.runs.get(&h).map(|s| (c.id.as_str(), *s, h)))
            .collect();
        if hits.is_empty() {
            return Ok(());
        }
        let base = base().map_err(|e| {
            Refusal::Unresolved(format!("the repository at the base cannot be read: {e:#}"))
        })?;
        let wanted: HashSet<u64> = hits.iter().map(|(_, _, h)| *h).collect();
        let in_base: HashSet<u64> = (runs(&words(&base)).into_iter())
            .filter(|h| wanted.contains(h))
            .collect();
        match hits.into_iter().find(|(_, _, h)| !in_base.contains(h)) {
            Some((commit, source, _)) => Err(Refusal::Words {
                commit: commit.to_string(),
                source,
            }),
            None => Ok(()),
        }
    }
}

/// `text`'s words as the check compares them: split at whitespace and at punctuation, which is dropped (a
/// comment's `//` is no word, and a sentence quoted in a string of code is the same words as said), each
/// lower-cased.
pub fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// The hash of every run of [`RUN`] words in `words`.
fn runs(words: &[String]) -> Vec<u64> {
    words
        .windows(RUN)
        .map(|w| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            w.hash(&mut h);
            h.finish()
        })
        .collect()
}

/// The sessions that worked on `item`: `also`; its builder, every session a start of it opened; the session
/// of every fix of its branches; what each was forked from, and every fork of each.
fn sessions(store: &Store, item: &str, also: &[&str]) -> Result<Vec<String>> {
    use crate::rsi::join::fix::{item_rows, Point};
    let mut out = Vec::new();
    also.iter().for_each(|s| add(&mut out, s));
    if let Some(t) = crate::task_graph::get(store, item)? {
        t.session.iter().for_each(|s| add(&mut out, s));
        (t.evidence.iter())
            .filter_map(|e| e.identity.strip_prefix("session:"))
            .for_each(|s| add(&mut out, s));
    }
    for point in [Point::Gate, Point::Review] {
        let Some(rows) = item_rows(store, item, point)? else {
            bail!("the rows of its fixes cannot be read yet");
        };
        (rows.iter())
            .filter_map(|r| r.data["session"].as_str())
            .for_each(|s| add(&mut out, s));
    }
    let mut at = 0;
    while at < out.len() {
        let s = out[at].clone();
        if let Some(o) = crate::fork::origin_of(store, &s)? {
            add(&mut out, &o.session);
        }
        for f in forks_of(store, &s)? {
            add(&mut out, &f);
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
fn forks_of(store: &Store, session: &str) -> Result<Vec<String>> {
    use crate::graph::{Edge, EdgeKind};
    let mut out = Vec::new();
    for r in store.scope_after(&Edge::scope_into(session), 0)? {
        if r.kind != theseus_store::kinds::EDGE {
            continue;
        }
        let e: Edge = r.decode()?;
        if EdgeKind::named(&e.kind) != Some(EdgeKind::Includes) {
            continue;
        }
        let Some(c) = store.get_compilation(&e.from)? else {
            bail!("a fork's compilation cannot be read");
        };
        if c.session_id != session && !out.contains(&c.session_id) {
            out.push(c.session_id);
        }
    }
    Ok(out)
}

/// The exchanges views laid of other sessions, gathered so each such session is read once.
#[derive(Default)]
struct Laid {
    exchanges: HashMap<String, HashSet<String>>,
}

impl Laid {
    /// What `session` read: its look backs' and recalls' sources, and what its views laid.
    fn read(
        &mut self,
        store: &Store,
        session: &str,
        own: &[String],
        heard: &mut Heard,
    ) -> Result<()> {
        let mut turns = HashSet::new();
        for (_, n) in store.session_nodes(session)? {
            if let Some(t) = &n.turn_id {
                turns.insert(t.clone());
            }
            match &n.body {
                Body::ToolResult { tool, .. } => {
                    if let Some(via) = crate::copies::route(tool) {
                        for id in copied(store, &n.id, via)? {
                            node_words(store, &id, own, Source::History, heard)?;
                        }
                    }
                }
                Body::Recall { items, .. } => {
                    for i in items {
                        node_words(store, &i.node_id, own, Source::Recall, heard)?;
                    }
                }
                _ => {}
            }
        }
        for turn in turns {
            let (views, unread) = crate::view::record::of_turn_unread(store, &turn)?;
            if !unread.is_empty() {
                bail!("a view of one of its turns cannot be read");
            }
            for v in views {
                for s in &v.shown {
                    self.piece(store, &s.piece, own, heard)?;
                }
            }
        }
        Ok(())
    }

    /// What one shown piece laid of another session.
    fn piece(
        &mut self,
        store: &Store,
        p: &crate::view::Piece,
        own: &[String],
        heard: &mut Heard,
    ) -> Result<()> {
        let (first, last) = (&p.range.first, &p.range.last);
        match &p.range.space {
            Space::Exchanges { .. } => {
                if first.id != last.id {
                    bail!("a view laid a range of several exchanges");
                }
                let Some(e) = crate::exchange::exchange(store, &first.id)? else {
                    bail!("an exchange a view laid cannot be found");
                };
                if !own.contains(&e.session_id) {
                    (self.exchanges.entry(e.session_id))
                        .or_default()
                        .insert(e.turn_id);
                }
            }
            Space::Recalled { session, node } if !own.contains(session) => {
                node_words(store, node, own, Source::Recall, heard)?;
            }
            Space::Folds { place, level } => fold_words(store, (place, *level), first, own, heard)?,
            _ => {}
        }
        Ok(())
    }

    /// The words of every exchange a view laid of another session: its nodes, read once a session.
    fn exchanges(&self, store: &Store, own: &[String], heard: &mut Heard) -> Result<()> {
        for (session, turns) in &self.exchanges {
            for (_, n) in store.session_nodes(session)? {
                if (n.turn_id.as_ref()).is_some_and(|t| turns.contains(t)) {
                    words_of(&n, own, Source::View, heard);
                }
            }
        }
        Ok(())
    }
}

/// The nodes a look back's result `result` showed, by its copy edges on `via`.
fn copied(store: &Store, result: &str, via: &str) -> Result<Vec<String>> {
    use crate::graph::{Edge, EdgeKind};
    let prefix = format!("{}|{result}|", EdgeKind::DerivedFrom.as_str());
    let mut out = Vec::new();
    for r in
        (store.inner()).first_with_prefix_strict(theseus_store::kinds::EDGE, &prefix, usize::MAX)?
    {
        let e: Edge = r.decode()?;
        if e.via == via && !out.contains(&e.to) {
            out.push(e.to);
        }
    }
    Ok(out)
}

/// The words of node `id`, unless it is of a session in `own`; a node that cannot be found is unresolved.
fn node_words(
    store: &Store,
    id: &str,
    own: &[String],
    source: Source,
    heard: &mut Heard,
) -> Result<()> {
    let Some((_, n)) = store.get_node(id)? else {
        bail!("a node it read cannot be found");
    };
    words_of(&n, own, source, heard);
    Ok(())
}

fn words_of(n: &Node, own: &[String], source: Source, heard: &mut Heard) {
    if !own.contains(&n.session_id) {
        heard.add(&crate::recall::text_of(n), source);
    }
}

/// A fold a view laid: its account, and the owner's words it keeps by reference.
fn fold_words(
    store: &Store,
    (place, level): (&str, u8),
    first: &Pos,
    own: &[String],
    heard: &mut Heard,
) -> Result<()> {
    let key = format!(
        "{}{place}\u{1}{level}\u{1}{:020}",
        crate::view::fold::FOLD_PREFIX,
        first.at_ms
    );
    let Some(fold) = store.get_meta::<crate::view::fold::FoldNode>(&key)? else {
        bail!("a fold a view laid cannot be found");
    };
    heard.add(&fold.title, Source::View);
    heard.add(&fold.account, Source::View);
    for id in &fold.his_words {
        node_words(store, id, own, Source::View, heard)?;
    }
    Ok(())
}

/// Every commit in `base..tip`, read in `dir`: each with its message, then the lines it adds against its first
/// parent.
pub fn commits(p: &Place, dir: &std::path::Path, base: &str, tip: &str) -> Result<Vec<Commit>> {
    let list = p.git_ok(dir, &["rev-list", &format!("{base}..{tip}")])?;
    let mut out = Vec::new();
    for id in list.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let mut text = p.git_ok(dir, &["log", "-1", "--format=%B", id])?;
        let parent = format!("{id}^1");
        let has_parent = p
            .git(dir, &["rev-parse", "-q", "--verify", &parent])?
            .status
            .success();
        let diff = match has_parent {
            true => p.git_ok(dir, &["diff", "--no-color", "-U0", &parent, id])?,
            false => p.git_ok(dir, &["show", "--no-color", "-U0", "--format=", id])?,
        };
        for l in diff.lines() {
            if let Some(added) = l.strip_prefix('+').filter(|_| !l.starts_with("+++")) {
                text.push('\n');
                text.push_str(added);
            }
        }
        out.push(Commit {
            id: id.to_string(),
            text,
        });
    }
    Ok(out)
}

/// The text of every file of the tree at `base`, read in `dir`: what the repository already says.
pub fn base_text(p: &Place, dir: &std::path::Path, base: &str) -> Result<String> {
    p.git_ok(
        dir,
        &["grep", "-h", "-I", "--no-color", "-e", "", base, "--"],
    )
}

#[cfg(test)]
#[path = "tests_outgoing_unit.rs"]
mod tests;
