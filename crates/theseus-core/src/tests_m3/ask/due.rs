//! A due question's card always reaches the owner's DM (theseus-27p6w): the
//! pacer's window paces the buzz, never the card. A child of `ask`.

use super::*;

/// The window a post of five minutes ago closed, and a question that falls due
/// inside it: its card posts on the tick it is due, silent, with the window
/// untouched; with the window open it pings and takes it.
#[tokio::test]
async fn a_due_question_posts_its_card_whatever_the_window_and_only_an_open_one_buzzes() {
    let r = owned_rig(write_script(), false);
    let res = turn(&r.core, None, "write out.txt").await;
    let corr = res.awaiting_confirm.unwrap();
    let t0 = r.core.kernel.action(&corr).unwrap().unwrap().planned_at_ms;
    let due = t0 + 61_000;
    r.core.outbox.pacer.took(due - 5 * 60_000);
    let took = r.core.outbox.pacer.last_ms();
    assert_eq!(r.core.escalate_questions(due), 1, "carded within the tick");
    let posts = needs_you_posts(&r);
    assert_eq!(posts.len(), 1);
    let body = crate::outbox::body_of(&posts[0]);
    assert_eq!(body["question"], corr.as_str());
    assert_eq!(body["silent"], true, "the window was closed: no buzz");
    assert_eq!(body["card"]["kind"], "approval", "{body}");
    assert!(body["text"].as_str().unwrap().contains("approve"), "{body}");
    assert_eq!(
        r.core.outbox.pacer.last_ms(),
        took,
        "a silent card takes none"
    );
    assert_eq!(
        r.core.escalate_questions(due + 60_000),
        0,
        "a post a question"
    );
    assert_eq!(needs_you_posts(&r).len(), 1);
    // A restart reads the same index: still carded, window still untouched.
    let again = crate::outbox::Outbox::new(r.core.store.clone(), r.core.kernel.clone());
    assert!(again.pinged(&corr), "the reread knows it posted");
    assert_eq!(again.pacer.last_ms(), 0, "and no window a silent card took");

    // With the window open the card pings and takes it.
    let r = owned_rig(write_script(), false);
    let res = turn(&r.core, None, "write out.txt").await;
    let corr = res.awaiting_confirm.unwrap();
    let t0 = r.core.kernel.action(&corr).unwrap().unwrap().planned_at_ms;
    assert_eq!(r.core.escalate_questions(t0 + 61_000), 1);
    let body = crate::outbox::body_of(&needs_you_posts(&r)[0]).clone();
    assert!(body["silent"].is_null(), "it pings: {body}");
    assert_ne!(r.core.outbox.pacer.last_ms(), 0, "and takes the window");
}

/// With `dm_cards` off the one-line post follows the same rule, and is
/// otherwise today's body.
#[tokio::test]
async fn with_dm_cards_off_the_one_line_post_is_on_time_and_silent_when_closed() {
    let r = owned_tweaked(write_script(), |c| c.notify.dm_cards = false);
    let res = turn(&r.core, None, "write out.txt").await;
    let corr = res.awaiting_confirm.unwrap();
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    let due = a.planned_at_ms + 61_000;
    r.core.outbox.pacer.took(due - 5 * 60_000);
    let took = r.core.outbox.pacer.last_ms();
    assert_eq!(r.core.escalate_questions(due), 1);
    // The request is read as the poster reads it (`pending_confirms`), not
    // from `confirm_list`: the list shows a held call's deadline as none,
    // while the post, whose card footer reads the time, keeps the real one.
    let req = r.core.pending_confirms(&res.session_id).unwrap().remove(0);
    let who = r.core.asker_name(&req);
    let mut want = crate::needs_you::body_of(&a, &req, &who);
    want["silent"] = json!(true);
    let posted = crate::outbox::body_of(&needs_you_posts(&r)[0]).clone();
    assert_eq!(posted, want, "today's body, silent");
    assert_eq!(r.core.outbox.pacer.last_ms(), took);
}

fn twice() -> Vec<Scripted> {
    let mut s = write_script();
    s.extend(write_script());
    s
}

/// The settle written for a question, if any.
fn settle_of(r: &Rig, corr: &str) -> Option<Value> {
    settles(r)
        .iter()
        .map(crate::outbox::body_of)
        .find(|b| b["question"] == corr)
        .cloned()
}

