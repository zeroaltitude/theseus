//! `owner_ask` through whole cores (theseus-j7cje): a question with 2 to 6
//! options that waits for the owner as an approval does, is answered by
//! number from the CLI, reads back as the call's result, and expires to its
//! default; and the always-on channel: the needs-you post after 60 seconds
//! with no screen open, 300 with one, never for a question answered in time,
//! settled when the question closes, and none for a question that has its
//! card in a Discord place. A child of `tests_m3` for its rig and helpers.

use super::*;
use crate::approval::Surface::{Cli, Mcp};
use theseus_protocol::error_code;

fn options(n: usize) -> Value {
    let labels = [
        "Hold",
        "Ship it behind the flag",
        "Ship it",
        "Roll back",
        "Ask again tomorrow",
        "Drop it",
    ];
    json!(labels[..n]
        .iter()
        .map(|l| json!({"label": l}))
        .collect::<Vec<_>>())
}

fn asking(n: usize, extra: Value) -> Vec<Scripted> {
    let mut input = json!({"question": "Ship the release?", "options": options(n)});
    for (k, v) in extra.as_object().into_iter().flatten() {
        input[k] = v.clone();
    }
    vec![
        Scripted::tools("", &[("t1", "owner_ask", input)]),
        Scripted::text("Going on."),
    ]
}

/// `work.answer` from the CLI.
async fn answer(
    r: &Rig,
    corr: &str,
    option: &str,
    note: Option<&str>,
) -> Result<Value, theseus_protocol::RpcError> {
    let mut params = json!({"question_id": corr, "option": option});
    if let Some(n) = note {
        params["note"] = json!(n);
    }
    rpc_as(
        &r.core,
        surface("sock#1", Cli),
        theseus_protocol::method::WORK_ANSWER,
        params,
    )
    .await
}

/// The text the call's result gave the model.
fn result_text(r: &Rig, sid: &str) -> String {
    results(&r.core, sid)
        .pop()
        .map(|(_, t)| t)
        .expect("the call has a result")
}

/// A question asked, waiting: its session, execution and id.
async fn asked(r: &Rig) -> (String, String, String) {
    let res = turn(&r.core, None, "decide").await;
    let corr = res.awaiting_confirm.clone().expect("the question waits");
    (res.session_id, res.execution_id.unwrap(), corr)
}

/// The owner answers from the CLI by number, the turn resumes, and the model
/// reads which option was chosen, with the note.
#[tokio::test]
async fn the_owner_answers_by_number_and_the_call_reads_it_back() {
    let r = rig(asking(3, json!({})));
    let (sid, exec, corr) = asked(&r).await;
    let q = r.core.confirm_list().unwrap().remove(0);
    assert_eq!(q.tool, "owner_ask");
    assert_eq!(
        q.options.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
        ["choice:0", "choice:1", "choice:2", "choice:other"]
    );
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    assert!(a.awaits_confirm(), "an ask waits as an approval does");
    let done = answer(&r, &corr, "2", Some("after lunch")).await.unwrap();
    assert_eq!(done["option"], "choice:1");
    assert_eq!(done["resumes"]["id"], exec.as_str());
    let cont = r.core.continue_execution(&exec).await.unwrap().unwrap();
    assert_eq!(cont.output, "Going on.");
    assert_eq!(
        result_text(&r, &sid),
        "The owner chose 2, \"Ship it behind the flag\"; note: after lunch."
    );
    let rows = ledgered(&r, "action.confirm_answered");
    assert_eq!(rows[0]["option"], "choice:1", "{rows:?}");
}

