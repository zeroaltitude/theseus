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
    if body["card"].is_object() {
        return card_text(body);
    }
    let who = body["asked_by"].as_str().unwrap_or("A task");
    let prompt = body["prompt"].as_str().unwrap_or("");
    let mut t = format!("🔔 **Needs you** · {who}\n**{}**", clip(prompt, 500));
    for (i, c) in body["choices"].as_array().into_iter().flatten().enumerate() {
        t.push_str(&format!("\n{}. {}", i + 1, c.as_str().unwrap_or("")));
    }
    if let Some(e) = body["expires_at_ms"].as_u64().filter(|e| *e > 0) {
        t.push_str(&format!("\n-# expires <t:{}:R>", e / 1000));
    }
    t
}

/// A needs-you card in plain words (theseus-suvpe): what it wants in bold (a
/// batch's card leads with its number), a question's options numbered, why it
/// asks, and what yes does; then when it expires and how to answer it.
fn card_text(body: &serde_json::Value) -> String {
    let c = &body["card"];
    let said = |k: &str| c[k].as_str().unwrap_or("");
    let lead = match (c["n"].as_u64(), c["of"].as_u64()) {
        (Some(n), Some(of)) => format!("**{n} of {of}** · "),
        _ => String::new(),
    };
    let mut t = format!("🔔 {lead}**{}**", clip(said("what"), 400));
    for (i, ch) in body["choices"].as_array().into_iter().flatten().enumerate() {
        t.push_str(&format!("\n{}. {}", i + 1, ch.as_str().unwrap_or("")));
    }
    t.push_str(&format!("\n{}", clip(said("why"), 300)));
    if body["choices"].as_array().is_none_or(Vec::is_empty) {
        t.push_str(&format!("\n{}", clip(said("yes"), 200)));
    }
    t.push_str("\n-# Needs you");
    if let Some(e) = body["expires_at_ms"].as_u64().filter(|e| *e > 0) {
        t.push_str(&format!(" · expires <t:{}:R>", e / 1000));
    }
    t.push_str(match body["choices"].as_array().is_none_or(Vec::is_empty) {
        true => " · tap, or reply yes or no",
        false => " · tap, or reply with its number",
    });
    t
}

/// What the Details press shows its presser alone: the whole request, as the
/// core lists it: what, the target, its preview, the rule that held it, who
/// asked, when it expires and the question's id.
pub fn details(req: &theseus_protocol::ConfirmRequest) -> String {
    let mut t = format!(
        "**Details**\n**What:** {}",
        clip(
            &theseus_protocol::term::words::summarize(&req.tool, &req.input),
            300
        )
    );
    if let Some(r) = req.resource.as_deref().filter(|r| !r.is_empty()) {
        t.push_str(&format!(
            "\n**Target:** `{}`",
            clip(&r.replace('`', "'"), 200)
        ));
    }
    t.push_str(&format!("\n**Tool:** `{}`", req.tool));
    t.push_str(&format!("\n**Rule:** {}", clip(req.reason.trim(), 400)));
    let asker = match &req.task {
        Some(task) => format!("task {}", task.short),
        None => format!("conversation …{}", tail(&req.session_id)),
    };
    t.push_str(&format!("\n**Asked by:** {asker}"));
    if req.expires_at_ms > 0 {
        t.push_str(&format!(
            "\n**Expires:** <t:{}:R>",
            req.expires_at_ms / 1000
        ));
    }
    t.push_str(&format!("\n**Id:** `{}`", req.correlation_id));
    let hunk = super::preview::hunk(req);
    if !hunk.is_empty() && t.chars().count() + hunk.chars().count() < 1900 {
        t.push_str(&hunk);
    }
    clip(&t, 2000)
}

fn tail(id: &str) -> String {
    let mut t: Vec<char> = id.chars().rev().take(8).collect();
    t.reverse();
    t.into_iter().collect()
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

#[cfg(test)]
mod tests {
    use serde_json::json;

    /// A card shows what, why and what yes does, in that order, with the
    /// expiry and how to answer beneath; a batch's leads with its number; an
    /// ask's options are numbered. A post without a `card` reads as before.
    #[test]
    fn the_card_speaks_plain_words() {
        let card = json!({"kind": "approval", "what": "edit `crates/theseus-core/src/compiler/effort.rs`",
            "why": "Theseus's self-repair: Calls from harness fail asks: `fs.edit` is set to ask before it runs.",
            "yes": "Yes runs this one call."});
        let body = json!({"kind": "needs_you", "card": card, "asked_by": "x", "prompt": "edit",
            "choices": [], "expires_at_ms": 1_700_000_000_000_u64, "text": "plain"});
        let t = super::needs_you(&body);
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines.len(), 4, "{t}");
        assert_eq!(
            lines[0],
            "🔔 **edit `crates/theseus-core/src/compiler/effort.rs`**"
        );
        assert!(lines[1].contains("is set to ask before it runs"), "{t}");
        assert_eq!(lines[2], "Yes runs this one call.");
        assert!(
            lines[3].starts_with("-# Needs you · expires <t:1700000000:R>"),
            "{t}"
        );
        assert!(
            !t.contains("enforcement") && !t.contains("theseus answer"),
            "{t}"
        );

        let mut nth = body;
        nth["card"]["n"] = json!(2);
        nth["card"]["of"] = json!(3);
        assert!(
            super::needs_you(&nth).starts_with("🔔 **2 of 3** · **edit"),
            "{nth}"
        );

        let ask = json!({"kind": "needs_you", "choices": ["Careful", "Ship it"],
            "card": {"kind": "ask", "what": "Which plan?", "why": "Theseus asks you to choose.", "yes": "x"}});
        let t = super::needs_you(&ask);
        assert!(t.contains("\n1. Careful\n2. Ship it\n"), "{t}");
        assert!(!t.contains("\nx\n"), "an ask has no yes line: {t}");

        let old = json!({"asked_by": "Theseus", "prompt": "Ship it?", "choices": []});
        assert_eq!(
            super::needs_you(&old),
            "🔔 **Needs you** · Theseus\n**Ship it?**"
        );
    }

    #[test]
    fn details_name_the_tool_the_rule_the_asker_and_the_id() {
        let req = theseus_protocol::ConfirmRequest {
            correlation_id: "act_0000feed".into(),
            session_id: "ses_00000000harbour".into(),
            tool: "fs.edit".into(),
            input: json!({"path": "a/b.rs"}),
            resource: Some("/work/a/b.rs".into()),
            reason: "fs.edit — approve (enforcement = approve)".into(),
            expires_at_ms: 1_700_000_000_000,
            ..Default::default()
        };
        let t = super::details(&req);
        for part in [
            "Edit a/b.rs",
            "`/work/a/b.rs`",
            "`fs.edit`",
            "enforcement = approve",
            "act_0000feed",
            "<t:1700000000:R>",
            "conversation …0harbour",
        ] {
            assert!(t.contains(part), "{part}: {t}");
        }
    }
}
