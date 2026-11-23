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
    // The catch-up gap shortened (15 cards after the first five, a gap apart).
    crate::runtime::sched::space_for_tests(Duration::from_millis(200));
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

/// The worker's list once it holds a row: a card is shown before its row is
/// written, so a test that reads the list waits for the row.
async fn listed_once_shown(r: &Rig) -> Vec<crate::runtime::sched::Rec> {
    let listed = || -> Vec<crate::runtime::sched::Rec> {
        r.core
            .store
            .get_meta("discord.sched")
            .unwrap()
            .unwrap_or_default()
    };
    r.until("its row is on the list", || !listed().is_empty())
        .await;
    listed()
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
    let mut list = listed_once_shown(&r).await;
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

/// A fired reminder's post is read by the same Clear done as a done task's and
/// archived by the same sweep, a day after its end; a reminder still waiting is
/// left in the forum.
#[tokio::test]
async fn a_fired_reminders_post_is_archived_with_a_done_tasks_by_the_same_press_and_sweep() {
    const KEY: &str = "discord.task_posts";
    let r = two_places_with("theseus-tasks", buoy).await;
    let home = home_up(&r).await;
    let forum = forum_up(&r).await;
    remind(&r, "waiting tide check", "in 90m").await;
    remind(&r, "fired tide check", "in 3s").await;
    start_task(&r, None);
    r.until("the three posts", || posts(&r, forum).len() == 3)
        .await;
    let rows = |r: &Rig| -> Vec<serde_json::Value> {
        r.core
            .store
            .get_meta::<Vec<serde_json::Value>>(KEY)
            .ok()
            .flatten()
            .unwrap_or_default()
    };
    r.until("the reminder's post is done", || {
        post_with(&r, forum, "fired tide check")
            .is_some_and(|p| last_tags(&r, p.id) == [tag_id(&r, forum, "done")])
    })
    .await;
    open_gate(&r);
    r.until("the task's and the reminder's rows ended", || {
        rows(&r).iter().filter(|p| p["ended_ms"].is_u64()).count() == 2
    })
    .await;
    r.until("a board with Clear done", || !boards(&r, home).is_empty())
        .await;
    let board = boards(&r, home).remove(0);
    r.press(&board.id, "✓ Clear done", (ANA, "ana"));
    r.until("both finished rows read", || {
        rows(&r)
            .iter()
            .filter(|p| p["ended_ms"].is_u64() && p["read_ms"].is_u64())
            .count()
            == 2
    })
    .await;
    // They ended a day and an hour ago.
    let now = theseus_protocol::now_unix_ms();
    let mut list = rows(&r);
    for p in &mut list {
        if p["ended_ms"].is_u64() {
            p["ended_ms"] = serde_json::json!(now - 25 * 3_600_000);
        }
    }
    r.core.store.put_meta(KEY, &list).unwrap();
    crate::runtime::shared_for_tests(&r.core).sweep_once().await;
    let archived = |name: &str| {
        let p = post_with(&r, forum, name).unwrap();
        r.fake
            .threads()
            .into_iter()
            .find(|t| t.id == p.id)
            .unwrap()
            .archived
    };
    assert!(archived("fired tide check"), "the fired reminder's post");
    assert!(archived("Buoy-paint"), "the done task's post");
    assert!(
        !archived("waiting tide check"),
        "a reminder that waits stays"
    );
}

// ---- a fired reminder set again, and the review's notes (issue 30) -------------

/// What the delivered reminder's Snooze buttons call: `wake.snooze` on a fired
/// wake, which sets it again under its own id.
async fn snooze_fired(r: &Rig, wake: &str, when: &str) {
    let _: WakeEditResult = rpc(r)
        .call(
            theseus_protocol::method::WAKE_SNOOZE,
            theseus_protocol::wakes::WakeSnoozeParams {
                wake: wake.into(),
                when: Some(when.into()),
                fired: true,
                author: None,
            },
        )
        .await
        .unwrap();
}

/// The fired reminder's post and card, once it has fired.
async fn fired(r: &Rig, home: u64, forum: u64, note: &str) -> (Msg, u64) {
    r.until("the card says it fired", || {
        card(r, home, note).is_some_and(|m| m.content.contains("Fired") && m.buttons.is_empty())
    })
    .await;
    r.until("the post is done", || {
        post_with(r, forum, note).is_some_and(|p| last_tags(r, p.id) == [tag_id(r, forum, "done")])
    })
    .await;
    (
        card(r, home, note).unwrap(),
        post_with(r, forum, note).unwrap().id,
    )
}

/// A reminder that fired and is set again from its notice (Snooze under its own
/// id) is the SAME card, with its buttons and the new time, and its post is
/// `scheduled` again with its mark off; a later pass makes no second card and no
/// edit.
#[tokio::test]
async fn a_fired_reminder_snoozed_from_its_notice_stands_again_on_the_same_card_and_post() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let forum = forum_up(&r).await;
    let wake = remind(&r, "tide check", "in 3s").await;
    let (first, post) = fired(&r, home, forum, "tide check").await;
    snooze_fired(&r, &wake, "in 90m").await;
    r.until("the card stands again", || {
        card(&r, home, "tide check")
            .is_some_and(|m| !m.content.contains("Fired") && m.buttons.len() == 2)
    })
    .await;
    let again = card(&r, home, "tide check").unwrap();
    assert_eq!(again.id, first.id, "the same card");
    assert!(again.content.contains("Snoozed"), "{}", again.content);
    assert_eq!(
        ids(&again),
        [format!("hq:cancel:{wake}"), format!("hq:snooze:{wake}")]
    );
    r.until("the post is scheduled again", || {
        last_tags(&r, post) == [tag_id(&r, forum, "scheduled")]
            && !last_name(&r, post).contains('✅')
    })
    .await;
    // Later passes: no second card, no edit.
    let shared = crate::runtime::task_home::daemon_for_tests(&r.core).expect("the binding runs");
    let mut pass = crate::runtime::sched::Worker::fresh(shared);
    assert!(pass.sync().await);
    assert!(pass.sync().await);
    tokio::time::sleep(Duration::from_millis(800)).await;
    let after = cards(&r, home);
    assert_eq!(after.len(), 1, "{after:#?}");
    assert_eq!(
        (after[0].id.clone(), after[0].edits),
        (again.id, again.edits)
    );
}

