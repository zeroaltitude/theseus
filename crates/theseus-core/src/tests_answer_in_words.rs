//! The owner's yes, said in words, through the whole core (theseus-3i6d1,
//! sub-items (3) and (2) of the `task.close`/`abandoned` line; the owner,
//! 2026-11-07 10:24: *"I had just approved it verbally… this shouldn't have
//! thrown up a response dialog, but just notified me, further reinforcing
//! that notify >> approve"*).
//!
//! Each case drives `turn.submit` over a real connection, so the words take
//! the path a message from the CLI takes. The pure reader's own table is in
//! `rpc::answer_in_words::tests`.

use std::sync::atomic::Ordering::SeqCst;
use std::time::Duration;

use serde_json::json;

use crate::approval::Surface;
use crate::session::SessionRecord;
use crate::task_graph::TaskState;
use crate::test_support::{ledger_rows_100k as rows, turn, until};
use crate::tests_task_graph::{rig_on, session, Rig};

const CENSUS: &str = "chart every buoy of the outer harbour, please";
const OTHER: &str = "sound the inner channel at low tide, please";

/// Say `text` in `sid` as the CLI does: the whole `turn.submit` path, which
/// is where the owner's words are read.
async fn say(r: &Rig, sid: &str, text: &str) {
    let got = crate::tests_capture::call_on(
        &r.core,
        Surface::Cli,
        "turn.submit",
        json!({"session_id": sid, "input": text}),
    )
    .await;
    assert!(got.error.is_none(), "{text:?}: {:?}", got.error);
}

/// The notices this path posts where approvals go, oldest first. Every one
/// of them is headed as every operator notice is (`owner_note::is_headed`).
fn notes(r: &Rig) -> Vec<String> {
    r.core
        .outbox
        .open_for(crate::outbox::OPERATOR_TARGET)
        .iter()
        .filter(|a| crate::outbox::kind_of(a) == "notice")
        .filter_map(|a| {
            crate::outbox::body_of(a)["text"]
                .as_str()
                .map(str::to_string)
        })
        .filter(|t| t.to_lowercase().contains("your yes"))
        .inspect(|t| assert!(crate::owner_note::is_headed(t), "not headed: {t:?}"))
        .collect()
}

/// Make a task the owner owns, held open, and return it. The quote must be
/// the owner's own words, so the task is his (`is_owners`).
async fn a_task(r: &Rig, sid: &str, title: &str, quote: &str) -> crate::task_graph::TaskRecord {
    r.calls(&[(
        "task_create",
        json!({"brief": format!("{quote} — and report the depths."),
               "title": title, "acceptance": ["a chart exists"],
               "arrangement": {"pieces": [{"quote": quote, "role": "objective"}]}}),
    )]);
    turn(&r.core, sid, &format!("START: {quote}")).await;
    let t = r.by_title(title);
    assert!(t.is_owners(), "{t:?}");
    t
}

/// Ask to abandon `t`: with `task.close` unset that is a layer-1 change, so
/// it waits for the operator.
async fn ask_to_abandon(r: &Rig, sid: &str, t: &crate::task_graph::TaskRecord) {
    r.calls(&[(
        "task_close",
        json!({"id": t.id, "version": 1, "outcome": "abandoned"}),
    )]);
    turn(&r.core, sid, "drop it").await;
}

/// (2) One question waiting, and a yes in words answers it — and because it
/// went through the same `confirm_action` a button press does, it is a real
/// yes: `said_yes` holds and exactly one consent row is written.
#[tokio::test]
async fn a_yes_in_words_answers_the_one_question_waiting() {
    let r = rig_on(tempfile::tempdir().unwrap(), |_| {});
    let c = &r.core;
    let s = session(c);
    r.model.hold_tasks.store(true, SeqCst);
    let t = a_task(&r, &s, "Chart the outer harbour", CENSUS).await;
    ask_to_abandon(&r, &s, &t).await;
    assert_eq!(c.confirm_list().unwrap().len(), 1);

    say(&r, &s, "Yes, you may abandon it").await;

    until("the abandon", || {
        r.task(&t.id).state == TaskState::Abandoned
    })
    .await;
    assert!(
        c.confirm_list().unwrap().is_empty(),
        "the question is still waiting"
    );
    // The yes was asked for and given, so the consent record is right here
    // (unlike a notify-posture abandon, which writes none).
    assert_eq!(rows(c, "task.change_accepted").len(), 1);
    assert_eq!(rows(c, "task.closed").len(), 1);
    // The answer is recorded as the owner's own, through the one answer
    // path, and says it came from his words.
    let answered = rows(c, "action.confirm_answered");
    assert_eq!(answered.len(), 1, "{answered:?}");
    assert_eq!(answered[0].data["approved"], json!(true));
    assert_eq!(answered[0].data["note"], json!("said in words"));
    // Nothing had to be asked twice: no notice was posted.
    assert!(notes(&r).is_empty(), "{:?}", notes(&r));
    r.model.hold_tasks.store(false, SeqCst);
}

