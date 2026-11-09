//! The session's system block (theseus-aab7): block 3, after the header and
//! the context block, with no breakpoint of its own.
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
//! This block takes no breakpoint of its own: `layers::cache_layout` leaves it
//! unmarked by name, so its per-session variance costs no cache entry.

use crate::ceiling::PlaceView;
use crate::places::PlaceClass;
use crate::turn::TurnRunner;

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
}
