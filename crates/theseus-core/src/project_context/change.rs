//! What changed in a granted repository since the last compile that read it
//! (`docs/design/project-context.md` §8, "After a grant").
//!
//! A grant names no file and no digest: it trusts the repository's
//! instruction files *as they change* ([`super::trust`]'s "What a grant is
//! not"). That is the design's "notify rather than ask", and the notice is
//! what this module is. Without it a grant is a standing permission whose
//! use leaves no trace: the text of a file edited after the grant enters
//! every request with an instruction's authority, and nothing in the block,
//! the ledger or the manifest says it is not the text the owner read when he
//! granted it.
//!
//! **The shape is the persona's, deliberately** ([`crate::persona`], whose
//! `Board::observe` and `persona.changed` row this mirrors): each compile
//! builds a sighting of the files' digests, compares it with the one the
//! store holds under `project.seen.<repo_id>`, and a difference is a
//! `project.changed` row and the new state, both in the turn's next frame.
//! Digests, never text. No watcher: the comparison rides the compile a
//! session's recompile already pays for.
//!
//! **The grant stores the first sighting** ([`grant`]). `Core::project_trust`
//! stores the digests of the instruction files on the chain down to the path
//! it was given (all of them) and of those a bounded walk of the root finds
//! ([`super::walk`], within an allowance) in the grant's own frame, so the first compile in any
//! directory of the repository compares with what was there when the owner
//! granted it. The notice is for a change *after* the owner looked. A granted
//! repository with no stored sighting (one granted by an earlier build) has
//! nothing to differ from: its first compile stores one and is quiet.
//!
//! **A sighting accumulates.** A compile looks only along its own chain,
//! from the root down to the session's directory, and it changes only what
//! it looked for ([`observe`]): a stored file elsewhere in the repository is
//! kept as it was stored, and is never reported gone by a compile that did
//! not look for it.
//!
//! **The posture** is `[project] on_change` (`config/project.rs`):
//!
//! - `notice` (the default): the new text is carried, and the change is
//!   named in the block and ledgered.
//! - `ask`: the *changed* files are named and not read until the owner grants
//!   the repository again; the unchanged ones are carried as before. What
//!   the owner read is **not** moved forward, so the file keeps waiting
//!   rather than arriving by itself on the next turn; only the digests
//!   already asked about move ([`Seen::asked`]), so a change that waits for
//!   days is one row and not one a turn. `project_untrust` drops the stored
//!   sighting, which makes `untrust` then `trust` the owner's accept; a
//!   `trust` naming the session's directory pins its files even where the
//!   walk did not reach them.
//!
//! **A session carrying `injection` holds the change whatever the posture
//! says** (S31-1, the owner 2026-11-13: the warning follows the content).
//! The text of a granted repository's instruction file is assembled into
//! block 3 and is no node (`judge::content::ContentItem::of_message`), so no
//! label can ever be raised on it, and asking the injection question of it
//! would answer yes on the honest files too. The remedy is not to label the
//! text. It is to withhold a *change* to it from a session that is already
//! labelled: `on_change` is a convenience about how much the owner wants to
//! be asked, and a label is not something a convenience gets to wave
//! through. The hold is `ask`'s own, reused rather than rebuilt -- the new
//! text waits, the unchanged files are carried as before, what the owner
//! read does not move forward, and his `untrust` then `trust` is the yes
//! that releases it. Only the sentence in the block differs, because the
//! reason differs.
//!
//! **There are TWO keys, and they are not the same key.**
//!
//! - The **reader key** is the paragraph above: `flagged`, passed in by the
//!   compiling turn. It holds a change from the session that is *reading*
//!   it.
//! - The **act key** is [`Held`] and [`hold_all`]: once a session carrying
//!   `injection` makes an acting call, or that label lands on a session,
//!   **every** granted repository is held **for every session**, clean ones
//!   and the owner's own edits included, until he grants it again (the
//!   owner, 2026-11-24).
//!
//! The reader key alone leaves the case the owner's ruling actually names:
//! a labelled session EDITS the file, and the next CLEAN session to open in
//! that repository would carry the new text. The act key compares nothing
//! at the act. It stores the hold, and the hold is read where a change is
//! found, at the next compile in any directory of any granted repository
//! ([`observe`]): every change against the owner's sighting is named there
//! and not read. So it does not matter which directory the write was in,
//! which repository, or when it lands -- a background job that writes after
//! its call returned is held as surely as the call's own write.
//!
//! The hold is a **stored record**, not a daemon's memory: a restart between
//! the act and the next session's open is not the owner's yes. It is dropped
//! where the sighting is dropped, by `project_untrust`, and replaced with a
//! fresh sighting by a `project_trust`, which is the owner's yes: he reads
//! the files and grants the repository again.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::OnChange;
use crate::context_files::{ContextFiles, ContextPath, Level};

