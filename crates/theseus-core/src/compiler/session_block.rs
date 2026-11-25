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
//! `dir`), which reaches the next turn, as an edited context file does.
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
//! tool touches that directory, which reaches the next turn, as a session
//! moving directory does (§2).
//!
//! A granted repository's file that has *changed* since the owner last read
//! it is named here too (§8, "After a grant"), and it is named and not read
//! at `[project] on_change = "ask"`, or when the compiling session carries
//! `injection`, or when a session carrying `injection` has acted or been
//! labelled since the owner last read the repository -- the last of which
//! holds every granted repository's new text for **every** session, this one
//! included, whether or not it carries anything, until the owner runs
//! `theseus project trust` on it (S31-1's two keys, `project_context::change`).
//! Each holds whatever that setting says. `turn/project_step.rs` is the same
//! notice's ledger half.
//!
//! This block takes **no** breakpoint (`layers::cache_layout` leaves it
//! unmarked, whatever its size). The provider's four are the header, the
//! persona, the project layer, and the conversation's automatic one, which
//! sits after this block and so covers it: what changes here changes only
//! the conversation's entry and leaves the three layers above it cached.
//! `tests_layers::the_request_is_three_layers_a_session_block_and_the_messages`
//! holds it (`[true, true, true, false]`).

use crate::ceiling::PlaceView;
use crate::session::SessionRecord;
use crate::turn::TurnRunner;

/// The block's name in a compilation's cache layout.
pub const NAME: &str = "session";

impl TurnRunner {
    /// Whether `session` carries `injection` (`SessionRecord::flagged`), as
    /// the store holds it now: read from the record, never a turn's copy of
    /// it, which a label raised while that turn ran is not in.
    ///
    /// It reads no mode. What `[content_based_security] injection` decides
    /// is whether the session's calls that act are refused; that something
    /// tried to steer the session is the same fact either way, and the hold
    /// below must not depend on which mode happens to be configured
    /// (S31-1).
    ///
    /// **It lives here, beside its one caller**: it was first written in
    /// `turn/data_step.rs`, a keel-held file, and a convenience that could sit
    /// outside a guarded file has no business inside one. The guard said so.
    pub(crate) fn session_flagged(&self, session: &str) -> bool {
        self.store
            .get_session::<SessionRecord>(session)
            .ok()
            .flatten()
            .is_some_and(|r| r.flagged())
    }
    /// Block 3's text for a session in `dir` (its own, as its client sent
    /// it; none is `[tools] cwd`): its directory. Empty, and so no block,
    /// without tools. Every place gets it (2026-11-22: a place's class no
    /// longer withholds context); `_place` stays for its callers until the
    /// sweep.
    ///
    /// `flagged` is whether the session carries `injection`
    /// (`SessionRecord::flagged`): a granted repository's *changed*
    /// instruction file is then named and not read, whatever `[project]
    /// on_change` says (S31-1's reader key). Every caller passes what the
    /// session's own record says, and the turn reads it **once** and hands
    /// the same answer here and to `observe_project`, so the block and the
    /// ledger cannot disagree about what was carried.
    ///
    /// It is **not** the only thing that holds a change. The act key's
    /// stored hold is read inside `project_context::change::observe`, from
    /// the store, and needs nothing of this session: once a labelled session
    /// has acted or been labelled, a change is held here for a session
    /// carrying nothing at all, which is the case the reader key cannot see.
    pub fn session_block(&self, dir: Option<&str>, place: PlaceView, flagged: bool) -> String {
        self.session_block_and_own(dir, place, flagged).0
    }

    /// [`Self::session_block`], and the record of the owner's own file's
    /// accepted version it carries, read with it
    /// (`project_context::block_and_own`): what the turn's compiled rows
    /// name.
    pub(crate) fn session_block_and_own(
        &self,
        dir: Option<&str>,
        _place: PlaceView,
        flagged: bool,
    ) -> (String, Option<crate::compiler::ContextFileRef>) {
        if !self.tools.enabled() {
            return (String::new(), None);
        }
        let cwd = self.tools.cwd_for(dir);
        let mut text = format!(
            "Directory: {}\n{}\n{}",
            cwd.display(),
            crate::cpu::commands::Cpus::of_commands().fact(),
            crate::cpu::os::Os::of_machine().fact(),
        );
        let (part, own) = crate::project_context::block_and_own(
            &self.cfg,
            &self.store,
            &self.context_files,
            &cwd,
            flagged,
        );
        if let Some(part) = part {
            text.push_str("\n\n");
            text.push_str(&part);
        }
        (text, own)
    }
}
