//! The task commands end to end (theseus-a5uds): a real core with the work
//! engine, the binding on the stand-ins' REST and gateway, and the stand-in's
//! command, autocomplete, select and modal drivers. Each test names a
//! behaviour: /tasks, /task's autocomplete (ranked, from memory), Details with
//! Stop and Follow, /answer with a select and with a modal (a note, a file),
//! a refused press, New task and Make this a task, /quiet, and a failed
//! task's Retry and Dismiss.

use super::*;
use theseus_protocol::LedgerKind;
use theseus_sim::fake_discord::commands::{Sender, Upload};
use theseus_sim::fake_discord::Reply;

const PICK_SITE: &str = "Pick-site: pick a survey site";
const SURVEY: &str = "Survey-north: survey the north pier";
const FAILS: &str = "Fail-chart: chart that fails";

pub(super) fn ana() -> Sender<'static> {
    Sender {
        user: ANA,
        name: "ana",
        channel: None,
    }
}

/// Another person, whose message the owner makes a task of.
fn ben() -> Sender<'static> {
    Sender {
        user: BEN,
        name: "ben",
        channel: None,
    }
}

fn cy_in_lab() -> Sender<'static> {
    Sender {
        user: CY,
        name: "cy",
        channel: Some(LAB),
    }
}

/// What the binding answered the interaction with first.
pub(super) fn first_reply(r: &Rig, iid: &str) -> Option<Reply> {
    r.fake
        .replies()
        .into_iter()
        .find(|x| x.interaction.as_deref() == Some(iid) && x.kind == "callback")
}

pub(super) async fn answered(r: &Rig, iid: &str) -> Reply {
    r.until("the binding answers", || first_reply(r, iid).is_some())
        .await;
    first_reply(r, iid).unwrap()
}

/// Every text the binding said to the interaction, in order.
pub(super) fn said(r: &Rig, iid: &str) -> String {
    r.fake
        .replies()
        .into_iter()
        .filter(|x| x.interaction.as_deref() == Some(iid))
        .filter_map(|x| x.content)
        .collect::<Vec<_>>()
        .join("\n")
}

fn tweak_ask(c: &mut theseus_core::Config) {
    tweak(c);
    // The model's owner_ask is deferred by default: these scripts call it directly.
    c.tools.defer.always_loaded.push("ask".into());
}

/// A task that asks which of six sites, so the answers are seven (six and
/// something else), and one that runs for a while.
async fn asking_six() -> Rig {
    let ask = json!({
        "question": "Which survey site?",
        "options": [{"label": "North pier"}, {"label": "South pier"}, {"label": "The reef"},
                    {"label": "The wreck"}, {"label": "The shoal"}, {"label": "The lighthouse"}],
        "default": 1,
    });
    let r = Rig::start_tweaked(
        move |_, _| {
            model(
                vec![
                    start_all(&[PICK_SITE, BUOY]),
                    Scripted::text("Started both."),
                ],
                vec![
                    (
                        PICK_SITE,
                        vec![
                            Scripted::tools("", &[("a1", "owner_ask", ask)]),
                            Scripted::text("Going on."),
                        ],
                    ),
                    (BUOY, sleeps(30)),
                ],
            )
        },
        guild(),
        &bindings(),
        &[],
        tweak_ask,
    )
    .await;
    r.say((ANA, "ana"), None, ASK);
    r.until("the ask is waiting", || {
        r.core.confirm_list().is_ok_and(|l| l.len() == 1)
    })
    .await;
    r
}

/// The waiting question's id.
fn waiting_id(r: &Rig) -> String {
    r.core.confirm_list().unwrap().remove(0).correlation_id
}

fn short6(id: &str) -> String {
    board_v2::short(id).to_string()
}