/// (3) Two questions waiting, and a bare yes answers NEITHER: the session is
/// told to ask which, with both named. Then the owner's answer naming one
/// takes that one and leaves the other waiting.
#[tokio::test]
async fn two_questions_waiting_take_no_bare_yes_and_the_one_he_names_is_taken() {
    let r = rig_on(tempfile::tempdir().unwrap(), |_| {});
    let c = &r.core;
    let s = session(c);
    r.model.hold_tasks.store(true, SeqCst);
    // Both questions come from sibling sessions that answer at this place,
    // as a task's own asks do. Two reasons they are not in the owner's own
    // conversation: a question holds its turn, so one conversation never has
    // two of its own waiting; and his next message withdraws his own
    // conversation's waiting question, which would mask the thing under test.
    let beside = |n: usize| {
        let rec = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
        c.store.put_session(&rec.session_id, &rec).unwrap();
        c.outbox
            .show_at(&rec.session_id, c.outbox.target(&s).as_deref())
            .unwrap();
        assert_eq!(c.outbox.target(&rec.session_id), c.outbox.target(&s), "{n}");
        rec.session_id
    };
    let (sa, sb) = (beside(1), beside(2));
    let census = a_task(&r, &sa, "Chart the outer harbour", CENSUS).await;
    ask_to_abandon(&r, &sa, &census).await;
    let other = a_task(&r, &sb, "Sound the inner channel", OTHER).await;
    ask_to_abandon(&r, &sb, &other).await;
    let waiting = c.confirm_list().unwrap();
    assert_eq!(waiting.len(), 2, "{waiting:?}");

    let (before_census, before_other) = (r.task(&census.id).state, r.task(&other.id).state);
    assert_ne!(before_census, TaskState::Abandoned);
    assert_ne!(before_other, TaskState::Abandoned);
    // The sitting's own words, which name neither task.
    say(&r, &s, "Yes, you may abandon it").await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(
        c.confirm_list().unwrap().len(),
        2,
        "a bare yes answered one of two questions"
    );
    assert_eq!(r.task(&census.id).state, before_census);
    assert_eq!(r.task(&other.id).state, before_other);
    assert!(
        rows(c, "task.change_accepted").is_empty(),
        "a consent row was written for a yes that answered nothing"
    );
    assert!(
        rows(c, "action.confirm_answered").is_empty(),
        "a question was answered by an ambiguous yes"
    );
    let said = notes(&r);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("Which one do you mean?"), "{said:?}");
    assert!(said[0].contains("Nothing has been approved"), "{said:?}");
    // Both are named in it, by the task each would abandon.
    for t in [&census, &other] {
        assert!(said[0].contains(&t.id), "{} is not named: {said:?}", t.id);
        assert!(
            said[0].contains(&t.title),
            "{} is not named: {said:?}",
            t.title
        );
    }

    // Naming one takes that one; the other is still waiting.
    say(&r, &s, &format!("yes, abandon {}", census.id)).await;
    until("the named abandon", || {
        r.task(&census.id).state == TaskState::Abandoned
    })
    .await;
    assert_eq!(
        r.task(&other.id).state,
        before_other,
        "the question he did not name was answered too"
    );
    assert_eq!(c.confirm_list().unwrap().len(), 1);
    assert_eq!(rows(c, "task.change_accepted").len(), 1);
    r.model.hold_tasks.store(false, SeqCst);
}

