//! A scheduled item's card and post (issue 30; `runtime/sched.rs`,
//! `render/sched.rs`), end to end on the stand-in: a reminder, a check-in and a
//! routine each get a message of their own in the task home and a post in the
//! default forum, kept to the item's life; the pinned board no longer lists
//! them; the presses on a card are the owner's alone.

use std::time::Duration;

use theseus_core::approval::{Client, Surface};
use theseus_core::provider::Scripted;
use theseus_protocol::routines::{RoutineCreateParams, RoutineEditResult};
use theseus_protocol::task_start::TaskStartParams;
use theseus_protocol::wakes::{WakeCreateParams, WakeEditResult};
use theseus_protocol::{WakeCancelParams, WakeCancelResult};
use theseus_sim::fake_discord::{Msg, Pressed};

use crate::rpc_client::RpcClient;
use crate::tests_default_forum::{forum_up, last_name, last_tags, post_with, posts, tag_id};
use crate::tests_gateway::{Rig, ANA, ANA_DM, BEN, CY, LAB};
use crate::tests_task_home::{
    boards, card_in, cards_in, home_record, home_up, no_dm, rig, session_of_place, two_places,
    two_places_with,
};
use crate::tests_task_threads::{answers, gate, open_gate, BUOY};

const V2: u64 = 1 << 15;
const SILENT: u64 = 1 << 12;

fn rpc(r: &Rig) -> std::sync::Arc<RpcClient> {
    RpcClient::connect(r.core.clone(), Client::new("test", Surface::Cli)).0
}

