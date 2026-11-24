//! WHO AM I (the owner's shape of a turn's context, 2026-11-24: who am I; who am I talking to; topic; here;
//! everywhere): a few lines laid by code at the head of the first user message, first in the front. They
//! name the profile the turn runs on and its model, the persona in play by its name, and where to read more.
//! Built from the request's own spec and the persona's name alone: nothing of the persona's card or files, no
//! setting's text. The same bytes from one turn to the next until the profile, its model or the persona
//! changes, so they sit in the cached front. The soul text stays in the system part: it is the voice, not a
//! description.

use super::text::tokens;

/// The most bytes of a name (a profile's, a model's, a persona's) the block lays, its "..." included: 80
/// characters of any width.
pub const NAME_BYTES: usize = 320;
/// The block's bound, in tokens by the repository's estimate ([`tokens`]): [`text`] keeps every block under it.
pub const MOST_TOKENS: u64 = 300;
/// How the block begins: the harness's words, never the person's (`task_graph::view::is_head` lists it, so a
/// stand-in model that reads the person's words skips it).
pub const HEAD: &str = "Who you are, as Theseus's settings stand for this turn (laid by code):";
/// The end of a name that was cut.
const MORE: &str = "...";

/// What the block names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Who<'a> {
    /// The profile the turn runs on, by its name in the settings.
    pub profile: &'a str,
    pub model: &'a str,
    /// The persona in play, by its display name.
    pub persona: &'a str,
}

/// The block's text: four lines, under [`MOST_TOKENS`] whatever the names. Each name is cut to [`NAME_BYTES`];
/// while the whole block is still over the bound, every name is cut a quarter shorter.
pub fn text(w: &Who<'_>) -> String {
    let mut most = NAME_BYTES;
    loop {
        let block = lay(w, most);
        if tokens(&block) < MOST_TOKENS || most == 0 {
            return block;
        }
        most = most * 3 / 4;
    }
}

/// Its size by today's estimate.
pub fn size(w: &Who<'_>) -> u64 {
    tokens(&text(w))
}

/// The four lines, each name cut to `most` bytes.
fn lay(w: &Who<'_>, most: usize) -> String {
    [
        HEAD.to_string(),
        format!(
            "- profile: {}; model: {}",
            cut(w.profile, most),
            cut(w.model, most)
        ),
        format!("- persona: {}", cut(w.persona, most)),
        "- to read more: config_show for the settings, look_theseus for what this turn was shown and why"
            .to_string(),
    ]
    .join("\n")
}

/// `s` on one line, without control characters, at most `most` bytes with its [`MORE`], cut on a character's
/// boundary (a cut never splits a character).
pub(super) fn cut(s: &str, most: usize) -> String {
    let one: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let one = one.trim();
    if one.len() <= most {
        return one.to_string();
    }
    let mut at = most.saturating_sub(MORE.len());
    while !one.is_char_boundary(at) {
        at -= 1;
    }
    format!("{}{MORE}", one[..at].trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn who() -> Who<'static> {
        Who {
            profile: "everyday",
            model: "claude-sonnet-5-5",
            persona: "Kestrel",
        }
    }

    #[test]
    fn who_am_i_names_the_profile_model_and_persona() {
        let t = text(&who());
        assert_eq!(t.lines().count(), 4, "{t}");
        assert!(
            t.contains("- profile: everyday; model: claude-sonnet-5-5"),
            "{t}"
        );
        assert!(t.contains("\n- persona: Kestrel\n"), "its name alone: {t}");
        assert!(t.contains("config_show") && t.contains("look_theseus"));
        assert!(
            crate::task_graph::view::is_head(&t),
            "the harness's own block"
        );
        let control = Who {
            persona: "one\u{7}two\r\nthree",
            ..who()
        };
        assert!(text(&control).contains("- persona: one two  three\n"));
    }

    /// 80 four-byte characters in each name (a valid name of 80 characters, 320 bytes): the whole block, its
    /// fixed words and any "..." included, is under the bound by the repository's own estimate; so is a block
    /// of names far longer, of any width.
    #[test]
    fn who_am_i_is_under_three_hundred_tokens() {
        let wide = "\u{1F30A}".repeat(80);
        assert_eq!(wide.len(), 320);
        let w = Who {
            profile: &wide,
            model: &wide,
            persona: &wide,
        };
        let t = text(&w);
        assert!(tokens(&t) < MOST_TOKENS, "{} tokens: {t}", tokens(&t));
        assert!(
            t.starts_with(HEAD) && t.contains("- persona: \u{1F30A}"),
            "{t}"
        );
        for long in [
            "x".repeat(10_000),
            "\u{e9}".repeat(10_000),
            "\u{1F30A}".repeat(10_000),
        ] {
            let w = Who {
                profile: &long,
                model: &long,
                persona: &long,
            };
            assert!(size(&w) < MOST_TOKENS, "{} tokens", size(&w));
        }
        assert!(size(&who()) < 100);
    }

    /// A cut that falls inside a character of two, three or four bytes steps back to the character's start:
    /// the name laid is the longest whole-character beginning of the name that fits with its "...".
    #[test]
    fn a_name_cut_inside_a_character_keeps_whole_characters() {
        for wide in ["\u{e9}", "\u{20AC}", "\u{1F30A}"] {
            for lead in 0..4 {
                let name = format!("{}{}", "x".repeat(lead), wide.repeat(NAME_BYTES));
                let got = cut(&name, NAME_BYTES);
                assert!(got.len() <= NAME_BYTES, "{} bytes", got.len());
                let kept = got.strip_suffix(MORE).expect("a cut name ends in ...");
                assert!(name.starts_with(kept), "a beginning of the name");
                assert!(
                    kept.len() + wide.len() > NAME_BYTES - MORE.len(),
                    "the longest that fits: {} bytes of {wide:?}",
                    kept.len()
                );
            }
        }
        assert_eq!(cut("Kestrel", NAME_BYTES), "Kestrel", "a short name whole");
    }
}
