//! The announcement place (theseus-nuqut), through the stand-in's gateway: with a
//! `[[channel]]` bound `notify = true` the operator lane writes the announcements there
//! and nothing else moves, and with none bound every post keeps the owner's DM, which is
//! what a daemon that has not bound one writes today.
//!
//! The owner's rule, 2026-11-12: *"urgency = main dm"*. To the thread: the morning FYI
//! and the digests, the self notices, the AWS month warning and the Google upkeep note.
//! Staying in the DM: approvals and needs-you asks, the questions and cards waiting on
//! him, retractions, and the day ceiling's warning, which can stop work.

use std::sync::Arc;

use serde_json::json;
use theseus_core::provider::{FakeProvider, Provider};
use theseus_sim::fake_discord::{FakeDiscord, Guild, DEFAULT_GUILD};

use super::{Rig, ANA, ANA_DM, BEN, CY, LAB};

/// The announcement channel, `#theseus-notify`, private to the owner.
const NOTIFY: u64 = 900_000_000_000_000_020;

fn model(_: &std::path::Path, _: Arc<FakeDiscord>) -> Arc<dyn Provider> {
    Arc::new(FakeProvider::scripted(vec![]))
}

/// `#lab`, `#theseus-notify` and ana's DM. `notify` says whether the second
/// channel is bound as the announcement place; without it the file binds the
/// same three places and marks none, which is today's shape.
fn file(notify: bool) -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = true\n\
         [[channel]]\nid = \"{NOTIFY}\"\nname = \"theseus-notify\"\nusers = [\"{ANA}\"]\nprivate = true\nnotify = {notify}\n\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
    )
}

/// The guild: ana owns it, `#lab` and `#theseus-notify` are hers.
fn guild() -> Guild {
    Guild::new(DEFAULT_GUILD, (ANA, "ana"))
        .member(BEN, "ben")
        .member(CY, "cy")
        .private_channel(LAB, "lab", &[ANA])
        .private_channel(NOTIFY, "theseus-notify", &[ANA])
}

async fn rig(notify: bool) -> Rig {
    Rig::start_tweaked(model, guild(), &file(notify), &[], |c| {
        c.places.owner = Some(vec![format!("discord:{ANA}")]);
    })
    .await
}

/// One operator post, as the core stages it: to `OPERATOR_TARGET`, whose
/// channel the binding picks.
fn stage(r: &Rig, body: serde_json::Value) {
    let (a, frame) = r.core.outbox.stage_to_operator(None, body).unwrap();
    r.core.store.append(&frame).unwrap();
    r.core.outbox.posted(&a);
}

/// Wait until `text` is posted in `channel`, then say where every one of the
/// run's posts of that text landed.
async fn landed(r: &Rig, channel: u64, text: &str) {
    r.until(&format!("{text:?} in {channel}"), || {
        r.posted(channel)
            .iter()
            .any(|m| m.versions[0].contains(text))
    })
    .await;
}

fn carries(r: &Rig, channel: u64, text: &str) -> bool {
    r.posted(channel)
        .iter()
        .any(|m| m.versions[0].contains(text))
}

/// The run's posts in `channel` that are not the place's own bind notice,
/// which every bound place says once.
fn notes(r: &Rig, channel: u64) -> usize {
    r.posted(channel)
        .iter()
        .filter(|m| !m.versions[0].contains("Theseus is bound here"))
        .count()
}