/// A reminder `note` in the DM's conversation, due `when`.
async fn remind(r: &Rig, note: &str, when: &str) -> String {
    let made: WakeEditResult = rpc(r)
        .call(
            theseus_protocol::method::WAKE_CREATE,
            WakeCreateParams {
                when: when.into(),
                note: note.into(),
                session_id: Some(session_of_place(r, &format!("dm:{ANA}"))),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    made.wake.wake_id
}

/// A routine, as `/routine add` makes it.
async fn routine(r: &Rig, title: &str) -> String {
    let made: RoutineEditResult = rpc(r)
        .call(
            theseus_protocol::method::ROUTINE_CREATE,
            RoutineCreateParams {
                title: title.into(),
                schedule: "weekdays 08:00".into(),
                brief: "Look at the tide table.".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    made.routine.id
}

fn card(r: &Rig, home: u64, needle: &str) -> Option<Msg> {
    card_in(r, home, needle)
}

fn cards(r: &Rig, home: u64) -> Vec<Msg> {
    cards_in(r, home)
}

fn ids(m: &Msg) -> Vec<&str> {
    m.buttons.iter().map(|b| b.custom_id.as_str()).collect()
}

fn press(r: &Rig, m: &Msg, button: &str, user: u64, name: &str) -> String {
    r.fake
        .press(&Pressed {
            message: &m.id,
            button,
            user,
            name,
        })
        .unwrap()
}

fn pending(r: &Rig, wake: &str) -> bool {
    r.core
        .kernel
        .pending_wakes()
        .unwrap()
        .into_iter()
        .any(|(_, w)| w.id == wake)
}

/// The face of the pressed card's answer: what the binding told the presser.
fn told(r: &Rig, press: &str) -> Option<(String, bool)> {
    r.fake
        .replies()
        .into_iter()
        .find(|x| x.interaction.as_deref() == Some(press) && x.content.is_some())
        .map(|x| (x.content.unwrap_or_default(), x.ephemeral))
}

/// A reminder set in the DM is a card of its own in the task home, silent, with
/// its two acts, and a post in the default forum tagged `scheduled` whose card
/// links to it and acts on nothing; the pinned board lists neither the reminder
/// nor a Coming up.
#[tokio::test]
async fn a_reminder_gets_a_card_of_its_own_and_a_post_and_the_board_lists_neither() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let forum = forum_up(&r).await;
    let wake = remind(&r, "tide check", "in 90m").await;
    r.until("the reminder's card", || {
        card(&r, home, "tide check").is_some()
    })
    .await;
    let c = card(&r, home, "tide check").unwrap();
    assert_eq!(
        ids(&c),
        [format!("hq:cancel:{wake}"), format!("hq:snooze:{wake}")]
    );
    assert!(c.content.contains("Reminder · from "), "{}", c.content);
    assert_eq!(c.flags & SILENT, SILENT, "no ping");
    assert!(c.mentions.is_empty(), "no mention");
    r.until("its post", || post_with(&r, forum, "tide check").is_some())
        .await;
    let post = post_with(&r, forum, "tide check").unwrap();
    let first = r.fake.messages(post.id).remove(0);
    assert!(
        first
            .buttons
            .iter()
            .all(|b| !b.custom_id.starts_with("hq:")),
        "the forum card acts on nothing: {:?}",
        ids(&first)
    );
    assert!(
        first.components_json.to_string().contains("Open the card"),
        "a link to the card: {}",
        first.components_json
    );
    assert_eq!(
        last_tags(&r, post.id),
        [tag_id(&r, forum, "scheduled")],
        "the one scheduled tag"
    );
    // The four tags a forum has are still there, with the one more.
    let names: Vec<String> = r
        .fake
        .forum_tags(forum)
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(
        names.iter().filter(|n| *n == "scheduled").count(),
        1,
        "made once: {names:?}"
    );
    tokio::time::sleep(Duration::from_millis(1500)).await;
    for b in boards(&r, home) {
        assert!(
            !b.content.contains("tide check") && !b.content.contains("Coming up"),
            "the board lists it: {}",
            b.content
        );
    }
}

/// Two reminders set together are two cards and two posts, none doubled, and a
/// later pass makes no more.
#[tokio::test]
async fn two_items_set_together_are_two_cards_and_two_posts_none_doubled() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let forum = forum_up(&r).await;
    remind(&r, "first tide check", "in 90m").await;
    remind(&r, "second tide check", "in 91m").await;
    r.until("both cards and both posts", || {
        cards(&r, home).len() == 2 && posts(&r, forum).len() == 2
    })
    .await;
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(cards(&r, home).len(), 2, "{:#?}", cards(&r, home));
    assert_eq!(posts(&r, forum).len(), 2);
}

/// Twenty reminders set at once are twenty cards, one request at a time, and
/// the DM's own replies do not wait for them.
#[tokio::test]
async fn a_burst_of_twenty_reminders_is_twenty_cards_and_no_reply_waits() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    for n in 0..20 {
        remind(&r, &format!("burst {n:02}"), "in 90m").await;
    }
    r.until("twenty cards", || cards(&r, home).len() == 20)
        .await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(cards(&r, home).len(), 20, "none doubled");
}

/// The owner's Cancel on the card cancels the wake as the owner, tells the
/// owner alone, and brings the card to "Cancelled by <owner>" with no buttons
/// and the post to `stopped`.
#[tokio::test]
async fn the_owners_cancel_on_a_card_cancels_it_and_ends_the_card_and_the_post() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let forum = forum_up(&r).await;
    let wake = remind(&r, "tide check", "in 90m").await;
    r.until("the card and its post", || {
        card(&r, home, "tide check").is_some() && post_with(&r, forum, "tide check").is_some()
    })
    .await;
    let c = card(&r, home, "tide check").unwrap();
    let pressed = press(&r, &c, &format!("hq:cancel:{wake}"), ANA, "ana");
    r.until("the wake is cancelled", || !pending(&r, &wake))
        .await;
    r.until("the card says how it ended", || {
        card(&r, home, "tide check")
            .is_some_and(|m| m.content.contains("Cancelled by ana") && m.buttons.is_empty())
    })
    .await;
    let after = card(&r, home, "tide check").unwrap();
    assert_eq!(after.id, c.id, "the same message, edited");
    let (said, alone) = told(&r, &pressed).expect("the presser was told");
    assert!(alone && said.contains("Cancelled"), "{said}");
    let post = post_with(&r, forum, "tide check").unwrap();
    r.until("the post is stopped", || {
        last_tags(&r, post.id) == [tag_id(&r, forum, "stopped")]
    })
    .await;
    r.until("the post is renamed", || {
        last_name(&r, post.id).contains('❌')
    })
    .await;
}