/// The sweep may have archived and locked the post of the fired reminder before
/// it was set again: it is opened, unlocked and `scheduled` again.
#[tokio::test]
async fn a_post_the_sweep_archived_is_opened_again_when_its_reminder_is_set_again() {
    const KEY: &str = "discord.task_posts";
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let forum = forum_up(&r).await;
    let wake = remind(&r, "tide check", "in 3s").await;
    let (_, post) = fired(&r, home, forum, "tide check").await;
    r.until("its row ended", || {
        r.core
            .store
            .get_meta::<Vec<serde_json::Value>>(KEY)
            .ok()
            .flatten()
            .is_some_and(|l| l.iter().any(|p| p["ended_ms"].is_u64()))
    })
    .await;
    let now = theseus_protocol::now_unix_ms();
    let mut list: Vec<serde_json::Value> = r.core.store.get_meta(KEY).unwrap().unwrap();
    for p in &mut list {
        if p["ended_ms"].is_u64() {
            p["ended_ms"] = serde_json::json!(now - 25 * 3_600_000);
            p["read_ms"] = serde_json::json!(now - 1_000);
        }
    }
    r.core.store.put_meta(KEY, &list).unwrap();
    crate::runtime::shared_for_tests(&r.core).sweep_once().await;
    let thread = |r: &Rig| r.fake.threads().into_iter().find(|t| t.id == post).unwrap();
    assert!(
        thread(&r).archived && thread(&r).locked,
        "the sweep took it"
    );
    snooze_fired(&r, &wake, "in 90m").await;
    r.until("the post is open and scheduled again", || {
        let t = thread(&r);
        !t.archived && !t.locked && last_tags(&r, post) == [tag_id(&r, forum, "scheduled")]
    })
    .await;
}

