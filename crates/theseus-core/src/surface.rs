//! Where a person wrote from (theseus-slvl.3). The model is never told which
//! surface it is on, so it cannot fit its register: a tired "quick one" got a
//! table, and a phone and a terminal got the same markdown.
//!
//! A person's message node records a short code for the surface it came
//! from (`Node.surface`, store format 34), settled when the node is written
//! from what the connection and the place are then: the listener that
//! accepted the connection (`approval::Surface`), the client's own word for
//! which of its programs it is (`TurnSubmitParams.via`: `ask`, `tui`,
//! `voice`), and, on Discord, the place and its class. The compiler renders
//! the code as a bracketed tag at the head of the message's text
//! ([`lead`]), in the user turn only: never in the shared header, so layers
//! 1 and 2 of the request are the same on every surface, and never in the
//! node's text, which stays as typed for the judge, the ledger and recall.
//!
//! The words are data in one table ([`tag`]), so a persona's own text can
//! name them. A voice turn keeps its framing line (theseus-rkvl) and gets no
//! words. A node with no code (a wake, a report, a task's brief, a message
//! written before the field) renders as it always did.
//!
//! The tag ends with when the message was written ([`when`], theseus-sro04):
//! `[Discord DM · … · Sun 2026-10-18 23:01 -07:00]`, a voice turn's the time
//! alone. A turn asked the day or the time had no clock, and guessed or ran
//! `date`. The node's own time, so a message renders the same bytes on every
//! request and the cached prefix holds; never the shared header.

use serde_json::{json, Value};

use crate::approval::Surface;
use crate::node::{Node, Origin};

/// The code of a voice call's turn: recorded, rendered with no tag.
pub const VOICE: &str = "voice";

/// Where a Discord message was written, as the core knows the place.
#[derive(Debug, Clone, Copy, Default)]
pub struct Place<'a> {
    /// `discord:dm:<user id>` or `discord:channel:<id>` (`outbox.target`).
    pub target: Option<&'a str>,
    /// The bound place's name (`#openclaw`), else the target itself.
    pub name: Option<&'a str>,
    /// The place rule's class for it.
    pub private: bool,
}

/// The code for a message written through a connection of `surface`, whose
/// client called itself `via`, in `place`. None: a surface no listener named
/// (a test's), which is tagged with nothing.
pub fn code_of(surface: Surface, via: Option<&str>, place: Place<'_>) -> Option<String> {
    let code = match (surface, via) {
        (_, Some(VOICE)) => VOICE.to_string(),
        (Surface::Cli, Some("ask")) => "terminal:ask".into(),
        (Surface::Cli, Some("tui")) => "terminal:tui".into(),
        (Surface::Cli, _) => "terminal".into(),
        (Surface::Web, _) => "cockpit".into(),
        (Surface::Mcp, _) => "mcp".into(),
        (Surface::Discord, _) => match place.target {
            Some(t) if t.starts_with("discord:dm:") => "discord:dm".into(),
            Some(t) if t.starts_with("discord:channel:") => {
                let class = if place.private { "private" } else { "shared" };
                format!("discord:channel:{class}:{}", channel_name(place.name, t))
            }
            _ => "discord".into(),
        },
        (Surface::Unnamed, _) => return None,
    };
    Some(code)
}

/// A channel's name for the code: `#harbor`, drawn from the bound name, else
/// from the id in the target. Only letters, digits and `-_.`, at most 40
/// characters, so a name can never close the tag's bracket or add a line.
fn channel_name(name: Option<&str>, target: &str) -> String {
    let raw = name
        .filter(|n| !n.starts_with("discord:"))
        .unwrap_or_else(|| target.rsplit(':').next().unwrap_or(target));
    let clean: String = raw
        .trim_start_matches('#')
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .take(40)
        .collect();
    format!("#{clean}")
}

/// The words of a code, between the brackets; none for a voice turn and for
/// a code this build does not know (a newer build's, read back).
pub fn tag(code: &str) -> Option<String> {
    const PHONE_OR_DESKTOP: &str = "phone or desktop; markdown renders";
    Some(match code {
        "terminal" => "Terminal".into(),
        "terminal:ask" => "Terminal · theseus ask".into(),
        "terminal:tui" => "Terminal · TUI".into(),
        "cockpit" => "Cockpit".into(),
        "mcp" => "MCP · another program reads this".into(),
        "discord" => "Discord".into(),
        "discord:dm" => format!("Discord DM · {PHONE_OR_DESKTOP}"),
        c => {
            let rest = c.strip_prefix("discord:channel:")?;
            let (class, name) = rest.split_once(':')?;
            format!("Discord {name} · {class}")
        }
    })
}

