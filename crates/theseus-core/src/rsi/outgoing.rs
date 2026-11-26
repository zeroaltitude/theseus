//! The outgoing check (issue 53): no push of the self loop, a branch's, a join's or a revert's, carries a run of
//! words that a session working on that item was shown from outside its own work.
//!
//! - **What was read** ([`Heard::of`], `read.rs`): for every session that worked on the item (its builder, across
//!   every start and every row of the item's branches; every fixer; their task and helper children; the forks of
//!   any of them and what they were forked from), the words it was shown from other sessions, found through the
//!   references those reads left in the store, never through a copy of their words: a look back's result as it was
//!   rendered (by its quotes' recorded positions, or a result stored before quotes by its own words); what recall
//!   laid; and every view of every turn, read through the whole view index to its end: another session's exchange
//!   as the renderer laid it (a result kept in parts followed through them), a recalled node, a fold by its handle
//!   through the ladder's own reader, a card's source exchanges. Its own sessions' words are not counted: they are
//!   the work. What was read is held as hashes of its runs, for one pass, and kept nowhere. Anything that cannot
//!   be read refuses the push ([`Why`]); the ledger index not being built yet, or the read passing its deadline,
//!   defers it ([`Unread::Defer`]).
//! - **What leaves** ([`leave::Leaving`], `leave.rs`): every commit of the exact graph, each with its message,
//!   its author and committer, every path it changes and the whole new text of every file it changes (read as
//!   the blob is, with no conversion), and the branch it goes to. A changed file that cannot be read as text, or
//!   is larger than [`FILE_BYTES`], is not inspected and is refused ([`Refusal::Uninspectable`]).
//! - **The test** ([`Heard::check`]): a run of [`RUN`] words found in both, words compared with case folded and
//!   everything that is not a letter or a digit a separator ([`words`]), within one piece of what leaves (no run
//!   across two files or two fields). A run that is also in one file of the repository's tree at the base (or in
//!   one of its paths) is not a match: quoting the code is not quoting a person.
//! - **The answer** ([`Refusal`]) names the commit, the kind of piece and the kind of read, never a word of what
//!   matched; and it is the same for a read that cannot be resolved.
//!
//! It does not catch a paraphrase.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::time::Duration;

pub mod clear;
pub mod leave;
pub(crate) mod read;

pub use clear::Stop;
pub use leave::Leaving;

/// How many words in a row are a match. Ten: a run that long, word for word, is a quotation; nine of the
/// commonest phrases in a commit message or a comment ("a test that fails without the fix passes with it") are
/// not words of a conversation, and the tests hold both sides of the line.
pub const RUN: usize = 10;

/// The longest a check may read what was read before its push is deferred to a later pass.
pub const DEADLINE: Duration = Duration::from_secs(120);

/// Deferrals for the deadline in a row of one head at the join, after which it is held as unresolved.
pub const DEFERRALS_HELD: usize = 3;

/// The largest changed file read; a larger one is not inspected, and is refused.
pub const FILE_BYTES: u64 = 4 << 20;

/// The most sessions one item's reads are gathered from; past it, the read is unresolved.
pub const SESSIONS_MOST: usize = 1024;

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

/// Which piece of what leaves held a run: never the piece's own name or words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Message,
    /// A file's text.
    File,
    /// A path the commit changes.
    Path,
    /// Its author's or its committer's name.
    Names,
    /// The name of the branch it is pushed to.
    Branch,
}

impl Part {
    pub fn words(self) -> &'static str {
        match self {
            Part::Message => "its message",
            Part::File => "a file it changes",
            Part::Path => "the name of a path it changes",
            Part::Names => "its author's or committer's name",
            Part::Branch => "the name of the branch",
        }
    }
}

/// Why what was read, or what would leave, cannot be cleared: a fixed category, never a record's words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// The ledger rows of its builds and fixes cannot be read.
    Rows,
    /// Its task records cannot be read.
    Tasks,
    /// One of its sessions, or what it was forked from, cannot be read.
    Session,
    /// More sessions worked on it than are read.
    Sessions,
    /// A turn's index of views cannot be read.
    ViewIndex,
    /// A view a turn's index lists is not there.
    ViewMissing,
    /// A view record cannot be read.
    ViewUnreadable,
    /// A quote a result kept by reference showed cannot be read as it was written.
    Quote,
    /// A node it was shown cannot be found.
    Node,
    /// An exchange a view laid cannot be found.
    Exchange,
    /// A fold a view laid cannot be found by its handle.
    Fold,
    /// The commits it would send cannot be read.
    Commits,
    /// The repository at the base cannot be read.
    Base,
}