/// With the thread bound: the morning FYI, the posted digest, the self digest,
/// a self build's news and the AWS month warning go to `#theseus-notify`, and
/// the needs-you ask, a card, a retraction and the day ceiling's warning stay
/// in ana's DM. Both sides are counted, so an empty run cannot pass.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_a_notify_thread_bound_the_announcements_go_there_and_the_asks_stay_in_the_dm() {
    let r = rig(true).await;
    let announced = [
        ("initiative", "morning: the pier held"),
        ("digest", "digest: two tasks ended"),
        ("self_digest", "self digest: three items started"),
        ("self_build", "self build: the mend is pushed"),
    ];
    for (kind, text) in announced {
        stage(&r, json!({"kind": kind, "text": text, "id": text}));
    }
    // `spend_ceiling` is written by four sites; the two that announce say so
    // on the body (`announce`), and the day ceiling's does not.
    stage(
        &r,
        json!({"kind": "spend_ceiling", "text": "aws month: 80% of the month's budget",
               "announce": true}),
    );
    let in_the_dm = [
        ("needs_you", "needs you: fs.write asks"),
        ("retraction", "retraction: two tasks to run again"),
        ("self_halt", "self-improvement halted"),
        ("spend_ceiling", "day ceiling: the day's calls stop here"),
    ];
    for (kind, text) in in_the_dm {
        stage(
            &r,
            json!({"kind": kind, "text": text, "prompt": text, "question": text}),
        );
    }
    for (_, text) in announced {
        landed(&r, NOTIFY, text).await;
        assert!(
            !carries(&r, ANA_DM, text),
            "{text:?} announced, so it is not in the DM"
        );
    }
    landed(&r, NOTIFY, "aws month").await;
    assert!(!carries(&r, ANA_DM, "aws month"));
    for (_, text) in in_the_dm {
        landed(&r, ANA_DM, text).await;
        assert!(
            !carries(&r, NOTIFY, text),
            "{text:?} needs the owner, so it is not in the thread"
        );
    }
    // The counts, so neither half can pass on an empty set.
    assert_eq!(notes(&r, NOTIFY), announced.len() + 1);
    assert_eq!(notes(&r, ANA_DM), in_the_dm.len());
    r.stop();
}

/// With no `notify = true` anywhere, every one of those posts is in ana's DM
/// and the announcement channel is untouched: a daemon that has bound no
/// announcement place writes exactly what it wrote before.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_no_notify_thread_bound_every_post_keeps_the_owners_dm() {
    let r = rig(false).await;
    let posts = [
        ("initiative", "morning: the pier held"),
        ("self_digest", "self digest: three items started"),
        ("needs_you", "needs you: fs.write asks"),
    ];
    for (kind, text) in posts {
        stage(
            &r,
            json!({"kind": kind, "text": text, "id": text, "prompt": text,
                   "question": text}),
        );
    }
    stage(
        &r,
        json!({"kind": "spend_ceiling", "text": "aws month: 80% of the month's budget",
               "announce": true}),
    );
    for (_, text) in posts {
        landed(&r, ANA_DM, text).await;
    }
    landed(&r, ANA_DM, "aws month").await;
    assert_eq!(notes(&r, ANA_DM), posts.len() + 1);
    assert_eq!(notes(&r, NOTIFY), 0, "the channel took no announcement");
    r.stop();
}

/// `notify here` (theseus-nuqut; the owner's S17-D7: what he binds, he binds by
/// talking) said in `#lab` moves the announcements there with no bindings-file
/// edit, and `notify off` gives them back to his DM. The asks never move.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn notify_here_binds_the_place_from_chat_and_notify_off_gives_it_back() {
    let r = rig(false).await;
    // Before the word: his DM, as today.
    stage(
        &r,
        json!({"kind": "initiative", "text": "before: the DM", "id": "b"}),
    );
    landed(&r, ANA_DM, "before: the DM").await;
    // `notify where` says so, and writes nothing.
    r.say((ANA, "ana"), Some(ANA_DM), "notify where");
    landed(&r, ANA_DM, "The announcements go to your DM").await;
    assert_eq!(r.shared_notify_said(), None, "a read writes nothing");
    // The word, in the channel that should carry them.
    r.say((ANA, "ana"), Some(LAB), "notify here");
    landed(&r, LAB, "The announcements come here now").await;
    assert_eq!(r.shared_notify_said(), Some(LAB));
    stage(
        &r,
        json!({"kind": "initiative", "text": "after: the channel", "id": "a"}),
    );
    landed(&r, LAB, "after: the channel").await;
    assert!(!carries(&r, ANA_DM, "after: the channel"));
    // An ask does not move, whatever is bound.
    stage(
        &r,
        json!({"kind": "needs_you", "text": "needs you: fs.write asks",
               "prompt": "needs you: fs.write asks", "question": "q1"}),
    );
    landed(&r, ANA_DM, "needs you: fs.write asks").await;
    assert!(!carries(&r, LAB, "needs you: fs.write asks"));
    // And back.
    r.say((ANA, "ana"), Some(LAB), "notify off");
    landed(&r, LAB, "go to your DM again").await;
    assert_eq!(r.shared_notify_said(), None);
    stage(
        &r,
        json!({"kind": "digest", "text": "last: the DM again", "id": "l"}),
    );
    landed(&r, ANA_DM, "last: the DM again").await;
    assert!(!carries(&r, LAB, "last: the DM again"));
    r.stop();
}