/// "Something else" is the owner's own words: it needs the note, and the
/// model reads them. A plain `action.confirm` yes chooses nothing.
#[tokio::test]
async fn something_else_takes_a_note_and_a_plain_yes_chooses_nothing() {
    let r = rig(asking(2, json!({})));
    let (sid, exec, corr) = asked(&r).await;
    let e = answer(&r, &corr, "other", None).await.unwrap_err();
    assert_eq!(e.code, error_code::INVALID_PARAMS, "{e:?}");
    assert!(e.message.contains("--note"), "{}", e.message);
    let e = answer(&r, &corr, "3", None).await.unwrap_err();
    assert!(e.message.contains("1 to 2"), "{}", e.message);
    let e = answer(&r, &corr, "approve", None).await.unwrap_err();
    assert!(e.message.contains("1 to 2"), "{}", e.message);
    let e = r
        .core
        .confirm_action(&corr, true, None, "test")
        .expect_err("a yes chooses nothing");
    assert!(e.to_string().contains("question with options"), "{e}");
    assert!(r
        .core
        .kernel
        .action(&corr)
        .unwrap()
        .unwrap()
        .awaits_confirm());
    answer(&r, &corr, "other", Some("neither: wait for Monday"))
        .await
        .unwrap();
    r.core.continue_execution(&exec).await.unwrap();
    assert_eq!(
        result_text(&r, &sid),
        "The owner took none of the options and answered in their own words: neither: wait for Monday"
    );
}

/// 2 and 6 options are asked; 1 and 7 are the model's mistake, answered as
/// an error naming the limit, with no question.
#[tokio::test]
async fn two_and_six_options_are_accepted_and_one_and_seven_are_refused() {
    for n in [2, 6] {
        let r = rig(asking(n, json!({})));
        let (_, _, corr) = asked(&r).await;
        assert!(r.core.kernel.action(&corr).unwrap().is_some());
    }
    for n in [1, 7] {
        let mut s = asking(2, json!({}));
        let mut input = json!({"question": "Ship?", "options": options(2)});
        input["options"] = json!((0..n)
            .map(|i| json!({"label": format!("O{i}")}))
            .collect::<Vec<_>>());
        s[0] = Scripted::tools("", &[("t1", "owner_ask", input)]);
        let r = rig(s);
        let res = turn(&r.core, None, "decide").await;
        assert!(res.awaiting_confirm.is_none(), "{n} options asked nothing");
        let (status, text) = results(&r.core, &res.session_id).pop().unwrap();
        assert_eq!(status, ResultStatus::Error, "{n}: {text}");
        assert!(text.contains("2 to 6 options"), "{n}: {text}");
    }
}

/// A question nobody answers expires at its own life, not the approvals': to
/// its default, or to "no one answered" when it has none. One not yet due
/// stays open.
#[tokio::test]
async fn an_unanswered_question_expires_to_its_default_or_to_nothing() {
    let r = rig(asking(3, json!({"default": 3, "expires_in": 1})));
    let (sid, exec, corr) = asked(&r).await;
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    let now = a.planned_at_ms;
    assert_eq!(r.core.expire_questions(now + 59_000), 0, "not yet due");
    assert_eq!(r.core.expire_questions(now + 61_000), 1);
    r.core.continue_execution(&exec).await.unwrap();
    let text = result_text(&r, &sid);
    assert!(
        text.contains(
            "No one answered within 1 minute; the default applied: option 3, \"Ship it\""
        ),
        "{text}"
    );
    let rows = ledgered(&r, "action.expired");
    assert_eq!(rows.len(), 1, "{rows:?}");

    let r = rig(asking(2, json!({"expires_in": 5})));
    let (sid, exec, corr) = asked(&r).await;
    let now = r.core.kernel.action(&corr).unwrap().unwrap().planned_at_ms;
    assert_eq!(r.core.expire_questions(now + 4 * 60_000), 0);
    assert_eq!(r.core.expire_questions(now + 5 * 60_000 + 1), 1);
    r.core.continue_execution(&exec).await.unwrap();
    let text = result_text(&r, &sid);
    assert!(
        text.contains("No one answered within 5 minutes, and the question had no default"),
        "{text}"
    );
}

/// A question left to its default life waits 30 minutes, not the approvals'.
#[tokio::test]
async fn a_question_that_names_no_life_waits_thirty_minutes() {
    let r = rig(asking(2, json!({})));
    let (_, _, corr) = asked(&r).await;
    let req = r.core.confirm_list().unwrap().remove(0);
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    assert_eq!(req.expires_at_ms, a.planned_at_ms + 30 * 60_000);
    assert_eq!(
        req.question(theseus_protocol::work::QuestionKind::Ask, true)
            .on_expiry
            .as_deref(),
        Some("no default")
    );
}

