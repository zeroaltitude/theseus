//! A thread per task in a guild channel, and the task forum (theseus-i3do6),
//! end to end: a real core with the work engine, the binding on the stand-in's
//! REST and gateway, and a stand-in model that starts tasks (the rig of
//! `tests_board.rs`).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_core::policy::Posture;
use theseus_core::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use theseus_sim::fake_discord::{FakeThread, Guild, Msg, DEFAULT_GUILD};

use crate::runtime::threads::{task_name, Mark};
use crate::tests_gateway::{Rig, ANA, ANA_DM, BEN, CY, LAB};

pub(crate) const ASK: &str = "START: run this as a background task, please";
pub(crate) const QUOTE: &str = "run this as a background task, please";
pub(crate) const REEF: &str = "Reef-chart: chart the reef";
pub(crate) const BUOY: &str = "Buoy-paint: paint the buoy";

/// One provider for the parent and its tasks: the session it is asked for is
/// found by what its first message holds.
struct Switch {
    parent: FakeProvider,
    kids: Vec<(&'static str, FakeProvider)>,
}

impl Provider for Switch {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        let first = req
            .messages
            .first()
            .map(Value::to_string)
            .unwrap_or_default();
        let p = self
            .kids
            .iter()
            .find(|(m, _)| first.contains(m))
            .map_or(&self.parent, |(_, p)| p);
        p.stream_message(req, on_delta)
    }
}

pub(crate) fn start(briefs: &[&str]) -> Scripted {
    let calls: Vec<(String, Value)> = briefs
        .iter()
        .enumerate()
        .map(|(i, b)| {
            (
                format!("t_task{i}"),
                json!({"brief": b, "arrangement": {"pieces": [{"quote": QUOTE, "role": "objective"}]}}),
            )
        })
        .collect();
    let calls: Vec<(&str, &str, Value)> = calls
        .iter()
        .map(|(id, v)| (id.as_str(), "task_create", v.clone()))
        .collect();
    Scripted::tools("Starting.", &calls)
}