/// Where the announcements go is the owner's alone: in a shared place the
/// words say so and write nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn in_a_shared_place_the_words_say_it_is_his_alone_and_write_nothing() {
    let shared_lab = file(false).replace(
        &format!("name = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = true"),
        &format!("name = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = false"),
    );
    let r = Rig::start_tweaked(
        model,
        Guild::new(DEFAULT_GUILD, (ANA, "ana"))
            .member(BEN, "ben")
            .member(CY, "cy")
            .channel(LAB, "lab")
            .private_channel(NOTIFY, "theseus-notify", &[ANA]),
        &shared_lab,
        &[],
        |c| {
            c.places.owner = Some(vec![format!("discord:{ANA}")]);
        },
    )
    .await;
    r.say((ANA, "ana"), Some(LAB), "notify here");
    landed(&r, LAB, "yours alone").await;
    assert_eq!(r.shared_notify_said(), None, "nothing was written");
    stage(
        &r,
        json!({"kind": "initiative", "text": "still the DM", "id": "s"}),
    );
    landed(&r, ANA_DM, "still the DM").await;
    assert!(!carries(&r, LAB, "still the DM"));
    r.stop();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn notify_off_overrides_the_file_announcement_channel() {
    let r = rig(true).await;
    r.say((ANA, "ana"), Some(LAB), "notify off");
    landed(&r, LAB, "go to your DM again").await;
    r.say((ANA, "ana"), Some(LAB), "notify where");
    landed(&r, LAB, "The announcements go to your DM.").await;
    stage(
        &r,
        json!({"kind": "digest", "text": "explicit DM announcement", "id": "dm"}),
    );
    landed(&r, ANA_DM, "explicit DM announcement").await;
    assert!(!carries(&r, NOTIFY, "explicit DM announcement"));
    r.stop();
}

/// A channel the bindings file does not name.
const UNBOUND: u64 = 900_000_000_000_000_030;

/// The ontology's mint, as the core words it: an FYI with its Undo.
fn mint(text: &str) -> serde_json::Value {
    use theseus_core::owner_note::{note, Kind};
    json!({"kind": "onto_card", "write": "jdg_mint", "undo": true,
           "text": note(Kind::Fyi, "Theseus started keeping track of something new",
                        "In \"pier\", this topic came up and Theseus had no record of it:",
                        &format!("{text}\nUndo: `theseus ontology undo jdg_mint`"))})
}

/// An FYI the core stages as a plain notice (a sweep's stop, the persona move).
fn fyi_notice(text: &str) -> serde_json::Value {
    use theseus_core::owner_note::{note, Kind};
    json!({"kind": "notice", "text": note(Kind::Fyi, "The tide sweep has stopped for today",
                                         "its day budget", text)})
}

/// The disk came back: an FYI the binding words itself.
fn disk_back() -> serde_json::Value {
    json!({"kind": "disk", "state": "ok", "free_mb": 90_000, "total_mb": 100_000,
           "warn_mb": 5_120, "floor_mb": 1_024})
}

