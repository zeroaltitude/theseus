//! The date and the git state, in the messages' tail (theseus-wadu,
//! `docs/design/project-context.md` §4). Both change within a session, so
//! neither can sit in block 3 (the cached prefix): they ride the tail as one
//! block after the conversation's breakpoint, through `attach_hint`, as the
//! deep-coding hint does. The prefix is byte-identical with or without them.
//!
//! The date is the owner's local day from the clock seam
//! (`theseus_protocol::now_unix_ms`), by the formatter recall headers use.
//! The git line is read through gitoxide (`theseus_tools::git_state`), on the
//! blocking pool, and bounded twice: by its own budget, and by a timeout here
//! that gives `Git: unknown` and goes on. In a shared place, or without
//! tools, only the date goes (no path of the owner's is read).

use super::*;
use std::time::Duration;

/// What the git read may spend itself; the timeout here is its backstop.
const GIT_BUDGET: Duration = Duration::from_millis(150);
const GIT_BACKSTOP: Duration = Duration::from_millis(400);

/// `Date: Tue 2026-11-10` for `ms`.
pub(super) fn date_line(ms: u64) -> String {
    format!(
        "Date: {}",
        crate::recall::stamp::day_in(ms, &crate::recall::stamp::system)
    )
}

impl TurnRunner {
    /// Attach the date, and the git state where the session may have one.
    pub(super) async fn env_tail(&self, t: &Turn<'_>, compiled: &mut Compiled) {
        if !self.cfg.project.tail_facts() {
            return;
        }
        let mut text = date_line(theseus_protocol::now_unix_ms());
        if t.tc.place().class != crate::places::PlaceClass::Shared && self.tools.enabled() {
            let cwd = self.tools.cwd_for(t.tc.dir);
            let read = tokio::task::spawn_blocking(move || {
                theseus_tools::git_state::line(&cwd, GIT_BUDGET)
            });
            let line = match tokio::time::timeout(GIT_BACKSTOP, read).await {
                Ok(Ok(l)) => l,
                _ => "Git: unknown".to_string(),
            };
            text.push('\n');
            text.push_str(&line);
        }
        Self::attach_hint(compiled, &text);
    }
}