/// A wait as long as a loaded machine needs.
pub(crate) async fn until(r: &Rig, what: &str, f: impl Fn() -> bool) {
    let t0 = std::time::Instant::now();
    while !f() {
        assert!(
            t0.elapsed() < Duration::from_secs(150),
            "timed out: {what}; #lab holds {:#?}; threads {:#?}",
            r.fake.messages(LAB),
            r.fake.threads()
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// The task's first step: a program that runs until the test lets it go (it
/// makes the file `go`), so the task is working, with its thread made, for
/// as long as the test needs, however loaded the machine.
pub(crate) fn gate(dir: &Path) -> Scripted {
    let go = dir.join("go");
    Scripted::tools(
        "",
        &[(
            "g1",
            "proc_run",
            json!({"argv": ["sh", "-c", format!("while [ ! -f {} ]; do sleep 0.05; done", go.display())]}),
        )],
    )
}

/// Let the gated task go.
pub(crate) fn open_gate(r: &Rig) {
    std::fs::write(r.dir.path().join("go"), "").unwrap();
}

/// The thread's place runs: the task's posts go there.
pub(crate) async fn bound(r: &Rig, thread: u64) {
    let target = format!("discord:channel:{thread}");
    until(r, "the task's place runs in its thread", || {
        r.core.outbox.target(&task_session(r)).as_deref() == Some(target.as_str())
            && r.core.runner.place_rule.class(&r.core.cfg, Some(&target))
                == theseus_core::places::PlaceClass::Private
    })
    .await;
}

/// A task that runs a program for a few seconds and says `last`.
pub(crate) fn sleeps(secs: u32, last: &str) -> Vec<Scripted> {
    vec![
        Scripted::tools(
            "",
            &[(
                "p1",
                "proc_run",
                json!({"argv": ["sleep", secs.to_string()]}),
            )],
        ),
        Scripted::text(last),
    ]
}

pub(crate) fn model(
    parent: Vec<Scripted>,
    kids: Vec<(&'static str, Vec<Scripted>)>,
) -> Arc<dyn Provider> {
    Arc::new(Switch {
        parent: FakeProvider::scripted(parent),
        kids: kids
            .into_iter()
            .map(|(m, s)| (m, FakeProvider::scripted(s)))
            .collect(),
    })
}

pub(crate) fn guild() -> Guild {
    Guild::new(DEFAULT_GUILD, (ANA, "ana"))
        .member(BEN, "ben")
        .member(CY, "cy")
        .private_channel(LAB, "lab", &[ANA, BEN, CY])
}

/// `#lab` (private or shared, with `extra` keys) and ana's DM.
pub(crate) fn bindings(private: bool, extra: &str) -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\", \"{CY}\"]\nmention_only = false\nprivate = {private}\n{extra}\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
    )
}

pub(crate) fn tweak(c: &mut theseus_core::Config) {
    c.policy.tools.insert("proc.run".into(), Posture::Open);
    c.discord.edit_interval_ms = 250;
}

/// The threads the bot made in `#lab`.
pub(crate) fn threads(r: &Rig) -> Vec<FakeThread> {
    r.fake
        .threads()
        .into_iter()
        .filter(|t| t.parent == LAB)
        .collect()
}

/// What the bot posted in a task's thread, as text.
pub(crate) fn said(r: &Rig, thread: u64) -> String {
    r.posted(thread)
        .iter()
        .map(|m| m.content.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) async fn rig(
    parent: Vec<Scripted>,
    kids: impl FnOnce(&Path) -> Vec<(&'static str, Vec<Scripted>)>,
    private: bool,
) -> Rig {
    Rig::start_tweaked(
        |dir, _| model(parent, kids(dir)),
        guild(),
        &bindings(private, ""),
        &[],
        tweak,
    )
    .await
}

#[test]
fn a_threads_name_is_the_mark_the_short_id_and_the_title_within_a_hundred() {
    let id = "exe_01J00000000000000000a1b2c3";
    assert_eq!(
        task_name(Mark::Running, id, "Fix the flaky   lifecycle test"),
        "⚙️ a1b2c3 · Fix the flaky lifecycle test"
    );
    assert!(task_name(Mark::Done, id, "x").starts_with("✅ a1b2c3 · "));
    assert!(task_name(Mark::Failed, id, "x").starts_with("❌ a1b2c3 · "));
    let long = task_name(Mark::Running, id, &"y".repeat(300));
    assert_eq!(long.chars().count(), 100, "{long}");
    assert_eq!(task_name(Mark::Running, id, "  "), "⚙️ a1b2c3 · task");
}

#[tokio::test]
async fn a_task_in_a_guild_channel_gets_a_thread_where_its_stream_lands_and_one_rename_at_the_end()
{
    let r = rig(
        vec![start(&[REEF]), Scripted::text("Started.")],
        |dir| {
            vec![(
                REEF,
                [vec![gate(dir)], sleeps(1, "Reef charted, all of it.")].concat(),
            )]
        },
        true,
    )
    .await;
    r.say((ANA, "ana"), Some(LAB), ASK);
    until(&r, "a thread for the task", || !threads(&r).is_empty()).await;
    let t = threads(&r).remove(0);
    // The thread is bound private, as its parent is, and the task works
    // until its thread is ready.
    bound(&r, t.id).await;
    open_gate(&r);
    // Discord makes a private thread when `type` is left out, and archives
    // at 4,320 minutes when the duration is.
    assert_eq!(t.kind, 11, "the type is given: {t:?}");
    assert_eq!(t.auto_archive, 1440, "the archive duration is given: {t:?}");
    assert!(!t.forum);
    assert!(
        t.names[0].starts_with("⚙️ ")
            && t.names[0].contains(" · ")
            && t.names[0].chars().count() <= 100,
        "{:?}",
        t.names
    );
    assert!(t.names[0].contains("Reef-chart"), "{:?}", t.names);
    // The channel got one line linking it, and it made no noise.
    until(&r, "the link in the channel", || {
        r.posted(LAB)
            .iter()
            .any(|m| m.content.contains(&format!("<#{}>", t.id)))
    })
    .await;
    let links: Vec<Msg> = r
        .posted(LAB)
        .into_iter()
        .filter(|m| m.content.contains(&format!("<#{}>", t.id)))
        .collect();
    assert_eq!(links.len(), 1, "one line: {links:#?}");
    assert!(links[0].silent(), "the line is silent: {:?}", links[0]);
    assert!(links[0].mentions.is_empty());
    // The task's stream (its tool line) is in the thread, not the channel;
    // its report, which is the conversation's news, is in the channel.
    until(&r, "the task's tool line in its thread", || {
        said(&r, t.id).contains("sleep 1")
    })
    .await;
    until(&r, "the task's report in the channel", || {
        r.posted(LAB).iter().any(|m| {
            m.content.contains("finished") && m.content.contains("Reef charted, all of it.")
        })
    })
    .await;
    // The report's first line links the task's own thread, so a tap opens it
    // (theseus-ypnxt.5).
    let report = r
        .posted(LAB)
        .into_iter()
        .find(|m| m.content.contains("finished") && m.content.contains("Reef charted"))
        .unwrap();
    let first = report.content.lines().next().unwrap_or_default();
    assert!(first.contains(&format!("<#{}>", t.id)), "{first}");
    assert!(
        !r.posted(LAB).iter().any(|m| m.content.contains("sleep")),
        "none of the task's tool lines are in the channel: {:#?}",
        r.posted(LAB)
    );
    // One rename, at the end: the mark changes, nothing else.
    until(&r, "the rename", || {
        threads(&r).first().is_some_and(|t| t.names.len() >= 2)
    })
    .await;
    let t = threads(&r).remove(0);
    assert!(t.names[1].starts_with("✅ "), "{:?}", t.names);
    assert_eq!(
        t.names[0].trim_start_matches("⚙️ "),
        t.names[1].trim_start_matches("✅ "),
        "only the mark changed"
    );
    // The thread is left to archive itself.
    assert!(!t.archived, "nothing archives it by hand: {t:?}");
    // Quiet for a while: no second rename.
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(threads(&r)[0].names.len(), 2, "{:?}", threads(&r)[0].names);
}

/// A task that only answers (a shared place's tasks run no program).
pub(crate) fn answers(last: &str) -> Vec<Scripted> {
    vec![Scripted::text(last)]
}

pub(crate) const FORUM: u64 = 900_000_000_000_000_030;

fn guild_with_forum() -> Guild {
    guild().channel(FORUM, "tasks")
}

/// A forum bound `private` or shared, beside `#lab` and the DM.
fn forum_bindings(lab_private: bool, forum_private: bool) -> String {
    bindings(
        lab_private,
        &format!(
            "[[channel]]\nid = \"{FORUM}\"\nname = \"tasks\"\nkind = \"forum\"\nusers = [\"{ANA}\"]\nprivate = {forum_private}\n"
        ),
    )
}

/// The posts the bot made in the forum.
fn posts(r: &Rig) -> Vec<FakeThread> {
    r.fake
        .threads()
        .into_iter()
        .filter(|t| t.parent == FORUM)
        .collect()
}

/// A task that writes outside the roots, so it waits on a card (behind the
/// gate, so that its thread is made first: a card staged before the thread
/// lands in the channel, as it did before threads).
pub(crate) fn asks(dir: &Path, name: &str) -> Vec<Scripted> {
    let path = dir.join("outside").join(name);
    vec![
        Scripted::tools(
            "",
            &[(
                "w1",
                "fs_write",
                json!({"path": path.to_string_lossy(), "content": name}),
            )],
        ),
        Scripted::text("Heard you, and written."),
    ]
}

#[tokio::test]
async fn a_guests_message_in_a_private_task_thread_is_a_comment_and_is_never_lost() {
    let r = rig(
        vec![
            start(&[REEF]),
            Scripted::text("Started."),
            Scripted::text("Noted."),
        ],
        |dir| vec![(REEF, [vec![gate(dir)], answers("Reef charted.")].concat())],
        true,
    )
    .await;
    r.say((ANA, "ana"), Some(LAB), ASK);
    until(&r, "a thread for the task", || !threads(&r).is_empty()).await;
    let t = threads(&r).remove(0);
    bound(&r, t.id).await;
    // cy is allowed in #lab; ben is not. cy is a guest in a private task's
    // thread, so the message is a comment: no session reads it, the owner is
    // told in the channel (theseus-bh3t.4).
    r.say((CY, "cy"), Some(t.id), "also chart the north reef");
    r.say((BEN, "ben"), Some(t.id), "ben is not allowed here");
    let task = task_session(&r);
    let parent = parent_session(&r);
    let held = |sid: &str| -> Vec<String> {
        r.core
            .store
            .session_nodes(sid)
            .unwrap()
            .into_iter()
            .map(|(_, n)| format!("{:?}", n.body))
            .collect()
    };
    until(
        &r,
        "the owner is told of the comment in the channel",
        || {
            r.posted(LAB).iter().any(|m| {
                m.content.contains("💬 cy on") && m.content.contains("also chart the north")
            })
        },
    )
    .await;
    until(&r, "the thread says the task does not read it", || {
        said(&r, t.id).contains("doesn't read guests' messages")
    })
    .await;
    assert!(
        !held(&task)
            .iter()
            .chain(held(&parent).iter())
            .any(|b| b.contains("also chart the north reef")),
        "no session read the comment"
    );
    let rows = r.ledger("discord.message.in");
    assert!(
        rows.iter()
            .any(|d| d["task_thread"] == true && d["chars"] == 25),
        "{rows:?}"
    );
    assert!(
        !held(&task)
            .iter()
            .chain(held(&parent).iter())
            .any(|b| b.contains("ben is not allowed here")),
        "a user the channel does not list is not heard in its thread"
    );
    open_gate(&r);
}

/// What a session's nodes hold, as text.
pub(crate) fn held(r: &Rig, sid: &str) -> Vec<String> {
    r.core
        .store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .map(|(_, n)| format!("{:?}", n.body))
        .collect()
}

/// The owner's message in a running task's thread goes to the task, not to
/// the conversation that started it; a guest's is a comment, which no session
/// reads (theseus-bh3t.4); and the thread's first line links back to the
/// channel (theseus-esxyb).
#[tokio::test]
async fn the_owners_message_joins_a_running_task_and_a_guests_is_a_comment() {
    let r = rig(
        vec![
            start(&[REEF]),
            Scripted::text("Started."),
            Scripted::text("Noted."),
        ],
        |dir| vec![(REEF, [vec![gate(dir)], answers("Reef charted.")].concat())],
        true,
    )
    .await;
    r.say((ANA, "ana"), Some(LAB), ASK);
    until(&r, "a thread for the task", || !threads(&r).is_empty()).await;
    let t = threads(&r).remove(0);
    bound(&r, t.id).await;
    until(&r, "the jump link back to the channel", || {
        said(&r, t.id).contains(&format!("↩ back to <#{LAB}>"))
    })
    .await;
    let (task, parent) = (task_session(&r), parent_session(&r));
    r.say((ANA, "ana"), Some(t.id), "chart the north reef too");
    r.say((CY, "cy"), Some(t.id), "cy says hello to the reef");
    until(&r, "the owner's message reached the task", || {
        held(&r, &task)
            .iter()
            .any(|b| b.contains("chart the north reef too"))
    })
    .await;
    // cy is a guest in a private task's thread: a comment, which no session
    // reads (the redirect is a shared place's rule, in tests_war_room).
    until(&r, "the guest's comment reached the owner", || {
        r.posted(LAB)
            .iter()
            .any(|m| m.content.contains("💬 cy on") && m.content.contains("cy says hello"))
    })
    .await;
    assert!(
        !held(&r, &parent)
            .iter()
            .any(|b| b.contains("cy says hello to the reef")),
        "the guest's comment did not go to the conversation"
    );
    assert!(
        !held(&r, &parent)
            .iter()
            .any(|b| b.contains("chart the north reef too")),
        "the owner's message did not go to the conversation"
    );
    assert!(
        !held(&r, &task)
            .iter()
            .any(|b| b.contains("cy says hello to the reef")),
        "the guest's message did not go to the running task"
    );
    // The core took the owner's message as the owner's (the origin rides
    // `speaking`): it counts toward the report's joined line, and the
    // guest's does not (theseus-slmj1).
    until(&r, "the core counts the owner's message as joined", || {
        r.core.runner.tools.job_waits.steers.joined(&task) == 1
    })
    .await;
    open_gate(&r);
}

/// A message for a task that has ended goes to the conversation, quoting the
/// task with the shared line, whoever wrote it.
#[tokio::test]
async fn the_owners_message_for_an_ended_task_goes_to_the_conversation_with_the_shared_line() {
    let r = rig(
        vec![
            start(&[REEF]),
            Scripted::text("Started."),
            Scripted::text("Noted."),
            Scripted::text("Noted again."),
        ],
        |_| vec![(REEF, answers("Reef charted."))],
        true,
    )
    .await;
    r.say((ANA, "ana"), Some(LAB), ASK);
    until(&r, "a thread for the task", || !threads(&r).is_empty()).await;
    let t = threads(&r).remove(0);
    until(&r, "the thread is renamed at the task's end", || {
        threads(&r)[0].names.len() == 2
    })
    .await;
    let parent = parent_session(&r);
    r.say((ANA, "ana"), Some(t.id), "one more chart of the reef");
    until(
        &r,
        "the conversation heard it, with the shared line",
        || {
            held(&r, &parent).iter().any(|b| {
                b.contains("one more chart of the reef")
                    && b.contains("[Written to the task")
                    && b.contains("and the task is over]")
            })
        },
    )
    .await;
    until(&r, "the thread says where it went", || {
        said(&r, t.id).contains("sent to the conversation that started it")
    })
    .await;
}

/// The task session: the one the work model calls a task.
pub(crate) fn task_session(r: &Rig) -> String {
    r.core
        .kernel
        .tasks(None)
        .unwrap()
        .into_iter()
        .map(|t| t.session_id)
        .next()
        .expect("a task")
}

/// The session `#lab` runs on.
pub(crate) fn parent_session(r: &Rig) -> String {
    r.core
        .outbox
        .place_session(&format!("channel:{LAB}"))
        .unwrap()
        .expect("lab's session")
}

#[tokio::test]
async fn a_shared_channels_task_thread_is_shared_and_no_private_title_reaches_it() {
    let r = rig(
        vec![
            start(&[REEF]),
            Scripted::text("Started."),
            start(&[BUOY]),
            Scripted::text("Started it."),
        ],
        |_| {
            vec![
                (REEF, answers("Reef charted.")),
                (BUOY, answers("Buoy painted.")),
            ]
        },
        false,
    )
    .await;
    r.say((ANA, "ana"), Some(LAB), ASK);
    until(&r, "a thread for the shared channel's task", || {
        !threads(&r).is_empty()
    })
    .await;
    let t = threads(&r).remove(0);
    // The thread is bound with its parent's class: a task never becomes
    // private by moving into its thread.
    let target = format!("discord:channel:{}", t.id);
    until(&r, "the task's place runs in its thread", || {
        r.core.outbox.target(&task_session(&r)).as_deref() == Some(target.as_str())
    })
    .await;
    assert_eq!(
        r.core.runner.place_rule.class(&r.core.cfg, Some(&target)),
        theseus_core::places::PlaceClass::Shared,
        "a shared channel's thread is a shared place"
    );
    // A task of the owner's DM, with a private title.
    r.say((ANA, "ana"), None, ASK);
    until(&r, "the DM's task ran", || {
        r.core.kernel.tasks(None).unwrap().len() == 2
    })
    .await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        threads(&r).len(),
        1,
        "the DM's task made no thread: {:?}",
        r.fake.threads()
    );
    let everything: String = r
        .fake
        .all_messages()
        .into_iter()
        .filter(|m| m.channel == LAB.to_string() || m.channel == t.id.to_string())
        .map(|m| m.content)
        .chain(r.fake.threads().into_iter().flat_map(|t| t.names))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !everything.contains("Buoy"),
        "a private place's title never reaches the shared channel or its thread: {everything}"
    );
}

#[tokio::test]
async fn a_channel_that_turns_task_threads_off_keeps_its_tasks_in_the_channel() {
    let r = Rig::start_tweaked(
        |_, _| {
            model(
                vec![start(&[REEF]), Scripted::text("Started.")],
                vec![(REEF, answers("Reef charted, in the channel."))],
            )
        },
        guild(),
        &bindings(true, "task_threads = false\n"),
        &[],
        tweak,
    )
    .await;
    r.say((ANA, "ana"), Some(LAB), ASK);
    until(&r, "the task's words in the channel", || {
        r.posted(LAB)
            .iter()
            .any(|m| m.content.contains("Reef charted, in the channel."))
    })
    .await;
    assert!(threads(&r).is_empty(), "{:?}", r.fake.threads());
}

#[tokio::test]
async fn a_forum_post_is_the_tasks_live_card_and_its_tags_follow_its_state() {
    let r = Rig::start_tweaked(
        |_, _| {
            model(
                vec![start(&[REEF]), Scripted::text("Started.")],
                vec![(REEF, sleeps(2, "Reef charted."))],
            )
        },
        guild_with_forum(),
        &forum_bindings(true, true),
        &[],
        tweak,
    )
    .await;
    r.fake.set_forum(FORUM);
    r.say((ANA, "ana"), None, ASK);
    until(&r, "a post for the task", || !posts(&r).is_empty()).await;
    let post = posts(&r).remove(0);
    assert!(post.forum && post.kind == 11 || post.forum, "{post:?}");
    assert_eq!(post.auto_archive, 1440);
    assert_eq!(
        post.names.len(),
        1,
        "a post is never renamed: {:?}",
        post.names
    );
    assert!(post.names[0].contains("Reef-chart"), "{:?}", post.names);
    // The six tags exist, made once.
    let tags = r.fake.forum_tags(FORUM);
    let names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "needs you",
            "running",
            "done",
            "failed",
            "paused",
            "stopped"
        ],
        "{tags:?}"
    );
    let id_of = |n: &str| tags.iter().find(|t| t.name == n).unwrap().id.clone();
    assert_eq!(post.tag_sets[0], [id_of("running")], "{:?}", post.tag_sets);
    // The first message is a V2 card with the task's title.
    let card = r.fake.messages(post.id).remove(0);
    assert!(card.flags & theseus_sim::fake_discord::IS_COMPONENTS_V2 != 0);
    assert!(card.content.contains("Reef-chart"), "{}", card.content);
    // The DM's board links to the post.
    let url = format!("https://discord.com/channels/{DEFAULT_GUILD}/{}", post.id);
    until(&r, "the DM board's row links the post", || {
        r.posted(ANA_DM).iter().any(|m| m.content.contains(&url))
    })
    .await;
    // The task ends: the final tag is written, and the card says so.
    until(&r, "the final tag", || {
        posts(&r)
            .first()
            .is_some_and(|p| p.tag_sets.last() == Some(&vec![id_of("done")]))
    })
    .await;
    until(&r, "the card says done", || {
        r.fake.messages(post.id)[0].content.contains("done")
    })
    .await;
}