/// A series that fired goes on under its id: its card stays a scheduled one
/// (never "Fired") and its post stays `scheduled`.
#[tokio::test]
async fn a_series_that_fired_stays_a_scheduled_card_and_post() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let forum = forum_up(&r).await;
    let _: WakeEditResult = rpc(&r)
        .call(
            theseus_protocol::method::WAKE_CREATE,
            WakeCreateParams {
                when: "in 3s".into(),
                note: "water the ferns".into(),
                every: Some("1h".into()),
                session_id: Some(session_of_place(&r, &format!("dm:{ANA}"))),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    r.until("the card", || card(&r, home, "water the ferns").is_some())
        .await;
    let first = card(&r, home, "water the ferns").unwrap();
    r.until("its time has moved on", || {
        card(&r, home, "water the ferns").is_some_and(|m| m.content != first.content)
    })
    .await;
    let c = card(&r, home, "water the ferns").unwrap();
    assert_eq!(c.id, first.id);
    assert!(
        !c.content.contains("Fired") && c.buttons.len() == 2,
        "{}",
        c.content
    );
    let post = post_with(&r, forum, "water the ferns").unwrap();
    assert_eq!(last_tags(&r, post.id), [tag_id(&r, forum, "scheduled")]);
}

/// A routine's next run moved (its wake snoozed): the same card, its time on,
/// its acts kept, its post `scheduled`.
#[tokio::test]
async fn a_routines_next_run_moving_edits_its_card_and_leaves_its_post_scheduled() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let forum = forum_up(&r).await;
    routine(&r, "Tide digest").await;
    r.until("its card and post", || {
        card(&r, home, "Tide digest").is_some() && post_with(&r, forum, "Tide digest").is_some()
    })
    .await;
    let first = card(&r, home, "Tide digest").unwrap();
    let wakes: serde_json::Value = rpc(&r)
        .call(theseus_protocol::method::WAKE_LIST, serde_json::json!({}))
        .await
        .unwrap();
    let w = wakes["wakes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["purpose"] == "routine")
        .expect("the routine's wake")["short"]
        .clone();
    let _: WakeEditResult = rpc(&r)
        .call(
            theseus_protocol::method::WAKE_SNOOZE,
            theseus_protocol::wakes::WakeSnoozeParams {
                wake: w.as_str().unwrap().into(),
                when: Some("in 5h".into()),
                fired: false,
                author: None,
            },
        )
        .await
        .unwrap();
    r.until("its time is on", || {
        card(&r, home, "Tide digest").is_some_and(|m| m.content != first.content)
    })
    .await;
    let c = card(&r, home, "Tide digest").unwrap();
    assert_eq!((c.id.clone(), c.buttons.len()), (first.id, 2));
    let post = post_with(&r, forum, "Tide digest").unwrap();
    assert_eq!(last_tags(&r, post.id), [tag_id(&r, forum, "scheduled")]);
}

/// Twelve items set at once: the first few cards at once, the rest a gap apart,
/// so a task's report to the task home is not behind all of them.
#[tokio::test]
async fn a_catch_up_makes_its_first_cards_at_once_and_the_rest_a_gap_apart() {
    crate::runtime::sched::space_for_tests(Duration::from_millis(400));
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    for n in 0..12 {
        remind(&r, &format!("catch up {n:02}"), "in 90m").await;
    }
    r.until("twelve cards", || cards(&r, home).len() == 12)
        .await;
    let mut at: Vec<u64> = cards(&r, home).iter().map(|m| m.created_ms).collect();
    at.sort_unstable();
    assert!(
        at[11] - at[0] >= 1_200,
        "paced: the last is at least three gaps after the first: {at:?}"
    );
    assert!(at[4] - at[0] < 1_200, "the first five at once: {at:?}");
}

/// A card the owner deleted by hand: one whose item has ended is not made again;
/// one whose item stands is.
#[tokio::test]
async fn a_card_deleted_by_hand_is_made_again_only_while_its_item_stands() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let shared = crate::runtime::shared_for_tests(&r.core);
    let delete = |m: &Msg| {
        let (c, id) = (
            m.channel.parse::<u64>().unwrap(),
            m.id.parse::<u64>().unwrap(),
        );
        let http = shared.http.clone();
        async move {
            http.delete_message(
                twilight_model::id::Id::new(c),
                twilight_model::id::Id::new(id),
            )
            .await
            .unwrap();
        }
    };
    // Stands: deleted, then set again (its card needs an edit): made again.
    let stands = remind(&r, "standing check", "in 90m").await;
    // Ended: deleted, then cancelled: not made again.
    let ends = remind(&r, "ending check", "in 91m").await;
    r.until("both cards", || {
        card(&r, home, "standing check").is_some() && card(&r, home, "ending check").is_some()
    })
    .await;
    delete(&card(&r, home, "standing check").unwrap()).await;
    delete(&card(&r, home, "ending check").unwrap()).await;
    let _: WakeEditResult = rpc(&r)
        .call(
            theseus_protocol::method::WAKE_SNOOZE,
            theseus_protocol::wakes::WakeSnoozeParams {
                wake: stands,
                when: Some("in 2h".into()),
                fired: false,
                author: None,
            },
        )
        .await
        .unwrap();
    let _: WakeCancelResult = rpc(&r)
        .call(
            theseus_protocol::method::WAKE_CANCEL,
            WakeCancelParams {
                wake: ends,
                author: None,
            },
        )
        .await
        .unwrap();
    r.until("the standing item has a card again", || {
        card(&r, home, "standing check").is_some_and(|m| m.buttons.len() == 2)
    })
    .await;
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(cards(&r, home).len(), 1, "{:#?}", cards(&r, home));
    assert!(
        card(&r, home, "ending check").is_none(),
        "the owner deleted it"
    );
}