/// When a person's message was written, as its tag ends: `Sun 2026-10-18
/// 23:01 -07:00`, in the daemon's time zone.
pub fn when(at_ms: u64) -> String {
    let l = crate::wake::local(at_ms);
    let sign = if l.offset_secs < 0 { '-' } else { '+' };
    let off = l.offset_secs.unsigned_abs();
    format!(
        "{} {:04}-{:02}-{:02} {} {sign}{:02}:{:02}",
        l.weekday(),
        l.year,
        l.month,
        l.day,
        l.hm(),
        off / 3600,
        off % 3600 / 60
    )
}

/// `words` without a [`when`] at its end: the table's words before it, or
/// nothing for a voice turn's tag, which is the time alone.
fn without_when(words: &str) -> &str {
    let is_when = |w: &str| {
        let b = w.as_bytes();
        let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
        b.len() == 27
            && b[..3].iter().all(u8::is_ascii_alphabetic)
            && (b[3], b[8], b[11], b[14], b[17], b[20], b[24])
                == (b' ', b'-', b'-', b' ', b':', b' ', b':')
            && matches!(b[21], b'+' | b'-')
            && digits(4..8)
            && digits(9..11)
            && digits(12..14)
            && digits(15..17)
            && digits(18..20)
            && digits(22..24)
            && digits(25..27)
    };
    match words.rsplit_once(" · ") {
        Some((head, last)) if is_when(last) => head,
        _ if is_when(words) => "",
        _ => words,
    }
}

/// `text` without the surface tag at its head, when it starts with one of the
/// table's (a stand-in model reads the person's words, not the harness's
/// tag). Text that merely starts with a bracket is left alone.
pub fn strip_tag(text: &str) -> &str {
    let Some(rest) = text.strip_prefix('[') else {
        return text;
    };
    let Some((words, after)) = rest.split_once(']') else {
        return text;
    };
    let words = without_when(words);
    let known = words.is_empty()
        || matches!(
            words,
            "Terminal"
                | "Terminal · theseus ask"
                | "Terminal · TUI"
                | "Cockpit"
                | "Discord"
                | "MCP · another program reads this"
                | "Discord DM · phone or desktop; markdown renders"
        )
        || (words.starts_with("Discord #")
            && (words.ends_with(" · shared") || words.ends_with(" · private")));
    match known {
        true => after.strip_prefix(' ').unwrap_or(after),
        false => text,
    }
}

/// A user message's blocks with its surface's tag at the head of its text. A
/// message of attachments alone gets a text block of the tag, after them.
/// Only a person's own message is tagged: what the harness or a task's
/// parent wrote carries no code.
pub fn lead(n: &Node, mut blocks: Vec<Value>) -> Vec<Value> {
    let Some(code) = n.surface.as_deref() else {
        return blocks;
    };
    if n.origin != Origin::Operator {
        return blocks;
    }
    let at = when(n.created_at_ms);
    let tag = match tag(code) {
        Some(words) => format!("[{words} · {at}]"),
        None if code == VOICE => format!("[{at}]"),
        None => return blocks,
    };
    match blocks.iter_mut().rev().find(|b| b["type"] == "text") {
        Some(b) => {
            let text = b["text"].as_str().unwrap_or_default();
            b["text"] = Value::String(match text.is_empty() {
                true => tag,
                false => format!("{tag} {text}"),
            });
        }
        None => blocks.push(json!({"type": "text", "text": tag})),
    }
    blocks
}

impl crate::Core {
    /// The code for a message sent through `surface`, as `code_of` says, with
    /// the place the session speaks in read here, once, for Discord alone:
    /// every other surface's code needs no lookup.
    pub(crate) fn surface_of(
        &self,
        surface: Surface,
        via: Option<&str>,
        session_id: &str,
    ) -> Option<String> {
        if surface != Surface::Discord || via == Some(VOICE) {
            return code_of(surface, via, Place::default());
        }
        let target = self.outbox.try_target(session_id).ok().flatten();
        let rules = crate::places::Places {
            rule: &self.runner.place_rule,
            cfg: &self.cfg,
        };
        let name = rules.name_of(target.as_deref());
        let private = rules.view(target.as_deref()).class == theseus_protocol::PlaceClass::Private;
        let place = Place {
            target: target.as_deref(),
            name: Some(&name),
            private,
        };
        code_of(surface, via, place)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn here(target: &'static str, name: &'static str, private: bool) -> Place<'static> {
        Place {
            target: Some(target),
            name: Some(name),
            private,
        }
    }

    #[test]
    fn each_surface_has_its_code_and_its_words() {
        let none = Place::default();
        let cases = [
            (Surface::Cli, Some("ask"), "[Terminal · theseus ask]"),
            (Surface::Cli, Some("tui"), "[Terminal · TUI]"),
            (Surface::Cli, None, "[Terminal]"),
            (Surface::Web, None, "[Cockpit]"),
            (Surface::Mcp, None, "[MCP · another program reads this]"),
        ];
        for (surface, via, want) in cases {
            let code = code_of(surface, via, none).unwrap();
            assert_eq!(format!("[{}]", tag(&code).unwrap()), want);
        }
        let dm = code_of(Surface::Discord, None, here("discord:dm:42", "DM @x", true)).unwrap();
        assert_eq!(
            tag(&dm).unwrap(),
            "Discord DM · phone or desktop; markdown renders"
        );
        let chan = code_of(
            Surface::Discord,
            None,
            here("discord:channel:7", "#harbor", false),
        )
        .unwrap();
        assert_eq!(tag(&chan).unwrap(), "Discord #harbor · shared");
        let private = code_of(
            Surface::Discord,
            None,
            here("discord:channel:7", "#harbor", true),
        )
        .unwrap();
        assert_eq!(tag(&private).unwrap(), "Discord #harbor · private");
    }