/// Snooze 1 h moves the card's own time, keeps its acts, and is the same
/// message.
#[tokio::test]
async fn the_owners_snooze_on_a_card_moves_its_time_on_the_same_message() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let wake = remind(&r, "tide check", "in 10m").await;
    r.until("the card", || card(&r, home, "tide check").is_some())
        .await;
    let c = card(&r, home, "tide check").unwrap();
    let pressed = press(&r, &c, &format!("hq:snooze:{wake}"), ANA, "ana");
    r.until("the card says it is snoozed", || {
        card(&r, home, "tide check").is_some_and(|m| m.content.contains("Snoozed"))
    })
    .await;
    let after = card(&r, home, "tide check").unwrap();
    assert_eq!(after.id, c.id);
    assert_eq!(ids(&after).len(), 2, "its acts stay");
    let (said, alone) = told(&r, &pressed).expect("the presser was told");
    assert!(alone && said.contains("set again"), "{said}");
}

/// A reminder that falls due is brought to "Fired", its buttons gone, and its
/// post to `done` and ✅; a reminder due in seconds still had its card.
#[tokio::test]
async fn a_reminder_that_fires_is_brought_to_fired_and_its_post_is_done() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let forum = forum_up(&r).await;
    remind(&r, "tide check", "in 3s").await;
    r.until("the card", || card(&r, home, "tide check").is_some())
        .await;
    r.until("the card says it fired", || {
        card(&r, home, "tide check")
            .is_some_and(|m| m.content.contains("Fired") && m.buttons.is_empty())
    })
    .await;
    let post = post_with(&r, forum, "tide check").expect("its post");
    r.until("the post is done", || {
        last_tags(&r, post.id) == [tag_id(&r, forum, "done")]
            && last_name(&r, post.id).contains('✅')
    })
    .await;
    // The reminder itself is delivered where it was set, as before.
    r.until("the reminder in the DM", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains("tide check"))
    })
    .await;
}

/// Only the owner's press acts: a member of the guild who is no owner, and a
/// person the home is not bound to, are refused where every press on a place
/// is, and the card and the wake are as they were.
#[tokio::test]
async fn a_card_is_pressed_by_the_owner_alone() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let wake = remind(&r, "tide check", "in 90m").await;
    r.until("the card", || card(&r, home, "tide check").is_some())
        .await;
    let c = card(&r, home, "tide check").unwrap();
    for (user, name) in [(BEN, "ben"), (CY, "cy")] {
        for button in [format!("hq:cancel:{wake}"), format!("hq:snooze:{wake}")] {
            let pressed = press(&r, &c, &button, user, name);
            r.until("the press refused", || told(&r, &pressed).is_some())
                .await;
            let (said, alone) = told(&r, &pressed).unwrap();
            assert!(alone && said.contains("Only the people"), "{said}");
        }
    }
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(pending(&r, &wake), "nothing was cancelled");
    let after = card(&r, home, "tide check").unwrap();
    assert_eq!(
        (after.edits, ids(&after).len()),
        (c.edits, 2),
        "the card is as it was"
    );
}