#[tokio::test]
async fn tasks_shows_this_places_snapshot_to_the_presser_alone_and_refresh_draws_it_again() {
    let r = one_asking_one_running().await;
    let iid = r
        .fake
        .slash(&ana(), "tasks", &[("show", "working")])
        .unwrap();
    r.until("the snapshot", || r.fake.reply_message(&iid).is_some())
        .await;
    let m = r.fake.reply_message(&iid).unwrap();
    assert!(
        m.flags & 64 != 0 && m.flags & IS_COMPONENTS_V2 != 0,
        "ephemeral V2"
    );
    assert!(m.content.contains("Buoy-paint"), "{}", m.content);
    assert!(
        !m.content.contains("Reef-chart"),
        "working only: {}",
        m.content
    );
    assert!(m
        .buttons
        .iter()
        .any(|b| b.custom_id == "tb:refresh:working"));
    // The default is the board (open).
    let all = r.fake.slash(&ana(), "tasks", &[]).unwrap();
    r.until("all of it", || r.fake.reply_message(&all).is_some())
        .await;
    let c = r.fake.reply_message(&all).unwrap().content;
    assert!(c.contains("Buoy-paint") && c.contains("Reef-chart"), "{c}");
    // ↻ draws it again, in the message it is on.
    let again = r
        .fake
        .press_reply(&iid, "tb:refresh:working", ANA, "ana")
        .unwrap();
    let rep = answered(&r, &again).await;
    assert_eq!(rep.response_type, Some(7), "an edit in place: {rep:?}");
    // Nothing was posted in the channel for any of it.
    assert!(r
        .posted(ANA_DM)
        .iter()
        .all(|m| !m.content.contains("Tasks here")));
}

/// theseus-ypnxt.5: a finished task is on `/tasks show:done` and `show:all`
/// from the store, with its title, how it ended, what it cost, the first line
/// of its report and when, after the board has cleared it; `show:open` (the
/// default, once `all`) is the board from memory and does not have it.
#[tokio::test]
async fn tasks_done_and_all_read_the_finished_tasks_from_the_store_after_the_board_forgot_them() {
    let r = Rig::start_tweaked(
        |_, _| {
            model(
                vec![start_all(&[BUOY]), Scripted::text("Started it.")],
                vec![(BUOY, sleeps(1))],
            )
        },
        guild(),
        &bindings(),
        &[],
        tweak,
    )
    .await;
    r.say((ANA, "ana"), None, ASK);
    r.until("the task done and not yet seen", || {
        boards(&r, ANA_DM)
            .first()
            .is_some_and(|b| b.content.contains("✅ 1 done since you looked"))
    })
    .await;
    // The board is cleared: its memory of the task goes with it.
    let board = boards(&r, ANA_DM).remove(0);
    r.press(&board.id, "✓ Clear done", (ANA, "ana"));
    r.until("the board says all clear", || {
        boards(&r, ANA_DM)
            .first()
            .is_some_and(|b| b.content.contains("all clear"))
    })
    .await;
    async fn shown(r: &Rig, show: &[(&str, &str)]) -> String {
        let iid = r.fake.slash(&ana(), "tasks", show).unwrap();
        r.until("the answer", || r.fake.reply_message(&iid).is_some())
            .await;
        r.fake.reply_message(&iid).unwrap().content
    }
    // open (the default) is the board: nothing, as it was.
    let open = shown(&r, &[]).await;
    assert!(!open.contains("Buoy-paint"), "{open}");
    let open = shown(&r, &[("show", "open")]).await;
    assert!(!open.contains("Buoy-paint"), "{open}");
    // done and all read the store.
    for show in ["done", "all"] {
        let c = shown(&r, &[("show", show)]).await;
        assert!(
            c.contains("Buoy-paint") && c.contains("done <t:"),
            "{show}: {c}"
        );
        assert!(c.contains("1 finished"), "{show}: {c}");
        assert!(c.contains('$'), "what it cost: {c}");
    }
    // The report's first line is there.
    let c = shown(&r, &[("show", "done")]).await;
    assert!(c.contains("Done."), "{c}");
    // The refresh button keeps the show.
    let iid = r.fake.slash(&ana(), "tasks", &[("show", "done")]).unwrap();
    r.until("the answer", || r.fake.reply_message(&iid).is_some())
        .await;
    assert!(r
        .fake
        .reply_message(&iid)
        .unwrap()
        .buttons
        .iter()
        .any(|b| b.custom_id == "tb:refresh:done"));
}