/// An approval nobody answered expires: its settled card offers "Ask again"
/// while its session can take a turn. Anyone but the owner's press is refused;
/// the owner's sets a wake due now whose note says what was asked, and the
/// question the session then plans is carded at once, pinging, whatever the
/// wait and the window say.
#[tokio::test]
async fn an_expired_approval_offers_ask_again_and_its_next_question_is_carded_at_once() {
    let r = owned_rig(twice(), false);
    let res = turn(&r.core, None, "write out.txt").await;
    let (sid, exec, corr) = (
        res.session_id.clone(),
        res.execution_id.clone().unwrap(),
        res.awaiting_confirm.clone().unwrap(),
    );
    let t0 = r.core.kernel.action(&corr).unwrap().unwrap().planned_at_ms;
    assert_eq!(r.core.escalate_questions(t0 + 61_000), 1);
    // A call's question no longer expires (theseus-v2dnc): it is held. What
    // can still be asked again is a question an older build expired, so one is
    // made as that build made it: declined by expiry, its execution woken, its
    // card settled `expired`.
    assert_eq!(r.core.expire_questions(t0 + 20 * 60_000), 0, "held");
    assert!(r
        .core
        .kernel
        .action(&corr)
        .unwrap()
        .unwrap()
        .awaits_confirm());
    r.core
        .kernel
        .frame(&[&exec], |k| {
            k.decline_action(&corr, "expiry", "nobody answered within 15 minutes")?;
            let _ = k.frame(&[&exec], |k| k.wake(&exec, "expired"));
            Ok(())
        })
        .unwrap();
    r.core
        .card_closed(&corr, crate::outbox::Closed::new("expired", None));
    let settle = settle_of(&r, &corr).expect("the card settles");
    assert_eq!(settle["closed"]["how"], "expired", "{settle}");
    assert_eq!(
        settle["again"], true,
        "its session can take a turn: {settle}"
    );
    r.core.continue_execution(&exec).await.unwrap();

    let guest = rpc_as(
        &r.core,
        surface("sock#2", Mcp),
        theseus_protocol::method::WORK_ANSWER,
        json!({"question_id": corr, "option": "ask_again"}),
    )
    .await;
    assert_eq!(guest.unwrap_err().code, error_code::REFUSED);
    assert!(r
        .core
        .kernel
        .execution(&exec)
        .unwrap()
        .unwrap()
        .wakes
        .is_empty());

    let done = answer(&r, &corr, "ask_again", None).await.unwrap();
    assert_eq!(done["detail"], "asked again");
    let wakes = r.core.kernel.execution(&exec).unwrap().unwrap().wakes;
    assert_eq!(wakes.len(), 1, "{wakes:?}");
    assert!(
        wakes[0]
            .note
            .starts_with("The owner asked again for: write `"),
        "{}",
        wakes[0].note
    );
    let again = answer(&r, &corr, "ask_again", None).await.unwrap();
    assert_eq!(again["detail"], "already asked again", "once");

    // The wake's turn, run by hand: the model plans the call again.
    let res = turn(&r.core, Some(&sid), "again").await;
    let corr2 = res.awaiting_confirm.unwrap();
    let planned = r.core.kernel.action(&corr2).unwrap().unwrap().planned_at_ms;
    assert!(r.core.outbox.pacer.last_ms() > 0, "the window is closed");
    assert_eq!(r.core.escalate_questions(planned), 1, "carded at once");
    let body = needs_you_posts(&r)
        .iter()
        .map(crate::outbox::body_of)
        .find(|b| b["question"] == corr2.as_str())
        .cloned()
        .expect("its card");
    assert!(body["silent"].is_null(), "prompted, so it pings: {body}");
}

/// An expired question whose session cannot take a turn has no button; and
/// only a question that expired can be asked again.
#[tokio::test]
async fn an_expired_card_of_an_ended_session_has_no_button_and_an_answered_one_cannot_ask_again() {
    let r = owned_rig(twice(), false);
    let res = turn(&r.core, None, "write out.txt").await;
    let (exec, corr) = (res.execution_id.unwrap(), res.awaiting_confirm.unwrap());
    let t0 = r.core.kernel.action(&corr).unwrap().unwrap().planned_at_ms;
    assert_eq!(r.core.escalate_questions(t0 + 61_000), 1);
    r.core.kernel.cancel_execution(&exec, "test").unwrap();
    r.core
        .outbox
        .closed(&corr, crate::outbox::Closed::new("expired", None))
        .unwrap();
    assert_eq!(settle_of(&r, &corr).unwrap()["again"], false);

    let r = owned_rig(twice(), false);
    let res = turn(&r.core, None, "write out.txt").await;
    let corr = res.awaiting_confirm.unwrap();
    answer(&r, &corr, "approve", None).await.unwrap();
    let e = answer(&r, &corr, "ask_again", None).await.unwrap_err();
    assert!(e.message.contains("expired"), "{}", e.message);
}
