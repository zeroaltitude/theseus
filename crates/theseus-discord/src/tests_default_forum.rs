//! The default forum (theseus-bh3t.6), end to end: a real core with the work
//! engine, the binding on the stand-in's REST and gateway, and the stand-in
//! model that starts tasks (the rig of `tests_task_threads.rs`). The guild is
//! trusted (`private = true` beside `guild_id`), so the forum acts.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use theseus_core::approval::{Answerer, Surface};
use theseus_core::provider::{DeltaSink, Provider, ProviderFuture, ProviderRequest, Scripted};
use theseus_core::Config;
use theseus_protocol::threads::ThreadNewParams;
use theseus_sim::fake_discord::{FakeDiscord, FakeThread, Guild, BOT_ID, DEFAULT_GUILD};

use crate::runtime::default_forum::{Stand, BOT_BITS, OWNER_BITS};
use crate::tests_gateway::{Rig, ANA, ANA_DM, BEN, CY, LAB};
use crate::tests_task_threads::{
    answers, gate, held, model, open_gate, sleeps, start, task_session, tweak, until, ASK, BUOY,
    FORUM, REEF,
};

const OPS: u64 = 900_000_000_000_000_011;
const REEL: u64 = 900_000_000_000_000_012;
const SKIFF: &str = "Skiff-wash: wash the skiff";
const ANCHOR: &str = "Anchor-check: check the anchor";

fn guild(with_forum: bool) -> Guild {
    let g = Guild::new(DEFAULT_GUILD, (ANA, "ana"))
        .member(BEN, "ben")
        .member(CY, "cy")
        .private_channel(LAB, "lab", &[ANA, BEN, CY])
        .channel(OPS, "ops")
        .private_channel(REEL, "reel", &[ANA]);
    if with_forum {
        g.channel(FORUM, "Theseus")
    } else {
        g
    }
}

/// `#lab` (private) with `lab_extra` keys, a shared `#ops`, a private `#reel`
/// and ana's DM, in a guild the owner trusts or not.
fn bindings(trusted: bool, lab_extra: &str) -> String {
    let word = if trusted { "private = true\n" } else { "" };
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n{word}\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\", \"{CY}\"]\nmention_only = false\nprivate = true\n{lab_extra}\
         [[channel]]\nid = \"{OPS}\"\nname = \"ops\"\nusers = [\"{ANA}\", \"{CY}\"]\nmention_only = false\nprivate = false\n\
         [[channel]]\nid = \"{REEL}\"\nname = \"reel\"\nusers = [\"{ANA}\"]\nmention_only = false\nprivate = true\n\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
    )
}

/// The rig's config: ana is the one owner, and `#reel` is the activity
/// thread `tides`' place once the thread exists.
fn tune(c: &mut Config) {
    tweak(c);
    c.places.owner = Some(vec![format!("discord:{ANA}")]);
    c.discord.thread = vec![theseus_core::config::discord::ThreadBind {
        channel: REEL.to_string(),
        thread: "tides".into(),
    }];
}

/// A model that holds the answers of the tasks whose brief holds `mark` for a
/// second, so that such a task is seen working before it ends.
struct Slow {
    inner: Arc<dyn Provider>,
    mark: &'static str,
}

impl Provider for Slow {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        let slow = req
            .messages
            .first()
            .is_some_and(|m| m.to_string().contains(self.mark));
        Box::pin(async move {
            if slow {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            self.inner.stream_message(req, on_delta).await
        })
    }
}