/// A yes that arrives after its question has expired answers nothing, and
/// the conversation is told why rather than left silent.
///
/// A call's question is held past its life and never expired
/// (theseus-v2dnc) — the one exception is a session nobody is left in, so
/// the test retires the session to drive the real expiry path
/// (`decline_expired`) and then puts it back, so the yes arrives in a live
/// conversation exactly as the owner's would.
#[tokio::test]
async fn a_yes_after_its_question_expired_answers_nothing_and_says_why() {
    let r = rig_on(tempfile::tempdir().unwrap(), |_| {});
    let c = &r.core;
    let s = session(c);
    r.model.hold_tasks.store(true, SeqCst);
    let t = a_task(&r, &s, "Chart the outer harbour", CENSUS).await;
    ask_to_abandon(&r, &s, &t).await;
    assert_eq!(c.confirm_list().unwrap().len(), 1);

    let retire = |on: bool| {
        c.store
            .update_session(&s, |rec: &mut SessionRecord| {
                rec.retired = on.then(|| theseus_protocol::sessions::SessionRetired {
                    reason: theseus_protocol::sessions::RetiredReason::ByHand,
                    at_ms: theseus_protocol::now_unix_ms(),
                });
                Ok(vec![])
            })
            .unwrap();
    };
    let before = r.task(&t.id).state;
    assert_ne!(before, TaskState::Abandoned);
    retire(true);
    let gone = c.expire_questions(theseus_protocol::now_unix_ms() + 86_400_000);
    retire(false);
    assert_eq!(gone, 1, "the question did not expire");
    assert!(c.confirm_list().unwrap().is_empty());

    say(&r, &s, &format!("yes, abandon {}", t.id)).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(r.task(&t.id).state, before, "the expired abandon ran");
    assert!(rows(c, "task.change_accepted").is_empty());
    let said = notes(&r);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("no longer waiting"), "{said:?}");
    assert!(said[0].contains("expired"), "{said:?}");
    r.model.hold_tasks.store(false, SeqCst);
}

/// A yes said in another conversation is not an answer to this one's
/// question: `turn.submit` reads only what waits in the session the words
/// were said in.
#[tokio::test]
async fn a_yes_in_another_conversation_does_not_count() {
    let r = rig_on(tempfile::tempdir().unwrap(), |_| {});
    let c = &r.core;
    let asking = session(c);
    r.model.hold_tasks.store(true, SeqCst);
    let t = a_task(&r, &asking, "Chart the outer harbour", CENSUS).await;
    ask_to_abandon(&r, &asking, &t).await;
    assert_eq!(c.confirm_list().unwrap().len(), 1);
    let before = r.task(&t.id).state;
    assert_ne!(before, TaskState::Abandoned);

    let elsewhere = {
        let rec = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
        c.store.put_session(&rec.session_id, &rec).unwrap();
        c.outbox.bind_place("dm:43", &rec.session_id).unwrap();
        rec.session_id
    };
    say(&r, &elsewhere, "Yes, you may abandon it").await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(
        c.confirm_list().unwrap().len(),
        1,
        "a yes in another conversation answered this one's question"
    );
    assert_eq!(r.task(&t.id).state, before);
    assert!(rows(c, "task.change_accepted").is_empty());
    assert!(
        notes(&r).is_empty(),
        "a bare yes in another conversation said something"
    );
    r.model.hold_tasks.store(false, SeqCst);
}