#[tokio::test]
async fn task_autocomplete_ranks_needs_you_first_stops_at_25_and_asks_the_daemon_nothing() {
    let r = one_asking_one_running().await;
    // The board has settled; from here a keystroke may not reach the daemon.
    tokio::time::sleep(Duration::from_millis(400)).await;
    let before = crate::rpc_client::calls().len();
    let mut ms = Vec::new();
    let mut first = None;
    for typed in ["", "b", "bu", "buo", "buoy"] {
        // The work a keystroke costs, in CPU time: a loaded machine makes the
        // wall clock say how long the binding waited for a turn on the CPU,
        // and the 25 ms poll below is on it too (theseus-1b2zm).
        let t0 = theseus_sim::process_cpu();
        let iid = r
            .fake
            .autocomplete_typed(&ana(), "task", "name", typed)
            .unwrap();
        let rep = answered(&r, &iid).await;
        ms.push((theseus_sim::process_cpu() - t0).as_millis());
        if typed.is_empty() {
            first = Some(rep);
        } else if typed == "buoy" {
            assert_eq!(rep.choices.len(), 1, "{:?}", rep.choices);
            assert!(rep.choices[0].0.contains("Buoy-paint"), "{:?}", rep.choices);
        }
    }
    let rep = first.unwrap();
    assert_eq!(rep.response_type, Some(8));
    assert_eq!(rep.choices[0].1, "new", "{:?}", rep.choices);
    assert!(
        rep.choices[1].0.contains("Reef-chart"),
        "needs you first: {:?}",
        rep.choices
    );
    assert!(rep.choices[2].0.contains("Buoy-paint"), "{:?}", rep.choices);
    assert!(rep.choices.len() <= 25);
    let asked: Vec<String> = crate::rpc_client::calls()[before..]
        .iter()
        .filter(|m| {
            [
                "work.watch",
                "confirm.list",
                "work.answer",
                "session.list",
                "task.list",
            ]
            .contains(&m.as_str())
        })
        .cloned()
        .collect();
    assert!(
        asked.is_empty(),
        "five keystrokes asked the daemon: {asked:?}"
    );
    ms.sort_unstable();
    assert!(
        ms[2] < 50,
        "the median keystroke took {}ms of CPU (all {ms:?})",
        ms[2]
    );
}

#[tokio::test]
async fn cancel_autocomplete_lists_the_places_tasks_by_short_id() {
    let r = one_asking_one_running().await;
    let board = boards(&r, ANA_DM).remove(0);
    let (_, running) = ids_of(&board);
    tokio::time::sleep(Duration::from_millis(400)).await;
    let iid = r
        .fake
        .autocomplete_typed(&ana(), "cancel", "id", "")
        .unwrap();
    let rep = answered(&r, &iid).await;
    assert!(
        rep.choices
            .iter()
            .any(|c| c.1 == short6(&running) && c.0.contains("Buoy-paint")),
        "{:?}",
        rep.choices
    );
}