/// A press on a card whose item the model no longer holds does nothing and says
/// so; it never sets a long-fired reminder again.
#[tokio::test]
async fn a_press_on_a_card_whose_item_is_no_longer_held_does_nothing_and_says_so() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let wake = remind(&r, "tide check", "in 4s").await;
    r.until("the card", || card(&r, home, "tide check").is_some())
        .await;
    let c = card(&r, home, "tide check").unwrap();
    // Discord is slow, so the card still has its buttons after the reminder fired.
    r.fake.set_delay_ms(3000);
    let shared = crate::runtime::task_home::daemon_for_tests(&r.core).expect("the binding runs");
    r.until("the model has it fired", || {
        shared
            .tasks
            .scheduled()
            .iter()
            .any(|v| v.id == wake && v.state.ended())
    })
    .await;
    // Long over: the model has let it go.
    shared.tasks.changed(theseus_protocol::WorkChanged {
        position: u64::MAX / 2,
        at_ms: 0,
        views: vec![],
        gone: vec![wake.clone()],
    });
    // The agenda's last read, which listed it before it fired, is read again.
    shared.read_future().await;
    let pressed = press(&r, &c, &format!("hq:snooze:{wake}"), ANA, "ana");
    r.until("the presser is told", || told(&r, &pressed).is_some())
        .await;
    let (said, alone) = told(&r, &pressed).unwrap();
    assert!(alone && said.contains("no longer on the board"), "{said}");
    r.fake.set_delay_ms(0);
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(
        !pending(&r, &wake),
        "a long-fired reminder was not set again"
    );
}

/// Before the work model is loaded an item missing from it is not an item gone:
/// the pass leaves its card as it was; once the model is loaded it is brought to
/// its end.
#[tokio::test]
async fn an_item_missing_from_a_model_that_is_not_loaded_is_not_cancelled() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    remind(&r, "tide check", "in 90m").await;
    r.until("the card", || card(&r, home, "tide check").is_some())
        .await;
    let c = card(&r, home, "tide check").unwrap();
    let mut list = listed_once_shown(&r).await;
    // A second entry on the list, due in two hours, that nothing holds: its
    // card is the first one's message.
    let mut ghost = list[0].clone();
    ghost.id = "wak_01J0ghostghostghostghost0002".into();
    ghost.title = "ghost check".into();
    ghost.due_ms = Some(theseus_protocol::now_unix_ms() + 7_200_000);
    ghost.digest = 0;
    list.push(ghost);
    r.core.store.put_meta("discord.sched", &list).unwrap();
    let shared = crate::runtime::task_home::daemon_for_tests(&r.core).expect("the binding runs");
    shared.tasks.loaded_for_tests(false);
    let mut pass = crate::runtime::sched::Worker::fresh(shared.clone());
    assert!(pass.sync().await);
    let held = card(&r, home, "tide check").expect("the card is as it was");
    assert_eq!((held.id.clone(), held.edits), (c.id.clone(), c.edits));
    shared.tasks.loaded_for_tests(true);
    assert!(pass.sync().await);
    let ended = card(&r, home, "ghost check").expect("brought to its end once loaded");
    assert!(ended.content.contains("Cancelled"), "{}", ended.content);
}

