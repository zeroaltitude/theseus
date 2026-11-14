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
//! **The first sighting says nothing.** A repository granted and compiled
//! for the first time has no stored state to differ from, so it stores one
//! and is quiet. The notice is for a change *after* the owner looked.
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
//!   sighting, which makes `untrust` then `trust` the owner's accept.
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
//! - The **writer key** is [`Held`] and [`wrote`]: a **swept** file of a
//!   granted repository that moved while a flagged session's ACTING call ran
//!   is held **for every session**, clean ones included, until the owner
//!   says yes.
//!
//! The reader key alone leaves the case the owner's ruling actually names.
//! A flagged session EDITS the file; the next CLEAN session to open in that
//! repository carries the new text, because *it* is not flagged and at
//! `notice` nothing holds it. The reason the first half keyed on the reader
//! was true of its mechanism -- a change is found by comparing a stored
//! sighting later, and a later comparison cannot say who wrote -- but the
//! daemon is present at the moment of every acting call, and at that moment
//! it can compare. It is writer-keyed **without knowing who wrote**: a file
//! that moved while a flagged session's acting call ran is treated as that
//! call's. That covers a shell write as surely as a file tool's, and a call
//! the operator confirmed as surely as one that ran straight off the gate
//! (the sweep is inside `ToolRuntime::execute`, which every path that runs a
//! call goes through). It errs the safe way on the rest -- a file that moved
//! under such a call for an unrelated reason (a `git pull` in another
//! terminal) is held too, and a held file is read again the moment the owner
//! grants the repository.
//!
//! **"Swept" is the load-bearing word, and it is narrower than "a file".**
//! [`swept`] is the union of the files the owner's stored sighting of a
//! granted repository already names and, for the repository the session is
//! working in, the chain from its root down to the session's own directory;
//! and [`wrote`] does that for EVERY granted repository, not only the
//! session's own. What that leaves open is ANY instruction file in neither
//! of those two sets -- whether it was created or merely changed, and
//! whether or not its directory has ever been compiled in. That is wider
//! than "a brand-new file in a directory nobody has opened", and it is
//! listed case by case with the rest of the remainder in [`wrote`]'s own
//! doc, where the sweep is.
//!
//! The writer key's hold is a **stored record**, not a daemon's memory: a
//! restart between the write and the next session's open is not the owner's
//! yes and must not launder it. It is dropped where the sighting is dropped,
//! by `project_untrust`, which is what makes `untrust` then `trust` release
//! it -- the same yes, and no new command surface for a state that already
//! had one.

use std::collections::BTreeMap;
use std::path::Path;

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

/// The META key prefix of a repository whose instruction file moved under a
/// flagged session's acting call, by `flow::spec::repo_id`.
pub const HELD_PREFIX: &str = "project.held.";

/// The META key of one repository's hold.
pub fn held_key(id: &str) -> String {
    format!("{HELD_PREFIX}{id}")
}

