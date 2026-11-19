//! The task home (theseus-xlxp4) and what rides with it (theseus-lxfz0,
//! theseus-90qqy), end to end: a real core with the work engine, the binding
//! on the stand-in's REST and gateway, and the stand-in model that starts
//! tasks (the rig of `tests_default_forum.rs`; the guild is trusted, so the
//! forum and the home act).

use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_core::approval::{Client, Surface};
use theseus_core::provider::{FakeProvider, Scripted};
use theseus_protocol::threads::ThreadNewParams;
use theseus_protocol::wakes::{WakeCreateParams, WakeEditResult};
use theseus_sim::fake_discord::{Msg, BOT_ID, DEFAULT_GUILD};

use crate::render::board_v2;
use crate::rpc_client::RpcClient;
use crate::runtime::default_forum::{BOT_BITS, OWNER_BITS};
use crate::tests_default_forum::{
    bindings, cli, dm_lines, forum_up, guild, post_with, posts, rig_in, tune, REEL,
};
use crate::tests_gateway::{Rig, ANA, ANA_DM, LAB};
use crate::tests_task_threads::{
    answers, gate, held, open_gate, parent_session, replied, start, task_session, until, ASK, BUOY,
    REEF,
};

const V2: u64 = 1 << 15;

/// The default forum's rig with the task home on, as the config has it by
/// default, and `cfg` after.
pub(crate) async fn rig(
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
pub(crate) fn home_record(r: &Rig) -> Option<u64> {
    let v: Option<Value> = r.core.store.get_meta("discord.task_home").ok().flatten();
    v?["channel"].as_u64()
}

/// The home is recorded (`find_task_home`) and stands: its state holds the channel, which `instead_of`
/// reads, and the core was told, in that order, so the DM lane no longer draws the board (theseus-0prmm).
pub(crate) async fn home_up(r: &Rig) -> u64 {
    let dm = format!("discord:dm:{ANA}");
    until(r, "the task home stands", || {
        home_record(r).is_some() && r.core.outbox.report_target(Some(dm.clone())).1.is_some()
    })
    .await;
    home_record(r).unwrap()
}

/// The card of a scheduled item in `channel`: a V2 message that is not the
/// board, whose words hold `needle` (issue 30).
pub(crate) fn card_in(r: &Rig, channel: u64, needle: &str) -> Option<Msg> {
    r.posted(channel)
        .into_iter()
        .find(|m| m.flags & V2 != 0 && !is_board(m) && m.content.contains(needle))
}

/// Every card of a scheduled item in `channel`.
pub(crate) fn cards_in(r: &Rig, channel: u64) -> Vec<Msg> {
    r.posted(channel)
        .into_iter()
        .filter(|m| m.flags & V2 != 0 && !is_board(m))
        .collect()
}

fn is_board(m: &Msg) -> bool {
    m.buttons.iter().any(|b| b.custom_id == board_v2::CLEAR)
}

/// The bot's V2 messages in `channel` that carry the board's Clear button.
pub(crate) fn boards(r: &Rig, channel: u64) -> Vec<Msg> {
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
    let board = boards(&r, ANA_DM)[0].id.clone();
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
    let after: Vec<String> = r.posted(ANA_DM).into_iter().map(|m| m.id).collect();
    assert_eq!(only_the_board_left(&dm_before, &board, &after), Ok(()));
    open_gate(&r);
}

/// Whether every message of the DM but the board's `board` is still there,
/// and the board is not: the first thing that differs, if not.
fn only_the_board_left(before: &[String], board: &str, after: &[String]) -> Result<(), String> {
    if !before.iter().any(|id| id == board) {
        return Err(format!("{board} was not in {before:?}"));
    }
    if after.iter().any(|id| id == board) {
        return Err(format!("{board} is not gone: {after:?}"));
    }
    let want: Vec<&String> = before.iter().filter(|id| *id != board).collect();
    let kept: Vec<&String> = after.iter().filter(|id| before.contains(id)).collect();
    if kept == want {
        return Ok(());
    }
    Err(format!(
        "only the board left the DM: {before:?} -> {after:?}"
    ))
}

/// The check names the board: one other message gone fails it, and so does
/// the board staying while another message goes (the old check,
/// `kept + 1 >= before`, passed that) (theseus-4v88e).
#[test]
fn the_check_names_the_board_and_keeps_every_other_message() {
    let ids = |v: &[&str]| v.iter().map(ToString::to_string).collect::<Vec<_>>();
    let before = ids(&["a", "b", "c"]);
    assert_eq!(only_the_board_left(&before, "b", &ids(&["a", "c"])), Ok(()));
    assert!(only_the_board_left(&before, "b", &ids(&["a"])).is_err());
    assert!(only_the_board_left(&before, "b", &ids(&["b", "c"])).is_err());
}

/// A restart end to end: the first run drew the DM's board before any home
/// stood (the home was off) and ended; the next run, started on the same
/// Discord with the home on, finds its DM lane in place when the home answers,
/// removes the earlier run's pinned board, and touches nothing else in the DM
/// (theseus-4v88e).
#[tokio::test]
async fn a_restart_removes_the_earlier_runs_dm_board_once_the_home_stands() {
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
    let board = boards(&r, ANA_DM)[0].id.clone();
    let before: Vec<String> = r.posted(ANA_DM).into_iter().map(|m| m.id).collect();
    // The first process ends: it takes no event and posts nothing more.
    r.stop();
    let again = Rig::start_over(
        r.fake.clone(),
        |_, _| Arc::new(FakeProvider::scripted(vec![])),
        &bindings(true, ""),
        &[
            format!("channel:{}", crate::tests_default_forum::OPS),
            format!("channel:{REEL}"),
        ],
        |c| {
            tune(c);
            c.discord.task_home = "theseus-tasks".into();
        },
    )
    .await;
    let home = home_up(&again).await;
    until(&again, "the earlier board is gone", || {
        !again.posted(ANA_DM).iter().any(|m| m.id == board)
    })
    .await;
    let after: Vec<String> = again.posted(ANA_DM).into_iter().map(|m| m.id).collect();
    assert_eq!(only_the_board_left(&before, &board, &after), Ok(()));
    assert!(
        boards(&again, ANA_DM).is_empty(),
        "no board drawn in the DM"
    );
    // The home is the channel the config named, made once, and not the DM.
    let named: Vec<u64> = again
        .fake
        .made_channels()
        .into_iter()
        .filter(|c| c.kind == 0 && c.name == "theseus-tasks")
        .map(|c| c.id)
        .collect();
    assert_eq!(
        named,
        [home],
        "the home is the one channel the config named"
    );
    again.stop();
}

// ---- where the future is shown (issue 30) -----------------------------------

/// A rig whose DM and `#lab` each have a conversation (a word said in each),
/// `#lab` and `#reel` private beside the DM, the home as `home` says.
pub(crate) async fn two_places(home: &str) -> Rig {
    let home = home.to_string();
    let r = rig(
        vec![Scripted::text("Hello."), Scripted::text("Hello.")],
        |_| vec![],
        &bindings(true, ""),
        |_| {},
        move |c| c.discord.task_home = home,
    )
    .await;
    r.say((ANA, "ana"), None, "hello");
    until(&r, "the DM answered", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains("Hello."))
    })
    .await;
    r.say((ANA, "ana"), Some(LAB), "hello");
    until(&r, "#lab answered", || {
        r.posted(LAB).iter().any(|m| m.content.contains("Hello."))
    })
    .await;
    r
}