use super::Found;

/// The META key prefix of a granted repository's last-seen state, by
/// `flow::spec::repo_id`.
pub const SEEN_PREFIX: &str = "project.seen.";

/// The META key of one repository's state.
pub fn seen_key(id: &str) -> String {
    format!("{SEEN_PREFIX}{id}")
}

/// The META key prefix of a granted repository held by a labelled act, by
/// `flow::spec::repo_id`.
pub const HELD_PREFIX: &str = "project.held.";

/// The META key of one repository's hold.
pub fn held_key(id: &str) -> String {
    format!("{HELD_PREFIX}{id}")
}

/// **The act key's hold** (S31-1): a granted repository held because a
/// session carrying `injection` made an acting call, or was labelled, while
/// it was granted ([`hold_all`]). Its presence holds that repository's
/// changes out of **every** session's instruction block, whatever
/// `[project] on_change` says and whatever `[content_based_security]
/// injection` is set to, until the owner grants the repository again.
///
/// **The hold is the repository's, not a digest's.** A file the owner then
/// edits HIMSELF is held too: nothing here can tell his hand from another
/// write, and the repository is what he re-reads when he re-grants. The
/// hold is `is_some`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Held {
    /// Written empty, and read by nothing: a hold is not a list of files.
    /// Kept so that a hold stored by an earlier build, which named the
    /// files that moved under the call, still decodes.
    pub files: BTreeMap<String, String>,
    /// The labelled session whose act stored the hold. Not "who wrote":
    /// the claim is that it acted, which is the one the ledger row makes.
    pub by: String,
    pub at_ms: u64,
}

impl Held {
    pub fn read(store: &crate::store::Store, id: &str) -> Option<Self> {
        store.get_meta::<Self>(&held_key(id)).ok().flatten()
    }

    pub fn record(&self, id: &str) -> anyhow::Result<theseus_store::NewRecord> {
        theseus_store::NewRecord::json(theseus_store::kinds::META, Some(&held_key(id)), self)
    }
}

/// What a compile saw of a granted repository: each discovered instruction
/// file's digest by the path the block shows, empty for a file the reader
/// could not read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seen {
    pub files: BTreeMap<String, String>,
    /// At `ask` posture, the digests of the changed files the owner has
    /// already been told of, so a change that keeps waiting is one row and
    /// not one a turn. Empty at `notice`, where `files` moves instead.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub asked: BTreeMap<String, String>,
    pub at_ms: u64,
}

impl Seen {
    pub fn read(store: &crate::store::Store, id: &str) -> Option<Self> {
        store.get_meta::<Self>(&seen_key(id)).ok().flatten()
    }

    pub fn record(&self, id: &str) -> anyhow::Result<theseus_store::NewRecord> {
        theseus_store::NewRecord::json(theseus_store::kinds::META, Some(&seen_key(id)), self)
    }
}

/// One file's difference between two sightings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The path as the block's header and the manifest show it.
    pub path: String,
    /// Digests; empty is absent -- a file that has appeared, or gone.
    pub old: String,
    pub new: String,
}

impl Change {
    /// A file the last sighting did not have at all.
    pub fn is_new(&self) -> bool {
        self.old.is_empty()
    }

    /// A file that was there and is not discovered now.
    pub fn is_gone(&self) -> bool {
        self.new.is_empty()
    }
}

/// What differs between two sightings, by path.
pub fn diff(old: &Seen, new: &Seen) -> Vec<Change> {
    let paths: std::collections::BTreeSet<_> = old.files.keys().chain(new.files.keys()).collect();
    paths
        .into_iter()
        .filter_map(|p| {
            let (o, n) = (old.files.get(p), new.files.get(p));
            (o != n).then(|| Change {
                path: p.clone(),
                old: o.cloned().unwrap_or_default(),
                new: n.cloned().unwrap_or_default(),
            })
        })
        .collect()
}