/// **The writer key's hold** (S31-1): a granted repository whose instruction
/// file moved while a session carrying `injection` was making an acting
/// call. Its presence holds that repository's changes out of **every**
/// session's instruction block, whatever `[project] on_change` says and
/// whatever `[content_based_security] injection` is set to, until the owner
/// grants the repository again.
///
/// **The hold is the repository's, not a digest's.** A held file the owner
/// then edits HIMSELF is still held: nothing here can tell his hand from
/// another write, and the repository is what he re-reads when he re-grants.
/// [`files`](Self::files) is therefore what the row and the block SAY, not
/// what they test; the hold is `is_some`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Held {
    /// The files that moved, by the path the block shows, to the digest the
    /// call left. A file the call DELETED is not here: there is no new text
    /// to hold, and `withheld` has never withheld an absence.
    pub files: BTreeMap<String, String>,
    /// The session whose acting call the files moved under. Not "who wrote":
    /// this is the call it moved under, which is the honest claim and the
    /// one the ledger row makes.
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
    /// writer key, each of which holds whatever the others say (S31-1). It
    /// is the decision rather than the setting, so every reader below acts
    /// on one answer and cannot disagree about which input produced it.
    pub ask: bool,
    /// Whether the holder is the *reading* session's label rather than the
    /// posture. It changes only what the block's line *says*: the hold
    /// itself is the same hold, and `ask` above is what performs it.
    pub flagged: bool,
    /// The writer key's stored hold, when there is one: a file of this
    /// repository moved under a flagged session's acting call, and is held
    /// for every session until the owner's yes. It outranks the other two in
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
                 They moved during a call of a session that carries the `injection` \
                 label ({}), so their new text is not here for any session: he grants \
                 the repository again (`theseus project untrust`, then `theseus project \
                 trust`) once he has read it.",
                h.by
            )
        } else if self.flagged {
            format!(
                "Project instructions changed since the owner last read them: {what}. \
                 This session carries the `injection` label, so their new text is not \
                 here: he grants the repository again (`theseus project untrust`, then \
                 `theseus project trust`) once he has read it."
            )
        } else if self.ask {
            format!(
                "Project instructions changed since the owner last read them: {what}. \
                 He asks to see a change before it is read, so their new text is not \
                 here: he grants the repository again (`theseus project untrust`, then \
                 `theseus project trust`) once he has read it."
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
/// read, as [`super::trust::granted`] is. Called twice in a turn -- once by
/// [`super::block`] to name the change and withhold what is held, once by
/// the turn to write the row and the state -- and deterministic on its
/// inputs, so the block and the ledger cannot disagree.
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
    flagged: bool,
) -> Observed {
    let id = crate::flow::spec::repo_id(root);
    let held = Held::read(store, &id);
    let ask = cfg.project.on_change() == OnChange::Ask || flagged || held.is_some();
    let at_ms = theseus_protocol::now_unix_ms();
    let now = sighting(files, found, at_ms);
    let old = Seen::read(store, &id);
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

/// What a flagged session's acting call did to a granted repository's
/// instruction files, when it moved any.
#[derive(Debug, Clone)]
pub struct Wrote {
    /// The same observation the compile makes, taken at the moment of the
    /// call: the rows to write (`fresh`) and the sighting to store
    /// (`write`), which is `ask`'s -- what the owner read does NOT move, so
    /// the file keeps waiting, and only what has been said about moves, so
    /// one change is one row and not one a call.
    pub observed: Observed,
    /// The hold to store, which is what makes the next CLEAN session wait
    /// too.
    pub held: Held,
}

/// The files of `root` this sweep compares: the union of **what the owner's
/// stored sighting of that repository already names** and, when the session
/// is working inside this repository, **the chain from its root down to the
/// session's own directory**.
///
/// Two sets, because neither alone is the answer. The chain alone is what
/// round 1 swept, and it misses every directory of the repository that is
/// not an ancestor of the cwd. The stored sighting alone misses the files
/// the session is working beside right now, which may never have been
/// compiled.
///
/// ⛔ **Together they are still NOT every file a grant covers that anything
/// has ever looked at**, which is what this doc claimed until round 3. A
/// sighting is stored per repository but taken from whichever directory the
/// compiling session was in, and it REPLACES its predecessor -- so a file
/// the owner has read falls out of it the next time an ordinary clean
/// session opens elsewhere in the same repository. And a granted repository
/// no session has ever been opened in has no stored sighting at all.
///
/// **It is deliberately not a directory walk.** A walk of a granted root per
/// acting call is a cost that scales with the repository rather than with
/// the grant, and the sweep runs inside the tool path. What that choice
/// leaves open is written down in [`wrote`]'s doc rather than glossed, and
/// it is *any* file in neither set -- not only a new one, and not only one
/// whose directory is undiscovered.
///
/// A stored path that no longer resolves to a regular file is simply left
/// out, which is exactly right: [`diff`] takes the union of the two
/// sightings' keys, so a file that was there and is not now comes back as a
/// change with an empty new digest -- gone, named in the row, held in
/// nothing.
fn swept(store: &crate::store::Store, root: &Path, here: &super::Discovered) -> Vec<Found> {
    let mut by_path: BTreeMap<String, Found> = BTreeMap::new();
    if here.root.as_deref() == Some(root) {
        for f in &here.project {
            by_path.insert(f.shown.clone(), f.clone());
        }
    }
    let Some(seen) = Seen::read(store, &crate::flow::spec::repo_id(root)) else {
        return by_path.into_values().collect();
    };
    for shown in seen.files.keys() {
        if by_path.contains_key(shown) {
            continue;
        }
        let path = Path::new(shown);
        // The depth the block orders by, recovered from the path itself; a
        // stored path is always under the root that stored it.
        let depth = path
            .parent()
            .and_then(|d| d.strip_prefix(root).ok())
            .map_or(0, |rel| rel.components().count());
        if let Some(f) = super::found(path, depth) {
            by_path.insert(shown.clone(), f);
        }
    }
    by_path.into_values().collect()
}

/// **The writer key** (S31-1). What a session carrying `injection` did to
/// the granted repositories' instruction files with the acting call that has
/// just run in `cwd` -- one [`Wrote`] per repository whose files moved, and
/// the empty vector when none did, which is the overwhelmingly common answer
/// and the only one that costs anything less than a few digests.
///
/// Called after each acting call of a flagged session and nowhere else
/// ([`crate::content::acts`] decides "acting", the same predicate the
/// freeze reads, so the set of calls a label refuses and the set it watches
/// cannot drift apart).
///
/// **Every granted repository is swept, not only the session's own** (round
/// 2). A call makes no promise to stay inside the directory its session
/// started in: it can write one directory sideways, and it can write into a
/// different repository the owner happens to have granted. Round 1 asked
/// [`super::discover`] for the cwd's repository and swept `chain(root, cwd)`
/// inside it, and poisoned text went past it both ways. [`super::trust::Grants::read`]
/// returns every granted repository in ONE keyed read, so the whole set
/// costs one read more than one repository did; what each root contributes
/// is [`swept`]'s union.
///
/// It asks [`observe`] with `flagged` true rather than building a second
/// comparison, so the row this writes and the row a compile writes are the
/// same row from the same code on the same inputs. What it adds is the
/// [`Held`] record: the compile's hold ends with the session that carries
/// the label, and this one does not.
///
/// **Edges, decided here rather than left implicit:**
///
/// - A file that moved under the call for some other reason (a `git pull` in
///   another terminal) is held. That errs the safe way, and the cost of
///   erring is one re-grant.
/// - A file the call CREATED is held **where it is swept**: a `CLAUDE.md`
///   where there was none is the same channel as an edit, and `diff` already
///   reports it as a change with an empty old digest.
/// - A file the call DELETED is named in the row and held in nothing: there
///   is no new text to withhold. Its absence is what every session gets,
///   which is not a channel into anybody's instruction block.
/// - A granted repository whose root is not a directory right now (an
///   unmounted disk, a tree that was moved) is skipped rather than reported
///   as a repository whose every file went away. Holding a repository the
///   machine cannot see would be a hold nobody could clear by reading
///   anything.
///
/// **What this does NOT reach, stated because a comment that overstates a
/// containment is worse than no comment:**
///
/// 1. **Any instruction file that is neither in a granted repository's
///    currently-stored sighting nor on the chain from the session's own
///    repository root to its own working directory** -- whether it was
///    CREATED or merely CHANGED, and whether or not its directory has ever
///    been compiled in. [`swept`] says why that is not a walk. This is the
///    one bypass of the three found in round 1 that is still open, and it
///    is open on purpose rather than by oversight.
///
///    ⛔ It is **wider** than "a brand-new file in a directory no compile
///    has discovered", which is how rounds 1 and 2 stated it, and two of
///    its cases are ones that wording excluded:
///
///    - **A granted repository no session has ever been opened in.** The
///      grant stores no sighting -- `rpc::project_trust` only DELETES
///      [`Seen`] and [`Held`], on untrust -- and the repository is not the
///      session's own, so BOTH halves of [`swept`] are empty and the
///      repository's own `AGENTS.md`, the very file the grant was given
///      for, is unswept. Nothing was created and nothing is undiscovered;
///      the repository was simply never opened. It arrives with **no
///      "changed" notice at all**, because a repository's first sighting is
///      not a diff -- quieter than the new-file case, which at least
///      produces a row.
///    - **A file the owner has already read, in a directory he has already
///      worked in.** A sighting is taken from one session's chain and
///      replaces its predecessor, so an ORDINARY clean session opening
///      elsewhere in the same repository evicts that file from it, and from
///      then on it is in neither set. ⛔ Nothing adversarial causes the
///      eviction -- ordinary use does.
/// 2. A background job that writes AFTER its call has returned is not seen
///    by that call's sweep. The next acting call of the session catches it;
///    a clean session that opens before then does not.
/// 3. Text written by a session that was never labelled at all. Neither key
///    reaches it; it is the tool floor's case, and an open owner question.
pub fn wrote(
    cfg: &crate::config::Config,
    store: &crate::store::Store,
    files: &ContextFiles,
    cwd: &Path,
    session: &str,
) -> Vec<Wrote> {
    let Ok(grants) = super::trust::Grants::read(store) else {
        return Vec::new();
    };
    if grants.repos.is_empty() {
        return Vec::new();
    }
    let here = super::discover(&cfg.state_dir(), cwd);
    let at_ms = theseus_protocol::now_unix_ms();
    grants
        .repos
        .values()
        .filter_map(|g| {
            let root = Path::new(&g.root);
            if !root.is_dir() {
                return None;
            }
            let found = swept(store, root, &here);
            if found.is_empty() {
                return None;
            }
            let observed = observe(cfg, store, files, root, &found, true);
            if observed.changes.is_empty() {
                return None;
            }
            let held = Held {
                files: observed
                    .changes
                    .iter()
                    .filter(|c| !c.is_gone())
                    .map(|c| (c.path.clone(), c.new.clone()))
                    .collect(),
                by: session.to_string(),
                at_ms,
            };
            Some(Wrote { observed, held })
        })
        .collect()
}