// ---- the review of the card and post intents ------------------------------------

/// The task's card, deleted at the stand-in, with the list emptied and an intent
/// standing (a kill before the create arrived): the pinned board, which lists the
/// scheduled task with the very marker the card carries, is never taken for the
/// card; the task's own card is made.
#[tokio::test]
async fn an_intent_with_no_card_does_not_adopt_the_pinned_board() {
    let r = two_places_with("theseus-tasks", buoy).await;
    let home = home_up(&r).await;
    let task = start_task(&r, Some("in 90m"));
    r.until("its card and the board", || {
        card(&r, home, "Buoy-paint").is_some() && !boards(&r, home).is_empty()
    })
    .await;
    let c = card(&r, home, "Buoy-paint").unwrap();
    let board = boards(&r, home).remove(0);
    let shared = crate::runtime::task_home::daemon_for_tests(&r.core).expect("the binding runs");
    shared
        .http
        .delete_message(
            twilight_model::id::Id::new(home),
            twilight_model::id::Id::new(c.id.parse().unwrap()),
        )
        .await
        .unwrap();
    r.core
        .store
        .put_meta("discord.sched", &Vec::<crate::runtime::sched::Rec>::new())
        .unwrap();
    r.core
        .store
        .put_meta(
            "discord.sched_intents",
            &serde_json::json!([{"id": task, "channel": home,
                                 "at_ms": theseus_protocol::now_unix_ms()}]),
        )
        .unwrap();
    let mut pass = crate::runtime::sched::Worker::fresh(shared.clone());
    assert!(pass.sync().await);
    let made = card(&r, home, "Buoy-paint").expect("the task's own card is made");
    assert_ne!(made.id, board.id, "not the board");
    assert_ne!(made.id, c.id, "a new message: the old was deleted");
    let after = boards(&r, home).remove(0);
    assert!(
        after.versions.iter().all(|v| v.starts_with("📋")),
        "the board never showed a card: {:?}",
        after.versions
    );
}

/// Two items whose cards are lost (their entries gone), one with an intent
/// standing that cannot be read for: that one gets no card made blind, and the
/// other's card is made all the same.
#[tokio::test]
async fn a_read_that_fails_for_one_item_leaves_the_others_cards_to_be_made() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let a = remind(&r, "first tide check", "in 90m").await;
    remind(&r, "second tide check", "in 91m").await;
    r.until("both cards", || {
        card(&r, home, "first tide check").is_some()
            && card(&r, home, "second tide check").is_some()
    })
    .await;
    let shared = crate::runtime::task_home::daemon_for_tests(&r.core).expect("the binding runs");
    for note in ["first tide check", "second tide check"] {
        let c = card(&r, home, note).unwrap();
        shared
            .http
            .delete_message(
                twilight_model::id::Id::new(home),
                twilight_model::id::Id::new(c.id.parse().unwrap()),
            )
            .await
            .unwrap();
    }
    r.core
        .store
        .put_meta("discord.sched", &Vec::<crate::runtime::sched::Rec>::new())
        .unwrap();
    r.core
        .store
        .put_meta(
            "discord.sched_intents",
            &serde_json::json!([{"id": a, "channel": home,
                                 "at_ms": theseus_protocol::now_unix_ms()}]),
        )
        .unwrap();
    r.fake.fail_message_lists(true);
    let mut pass = crate::runtime::sched::Worker::fresh(shared.clone());
    assert!(!pass.sync().await, "the pass says it failed");
    assert!(
        card(&r, home, "second tide check").is_some(),
        "the other item's card is made"
    );
    assert!(
        card(&r, home, "first tide check").is_none(),
        "nothing is made blind for the item whose read failed"
    );
    r.fake.fail_message_lists(false);
    assert!(pass.sync().await);
    assert!(
        card(&r, home, "first tide check").is_some(),
        "made once the read works"
    );
}

