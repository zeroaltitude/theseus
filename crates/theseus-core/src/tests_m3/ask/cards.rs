//! The needs-you post as cards (theseus-suvpe): each question in plain words
//! from its structured fields, its asker a human name, and `[notify]
//! dm_cards = false` the post of before, byte for byte. A child of `ask`.

use super::*;

/// An approval that waited is one card: what it wants (the tool in words and
/// its target), why it asks and what yes does, never the reason's text, and
/// the CLI hint stays in the plain `text` for anything that shows the post as
/// it is.
#[tokio::test]
async fn a_dm_question_is_a_card_in_plain_words() {
    let r = owned_rig(write_script(), false);
    let res = turn(&r.core, None, "write out.txt").await;
    let corr = res.awaiting_confirm.unwrap();
    let t0 = r.core.kernel.action(&corr).unwrap().unwrap().planned_at_ms;
    assert_eq!(r.core.escalate_questions(t0 + 61_000), 1);
    let req = r.core.confirm_list().unwrap().remove(0);
    let body = crate::outbox::body_of(&needs_you_posts(&r)[0]).clone();
    let card = &body["card"];
    assert_eq!(card["kind"], "approval", "{body}");
    assert!(
        card["what"].as_str().unwrap().starts_with("write `"),
        "{card}"
    );
    // In plain words (theseus-cudar): who wants to do what, the risk, what
    // Yes and No do, and the rule that holds it; no reason without one given.
    assert_eq!(card["who"], "write out.txt", "{card}");
    assert_eq!(card["risk"], "writes", "{card}");
    assert_eq!(card["why"], "", "the model gave no reason: {card}");
    assert!(
        card["held"].as_str().unwrap().contains("set to ask"),
        "{card}"
    );
    assert_eq!(card["yes"], "Yes runs this one call.");
    assert!(
        card["no"].as_str().unwrap().starts_with("No refuses it"),
        "{card}"
    );
    assert!(
        card["line"].as_str().unwrap().contains("wanted to write"),
        "{card}"
    );
    let text = body["text"].as_str().unwrap();
    assert!(
        text.contains("wants to write `") && text.contains("risk: writes"),
        "{text}"
    );
    assert!(
        !text.contains(req.reason.as_str()),
        "the reason is not the card: {text}"
    );
    assert!(text.contains("theseus answer"), "{text}");
}

/// A self task's question is asked by "Theseus's self-repair: <the backlog
/// item's title>", from its task record, never the objective's "Fix the
/// cause" text a session takes for its title; a user task keeps its own title.
#[tokio::test]
async fn a_self_tasks_question_names_its_backlog_title() {
    let r = owned_rig(vec![], false);
    let put = |id: &str, session: &str, title: &str, principal: &str| {
        let t = theseus_protocol::tasks::TaskRecord {
            id: id.into(),
            title: title.into(),
            session: Some(session.into()),
            origin: theseus_protocol::tasks::TaskOrigin {
                principal: principal.into(),
                ..Default::default()
            },
            ..Default::default()
        };
        r.core
            .store
            .append(&[crate::task_graph::record(&t).unwrap()])
            .unwrap();
    };
    put("tsk_1", "ses_self", "Calls from harness fail", "self");
    put("tsk_2", "ses_mine", "Chart the harbour", "owner");
    let mut rec = SessionRecord::new(SessionKind::Task, None);
    rec.session_id = "ses_self".into();
    rec.title = Some("Fix the cause `action.failed:harness`: Calls from harness fail.".into());
    r.core.store.put_session(&rec.session_id, &rec).unwrap();
    let asks = |session: &str| theseus_protocol::ConfirmRequest {
        session_id: session.into(),
        task: Some(theseus_protocol::TaskRef {
            task_id: session.into(),
            short: "7f3a2c".into(),
            title: None,
        }),
        ..Default::default()
    };
    // The locked needs-you card's item text, in plain words: the owner approved
    // this line, 2026-10-22 23:2x, theseus-jsdj5. The item was filed under the
    // old words ("Calls from harness fail") and reads plain all the same.
    assert_eq!(
        r.core.asker_name(&asks("ses_self")),
        "Theseus's self-repair: Harness tool calls failing"
    );
    assert_eq!(r.core.asker_name(&asks("ses_mine")), "Chart the harbour");
    assert_eq!(r.core.asker_name(&asks("ses_none")), "task 7f3a2c");
}

/// With `[notify] dm_cards = false` the single post and the batch are the
/// bodies of before: no `card`, `body_of`'s text, the batch's one nudge.
#[tokio::test]
async fn dm_cards_off_is_todays_post() {
    let r = owned_tweaked(write_script(), |c| c.notify.dm_cards = false);
    let res = turn(&r.core, None, "write out.txt").await;
    let corr = res.awaiting_confirm.unwrap();
    let t0 = r.core.kernel.action(&corr).unwrap().unwrap().planned_at_ms;
    assert_eq!(r.core.escalate_questions(t0 + 61_000), 1);
    // The request is read as the poster reads it (`pending_confirms`), not
    // from `confirm_list`: the list shows a held call's deadline as none,
    // while the post, whose card footer reads the time, keeps the real one.
    let req = r.core.pending_confirms(&res.session_id).unwrap().remove(0);
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    let who = r.core.asker_name(&req);
    let posted = crate::outbox::body_of(&needs_you_posts(&r)[0]).clone();
    assert_eq!(posted, crate::needs_you::body_of(&a, &req, &who));
    assert!(posted["card"].is_null(), "{posted}");
    assert!(
        posted["text"].as_str().unwrap().starts_with("● "),
        "{posted}"
    );
}

/// A thread's session is asked by the thread's title, not by the first
/// message it was given; a task's title still wins where a task asks
/// (theseus-zcn80).
#[tokio::test]
async fn a_thread_sessions_question_is_asked_by_the_thread() {
    use crate::approval::{Answerer, Surface};
    let r = owned_rig(vec![], false);
    let cli = || Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    };
    r.core
        .thread_new(
            &theseus_protocol::threads::ThreadNewParams {
                slug: "harbour-log".into(),
                title: Some("Harbour log notes".into()),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    let sid = r
        .core
        .thread_session("harbour-log", cli())
        .unwrap()
        .session_id;
    let mut rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    rec.title = Some("Say ok, nothing else.".into());
    r.core.store.put_session(&sid, &rec).unwrap();
    let req = theseus_protocol::ConfirmRequest {
        session_id: sid,
        ..Default::default()
    };
    assert_eq!(r.core.asker_name(&req), "Harbour log notes");
}