impl Why {
    pub fn words(self) -> &'static str {
        match self {
            Why::Rows => "the rows of its builds and fixes cannot be read",
            Why::Tasks => "its task records cannot be read",
            Why::Session => "one of the sessions that worked on it cannot be read",
            Why::Sessions => "more sessions worked on it than the check reads",
            Why::ViewIndex => "the index of a turn's views cannot be read",
            Why::ViewMissing => "a view a turn lists is missing",
            Why::ViewUnreadable => "a view record cannot be read",
            Why::Quote => "a quote a look back showed cannot be read as it was written",
            Why::Node => "a node it was shown cannot be found",
            Why::Exchange => "an exchange a view laid cannot be found",
            Why::Fold => "a fold a view laid cannot be found",
            Why::Commits => "the commits it would send cannot be read",
            Why::Base => "the repository at the base cannot be read",
        }
    }
}

/// Why a check is put off to a later pass: no hold, no card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deferral {
    /// The ledger's index is still being built.
    NotReady,
    /// The read passed [`DEADLINE`].
    Deadline,
}

impl Deferral {
    pub fn words(self) -> &'static str {
        match self {
            Deferral::NotReady => "the ledger's index is still being built",
            Deferral::Deadline => "reading what its sessions read took longer than the check waits",
        }
    }
}

/// What was read could not be gathered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unread {
    /// Try again on a later pass.
    Defer(Deferral),
    /// It cannot be cleared: the push is refused.
    Unresolved(Why),
}

impl From<Why> for Unread {
    fn from(w: Why) -> Self {
        Unread::Unresolved(w)
    }
}

/// Why a push is refused. It never holds a word of what matched, nor a name that held it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// A piece of what leaves holds a run of words a session working on the item read from `source`. `commit` is
    /// none for the branch's name.
    Words {
        commit: Option<String>,
        part: Part,
        source: Source,
    },
    /// A file `commit` changes cannot be read as text, or is too large to read: it is not inspected.
    Uninspectable { commit: String },
    /// What a session working on the item read, or what would leave, cannot be read.
    Unresolved(Why),
}

pub(crate) fn short(id: &str) -> &str {
    &id[..id.len().min(12)]
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Words {
                commit,
                part,
                source,
            } => {
                let at = match commit {
                    Some(c) => format!("commit {}, in {}", short(c), part.words()),
                    None => part.words().to_string(),
                };
                write!(
                    f,
                    "{at} holds {RUN} or more words in a row that a session working on this item read from {}: \
                     words of one of the owner's conversations are not pushed",
                    source.words()
                )
            }
            Refusal::Uninspectable { commit } => write!(
                f,
                "commit {} changes a file that cannot be read as text or is too large to read, so it cannot be \
                 checked and is not pushed",
                short(commit)
            ),
            Refusal::Unresolved(why) => write!(f, "{}, so nothing is pushed", why.words()),
        }
    }
}

/// What the sessions working on an item read from outside their own work, as the hashes of its runs.
#[derive(Debug, Default, Clone)]
pub struct Heard {
    runs: HashMap<u64, Source>,
}

impl Heard {
    pub(crate) fn add(&mut self, text: &str, source: Source) {
        for h in runs(&words(text)) {
            self.runs.entry(h).or_insert(source);
        }
    }

    /// How many runs it holds.
    pub fn len(&self) -> usize {
        self.runs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    /// Whether `leaving` may leave: no piece of it holds a run of what was read that the repository at the base
    /// does not hold too. `base` gives, of the runs asked for, those one file or one path at the base holds; it is
    /// read only when something matched.
    pub fn check(
        &self,
        leaving: &Leaving,
        base: impl FnOnce(&HashSet<u64>) -> Result<HashSet<u64>, Why>,
    ) -> Result<(), Refusal> {
        let mut hits: Vec<(Option<&str>, Part, Source, u64)> = Vec::new();
        for (commit, part, text) in leaving.pieces() {
            for h in runs(&words(text)) {
                if let Some(s) = self.runs.get(&h) {
                    hits.push((commit, part, *s, h));
                }
            }
        }
        if hits.is_empty() {
            return Ok(());
        }
        let wanted: HashSet<u64> = hits.iter().map(|h| h.3).collect();
        let in_base = base(&wanted).map_err(Refusal::Unresolved)?;
        match hits.into_iter().find(|h| !in_base.contains(&h.3)) {
            Some((commit, part, source, _)) => Err(Refusal::Words {
                commit: commit.map(str::to_string),
                part,
                source,
            }),
            None => Ok(()),
        }
    }
}

/// `text`'s words as the check compares them: split at everything that is not a letter or a digit, which is
/// dropped (a comment's `//` is no word, a hyphenated file name is its words, and a sentence quoted in a string of
/// code is the same words as said), each lower-cased.
pub fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// The hash of every run of [`RUN`] words in `words`.
pub(crate) fn runs(words: &[String]) -> Vec<u64> {
    words
        .windows(RUN)
        .map(|w| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            w.hash(&mut h);
            h.finish()
        })
        .collect()
}

#[cfg(test)]
#[path = "tests_outgoing_unit.rs"]
mod tests;
