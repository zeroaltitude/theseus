//! A reply's citations on Discord (theseus-ejdk): each handle the reply cites
//! (`[m:a1b2c3]`) is a numbered mark in its text, small-text footnotes (`-#`)
//! stand under its last text part, and a row of buttons under the reply opens
//! each source: a link to the original message when the source was a Discord
//! message and its place is one this binding can name, else a "Show source"
//! button whose press answers with an ephemeral excerpt. Components V2 are
//! not used anywhere in this binding, so the closest equivalent it supports
//! is classic components: `-#` text and one action row of buttons.
//!
//! Pure, as the rest of `render`: a shared place's citations arrive already
//! cut to what it may show (the core resolves them), so nothing here decides
//! what is private.

use theseus_protocol::cite::{mark_into, Citation, DiscordSource};

/// The most buttons a reply's row holds (Discord's row holds five).
pub const MAX_SOURCES: usize = 5;

/// How much of a source's header a footnote keeps.
const NOTE_CHARS: usize = 200;

/// One button under a reply: the footnote's number, the handle it opens, and
/// where to send the press: a message's link, or none for "Show source".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub number: usize,
    pub handle: String,
    pub link: Option<String>,
}

/// The reply's texts with each cited handle as `[1]`, `[2]`, the numbers
/// going on across the texts, and the citations in that order. A handle with
/// no citation stays as the model wrote it.
pub fn marked<'a>(
    texts: &[(u32, String)],
    cites: &'a [Citation],
) -> (Vec<(u32, String)>, Vec<&'a Citation>) {
    let mut order = Vec::new();
    let out = texts
        .iter()
        .map(|(l, t)| (*l, mark_into(t, cites, &mut order)))
        .collect();
    (out, order)
}

/// The footnotes under a reply, one `-#` line each: the number and the
/// source's frozen header (whose, where, when). None when nothing is cited.
pub fn footnotes(order: &[&Citation]) -> Option<String> {
    if order.is_empty() {
        return None;
    }
    let lines: Vec<String> = order
        .iter()
        .enumerate()
        .map(|(i, c)| format!("-# [{}] {}", i + 1, super::clip(&c.header, NOTE_CHARS)))
        .collect();
    Some(lines.join("\n"))
}

/// The buttons for `order`: its first `MAX_SOURCES`, each with the link
/// `link` gives it, if any.
pub fn sources(
    order: &[&Citation],
    link: impl Fn(&DiscordSource) -> Option<String>,
) -> Vec<Source> {
    order
        .iter()
        .take(MAX_SOURCES)
        .enumerate()
        .map(|(i, c)| Source {
            number: i + 1,
            handle: c.handle.clone(),
            link: c.discord.as_ref().and_then(&link),
        })
        .collect()
}

/// A message's link: `https://discord.com/channels/<guild or @me>/<channel>/<message>`.
pub fn jump_link(guild: Option<&str>, channel: u64, message: &str) -> String {
    format!(
        "https://discord.com/channels/{}/{channel}/{message}",
        guild.unwrap_or("@me")
    )
}

/// The ephemeral answer to a "Show source" press: the source's header, its
/// excerpt quoted, and the command that opens it whole.
pub fn excerpt_text(c: &Citation) -> String {
    let quoted = c
        .excerpt
        .lines()
        .map(|l| format!("> {l}"))
        .collect::<Vec<_>>()
        .join("\n");
    let text = format!(
        "**Source** · {}\n{quoted}\n-# `theseus history {}` opens it whole",
        c.header, c.handle
    );
    super::clip(&text, 1900)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cite(handle: &str, discord: Option<&str>) -> Citation {
        Citation {
            handle: handle.into(),
            node_id: "msg_1".into(),
            session_id: "ses_1".into(),
            position: 7,
            header: "a message from ada in #harbor, 2026-09-30 14:34 UTC (as of @7)".into(),
            at_unix_ms: 0,
            excerpt: "We chose oak.\nIt is dry.".into(),
            discord: discord.map(|m| DiscordSource {
                target: "discord:channel:100000000000000001".into(),
                message_id: m.into(),
            }),
        }
    }

    /// Marks and footnotes number across the reply's texts, a repeated
    /// handle keeps its number, and an unknown handle stays text.
    #[test]
    fn the_marks_and_notes_are_their_golden_bytes() {
        let cites = [
            cite("m:a1b2c3", Some("200000000000000002")),
            cite("m:d4e5f6", None),
        ];
        let texts = [
            (1, "Oak [m:a1b2c3].".to_string()),
            (
                2,
                "Pine [m:d4e5f6], oak [m:a1b2c3], and [m:ffffff].".to_string(),
            ),
        ];
        let (marked, order) = marked(&texts, &cites);
        assert_eq!(marked[0].1, "Oak [1].");
        assert_eq!(marked[1].1, "Pine [2], oak [1], and [m:ffffff].");
        assert_eq!(
            footnotes(&order).unwrap(),
            "-# [1] a message from ada in #harbor, 2026-09-30 14:34 UTC (as of @7)\n\
             -# [2] a message from ada in #harbor, 2026-09-30 14:34 UTC (as of @7)"
        );
        assert_eq!(footnotes(&[]), None);
    }

    /// A Discord source with a place this binding names is a link; one with
    /// none, or any other source, is a "Show source" button.
    #[test]
    fn a_source_links_when_its_message_can_be_named() {
        let cites = [
            cite("m:a1b2c3", Some("200000000000000002")),
            cite("m:d4e5f6", None),
        ];
        let order: Vec<&Citation> = cites.iter().collect();
        let got = sources(&order, |d| {
            Some(jump_link(
                Some("300000000000000003"),
                100_000_000_000_000_001,
                &d.message_id,
            ))
        });
        assert_eq!(
            got,
            [
                Source {
                    number: 1,
                    handle: "m:a1b2c3".into(),
                    link: Some(
                        "https://discord.com/channels/300000000000000003/100000000000000001/200000000000000002"
                            .into()
                    ),
                },
                Source { number: 2, handle: "m:d4e5f6".into(), link: None },
            ]
        );
        assert_eq!(
            jump_link(None, 5, "9"),
            "https://discord.com/channels/@me/5/9"
        );
    }

    #[test]
    fn the_excerpt_quotes_the_source_and_names_the_command() {
        assert_eq!(
            excerpt_text(&cite("m:a1b2c3", None)),
            "**Source** · a message from ada in #harbor, 2026-09-30 14:34 UTC (as of @7)\n\
             > We chose oak.\n> It is dry.\n\
             -# `theseus history m:a1b2c3` opens it whole"
        );
    }
}