type Kids = Vec<(&'static str, Vec<Scripted>)>;

/// A trusted rig: `parent` is the model's script for the conversations,
/// `kids` the tasks', `setup` the stand-in before the binding starts.
async fn rig(
    parent: Vec<Scripted>,
    kids: impl FnOnce(&Path) -> Kids,
    binds: &str,
    setup: impl FnOnce(&FakeDiscord),
    cfg: impl FnOnce(&mut Config),
) -> Rig {
    rig_in(guild(false), parent, kids, binds, setup, cfg).await
}

async fn rig_in(
    guild: Guild,
    parent: Vec<Scripted>,
    kids: impl FnOnce(&Path) -> Kids,
    binds: &str,
    setup: impl FnOnce(&FakeDiscord),
    cfg: impl FnOnce(&mut Config),
) -> Rig {
    Rig::start_tweaked(
        |dir, fake| {
            setup(&fake);
            Arc::new(Slow {
                inner: model(parent, kids(dir)),
                mark: SKIFF,
            })
        },
        guild,
        binds,
        &[format!("channel:{OPS}"), format!("channel:{REEL}")],
        |c| {
            tune(c);
            cfg(c);
        },
    )
    .await
}

/// The forum the binding found or made, from its META record.
async fn forum_up(r: &Rig) -> u64 {
    until(r, "the forum is recorded", || record(r).is_some()).await;
    record(r).unwrap()
}

fn record(r: &Rig) -> Option<u64> {
    let v: Option<Value> = r
        .core
        .store
        .get_meta("discord.default_forum")
        .ok()
        .flatten();
    v?["channel"].as_u64()
}

fn posts(r: &Rig, forum: u64) -> Vec<FakeThread> {
    r.fake
        .threads()
        .into_iter()
        .filter(|t| t.parent == forum)
        .collect()
}

/// The post whose first message holds `needle`.
fn post_with(r: &Rig, forum: u64, needle: &str) -> Option<FakeThread> {
    posts(r, forum).into_iter().find(|p| {
        r.fake
            .messages(p.id)
            .first()
            .is_some_and(|m| m.content.contains(needle))
    })
}

fn tag_id(r: &Rig, forum: u64, name: &str) -> String {
    r.fake
        .forum_tags(forum)
        .into_iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("no tag {name}"))
        .id
}

fn last_tags(r: &Rig, post: u64) -> Vec<String> {
    r.fake
        .threads()
        .into_iter()
        .find(|t| t.id == post)
        .and_then(|t| t.tag_sets.last().cloned())
        .unwrap_or_default()
}

fn dm_lines(r: &Rig, needle: &str) -> usize {
    r.posted(ANA_DM)
        .iter()
        .filter(|m| m.content.contains(needle))
        .count()
}

fn cli() -> Answerer {
    Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
    }
}

// ------------------------------------------------------------------ finding

