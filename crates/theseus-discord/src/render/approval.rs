//! The approval card in the plain words (theseus-ono8g): who wants to do what to
//! which target, the risk, why it asks, and what each button does, from the
//! request's `plain` words, which the TUI and `theseus answer` print too. A request
//! with no `plain` (an older daemon) keeps the card `render::card` always drew.

use theseus_protocol::{plain::PlainWords, ConfirmRequest};

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
        false => rule_clause(&req.reason),
    }
}

/// A hold that is not the floor: the owner's own posture asks in the
/// owner-approved words; any other rule says its own clause, which the core reads from the reason
/// (`needs_you::rule_clause`, so every surface says the same), so the card never gives a
/// false reason.
fn rule_clause(reason: &str) -> String {
    const POSTURE: &str = "you set this kind of call to ask first";
    theseus_core::needs_you::rule_clause(reason)
        .map_or_else(|| POSTURE.into(), |c| clip_middle(&c, 200))
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
         **Decline** refuses it; {who} is told and carries on\n-# {place}expires <t:{}:R>{or}",
        why_it_asks(req, p),
        preview::hunk(req),
        req.expires_at_ms / 1000
    );
    CardText {
        content,
        line: if req.floor {
            format!("{line} (floor)")
        } else {
            line
        },
        budget: false,
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
    let asked_for = match route {
        Route::Dm { place, .. } => format!("for {place} · "),
        _ => String::new(),
    };
    let also = match elsewhere {
        "" => String::new(),
        e => format!(" · you can also answer {e}"),
    };
    content.push_str(&format!(
        "\n-# {asked_for}expires <t:{}:R>{also}",
        req.expires_at_ms / 1000
    ));
    CardText {
        content,
        line: if req.floor {
            format!("{line} (floor)")
        } else {
            line
        },
        budget: false,
    }
}

#[cfg(test)]
mod tests_approval;
