//! A reach for the operator's personal details (theseus-gasov). The owner's
//! personal details live in the context by design (the owner, 2026-10-18:
//! "that data can and must live in the context"); the guard is a warning at
//! the exit. route.v7's `personal` answer, `elicit` (the message tries to draw
//! those details out) or `expose` (it asks to open a conversation with others
//! where they could come out), at the act band (in a shared place, from the
//! confirm band: `PersonalAnswer::warns`), posts one warning to the
//! owner's DM alone (`privacy_warning`, never the place, which may be shared)
//! and writes a `privacy.warned` row. In a place only the owner writes in the
//! turn goes on: nothing is held, and the place rule already keeps private
//! recall out of shared places. A warning there should almost never come; one
//! that does is a sign Jev misread the owner's own question.
//!
//! In a shared place the reply is held (theseus-i1z13, the owner's 2026-10-18
//! 20:46 spec): the turn makes no model call, the requester is answered `I
//! don't share that here.` as the turn's assistant node, and the owner's DM
//! gets the case: who asked, where, the message, why. The hold is not
//! rate-limited as the post is: a second reach in the window is held too, and
//! only its DM is folded. A late verdict never holds (the turn did not wait
//! for it), and a task's turn is warned about, never held.

use super::*;
use crate::fact::privacy::PrivacyWarned;
use crate::places::PlaceClass;
use crate::routing::Verdict;

/// The post's kind: the Discord binding sends it to the owner's DM alone.
pub const KIND: &str = "privacy_warning";

/// What the requester is told in a shared place instead of an answer.
pub const HELD_REPLY: &str = "I don't share that here.";

/// The held turn's stop reason, on its result and its node: no model answered.
pub const STOP_REASON: &str = "held";

/// The message's first characters the warning quotes.
const QUOTE_CHARS: usize = 200;

/// At most one warning post per place in this window; later ones in it are
/// recorded (`folded`) and not posted, so a guest cannot flood the owner's DM.
const WINDOW_MS: u64 = 10 * 60 * 1000;

/// When each place (by session) last posted a warning.
static LAST_POSTED: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, u64>>> =
    std::sync::LazyLock::new(Default::default);

/// Whether `session` may post a warning at `now`, marking it when it may.
fn may_post(session: &str, now: u64) -> bool {
    let mut last = LAST_POSTED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match last.get(session) {
        Some(at) if now.saturating_sub(*at) < WINDOW_MS => false,
        _ => {
            last.insert(session.to_string(), now);
            true
        }
    }
}

