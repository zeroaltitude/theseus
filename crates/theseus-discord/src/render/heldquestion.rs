//! What a Discord card may honestly say about a question's wait
//! (theseus-lrc3m, surface 3). The sibling of the TUI's `held.rs` and the
//! cockpit's `heldquestion.ts`, and named for the cockpit's file because this
//! crate's `render/held.rs` is about held *turns*, not held questions.
//!
//! Since theseus-v2dnc a call's question is held past its life, never
//! expired: its `expires_at_ms` is the time of its **first notice**, not a
//! deadline (`rpc/confirms.rs` stamps `expiry_of(a, ttl)`, and
//! `rpc/question_waits.rs`'s `held_past_life` holds it afterwards). So
//! `expires <t:…:R>` on such a card is a false sentence from the moment it is
//! posted: it reads "expires in 5 minutes", and in five minutes the question
//! is not gone, it is noticed and then waits for an answer forever.
//!
//! **Why this is not a `now_ms` test.** A Discord card's words are frozen
//! when it is posted. The `card` event is emitted once, at the moment the
//! call asks (`theseus-core`'s `toolrun.rs` and `turn.rs`), rendered once by
//! the courier, and re-rendered only when the question *closes*
//! (`render::settled`). The one live thing in it is Discord's own `<t:…:R>`,
//! which the client redraws. So a card that promises an expiry at post time
//! keeps promising it: two hours later the footer reads "expires 2 hours
//! ago". A fix that chose its words by comparing `now` with the first notice
//! would be deciding at post time, when the question is always inside its
//! first life, and would therefore leave that footer exactly as it is. The
//! honest words have to be unconditional, and they are: a held question's
//! card names the time it was **asked**, which `<t:…:R>` then keeps true at
//! every moment ("held since 3 minutes ago", "held since 2 hours ago").
//! Piece C's `deadline_of`/`deadlineOf` take no `now` either.
//!
//! The owner unlocked this surface on 2026-11-10 (Discord DM), to the
//! question whether to exclude the footer or fix it: *"fix the footer,
//! keeping the features i loved about the footer"*. So the shape of the
//! change is the narrowest one that stops the lie: the words beside the
//! timestamp, and nothing else. The live `<t:…:R>`, the buttons, the Details
//! fields, the task prefix and an `owner_ask`'s `(fallback)` clause are all
//! untouched.
//!
//! The `-# Needs you ·` line (the needs-you post's own footer,
//! `render::ask::card_text`) kept saying `expires` here when this surface
//! landed, because it is a different renderer and was not this commit's
//! question. The owner fixed it the same way later that night (theseus-la5oc,
//! Discord DM 2026-11-10 22:31, *"your recommendation"* to **FIX IT THE SAME
//! WAY**): `needs_you::card_body` now carries `held`/`requested_at_ms`
//! alongside `expires_at_ms`, and `card_text` picks the same two clauses this
//! file's `footer_clause` does, for the same reason.

use theseus_protocol::work::ask::held_tool;
use theseus_protocol::ConfirmRequest;

/// Whether this request's question is held until it is answered, so no card
/// of it may promise a deadline.
///
/// `held_tool` alone is a proxy — it is the complement of the daemon's
/// `EXPIRING_TOOLS` — and there are two questions it answers wrongly, so both
/// are excluded here rather than left to the caller:
///
/// * a **budget** question holds too, but says so in its own words and shows
///   no time at all (`render::card` returns before any footer with a
///   timestamp), so it is not this function's business;
/// * a **task change** really does expire (`fact::task_graph`'s
///   `TaskChangeExpired`), so its `expires <t:…:R>` is true and must stay.
///
/// What is left is exactly what `render::card` routes to `approval::card` and
/// `approval::classic`: a tool call waiting for approval.
pub(super) fn is_held(req: &ConfirmRequest) -> bool {
    req.budget.is_none() && req.change.is_none() && held_tool(&req.tool)
}

/// What a card's footer says about the question's time. A held question's
/// names the time it was **asked**, which the client draws as "3 minutes
/// ago", and says plainly that it is still waiting: the same thing the TUI's
/// and the cockpit's `waiting …` say, worded for a live Discord timestamp.
/// A question with a real deadline says when it expires, exactly as before.
pub(super) fn footer_clause(req: &ConfirmRequest) -> String {
    if is_held(req) {
        format!(
            "held since <t:{}:R> · still waiting",
            req.requested_at_ms / 1000
        )
    } else {
        format!("expires <t:{}:R>", req.expires_at_ms / 1000)
    }
}