/// Before the bot knows its own id no card is adopted (and none is made):
/// an error and a retry.
#[tokio::test]
async fn no_card_is_adopted_while_the_bots_own_id_is_unknown() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let wake = remind(&r, "tide check", "in 90m").await;
    r.until("its card", || card(&r, home, "tide check").is_some())
        .await;
    let shared = crate::runtime::task_home::daemon_for_tests(&r.core).expect("the binding runs");
    let known = shared.bot_id();
    // The card is at Discord, its entry lost, an intent standing.
    r.core
        .store
        .put_meta("discord.sched", &Vec::<crate::runtime::sched::Rec>::new())
        .unwrap();
    r.core
        .store
        .put_meta(
            "discord.sched_intents",
            &serde_json::json!([{"id": wake, "channel": home,
                                 "at_ms": theseus_protocol::now_unix_ms()}]),
        )
        .unwrap();
    shared.set_bot_id_for_tests(0);
    let mut pass = crate::runtime::sched::Worker::fresh(shared.clone());
    assert!(
        !pass.sync().await,
        "no adoption: the pass fails and retries"
    );
    assert_eq!(cards(&r, home).len(), 1, "and none was made");
    shared.set_bot_id_for_tests(known);
    assert!(pass.sync().await);
    assert_eq!(cards(&r, home).len(), 1, "found, not made again");
}

// ---- the second snooze of a fired reminder, and the review's notes (issue 30) ---

fn pending_notes(r: &Rig, note: &str) -> usize {
    r.core
        .kernel
        .pending_wakes()
        .unwrap()
        .into_iter()
        .filter(|(_, w)| w.note == note)
        .count()
}

fn posts_about(r: &Rig, forum: u64, note: &str) -> usize {
    posts(r, forum)
        .iter()
        .filter(|p| {
            r.fake
                .messages(p.id)
                .first()
                .is_some_and(|m| m.content.contains(note))
        })
        .count()
}

/// The reminder's snooze from its notice, twice (the card's press, then the
/// notice's; or a double tap), leaves one reminder, one card and one post.
#[tokio::test]
async fn two_snoozes_of_a_fired_reminder_leave_one_reminder_one_card_and_one_post() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let forum = forum_up(&r).await;
    let wake = remind(&r, "tide check", "in 3s").await;
    fired(&r, home, forum, "tide check").await;
    snooze_fired(&r, &wake, "in 90m").await;
    snooze_fired(&r, &wake, "in 120m").await;
    r.until("the card stands again", || {
        card(&r, home, "tide check").is_some_and(|m| m.buttons.len() == 2)
    })
    .await;
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(pending_notes(&r, "tide check"), 1, "one reminder");
    assert_eq!(cards(&r, home).len(), 1, "{:#?}", cards(&r, home));
    assert_eq!(posts_about(&r, forum, "tide check"), 1);
}

/// A post the owner deleted cannot be reopened (404): the item is given a first
/// post, and the 404 is not tried again.
#[tokio::test]
async fn a_post_the_owner_deleted_is_made_again_when_its_reminder_is_set_again() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let forum = forum_up(&r).await;
    let wake = remind(&r, "tide check", "in 3s").await;
    let (_, post) = fired(&r, home, forum, "tide check").await;
    // The owner deletes it once its end is fully shown (the name too): a delete
    // between the tag and the name is the order `a_post_the_owner_deleted_while_its_item_stands_is_made_again`
    // and the worker's own tests hold.
    r.until("its end is fully shown", || {
        last_name(&r, post).contains('✅')
    })
    .await;
    r.fake.delete_thread(post);
    snooze_fired(&r, &wake, "in 90m").await;
    r.until("a new post, scheduled", || {
        post_with(&r, forum, "tide check").is_some_and(|p| {
            p.id != post && last_tags(&r, p.id) == [tag_id(&r, forum, "scheduled")]
        })
    })
    .await;
    assert_eq!(posts_about(&r, forum, "tide check"), 1);
    let gone = r.fake.seen().iter().filter(|s| s.outcome == "gone").count();
    assert_eq!(gone, 1, "the 404 is told once, not tried again");
}

