//! The date and the git state, in the messages' tail, and what the place tells
//! the model of the person's own routes (the chat route, the voice channel;
//! issue 8, `self_card::told`) (theseus-wadu,
//! `docs/design/project-context.md` §4). Both change within a session, so
//! neither can sit in block 3 (the cached prefix): they are the first part of
//! the harness's block beside the message (`turn::now_step`, issue 46), read
//! once at the turn's first compile. The prefix is byte-identical with or
//! without them.
//!
//! The date is the owner's local day from the clock seam
//! (`theseus_protocol::now_unix_ms`), by the formatter recall headers use.
//! The git line is read through gitoxide (`theseus_tools::git_state`), on the
//! blocking pool, and bounded twice: by its own budget, and by a timeout here
//! that gives `Git: unknown` and goes on. Without tools only the date goes (no
//! path is read). A shared place is told the git state as any place is
//! (2026-11-22: nothing of the context is withheld by a place's class); what it
//! may act on is its tool ceiling's, which this line does not touch.

use super::*;
use std::time::Duration;

/// What the git read may spend itself; the timeout here is its backstop.
const GIT_BUDGET: Duration = Duration::from_millis(150);
const GIT_BACKSTOP: Duration = Duration::from_millis(400);

/// How this part begins, as the view's and the commitments' parts have
/// openers of their own: a reader of a request can tell the harness's block
/// from the conversation's, and a stand-in model that reads the last user
/// text skips it (`task_graph::view::is_head`).
pub const HEAD: &str = "Date: ";

/// `Date: Tue 2026-11-10` for `ms`.
pub(super) fn date_line(ms: u64) -> String {
    format!(
        "{HEAD}{}",
        crate::recall::stamp::day_in(ms, &crate::recall::stamp::system)
    )
}

impl TurnRunner {
    /// The date, the git state where the session may have one, and the
    /// sentences of the place the turn was spoken in; none where
    /// `[project] tail_facts` is off.
    pub(super) async fn env_text(&self, t: &Turn<'_>) -> Option<String> {
        if !self.cfg.project.tail_facts() {
            return None;
        }
        let mut text = date_line(theseus_protocol::now_unix_ms());
        if self.tools.enabled() {
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
        // What the place tells the model of the person's own routes (the chat
        // route, the voice channel), which the header cannot say: it is the same
        // bytes on every surface. A turn addressed to a thread has no place of
        // its own, so it is told those of the place its message came from.
        let at = self.outbox.speaks_at(t.tc.session_id).ok().flatten();
        let place = self
            .place_rule
            .place_under(&self.cfg, at.as_deref(), t.answer_at.as_deref());
        for line in crate::self_card::told(&self.cfg, place) {
            text.push('\n');
            text.push_str(&line);
        }
        // The configured profiles, where the tools that take them may be used (issue 37).
        if let Some(line) = crate::profile_tool::told(&self.cfg, &self.tools, place, &t.tc) {
            text.push('\n');
            text.push_str(&line);
        }
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This block is the harness's, so every reader that tells the harness's
    /// blocks from the person's words must skip it. Without the opener in
    /// `view::is_head`, a stand-in model reads the date as the turn's opening
    /// text (`theseus-sim/src/fake_model/steps.rs`), and the turn bench's
    /// `default_first_request` then finds no request of its own: the request
    /// is sent and answered, only never recognised.
    #[test]
    fn the_block_is_read_as_the_harness_s_and_not_as_the_person_s_words() {
        let date = date_line(1_762_800_000_000);
        assert!(date.starts_with(HEAD), "{date}");
        assert!(crate::task_graph::view::is_head(&date), "{date}");
        // The whole block, as a turn with tools attaches it.
        let both = format!("{date}\nGit: unknown");
        assert!(crate::task_graph::view::is_head(&both), "{both}");
    }
}