/// A press on a card whose item is over (cancelled elsewhere while the card's
/// edit was still on its way) says so and cancels nothing again; the card comes
/// to how it ended.
#[tokio::test]
async fn a_press_on_a_card_whose_item_is_over_says_so_and_the_card_comes_to_its_end() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let wake = remind(&r, "tide check", "in 90m").await;
    r.until("the card", || card(&r, home, "tide check").is_some())
        .await;
    let c = card(&r, home, "tide check").unwrap();
    let shared = crate::runtime::task_home::daemon_for_tests(&r.core).expect("the binding runs");
    // Discord answers slowly, so the card's edit lands after the press.
    r.fake.set_delay_ms(2500);
    let _: WakeCancelResult = rpc(&r)
        .call(
            theseus_protocol::method::WAKE_CANCEL,
            WakeCancelParams {
                wake: wake.clone(),
                author: None,
            },
        )
        .await
        .unwrap();
    r.until("the work model has it over", || {
        shared
            .tasks
            .scheduled()
            .iter()
            .any(|v| v.id == wake && v.state.ended())
    })
    .await;
    let pressed = press(&r, &c, &format!("hq:cancel:{wake}"), ANA, "ana");
    r.until("the presser is told", || told(&r, &pressed).is_some())
        .await;
    let (said, alone) = told(&r, &pressed).unwrap();
    assert!(alone && said.contains("over already"), "{said}");
    r.fake.set_delay_ms(0);
    r.until("the card comes to its end", || {
        card(&r, home, "tide check")
            .is_some_and(|m| m.content.contains("Cancelled") && m.buttons.is_empty())
    })
    .await;
    // Cancelled by the call of the command line, so it says so: the record of
    // the cancel names its author.
    let ended = card(&r, home, "tide check").unwrap().content;
    assert!(ended.contains("Cancelled from the terminal"), "{ended}");
}

/// A task set for later and cancelled by the call of the command line: its
/// card says `Cancelled from the terminal`, the post is `stopped`.
#[tokio::test]
async fn a_task_cancelled_from_the_terminal_says_so_on_its_card() {
    let r = two_places_with("theseus-tasks", buoy).await;
    let home = home_up(&r).await;
    let task = start_task(&r, Some("in 90m"));
    r.until("its card", || card(&r, home, "Buoy-paint").is_some())
        .await;
    let _: theseus_protocol::TaskCancelResult = rpc(&r)
        .call(
            theseus_protocol::method::TASK_CANCEL,
            theseus_protocol::TaskCancelParams {
                task: task.clone(),
                author: None,
            },
        )
        .await
        .unwrap();
    r.until("the card says who cancelled it", || {
        card(&r, home, "Buoy-paint").is_some_and(|m| {
            m.content.contains("Cancelled from the terminal") && m.buttons.is_empty()
        })
    })
    .await;
}

/// A routine's card has the board line's words and its acts (Run now, Pause);
/// a press of Pause by the owner brings the card to paused, a press of Resume
/// back; its post stays `scheduled` while the routine stands.
#[tokio::test]
async fn a_routine_has_a_card_with_its_boards_acts_and_a_post_that_stays_scheduled() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let forum = forum_up(&r).await;
    let id = routine(&r, "Tide digest").await;
    r.until("the routine's card and post", || {
        card(&r, home, "Tide digest").is_some() && post_with(&r, forum, "Tide digest").is_some()
    })
    .await;
    let c = card(&r, home, "Tide digest").unwrap();
    assert_eq!(ids(&c), [format!("hr:run:{id}"), format!("hr:pause:{id}")]);
    assert!(
        c.content.contains("Routine · weekdays 08:00"),
        "{}",
        c.content
    );
    press(&r, &c, &format!("hr:pause:{id}"), ANA, "ana");
    r.until("the card says paused", || {
        card(&r, home, "Tide digest").is_some_and(|m| {
            m.content.contains("paused") && ids(&m).contains(&format!("hr:resume:{id}").as_str())
        })
    })
    .await;
    assert_eq!(card(&r, home, "Tide digest").unwrap().id, c.id);
    let post = post_with(&r, forum, "Tide digest").unwrap();
    assert_eq!(last_tags(&r, post.id), [tag_id(&r, forum, "scheduled")]);
    assert!(boards(&r, home)
        .iter()
        .all(|b| !b.content.contains("Routines")));
}