/// The Details press's line about the question's time, its field name
/// included: `**Held since:**` for a held question, `**Expires:**` for one
/// with a real deadline. None when there is no time to show, as before.
pub(super) fn details_line(req: &ConfirmRequest) -> Option<String> {
    if is_held(req) {
        (req.requested_at_ms > 0).then(|| {
            format!(
                "\n**Held since:** <t:{}:R> · still waiting",
                req.requested_at_ms / 1000
            )
        })
    } else {
        (req.expires_at_ms > 0)
            .then(|| format!("\n**Expires:** <t:{}:R>", req.expires_at_ms / 1000))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};
    use theseus_protocol::ConfirmRequest;

    use crate::render::{card, question_details, Route};

    /// A tool call waiting for approval: a held question, whatever the clock
    /// says. `extra` overrides any field.
    fn held(extra: Value) -> ConfirmRequest {
        let mut v = json!({
            "correlation_id": "act_0000reef", "session_id": "ses_0000lagoon",
            "execution_id": "exe_0000lagoon", "tool": "fs.write",
            "input": {"path": "/srv/chart/reef.md", "content": "reef"},
            "reason": "fs.write - approve (enforcement = approve)",
            "by": "operator", "requested_at_ms": 1_790_000_000_000_u64,
            "expires_at_ms": 1_790_000_300_000_u64,
        });
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        serde_json::from_value(v).unwrap()
    }

    /// The footer, which is the card's last line.
    fn footer(req: &ConfirmRequest) -> String {
        card(req, &Route::Here, "")
            .content
            .lines()
            .last()
            .unwrap()
            .to_string()
    }

    /// The owner unlocked this surface on 2026-11-10: *"fix the footer,
    /// keeping the features i loved about the footer"*. A held question's
    /// card may not promise an expiry, because it has none: it says when it
    /// was asked, as a live Discord timestamp, and that it is still waiting.
    ///
    /// **Both states, one text, deliberately.** The dispatch asked for the
    /// honest words "past its first life" and today's words within it. A
    /// Discord card cannot do that: its words are frozen when it is posted
    /// (the `card` event is emitted once, at the moment the call asks, and
    /// the only re-render is the settle), so the only live thing in the
    /// footer is `<t:…:R>` itself. Words chosen by comparing `now` with the
    /// first notice would be chosen at post time, inside the first life
    /// always, and the footer would keep saying "expires 2 hours ago" for
    /// ever after - the lie the owner asked to have fixed. And the words are
    /// false inside the first life too: since theseus-v2dnc that time is the
    /// question's **first notice**, not a deadline. So this pins the opposite
    /// of a clock test: the footer reads the same whether the first notice is
    /// still ahead or long past, which is what makes it true in both.
    #[test]
    fn a_held_questions_footer_says_it_is_still_waiting_not_that_it_expires() {
        let within = held(json!({"expires_at_ms": 1_790_000_300_000_u64}));
        let past = held(json!({"expires_at_ms": 1_000_000_000_000_u64}));
        let honest = "-# held since <t:1790000000:R> · still waiting";
        assert_eq!(footer(&within), honest, "inside its first life");
        assert_eq!(footer(&past), honest, "past its first life");
        for req in [&within, &past] {
            let c = card(req, &Route::Here, "").content;
            assert!(!c.contains("expires"), "no promise of an expiry: {c}");
        }
    }

    /// The clause that says where else it can be answered still follows the
    /// footer, and a DM route still names the place: the honest words replace
    /// the deadline and nothing around it. Both card shapes, because the
    /// footer is built twice: `approval::card` from the plain words, with the
    /// task's `or` clause, and `approval::classic` for a request without them
    /// (an older daemon), with the `you can also answer` clause.
    #[test]
    fn the_held_footer_keeps_the_place_and_the_answer_elsewhere_clauses() {
        let task =
            json!({"task_id": "tsk_000000reef", "short": "490f10", "title": "chart the reef"});
        let route = Route::Dm {
            user: 271_828_182_845_904_523,
            dm: "a DM".into(),
            place: "#general".into(),
            why: "this channel is shared".into(),
        };

        let classic = held(json!({"task": task}));
        assert!(
            card(&classic, &route, "with `theseus answer`").content.ends_with(
                "-# for #general · held since <t:1790000000:R> · still waiting · you can also answer with `theseus answer`"
            ),
            "{}",
            card(&classic, &route, "with `theseus answer`").content
        );

        let plain = held(json!({"task": task, "plain": {"who": "chart the reef",
            "what": "write `/srv/chart/reef.md`", "why": "", "held": "`fs.write` is set to ask",
            "risk": "writes", "yes": "runs this one call.", "no": "refuses it."}}));
        assert!(
            card(&plain, &route, "with `theseus answer`").content.ends_with(
                "-# for #general · held since <t:1790000000:R> · still waiting · or `theseus answer 490f10 approve`"
            ),
            "{}",
            card(&plain, &route, "with `theseus answer`").content
        );
    }

    /// The Details press (the second half of surface 3) says the same thing,
    /// and every other field it listed is still there, in its order.
    #[test]
    fn a_held_questions_details_say_it_is_still_waiting_and_keep_every_field() {
        let req = held(json!({"resource": "/srv/chart/reef.md"}));
        let t = question_details(&req);
        assert!(
            t.contains("\n**Held since:** <t:1790000000:R> · still waiting"),
            "{t}"
        );
        assert!(!t.contains("Expires"), "{t}");
        for part in [
            "**Details**",
            "**What:**",
            "**Target:** `/srv/chart/reef.md`",
            "**Tool:** `fs.write`",
            "**Rule:** fs.write - approve (enforcement = approve)",
            "**Asked by:**",
            "**Id:** `act_0000reef`",
        ] {
            assert!(t.contains(part), "{part} is still listed: {t}");
        }
    }

    /// **The contract.** The features the owner said to keep are byte-identical:
    /// an `owner_ask`'s footer with its `(fallback)` clause (its question does
    /// expire, so its deadline is true), a task change's footer (it expires
    /// too - `TaskChangeExpired`), and a needs-you post with no `held` field
    /// (an older daemon's, or one of these two kinds escalated to the DM the
    /// same way) still reading `expires`, byte-identical to before theseus-la5oc.
    /// None of the three is a held question, and none of them moves.
    #[test]
    fn the_cards_whose_deadline_is_real_are_untouched() {
        let ask = held(json!({"tool": "owner_ask",
            "input": {"question": "Ship it?", "options": [{"label": "Yes"}, {"label": "No"}], "default": 1}}));
        assert_eq!(
            footer(&ask),
            "-# expires <t:1790000300:R> (default 1: Yes)",
            "an owner_ask expires, and its fallback clause stays"
        );

        let change = held(json!({"tool": "task.update", "input": {},
            "change": {"task": "tsk_000000reef", "title": "Chart the reef", "field": "acceptance",
                       "before": "every marker has a depth", "after": "the chart is signed"}}));
        assert_eq!(
            footer(&change),
            "-# Accept applies it; Decline leaves the task as it is · expires <t:1790000300:R>",
            "a task change expires, so its deadline is true"
        );

        let body = json!({"kind": "needs_you", "asked_by": "a task", "prompt": "Ship it?",
            "choices": [], "expires_at_ms": 1_790_000_300_000_u64,
            "card": {"kind": "approval", "what": "ship it", "why": "because", "yes": "Yes ships it."}});
        let post = crate::render::needs_you(&body);
        assert!(
            post.lines()
                .last()
                .unwrap()
                .starts_with("-# Needs you · expires <t:1790000300:R>"),
            "{post}"
        );
    }

    /// The needs-you post's own footer (`render::ask::card_text`) is a
    /// different renderer from the card this file builds, so surface 3 (the
    /// test above, which this scenario used to sit inside, before
    /// theseus-la5oc) pinned it still saying `expires` and moved on — a held
    /// question's needs-you post was the one surface that fix did not reach.
    /// The owner fixed it the same way at 22:31: `needs_you::card_body` now
    /// adds `held`/`requested_at_ms` beside `expires_at_ms`, and a held
    /// question's needs-you post says `held since … still waiting` too.
    #[test]
    fn the_needs_you_posts_footer_is_fixed_the_same_way() {
        let held_body = json!({"kind": "needs_you", "asked_by": "a task", "prompt": "Ship it?",
            "choices": [], "expires_at_ms": 1_790_000_300_000_u64, "held": true,
            "requested_at_ms": 1_790_000_000_000_u64,
            "card": {"kind": "approval", "what": "ship it", "why": "because", "yes": "Yes ships it."}});
        let post = crate::render::needs_you(&held_body);
        assert!(
            post.lines()
                .last()
                .unwrap()
                .starts_with("-# Needs you · held since <t:1790000000:R> · still waiting"),
            "{post}"
        );
        assert!(!post.contains("expires"), "no promise of an expiry: {post}");

        let real_deadline = json!({"kind": "needs_you", "asked_by": "a task", "prompt": "Ship it?",
            "choices": [], "expires_at_ms": 1_790_000_300_000_u64, "held": false,
            "requested_at_ms": 1_790_000_000_000_u64,
            "card": {"kind": "approval", "what": "ship it", "why": "because", "yes": "Yes ships it."}});
        assert!(
            crate::render::needs_you(&real_deadline)
                .lines()
                .last()
                .unwrap()
                .starts_with("-# Needs you · expires <t:1790000300:R>"),
            "not held: the wording is exactly what it was before"
        );
    }

    /// A budget question holds too, and says so in its own words with no time
    /// at all: `is_held` is not its business, and its card does not move.
    #[test]
    fn a_budget_question_shows_no_time_and_is_not_this_fixs_business() {
        let req = held(json!({"tool": "budget.reset", "expires_at_ms": 0,
            "reason": "This session has spent $99.48 of its $100 limit. Reset its spend to $0 and continue?",
            "budget": {"spent_usd": 99.48, "limit_usd": 100.0, "needed_usd": 0.6, "lifetime_usd": 99.48}}));
        let c = card(&req, &Route::Here, "").content;
        assert!(!c.contains("<t:"), "no timestamp at all: {c}");
        assert!(!c.contains("held since"), "{c}");
        assert!(!super::is_held(&req), "a budget question is excluded");
    }
}
