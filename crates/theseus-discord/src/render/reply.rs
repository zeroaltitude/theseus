//! A reply's messages and a wake's header (theseus-q4v, DD8), apart from
//! `render.rs` for the shape budget's file ceiling.

use theseus_protocol::TurnSubmitResult;

use super::{cite, clip, footer, split_text, DISCORD_LIMIT, PART_LIMIT};

/// A reply's messages, in order: each loop's text in its final form, under
/// the keys the stream used, and the footer riding on the last part when it
/// fits, else standing alone (theseus-q4v). The sources the reply cites
/// (theseus-ejdk) are numbered marks in its text, with small-text footnotes
/// under the last text part (`cite::footnotes`).
pub fn reply_parts(
    turn_id: &str,
    texts: &[(u32, String)],
    result: Option<&TurnSubmitResult>,
) -> Vec<(String, String)> {
    let cites = result.map_or(&[][..], |r| r.cites.as_slice());
    let (texts, order) = cite::marked(texts, cites);
    let mut out: Vec<(String, String)> = Vec::new();
    for (li, text) in &texts {
        for (i, part) in split_text(text, PART_LIMIT).into_iter().enumerate() {
            out.push((format!("{turn_id}:L{li}:p{i}"), part));
        }
    }
    if let Some(notes) = cite::footnotes(&order) {
        match out.last_mut() {
            Some((_, c)) if c.len() + notes.len() + 1 < PART_LIMIT => {
                c.push('\n');
                c.push_str(&notes);
            }
            _ => out.push((format!("{turn_id}:cites"), notes)),
        }
    }
    if let Some(f) = result.map(footer) {
        match out.last_mut() {
            Some((_, c)) if c.len() + f.len() < DISCORD_LIMIT => {
                c.push('\n');
                c.push_str(&f);
            }
            _ => out.push((format!("{turn_id}:footer"), f)),
        }
    }
    out
}

/// A thread's reply in the owner's DM opens with the thread's title as small
/// text (`-# → ex-ai pr`), so the owner sees which line of work spoke and can
/// answer it by replying (theseus-sjzm). It rides on the first part when it
/// fits.
pub fn thread_tag(parts: &mut [(String, String)], title: &str) {
    let title: String = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let head = format!("-# → {}", clip(&title, 80));
    if let Some((_, first)) = parts.first_mut() {
        if first.len() + head.len() < DISCORD_LIMIT {
            *first = format!("{head}\n{first}");
        }
    }
}

/// A wake's turn (DD8): its reply opens with each wake's line, so the place
/// sees why Theseus spoke unasked, and so does a turn a task's report started
/// (W1: `📋 task a1b2c3 reported`). The lines ride on the first part when
/// they fit; the session's history has them either way.
pub fn wake_header(parts: &mut [(String, String)], wakes: &[String]) {
    if wakes.is_empty() {
        return;
    }
    let head = wakes
        .iter()
        .map(|w| format!("-# {}", clip(w, 300)))
        .collect::<Vec<_>>()
        .join("\n");
    if let Some((_, first)) = parts.first_mut() {
        if first.len() + head.len() < DISCORD_LIMIT {
            *first = format!("{head}\n{first}");
        }
    }
}