/// With the forum off, the card is still made and no post is.
#[tokio::test]
async fn with_the_forum_off_there_is_a_card_and_no_post() {
    let r = rig(
        vec![Scripted::text("Hello.")],
        |_| vec![],
        &crate::tests_default_forum::bindings(true, ""),
        |_| {},
        |c| c.discord.forum = false,
    )
    .await;
    r.say((ANA, "ana"), None, "hello");
    r.until("the DM answered", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains("Hello."))
    })
    .await;
    let home = home_up(&r).await;
    remind(&r, "tide check", "in 90m").await;
    r.until("the card", || card(&r, home, "tide check").is_some())
        .await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(r.fake.threads().is_empty(), "{:#?}", r.fake.threads());
}

/// With no task home, nothing is made: the place's own board lists the
/// reminder as it did.
#[tokio::test]
async fn with_no_task_home_there_is_no_card_and_the_places_board_lists_it() {
    let r = two_places("").await;
    remind(&r, "tide check", "in 90m").await;
    r.until("the DM's board lists it", || {
        boards(&r, ANA_DM)
            .first()
            .is_some_and(|b| b.content.contains("tide check"))
    })
    .await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let any = r.fake.all_messages().into_iter().any(|m| {
        m.flags & V2 != 0
            && m.buttons
                .iter()
                .all(|b| b.custom_id != crate::render::board_v2::CLEAR)
            && m.content.contains("tide check")
    });
    assert!(!any, "a card was made with no task home");
}

/// With the task home and no DM bound, nothing is made: the channel's own board
/// lists its reminder, as before.
#[tokio::test]
async fn with_no_dm_bound_there_is_no_card() {
    let r = rig(
        vec![Scripted::text("Hello.")],
        |_| vec![],
        &no_dm(),
        |_| {},
        |_| {},
    )
    .await;
    r.until("the task channel is recorded", || home_record(&r).is_some())
        .await;
    let home = home_record(&r).unwrap();
    r.say((ANA, "ana"), Some(LAB), "hello");
    r.until("#lab answered", || {
        r.posted(LAB).iter().any(|m| m.content.contains("Hello."))
    })
    .await;
    let made: WakeEditResult = rpc(&r)
        .call(
            theseus_protocol::method::WAKE_CREATE,
            WakeCreateParams {
                when: "in 90m".into(),
                note: "lab net mending".into(),
                session_id: Some(session_of_place(&r, &format!("channel:{LAB}"))),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(!made.wake.wake_id.is_empty());
    r.until("#lab's board lists it", || {
        boards(&r, LAB)
            .first()
            .is_some_and(|b| b.content.contains("lab net mending"))
    })
    .await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(cards(&r, home).is_empty(), "{:#?}", cards(&r, home));
}

/// A private reminder's words reach the task home, the private forum and the
/// conversation that set it, and no other place the stand-in holds.
#[tokio::test]
async fn a_private_reminders_words_stay_in_the_private_places() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let forum = forum_up(&r).await;
    let secret = "zebra-quartz-ledger";
    let wake = remind(&r, secret, "in 90m").await;
    r.until("the card and the post", || {
        card(&r, home, secret).is_some() && post_with(&r, forum, secret).is_some()
    })
    .await;
    let c = card(&r, home, secret).unwrap();
    press(&r, &c, &format!("hq:snooze:{wake}"), ANA, "ana");
    r.until("snoozed", || {
        card(&r, home, secret).is_some_and(|m| m.content.contains("Snoozed"))
    })
    .await;
    press(&r, &c, &format!("hq:cancel:{wake}"), ANA, "ana");
    r.until("cancelled", || {
        card(&r, home, secret).is_some_and(|m| m.content.contains("Cancelled"))
    })
    .await;
    let private_posts: Vec<String> = posts(&r, forum).iter().map(|p| p.id.to_string()).collect();
    let dm = ANA_DM.to_string();
    let home = home.to_string();
    for m in r.fake.all_messages() {
        let allowed = m.channel == home || m.channel == dm || private_posts.contains(&m.channel);
        assert!(
            allowed || !m.versions.iter().any(|v| v.contains(secret)),
            "a private title reached {}: {m:#?}",
            m.channel
        );
    }
}

/// A process that started again with no list (a store from before this build):
/// the item is given its card again under the same nonce, which is the same
/// message, so the task home holds one card.
#[tokio::test]
async fn a_pass_with_no_list_finds_the_items_card_by_its_nonce_and_makes_no_second() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    remind(&r, "tide check", "in 90m").await;
    r.until("the card", || card(&r, home, "tide check").is_some())
        .await;
    let first = card(&r, home, "tide check").unwrap();
    r.core
        .store
        .put_meta("discord.sched", &Vec::<crate::runtime::sched::Rec>::new())
        .unwrap();
    let shared = crate::runtime::task_home::daemon_for_tests(&r.core).expect("the binding runs");
    let mut again = crate::runtime::sched::Worker::fresh(shared);
    assert!(again.sync().await, "the pass went through");
    let after = cards(&r, home);
    assert_eq!(after.len(), 1, "{after:#?}");
    assert_eq!(after[0].id, first.id);
}