/// What a compile makes of a granted repository's files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observed {
    /// The repository's id (`repo_id`) and its canonical root, for the row.
    pub id: String,
    pub root: String,
    /// What differs from what the owner last read. The block names all of
    /// them, every compile: a model reading a request must be told what it
    /// is reading whether or not the row has been written.
    pub changes: Vec<Change>,
    /// The differences not already ledgered: one row each. The same as
    /// `changes` at `notice` posture, where the sighting moves and the
    /// notice is said once; at `ask` a change that keeps waiting is already
    /// in `Seen::asked` and is not said again.
    pub fresh: Vec<Change>,
    /// The sighting to store: none while a change is held, which is what
    /// makes the file keep waiting.
    pub write: Option<Seen>,
    /// Whether this compile holds a change back: `ask` posture, **or** a
    /// session carrying `injection`, **or** a stored [`Held`] from the
    /// act key, each of which holds whatever the others say (S31-1). It
    /// is the decision rather than the setting, so every reader below acts
    /// on one answer and cannot disagree about which input produced it.
    pub ask: bool,
    /// Whether the holder is the *reading* session's label rather than the
    /// posture. It changes only what the block's line *says*: the hold
    /// itself is the same hold, and `ask` above is what performs it.
    pub flagged: bool,
    /// The act key's stored hold, when there is one: a session carrying
    /// `injection` has acted since the owner last read this repository, and
    /// its changes are held for every session until his yes. It outranks the other two in
    /// the line, because it is the only one that says something about a
    /// session OTHER than the one reading.
    pub held: Option<Held>,
}

impl Observed {
    /// The paths whose change is not carried: the changed files at `ask`
    /// posture, and nothing at all at `notice`. A file that is *gone* is not
    /// withheld -- there is nothing to withhold.
    pub fn withheld(&self) -> Vec<&str> {
        if !self.ask {
            return Vec::new();
        }
        self.changes
            .iter()
            .filter(|c| !c.is_gone())
            .map(|c| c.path.as_str())
            .collect()
    }

    /// Whether `found` is one of them.
    pub fn withholds(&self, shown: &str) -> bool {
        self.withheld().contains(&shown)
    }

    /// §8's line for the block: what changed, and what this compile did
    /// about it. None when nothing changed.
    pub fn line(&self) -> Option<String> {
        if self.changes.is_empty() {
            return None;
        }
        let what = names(&self.changes);
        Some(if let Some(h) = &self.held {
            format!(
                "Project instructions changed since the owner last read them: {what}. \
                 A session that carries the `injection` label ({}) has acted since he \
                 last read them, so their new text is not here for any session: he grants \
                 the repository again (`theseus project untrust`, then `theseus project \
                 trust` naming this session's directory) once he has read it.",
                h.by
            )
        } else if self.flagged {
            format!(
                "Project instructions changed since the owner last read them: {what}. \
                 This session carries the `injection` label, so their new text is not \
                 here: he grants the repository again (`theseus project untrust`, then \
                 `theseus project trust` naming this session's directory) once he has \
                 read it."
            )
        } else if self.ask {
            format!(
                "Project instructions changed since the owner last read them: {what}. \
                 He asks to see a change before it is read, so their new text is not \
                 here: he grants the repository again (`theseus project untrust`, then \
                 `theseus project trust` naming this session's directory) once he has \
                 read it."
            )
        } else {
            format!(
                "Project instructions changed since the owner last read them: {what}. \
                 Their new text is above, and the change is in his ledger."
            )
        })
    }
}

/// The changed files as the line names them: the path, and for one that has
/// appeared or gone, which of the two it is.
fn names(changes: &[Change]) -> String {
    let one = |c: &Change| match (c.is_new(), c.is_gone()) {
        (true, _) => format!("{} (new)", c.path),
        (_, true) => format!("{} (gone)", c.path),
        _ => c.path.clone(),
    };
    changes.iter().map(one).collect::<Vec<_>>().join(", ")
}

