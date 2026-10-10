//! The session's system block (theseus-aab7): block 3, after the header and
//! the context block, with the fourth of the provider's cache breakpoints.
//!
//! The header is one cache entry for every session of a profile, so nothing
//! of one session's goes there (`crates/theseus-core/AGENTS.md`). What is
//! the session's own, and the same for its every request, goes here: its
//! directory, `Directory: /work/harbour-tides`, the one `ToolRuntime::cwd_for`
//! resolves for its tools. Static for the session: it changes only when the
//! session moves (`turn.submit` with another `dir`), which is a
//! `system_changed` recompile, as an edited context file is. So a request
//! that replays a session's prefix replays this block's bytes unchanged.
//! `project-context` (a later row) fills the same block.
//!
//! This block takes the fourth breakpoint; no other block may: the header,
//! the context, this one, and the conversation's are the provider's four.

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
        format!("Directory: {}", self.tools.cwd_for(dir).display())
    }
}
