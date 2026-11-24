//! WHO AM I (the owner's shape of a turn's context, 2026-11-24: who am I; who am I talking to; topic; here;
//! everywhere): a few lines laid by code at the head of the first user message, first in the front. They
//! name the profile the turn runs on and its model, the persona in play by name with its card's one line,
//! and where to read more. Built from the request's own spec and the persona's card alone: no secret, no
//! path, no setting's text, nothing of the persona's files. The same bytes from one turn to the next until
//! the profile, its model or the persona changes, so they sit in the cached front. The soul text stays in
//! the system part: it is the voice, not a description.

use super::text::tokens;

/// The most characters of a name (a profile's, a model's, a persona's) the block lays.
pub const NAME_CHARS: usize = 80;
/// The most characters of the persona's card line the block lays.
pub const CARD_CHARS: usize = 200;
/// Its bound, in tokens by today's estimate (a test holds every block under it).
pub const MOST_TOKENS: u64 = 300;
/// How the block begins: the harness's words, never the person's (`task_graph::view::is_head` lists it, so a
/// stand-in model that reads the person's words skips it).
pub const HEAD: &str = "Who you are, as Theseus's settings stand for this turn (laid by code):";

/// What the block names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Who<'a> {
    /// The profile the turn runs on, by its name in the settings.
    pub profile: &'a str,
    pub model: &'a str,
    /// The persona in play, by its display name.
    pub persona: &'a str,
    /// Its card, as the persona's settings hold it; none for the built-in one.
    pub card: Option<&'a str>,
}

/// The block's text: four lines.
pub fn text(w: &Who<'_>) -> String {
    let persona = match w.card.and_then(card_line) {
        Some(card) => format!("- persona: {}: {card}", cut(w.persona, NAME_CHARS)),
        None => format!("- persona: {}", cut(w.persona, NAME_CHARS)),
    };
    [
        HEAD.to_string(),
        format!(
            "- profile: {}; model: {}",
            cut(w.profile, NAME_CHARS),
            cut(w.model, NAME_CHARS)
        ),
        persona,
        "- to read more: config_show for the settings, look_theseus for what this turn was shown and why"
            .to_string(),
    ]
    .join("\n")
}

/// Its size by today's estimate.
pub fn size(w: &Who<'_>) -> u64 {
    tokens(&text(w))
}

/// A card's first line that holds anything, cut to [`CARD_CHARS`].
fn card_line(card: &str) -> Option<String> {
    let line = card.lines().map(str::trim).find(|l| !l.is_empty())?;
    Some(cut(line, CARD_CHARS))
}

/// `s` on one line, without control characters, at most `most` characters (cut on a character's boundary).
fn cut(s: &str, most: usize) -> String {
    let one: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let one = one.trim();
    match one.char_indices().nth(most) {
        Some((at, _)) => format!("{}...", one[..at].trim_end()),
        None => one.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn who<'a>(card: Option<&'a str>) -> Who<'a> {
        Who {
            profile: "everyday",
            model: "claude-sonnet-5-5",
            persona: "Kestrel",
            card,
        }
    }

    #[test]
    fn who_am_i_names_the_profile_model_and_persona() {
        let t = text(&who(Some("A patient harbour pilot.\nSecond line, never laid.")));
        assert_eq!(t.lines().count(), 4, "{t}");
        assert!(t.contains("- profile: everyday; model: claude-sonnet-5-5"), "{t}");
        assert!(t.contains("- persona: Kestrel: A patient harbour pilot."), "{t}");
        assert!(!t.contains("Second line"), "the card's one line only: {t}");
        assert!(t.contains("config_show") && t.contains("look_theseus"));
        assert!(crate::task_graph::view::is_head(&t), "the harness's own block");
        let built_in = text(&who(None));
        assert!(built_in.contains("- persona: Kestrel\n"), "{built_in}");
    }

    #[test]
    fn who_am_i_is_under_three_hundred_tokens() {
        let long = "x".repeat(10_000);
        let w = Who {
            profile: &long,
            model: &long,
            persona: &long,
            card: Some(&long),
        };
        assert!(size(&w) < MOST_TOKENS, "{} tokens", size(&w));
        assert!(size(&who(Some("A patient harbour pilot."))) < 100);
        assert!(!text(&w).contains('\r'));
        let control = Who {
            card: Some("one\u{7}two"),
            ..who(None)
        };
        assert!(text(&control).contains("one two"));
    }
}