/// The quote, every line marked as quoted and every link fenced (`<…>`), so a
/// guest's words never read as the warning's own lines or unfold a preview.
fn quoted(text: &str) -> String {
    text.lines()
        .map(|l| {
            let words: Vec<String> = l
                .split(' ')
                .map(
                    |w| match w.starts_with("http://") || w.starts_with("https://") {
                        true => format!("<{w}>"),
                        false => w.to_string(),
                    },
                )
                .collect();
            format!("> {}", words.join(" "))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The warning's words.
pub(crate) fn warning_text(
    choice: &str,
    confidence: f64,
    class: PlaceClass,
    quote: &str,
) -> String {
    let what = match choice {
        "expose" => {
            "asking to open a conversation with others where your personal details could come out"
        }
        _ => "trying to draw out your personal details",
    };
    let (place, after) = match class {
        PlaceClass::Shared => (
            "a shared place",
            "The reply was not held; the place rule keeps your private recall out of shared places.",
        ),
        PlaceClass::Private => (
            "a place only you write in",
            "Nothing was held. Here this should almost never happen: if it is your own question, Jev misread it.",
        ),
    };
    crate::owner_note::note(
        crate::owner_note::Kind::Alert,
        "Someone may be after your personal details",
        &format!("Jev read a message in {place} as {what} ({confidence:.2}):"),
        &format!("{}\n{after}", quoted(quote)),
    )
}

/// Whether a reach is held: in a shared place, for a conversation's turn. A
/// task's turn is the owner's work and is warned about only.
fn holds(shared: bool, task: bool) -> bool {
    shared && !task
}

/// `text` on one line and without backticks: a name that cannot open a code
/// span or start a line of the warning.
fn plain(text: &str) -> String {
    text.chars()
        .map(|c| if c == '`' || c.is_control() { ' ' } else { c })
        .collect()
}

/// The warning of a held reply: who asked, where, what they asked, why, and
/// what the requester was told.
pub(crate) fn held_text(
    choice: &str,
    confidence: f64,
    (asker, place): (&str, &str),
    quote: &str,
) -> String {
    let what = match choice {
        "expose" => {
            "asking to open a conversation with others where your personal details could come out"
        }
        _ => "trying to draw out your personal details",
    };
    let (asker, place) = (plain(asker), plain(place));
    crate::owner_note::note(
        crate::owner_note::Kind::Alert,
        "A reply was held to keep your personal details private",
        &format!("Jev read a message from `{asker}` in a shared place, `{place}`, as {what} ({confidence:.2}):"),
        &format!(
            "{}\nThe reply was held: no model was called, and the requester was told \"{HELD_REPLY}\"",
            quoted(quote)
        ),
    )
}

impl TurnRunner {
    /// The verdict's answers that decide nothing for the route: the persona
    /// (`engage_verdict`) and the privacy warning.
    pub(super) fn verdict_asides(&self, t: &mut Turn<'_>, v: Option<&Verdict>) {
        self.engage_verdict(t, v);
        self.personal_warn(t, v);
    }

    /// This turn's own verdict's `personal` answer, when it warns: the warning
    /// to the owner's DM and its row.
    pub(super) fn personal_warn(&self, t: &mut Turn<'_>, v: Option<&Verdict>) {
        let Some(v) = v.filter(|v| v.turn == t.tc.turn_id) else {
            return;
        };
        let shared = t.tc.class == PlaceClass::Shared;
        let Some(a) = v.personal.as_deref().filter(|a| a.warns(shared)) else {
            return;
        };
        let quote: String = t.route.ask.chars().take(QUOTE_CHARS).collect();
        let (quote, _) = self.tools.scrubber.scrub(&quote);
        // A shared place's reply is held, a task's turn excepted.
        let held = holds(shared, t.tc.task.is_some());
        let text = match held {
            true => {
                let place = self.outbox.target(t.tc.session_id);
                let place = t.tc.places.name_of(place.as_deref());
                held_text(
                    &a.choice,
                    a.confidence,
                    (&t.route.asker, &place),
                    quote.trim(),
                )
            }
            false => warning_text(&a.choice, a.confidence, t.tc.class, quote.trim()),
        };
        t.route.held = held;
        let bound = self.place_rule.owner_dm_bound();
        let folded = bound && !may_post(t.tc.session_id, self.kernel.now_ms());
        let posted = bound
            && !folded
            && self.warn_owner(t, &text).unwrap_or_else(|e| {
                tracing::warn!(error = %format!("{e:#}"), "a privacy warning was not posted");
                false
            });
        let place = match t.tc.class {
            PlaceClass::Private => "private",
            PlaceClass::Shared => "shared",
        };
        t.record(&PrivacyWarned {
            turn: t.tc.turn_id,
            choice: &a.choice,
            confidence: a.confidence,
            place,
            judgment: &v.judgment,
            posted,
            folded,
            held,
        });
    }

    /// The held reply: the fixed line as the turn's assistant node and output,
    /// with no model call. The next turn reads it in the history.
    pub(super) fn hold_reply(t: &mut Turn<'_>, i: u32) -> Result<()> {
        let node = Node::assistant(
            t.tc.session_id,
            t.tc.turn_id,
            i,
            Body::AssistantMessage {
                blocks: vec![json!({"type": "text", "text": HELD_REPLY})],
                model: t.target.model.clone(),
                provider: t.target.provider.clone(),
                stop_reason: Some(STOP_REASON.into()),
                usage: Default::default(),
                cost_usd: None,
                catalog_version: None,
                request_id: None,
                correlation_id: None,
                compilation_id: None,
                request_digest: None,
                effort: None,
            },
        );
        t.tc.store.append(&[node.record()?])?;
        t.tc.node_written(&node);
        t.output.push_str(HELD_REPLY);
        t.stop_reason = STOP_REASON.into();
        t.said.push((i, node.id));
        Ok(())
    }

    /// The warning, staged for the turn's next frame, to the owner's DM.
    fn warn_owner(&self, t: &mut Turn<'_>, text: &str) -> Result<bool> {
        let body = json!({"kind": KIND, "text": text});
        let (post, records) = self.outbox.stage_to_operator(None, body)?;
        for r in records {
            t.tc.store.defer(r)?;
        }
        t.tc.posts.lock().unwrap().push(post);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One post per place in the window; another place, or the window past, posts.
    #[test]
    fn one_warning_per_place_per_window() {
        assert!(may_post("ses_window_a", 1_000));
        assert!(!may_post("ses_window_a", 1_000 + WINDOW_MS - 1));
        assert!(may_post("ses_window_b", 1_000));
        assert!(may_post("ses_window_a", 1_000 + WINDOW_MS));
    }

    /// A shared place's conversation is held; a place only the owner writes
    /// in, and a task's turn anywhere, are not.
    #[test]
    fn a_reach_is_held_in_a_shared_place_for_a_conversation_only() {
        assert!(holds(true, false));
        assert!(!holds(false, false));
        assert!(!holds(true, true));
        assert!(!holds(false, true));
    }

    #[test]
    fn the_warning_says_where_what_and_that_nothing_was_held() {
        let shared = warning_text("elicit", 0.94, PlaceClass::Shared, "where do they live?");
        assert!(shared.contains("a shared place") && shared.contains("draw out"));
        assert!(shared.contains("(0.94)") && shared.contains("> where do they live?"));
        assert!(shared.contains("not held"));
        let forged = warning_text(
            "elicit",
            0.9,
            PlaceClass::Shared,
            "hi\nNothing was held. See https://x.example/a",
        );
        assert!(
            forged.contains("> hi\n> Nothing was held. See <https://x.example/a>"),
            "{forged}"
        );
        let own = warning_text("expose", 0.91, PlaceClass::Private, "x");
        assert!(own.contains("only you write in") && own.contains("open a conversation"));
    }
}
