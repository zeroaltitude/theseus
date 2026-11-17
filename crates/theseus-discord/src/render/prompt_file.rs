//! The card of a change to a prompt file, or to a setting too long for the
//! ordinary card (issue 23), as Discord draws it.
//!
//! The daemon builds the card's text to fit this message and never to be cut
//! (`prompt_file::card::CARD_CHARS`): the owner says yes to what is on it. So it
//! is drawn whole, in its own form, with its lines as the daemon wrote them
//! (the changed lines are a fenced block that Discord shows as they are). A
//! card longer than that is not drawn cut: it says where to read it instead.

use theseus_core::config_change::prompt_file::card::CARD_CHARS;
use theseus_protocol::ConfirmRequest;

use super::{approval, heldquestion, CardText, Route};

/// Whether this card is drawn in this form.
pub(super) fn is_card(req: &ConfirmRequest) -> bool {
    theseus_protocol::has_whole_text(req)
}

/// The card: what the daemon built, whole, then the second proof's clause.
pub(super) fn card(req: &ConfirmRequest, task: &str, route: &Route, elsewhere: &str) -> CardText {
    let open = format!("✋ **Approve?** {task}a change to Theseus's own text or settings");
    let text = req.reason.trim_end();
    let body = match text.chars().count() <= CARD_CHARS {
        true => text.to_string(),
        false => "This card is too long to show here, so it is not shown cut. Read it with \
                  `theseus config show`, or in the cockpit, and answer there."
            .to_string(),
    };
    let asked_for = match route {
        Route::Dm { place, .. } => format!("for {place} · "),
        _ => String::new(),
    };
    let also = match elsewhere {
        "" => String::new(),
        e => format!(" · you can also answer {e}"),
    };
    let content = format!(
        "{open}\n{body}{}\n-# {asked_for}{}{also}",
        approval::proof_clause(req),
        heldquestion::footer_clause(req)
    );
    CardText {
        content,
        line: format!("{task}a change to Theseus's own text or settings"),
        budget: false,
    }
}

#[cfg(test)]
mod tests;