#[tokio::test]
async fn details_stop_needs_the_second_press_and_follow_mirrors_and_seals() {
    let r = one_asking_one_running().await;
    let board = boards(&r, ANA_DM).remove(0);
    let (_, running) = ids_of(&board);
    let iid = r.fake.slash(&ana(), "task", &[("name", &running)]).unwrap();
    r.until("details", || r.fake.reply_message(&iid).is_some())
        .await;
    let view = r.fake.reply_message(&iid).unwrap();
    assert!(view.content.contains("Buoy-paint"), "{}", view.content);
    let ids: Vec<&str> = view.buttons.iter().map(|b| b.custom_id.as_str()).collect();
    assert!(
        ids.contains(&format!("tb:stop:{running}").as_str()),
        "{ids:?}"
    );
    assert!(
        ids.contains(&format!("tb:follow:{running}").as_str()),
        "{ids:?}"
    );
    // Follow: one silent message in the place.
    let f = r
        .fake
        .press_reply(&iid, &format!("tb:follow:{running}"), ANA, "ana")
        .unwrap();
    r.until("the follow is accepted", || {
        said(&r, &f).contains("Following")
    })
    .await;
    let mirror = |r: &Rig| {
        r.posted(ANA_DM)
            .into_iter()
            .find(|m| m.content.contains("Buoy-paint") && m.flags & IS_COMPONENTS_V2 == 0)
    };
    r.until("the mirror message", || mirror(&r).is_some()).await;
    assert!(mirror(&r).unwrap().silent(), "a follow never pings");
    // Following the same task twice is said, not done.
    let again = r
        .fake
        .press_reply(&iid, &format!("tb:follow:{running}"), ANA, "ana")
        .unwrap();
    r.until("already following", || said(&r, &again).contains("already"))
        .await;
    // Stop asks once more, and the second press stops the task.
    let stop = r
        .fake
        .press_reply(&iid, &format!("tb:stop:{running}"), ANA, "ana")
        .unwrap();
    r.until("the second question", || {
        r.fake
            .reply_message(&stop)
            .is_some_and(|m| m.content.contains("Stop **Buoy-paint"))
    })
    .await;
    let state = |exe: &str| r.core.kernel.execution(exe).unwrap().unwrap().state;
    assert_ne!(state(&running).as_str(), "cancelled");
    let labels: Vec<String> = r
        .fake
        .reply_message(&stop)
        .unwrap()
        .buttons
        .into_iter()
        .map(|b| b.label)
        .collect();
    assert_eq!(
        labels,
        [format!("Stop {}", short6(&running)), "Keep it".to_string()]
    );
    r.fake
        .press_reply(&stop, &format!("tb:yes:{running}"), ANA, "ana")
        .unwrap();
    r.until("the task cancelled", || {
        state(&running).as_str() == "cancelled"
    })
    .await;
    r.until("the mirror sealed with its report line", || {
        mirror(&r).is_some_and(|m| m.content.contains("🏁") && m.content.contains("cancelled"))
    })
    .await;
    // Sealed: no more edits.
    let edits = mirror(&r).unwrap().edits;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(mirror(&r).unwrap().edits, edits);
}

/// A reply to a Follow mirror is the owner's word to the task, even while it
/// runs; a message that is not a reply is the conversation's; and once the task
/// has ended a reply goes to the conversation with the shared line
/// (theseus-esxyb).
#[tokio::test]
async fn a_reply_to_the_follow_mirror_joins_the_task_and_an_ended_ones_goes_to_the_conversation() {
    let r = one_asking_one_running().await;
    let board = boards(&r, ANA_DM).remove(0);
    let (running, _) = ids_of(&board);
    let iid = r.fake.slash(&ana(), "task", &[("name", &running)]).unwrap();
    r.until("details", || r.fake.reply_message(&iid).is_some())
        .await;
    r.fake
        .press_reply(&iid, &format!("tb:follow:{running}"), ANA, "ana")
        .unwrap();
    let mirror = |r: &Rig| {
        r.posted(ANA_DM)
            .into_iter()
            .find(|m| m.content.contains("Reef-chart") && m.flags & IS_COMPONENTS_V2 == 0)
    };
    r.until("the mirror message", || mirror(&r).is_some()).await;
    let mirror_id = mirror(&r).unwrap().id;
    assert!(
        mirror(&r)
            .unwrap()
            .content
            .contains("reply here to talk to the task"),
        "{}",
        mirror(&r).unwrap().content
    );
    let task = r
        .core
        .kernel
        .execution(&running)
        .unwrap()
        .unwrap()
        .session_id
        .to_string();
    let home = r
        .core
        .outbox
        .place_session(&format!("dm:{ANA}"))
        .unwrap()
        .expect("the DM's session");
    let held = |sid: &str| -> Vec<String> {
        r.core
            .store
            .session_nodes(sid)
            .unwrap()
            .into_iter()
            .map(|(_, n)| format!("{:?}", n.body))
            .collect()
    };
    let typed = |content| theseus_sim::fake_discord::Typed {
        user: ANA,
        name: "ana",
        channel: None,
        content,
        file: None,
    };
    r.fake
        .say_reply(&typed("paint it blue instead"), &mirror_id)
        .unwrap();
    r.until("the reply reached the task", || {
        held(&task)
            .iter()
            .any(|b| b.contains("paint it blue instead"))
    })
    .await;
    assert!(
        !held(&home)
            .iter()
            .any(|b| b.contains("paint it blue instead")),
        "the conversation did not hear what the task took"
    );
    // Not a reply: the conversation's, as ever.
    r.fake.say(&typed("and what of the reef")).unwrap();
    r.until("a plain message is the conversation's", || {
        held(&home)
            .iter()
            .any(|b| b.contains("and what of the reef"))
    })
    .await;
    assert!(!held(&task)
        .iter()
        .any(|b| b.contains("and what of the reef")));
    // The task ends; a reply to its sealed mirror goes to the conversation.
    r.core
        .task_cancel_by(&task, "discord:operator")
        .await
        .unwrap();
    r.until("the mirror sealed", || {
        mirror(&r).is_some_and(|m| m.content.contains("🏁"))
    })
    .await;
    r.fake
        .say_reply(&typed("one last thing for the buoy"), &mirror_id)
        .unwrap();
    r.until("the conversation heard it, with the shared line", || {
        held(&home).iter().any(|b| {
            b.contains("one last thing for the buoy")
                && b.contains("[Written to the task")
                && b.contains("and the task is over]")
        })
    })
    .await;
}

