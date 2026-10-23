//! A place's home grows past a size (theseus-jg1eg, the follow-up of theseus-vydhq): on 2026-10-19 a
//! one-line message in the owner's DM cost 282k request tokens, the whole history of a home nobody
//! had split, and nothing stopped it growing back after the split of 2026-10-20 took its next
//! request from 253,640 to 32,797 tokens. A split moves context, so the daemon never makes one on
//! its own: it tells the owner, once, and gives the command.
//!
//! - **The measure** is the home's request as the compile counts it (`Compiled::est_tokens`, the
//!   figure a turn already has: no read of its own), taken at a turn's first loop.
//! - **The bound** is `[threads] home_warn_percent` (50) of the serving model's context window
//!   (the catalog's `context_window`; 1,000,000 when none is known): a home is warned about when
//!   a request is near the window, where a request can fail and every turn is costly. A long
//!   session is otherwise fine, so no smaller figure warns: the owner, 2026-10-21, found the
//!   60,000 of `[threads] working_tokens` (what a thread's request stays within after a cold
//!   resume, a different thing) too aggressive for a home. No budget of its own, and none is raised.
//! - **A home** is the session a place runs on (`Outbox::place_session`), not a task's and not a
//!   thread's.
//! - **Once** is a META row by the home's session, `home.size.warned.<session>`, written in the
//!   frame that carries the post, so a restart keeps it and a post never goes without it. A
//!   successor home (`bind_place`) is another session, so it starts unwarned. A home whose request
//!   falls back to the bound or under it (a split took its history out) has its row written
//!   `warned: false`, and is told again if it grows past the bound once more.
//! - **The text** names the place as the owner sees it, the size and its share of the window, and
//!   one command that runs as written: a dry-run split of the home's oldest settled stretch (its
//!   first turn through the turn before the newest `[threads] resume_keep_turns`), named
//!   `earlier-<date of the stretch's last turn>`. A home with no such stretch gets the size and
//!   `theseus history`.
//! - **Where** it goes is the owner's DM alone, as the other warnings go (`stage_to_operator`; the
//!   Discord binding posts kind `home_size` there). With no DM bound nothing is posted and nothing
//!   is marked, so the home is told when one is.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use theseus_store::{kinds, NewRecord};

use crate::store::Transcript;
use crate::stub::Kind;

use super::*;

/// The window a request is weighed against when its model's is not known.
const FALLBACK_WINDOW: u64 = 1_000_000;

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

/// The oldest settled stretch of a home, as `thread split` takes it.
#[derive(Debug, PartialEq, Eq)]
pub struct Stretch {
    /// The thread's name: `earlier-` and the local date of the stretch's last turn.
    pub slug: String,
    /// The WAL positions of the stretch's first and last messages.
    pub first: u64,
    pub last: u64,
}

/// The warning, as the owner reads it: the place's name, the size and its share of the window,
/// and the dry run of a split of `stretch` (as the place is stored) with its undo.
pub fn warning_text(
    name: &str,
    tokens: u64,
    window: u64,
    split: Option<(&str, &Stretch)>,
) -> String {
    let n = crate::narrative::thousands;
    let trigger = format!(
        "it holds {} tokens, {}% of the model's window, past the warning line",
        n(tokens),
        tokens * 100 / window.max(1)
    );
    let body = match split {
        Some((place, s)) => format!(
            "To see what moving its oldest turns into a thread would save: `theseus thread \
             split home@{place} {} --first @{} --last @{} --dry-run`. Without `--dry-run` it \
             splits, and `theseus thread unsplit` with the id it prints undoes it.",
            s.slug, s.first, s.last
        ),
        None => "`theseus history` shows what it holds.".into(),
    };
    crate::owner_note::note(
        crate::owner_note::Kind::Fyi,
        &format!("{name}'s home is getting long"),
        &trigger,
        &body,
    )
}

