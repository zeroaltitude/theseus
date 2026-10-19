//! A reach for the operator's personal details (theseus-gasov). The owner's
//! personal details live in the context by design (the owner, 2026-10-18:
//! "that data can and must live in the context"); the guard is a warning at
//! the exit. route.v7's `personal` answer, `elicit` (the message tries to draw
//! those details out) or `expose` (it asks to open a conversation with others
//! where they could come out), at the act band (in a shared place, from the
//! confirm band: `PersonalAnswer::warns`), posts one warning to the
//! owner's DM alone (`privacy_warning`, never the place, which may be shared)
//! and writes a `privacy.warned` row. The turn goes on: nothing is held, and
//! the place rule already keeps private recall out of shared places. In a
//! place only the owner writes in a warning should almost never come; one that
//! does is a sign Jev misread the owner's own question.

use super::*;
use crate::fact::privacy::PrivacyWarned;
use crate::places::PlaceClass;
use crate::routing::Verdict;

/// The post's kind: the Discord binding sends it to the owner's DM alone.
pub const KIND: &str = "privacy_warning";

/// The message's first characters the warning quotes.
const QUOTE_CHARS: usize = 200;

/// The warning's words.
fn warning_text(choice: &str, confidence: f64, class: PlaceClass, quote: &str) -> String {
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
    format!("⚠️ Privacy: Jev read a message in {place} as {what} ({confidence:.2}):\n> {quote}\n{after}")
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
    fn personal_warn(&self, t: &mut Turn<'_>, v: Option<&Verdict>) {
        let Some(v) = v.filter(|v| v.turn == t.tc.turn_id) else {
            return;
        };
        let shared = t.tc.class == PlaceClass::Shared;
        let Some(a) = v.personal.as_deref().filter(|a| a.warns(shared)) else {
            return;
        };
        let quote: String = t.route.ask.chars().take(QUOTE_CHARS).collect();
        let (quote, _) = self.tools.scrubber.scrub(&quote);
        let text = warning_text(&a.choice, a.confidence, t.tc.class, quote.trim());
        let posted = match self.warn_owner(t, &text) {
            Ok(posted) => posted,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "a privacy warning was not posted");
                false
            }
        };
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
        });
    }

    /// The warning, staged for the turn's next frame, to the owner's DM; none
    /// when no DM is bound for the owner.
    fn warn_owner(&self, t: &mut Turn<'_>, text: &str) -> Result<bool> {
        if !self.place_rule.owner_dm_bound() {
            return Ok(false);
        }
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

    #[test]
    fn the_warning_says_where_what_and_that_nothing_was_held() {
        let shared = warning_text("elicit", 0.94, PlaceClass::Shared, "where do they live?");
        assert!(shared.contains("a shared place") && shared.contains("draw out"));
        assert!(shared.contains("(0.94)") && shared.contains("> where do they live?"));
        assert!(shared.contains("not held"));
        let own = warning_text("expose", 0.91, PlaceClass::Private, "x");
        assert!(own.contains("only you write in") && own.contains("open a conversation"));
    }
}