#[tokio::test]
async fn answer_resolves_a_seven_answer_ask_through_its_select_and_settles_the_card_everywhere() {
    let r = asking_six().await;
    let q = waiting_id(&r);
    let iid = r.fake.slash(&ana(), "answer", &[]).unwrap();
    r.until("the card", || r.fake.reply_message(&iid).is_some())
        .await;
    let view = r.fake.reply_message(&iid).unwrap();
    assert!(
        view.content.contains("Which survey site?"),
        "{}",
        view.content
    );
    assert!(
        view.content.contains("**6.** The lighthouse"),
        "{}",
        view.content
    );
    let sel = format!("ws:{}", short6(&q));
    let ids: Vec<&str> = view.buttons.iter().map(|b| b.custom_id.as_str()).collect();
    assert_eq!(
        ids,
        [sel.as_str()],
        "seven answers are one select, not seven buttons"
    );
    let pick = r
        .fake
        .pick_in_reply(&iid, &sel, &["choice:2"], ANA, "ana")
        .unwrap();
    r.until("the answer", || {
        r.ledger("action.confirm_answered")
            .iter()
            .any(|row| row["option"] == "choice:2")
    })
    .await;
    assert_eq!(r.core.confirm_list().unwrap().len(), 0);
    // The view the select was on closes, saying it was answered.
    r.until("the view is closed", || {
        said(&r, &pick).contains("Answered")
    })
    .await;
    // The needs-you card that waited in the DM is settled too.
    r.until("every card settled", || {
        let cards: Vec<Msg> = r
            .posted(ANA_DM)
            .into_iter()
            .filter(|m| m.content.contains("Which survey site?"))
            .collect();
        !cards.is_empty()
            && cards
                .iter()
                .all(|m| m.buttons.is_empty() && m.content.contains("Answered"))
    })
    .await;
}