#[tokio::test]
async fn a_shared_forum_shows_a_shared_places_tasks_and_never_a_private_ones() {
    let r = Rig::start_tweaked(
        |_, _| {
            model(
                vec![
                    start(&[REEF]),
                    Scripted::text("Started."),
                    start(&[BUOY]),
                    Scripted::text("Started it."),
                ],
                vec![
                    (REEF, answers("Reef charted.")),
                    (BUOY, answers("Buoy painted.")),
                ],
            )
        },
        guild_with_forum(),
        &forum_bindings(false, false),
        &[],
        tweak,
    )
    .await;
    r.fake.set_forum(FORUM);
    r.say((ANA, "ana"), Some(LAB), ASK);
    until(&r, "a post for the shared channel's task", || {
        !posts(&r).is_empty()
    })
    .await;
    r.say((ANA, "ana"), None, ASK);
    until(&r, "the DM's task ran", || {
        r.core.kernel.tasks(None).unwrap().len() == 2
    })
    .await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let all = posts(&r);
    assert_eq!(all.len(), 1, "{all:?}");
    assert!(all[0].names[0].contains("Reef-chart"), "{:?}", all[0].names);
    assert!(
        !r.fake
            .all_messages()
            .iter()
            .filter(|m| m.channel == all[0].id.to_string())
            .any(|m| m.content.contains("Buoy")),
        "a private task's title never reaches a shared forum"
    );
}

