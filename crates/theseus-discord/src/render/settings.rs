//! A settings question's card, drawn as what it is (issue 22).
//!
//! A change to Theseus's own settings, an undo, an attached or removed server, a
//! restart and an install each carry the daemon's own whole words in their plain
//! words (`PlainWords::detail`, built by `config_change::words`). This form
//! draws those words first and whole: no tool name, no raw arguments, and never
//! cut. A card longer than one message goes as several, the buttons on the
//! last; the daemon's limits are not lowered for the drawing's sake (twelve
//! settings of 400 characters, before and after, are about 10,000 characters,
//! five messages).
//!
//! The line the card settles into is the daemon's own line (`PlainWords::what`),
//! so the owner reads what he approved or declined in words.

use serde_json::Value;
use theseus_protocol::ConfirmRequest;

use super::{approval, ask, heldquestion, split_text, CardText, Route, PART_LIMIT};

/// The key prefix of a card's earlier messages: its last carries the buttons
/// under the card's own key, and its settle leaves these as they are, so the
/// full card stays above the line that closes it.
pub const PART_KEY: &str = "confirmpart:";

/// Whether the daemon worded this question whole (a settings question).
pub fn is_settings(req: &ConfirmRequest) -> bool {
    req.plain.as_ref().is_some_and(|p| !p.detail.is_empty())
}

/// Whether the daemon offers this question no yes (a secret to keep takes a
/// value, which no button carries): its card draws no Approve and no
/// sentence about one, only where the value is typed and a Decline (issue 29).
pub fn offers_no_yes(req: &ConfirmRequest) -> bool {
    !req.options.is_empty()
        && !req
            .options
            .iter()
            .any(|o| theseus_protocol::answer::approves(&o.id))
}

/// Text as Discord draws it, literally: every character that Discord reads as
/// markdown (emphasis, strike, a spoiler, a masked link, a quote, a heading or a
/// list at a line's start, a code span, a timestamp or channel in angle
/// brackets) has a backslash before it. A value the owner approves is then
/// drawn as it is, and not as italics that drop a wildcard's `*`, or a spoiler
/// that hides the lines between two `||`.
pub fn md(s: &str) -> String {
    let lines: Vec<&str> = s.split_inclusive('\n').collect();
    let mut out = String::with_capacity(s.len() + s.len() / 8);
    let mut fence = false;
    for (i, line) in lines.iter().enumerate() {
        let bare = line.trim_end_matches('\n');
        // The daemon's own block of changed lines (a prompt file's, a long
        // value's) is drawn as it is: a code block shows its text literally,
        // and no row of it can be a fence (the daemon makes a summary of any
        // change whose row holds three backticks, `config_change::shown`). An
        // opening line with no closing one is not a block and is escaped.
        if fence {
            fence = bare != FENCE;
            out.push_str(line);
        } else if bare == FENCE_OPEN
            && lines[i + 1..]
                .iter()
                .any(|l| l.trim_end_matches('\n') == FENCE)
        {
            fence = true;
            out.push_str(line);
        } else {
            out.push_str(&escaped(line));
        }
    }
    out
}

/// The line that opens the daemon's block of changed lines, and the one that closes it.
const FENCE_OPEN: &str = "```diff";
const FENCE: &str = "```";