/// A process that started again reads the list the last one wrote: a second
/// pass over it makes no card and edits none (the digest of each is as sent).
#[tokio::test]
async fn a_restarted_pass_over_the_list_makes_and_edits_nothing() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    remind(&r, "first tide check", "in 90m").await;
    remind(&r, "second tide check", "in 91m").await;
    r.until("both cards", || cards(&r, home).len() == 2).await;
    tokio::time::sleep(Duration::from_millis(2000)).await;
    let before = cards(&r, home);
    let shared = crate::runtime::task_home::daemon_for_tests(&r.core).expect("the binding runs");
    let mut again = crate::runtime::sched::Worker::fresh(shared);
    assert!(again.sync().await, "the pass went through");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let after = cards(&r, home);
    assert_eq!(after.len(), 2, "none made twice: {after:#?}");
    let edits = |v: &[Msg]| {
        v.iter()
            .map(|m| (m.id.clone(), m.edits))
            .collect::<Vec<_>>()
    };
    assert_eq!(edits(&after), edits(&before), "none edited");
}

/// An item that was set and is nowhere any more (it fired, or was cancelled,
/// while the daemon was down) has its card brought to its true end, once.
#[tokio::test]
async fn a_card_whose_item_vanished_while_the_daemon_was_down_is_brought_to_its_end_once() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    remind(&r, "tide check", "in 90m").await;
    r.until("the card", || card(&r, home, "tide check").is_some())
        .await;
    let c = card(&r, home, "tide check").unwrap();
    let mut list: Vec<crate::runtime::sched::Rec> = r
        .core
        .store
        .get_meta("discord.sched")
        .unwrap()
        .expect("the list");
    // A second item on the list, due an hour ago, that the daemon no longer
    // holds: its card is the message of the first (all the stand-in has to edit).
    let mut ghost = list[0].clone();
    ghost.id = "wak_01J0ghostghostghostghost0001".into();
    ghost.title = "vanished tide check".into();
    ghost.due_ms = Some(theseus_protocol::now_unix_ms() - 3_600_000);
    ghost.digest = 0;
    list.push(ghost);
    r.core.store.put_meta("discord.sched", &list).unwrap();
    let shared = crate::runtime::task_home::daemon_for_tests(&r.core).expect("the binding runs");
    let mut again = crate::runtime::sched::Worker::fresh(shared);
    assert!(again.sync().await);
    let edited = card(&r, home, "vanished tide check").expect("the ghost's card was edited");
    assert_eq!(edited.id, c.id);
    assert!(
        edited.content.contains("Fired") && edited.buttons.is_empty(),
        "{}",
        edited.content
    );
    let n = edited.edits;
    assert!(again.sync().await);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        card(&r, home, "vanished tide check").unwrap().edits,
        n,
        "brought to its end once"
    );
}