/// An answer counts from the owner in a private place, as an approval's
/// does: the MCP surface is refused, and the question keeps waiting.
#[tokio::test]
async fn an_answer_to_a_question_counts_only_from_a_private_place() {
    let r = rig(asking(2, json!({})));
    let (_, _, corr) = asked(&r).await;
    let e = rpc_as(
        &r.core,
        surface("mcp#1", Mcp),
        theseus_protocol::method::WORK_ANSWER,
        json!({"question_id": corr, "option": "1"}),
    )
    .await
    .unwrap_err();
    assert_eq!(e.code, error_code::REFUSED, "{e:?}");
    assert!(r
        .core
        .kernel
        .action(&corr)
        .unwrap()
        .unwrap()
        .awaits_confirm());
}

// ----------------------------------------------------- the always-on channel

/// An owner's rig whose config `tweak` changes.
fn owned_tweaked(script: Vec<Scripted>, tweak: impl FnOnce(&mut Config)) -> Rig {
    let r = rig_with(script, tweak);
    bind_places(&r, false);
    r
}

fn needs_you_posts(r: &Rig) -> Vec<theseus_kernel::Action> {
    r.core
        .kernel
        .outbox_actions()
        .unwrap()
        .into_iter()
        .filter(|a| crate::outbox::kind_of(a) == "needs_you")
        .collect()
}

fn settles(r: &Rig) -> Vec<theseus_kernel::Action> {
    r.core
        .kernel
        .outbox_actions()
        .unwrap()
        .into_iter()
        .filter(|a| crate::outbox::kind_of(a) == "settle")
        .collect()
}

/// With no screen open a question still open after 60 s pings the owner's DM
/// once: not at 59 s, not twice. Answering it from the CLI settles the post.
#[tokio::test]
async fn a_question_with_no_screen_open_pings_after_sixty_seconds_and_settles_when_answered() {
    let r = owned_rig(asking(2, json!({})), false);
    let (_, exec, corr) = asked(&r).await;
    let t0 = r.core.kernel.action(&corr).unwrap().unwrap().planned_at_ms;
    assert_eq!(r.core.bus.interactive_watchers(), 0);
    assert_eq!(r.core.escalate_questions(t0 + 59_000), 0, "not yet");
    assert!(needs_you_posts(&r).is_empty());
    assert_eq!(r.core.escalate_questions(t0 + 60_000), 1);
    let posts = needs_you_posts(&r);
    assert_eq!(posts.len(), 1);
    let body = crate::outbox::body_of(&posts[0]);
    assert_eq!(body["question"], corr.as_str());
    assert!(body["text"].as_str().unwrap().contains("1. Hold"), "{body}");
    assert_eq!(
        crate::outbox::target_of(&posts[0]),
        crate::outbox::OPERATOR_TARGET
    );
    assert_eq!(r.core.escalate_questions(t0 + 90_000), 0, "once");
    assert!(settles(&r).is_empty());
    answer(&r, &corr, "1", None).await.unwrap();
    let settled = settles(&r);
    assert_eq!(settled.len(), 1, "the answer settled the post");
    let b = crate::outbox::body_of(&settled[0]);
    assert_eq!(b["card"], posts[0].correlation_id.as_str());
    assert_eq!(b["closed"]["how"], "answered");
    let _ = exec;
}

/// With a Theseus screen open the wait is 300 s.
#[tokio::test]
async fn a_question_with_a_screen_open_waits_five_minutes() {
    let r = owned_rig(asking(2, json!({})), false);
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    r.core
        .bus
        .watch_work("screen", tx, crate::push::WorkScope::default(), true);
    assert_eq!(r.core.bus.interactive_watchers(), 1);
    let (_, _, corr) = asked(&r).await;
    let t0 = r.core.kernel.action(&corr).unwrap().unwrap().planned_at_ms;
    assert_eq!(r.core.escalate_questions(t0 + 60_000), 0);
    assert_eq!(r.core.escalate_questions(t0 + 299_000), 0);
    assert_eq!(r.core.escalate_questions(t0 + 300_000), 1);
}