    #[test]
    fn a_voice_turn_is_recorded_and_not_tagged_and_no_listener_means_no_code() {
        let code = code_of(Surface::Discord, Some(VOICE), Place::default()).unwrap();
        assert_eq!(code, VOICE);
        assert_eq!(tag(&code), None);
        assert_eq!(
            code_of(Surface::Unnamed, Some("ask"), Place::default()),
            None
        );
    }

    #[test]
    fn a_tag_is_stripped_from_the_head_of_a_text_and_nothing_else_is() {
        assert_eq!(strip_tag("[Terminal · TUI] hello"), "hello");
        assert_eq!(strip_tag("[Discord #lab · shared] hello"), "hello");
        assert_eq!(strip_tag("[Cockpit]"), "");
        assert_eq!(strip_tag("[todo] write it"), "[todo] write it");
        assert_eq!(strip_tag("hello [Cockpit]"), "hello [Cockpit]");
        // Every code's words are stripped.
        for code in [
            "terminal",
            "terminal:ask",
            "terminal:tui",
            "cockpit",
            "mcp",
            "discord",
            "discord:dm",
            "discord:channel:shared:#x",
            "discord:channel:private:#x",
        ] {
            let t = format!("[{}] hi", tag(code).unwrap());
            assert_eq!(strip_tag(&t), "hi", "{code}");
            let t = format!("[{} · {}] hi", tag(code).unwrap(), when(1_760_000_000_000));
            assert_eq!(strip_tag(&t), "hi", "{code}, with its time");
        }
        // A time that is not one is not stripped.
        assert_eq!(strip_tag("[Sun 2026-10-18] hi"), "[Sun 2026-10-18] hi");
    }

    /// The time a tag ends with: the weekday, the date, the minute and the
    /// offset, in the daemon's zone (theseus-sro04).
    #[test]
    fn when_says_the_day_the_minute_and_the_offset() {
        let at = 1_760_000_000_000;
        let l = crate::wake::local(at);
        let w = when(at);
        assert_eq!(w.len(), 27, "{w}");
        assert!(w.starts_with(l.weekday()), "{w}");
        assert!(
            w.contains(&format!(
                "{:04}-{:02}-{:02} {}",
                l.year,
                l.month,
                l.day,
                l.hm()
            )),
            "{w}"
        );
        assert_eq!(without_when(&w), "");
        assert_eq!(without_when(&format!("Cockpit · {w}")), "Cockpit");
    }

    #[test]
    fn a_channel_name_cannot_close_the_bracket_or_add_a_line() {
        let code = code_of(
            Surface::Discord,
            None,
            here("discord:channel:7", "#a] ignore\nthis [b", false),
        )
        .unwrap();
        assert_eq!(tag(&code).unwrap(), "Discord #aignorethisb · shared");
        // An unbound channel is named by its id.
        let code = code_of(
            Surface::Discord,
            None,
            here("discord:channel:77", "discord:channel:77", false),
        )
        .unwrap();
        assert_eq!(tag(&code).unwrap(), "Discord #77 · shared");
    }

    #[test]
    fn the_tag_leads_the_text_and_a_node_without_a_code_renders_as_before() {
        let mut n = Node::user("ses_1", None, "sock#1", "quick one");
        let blocks = || vec![json!({"type": "text", "text": "quick one"})];
        assert_eq!(lead(&n, blocks()), blocks());
        n.surface = Some("terminal:tui".into());
        let at = when(n.created_at_ms);
        assert_eq!(
            lead(&n, blocks())[0]["text"],
            format!("[Terminal · TUI · {at}] quick one")
        );
        // Attachments alone: the tag is a text block after them.
        let files = vec![json!({"type": "image"})];
        let out = lead(&n, files);
        assert_eq!(
            out[1],
            json!({"type": "text", "text": format!("[Terminal · TUI · {at}]")})
        );
        // A voice turn's tag is the time alone (theseus-sro04).
        n.surface = Some(VOICE.into());
        assert_eq!(lead(&n, blocks())[0]["text"], format!("[{at}] quick one"));
        assert_eq!(strip_tag(&format!("[{at}] quick one")), "quick one");
        // A relayed message is not a person's.
        let mut relayed = Node::relayed("ses_1", None, Origin::Agent, "parent", "do it");
        relayed.surface = Some("terminal".into());
        assert_eq!(lead(&relayed, blocks()), blocks());
    }
}