#[tokio::test]
async fn a_task_waiting_on_a_card_has_the_card_in_its_thread_and_takes_a_message_there() {
    let r = Rig::start_tweaked(
        |dir, _| {
            model(
                vec![start(&[REEF]), Scripted::text("Started.")],
                vec![(REEF, [vec![gate(dir)], asks(dir, "reef.txt")].concat())],
            )
        },
        guild(),
        &bindings(true, ""),
        &[],
        tweak,
    )
    .await;
    r.say((ANA, "ana"), Some(LAB), ASK);
    until(&r, "a thread for the task", || !threads(&r).is_empty()).await;
    let t = threads(&r).remove(0);
    bound(&r, t.id).await;
    open_gate(&r);
    // The approval card is the task's own post, so it lands in its thread;
    // the channel keeps the conversation's.
    until(&r, "the card in the thread", || {
        r.posted(t.id)
            .iter()
            .any(|m| m.buttons.iter().any(|b| b.custom_id.starts_with("wa:")))
    })
    .await;
    assert!(
        !r.posted(LAB)
            .iter()
            .any(|m| m.content.contains("**Approve?**")),
        "the card is not in the channel: {:#?}",
        r.posted(LAB)
    );
    // The task waits, so the kernel takes a message: it reaches the task.
    r.say((ANA, "ana"), Some(t.id), "use the north jetty instead");
    let task = task_session(&r);
    until(&r, "the message reached the task", || {
        r.core
            .store
            .session_nodes(&task)
            .unwrap()
            .into_iter()
            .any(|(_, n)| format!("{:?}", n.body).contains("use the north jetty instead"))
    })
    .await;
    let parent = parent_session(&r);
    assert!(
        !r.core
            .store
            .session_nodes(&parent)
            .unwrap()
            .into_iter()
            .any(|(_, n)| format!("{:?}", n.body).contains("use the north jetty instead")),
        "the conversation did not hear what the task took"
    );
}

#[path = "tests_task_react.rs"]
mod react;