/// The sighting a grant of the repository at `root` stores, given by `path`
/// (§8, "After a grant"): the instruction files on the chain from the root
/// down to `path` (the files a compile there finds), then what
/// [`super::walk`] found under the root.
///
/// The chain is digested whole, outside the digests' allowance: a grant
/// that names a directory pins every file a compile there finds, so a file
/// the walk did not reach is pinned by a grant that names its directory.
/// Its cost is bounded by the depth of the named path (two names a
/// directory), not by an allowance. The walk's other finds are digested
/// within [`super::DIGEST_FILES`] files and [`super::WALK_TIME`], and never
/// replace a digest of the chain. A file found and not digested is not in
/// the sighting, so it is new when a compile first finds it.
pub fn grant(files: &ContextFiles, root: &Path, path: &Path, at_ms: u64) -> Seen {
    let (most, time) = (super::DIGEST_FILES, super::WALK_TIME);
    grant_within(files, root, path, at_ms, most, time)
}

/// [`grant`], with the walk's finds digested within `most` files and
/// `time`.
pub(crate) fn grant_within(
    files: &ContextFiles,
    root: &Path,
    path: &Path,
    at_ms: u64,
    most: usize,
    time: std::time::Duration,
) -> Seen {
    let dir = if path.is_dir() {
        path
    } else {
        path.parent().unwrap_or(root)
    };
    let chain = super::discover_in(Some(root), dir, None).project;
    let mut seen = sighting(files, &chain, at_ms);
    let rest: Vec<Found> = super::walk(root)
        .into_iter()
        .filter(|f| !seen.files.contains_key(&f.shown))
        .collect();
    for (shown, digest) in sighting_within(files, &rest, at_ms, most, time).files {
        seen.files.entry(shown).or_insert(digest);
    }
    seen
}

/// [`sighting`] of the first of `found` it reaches within `most` files and
/// `time`. The time is checked between files, not between the calls that
/// read one file: one read that blocks is not interrupted.
pub(crate) fn sighting_within(
    files: &ContextFiles,
    found: &[Found],
    at_ms: u64,
    most: usize,
    time: std::time::Duration,
) -> Seen {
    let start = std::time::Instant::now();
    let mut seen = sighting(files, &[], at_ms);
    for f in found.iter().take(most) {
        if start.elapsed() >= time {
            break;
        }
        seen.files
            .extend(sighting(files, std::slice::from_ref(f), at_ms).files);
    }
    seen
}

/// This compile's sighting of `found`, from the reader's own digests. The
/// reader's stat-based cache answers it, so a compile that goes on to
/// assemble the same files pays for reading them once.
pub fn sighting(files: &ContextFiles, found: &[Found], at_ms: u64) -> Seen {
    let paths: Vec<ContextPath> = found
        .iter()
        .map(|f| ContextPath {
            path: f.shown.clone(),
            level: Level::Project,
            public: false,
        })
        .collect();
    Seen {
        files: files
            .peek(&paths)
            .into_iter()
            .zip(found)
            .map(|(c, f)| (f.shown.clone(), c.file.digest.unwrap_or_default()))
            .collect(),
        asked: BTreeMap::new(),
        at_ms,
    }
}