/// The home's oldest settled stretch: its first turn through the turn before the newest `keep`,
/// by the turns of `nodes` that no split has moved. None when no turn is left before the newest
/// `keep`. A turn is the messages, calls and results that share a turn id.
pub fn stretch(nodes: &Transcript, keep: usize, taken: impl Fn(&str) -> bool) -> Option<Stretch> {
    let hidden = crate::moved::read(nodes).hidden;
    let mut order: Vec<&str> = Vec::new();
    let mut span: HashMap<&str, (u64, u64)> = HashMap::new();
    for (pos, n) in nodes {
        let Some(turn) = n.turn_id.as_deref() else {
            continue;
        };
        if matches!(n.kind, Kind::Recall | Kind::Summary | Kind::Moved) || hidden.contains(&n.id) {
            continue;
        }
        let e = span.entry(turn).or_insert_with(|| {
            order.push(turn);
            (*pos, *pos)
        });
        e.1 = *pos;
    }
    let settled = order.get(..order.len().checked_sub(keep)?)?;
    let last_turn = settled.last()?;
    let first = span[settled[0]].0;
    let last = settled.iter().map(|t| span[t].1).max()?;
    let day = crate::judge::spend::local_day(crate::id_ms(last_turn)?);
    let slug = (1..)
        .map(|i| match i {
            1 => format!("earlier-{day}"),
            i => format!("earlier-{day}-{i}"),
        })
        .find(|s| !taken(s))?;
    Some(Stretch { slug, first, last })
}

impl TurnRunner {
    /// The first loop's request of a turn, `tokens` as the compile counted it: when it is a
    /// home's and passes the bound, one post to the owner's DM and the marker, in the turn's next
    /// frame. A failure is logged; the turn is no worse for it. Once a turn: a turn routing moves
    /// compiles its first loop twice, and the marker waits for the frame, so a second weigh would
    /// read "not told" and post again.
    pub(super) fn home_size_checked(&self, t: &mut Turn<'_>, tokens: u64, loop_index: u32) {
        let window = t.fill.window.unwrap_or(FALLBACK_WINDOW);
        if loop_index > 0 || t.home_weighed {
            return;
        }
        t.home_weighed = true;
        if let Err(e) = self.home_size(t, tokens, window) {
            tracing::warn!(error = %format!("{e:#}"), "a home's size was not weighed");
        }
    }

    /// The place as the owner sees it (`#harbor`, `DM @name`); a place no binding names is
    /// "this place", never its raw target.
    fn home_name(&self, sid: &str, target: &str) -> String {
        let name = self.place_name(sid);
        if name == target {
            "This place".into()
        } else {
            name
        }
    }

    /// The home's oldest settled stretch, read from its session as it is now.
    fn home_stretch(&self, t: &Turn<'_>) -> Result<Option<Stretch>> {
        let nodes = t.tc.store.transcript(t.tc.session_id)?;
        let keep = self.cfg.threads.resume_keep_turns as usize;
        let board = self.threads.held();
        Ok(stretch(&nodes, keep, |slug| {
            board.as_ref().is_some_and(|b| b.get(slug).is_some())
        }))
    }

    fn home_size(&self, t: &mut Turn<'_>, tokens: u64, window: u64) -> Result<()> {
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
        let threshold = window * u64::from(self.cfg.threads.home_warn_percent) / 100;
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
            let split = self.home_stretch(t)?;
            let text = warning_text(
                &self.home_name(sid, &place),
                tokens,
                window,
                split.as_ref().map(|s| (stored, s)),
            );
            let body = json!({"kind": KIND, "text": text});
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
        let s = Stretch {
            slug: "earlier-2026-10-19".into(),
            first: 12,
            last: 412,
        };
        let t = warning_text("#harbor", 512_300, 1_000_000, Some(("channel:77", &s)));
        for want in [
            "#harbor's home is getting long\nit holds 512,300 tokens, 51% of the model's window",
            "theseus thread split home@channel:77 earlier-2026-10-19 --first @12 --last @412 --dry-run",
            "theseus thread unsplit",
        ] {
            assert!(t.contains(want), "{want}: {t}");
        }
        assert!(t.chars().count() < 450, "{t}");
        let t = warning_text("#harbor", 512_300, 1_000_000, None);
        assert!(t.contains("theseus history") && !t.contains("split"), "{t}");
    }
}