/// A reminder `note` in the conversation of `session`, as `wake.create` makes it.
pub(crate) async fn remind_in(r: &Rig, session: &str, note: &str) -> String {
    let rpc = RpcClient::connect(r.core.clone(), Client::new("test", Surface::Cli)).0;
    let made: WakeEditResult = rpc
        .call(
            theseus_protocol::method::WAKE_CREATE,
            WakeCreateParams {
                when: "in 90m".into(),
                note: note.into(),
                session_id: Some(session.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    made.wake.wake_id
}

pub(crate) fn session_of_place(r: &Rig, place: &str) -> String {
    r.core
        .outbox
        .place_session(place)
        .unwrap()
        .expect("the place's session")
}

/// With the home standing, a reminder set in the DM or in a channel is a card
/// of its own in the home, and on no other place's board, and no other place is
/// given a board; the home's board does not list it.
#[tokio::test]
async fn a_reminder_is_on_the_task_channels_board_alone() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    remind_in(
        &r,
        &session_of_place(&r, &format!("dm:{ANA}")),
        "dm tide check",
    )
    .await;
    remind_in(
        &r,
        &session_of_place(&r, &format!("channel:{LAB}")),
        "lab net mending",
    )
    .await;
    until(&r, "both on cards in the home", || {
        card_in(&r, home, "dm tide check").is_some()
            && card_in(&r, home, "lab net mending").is_some()
    })
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    for (place, name) in [(ANA_DM, "the DM"), (LAB, "#lab"), (REEL, "#reel")] {
        assert!(
            boards(&r, place).is_empty(),
            "{name} has a board: {:#?}",
            r.posted(place)
        );
    }
    assert_eq!(cards_in(&r, home).len(), 2, "{:#?}", r.posted(home));
}

/// With no home, a place shows the reminders of its own conversations: a
/// reminder set in `#lab` is on `#lab`'s board and not on the DM's.
#[tokio::test]
async fn without_a_task_channel_a_reminder_is_on_its_own_conversations_board() {
    let r = two_places("").await;
    remind_in(
        &r,
        &session_of_place(&r, &format!("channel:{LAB}")),
        "lab net mending",
    )
    .await;
    until(&r, "the reminder on #lab's board", || {
        boards(&r, LAB)
            .first()
            .is_some_and(|b| b.content.contains("lab net mending"))
    })
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    assert!(boards(&r, ANA_DM).is_empty(), "{:#?}", r.posted(ANA_DM));
    assert!(boards(&r, REEL).is_empty());
}

/// A board a place drew while it showed the future is edited once, in place,
/// to its true state when the task channel comes to stand: no "Coming up" is
/// left on it, and it is the same message.
#[tokio::test]
async fn a_board_drawn_before_the_task_channel_stood_is_edited_to_all_clear() {
    let r = two_places("").await;
    remind_in(
        &r,
        &session_of_place(&r, &format!("channel:{LAB}")),
        "lab net mending",
    )
    .await;
    until(&r, "the reminder on #lab's board", || {
        boards(&r, LAB)
            .first()
            .is_some_and(|b| b.content.contains("lab net mending"))
    })
    .await;
    let before = boards(&r, LAB)[0].id.clone();
    let shared = crate::runtime::task_home::daemon_for_tests(&r.core).expect("the binding runs");
    let home = shared
        .look_for_task_home(&DEFAULT_GUILD.to_string(), "theseus-tasks")
        .await
        .expect("the home stands");
    until(&r, "#lab's board no longer shows the reminder", || {
        boards(&r, LAB)
            .first()
            .is_some_and(|b| !b.content.contains("lab net mending"))
    })
    .await;
    let b = boards(&r, LAB);
    assert_eq!(
        (b.len(), b[0].id.as_str()),
        (1, before.as_str()),
        "the same board, edited"
    );
    assert!(
        b[0].content.contains("all clear") && !b[0].content.contains("Coming up"),
        "{}",
        b[0].content
    );
    until(&r, "the reminder's card in the home", || {
        card_in(&r, home, "lab net mending").is_some()
    })
    .await;
}

/// On the task channel's card, only the owner's press cancels a reminder: a
/// press by anyone else is refused where every press on a place is, and
/// writes nothing.
#[tokio::test]
async fn only_the_owners_press_cancels_a_reminder_on_the_task_channels_board() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    let wake = remind_in(
        &r,
        &session_of_place(&r, &format!("dm:{ANA}")),
        "dm tide check",
    )
    .await;
    until(&r, "the reminder's card in the home", || {
        card_in(&r, home, "dm tide check").is_some()
    })
    .await;
    let card = card_in(&r, home, "dm tide check").unwrap();
    let press = |user: u64, name: &str| {
        r.fake
            .press(&theseus_sim::fake_discord::Pressed {
                message: &card.id,
                button: &format!("hq:cancel:{wake}"),
                user,
                name,
            })
            .unwrap()
    };
    let due = |r: &Rig| {
        r.core
            .kernel
            .pending_wakes()
            .unwrap()
            .into_iter()
            .any(|(_, w)| w.id == wake)
    };
    let refused = press(crate::tests_gateway::BEN, "ben");
    until(&r, "ben's press refused", || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(refused.as_str())
                && x.content
                    .as_deref()
                    .is_some_and(|c| c.contains("Only the people"))
        })
    })
    .await;
    assert!(due(&r), "ben's press cancelled nothing");
    press(ANA, "ana");
    until(&r, "ana's press cancels it", || !due(&r)).await;
    until(&r, "the card says it was cancelled", || {
        card_in(&r, home, "dm tide check").is_some_and(|c| c.content.contains("Cancelled"))
    })
    .await;
}

