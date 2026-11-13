//! The session's system block (theseus-aab7): block 3, after the header and
//! the context block, with no cache breakpoint of its own.
//!
//! The header is one cache entry for every session of a profile, so nothing
//! of one session's goes there (`crates/theseus-core/AGENTS.md`). What is
//! the session's own, and the same for its every request, goes here: its
//! directory, `Directory: /work/harbour-tides`, the one `ToolRuntime::cwd_for`
//! resolves for its tools, the CPUs its commands have, read once per
//! process (`cpu::commands`, theseus-yct32), and the OS and architecture they
//! run on, read once per process too (`cpu::os`, theseus-wadu). Static for the
//! session: it changes only when the session moves (`turn.submit` with another
//! `dir`), which is a `system_changed` recompile, as an edited context file is.
//! So a request that replays a session's prefix replays this block's bytes
//! unchanged.
//!
//! Last comes the project's own instruction files (`project_context`,
//! theseus-wadu; `docs/design/project-context.md` §3): the owner's own
//! user-level file, then for a repository the owner has trusted
//! (`project.trust`, a record in the store) its files under a frame, and for
//! one the owner has not granted, the one
//! line naming how many files there are and how large they are, and nothing
//! else of them. A nested directory's files join the block the first time a
//! tool touches that directory, which is the `system_changed` recompile a
//! session moving directory already is (§2).
//!
//! A granted repository's file that has *changed* since the owner last read
//! it is named here too (§8, "After a grant"), and at `[project] on_change =
//! "ask"` it is named and not read. [`TurnRunner::observe_project`] is the
//! same notice's ledger half.
//!
//! This block takes **no** breakpoint (`layers::cache_layout` leaves it
//! unmarked, whatever its size). The provider's four are the header, the
//! persona, the project layer, and the conversation's automatic one, which
//! sits after this block and so covers it: what changes here changes only
//! the conversation's entry and leaves the three layers above it cached.
//! `tests_layers::the_request_is_three_layers_a_session_block_and_the_messages`
//! holds it (`[true, true, true, false]`).

use crate::ceiling::PlaceView;
use crate::fact::project::ProjectChanged;
use crate::places::PlaceClass;
use crate::turn::{Turn, TurnRunner};

/// The block's name in a compilation's cache layout.
pub const NAME: &str = "session";

impl TurnRunner {
    /// Block 3's text for a session in `dir` (its own, as its client sent
    /// it; none is `[tools] cwd`) speaking in `place`: its directory. Empty,
    /// and so no block, without tools, and in a shared place, where nothing
    /// of the owner's paths goes (the place rule).
    pub fn session_block(&self, dir: Option<&str>, place: PlaceView) -> String {
        if place.class == PlaceClass::Shared || !self.tools.enabled() {
            return String::new();
        }
        let cwd = self.tools.cwd_for(dir);
        let mut text = format!(
            "Directory: {}\n{}\n{}",
            cwd.display(),
            crate::cpu::commands::Cpus::of_commands().fact(),
            crate::cpu::os::Os::of_machine().fact(),
        );
        if let Some(part) =
            crate::project_context::block(&self.cfg, &self.store, &self.context_files, &cwd)
        {
            text.push_str("\n\n");
            text.push_str(&part);
        }
        text
    }

    /// §8's "After a grant", the ledger's half: compare what this compile
    /// reads of a granted repository's instruction files with what the last
    /// one saw, and write a `project.changed` row for each difference, plus
    /// the sighting that now stands, in the turn's next frame. A first
    /// sighting stores the state and says nothing, as a persona's does
    /// (`observe_persona`, whose shape this is).
    ///
    /// The block's half is `project_context::block`, which named the same
    /// changes in the text the model reads. Both come from one
    /// `project_context::change::observe` call on the same inputs, so they
    /// cannot disagree. At `ask` posture with a change there is no sighting
    /// to store -- that is what makes the withheld file keep waiting instead
    /// of arriving by itself on the next turn -- and the row says the text
    /// was not carried.
    pub(crate) fn observe_project(&self, t: &mut Turn<'_>, dir: Option<&str>, place: PlaceView) {
        if place.class == PlaceClass::Shared || !self.tools.enabled() {
            return;
        }
        let cwd = self.tools.cwd_for(dir);
        let found = crate::project_context::discover(&self.cfg.state_dir(), &cwd);
        let Some(root) = found.root.as_deref() else {
            return;
        };
        if found.project.is_empty() || !crate::project_context::trust::granted(&self.store, root) {
            return;
        }
        let seen = crate::project_context::change::observe(
            &self.cfg,
            &self.store,
            &self.context_files,
            root,
            &found.project,
        );
        for c in &seen.fresh {
            t.record(&ProjectChanged {
                id: &seen.id,
                root: &seen.root,
                path: &c.path,
                old: &c.old,
                new: &c.new,
                carried: !seen.withholds(&c.path),
            });
        }
        let Some(write) = &seen.write else { return };
        match write.record(&seen.id) {
            Ok(r) => {
                if let Err(e) = t.tc.store.defer(r) {
                    tracing::warn!(repo = %seen.id, error = %e,
                        "a granted repository's sighting was not stored");
                }
            }
            Err(e) => tracing::warn!(repo = %seen.id, error = %e,
                "a granted repository's sighting cannot be encoded"),
        }
    }
}