/// One line, every character Discord reads as markdown with a backslash before it.
fn escaped(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + s.len() / 8);
    for c in s.chars() {
        if matches!(
            c,
            '\\' | '*' | '_' | '~' | '|' | '>' | '<' | '#' | '-' | '[' | ']' | '(' | ')' | '`'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `text` as messages, each under the limit with room for its marker: cut at a
/// line, never between a backslash and the character it escapes, and, when
/// there is more than one, each says which of how many it is.
fn messages(text: &str) -> Vec<String> {
    let mut parts = split_text(text, PART_LIMIT - 16);
    for i in 0..parts.len().saturating_sub(1) {
        let trailing = parts[i].chars().rev().take_while(|c| *c == '\\').count();
        if trailing % 2 == 1 {
            parts[i].pop();
            parts[i + 1].insert(0, '\\');
        }
    }
    let n = parts.len();
    if n > 1 {
        for (i, p) in parts.iter_mut().enumerate() {
            *p = format!("-# ({}/{n})\n{p}", i + 1);
        }
    }
    parts
}

/// The card: the daemon's words, the proof's clause, the footer. The last
/// message is `content`; any before it are `more`, in order.
pub fn card(req: &ConfirmRequest, route: &Route, elsewhere: &str) -> CardText {
    let p = req.plain.clone().unwrap_or_default();
    let asked_for = match route {
        Route::Dm { place, .. } => format!("for {place} · "),
        _ => String::new(),
    };
    let also = match elsewhere {
        "" => String::new(),
        e => format!(" · you can also answer {e}"),
    };
    let proof = match offers_no_yes(req) {
        true => String::new(),
        false => approval::proof_clause(req),
    };
    let text = format!(
        "{}{proof}\n-# {asked_for}{}{also}",
        md(&p.detail),
        heldquestion::footer_clause(req)
    );
    let mut parts = messages(&text);
    let content = parts.pop().unwrap_or_default();
    let task = req
        .task
        .as_ref()
        .map(|t| format!("task `{}`: ", t.short))
        .unwrap_or_default();
    CardText {
        content,
        more: parts,
        line: format!("{task}{}", md(&p.what)),
        budget: false,
    }
}

/// A needs-you post for a settings question (the core's card body carries its
/// words under `settings`): the daemon's words whole and first, the proof's
/// clause, when it expires or since when it is held, and how else to answer;
/// as several messages when longer than one. None for a post that has none.
pub fn needs_you(body: &Value) -> Option<(Vec<String>, String)> {
    let s = body["settings"].as_object().filter(|s| !s.is_empty())?;
    let detail = s.get("detail")?.as_str().filter(|d| !d.is_empty())?;
    let short = s.get("short").and_then(Value::as_str).unwrap_or("");
    let proof = approval::proof_clause_of(
        s.get("needs_proof") == Some(&Value::Bool(true)),
        s.get("needs_fresh_proof") == Some(&Value::Bool(true)),
        None,
    );
    let how = match s.get("no_yes") == Some(&Value::Bool(true)) {
        true if s.get("form") == Some(&Value::Bool(true)) => {
            "tap Type the value here, or Decline, or type the value in your terminal with `theseus vault keep`".to_string()
        }
        true => {
            "tap Decline, or type the value in your terminal with `theseus vault keep`".to_string()
        }
        false => format!("tap Approve or Decline, or `theseus answer {short} approve`"),
    };
    let proof = match s.get("no_yes") == Some(&Value::Bool(true)) {
        true => String::new(),
        false => proof,
    };
    let text = format!(
        "{}{proof}\n-# Needs you{} · {how}",
        md(detail),
        ask::when_clause(body)
    );
    let mut parts = messages(&text);
    let last = parts.pop().unwrap_or_default();
    Some((parts, last))
}

/// The most characters one `/answer` view may show: a V2 message holds 4,000 in
/// all, and its buttons and footer need room.
const HERE: usize = 3_500;

/// The card's words for a view that is one message (`/answer`): the daemon's
/// words and the proof's clause, whole; none when they do not fit, and then the
/// question is not answered there, for a yes is never taken on a part of it.
pub fn words_here(req: &ConfirmRequest) -> Option<String> {
    let p = req.plain.as_ref()?;
    let text = format!("{}{}", md(&p.detail), approval::proof_clause(req));
    (text.chars().count() <= HERE).then_some(text)
}

/// What the Details press shows for a settings question: the daemon's words
/// whole and its id, or where to read them when they do not fit one message.
pub fn details(req: &ConfirmRequest) -> String {
    let id = format!("\n**Id:** `{}`", req.correlation_id);
    match words_here(req).filter(|w| w.chars().count() + id.chars().count() < PART_LIMIT) {
        Some(whole) => format!("**Details**\n{whole}{id}"),
        None => format!(
            "**Details**\n{}\nThe whole question is longer than one message: read it on its own \
             message, or with `theseus answer`.{id}",
            req.plain.as_ref().map_or_else(String::new, |p| md(&p.what))
        ),
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_file;