/// The forum is made once when the guild has none: private, with the owners'
/// and the bot's overwrites and the four tags; a second start makes none.
#[tokio::test]
async fn the_forum_is_made_once_private_with_its_tags_and_found_again() {
    let r = rig(vec![], |_| vec![], &bindings(true, ""), |_| {}, |_| {}).await;
    let forum = forum_up(&r).await;
    let made = r.fake.forums_made();
    assert_eq!(made.len(), 1, "{made:#?}");
    let b = &made[0];
    assert_eq!(
        (b["type"].as_u64(), b["name"].as_str()),
        (Some(15), Some("theseus"))
    );
    let tags: Vec<&str> = b["available_tags"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert_eq!(tags, ["needs you", "running", "done", "failed"]);
    let ows = b["permission_overwrites"].as_array().unwrap();
    assert_eq!(ows.len(), 3, "everyone, the owner, the bot: {ows:#?}");
    let of = |id: String| {
        ows.iter()
            .find(|o| o["id"] == id)
            .unwrap_or_else(|| panic!("{id}"))
    };
    let everyone = of(DEFAULT_GUILD.to_string());
    assert_eq!(
        (everyone["type"].as_u64(), everyone["deny"].as_str()),
        (Some(0), Some("1024"))
    );
    let owner = of(ANA.to_string());
    assert_eq!(owner["type"].as_u64(), Some(1));
    assert_eq!(owner["allow"], OWNER_BITS.to_string());
    assert_eq!(of(BOT_ID.to_string())["allow"], BOT_BITS.to_string());
    // A start on the same store finds it by its record: no second forum.
    let again = crate::runtime::shared_for_tests(&r.core);
    let stand = again
        .look_for_forum(&DEFAULT_GUILD.to_string(), "theseus")
        .await;
    assert!(
        matches!(stand, Stand::On(ref f) if f.channel == forum),
        "{stand:?}"
    );
    assert_eq!(r.fake.forums_made().len(), 1);
}

/// A forum of that name already there is the forum, whatever case: none is made.
#[tokio::test]
async fn a_forum_of_that_name_is_reused_and_none_is_made() {
    let r = rig_in(
        guild(true),
        vec![],
        |_| vec![],
        &bindings(true, ""),
        |f| f.set_forum(FORUM),
        |_| {},
    )
    .await;
    assert_eq!(forum_up(&r).await, FORUM);
    assert!(
        r.fake.forums_made().is_empty(),
        "{:#?}",
        r.fake.forums_made()
    );
}

/// Renamed in Discord, the forum is still found by its recorded id: no second.
#[tokio::test]
async fn a_renamed_forum_is_found_by_its_id() {
    let r = rig(vec![], |_| vec![], &bindings(true, ""), |_| {}, |_| {}).await;
    let forum = forum_up(&r).await;
    r.fake.rename_forum(forum, "the-ship-log");
    let again = crate::runtime::shared_for_tests(&r.core);
    let stand = again
        .look_for_forum(&DEFAULT_GUILD.to_string(), "theseus")
        .await;
    assert!(
        matches!(stand, Stand::On(ref f) if f.channel == forum),
        "{stand:?}"
    );
    assert_eq!(r.fake.forums_made().len(), 1, "no second forum");
    assert_eq!(record(&r), Some(forum));
}

/// A configured id that is no forum turns the feature off, tells the owner
/// once, and makes nothing.
#[tokio::test]
async fn a_configured_id_that_is_no_forum_turns_it_off_with_one_line() {
    let r = rig(
        vec![],
        |_| vec![],
        &bindings(true, ""),
        |_| {},
        |c| c.discord.forum_channel = "900000000000000999".into(),
    )
    .await;
    until(&r, "the owner's line", || {
        dm_lines(&r, "is not a forum") == 1
    })
    .await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(dm_lines(&r, "is not a forum"), 1);
    assert!(r.fake.forums_made().is_empty());
    assert_eq!(record(&r), None);
}

// ------------------------------------------------------------------- tasks

/// A DM's task has no thread to work in: its post is its place, the card
/// above, the stream below, the DM told by one quiet line, and the end
/// renames the post and sets its last tag.
#[tokio::test]
async fn a_dm_task_works_in_its_post_and_the_dm_gets_one_link_line() {
    let r = rig(
        vec![start(&[REEF]), Scripted::text("Started.")],
        |dir| vec![(REEF, [vec![gate(dir)], sleeps(1, "Reef charted.")].concat())],
        &bindings(true, ""),
        |_| {},
        |_| {},
    )
    .await;
    r.say((ANA, "ana"), None, ASK);
    let forum = forum_up(&r).await;
    until(&r, "the task's post", || !posts(&r, forum).is_empty()).await;
    let post = posts(&r, forum).remove(0);
    crate::tests_task_threads::bound(&r, post.id).await;
    assert!(
        post.names[0].starts_with("⚙️ ") && post.names[0].contains("Reef-chart"),
        "{:?}",
        post.names
    );
    assert_eq!(post.tag_sets[0], [tag_id(&r, forum, "running")]);
    let card = r.fake.messages(post.id).remove(0);
    assert!(card.flags & theseus_sim::fake_discord::IS_COMPONENTS_V2 != 0);
    assert!(card.content.contains("Reef-chart"), "{}", card.content);
    until(&r, "the DM's link line", || dm_lines(&r, "🧵") == 1).await;
    let line = r
        .posted(ANA_DM)
        .into_iter()
        .find(|m| m.content.contains("🧵"))
        .unwrap();
    assert!(
        line.content.contains(&format!("<#{}>", post.id)) && line.silent(),
        "{line:?}"
    );
    open_gate(&r);
    until(&r, "the end renames the post and tags it done", || {
        posts(&r, forum).first().is_some_and(|p| {
            p.names.last().is_some_and(|n| n.starts_with("✅ "))
                && p.tag_sets.last() == Some(&vec![tag_id(&r, forum, "done")])
        })
    })
    .await;
    assert_eq!(dm_lines(&r, "🧵"), 1, "still one line");
}

/// The owner's message in a homed post steers the running task.
#[tokio::test]
async fn the_owners_message_in_a_tasks_post_steers_it() {
    let r = rig(
        vec![start(&[REEF]), Scripted::text("Started.")],
        |dir| vec![(REEF, [vec![gate(dir)], answers("Reef charted.")].concat())],
        &bindings(true, ""),
        |_| {},
        |_| {},
    )
    .await;
    r.say((ANA, "ana"), None, ASK);
    let forum = forum_up(&r).await;
    until(&r, "the post", || !posts(&r, forum).is_empty()).await;
    let post = posts(&r, forum).remove(0);
    crate::tests_task_threads::bound(&r, post.id).await;
    let task = task_session(&r);
    r.say((ANA, "ana"), Some(post.id), "chart the north reef too");
    until(&r, "the message reached the task", || {
        held(&r, &task)
            .iter()
            .any(|b| b.contains("chart the north reef too"))
    })
    .await;
    until(&r, "it counts as joined", || {
        r.core.runner.tools.job_waits.steers.joined(&task) == 1
    })
    .await;
    open_gate(&r);
}

/// Every task, one post each, across four kinds of place: the DM's and a
/// private channel's work in their posts; a shared channel's keeps its thread
/// and its post is an index card linking it; an activity thread's is an index
/// card too. A message in an index card starts nothing.
#[tokio::test]
async fn every_task_gets_one_post_across_four_kinds_of_place() {
    let r = rig(
        vec![
            start(&[REEF]),
            Scripted::text("Started."),
            start(&[BUOY]),
            Scripted::text("Started it."),
            start(&[SKIFF]),
            Scripted::text("Started that."),
            start(&[ANCHOR]),
            Scripted::text("Started this."),
        ],
        |_| {
            vec![
                (REEF, answers("Reef charted.")),
                (BUOY, answers("Buoy painted.")),
                (SKIFF, answers("Skiff washed.")),
                (ANCHOR, answers("Anchor checked.")),
            ]
        },
        &bindings(true, ""),
        |_| {},
        |_| {},
    )
    .await;
    let forum = forum_up(&r).await;
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
    until(&r, "#reel runs the thread", || {
        r.core
            .threads()
            .get("tides")
            .is_some_and(|t| !t.surfaces.is_empty())
    })
    .await;
    for (channel, brief) in [
        (None, "Reef-chart"),
        (Some(LAB), "Buoy-paint"),
        (Some(OPS), "Skiff-wash"),
        (Some(REEL), "Anchor-check"),
    ] {
        r.say((ANA, "ana"), channel, ASK);
        until(&r, brief, || post_with(&r, forum, brief).is_some()).await;
    }
    assert_eq!(posts(&r, forum).len(), 4, "{:#?}", posts(&r, forum));
    // The private channel's task works in its post: no thread in #lab.
    assert!(r.fake.threads().iter().all(|t| t.parent != LAB));
    // The shared channel's keeps its public thread, and its post is a card
    // that links it.
    until(&r, "#ops' thread", || {
        r.fake.threads().iter().any(|t| t.parent == OPS)
    })
    .await;
    let thread = r
        .fake
        .threads()
        .into_iter()
        .find(|t| t.parent == OPS)
        .unwrap();
    let link = format!("https://discord.com/channels/{DEFAULT_GUILD}/{}", thread.id);
    until(&r, "the card links the thread", || {
        post_with(&r, forum, "Skiff-wash").is_some_and(|p| {
            r.fake.messages(p.id)[0]
                .components_json
                .to_string()
                .contains(&link)
        })
    })
    .await;
    let card = post_with(&r, forum, "Skiff-wash").unwrap();
    assert!(
        !card.names[0].starts_with("⚙️"),
        "an index card is no place: {:?}",
        card.names
    );
    open_gate(&r);
    until(&r, "the card's tag goes done", || {
        last_tags(&r, card.id) == [tag_id(&r, forum, "done")]
    })
    .await;
    assert_eq!(card.tag_sets[0], [tag_id(&r, forum, "running")]);
    a_message_in_the_card_starts_nothing(&r, card.id, &link).await;
    open_gate(&r);
}

/// A private channel with task threads off has no thread: its task's post is
/// an index card, which an archived post still takes its last tag on.
#[tokio::test]
async fn an_archived_index_card_still_takes_its_final_tag() {
    let r = rig(
        vec![start(&[BUOY]), Scripted::text("Started.")],
        |dir| vec![(BUOY, [vec![gate(dir)], answers("Buoy painted.")].concat())],
        &bindings(true, "task_threads = false\n"),
        |_| {},
        |_| {},
    )
    .await;
    let forum = forum_up(&r).await;
    r.say((ANA, "ana"), Some(LAB), ASK);
    until(&r, "the card", || {
        post_with(&r, forum, "Buoy-paint").is_some()
    })
    .await;
    let card = post_with(&r, forum, "Buoy-paint").unwrap();
    assert!(
        r.fake.threads().iter().all(|t| t.parent != LAB),
        "no thread either"
    );
    r.fake.archive_thread(card.id);
    open_gate(&r);
    until(&r, "the final tag lands on the archived post", || {
        last_tags(&r, card.id) == [tag_id(&r, forum, "done")]
    })
    .await;
}

/// An activity thread makes no post and no Discord thread: only a task does.
#[tokio::test]
async fn an_activity_thread_makes_no_post_and_no_discord_thread() {
    let r = rig(
        vec![Scripted::text("hello")],
        |_| vec![],
        &bindings(true, ""),
        |_| {},
        |_| {},
    )
    .await;
    let forum = forum_up(&r).await;
    r.core
        .thread_new(
            &ThreadNewParams {
                slug: "moons".into(),
                title: Some("Moons".into()),
                ..Default::default()
            },
            cli(),
        )
        .unwrap();
    r.say((ANA, "ana"), None, "thread:moons hello there");
    until(&r, "the reply", || {
        r.posted(ANA_DM).iter().any(|m| m.content.contains("hello"))
    })
    .await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(r.fake.threads().is_empty(), "{:#?}", r.fake.threads());
    assert!(posts(&r, forum).is_empty());
}

/// A message in an index card starts nothing and is answered by a line.
async fn a_message_in_the_card_starts_nothing(r: &Rig, card: u64, link: &str) {
    let before = r.ledger("discord.message.in").len();
    let sessions = r.core.kernel.tasks(None).unwrap().len();
    r.say((ANA, "ana"), Some(card), "hello card");
    until(r, "the pointer line", || {
        r.posted(card).iter().any(|m| {
            m.content
                .contains(&format!("-# This task works in {link}."))
        })
    })
    .await;
    assert_eq!(
        r.ledger("discord.message.in").len(),
        before,
        "no message reached a session"
    );
    assert_eq!(r.core.kernel.tasks(None).unwrap().len(), sessions);
}

// --------------------------------------------------------------- the switch

/// The requests a DM task makes, with the ids taken out.
async fn dm_task_requests(binds: &str, cfg: impl FnOnce(&mut Config)) -> (Rig, BTreeSet<String>) {
    let r = rig(
        vec![start(&[REEF]), Scripted::text("Started.")],
        |dir| vec![(REEF, [vec![gate(dir)], answers("Reef charted.")].concat())],
        binds,
        |_| {},
        cfg,
    )
    .await;
    r.say((ANA, "ana"), None, ASK);
    until(&r, "the task ran", || {
        r.core.kernel.tasks(None).unwrap().len() == 1
    })
    .await;
    open_gate(&r);
    until(&r, "its report", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains("Reef charted."))
    })
    .await;
    let seen = r
        .fake
        .seen()
        .into_iter()
        .map(|s| {
            let path: String = s
                .path
                .split('/')
                .map(|p| {
                    if p.bytes().all(|b| b.is_ascii_digit()) && !p.is_empty() {
                        "N"
                    } else {
                        p
                    }
                })
                .collect::<Vec<_>>()
                .join("/");
            format!("{} {path}", s.method)
        })
        .collect();
    (r, seen)
}

