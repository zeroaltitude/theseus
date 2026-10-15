//! Feedback in an agent's memory notes, proposed as standing records for the
//! persona. Found by shape, not by meaning: a note whose frontmatter says
//! `type: feedback` gives its first paragraph; a `## Feedback` heading gives
//! each bullet under it. The kind is `order` when the text is a command
//! (never, always, must, do not, stop) and `preference` otherwise. The owner
//! decides; nothing here is live.

use sha2::{Digest, Sha256};
use theseus_protocol::{ProposedStanding, StandingKind};

/// A record's longest text (`standing::MAX_TEXT`).
const MAX_TEXT: usize = 1000;

/// What a note proposes, and what it holds that is too long to propose.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub items: Vec<(StandingKind, String)>,
    pub too_long: usize,
}

/// The feedback items of one note's text.
pub fn of_note(text: &str) -> Found {
    let mut found = Found::default();
    let (front, body) = frontmatter(text);
    if front
        .iter()
        .any(|(k, v)| k == "type" && v.eq_ignore_ascii_case("feedback"))
    {
        if let Some(p) = first_paragraph(body) {
            push(&mut found, p);
        }
    }
    for b in bullets_under_feedback(body) {
        push(&mut found, b);
    }
    found
}

fn push(found: &mut Found, text: String) {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return;
    }
    if text.chars().count() > MAX_TEXT {
        found.too_long += 1;
        return;
    }
    if found.items.iter().any(|(_, t)| *t == text) {
        return;
    }
    found.items.push((kind_of(&text), text));
}

/// `order` for a command, `preference` otherwise.
pub fn kind_of(text: &str) -> StandingKind {
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .collect();
    let command = ["never", "always", "must", "stop", "don't", "dont"]
        .iter()
        .any(|w| words.contains(w))
        || lower.contains("do not");
    if command {
        StandingKind::Order
    } else {
        StandingKind::Preference
    }
}

/// A proposal's id: stable for the same persona, kind and text.
pub fn id_of(persona: &str, kind: StandingKind, text: &str) -> String {
    let d = Sha256::digest(format!("{persona}\n{}\n{text}", kind.name()).as_bytes());
    format!("stp_{}", &hex::encode(d)[..12])
}

pub fn proposal(persona: &str, from: &str, kind: StandingKind, text: String) -> ProposedStanding {
    ProposedStanding {
        id: id_of(persona, kind, &text),
        kind,
        text,
        from: from.to_string(),
    }
}

/// Frontmatter as (key, value) pairs and the rest of the text.
fn frontmatter(text: &str) -> (Vec<(String, String)>, &str) {
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return (Vec::new(), text);
    };
    let mut at = 0;
    let mut pairs = Vec::new();
    for l in rest.split_inclusive('\n') {
        let t = l.trim_end();
        if t == "---" {
            return (pairs, &rest[at + l.len()..]);
        }
        if let Some((k, v)) = t.split_once(':') {
            pairs.push((
                k.trim().to_lowercase(),
                v.trim().trim_matches('"').to_string(),
            ));
        }
        at += l.len();
    }
    (Vec::new(), text)
}

fn first_paragraph(body: &str) -> Option<String> {
    let mut para = Vec::new();
    for l in body.lines() {
        let t = l.trim();
        if t.is_empty() {
            if para.is_empty() {
                continue;
            }
            break;
        }
        if t.starts_with('#') && para.is_empty() {
            continue;
        }
        para.push(t);
    }
    (!para.is_empty()).then(|| para.join(" "))
}

/// The bullets under a heading named Feedback, until a heading of its level
/// or higher; a bullet's continuation lines (indented) belong to it.
fn bullets_under_feedback(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut level = None::<usize>;
    let mut cur: Option<String> = None;
    let mut fenced = false;
    for l in body.lines() {
        if l.trim_start().starts_with("```") {
            fenced = !fenced;
        }
        let t = l.trim_end();
        let hashes = t.len() - t.trim_start_matches('#').len();
        let is_heading = !fenced && hashes > 0 && t[hashes..].starts_with(' ');
        if is_heading {
            if let Some(b) = cur.take() {
                out.push(b);
            }
            let name = t[hashes..].trim().trim_matches('*').trim();
            level = if name.eq_ignore_ascii_case("feedback") {
                Some(hashes)
            } else if level.is_some_and(|lv| hashes <= lv) {
                None
            } else {
                level
            };
            continue;
        }
        if level.is_none() {
            continue;
        }
        let trimmed = t.trim_start();
        let bullet = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .filter(|_| t.len() - trimmed.len() < 2);
        match bullet {
            Some(b) => {
                if let Some(prev) = cur.take() {
                    out.push(prev);
                }
                cur = Some(b.trim().to_string());
            }
            None if trimmed.is_empty() => {
                if let Some(b) = cur.take() {
                    out.push(b);
                }
            }
            None => {
                if let Some(c) = cur.as_mut() {
                    c.push(' ');
                    c.push_str(trimmed);
                }
            }
        }
    }
    if let Some(b) = cur.take() {
        out.push(b);
    }
    out
}