/// A question answered inside the window never posts; a question asked in a
/// Discord place has its card there and gets no second post; and with no DM
/// bound there is no phone to ping.
#[tokio::test]
async fn an_answered_question_a_carded_one_and_a_dmless_daemon_post_nothing() {
    let r = owned_rig(asking(2, json!({})), false);
    let (_, _, corr) = asked(&r).await;
    let t0 = r.core.kernel.action(&corr).unwrap().unwrap().planned_at_ms;
    answer(&r, &corr, "2", None).await.unwrap();
    assert_eq!(r.core.escalate_questions(t0 + 120_000), 0);
    assert!(needs_you_posts(&r).is_empty());

    // A question in a Discord place: its card is posted at once.
    let r = owned_rig(asking(2, json!({})), false);
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    r.core
        .outbox
        .bind_place(&format!("dm:{OWNER}"), &rec.session_id)
        .unwrap();
    let res = turn(&r.core, Some(&rec.session_id), "decide").await;
    let corr = res.awaiting_confirm.unwrap();
    assert!(r.core.outbox.has_card(&corr), "its card is in the place");
    let t0 = r.core.kernel.action(&corr).unwrap().unwrap().planned_at_ms;
    assert_eq!(r.core.escalate_questions(t0 + 600_000), 0);
    assert!(needs_you_posts(&r).is_empty());

    // No DM with the owner.
    let r = rig(asking(2, json!({})));
    let (_, _, corr) = asked(&r).await;
    let t0 = r.core.kernel.action(&corr).unwrap().unwrap().planned_at_ms;
    assert_eq!(r.core.escalate_questions(t0 + 600_000), 0);
    assert!(needs_you_posts(&r).is_empty());
}

/// An approval pings like any question, and a restart does not post it again.
#[tokio::test]
async fn an_approval_pings_too_and_a_second_read_of_the_outbox_posts_none_again() {
    let r = owned_rig(write_script(), false);
    let res = turn(&r.core, None, "write out.txt").await;
    let corr = res.awaiting_confirm.unwrap();
    let t0 = r.core.kernel.action(&corr).unwrap().unwrap().planned_at_ms;
    assert_eq!(r.core.escalate_questions(t0 + 61_000), 1);
    assert!(r.core.outbox.pinged(&corr));
    let body = crate::outbox::body_of(&needs_you_posts(&r)[0]).clone();
    assert!(
        body["text"].as_str().unwrap().contains("theseus answer"),
        "{body}"
    );
    assert_eq!(r.core.escalate_questions(t0 + 700_000), 0);
}

/// theseus-5d6tp: a call's question nobody carded in its first life (the
/// daemon was down, or the DM unbound) is held, not expired, so it is carded
/// when it is next seen, however long it has waited; and an `owner_ask` past
/// its life is not, for it applies its default or says no one answered.
/// Before, `escalate_questions` stopped at the first life for every kind.
#[tokio::test]
async fn a_held_question_past_its_first_life_is_carded_and_an_expired_ask_is_not() {
    let r = owned_rig(write_script(), false);
    let res = turn(&r.core, None, "write out.txt").await;
    let corr = res.awaiting_confirm.unwrap();
    let t0 = r.core.kernel.action(&corr).unwrap().unwrap().planned_at_ms;
    let ttl = r.core.kernel.config().confirm_ttl_ms;
    assert_eq!(
        r.core.escalate_questions(t0 + 2 * ttl),
        1,
        "held past its first life: carded"
    );
    assert!(r.core.outbox.pinged(&corr));
    assert_eq!(needs_you_posts(&r).len(), 1);

    let r = owned_rig(asking(2, json!({})), false);
    let (_, _, ask) = asked(&r).await;
    let t0 = r.core.kernel.action(&ask).unwrap().unwrap().planned_at_ms;
    assert_eq!(
        r.core.escalate_questions(t0 + 31 * 60_000),
        0,
        "an ask past its 30 minutes expires: it says so itself"
    );
    assert!(!r.core.outbox.pinged(&ask));
    assert!(needs_you_posts(&r).is_empty());
}

