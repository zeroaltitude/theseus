//! A place's home grows past a size (theseus-jg1eg, the follow-up of theseus-vydhq): on 2026-10-19 a
//! one-line message in the owner's DM cost 282k request tokens, the whole history of a home nobody
//! had split, and nothing stopped it growing back after the split of 2026-10-20 took its next
//! request from 253,640 to 32,797 tokens. A split moves context, so the daemon never makes one on
//! its own: it tells the owner, once, and gives the command.
//!
//! - **The measure** is the home's request as the compile counts it (`Compiled::est_tokens`, the
//!   figure a turn already has: no read of its own), taken at a turn's first loop.
//! - **The bound** is `[threads] working_tokens` (60,000 unless set): the tokens a thread's request
//!   stays within after a cold resume. A home past what a thread is held to is the one a split would
//!   shrink; the post-split DM, 32,797, sits under it. No budget of its own, and none is raised.
//! - **A home** is the session a place runs on (`Outbox::place_session`), not a task's and not a
//!   thread's.
//! - **Once** is a META row by the home's session, `home.size.warned.<session>`, written in the
//!   frame that carries the post, so a restart keeps it and a post never goes without it. A
//!   successor home (`bind_place`) is another session, so it starts unwarned. A home whose request
//!   falls back to the bound or under it (a split took its history out) has its row written
//!   `warned: false`, and is told again if it grows past the bound once more.
//! - **Where** it goes is the owner's DM alone, as the other warnings go (`stage_to_operator`; the
//!   Discord binding posts kind `home_size` there). With no DM bound nothing is posted and nothing
//!   is marked, so the home is told when one is.

use serde::{Deserialize, Serialize};
use theseus_store::{kinds, NewRecord};

use super::*;

/// The post's kind: the Discord binding sends it to the owner's DM alone.
pub const KIND: &str = "home_size";

/// The META key of a home's marker, by its session.
pub const KEY_PREFIX: &str = "home.size.warned.";

/// What the owner was told about a home, and whether it still holds.
#[derive(Debug, Serialize, Deserialize)]
struct Marker {
    /// Told, and not since under the bound.
    warned: bool,
    place: String,
    /// The request's tokens when it was written.
    tokens: u64,
    /// The bound it was weighed against.
    threshold: u64,
    at_ms: u64,
}

/// What a turn's measure does to a home's marker.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    /// Past the bound and not told yet.
    Warn,
    /// Back at or under the bound after a warning: told again if it grows.
    Rearm,
    Quiet,
}

fn step(tokens: u64, threshold: u64, warned: bool) -> Step {
    match (tokens > threshold, warned) {
        (true, false) => Step::Warn,
        (false, true) => Step::Rearm,
        _ => Step::Quiet,
    }
}

/// The warning, as the owner reads it: the size, the bound, the command, its undo.
pub fn warning_text(place: &str, tokens: u64, threshold: u64) -> String {
    let n = crate::narrative::thousands;
    format!(
        "📏 The home of {place} has grown: its next request is {} tokens, past the {} a thread's \
         request stays within (`[threads] working_tokens`). Every turn there pays for it, and after \
         an idle spell the whole of it is written to the cache again. Nothing was split: a split \
         moves context, so it is yours to choose. To see what would move and what the next request \
         would be, run `theseus thread split home@{place} NAME --first @A --last @B --dry-run` \
         (`theseus history` shows the `@` positions); without `--dry-run` it splits, and \
         `theseus thread unsplit SPLIT` undoes it. You will not be told again about this home \
         unless it falls back under the bound and grows past it once more.",
        n(tokens),
        n(threshold)
    )
}

impl TurnRunner {
    /// The first loop's request of a turn, `tokens` as the compile counted it: when it is a
    /// home's and passes the bound, one post to the owner's DM and the marker, in the turn's next
    /// frame. A failure is logged; the turn is no worse for it. Once a turn: a turn routing moves
    /// compiles its first loop twice, and the marker waits for the frame, so a second weigh would
    /// read "not told" and post again.
    pub(super) fn home_size_checked(&self, t: &mut Turn<'_>, tokens: u64, loop_index: u32) {
        if loop_index > 0 || t.home_weighed {
            return;
        }
        t.home_weighed = true;
        if let Err(e) = self.home_size(t, tokens) {
            tracing::warn!(error = %format!("{e:#}"), "a home's size was not weighed");
        }
    }

    fn home_size(&self, t: &mut Turn<'_>, tokens: u64) -> Result<()> {
        let sid = t.tc.session_id;
        if !self.cfg.threads.enabled || t.tc.task.is_some() {
            return Ok(());
        }
        // The target is the place as `theseus places` prints it (`discord:dm:ID`), the place's
        // record is keyed by its stored name (`dm:ID`); `thread split` takes either.
        let Some(place) = self.outbox.target(sid) else {
            return Ok(());
        };
        let stored = place.strip_prefix("discord:").unwrap_or(&place);
        if self.outbox.place_session(stored)?.as_deref() != Some(sid) {
            return Ok(());
        }
        let threshold = self.cfg.threads.working_tokens;
        let key = format!("{KEY_PREFIX}{sid}");
        let warned =
            t.tc.store
                .get_meta::<Marker>(&key)?
                .is_some_and(|m| m.warned);
        let what = step(tokens, threshold, warned);
        if what == Step::Quiet {
            return Ok(());
        }
        let mut marker = Marker {
            warned: what == Step::Warn,
            place: place.clone(),
            tokens,
            threshold,
            at_ms: self.kernel.now_ms(),
        };
        if what == Step::Warn {
            if !self.place_rule.owner_dm_bound() {
                tracing::info!(
                    place,
                    tokens,
                    threshold,
                    "a home is past its size, and no DM with the owner is bound to tell"
                );
                return Ok(());
            }
            let body = json!({"kind": KIND, "text": warning_text(&place, tokens, threshold)});
            let (post, records) = self.outbox.stage_to_operator(None, body)?;
            for r in records {
                t.tc.store.defer(r)?;
            }
            t.tc.posts.lock().unwrap().push(post);
            tracing::warn!(
                place,
                tokens,
                threshold,
                "a place's home is past its size: the owner was told how to split it"
            );
        }
        marker.warned = what == Step::Warn;
        t.tc.store
            .defer(NewRecord::json(kinds::META, Some(&key), &marker)?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_home_is_told_once_past_the_bound_and_armed_again_under_it() {
        assert_eq!(step(60_001, 60_000, false), Step::Warn);
        assert_eq!(
            step(60_000, 60_000, false),
            Step::Quiet,
            "the bound itself is within"
        );
        assert_eq!(step(90_000, 60_000, true), Step::Quiet, "told already");
        assert_eq!(
            step(32_797, 60_000, true),
            Step::Rearm,
            "the split's request"
        );
        assert_eq!(step(32_797, 60_000, false), Step::Quiet);
    }

    #[test]
    fn the_warning_names_the_size_the_bound_and_the_commands() {
        let t = warning_text("discord:dm:5150", 253_640, 60_000);
        for want in [
            "253,640",
            "60,000",
            "theseus thread split home@discord:dm:5150 NAME",
            "--dry-run",
            "theseus thread unsplit SPLIT",
        ] {
            assert!(t.contains(want), "{want}: {t}");
        }
    }
}
