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

use theseus_protocol::ConfirmRequest;

use super::{approval, heldquestion, split_text, CardText, Route, PART_LIMIT};

/// The key prefix of a card's earlier messages: its last carries the buttons
/// under the card's own key, and its settle leaves these as they are, so the
/// full card stays above the line that closes it.
pub const PART_KEY: &str = "confirmpart:";

/// Whether the daemon worded this question whole (a settings question).
pub fn is_settings(req: &ConfirmRequest) -> bool {
    req.plain.as_ref().is_some_and(|p| !p.detail.is_empty())
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
    let text = format!(
        "{}{}\n-# {asked_for}{}{also}",
        p.detail,
        approval::proof_clause(req),
        heldquestion::footer_clause(req)
    );
    let mut parts = split_text(&text, PART_LIMIT);
    let content = parts.pop().unwrap_or_default();
    let task = req
        .task
        .as_ref()
        .map(|t| format!("task `{}`: ", t.short))
        .unwrap_or_default();
    CardText {
        content,
        more: parts,
        line: format!("{task}{}", p.what),
        budget: false,
    }
}

/// The most characters one `/answer` view may show: a V2 message holds 4,000 in
/// all, and its buttons and footer need room.
const HERE: usize = 3_500;

/// The card's words for a view that is one message (`/answer`): the daemon's
/// words and the proof's clause, whole; none when they do not fit, and then the
/// question is not answered there, for a yes is never taken on a part of it.
pub fn words_here(req: &ConfirmRequest) -> Option<String> {
    let p = req.plain.as_ref()?;
    let text = format!("{}{}", p.detail, approval::proof_clause(req));
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
            req.plain.as_ref().map_or("", |p| p.what.as_str())
        ),
    }
}

#[cfg(test)]
mod tests;