/// What a compile of `found` in the repository `root` sees: the sighting,
/// what it differs from, and whether the change is held. One keyed store
/// read, as [`super::trust::granted`] is.
///
/// `looked` is the directories the compile looked for files in. **A
/// sighting accumulates**: a stored path whose directory is not one of them
/// was not looked for, so it is carried into this sighting as it was
/// stored, and only a path in a directory the compile looked in can be
/// gone. A compile in one directory of a repository therefore neither
/// forgets nor reports as gone what a compile in another one saw.
///
/// Called twice in a turn -- once by [`super::block`] to name the change and
/// withhold what is held, once by the turn to write the row and the state --
/// and deterministic on its inputs, so the block and the ledger cannot
/// disagree.
///
/// `flagged` is whether the compiling session carries `injection`
/// (`ContentFlags::frozen`), and it holds the change on its own (S31-1).
/// **It is passed in, not read here, and it is read from the session's
/// record rather than from the mode.** Passed in, because both callers must
/// act on the *same* answer: a label landing between the block's call and
/// the turn's would otherwise let the block withhold text the row says was
/// carried. From the record rather than the mode, because the hold must not
/// depend on which `[content_based_security] injection` mode happens to be
/// configured -- that dependence is the hole S31-1 closed.
pub fn observe(
    cfg: &crate::config::Config,
    store: &crate::store::Store,
    files: &ContextFiles,
    root: &Path,
    found: &[Found],
    looked: &[PathBuf],
    flagged: bool,
) -> Observed {
    let id = crate::flow::spec::repo_id(root);
    let held = Held::read(store, &id);
    let ask = cfg.project.on_change() == OnChange::Ask || flagged || held.is_some();
    let at_ms = theseus_protocol::now_unix_ms();
    let mut now = sighting(files, found, at_ms);
    // A hold on a repository with no sighting (one granted by an earlier
    // build) compares with an empty one: every file is new, and held.
    let old = Seen::read(store, &id).or_else(|| held.as_ref().map(|_| Seen::default()));
    if let Some(o) = &old {
        carry(&mut now, o, looked);
    }
    // The first sighting has nothing to differ from: it is stored, and says
    // nothing.
    let changes = old.as_ref().map(|o| diff(o, &now)).unwrap_or_default();
    let asked = old.as_ref().map(|o| o.asked.clone()).unwrap_or_default();
    // What this compile asks about: every change, by its new digest, so a
    // change that keeps waiting is recognised and not said twice.
    let want: BTreeMap<String, String> = changes
        .iter()
        .map(|c| (c.path.clone(), c.new.clone()))
        .collect();
    let fresh: Vec<Change> = if ask {
        changes
            .iter()
            .filter(|c| asked.get(&c.path) != Some(&c.new))
            .cloned()
            .collect()
    } else {
        changes.clone()
    };
    let write = match &old {
        // The files are what the owner last read: nothing to say, nothing to
        // store.
        Some(_) if changes.is_empty() => None,
        // The owner has not read the change yet, so the next compile must
        // see it too: `files` stays what he read, which is what makes the
        // file keep waiting instead of arriving by itself. Only what this
        // compile asks about moves, and only when it is not already there.
        Some(o) if ask => (want != asked).then(|| Seen {
            files: o.files.clone(),
            asked: want,
            at_ms,
        }),
        // A first sighting, and every notice: what this compile read.
        _ => Some(now),
    };
    Observed {
        id,
        root: root.display().to_string(),
        changes,
        fresh,
        write,
        ask,
        flagged,
        held,
    }
}

/// Carry into `now` every path of `old` whose directory is not one of
/// `looked`, at the digest `old` stored for it: this compile did not look
/// for it, so it says nothing about it.
fn carry(now: &mut Seen, old: &Seen, looked: &[PathBuf]) {
    let looked: std::collections::HashSet<&Path> = looked.iter().map(PathBuf::as_path).collect();
    for (path, digest) in &old.files {
        let here = Path::new(path).parent().is_some_and(|d| looked.contains(d));
        if !here {
            now.files
                .entry(path.clone())
                .or_insert_with(|| digest.clone());
        }
    }
}

/// **The act key** (S31-1, the owner 2026-11-24): the records that put a
/// [`Held`] naming `by` on every granted repository that has none. A
/// repository already held keeps the hold it has.
///
/// Called after each acting call of a session carrying `injection`
/// ([`crate::content::acts`] decides "acting", the same predicate the freeze
/// reads, so the set of calls a label refuses and the set that hold cannot
/// drift apart), and when that label lands on a session
/// (`Core::tell_label`), so a label that lands after the write holds it too.
///
/// It compares nothing, so it is the same for a write in any directory of
/// any granted repository, for a repository no session has opened in, and
/// for a write that lands after the call returned. [`observe`] reads the
/// hold at the next compile and names every change against the owner's
/// sighting there, without reading it. One keyed read of the grants and one
/// of each granted repository's hold; no file is read.
pub fn hold_all(
    store: &crate::store::Store,
    by: &str,
) -> anyhow::Result<Vec<theseus_store::NewRecord>> {
    let grants = super::trust::Grants::read(store)?;
    let at_ms = theseus_protocol::now_unix_ms();
    grants
        .repos
        .keys()
        .filter(|id| Held::read(store, id).is_none())
        .map(|id| {
            Held {
                files: BTreeMap::new(),
                by: by.to_string(),
                at_ms,
            }
            .record(id)
        })
        .collect()
}