/// With the notices place bound, an FYI that answers no conversation goes there
/// and not to the DM, the mint keeping its Undo: the note the owner named, a plain
/// FYI notice, one the binding words itself, and capture's kept-standing note (an FYI
/// since issue 57, its Undo with it). A question, an alert and a card stay in the
/// DM. Both sides are counted, so an empty run cannot pass.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_a_notices_place_bound_an_fyi_goes_there_and_what_needs_the_owner_stays_in_the_dm() {
    use theseus_core::owner_note::{note, Kind};
    let r = rig(true).await;
    stage(&r, mint("fyi-mint-pier"));
    stage(&r, fyi_notice("fyi-sweep-stopped"));
    stage(&r, disk_back());
    let ask = note(
        Kind::Decision,
        "Is this a topic?",
        "a word came up",
        "dm-onto-ask",
    );
    stage(
        &r,
        json!({"kind": "onto_card", "write": "jdg_ask", "undo": false, "text": ask}),
    );
    let alert = note(
        Kind::Alert,
        "A hand ran away",
        "its spend",
        "dm-hands-alert",
    );
    stage(&r, json!({"kind": "hands_alert", "text": alert}));
    let retraction = note(
        Kind::Decision,
        "Run two tasks again?",
        "a retraction",
        "dm-retraction",
    );
    stage(&r, json!({"kind": "retraction", "text": retraction}));
    // Capture's kept record is an FYI: it moves (issue 57). Its ask, a Decision, would not.
    let kept = theseus_core::capture::kept_card("fyi-kept-standing");
    stage(
        &r,
        json!({"kind": "standing_card", "text": kept, "card": {"mode": "kept", "id": "std_1"}}),
    );
    let moved = [
        "fyi-mint-pier",
        "fyi-sweep-stopped",
        "back to normal",
        "fyi-kept-standing",
    ];
    for text in moved {
        landed(&r, NOTIFY, text).await;
        assert!(
            !carries(&r, ANA_DM, text),
            "{text:?} only informs: not in the DM"
        );
    }
    let kept_dm = ["dm-onto-ask", "dm-hands-alert", "dm-retraction"];
    for text in kept_dm {
        landed(&r, ANA_DM, text).await;
        assert!(!carries(&r, NOTIFY, text), "{text:?} stays in the DM");
    }
    let posted = r.posted(NOTIFY);
    let mint = posted
        .iter()
        .find(|m| m.versions[0].contains("fyi-mint-pier"))
        .unwrap();
    assert!(mint.versions[0].contains("started keeping track of something new"));
    let labels: Vec<&str> = mint.buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, ["Undo"], "the mint keeps its undo: {mint:?}");
    assert_eq!(mint.buttons[0].custom_id, "onto:undo:jdg_mint");
    assert_eq!(notes(&r, NOTIFY), moved.len());
    assert_eq!(notes(&r, ANA_DM), kept_dm.len());
    r.stop();
}

/// With no notices place bound, every FYI is in the owner's DM, as before.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_no_notices_place_bound_an_fyi_keeps_the_owners_dm() {
    let r = rig(false).await;
    stage(&r, mint("fyi-mint-pier"));
    stage(&r, fyi_notice("fyi-sweep-stopped"));
    stage(&r, disk_back());
    for text in ["fyi-mint-pier", "fyi-sweep-stopped", "back to normal"] {
        landed(&r, ANA_DM, text).await;
    }
    assert_eq!(notes(&r, ANA_DM), 3);
    assert_eq!(notes(&r, NOTIFY), 0, "the channel took no FYI");
    r.stop();
}

/// The place named from chat is one the bindings no longer name: the binding
/// cannot post there, so an FYI falls back to the owner's DM and is not lost.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_fyi_whose_notices_place_is_not_bound_falls_back_to_the_dm() {
    let r = Rig::start_tweaked(
        model,
        guild().private_channel(UNBOUND, "gone", &[ANA]),
        &file(false),
        &[],
        |c| {
            c.places.owner = Some(vec![format!("discord:{ANA}")]);
        },
    )
    .await;
    r.core
        .store
        .put_meta("discord.notify_place", &UNBOUND.to_string())
        .unwrap();
    assert_eq!(r.shared_notify_said(), Some(UNBOUND));
    stage(&r, mint("fyi-mint-pier"));
    landed(&r, ANA_DM, "fyi-mint-pier").await;
    assert!(!carries(&r, UNBOUND, "fyi-mint-pier"));
    assert_eq!(notes(&r, NOTIFY), 0);
    r.stop();
}