/// A post the bot may not edit (403) is not tried again at every pass; it is
/// said once what is missing, and it is opened once the permission is back.
#[tokio::test]
async fn a_post_that_cannot_be_reopened_is_not_tried_again_and_says_so_once() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let forum = forum_up(&r).await;
    let wake = remind(&r, "tide check", "in 3s").await;
    let (_, post) = fired(&r, home, forum, "tide check").await;
    // Its end is fully shown (the name too) before the permission goes: the
    // refusal must not land in the middle of the end's own writes.
    r.until("its end is fully shown", || {
        last_name(&r, post).contains('✅')
    })
    .await;
    r.fake.refuse_thread_patch(true);
    snooze_fired(&r, &wake, "in 90m").await;
    let shared = crate::runtime::task_home::daemon_for_tests(&r.core).expect("the binding runs");
    for _ in 0..8 {
        let mut pass = crate::runtime::sched::Worker::fresh(shared.clone());
        assert!(pass.sync().await);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let refused = r
        .fake
        .seen()
        .iter()
        .filter(|s| s.outcome == "refused")
        .count();
    assert!(refused <= 2, "tried {refused} times in a second");
    r.fake.refuse_thread_patch(false);
    r.until("the post opens once it may", || {
        last_tags(&r, post) == [tag_id(&r, forum, "scheduled")]
    })
    .await;
}

/// A press before the work model has loaded is not "gone": it says Theseus is
/// still starting, and writes nothing.
#[tokio::test]
async fn a_press_before_the_work_model_is_loaded_says_it_is_still_starting() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let wake = remind(&r, "tide check", "in 90m").await;
    r.until("the card", || card(&r, home, "tide check").is_some())
        .await;
    let c = card(&r, home, "tide check").unwrap();
    let shared = crate::runtime::task_home::daemon_for_tests(&r.core).expect("the binding runs");
    shared.tasks.loaded_for_tests(false);
    let pressed = press(&r, &c, &format!("hq:cancel:{wake}"), ANA, "ana");
    r.until("told", || told(&r, &pressed).is_some()).await;
    let (said, alone) = told(&r, &pressed).unwrap();
    assert!(alone && said.contains("still starting"), "{said}");
    shared.tasks.loaded_for_tests(true);
    assert!(pending(&r, &wake), "nothing was written");
}

/// A post the owner deletes while its item stands (every call on it answers 404):
/// it is made again, `scheduled`, and the 404 is not tried again for ever.
#[tokio::test]
async fn a_post_the_owner_deleted_while_its_item_stands_is_made_again() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let forum = forum_up(&r).await;
    let wake = remind(&r, "tide check", "in 90m").await;
    r.until("its card and its post, scheduled", || {
        card(&r, home, "tide check").is_some()
            && post_with(&r, forum, "tide check")
                .is_some_and(|p| last_tags(&r, p.id) == [tag_id(&r, forum, "scheduled")])
    })
    .await;
    let post = post_with(&r, forum, "tide check").unwrap().id;
    r.fake.delete_thread(post);
    // The reminder moves, so its post's card has something to say.
    let _: WakeEditResult = rpc(&r)
        .call(
            theseus_protocol::method::WAKE_SNOOZE,
            theseus_protocol::wakes::WakeSnoozeParams {
                wake: wake.clone(),
                when: Some("in 2h".into()),
                fired: false,
                author: None,
            },
        )
        .await
        .unwrap();
    r.until("a new post, scheduled", || {
        post_with(&r, forum, "tide check").is_some_and(|p| {
            p.id != post && last_tags(&r, p.id) == [tag_id(&r, forum, "scheduled")]
        })
    })
    .await;
    assert_eq!(posts_about(&r, forum, "tide check"), 1);
}