#[tokio::test]
async fn an_asks_own_words_open_a_modal_whose_note_and_file_are_the_answer() {
    let r = asking_six().await;
    let q = waiting_id(&r);
    let iid = r.fake.slash(&ana(), "answer", &[("question", &q)]).unwrap();
    r.until("the card", || r.fake.reply_message(&iid).is_some())
        .await;
    let sel = format!("ws:{}", short6(&q));
    let pick = r
        .fake
        .pick_in_reply(&iid, &sel, &["choice:other"], ANA, "ana")
        .unwrap();
    let rep = answered(&r, &pick).await;
    let modal = rep.modal.expect("a modal opened");
    assert_eq!(rep.response_type, Some(9));
    let custom = format!("wm:{}:choice:other", short6(&q));
    assert_eq!(modal["custom_id"], custom.as_str());
    let kinds: Vec<u64> = modal["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["component"]["type"].as_u64().unwrap())
        .collect();
    assert_eq!(kinds, [4, 19], "a text input and a file upload");
    // Submit: words and a text file.
    let sub = r
        .fake
        .submit_modal(
            &ana(),
            &custom,
            &[("note", "the site by the old lighthouse")],
            &[],
            Some(&Upload {
                field: "file",
                name: "map.txt",
                bytes: b"lat 12.3 lon 45.6",
                content_type: "text/plain",
            }),
        )
        .unwrap();
    r.until("answered", || said(&r, &sub).contains("Answered"))
        .await;
    let row = r
        .ledger("action.confirm_answered")
        .into_iter()
        .find(|row| row["option"] == "choice:other")
        .expect("the answer is on the record");
    let note = row.to_string();
    assert!(
        note.contains("old lighthouse") && note.contains("lat 12.3 lon 45.6"),
        "{note}"
    );
    assert_eq!(r.core.confirm_list().unwrap().len(), 0);
}

#[tokio::test]
async fn decline_with_a_note_opens_a_modal_and_the_note_goes_with_the_decline() {
    let r = one_asking_one_running().await;
    let q = waiting_id(&r);
    let iid = r.fake.slash(&ana(), "answer", &[]).unwrap();
    r.until("the card", || r.fake.reply_message(&iid).is_some())
        .await;
    let view = r.fake.reply_message(&iid).unwrap();
    let ids: Vec<&str> = view.buttons.iter().map(|b| b.custom_id.as_str()).collect();
    let note_button = format!("wn:{}:decline", short6(&q));
    assert!(ids.contains(&note_button.as_str()), "{ids:?}");
    let press = r.fake.press_reply(&iid, &note_button, ANA, "ana").unwrap();
    let rep = answered(&r, &press).await;
    assert_eq!(rep.response_type, Some(9));
    let custom = format!("wm:{}:decline", short6(&q));
    assert_eq!(rep.modal.unwrap()["custom_id"], custom.as_str());
    r.fake
        .submit_modal(
            &ana(),
            &custom,
            &[("note", "not outside the roots")],
            &[],
            None,
        )
        .unwrap();
    r.until("declined", || {
        r.ledger("action.confirm_answered")
            .iter()
            .any(|row| row.to_string().contains("not outside the roots"))
    })
    .await;
    assert_eq!(r.core.confirm_list().unwrap().len(), 0);
}

#[tokio::test]
async fn answer_autocomplete_lists_what_the_presser_may_answer_and_reads_the_daemon_once() {
    let r = one_asking_one_running().await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    let before = crate::rpc_client::calls().len();
    let mut last = None;
    for typed in ["", "r", "re", "ree"] {
        let iid = r
            .fake
            .autocomplete_typed(&ana(), "answer", "question", typed)
            .unwrap();
        last = Some(answered(&r, &iid).await);
    }
    let rep = last.unwrap();
    assert_eq!(rep.choices.len(), 1, "{:?}", rep.choices);
    assert_eq!(rep.choices[0].1, waiting_id(&r));
    let fetches = crate::rpc_client::calls()[before..]
        .iter()
        .filter(|m| *m == "confirm.list")
        .count();
    assert_eq!(fetches, 1, "four keystrokes, one read of the daemon");
    // A person who may not answer here is offered nothing.
    let cy = r
        .fake
        .autocomplete_typed(&cy_in_lab(), "answer", "question", "")
        .unwrap();
    assert!(answered(&r, &cy).await.choices.is_empty());
}

#[tokio::test]
async fn a_modal_answer_from_someone_who_may_not_counts_for_nothing_and_tells_the_presser_alone() {
    let r = asking_six().await;
    let q = waiting_id(&r);
    let before = r.posted(LAB).len();
    let sub = r
        .fake
        .submit_modal(
            &cy_in_lab(),
            &format!("wm:{}:choice:other", short6(&q)),
            &[("note", "my way")],
            &[],
            None,
        )
        .unwrap();
    r.until("told", || said(&r, &sub).contains("did not count"))
        .await;
    let rep = first_reply(&r, &sub).unwrap();
    assert!(rep.ephemeral, "{rep:?}");
    assert_eq!(r.core.confirm_list().unwrap().len(), 1, "it keeps waiting");
    assert_eq!(
        r.posted(LAB).len(),
        before,
        "nothing was said in the channel"
    );
}

pub(super) async fn survey_rig() -> Rig {
    let r = Rig::start_tweaked(
        |_, _| {
            model(
                vec![start_all(&[BUOY]), Scripted::text("Started it.")],
                vec![(BUOY, sleeps(30)), (SURVEY, sleeps(30))],
            )
        },
        guild(),
        &bindings(),
        &[],
        tweak,
    )
    .await;
    r.say((ANA, "ana"), None, ASK);
    r.until("the board", || {
        boards(&r, ANA_DM)
            .first()
            .is_some_and(|b| b.content.contains("1 running"))
    })
    .await;
    r
}

/// The session a started task opened, from its row.
pub(super) fn started(r: &Rig, brief_word: &str) -> Option<String> {
    r.ledger("task.started_by_owner")
        .into_iter()
        .find(|row| row.to_string().contains(brief_word))
        .and_then(|row| row["task"].as_str().map(str::to_string))
}

#[tokio::test]
async fn new_task_opens_a_modal_and_starts_a_task_homed_here() {
    let r = survey_rig().await;
    let iid = r.fake.slash(&ana(), "task", &[("name", "new")]).unwrap();
    let rep = answered(&r, &iid).await;
    let modal = rep.modal.expect("the new-task modal");
    assert_eq!(modal["custom_id"], "nt:");
    let ids: Vec<String> = modal["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["component"]["custom_id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, ["brief", "budget", "report", "expect"]);
    // The board's own button opens the same modal.
    let board = boards(&r, ANA_DM).remove(0);
    let btn = r.press(&board.id, "➕ New task", (ANA, "ana"));
    assert!(answered(&r, &btn).await.modal.is_some());
    let sub = r
        .fake
        .submit_modal(
            &ana(),
            "nt:",
            &[("brief", SURVEY)],
            &[
                ("budget", &["5"]),
                ("report", &["board"]),
                ("expect", &["long"]),
            ],
            None,
        )
        .unwrap();
    r.until("started", || said(&r, &sub).contains("Started"))
        .await;
    let session = started(&r, "Survey-north").unwrap_or_else(|| {
        panic!(
            "task.started_by_owner: {:?}",
            r.ledger("task.started_by_owner")
        )
    });
    assert!(held(&r, &session).is_none(), "no quote, no hold");
    // The modal's "long" reached the task (theseus-bh3t.1).
    let expect = r
        .core
        .store
        .get_meta::<theseus_protocol::task_start::Expect>(&theseus_core::task::expect_key(&session))
        .unwrap();
    assert_eq!(expect, Some(theseus_protocol::task_start::Expect::Long));
    r.until("the board shows it", || {
        boards(&r, ANA_DM)
            .first()
            .is_some_and(|b| b.content.contains("Survey-north"))
    })
    .await;
}

#[tokio::test]
async fn make_this_a_task_on_the_owners_own_message_is_offered_as_a_quote() {
    let r = survey_rig().await;
    let text = "the harbour log says the reef sensor died at noon";
    let iid = r
        .fake
        .message_command(&ana(), "Make this a task", text)
        .unwrap();
    let rep = answered(&r, &iid).await;
    let modal = rep.modal.expect("the same modal");
    let quote = modal["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["component"]["custom_id"] == "quote")
        .expect("the message is offered as a quote");
    assert_eq!(quote["component"]["value"], text);
    assert_eq!(quote["label"], "The message");
    // The id the Discord path produced for the presser's own message.
    assert_eq!(modal["custom_id"], "nt:");
    let sub = r
        .fake
        .submit_modal(
            &ana(),
            "nt:",
            &[("brief", SURVEY), ("quote", text)],
            &[("budget", &["2"]), ("report", &["home"])],
            None,
        )
        .unwrap();
    r.until("started", || said(&r, &sub).contains("Started"))
        .await;
    started(&r, "Survey-north").unwrap_or_else(|| {
        panic!(
            "task.started_by_owner: {:?}",
            r.ledger("task.started_by_owner")
        )
    });
}

#[tokio::test]
async fn make_this_a_task_on_another_persons_message_is_offered_as_outside_text() {
    let r = survey_rig().await;
    let text = "the harbour log says the reef sensor died at noon";
    let iid = r
        .fake
        .message_command_by(&ana(), &ben(), "Make this a task", text)
        .unwrap();
    let rep = answered(&r, &iid).await;
    let modal = rep.modal.expect("the same modal");
    let quote = modal["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["component"]["custom_id"] == "quote")
        .expect("the message is offered as a quote");
    assert_eq!(quote["component"]["value"], text);
    assert_eq!(quote["label"], "The message (read as outside text)");
    // The id the Discord path produced for another person's message: the
    // `!` is what keeps the quote marking the task's session.
    assert_eq!(modal["custom_id"], "nt:!");
    let sub = r
        .fake
        .submit_modal(
            &ana(),
            "nt:!",
            &[("brief", SURVEY), ("quote", text)],
            &[("budget", &["2"]), ("report", &["home"])],
            None,
        )
        .unwrap();
    r.until("started", || said(&r, &sub).contains("Started"))
        .await;
    started(&r, "Survey-north").unwrap_or_else(|| {
        panic!(
            "task.started_by_owner: {:?}",
            r.ledger("task.started_by_owner")
        )
    });
}

#[tokio::test]
async fn a_new_task_from_a_shared_place_is_refused_to_the_presser_with_where_to_start_it() {
    let r = survey_rig().await;
    // A channel nobody bound private: its place is shared.
    let sub = r
        .fake
        .submit_modal(
            &Sender {
                user: BEN,
                name: "ben",
                channel: Some(LAB),
            },
            "nt:",
            &[("brief", SURVEY)],
            &[("budget", &["2"]), ("report", &["home"])],
            None,
        )
        .unwrap();
    // #lab is private here, but ben is not allowed in it: the place refuses him.
    r.until("told", || !said(&r, &sub).is_empty()).await;
    assert!(started(&r, "Survey-north").is_none());
}

#[tokio::test]
async fn quiet_silences_the_places_task_pings_for_a_while_and_off_restores_them() {
    let r = survey_rig().await;
    let iid = r.fake.slash(&ana(), "quiet", &[("for", "30m")]).unwrap();
    r.until("told", || said(&r, &iid).contains("Quiet until"))
        .await;
    r.until("the board says so", || {
        boards(&r, ANA_DM)
            .first()
            .is_some_and(|b| b.content.contains("🔕 quiet until"))
    })
    .await;
    let key = format!("discord.quiet:discord:dm:{ANA}");
    let until = r.core.store.get_meta::<u64>(&key).unwrap().unwrap_or(0);
    assert!(
        until > theseus_protocol::now_unix_ms(),
        "one META key keeps it"
    );
    let off = r.fake.slash(&ana(), "quiet", &[("for", "off")]).unwrap();
    r.until("told", || said(&r, &off).contains("pings are back"))
        .await;
    r.until("the board is loud again", || {
        boards(&r, ANA_DM)
            .first()
            .is_some_and(|b| !b.content.contains("🔕"))
    })
    .await;
    assert_eq!(r.core.store.get_meta::<u64>(&key).unwrap().unwrap_or(0), 0);
}

#[tokio::test]
async fn a_failed_task_on_the_board_has_retry_and_dismiss() {
    let r = Rig::start_tweaked(
        |_, _| {
            model(
                vec![start_all(&[FAILS]), Scripted::text("Started it.")],
                // It runs a moment first, so the board has seen it working.
                vec![(
                    FAILS,
                    vec![
                        sleeps(2).remove(0),
                        Scripted::Fail(theseus_core::provider::ProviderError::InvalidRequest {
                            status: 400,
                            message: "the tool schema was refused".into(),
                        }),
                    ],
                )],
            )
        },
        guild(),
        &bindings(),
        &[],
        tweak,
    )
    .await;
    r.say((ANA, "ana"), None, ASK);
    let has_retry = |r: &Rig| {
        boards(r, ANA_DM).first().is_some_and(|b| {
            b.buttons
                .iter()
                .any(|x| x.custom_id.starts_with("tb:retry:"))
        })
    };
    r.until("the failed task offers Retry and Dismiss", || has_retry(&r))
        .await;
    let board = boards(&r, ANA_DM).remove(0);
    let dismiss = board
        .buttons
        .iter()
        .find(|b| b.custom_id.starts_with("tb:dismiss:"))
        .unwrap()
        .custom_id
        .clone();
    let iid = r.press(&board.id, &dismiss, (ANA, "ana"));
    r.until("dismissed", || said(&r, &iid).contains("Dismissed"))
        .await;
    r.until("the buttons are gone", || !has_retry(&r)).await;
    let _ = LedgerKind::DiscordCommand;
}
