//! The approval card in the plain words (theseus-ono8g): who wants to do what to
//! which target, the risk, why it asks, and what each button does, from the
//! request's `plain` words, which the TUI and `theseus answer` print too. A request
//! with no `plain` (an older daemon) keeps the card `render::card` always drew.

use theseus_protocol::{owner_proof::ProofStanding, plain::PlainWords, ConfirmRequest};

use super::{clip_middle, preview, summarize, CardText, Route, ASK, FLOOR_ASK};

/// How a plain approval card opens (a floor call's lock replaces the hand).
pub const OPEN: &str = "✋ **Approve?** ";
const FLOOR_OPEN: &str = "🔒 **Approve?** ";

/// Strips either opening, and the old ones, from a card's first line.
pub fn strip_ask(line: &str) -> &str {
    [
        OPEN,
        FLOOR_OPEN,
        super::FLOOR_ASK,
        super::BUDGET_ASK,
        super::ASK,
    ]
    .iter()
    .find_map(|p| line.strip_prefix(p))
    .unwrap_or(line)
}

/// What the file write adds to its target: whether the file is new, and how big.
fn detail(req: &ConfirmRequest) -> String {
    let bytes = req
        .input
        .get("content")
        .and_then(|c| c.as_str())
        .map(str::len);
    match (req.tool.as_str(), bytes) {
        ("fs.write", Some(n)) if req.reason.starts_with("create ") => {
            format!(" (new file, {n} bytes)")
        }
        ("fs.write", Some(n)) => format!(" (replaces the file, {n} bytes)"),
        _ => String::new(),
    }
}

/// Why the call waits, in a sentence with no rule number.
fn why_it_asks(req: &ConfirmRequest, p: &PlainWords) -> String {
    match req.floor {
        true => p.held.clone(),
        false => rule_clause(req),
    }
}

/// A hold that is not the floor: the owner's own posture asks in the
/// owner-approved words; any other rule says its own clause, which the core reads from the reason
/// (`needs_you::rule_clause`, so every surface says the same), so the card never gives a
/// false reason.
fn rule_clause(req: &ConfirmRequest) -> String {
    const POSTURE: &str = "you set this kind of call to ask first";
    theseus_core::needs_you::card_clause(req)
        .map_or_else(|| POSTURE.into(), |c| clip_middle(&c, 200))
}

/// What the card's third answer does, when it offers one that clears a hold (issue 54).
fn clear_clause(req: &ConfirmRequest) -> String {
    match &req.grant_offer {
        Some(o) if o.kind == theseus_protocol::grants::KIND_CLEAR => format!(
            "**{}** runs it and {} · ",
            o.button(),
            match o.pattern.as_str() {
                theseus_protocol::grants::CLEAR_FLAG => {
                    "clears the flag (what it flagged stays here)"
                }
                _ => "stops asking until this conversation reads outside text again",
            }
        ),
        _ => String::new(),
    }
}

/// The card for a request that carries its plain words.
pub fn card(req: &ConfirmRequest, p: &PlainWords, route: &Route, elsewhere: &str) -> CardText {
    // A verb in bold; an unglossed tool's own summary has none to bold.
    let what = match p.what.split_once(' ') {
        Some((v, rest)) if v.bytes().all(|c| c.is_ascii_lowercase()) => format!("**{v}** {rest}"),
        _ => p.what.clone(),
    };
    let task = req
        .task
        .as_ref()
        .map(|t| format!("task `{}` · ", t.short))
        .unwrap_or_default();
    let line = format!("{task}*{}* wants to {what}{}", p.who, detail(req));
    let risk = match (p.risk.as_str(), req.tool.starts_with("fs.")) {
        ("writes", true) => "writes a file",
        (r, _) => r,
    };
    let open = if req.floor { FLOOR_OPEN } else { OPEN };
    let who = if req.task.is_some() {
        "the task"
    } else {
        "the session"
    };
    let place = match route {
        Route::Dm { place, .. } => format!("for {place} · "),
        _ => String::new(),
    };
    let or = match (elsewhere, &req.task) {
        ("", _) => String::new(),
        (_, Some(t)) => format!(" · or `theseus answer {} approve`", t.short),
        (e, None) => format!(" · you can also answer {e}"),
    };
    let content = format!(
        "{open}{line}\nrisk: {risk} · why it asks: {}{}\n**Approve** runs this one call · \
         {}**Decline** refuses it; {who} is told and carries on{}\n-# {place}{}{or}",
        why_it_asks(req, p),
        preview::hunk(req),
        clear_clause(req),
        proof_clause(req),
        super::heldquestion::footer_clause(req)
    );
    CardText {
        content,
        line: if req.floor {
            format!("{line} (floor)")
        } else {
            line
        },
        budget: false,
        more: Vec::new(),
    }
}