/// The bindings of `bindings(true, "")` with no `[[dm]]`: channels only.
pub(crate) fn no_dm() -> String {
    bindings(true, "").replace(&format!("[[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"), "")
}

/// With the task channel expected and no DM bound, there is no place to draw
/// the task channel's board, so the no-task-channel rule applies: a reminder
/// set in a private channel is on that channel's board, and is cancelled from it.
#[tokio::test]
async fn with_no_dm_bound_a_reminder_is_on_its_own_channels_board_and_is_cancelled_from_it() {
    let r = rig(
        vec![Scripted::text("Hello.")],
        |_| vec![],
        &no_dm(),
        |_| {},
        |_| {},
    )
    .await;
    until(&r, "the task channel is recorded", || {
        home_record(&r).is_some()
    })
    .await;
    let home = home_record(&r).unwrap();
    r.say((ANA, "ana"), Some(LAB), "hello");
    until(&r, "#lab answered", || {
        r.posted(LAB).iter().any(|m| m.content.contains("Hello."))
    })
    .await;
    let wake = remind_in(
        &r,
        &session_of_place(&r, &format!("channel:{LAB}")),
        "lab net mending",
    )
    .await;
    until(&r, "the reminder on #lab's board", || {
        boards(&r, LAB)
            .first()
            .is_some_and(|b| b.content.contains("lab net mending"))
    })
    .await;
    assert!(
        boards(&r, home).is_empty(),
        "the task channel has no place to draw it"
    );
    let board = boards(&r, LAB).remove(0);
    r.fake
        .press_values(
            &theseus_sim::fake_discord::Pressed {
                message: &board.id,
                button: crate::render::horizon::REMINDER_PICK,
                user: ANA,
                name: "ana",
            },
            &[&format!("hq:cancel:{wake}")],
        )
        .unwrap();
    until(&r, "the wake is cancelled", || {
        !r.core
            .kernel
            .pending_wakes()
            .unwrap()
            .into_iter()
            .any(|(_, w)| w.id == wake)
    })
    .await;
    until(&r, "the line leaves #lab's board", || {
        boards(&r, LAB)
            .first()
            .is_some_and(|b| !b.content.contains("lab net mending"))
    })
    .await;
}