/// `forum = false` is today exactly: nothing read, made or written, and the
/// forum requests are made.
#[tokio::test]
async fn forum_false_is_todays_behaviour() {
    let (r, off) = dm_task_requests(&bindings(true, ""), |c| c.discord.forum = false).await;
    assert!(
        off.iter().all(|s| !s.contains("/guilds/N/channels")),
        "no forum request: {off:#?}"
    );
    assert_eq!(record(&r), None);
    assert!(r.fake.forums_made().is_empty());
    assert_eq!(dm_lines(&r, "🧵"), 0);
    assert!(r.fake.threads().is_empty());
}

/// A refusal (403): today's behaviour, one line to the owner for the run
/// however many tasks and reconnects follow.
#[tokio::test]
async fn a_refused_forum_falls_back_with_exactly_one_owner_line() {
    let r = rig(
        vec![
            start(&[REEF]),
            Scripted::text("Started."),
            start(&[BUOY]),
            Scripted::text("Started it."),
        ],
        |dir| {
            vec![
                (REEF, [vec![gate(dir)], answers("Reef charted.")].concat()),
                (BUOY, [vec![gate(dir)], answers("Buoy painted.")].concat()),
            ]
        },
        &bindings(true, ""),
        |f| f.refuse_forum(true),
        |_| {},
    )
    .await;
    until(&r, "the owner's line", || {
        dm_lines(&r, "I can't make the Theseus forum") == 1
    })
    .await;
    r.say((ANA, "ana"), None, ASK);
    r.say((ANA, "ana"), Some(LAB), ASK);
    // Today's: the channel's task has its thread; the DM's has none.
    until(&r, "#lab's thread", || {
        r.fake.threads().iter().any(|t| t.parent == LAB)
    })
    .await;
    assert!(r.fake.forums_made().is_empty());
    assert!(r.fake.threads().iter().all(|t| !t.forum));
    assert!(r.fake.gateway().unwrap().ready_again());
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(
        dm_lines(&r, "I can't make the Theseus forum"),
        1,
        "once a run"
    );
    assert_eq!(dm_lines(&r, "🧵"), 0, "the DM's task has no post");
    open_gate(&r);
}
