//! The task home (theseus-xlxp4) and what rides with it (theseus-lxfz0,
//! theseus-90qqy), end to end: a real core with the work engine, the binding
//! on the stand-in's REST and gateway, and the stand-in model that starts
//! tasks (the rig of `tests_default_forum.rs`; the guild is trusted, so the
//! forum and the home act).

use std::path::Path;

use serde_json::{json, Value};
use theseus_core::provider::Scripted;
use theseus_protocol::threads::ThreadNewParams;
use theseus_sim::fake_discord::{Msg, BOT_ID, DEFAULT_GUILD};

use crate::render::board_v2;
use crate::runtime::default_forum::{BOT_BITS, OWNER_BITS};
use crate::tests_default_forum::{
    bindings, cli, dm_lines, forum_up, guild, post_with, posts, rig_in, REEL,
};
use crate::tests_gateway::{Rig, ANA, ANA_DM, LAB};
use crate::tests_task_threads::{
    answers, gate, held, open_gate, parent_session, replied, start, task_session, until, ASK, BUOY,
    REEF,
};

const V2: u64 = 1 << 15;

/// The default forum's rig with the task home on, as the config has it by
/// default, and `cfg` after.
async fn rig(
    parent: Vec<Scripted>,
    kids: impl FnOnce(&Path) -> Vec<(&'static str, Vec<Scripted>)>,
    binds: &str,
    setup: impl FnOnce(&theseus_sim::fake_discord::FakeDiscord),
    cfg: impl FnOnce(&mut theseus_core::Config),
) -> Rig {
    rig_in(guild(false), parent, kids, binds, setup, |c| {
        c.discord.task_home = "theseus-tasks".into();
        cfg(c);
    })
    .await
}

/// The home's id, from its META record.
fn home_record(r: &Rig) -> Option<u64> {
    let v: Option<Value> = r.core.store.get_meta("discord.task_home").ok().flatten();
    v?["channel"].as_u64()
}

/// The home is recorded (`find_task_home`) and stands: its state holds the channel, which `instead_of`
/// reads, and the core was told, in that order, so the DM lane no longer draws the board (theseus-0prmm).
async fn home_up(r: &Rig) -> u64 {
    let dm = format!("discord:dm:{ANA}");
    until(r, "the task home stands", || {
        home_record(r).is_some() && r.core.outbox.report_target(Some(dm.clone())).1.is_some()
    })
    .await;
    home_record(r).unwrap()
}

/// The bot's V2 messages in `channel` that carry the board's Clear button.
fn boards(r: &Rig, channel: u64) -> Vec<Msg> {
    r.posted(channel)
        .into_iter()
        .filter(|m| m.flags & V2 != 0 && m.buttons.iter().any(|b| b.custom_id == board_v2::CLEAR))
        .collect()
}

/// The bot's V2 messages in `channel` with an answer button: approval cards.
fn cards(r: &Rig, channel: u64) -> Vec<Msg> {
    r.posted(channel)
        .into_iter()
        .filter(|m| m.buttons.iter().any(|b| b.custom_id.starts_with("wa:")))
        .collect()
}

/// A report in `channel` that says `last`.
fn report(r: &Rig, channel: u64, last: &str) -> Option<Msg> {
    r.posted(channel)
        .into_iter()
        .find(|m| m.content.contains("finished") && m.content.contains(last))
}

/// A task that waits at a gate until the test lets it go, then writes outside
/// the roots, so it asks for a card, and says `Written.`.
fn gated_asks(dir: &Path, name: &str) -> Vec<Scripted> {
    let path = dir.join("outside").join(name);
    vec![
        gate(dir),
        Scripted::tools(
            "",
            &[(
                "w1",
                "fs_write",
                json!({"path": path.to_string_lossy(), "content": name}),
            )],
        ),
        Scripted::text("Written."),
    ]
}

/// `#reel` runs the thread `tides`, so that a task started there has an index
/// card for its place.
async fn tides(r: &Rig) {
    r.core
        .thread_new(
            &ThreadNewParams {
                slug: "tides".into(),
                title: Some("Tides".into()),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    until(r, "#reel runs the thread", || {
        r.core
            .threads()
            .get("tides")
            .is_some_and(|t| !t.surfaces.is_empty())
    })
    .await;
}

/// `#lab` as a voice binding: a place with no thread home.
fn voice_lab() -> String {
    bindings(true, "voice = true\n")
}

/// A task started from a voice place gets its forum post, reports in the task
/// home with its post named, asks in its post, and posts nothing in the voice
/// channel's chat: no board, no report, no card, no pointer.
#[tokio::test]
async fn a_voice_places_task_posts_nothing_in_the_voice_chat() {
    let r = rig(
        vec![start(&[REEF]), Scripted::text("Started.")],
        |dir| vec![(REEF, gated_asks(dir, "reef.txt"))],
        &voice_lab(),
        |_| {},
        |_| {},
    )
    .await;
    let forum = forum_up(&r).await;
    let home = home_up(&r).await;
    r.say((ANA, "ana"), Some(LAB), ASK);
    until(&r, "the task's post", || {
        post_with(&r, forum, "Reef-chart").is_some()
    })
    .await;
    let post = post_with(&r, forum, "Reef-chart").unwrap();
    let session = task_session(&r);
    // Its cards go to its post, and its report to the home, in the core's own
    // records.
    until(&r, "the task answers at its post", || {
        r.core.outbox.target(&session).as_deref()
            == Some(format!("discord:channel:{}", post.id).as_str())
    })
    .await;
    let exec = r.core.kernel.tasks(None).unwrap().remove(0);
    assert_eq!(
        exec.reports_to.as_deref(),
        Some(format!("discord:channel:{home}").as_str()),
        "the record says where the report goes"
    );
    // The start tool's result says so, as a fact.
    assert!(
        held(&r, &parent_session(&r))
            .iter()
            .any(|b| b.contains("reports to #theseus-tasks and its forum post when it finishes")),
        "{:#?}",
        held(&r, &parent_session(&r))
    );
    open_gate(&r);
    until(&r, "its card in its post", || {
        !cards(&r, post.id).is_empty()
    })
    .await;
    let card = cards(&r, post.id).remove(0);
    r.press(&card.id, "Approve", (ANA, "ana"));
    until(&r, "its report in the task home", || {
        report(&r, home, "Written.").is_some()
    })
    .await;
    let first = report(&r, home, "Written.").unwrap();
    let first = first.content.lines().next().unwrap_or_default().to_string();
    assert!(first.contains(&format!("<#{}>", post.id)), "{first}");
    until(&r, "the board in the home", || !boards(&r, home).is_empty()).await;
    assert_eq!(boards(&r, home).len(), 1, "one board");
    let pointer = format!("🧵 <#{}>", post.id);
    let pointers: Vec<Msg> = r
        .posted(home)
        .into_iter()
        .filter(|m| m.content.contains(&pointer))
        .collect();
    assert_eq!(pointers.len(), 1, "one pointer: {pointers:#?}");
    assert!(pointers[0].silent() && pointers[0].mentions.is_empty());
    // Nothing of the task's is in the voice channel's chat: the conversation's
    // own lines are all it holds.
    assert!(boards(&r, LAB).is_empty(), "no board in the voice chat");
    assert!(cards(&r, LAB).is_empty(), "no card in the voice chat");
    for m in r.posted(LAB) {
        assert!(
            m.flags & V2 == 0
                && !m.content.contains("📋")
                && !m.content.contains("🧵")
                && !m.content.contains(&post.id.to_string()),
            "the voice chat holds no task post: {m:#?}"
        );
    }
}

/// The home is made once, private, with the owner's and the bot's overwrites;
/// a start on the same store finds it, by its record and, when the record
/// does not hold, by name; a refusal is one line to the owner.
#[tokio::test]
async fn the_task_home_is_made_once_and_found_after_a_restart() {
    let r = rig(vec![], |_| vec![], &bindings(true, ""), |_| {}, |_| {}).await;
    let home = home_up(&r).await;
    let made = || {
        r.fake
            .made_channels()
            .into_iter()
            .filter(|c| c.kind == 0 && c.name == "theseus-tasks")
            .collect::<Vec<_>>()
    };
    assert_eq!(made().len(), 1, "{:#?}", made());
    let c = &made()[0];
    assert_eq!(c.id, home);
    assert!(c.parent.is_none());
    let of = |id: u64| {
        c.overwrites
            .iter()
            .find(|o| o.id == id)
            .unwrap_or_else(|| panic!("{id}"))
    };
    assert_eq!(c.overwrites.len(), 3, "everyone, the owner, the bot");
    let everyone = of(DEFAULT_GUILD);
    assert_eq!((everyone.kind, everyone.deny), (0, 1024));
    assert_eq!(of(ANA).allow, OWNER_BITS);
    assert_eq!(of(BOT_ID).allow, BOT_BITS);
    // A start on the same store finds it by its record.
    let again = crate::runtime::shared_for_tests(&r.core);
    let guild = DEFAULT_GUILD.to_string();
    assert_eq!(
        again.look_for_task_home(&guild, "theseus-tasks").await,
        Some(home)
    );
    assert_eq!(made().len(), 1);
    // A record that does not hold (another name was asked for): by name.
    r.core
        .store
        .put_meta(
            "discord.task_home",
            &json!({"guild": guild, "channel": 7, "wanted": "other"}),
        )
        .unwrap();
    let third = crate::runtime::shared_for_tests(&r.core);
    assert_eq!(
        third.look_for_task_home(&guild, "theseus-tasks").await,
        Some(home)
    );
    assert_eq!(made().len(), 1, "no second channel");
}

/// A bot that may not make channels says so once and the home is off.
#[tokio::test]
async fn a_refused_task_home_is_one_line_to_the_owner() {
    let r = rig(
        vec![],
        |_| vec![],
        &bindings(true, ""),
        |f| f.refuse_channels(true),
        |_| {},
    )
    .await;
    until(&r, "the owner's line", || dm_lines(&r, "task channel") == 1).await;
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert_eq!(dm_lines(&r, "task channel"), 1);
    assert_eq!(home_record(&r), None);
    assert!(r.fake.made_channels().is_empty());
}

/// Two DM tasks: one pinned board in the home, their two reports there, one
/// quiet pointer each; the DM keeps the conversation and nothing else.
#[tokio::test]
async fn the_task_home_holds_the_board_reports_and_one_pointer_per_task() {
    let r = rig(
        vec![start(&[REEF, BUOY]), Scripted::text("Started both.")],
        |dir| {
            vec![
                (REEF, [vec![gate(dir)], answers("Reef charted.")].concat()),
                (BUOY, [vec![gate(dir)], answers("Buoy painted.")].concat()),
            ]
        },
        &bindings(true, ""),
        |_| {},
        |_| {},
    )
    .await;
    let forum = forum_up(&r).await;
    let home = home_up(&r).await;
    r.say((ANA, "ana"), None, ASK);
    until(&r, "two posts", || posts(&r, forum).len() == 2).await;
    replied(&r, ANA_DM, "Started both.").await;
    until(&r, "the board in the home", || !boards(&r, home).is_empty()).await;
    open_gate(&r);
    until(&r, "both reports in the home", || {
        report(&r, home, "Reef charted.").is_some() && report(&r, home, "Buoy painted.").is_some()
    })
    .await;
    until(&r, "the board is pinned", || boards(&r, home)[0].pinned).await;
    assert_eq!(boards(&r, home).len(), 1, "one board");
    assert!(boards(&r, home)[0].silent(), "the board never pings");
    for p in posts(&r, forum) {
        let line = format!("🧵 <#{}>", p.id);
        let n = r
            .posted(home)
            .iter()
            .filter(|m| m.content.contains(&line))
            .count();
        assert_eq!(n, 1, "one pointer to {}", p.id);
    }
    assert!(r
        .posted(home)
        .iter()
        .filter(|m| m.content.contains("🧵"))
        .all(|m| m.silent() && m.mentions.is_empty()));
    // The DM holds the conversation alone.
    for m in r.posted(ANA_DM) {
        assert!(
            m.flags & V2 == 0
                && !m.content.contains("finished")
                && !m.content.contains("🧵")
                && !m.content.contains("📋"),
            "the DM holds the conversation: {m:#?}"
        );
    }
    let dm = r
        .core
        .outbox
        .place_session(&format!("dm:{ANA}"))
        .unwrap()
        .unwrap();
    assert!(
        held(&r, &dm)
            .iter()
            .any(|b| b.contains("reports to #theseus-tasks and its forum post when it finishes")),
        "{:#?}",
        held(&r, &dm)
    );
    // The core's records say the reports went to the home.
    for t in r.core.kernel.tasks(None).unwrap() {
        assert_eq!(
            t.reports_to.as_deref(),
            Some(format!("discord:channel:{home}").as_str())
        );
    }
}

/// With `task_home = ""` the DM has its board, its reports and its pointers as
/// it did, and no channel is made.
#[tokio::test]
async fn task_home_off_is_todays_dm() {
    let r = rig(
        vec![start(&[REEF]), Scripted::text("Started.")],
        |dir| vec![(REEF, [vec![gate(dir)], answers("Reef charted.")].concat())],
        &bindings(true, ""),
        |_| {},
        |c| c.discord.task_home = String::new(),
    )
    .await;
    let forum = forum_up(&r).await;
    r.say((ANA, "ana"), None, ASK);
    until(&r, "the post", || posts(&r, forum).len() == 1).await;
    until(&r, "the board in the DM", || !boards(&r, ANA_DM).is_empty()).await;
    open_gate(&r);
    until(&r, "the report in the DM", || {
        report(&r, ANA_DM, "Reef charted.").is_some()
    })
    .await;
    until(&r, "the board is pinned", || boards(&r, ANA_DM)[0].pinned).await;
    assert_eq!(boards(&r, ANA_DM).len(), 1);
    assert_eq!(dm_lines(&r, "🧵"), 1, "the DM's own pointer, as today");
    assert!(r
        .fake
        .made_channels()
        .iter()
        .all(|c| c.name != "theseus-tasks"));
    assert_eq!(home_record(&r), None);
    let t = r.core.kernel.tasks(None).unwrap().remove(0);
    assert_eq!(
        t.reports_to.as_deref(),
        Some(format!("discord:dm:{ANA}").as_str())
    );
}

/// A task of an activity thread has an index card: its card is its post's, not
/// the thread's channel's, and no one is mentioned in the channel.
#[tokio::test]
async fn a_tasks_card_goes_to_its_post_not_the_channel() {
    let r = rig(
        vec![start(&[REEF]), Scripted::text("Started this.")],
        |dir| vec![(REEF, gated_asks(dir, "reef.txt"))],
        &bindings(true, ""),
        |_| {},
        |_| {},
    )
    .await;
    let forum = forum_up(&r).await;
    tides(&r).await;
    r.say((ANA, "ana"), Some(REEL), ASK);
    until(&r, "its index card", || {
        post_with(&r, forum, "Reef-chart").is_some()
    })
    .await;
    let post = post_with(&r, forum, "Reef-chart").unwrap();
    let session = task_session(&r);
    until(&r, "the task answers at its post", || {
        r.core.outbox.target(&session).as_deref()
            == Some(format!("discord:channel:{}", post.id).as_str())
    })
    .await;
    open_gate(&r);
    until(&r, "the card in the post", || {
        !cards(&r, post.id).is_empty()
    })
    .await;
    replied(&r, REEL, "Started this.").await;
    assert!(cards(&r, REEL).is_empty(), "no card in the channel");
    assert!(
        r.posted(REEL).iter().all(|m| m.mentions.is_empty()),
        "no one is mentioned in the channel: {:#?}",
        r.posted(REEL)
    );
}

/// With the home off a voice place's task reports to the owner's DM: its report
/// is there, naming its post; the voice chat holds none of it.
#[tokio::test]
async fn a_voice_places_task_reports_to_the_dm_when_the_home_is_off() {
    let r = rig(
        vec![start(&[REEF]), Scripted::text("Started.")],
        |dir| vec![(REEF, [vec![gate(dir)], answers("Reef charted.")].concat())],
        &voice_lab(),
        |_| {},
        |c| c.discord.task_home = String::new(),
    )
    .await;
    let forum = forum_up(&r).await;
    r.say((ANA, "ana"), Some(LAB), ASK);
    until(&r, "the task's post", || {
        post_with(&r, forum, "Reef-chart").is_some()
    })
    .await;
    let post = post_with(&r, forum, "Reef-chart").unwrap();
    assert!(
        held(&r, &parent_session(&r))
            .iter()
            .any(|b| b.contains("reports to the DM and its forum post when it finishes")),
        "{:#?}",
        held(&r, &parent_session(&r))
    );
    open_gate(&r);
    until(&r, "its report in the DM", || {
        report(&r, ANA_DM, "Reef charted.").is_some()
    })
    .await;
    let first = report(&r, ANA_DM, "Reef charted.").unwrap();
    let first = first.content.lines().next().unwrap_or_default().to_string();
    assert!(first.contains(&format!("<#{}>", post.id)), "{first}");
    let exec = r.core.kernel.tasks(None).unwrap().remove(0);
    assert_eq!(
        exec.reports_to.as_deref(),
        Some(format!("discord:dm:{ANA}").as_str())
    );
    assert!(boards(&r, LAB).is_empty() && report(&r, LAB, "Reef charted.").is_none());
}

/// The DM drew its board before the home answered (here: the home was off at
/// the start and the binding looks for it afterwards, as a start whose first
/// look failed would); once the home stands the DM's board is gone and the
/// board is drawn at the home (theseus-ysp2k; the place's `BoardMoved`).
#[tokio::test]
async fn the_home_standing_removes_the_dms_board() {
    let r = rig(
        vec![start(&[REEF]), Scripted::text("Started.")],
        |dir| vec![(REEF, [vec![gate(dir)], answers("Reef charted.")].concat())],
        &bindings(true, ""),
        |_| {},
        |c| c.discord.task_home = String::new(),
    )
    .await;
    let forum = forum_up(&r).await;
    r.say((ANA, "ana"), None, ASK);
    until(&r, "the post", || posts(&r, forum).len() == 1).await;
    until(&r, "the board in the DM", || !boards(&r, ANA_DM).is_empty()).await;
    let dm_before: Vec<String> = r.posted(ANA_DM).into_iter().map(|m| m.id).collect();
    let shared = crate::runtime::task_home::daemon_for_tests(&r.core).expect("the binding runs");
    let home = shared
        .look_for_task_home(&DEFAULT_GUILD.to_string(), "theseus-tasks")
        .await
        .expect("the home stands");
    until(&r, "the DM's board is gone", || {
        boards(&r, ANA_DM).is_empty()
    })
    .await;
    until(&r, "the board at the home", || !boards(&r, home).is_empty()).await;
    let kept: Vec<String> = r
        .posted(ANA_DM)
        .into_iter()
        .map(|m| m.id)
        .filter(|id| dm_before.contains(id))
        .collect();
    assert!(
        kept.len() + 1 >= dm_before.len(),
        "only the board left the DM: {dm_before:?} -> {kept:?}"
    );
    open_gate(&r);
}