/// What a question that needs the owner's second proof adds to its card
/// (issue 22), said before the press: whether Approve goes through on a proof
/// given here earlier (with how long it is still good for, in the daemon's own
/// words, whole) or opens a form for the code from the authenticator app.
/// Nothing for a question that asks for none. The words are fixed when the card
/// is drawn; the press is decided by the daemon at press time.
pub(super) fn proof_clause(req: &ConfirmRequest) -> String {
    proof_clause_of(req.needs_proof, req.needs_fresh_proof, req.proof.as_ref())
}

/// [`proof_clause`], from the facts it reads. `standing` is the daemon's
/// words for the place the card is answered in, when they are known.
pub(super) fn proof_clause_of(
    needs_proof: bool,
    needs_fresh_proof: bool,
    standing: Option<&ProofStanding>,
) -> String {
    match (needs_proof, needs_fresh_proof, standing) {
        (false, _, _) => String::new(),
        (true, true, _) => {
            "\n🔐 **Second proof, every time:** **Approve** opens a form for the code \
                           from your authenticator app"
                .into()
        }
        (true, false, Some(p)) if !p.code_needed => {
            format!("\n🔐 {}; **Approve** goes through", p.line)
        }
        (true, false, Some(p)) if p.not_enrolled => format!("\n🔐 {}", p.line),
        (true, false, Some(_)) => {
            "\n🔐 **Second proof:** none given here earlier covers this: \
                                  **Approve** opens a form for the code from your authenticator app"
                .into()
        }
        (true, false, None) => {
            "\n🔐 **Second proof:** **Approve** goes through if a proof you gave \
                               here earlier still covers it, and otherwise opens a form for the \
                               code from your authenticator app"
                .into()
        }
    }
}

/// The card as it was before the plain words, for a request without them (an
/// older daemon): the tool's name and its summary, the rule's reason.
pub(super) fn classic(
    req: &ConfirmRequest,
    task: &str,
    route: &Route,
    elsewhere: &str,
) -> CardText {
    let line = format!("{task}`{}` {}", req.tool, summarize(&req.tool, &req.input));
    let mut content = format!("{}{line}", if req.floor { FLOOR_ASK } else { ASK });
    if !req.reason.is_empty() {
        content.push_str(&format!("\n{}", clip_middle(&req.reason, 300)));
    }
    content.push_str(&preview::hunk(req));
    content.push_str(&proof_clause(req));
    let asked_for = match route {
        Route::Dm { place, .. } => format!("for {place} · "),
        _ => String::new(),
    };
    let also = match elsewhere {
        "" => String::new(),
        e => format!(" · you can also answer {e}"),
    };
    content.push_str(&format!(
        "\n-# {asked_for}{}{also}",
        super::heldquestion::footer_clause(req)
    ));
    CardText {
        content,
        line: if req.floor {
            format!("{line} (floor)")
        } else {
            line
        },
        budget: false,
        more: Vec::new(),
    }
}

#[cfg(test)]
mod tests_approval;
