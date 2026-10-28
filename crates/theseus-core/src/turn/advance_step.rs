//! A loop's own line (theseus-xlias): the first line of the text it wrote
//! before its tool calls, carried as `say` on its `loop.ended` row, so the work
//! board can show where a task is in its work with no model call of its own.
//! It is scrubbed first and cut after, so a secret the cut would split is never
//! half shown; a loop with no tool call writes none (its text is the reply).

use super::*;
use crate::scrub::Scrubber;

/// The longest a `say` is, in characters, the ellipsis included.
pub const SAY_CHARS: usize = 120;

/// The first non-empty line of `text`, scrubbed, cut at `SAY_CHARS` at a word.
/// None when the text has no line.
pub fn say_of(text: &str, scrubber: &Scrubber) -> Option<String> {
    let first = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let (line, _) = scrubber.scrub(first);
    let line = line.trim();
    if line.chars().count() <= SAY_CHARS {
        return Some(line.to_string());
    }
    let cut: String = line.chars().take(SAY_CHARS - 1).collect();
    // Back to the last word, unless the line is one long word.
    let at_word = cut.rfind(char::is_whitespace).filter(|&i| i > 0);
    let kept = at_word.map_or(cut.as_str(), |i| &cut[..i]);
    Some(format!("{}…", kept.trim_end()))
}

impl TurnRunner {
    /// The Advancer decides whether the turn continues; the loop's end is
    /// recorded either way. `retry`: the call is made again, an answer cut at
    /// the window (theseus-9p88) or a refusal's fallback (theseus-7gir.18),
    /// which the retry's own line has said.
    pub(super) fn advance(
        &self,
        t: &mut Turn<'_>,
        resp: &ModelResponse,
        uses: usize,
        answered: u32,
        i: u32,
        retry: bool,
    ) -> bool {
        let advancer = UntilNoToolCalls {
            max_loops: t.target.max_loops,
            notify: t.target.on_max_loops == crate::config::MaxLoopsMode::Notify,
        };
        let stop = resp.stop_reason.as_deref();
        let outcome = LoopOutcome {
            loop_index: i,
            provider_stop_reason: resp.stop_reason.clone(),
            tool_calls: answered,
            output_chars: resp.text.chars().count(),
        };
        let a0 = t.trace.now_us();
        let say = (uses > 0)
            .then(|| say_of(&resp.text, &self.tools.scrubber))
            .flatten();
        let decision = if t.awaiting.is_some() {
            Decision::EndTurn("awaiting_confirm".into())
        } else if retry {
            Decision::Continue
        } else if matches!(
            stop,
            Some("refusal") | Some("max_tokens") | Some(WINDOW_EXCEEDED)
        ) {
            Decision::EndTurn(stop.unwrap_or_default().to_string())
        } else {
            advancer.decide(&outcome)
        };
        let on = t.checkpoint.moved_to(&t.target.profile);
        t.record(&fact::turn::LoopEnded {
            turn_id: t.tc.turn_id,
            outcome: &outcome,
            advancer: advancer.name(),
            decision: &decision,
            usage: &resp.usage,
            provider_stop_reason: resp.stop_reason.as_deref(),
            uses,
            answered,
            retry,
            a0,
            say: say.as_deref(),
            on: on.as_deref(),
        });
        match decision {
            Decision::Continue => true,
            Decision::EndTurn(reason) => {
                t.stop_reason = reason;
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A token shape the scrubber knows, built from parts so no whole one sits
    /// in the source.
    fn token() -> String {
        format!("{}{}", "ghp_", "a1B2c3D4".repeat(5))
    }

    #[test]
    fn the_first_line_is_the_say_and_the_rest_is_left() {
        let s = Scrubber::default();
        let say = say_of(
            "\n  Running the tests again with the fix…  \nsecond line",
            &s,
        );
        assert_eq!(
            say.as_deref(),
            Some("Running the tests again with the fix…")
        );
        assert_eq!(say_of("", &s), None);
        assert_eq!(say_of("  \n \n", &s), None);
    }

    #[test]
    fn a_say_is_scrubbed_before_it_is_cut() {
        let t = token();
        let s = Scrubber::with_values(vec![(t.clone(), "gh_token".into())]);
        let say = say_of(&format!("Calling the API with {t} now"), &s).unwrap();
        assert!(!say.contains(&t), "{say}");
        assert!(!say.contains("a1B2c3D4"), "{say}");
        // A secret that straddles the cut is never half shown.
        let pad = "word ".repeat(22);
        let say = say_of(&format!("{pad}{t} tail"), &s).unwrap();
        assert!(!say.contains("ghp_"), "{say}");
        assert!(!say.contains("a1B2"), "{say}");
    }

    #[test]
    fn a_say_is_cut_at_a_word_within_120_characters() {
        let s = Scrubber::default();
        let long = "alpha beta gamma delta ".repeat(20);
        let say = say_of(&long, &s).unwrap();
        assert!(say.chars().count() <= SAY_CHARS, "{}", say.chars().count());
        assert!(say.ends_with("delta…") || say.ends_with("gamma…") || say.ends_with("beta…"));
        assert!(say.ends_with('…'), "{say}");
        // The cut falls on a word: what is left before the ellipsis is whole.
        let kept = say.trim_end_matches('…');
        assert!(
            kept.split(' ')
                .all(|w| ["alpha", "beta", "gamma", "delta"].contains(&w)),
            "{kept}"
        );
        // A line of exactly 120 characters is kept whole.
        let exact = "x".repeat(SAY_CHARS);
        assert_eq!(say_of(&exact, &s).as_deref(), Some(exact.as_str()));
        // One long word is cut inside it, since there is no word to stop at.
        let one = "y".repeat(300);
        let say = say_of(&one, &s).unwrap();
        assert_eq!(say.chars().count(), SAY_CHARS);
    }
}