// ---- a task set to start later (issue 30, the owner's addition) ---------------

fn owner() -> theseus_core::approval::Answerer {
    crate::tests_default_forum::cli()
}

/// The briefs of the tests below: held at a gate while it runs, then done.
fn buoy(dir: &std::path::Path) -> Vec<(&'static str, Vec<Scripted>)> {
    vec![(BUOY, [vec![gate(dir)], answers("Buoy painted.")].concat())]
}

/// A task for the DM's conversation, to start `when` (none: at once).
fn start_task(r: &Rig, when: Option<&str>) -> String {
    r.core
        .task_start(
            &TaskStartParams {
                brief: BUOY.into(),
                home: Some(session_of_place(r, &format!("dm:{ANA}"))),
                start_when: when.map(str::to_string),
                ..Default::default()
            },
            owner(),
        )
        .unwrap()
        .session_id
        .replacen("ses_", "exe_", 1)
}

/// A task set to start later is `scheduled` in its post (tag and card words),
/// has one card of its own in the task home (Cancel and Snooze 1 h) and no second
/// post, is on the pinned board as scheduled and not running; at its start the
/// card says it started, and the post is `running`; at its end `done`.
#[tokio::test]
async fn a_task_set_for_later_is_scheduled_then_running_then_done_with_one_card_and_one_post() {
    let r = two_places_with("theseus-tasks", buoy).await;
    let home = home_up(&r).await;
    let forum = forum_up(&r).await;
    let task = start_task(&r, Some("in 6s"));
    r.until("its card in the task home", || {
        card(&r, home, "Buoy-paint").is_some()
    })
    .await;
    let c = card(&r, home, "Buoy-paint").unwrap();
    assert_eq!(
        ids(&c),
        [format!("hq:cancel:{task}"), format!("hq:snooze:{task}")]
    );
    assert!(c.content.contains("Task · from DM @ana"), "{}", c.content);
    assert!(c.content.starts_with("🕒"), "{}", c.content);
    r.until("its post says scheduled", || {
        post_with(&r, forum, "scheduled · starts").is_some()
    })
    .await;
    let post = post_with(&r, forum, "scheduled · starts").unwrap();
    assert_eq!(
        posts(&r, forum).len(),
        1,
        "the task's own post, and no second"
    );
    assert_eq!(last_tags(&r, post.id), [tag_id(&r, forum, "scheduled")]);
    let first = r.fake.messages(post.id).remove(0).content;
    assert!(
        !first.contains("running") && !first.contains("started"),
        "{first}"
    );
    r.until("the pinned board lists it as scheduled", || {
        boards(&r, home).first().is_some_and(|b| {
            b.content.contains("scheduled · starts") && b.content.contains("1 scheduled")
        })
    })
    .await;
    let b = boards(&r, home).remove(0).content;
    assert!(!b.contains("running") && !b.contains("Coming up"), "{b}");
    // Its start: the card says so and loses its buttons; the post is running.
    r.until("the card says it started", || {
        card(&r, home, "Buoy-paint")
            .is_some_and(|m| m.content.contains("Started") && m.buttons.is_empty())
    })
    .await;
    assert_eq!(
        card(&r, home, "Buoy-paint").unwrap().id,
        c.id,
        "the same card"
    );
    r.until("the post is running", || {
        last_tags(&r, post.id) == [tag_id(&r, forum, "running")]
    })
    .await;
    assert_eq!(cards(&r, home).len(), 1, "no second card for the task");
    open_gate(&r);
    r.until("the post is done", || {
        last_tags(&r, post.id) == [tag_id(&r, forum, "done")]
    })
    .await;
}