/// (B6, theseus-2yowd) The residue theseus-3i6d1 left open: the turn an
/// ambiguous yes starts now carries a harness fact naming what was and was
/// not approved, so the model is told what the mechanism could not guess.
///
/// Observable, and both halves are asserted: the fact is in the request of
/// **that** turn, and in no later turn's. A positive count goes with the
/// negative one (proof rule 3) — the block names both waiting questions, so
/// an empty set could not pass this.
#[tokio::test]
async fn an_ambiguous_yes_tells_the_turn_it_starts_what_was_not_approved() {
    let r = rig_on(tempfile::tempdir().unwrap(), |cfg| {
        cfg.standing.yes_fact = true;
    });
    let c = &r.core;
    let s = session(c);
    r.model.hold_tasks.store(true, SeqCst);
    // Two questions waiting at this place, as the sitting's two were: a
    // conversation holds at most one of its own, because the question holds
    // its turn (the note on the test above).
    let (sa, sb) = (beside(c, &s), beside(c, &s));
    let census = a_task(&r, &sa, "Chart the outer harbour", CENSUS).await;
    ask_to_abandon(&r, &sa, &census).await;
    let other = a_task(&r, &sb, "Sound the inner channel", OTHER).await;
    ask_to_abandon(&r, &sb, &other).await;
    assert_eq!(c.confirm_list().unwrap().len(), 2);

    say(&r, &s, "Yes, you may abandon it").await;

    // The fact rode exactly one request, and it is the one this turn made.
    let carried = facts(&r);
    assert_eq!(carried.len(), 1, "{carried:?}");
    let fact = &carried[0];
    assert!(fact.contains("Approved: nothing."), "{fact}");
    assert!(
        fact.contains("more than one question waiting here could be the one it meant"),
        "{fact}"
    );
    // The positive half: both questions are named, by the task each would
    // abandon, so the block cannot be empty and pass.
    assert!(
        fact.contains("NOT approved, and still waiting (2):"),
        "{fact}"
    );
    for t in [&census, &other] {
        assert!(fact.contains(&t.title), "{} is not named: {fact}", t.title);
    }
    assert!(
        fact.contains("Do not act as though any of them were approved"),
        "{fact}"
    );
    // And nothing ran: the fact is a telling, not a new way to approve.
    assert_eq!(c.confirm_list().unwrap().len(), 2);
    assert!(rows(c, "action.confirm_answered").is_empty());

    // The next turn in the same conversation does not carry it.
    say(&r, &s, "what is waiting?").await;
    assert_eq!(facts(&r).len(), 1, "the fact rode a second turn: {:?}", facts(&r));
    r.model.hold_tasks.store(false, SeqCst);
}

/// The same ambiguous yes with `[standing] yes_fact` at its default (off)
/// tells the turn nothing: the notice where approvals go is all that is said,
/// exactly as before this row. The negative is paired with the notice's
/// positive count, so a run in which nothing happened at all cannot pass.
#[tokio::test]
async fn the_turn_is_told_nothing_while_the_key_is_off() {
    let r = rig_on(tempfile::tempdir().unwrap(), |_| {});
    let c = &r.core;
    assert!(!c.cfg.standing.yes_fact, "the key's default is off");
    let s = session(c);
    r.model.hold_tasks.store(true, SeqCst);
    let (sa, sb) = (beside(c, &s), beside(c, &s));
    let census = a_task(&r, &sa, "Chart the outer harbour", CENSUS).await;
    ask_to_abandon(&r, &sa, &census).await;
    let other = a_task(&r, &sb, "Sound the inner channel", OTHER).await;
    ask_to_abandon(&r, &sb, &other).await;

    say(&r, &s, "Yes, you may abandon it").await;

    assert!(facts(&r).is_empty(), "{:?}", facts(&r));
    // The notice still went where approvals go, so this is the key's doing.
    let said = notes(&r);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("Which one do you mean?"), "{said:?}");
    assert_ne!(r.task(&other.id).state, TaskState::Abandoned);
    assert_ne!(r.task(&census.id).state, TaskState::Abandoned);
    r.model.hold_tasks.store(false, SeqCst);
}

/// A sibling session that answers at `sid`'s place, as a task's own asks do.
fn beside(c: &std::sync::Arc<crate::Core>, sid: &str) -> String {
    let rec = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    c.store.put_session(&rec.session_id, &rec).unwrap();
    c.outbox
        .show_at(&rec.session_id, c.outbox.target(sid).as_deref())
        .unwrap();
    assert_eq!(c.outbox.target(&rec.session_id), c.outbox.target(sid));
    rec.session_id
}

/// Every model request that carried the turn's yes fact, in order: the block
/// is found by its own head, so nothing else in a request can pass for it.
fn facts(r: &Rig) -> Vec<String> {
    r.model
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter_map(|q| {
            q.messages.last()?["content"]
                .as_array()?
                .iter()
                .filter_map(|b| b["text"].as_str())
                .find(|t| t.starts_with(crate::yes_fact::HEAD))
                .map(str::to_string)
        })
        .collect()
}