/// Half-way to a question's expiry the work board reminds once (rule 2 of the
/// notification policy): the driver's tick calls `remind_questions`, which
/// reads nothing before the reminder is due and makes it once.
#[tokio::test]
async fn a_question_at_half_its_life_is_reminded_of_once() {
    use theseus_protocol::notices::{Urgency, Why};
    let r = rig(asking(2, json!({"expires_in": 10})));
    r.core.push.ensure(&r.core).await.unwrap();
    let (sid, _, corr) = asked(&r).await;
    // The board applies frames on a thread of its own: until it has the
    // question, a reminder finds none to remind (theseus-fpvn8).
    crate::test_support::board_caught_up(&r.core, &sid).await;
    let t0 = r.core.kernel.action(&corr).unwrap().unwrap().planned_at_ms;
    r.core.push.remind_questions(&r.core, t0 + 4 * 60_000);
    let told = |r: &Rig| {
        r.core
            .push
            .notices_since(0)
            .0
            .into_iter()
            .filter(|n| n.why == Why::Reminder)
            .collect::<Vec<_>>()
    };
    assert!(told(&r).is_empty(), "not yet: half of 10 minutes is 5");
    r.core.push.remind_questions(&r.core, t0 + 5 * 60_000 + 1);
    let once = told(&r);
    assert_eq!(once.len(), 1, "{once:?}");
    assert_eq!(once[0].urgency, Urgency::Interrupt);
    assert!(once[0].line.contains("still waiting"), "{}", once[0].line);
    r.core.push.remind_questions(&r.core, t0 + 6 * 60_000);
    assert_eq!(told(&r).len(), 1, "once");
}

/// The one pacing rule for unprompted posts (`pacer.rs`): after one needs-you,
/// questions due inside the window post at once, silently and together
/// (theseus-27p6w): a header and a card for each, each marked so none posts
/// again, and the window is not taken.
#[tokio::test]
async fn unprompted_pings_go_one_a_window_and_the_rest_together() {
    let ask = || {
        let input =
            json!({"question": "Ship the release?", "options": options(2), "expires_in": 120});
        Scripted::tools("", &[("t1", "owner_ask", input)])
    };
    let r = owned_rig(
        vec![ask(), ask(), ask(), Scripted::text("Going on.")],
        false,
    );
    let (_, _, a) = asked(&r).await;
    let t0 = r.core.kernel.action(&a).unwrap().unwrap().planned_at_ms;
    assert_eq!(
        r.core.escalate_questions(t0 + 61_000),
        1,
        "the first goes at once"
    );
    let (_, _, b) = asked(&r).await;
    let (_, _, c) = asked(&r).await;
    let took = r.core.outbox.pacer.last_ms();
    assert_eq!(
        r.core.escalate_questions(t0 + 10 * 60_000),
        2,
        "due with the window closed: together, and silent"
    );
    assert_eq!(r.core.outbox.pacer.last_ms(), took, "no window taken");
    assert_eq!(
        needs_you_posts(&r).len(),
        4,
        "the first, then a header and a card each"
    );
    let posts = needs_you_posts(&r);
    let header = crate::outbox::body_of(&posts[1]);
    assert_eq!(
        header["questions"],
        json!([b.clone(), c.clone()]),
        "{header}"
    );
    assert!(
        header["question"].is_null(),
        "a header is no one question's card"
    );
    assert!(
        header["text"]
            .as_str()
            .unwrap()
            .contains("2 questions need you"),
        "{header}"
    );
    let cards: Vec<_> = posts[2..].iter().map(crate::outbox::body_of).collect();
    assert_eq!(
        cards
            .iter()
            .map(|c| c["question"].clone())
            .collect::<Vec<_>>(),
        [json!(b), json!(c)]
    );
    assert_eq!(header["silent"], true, "the window was closed: no buzz");
    assert!(cards.iter().all(|c| c["silent"] == true), "nor from a card");
    assert_eq!(cards[1]["card"]["n"], 2);
    assert_eq!(cards[1]["card"]["of"], 2);
    assert!(r.core.outbox.pinged(&b) && r.core.outbox.pinged(&c));
    assert_eq!(r.core.escalate_questions(t0 + 31 * 60_000), 0, "once each");
    // Answering one of them settles that card alone.
    answer(&r, &b, "1", None).await.unwrap();
    let settled: Vec<Value> = settles(&r)
        .iter()
        .map(|s| crate::outbox::body_of(s)["question"].clone())
        .collect();
    assert!(
        settled.contains(&json!(b)) && !settled.contains(&json!(c)),
        "{settled:?}"
    );
}