/// A task started at once is never drawn `scheduled`: no card of its own, no
/// scheduled tag on its post, no such word on its card.
#[tokio::test]
async fn a_task_started_at_once_is_never_scheduled() {
    let r = two_places_with("theseus-tasks", buoy).await;
    let home = home_up(&r).await;
    let forum = forum_up(&r).await;
    start_task(&r, None);
    r.until("its post", || post_with(&r, forum, "Buoy-paint").is_some())
        .await;
    let post = post_with(&r, forum, "Buoy-paint").unwrap();
    r.until("it is running", || {
        last_tags(&r, post.id) == [tag_id(&r, forum, "running")]
    })
    .await;
    assert!(
        !r.fake
            .forum_tags(forum)
            .iter()
            .any(|t| t.name == "scheduled"),
        "no scheduled tag was made for it"
    );
    assert!(!r.fake.messages(post.id)[0].content.contains("scheduled"));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(cards(&r, home).is_empty(), "{:#?}", cards(&r, home));
    open_gate(&r);
}

/// The owner's Cancel on the task's card cancels the task as the owner and ends
/// the card and the post; a person who is no owner is refused; Snooze 1 h moves
/// its start on the same card.
#[tokio::test]
async fn the_owners_cancel_and_snooze_on_a_scheduled_tasks_card_act_on_the_task() {
    let r = two_places_with("theseus-tasks", buoy).await;
    let home = home_up(&r).await;
    let forum = forum_up(&r).await;
    let task = start_task(&r, Some("in 90m"));
    r.until("its card", || card(&r, home, "Buoy-paint").is_some())
        .await;
    let c = card(&r, home, "Buoy-paint").unwrap();
    let refused = press(&r, &c, &format!("hq:cancel:{task}"), BEN, "ben");
    r.until("ben is refused", || told(&r, &refused).is_some())
        .await;
    assert!(told(&r, &refused).unwrap().0.contains("Only the people"));
    let snoozed = press(&r, &c, &format!("hq:snooze:{task}"), ANA, "ana");
    r.until("the card says snoozed", || {
        card(&r, home, "Buoy-paint").is_some_and(|m| m.content.contains("Snoozed"))
    })
    .await;
    let (said, alone) = told(&r, &snoozed).unwrap();
    assert!(alone && said.contains("will start at"), "{said}");
    assert_eq!(card(&r, home, "Buoy-paint").unwrap().id, c.id);
    let pressed = press(&r, &c, &format!("hq:cancel:{task}"), ANA, "ana");
    r.until("the card says cancelled by ana", || {
        card(&r, home, "Buoy-paint")
            .is_some_and(|m| m.content.contains("Cancelled by ana") && m.buttons.is_empty())
    })
    .await;
    let (said, alone) = told(&r, &pressed).unwrap();
    assert!(alone && said.contains("will not start"), "{said}");
    let post = post_with(&r, forum, "Buoy-paint").unwrap();
    r.until("the post is stopped", || {
        last_tags(&r, post.id) == [tag_id(&r, forum, "stopped")]
    })
    .await;
}

/// A restart while the task waits: a fresh pass over the list makes no second
/// card and edits none, and the task's view still draws as scheduled.
#[tokio::test]
async fn a_restart_while_a_task_waits_keeps_it_scheduled_and_makes_no_second_card() {
    let r = two_places_with("theseus-tasks", buoy).await;
    let home = home_up(&r).await;
    let forum = forum_up(&r).await;
    start_task(&r, Some("in 90m"));
    r.until("its card and its scheduled post", || {
        card(&r, home, "Buoy-paint").is_some()
            && post_with(&r, forum, "scheduled · starts").is_some()
    })
    .await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let before = cards(&r, home);
    let shared = crate::runtime::task_home::daemon_for_tests(&r.core).expect("the binding runs");
    let mut again = crate::runtime::sched::Worker::fresh(shared);
    assert!(again.sync().await);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let after = cards(&r, home);
    assert_eq!(after.len(), 1, "{after:#?}");
    assert_eq!(
        (after[0].id.clone(), after[0].edits),
        (before[0].id.clone(), before[0].edits)
    );
    assert_eq!(posts(&r, forum).len(), 1);
}
