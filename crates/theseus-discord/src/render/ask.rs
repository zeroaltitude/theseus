//! A question with options on Discord (theseus-j7cje): the words of its
//! settle and of its needs-you post. A child module of `render` (which is at
//! its ceiling); `discord-board` dresses the card later.

use theseus_core::outbox::Closed;

use super::clip;

/// How a card reads once its `owner_ask` closed: the owner's choice, or the
/// clock's.
pub fn settled(closed: &Closed, line: &str) -> String {
    let by = closed.by.as_deref().unwrap_or("the owner");
    let note = closed.note.as_deref().unwrap_or("");
    match closed.how.as_str() {
        "asked_expired" => format!(
            "⌛ **Expired**: nobody answered within {note}; its default applies, or the model \
             decides · {line}"
        ),
        _ => format!("✅ **Answered** by {by}: {} · {line}", clip(note, 200)),
    }
}

/// The needs-you post's text: who needs you, what, the options numbered, and
/// when it expires (a Discord timestamp the client draws).
pub fn needs_you(body: &serde_json::Value) -> String {
    let who = body["asked_by"].as_str().unwrap_or("A task");
    let prompt = body["prompt"].as_str().unwrap_or("");
    let mut t = format!("🔔 **{who} needs you**: {}", clip(prompt, 500));
    for (i, c) in body["choices"].as_array().into_iter().flatten().enumerate() {
        t.push_str(&format!("\n{}. {}", i + 1, c.as_str().unwrap_or("")));
    }
    if let Some(e) = body["expires_at_ms"].as_u64().filter(|e| *e > 0) {
        t.push_str(&format!("\n-# expires <t:{}:R>", e / 1000));
    }
    t
}

/// The card of an `owner_ask` question: the question, its options numbered
/// (`Buttons::Ask` draws one button each), and when it expires and what it
/// expires to. `task` is the line's task prefix.
pub fn card(
    req: &theseus_protocol::ConfirmRequest,
    task: &str,
    route: &super::Route,
    elsewhere: &str,
) -> super::CardText {
    let ask = theseus_protocol::work::ask::AskInput::parse(&req.input).ok();
    let question = ask.as_ref().map_or("a question", |a| a.question.as_str());
    let mut content = format!("❓ **{task}{}**", clip(question, 500));
    for (i, o) in ask.iter().flat_map(|a| a.options.iter()).enumerate() {
        content.push_str(&format!("\n**{}.** {}", i + 1, o.label));
        if let Some(d) = o.detail.as_deref().filter(|d| !d.trim().is_empty()) {
            content.push_str(&format!(" — {}", clip(d, 200)));
        }
    }
    let asked_for = match route {
        super::Route::Dm { place, .. } => format!("for {place} · "),
        _ => String::new(),
    };
    let also = match elsewhere {
        "" => String::new(),
        e => format!(" · you can also answer {e}"),
    };
    let fallback = ask
        .as_ref()
        .map_or("no default".to_string(), |a| a.on_expiry());
    content.push_str(&format!(
        "\n-# {asked_for}expires <t:{}:R> ({fallback}){also}",
        req.expires_at_ms / 1000
    ));
    super::CardText {
        content,
        line: format!("{task}asks: {}", clip(question, 80)),
        budget: false,
    }
}