/// A series' occurrence that fired is snoozed twice (its notice's button, then
/// the CLI): the series stands, and the occurrence is ONE extra reminder, with
/// one card and one post of its own, moved by the second snooze (the review's
/// N-c; the kernel's link is proven in its own test).
#[tokio::test]
async fn two_snoozes_of_a_fired_series_occurrence_give_one_extra_card_and_one_post() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let forum = forum_up(&r).await;
    let made: WakeEditResult = rpc(&r)
        .call(
            theseus_protocol::method::WAKE_CREATE,
            WakeCreateParams {
                when: "in 3s".into(),
                note: "water the plants".into(),
                every: Some("30m".into()),
                session_id: Some(session_of_place(&r, &format!("dm:{ANA}"))),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let series = made.wake.wake_id;
    r.until("the series' card and post", || {
        card(&r, home, "water the plants").is_some()
            && posts_about(&r, forum, "water the plants") == 1
    })
    .await;
    r.until("the occurrence fired", || {
        !r.ledger("wake.fired").is_empty()
    })
    .await;
    snooze_fired(&r, &series, "in 90m").await;
    r.until("the occurrence's own card and post", || {
        cards(&r, home).len() == 2 && posts_about(&r, forum, "water the plants") == 2
    })
    .await;
    snooze_fired(&r, &series, "in 120m").await;
    // Time for any third card or post to be made.
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(
        pending_notes(&r, "water the plants"),
        2,
        "the series and one extra"
    );
    assert_eq!(cards(&r, home).len(), 2, "{:#?}", cards(&r, home));
    assert_eq!(posts_about(&r, forum, "water the plants"), 2);
}

/// A card made at Discord before a stop of days (its entry never saved) is
/// found after it by its intent, and no second is made: a card never archives,
/// so the intent is not dropped by its age (the review's R3). Discord's nonce
/// window is closed, as the real one is by then.
#[tokio::test]
async fn a_card_made_before_a_stop_of_days_is_found_after_it_not_made_again() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let wake = remind(&r, "tide check", "in 90m").await;
    r.until("its card", || card(&r, home, "tide check").is_some())
        .await;
    let first = card(&r, home, "tide check").unwrap();
    r.fake.set_nonce_window_ms(1);
    tokio::time::sleep(Duration::from_millis(50)).await;
    r.core
        .store
        .put_meta("discord.sched", &Vec::<crate::runtime::sched::Rec>::new())
        .unwrap();
    let nine_days = 9 * 24 * 3_600_000;
    r.core
        .store
        .put_meta(
            "discord.sched_intents",
            &serde_json::json!([{"id": wake, "channel": home,
                                 "at_ms": theseus_protocol::now_unix_ms() - nine_days}]),
        )
        .unwrap();
    let shared = crate::runtime::task_home::daemon_for_tests(&r.core).expect("the binding runs");
    let mut pass = crate::runtime::sched::Worker::fresh(shared);
    let before = r
        .core
        .store
        .shared()
        .scan(0, None, 10000)
        .unwrap()
        .last()
        .unwrap()
        .position;
    assert!(pass.sync().await);
    let writes = r.core.store.shared().scan(before + 1, None, 1000).unwrap();
    let card_saved = writes
        .iter()
        .position(|r| r.key.as_deref() == Some("discord.sched"))
        .unwrap();
    let intent_cleared = writes
        .iter()
        .position(|r| r.key.as_deref() == Some("discord.sched_intents"))
        .unwrap();
    assert!(
        card_saved < intent_cleared,
        "recovery must save the card before forgetting its intent"
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        cards(&r, home).len(),
        1,
        "no second card: {:#?}",
        cards(&r, home)
    );
    assert_eq!(card(&r, home, "tide check").unwrap().id, first.id);
}

#[tokio::test]
async fn reminder_cards_mask_known_values_before_title_clipping() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let value = "vault-reminder-4c8e1f7a92b3";
    r.core.secrets.hold(
        "reminder_token",
        theseus_core::secrets::Secret::new(value.into()),
    );
    let note = format!("{}{}", "x".repeat(110), value);
    let wake = remind(&r, &note, "in 90m").await;
    r.until("the masked reminder", || !cards(&r, home).is_empty())
        .await;
    let sent = cards(&r, home)
        .into_iter()
        .map(|m| m.content)
        .collect::<String>();
    assert!(!sent.contains("vault-rem"), "{sent}");
    assert!(sent.contains("[redacted"), "{sent}");
    let original = cards(&r, home).remove(0);
    press(&r, &original, &format!("hq:cancel:{wake}"), ANA, "ana");
    r.until("the masked card edited", || {
        cards(&r, home)
            .iter()
            .any(|m| m.edits > original.edits && m.content.contains("Cancelled"))
    })
    .await;
    let edited = cards(&r, home).remove(0);
    assert!(!edited.content.contains("vault-rem"), "{}", edited.content);
    assert!(edited.content.contains("[redacted"), "{}", edited.content);
    r.stop();
}