/// The task channel's board is drawn by an owner's DM place, not by whichever
/// DM the file names first.
#[tokio::test]
async fn the_task_channels_board_is_drawn_by_an_owners_dm_not_the_first_dm() {
    let first = format!(
        "[[dm]]\nuser = \"{}\"\nname = \"ben\"\n",
        crate::tests_gateway::BEN
    );
    let binds = bindings(true, "").replace("[[dm]]\n", &format!("{first}[[dm]]\n"));
    let r = rig(vec![], |_| vec![], &binds, |_| {}, |_| {}).await;
    let shared = crate::runtime::task_home::daemon_for_tests(&r.core).expect("the binding runs");
    assert_eq!(shared.owner_dm_place(), Some(format!("discord:dm:{ANA}")));
}

/// A process that started again over what the last one left in Discord: the
/// task channel found by its name, its board an older run's pinned message, the
/// owner's DM bound, and a first word said in the DM.
pub(crate) async fn started_again(r: &Rig) -> Rig {
    r.stop();
    // The stand-in's gateway must have dropped the stopped run's client, or the
    // new run's connect is taken for the old one's.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let again = Rig::start_over(
        r.fake.clone(),
        |_, _| Arc::new(FakeProvider::scripted(vec![Scripted::text("Hello.")])),
        &bindings(true, ""),
        &[
            format!("channel:{}", crate::tests_default_forum::OPS),
            format!("channel:{REEL}"),
        ],
        |c| {
            tune(c);
            c.discord.task_home = "theseus-tasks".into();
        },
    )
    .await;
    again.say((ANA, "ana"), None, "hello");
    until(&again, "the DM answered", || {
        again
            .posted(ANA_DM)
            .iter()
            .filter(|m| m.content.contains("Hello."))
            .count()
            >= 2
    })
    .await;
    again
}

/// The owner's setup: an older run's cards are in the task channel, which the new
/// process finds by its name. A reminder set in the DM is a new card of its own
/// there at once (issue 30: the fact the owner's first use asks about): the
/// channel's one board is not edited to show it, and the older card is left as it is.
#[tokio::test]
async fn the_task_channels_old_pinned_board_shows_a_new_reminder_at_once() {
    let r = two_places("theseus-tasks").await;
    let home = home_up(&r).await;
    remind_in(
        &r,
        &session_of_place(&r, &format!("dm:{ANA}")),
        "first tide check",
    )
    .await;
    until(&r, "the first reminder's card", || {
        card_in(&r, home, "first tide check").is_some()
    })
    .await;
    let old = card_in(&r, home, "first tide check").unwrap();
    let again = started_again(&r).await;
    let home = home_up(&again).await;
    remind_in(
        &again,
        &session_of_place(&again, &format!("dm:{ANA}")),
        "second tide check",
    )
    .await;
    until(
        &again,
        "the second reminder's card in the task channel",
        || card_in(&again, home, "second tide check").is_some(),
    )
    .await;
    let b = boards(&again, home);
    assert!(
        b.iter().all(|b| !b.content.contains("second tide check")),
        "the board is not the place of a reminder: {b:#?}"
    );
    let still = card_in(&again, home, "first tide check").expect("the older card is still there");
    assert_eq!(still.id, old.id, "the older run's card, untouched");
}