/// `[notify] dm_cards = false` is the post of before, byte for byte: after one
/// needs-you, questions due inside the window go at once in one silent post
/// (no buttons, no settle), each marked so none posts again.
#[tokio::test]
async fn with_dm_cards_off_the_rest_go_together_in_one_nudge() {
    let ask = || {
        let input =
            json!({"question": "Ship the release?", "options": options(2), "expires_in": 120});
        Scripted::tools("", &[("t1", "owner_ask", input)])
    };
    let r = owned_tweaked(
        vec![ask(), ask(), ask(), Scripted::text("Going on.")],
        |c| c.notify.dm_cards = false,
    );
    let (_, _, a) = asked(&r).await;
    let t0 = r.core.kernel.action(&a).unwrap().unwrap().planned_at_ms;
    assert_eq!(
        r.core.escalate_questions(t0 + 61_000),
        1,
        "the first goes at once"
    );
    let (_, _, b) = asked(&r).await;
    let (_, _, c) = asked(&r).await;
    let took = r.core.outbox.pacer.last_ms();
    assert_eq!(
        r.core.escalate_questions(t0 + 10 * 60_000),
        2,
        "due with the window closed: together, and silent"
    );
    assert_eq!(r.core.outbox.pacer.last_ms(), took, "no window taken");
    let posts = needs_you_posts(&r);
    assert_eq!(posts.len(), 2);
    let batch = crate::outbox::body_of(&posts[1]);
    assert_eq!(batch["questions"], json!([b.clone(), c.clone()]), "{batch}");
    assert!(
        batch["question"].is_null(),
        "a batch is no one question's card"
    );
    assert!(
        batch["text"]
            .as_str()
            .unwrap()
            .contains("2 questions need you"),
        "{batch}"
    );
    assert!(r.core.outbox.pinged(&b) && r.core.outbox.pinged(&c));
    assert_eq!(batch["silent"], true, "the window was closed");
    assert_eq!(r.core.escalate_questions(t0 + 31 * 60_000), 0, "once each");
    // Answering one of them settles nothing: the batch is no card.
    answer(&r, &b, "1", None).await.unwrap();
    assert!(settles(&r)
        .iter()
        .all(|s| crate::outbox::body_of(s)["question"] != json!(b)));
}

/// The rule is a setting: 0 turns it off, and each due question pings at once.
#[tokio::test]
async fn a_pace_of_zero_turns_the_rule_off() {
    let ask = || {
        let input =
            json!({"question": "Ship the release?", "options": options(2), "expires_in": 120});
        Scripted::tools("", &[("t1", "owner_ask", input)])
    };
    let r = rig_with(vec![ask(), ask(), Scripted::text("Going on.")], |c| {
        c.notify.unprompted_pace_secs = 0
    });
    bind_places(&r, false);
    let (_, _, a) = asked(&r).await;
    let t0 = r.core.kernel.action(&a).unwrap().unwrap().planned_at_ms;
    assert_eq!(r.core.escalate_questions(t0 + 61_000), 1);
    let _ = asked(&r).await;
    assert_eq!(r.core.escalate_questions(t0 + 62_000), 1, "no window");
    assert_eq!(needs_you_posts(&r).len(), 2);
}

mod cards;
mod due;
