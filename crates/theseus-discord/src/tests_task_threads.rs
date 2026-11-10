//! A thread per task in a guild channel, and the task forum (theseus-i3do6),
//! end to end: a real core with the work engine, the binding on the stand-in's
//! REST and gateway, and a stand-in model that starts tasks (the rig of
//! `tests_board.rs`).

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_core::policy::Posture;
use theseus_core::provider::{
    DeltaSink, FakeProvider, Provider, ProviderFuture, ProviderRequest, Scripted,
};
use theseus_sim::fake_discord::{FakeDiscord, FakeThread, Guild, Msg, DEFAULT_GUILD};

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
    /// With the rig's Discord: each task's last answer waits for its place.
    places: Option<Places>,
}

/// A task's answer that ends it (no tool call) waits, up to `HOLD`, until
/// the binding has made as many places (a thread, or a channel of its own)
/// as tasks have started: the task ends with its place made. A place is made
/// for a task that is working, never for one that has ended, and a scripted
/// task that answers at once could end before it on a fast host. Every fresh
/// host (a cloud x86 host, an aarch64 laptop) ended it first, and the tests
/// that wait for the thread timed out; the owner's busier machine made the
/// thread first (theseus-ihu7l). A shared place's task gets no `proc_run`
/// (only the public tools), so `gate` cannot hold it there. A task with no
/// place (a DM's) ends `HOLD` late.
struct Places {
    fake: Arc<FakeDiscord>,
    /// Each task's place count to wait for: the order it started in.
    started: Mutex<HashMap<usize, usize>>,
}

const HOLD: Duration = Duration::from_secs(3);

impl Places {
    /// How many places there are now: threads, and the task channels made.
    fn made(&self) -> usize {
        let channels = self.fake.made_channels();
        self.fake.threads().len()
            + channels
                .iter()
                .filter(|c| c.kind == 0 && !c.deleted)
                .count()
    }

    /// The places task `kid` waits for.
    fn wanted(&self, kid: usize) -> usize {
        let mut started = self.started.lock().unwrap();
        let n = started.len() + 1;
        *started.entry(kid).or_insert(n)
    }
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
        let kid = self.kids.iter().position(|(m, _)| first.contains(m));
        let p = kid.map_or(&self.parent, |k| &self.kids[k].1);
        let Some((w, n)) = self.places.as_ref().zip(kid).map(|(w, k)| (w, w.wanted(k))) else {
            return p.stream_message(req, on_delta);
        };
        Box::pin(async move {
            let answer = p.stream_message(req, on_delta).await?;
            if answer.stop_reason.as_deref() != Some("tool_use") {
                let t0 = Instant::now();
                while w.made() < n && t0.elapsed() < HOLD {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
            Ok(answer)
        })
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

/// A wait as long as a loaded machine needs, and shorter than nextest's
/// 120 s, so a wait that never ends names itself (theseus-1dbb5).
pub(crate) async fn until(r: &Rig, what: &str, f: impl Fn() -> bool) {
    let t0 = std::time::Instant::now();
    while !f() {
        assert!(
            t0.elapsed() < Duration::from_secs(100),
            "timed out: {what}; #lab holds {:#?}; threads {:#?}",
            r.fake.messages(LAB),
            r.fake.threads()
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// The turn in `place` has ended on `reply`. The places share one model
/// script, so a test types in the next place only after this: else that
/// place's turn takes this one's reply, and the next task starts in the
/// wrong place (theseus-1dbb5).
pub(crate) async fn replied(r: &Rig, place: u64, reply: &str) {
    until(r, reply, || {
        r.posted(place).iter().any(|m| m.content.starts_with(reply))
    })
    .await;
}

/// The loop a gated task runs: it writes its pid to `gate.pid`, then waits
/// until the test lets it go (the file `go`), or its directory is gone (the
/// rig was dropped), or `secs` have passed (a test killed outright leaves its
/// directory behind), so a gated job never outlives its test (theseus-ynfq9).
/// The cap counts the loop's own 0.05 s sleeps, so it is never short.
fn gate_script(dir: &Path, secs: u32) -> String {
    let d = dir.display();
    format!(
        "echo $$ > {d}/gate.pid; n=0; while [ ! -f {d}/go ] && [ -d {d} ] && [ $n -lt {} ]; \
         do sleep 0.05; n=$((n+1)); done",
        secs * 20
    )
}

/// The task's first step: a program that runs until the test lets it go (it
/// makes the file `go`), so the task is working, with its thread made, for
/// as long as the test needs, however loaded the machine. It ends with the
/// test, however the test ends, and in 300 s at the latest.
pub(crate) fn gate(dir: &Path) -> Scripted {
    gate_capped(dir, 300)
}

fn gate_capped(dir: &Path, secs: u32) -> Scripted {
    Scripted::tools(
        "",
        &[(
            "g1",
            "proc_run",
            json!({"argv": ["sh", "-c", gate_script(dir, secs)]}),
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
    Arc::new(switch(parent, kids, None))
}

/// `model`, each task's last answer waiting for its place (`Places`).
pub(crate) fn placed_model(
    fake: Arc<FakeDiscord>,
    parent: Vec<Scripted>,
    kids: Vec<(&'static str, Vec<Scripted>)>,
) -> Arc<dyn Provider> {
    let places = Places {
        fake,
        started: Mutex::default(),
    };
    Arc::new(switch(parent, kids, Some(places)))
}

fn switch(
    parent: Vec<Scripted>,
    kids: Vec<(&'static str, Vec<Scripted>)>,
    places: Option<Places>,
) -> Switch {
    Switch {
        parent: FakeProvider::scripted(parent),
        kids: kids
            .into_iter()
            .map(|(m, s)| (m, FakeProvider::scripted(s)))
            .collect(),
        places,
    }
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
        |dir, fake| placed_model(fake, parent, kids(dir)),
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
    replied(&r, LAB, "Started.").await;
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
        |_, fake| {
            placed_model(
                fake,
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
    replied(&r, LAB, "Started.").await;
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

/// Whether process `pid` has ended: gone, or a zombie nobody has reaped yet.
fn ended(pid: &str) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/status")) {
        Ok(s) => s
            .lines()
            .any(|l| l.starts_with("State:") && l.contains('Z')),
        Err(_) => true,
    }
}

#[tokio::test]
async fn a_gate_left_shut_ends_with_its_rig() {
    let r = rig(
        vec![start(&[REEF]), Scripted::text("Started.")],
        |dir| vec![(REEF, [vec![gate(dir)], answers("Reef charted.")].concat())],
        true,
    )
    .await;
    r.say((ANA, "ana"), Some(LAB), ASK);
    let pid_file = r.dir.path().join("gate.pid");
    until(&r, "the gated task is working", || pid_file.exists()).await;
    let pid = loop {
        let pid = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .to_string();
        if !pid.is_empty() {
            break pid;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert!(!ended(&pid), "the gate runs while its rig lives");
    drop(r);
    let t0 = Instant::now();
    while !ended(&pid) {
        assert!(
            t0.elapsed() < Duration::from_secs(5),
            "the gate (pid {pid}) outlived its rig"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn a_gate_ends_at_its_cap() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = std::process::Command::new("sh")
        .args(["-c", &gate_script(dir.path(), 1)])
        .spawn()
        .unwrap();
    let t0 = Instant::now();
    while child.try_wait().unwrap().is_none() {
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "the gate ran past its cap with its directory in place"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(dir.path().exists() && !dir.path().join("go").exists());
}

/// **The `until` family, by membership** (theseus-a0kte). Every test file that
/// waits with this module's `until` shares its guard (the 100 s bound above),
/// so a hang or a timeout there is one mechanism. That membership used to
/// live only in a bead's prose, and the prose named a test that could not be
/// in it. This is the record, and the test below holds it to the source.
///
/// **Why this list stays bespoke and does not use `complete::MustBeComplete`**
/// (item 5, B7): theseus-discord does not include `complete.rs`, and the
/// scan's whole value is its failure, which already names both sets (what the
/// walk found and what the list holds) and the edit to make. Re-expressing it
/// would move the same walk behind another message, so the walk's holes (item
/// 1) were the defect, and are closed here.
///
/// `tests_task_react` is a child of this module (`mod react`, by `#[path]`)
/// and reaches `until` through `super::*`, so it counts without an import.
///
/// **Not in the family**, with the reason: `tests_gateway::horizon` waits with
/// `Rig::until` (`tests_gateway.rs`), a method of the gateway rig that 17 files
/// under `tests_gateway/` call as `r.until(`, horizon among them (two separate
/// reds, accounted apart). Four more private `until` helpers live in the crate
/// (`tests_outbox.rs`, `courier/tests_catchup.rs`, `runtime/voice/tests_tasks.rs`,
/// `runtime/emoji.rs`) and are no part of this guard either;
/// and theseusd's `an_acting_call_waits_and_the_cli_approves_it` is a test of
/// another crate with Discord disabled, so it shares no helper with these.
#[cfg(test)]
const UNTIL_FAMILY: &[&str] = &[
    "tests_default_forum.rs",
    "tests_dm_flow_golden.rs",
    "tests_forum_archive.rs",
    "tests_task_channels.rs",
    "tests_task_home.rs",
    "tests_task_owed.rs",
    "tests_task_react.rs",
    "tests_task_threads.rs",
    "tests_task_watch.rs",
    "tests_walk1b.rs",
    "tests_war_room.rs",
];

/// **What the fake-Discord forum fix can reach, which is a different
/// partition** (theseus-a0kte, re-derived at 12889bcf). `UNTIL_FAMILY` above
/// answers one question: which files share the 100 s `until` guard. A recipe
/// built on the fake-Discord forum fix answers another: which tests that fix's
/// code could have broken. **The two sets are not the same**, and reading one
/// as the other is how a test that cannot share the mechanism got into the
/// recipe and stayed there.
///
/// The mechanism is two conditions, and **both** are required: `[discord]
/// forum` on (`DiscordConfig::default`, true) **and exactly one trusted
/// guild**. `runtime/default_forum.rs`'s `start_default_forum` reads
/// `Bindings::trusted()`, keeps it only when `len() == 1`, and then filters on
/// `cfg.forum`; with none or several the stand is set `Stand::Off` and no
/// forum is ever looked for or made, so none of that fix's code runs.
/// `Bindings::trusted()` counts the guilds the file marks `private`, which in
/// format 1 is `private` beside the top-level `guild_id` — **not** a `private`
/// inside a `[[channel]]`, which is the channel's own class and trusts no
/// guild.
///
/// So the family splits in two by the `bindings` builder its rig uses, seven
/// reachable and four not:
///
/// - **Reachable (7)**: `tests_default_forum.rs` and `tests_task_channels.rs`,
///   whose own builders write `private = true` beside `guild_id`; and
///   `tests_dm_flow_golden.rs`, `tests_forum_archive.rs`,
///   `tests_task_home.rs`, `tests_task_owed.rs` and `tests_walk1b.rs`, which
///   borrow the first of those and call it with `trusted = true`.
/// - **Unreachable (4)**, `MECHANISM_UNREACHABLE` below: `tests_task_threads.rs`
///   (this module's own `bindings`, whose only `private` is inside
///   `[[channel]]`), `tests_task_react.rs` (its child, which takes it through
///   `super::*`), and `tests_task_watch.rs` and `tests_war_room.rs`, which
///   borrow this module's. Every rig of these four trusts **zero** guilds.
///
/// The two accounted apart are unreachable too, for two different reasons.
/// `tests_gateway::horizon`'s rig is `tests_gateway`'s own builder, which
/// writes no `private` beside `guild_id` either, so it trusts zero guilds and
/// has no forum to break; what makes that pair load-sensitive is its own
/// `Rig::until`, bounded at 20 s against this module's 100 s. And theseusd's
/// `an_acting_call_waits_and_the_cli_approves_it` runs a daemon with
/// `[discord] enabled = false` (its fixture's `safe_note`, which that test's
/// own config never overrides), so it has no Discord runtime at all: **it has
/// never been able to share this mechanism**, and every red of it that was
/// attributed to the mechanism was misattributed.
#[cfg(test)]
const MECHANISM_UNREACHABLE: &[&str] = &[
    "tests_task_react.rs",
    "tests_task_threads.rs",
    "tests_task_watch.rs",
    "tests_war_room.rs",
];

/// The family files that write a `bindings` of their own, and so are classed
/// by running it (`only_a_rig_that_trusts_one_guild_can_reach_the_forum`) and
/// not by whose they borrow.
#[cfg(test)]
const OWN_BINDINGS: &[&str] = &[
    "tests_default_forum.rs",
    "tests_task_channels.rs",
    "tests_task_threads.rs",
];

/// The mechanism's own gate, run on every `bindings` builder the family's
/// rigs use: the trusted-guild count each file's rig gives `Bindings`.
#[test]
fn only_a_rig_that_trusts_one_guild_can_reach_the_forum() {
    let trusted = |text: &str| {
        crate::bindings::Bindings::parse(text)
            .expect("a rig's bindings file parses")
            .trusted()
            .len()
    };
    // `[discord] forum` is on by default, so the guild count is the whole gate.
    assert!(
        theseus_core::config::DiscordConfig::default().forum,
        "the default changed: the guild count is no longer the whole gate"
    );
    // This module's rig, which `tests_task_watch` and `tests_war_room` borrow:
    // its `private` is inside `[[channel]]`, so no guild is trusted, whichever
    // way the channel is bound.
    assert_eq!(
        trusted(&bindings(true, "")),
        0,
        "this module's rig, private"
    );
    assert_eq!(
        trusted(&bindings(false, "")),
        0,
        "this module's rig, shared"
    );
    // The trusted rig, which five more family files borrow. Both arms, so the
    // builder's own `trusted` argument is what decides it.
    let forum_rig = crate::tests_default_forum::bindings;
    assert_eq!(trusted(&forum_rig(true, "")), 1, "the trusted rig");
    assert_eq!(trusted(&forum_rig(false, "")), 0, "the same rig, untrusted");
    // `tests_task_channels` writes its own, always trusted.
    assert_eq!(trusted(&crate::tests_task_channels::bindings(true)), 1);
    assert_eq!(trusted(&crate::tests_task_channels::bindings(false)), 1);
    // The horizon pair's rig, accounted apart: no trusted guild either.
    assert_eq!(
        trusted(&crate::tests_gateway::bindings(true)),
        0,
        "horizon's"
    );
    assert_eq!(
        trusted(&crate::tests_gateway::bindings(false)),
        0,
        "horizon's"
    );
}

/// `MECHANISM_UNREACHABLE` is exactly the family files whose rig is this
/// module's, held to the source: a file that changes which `bindings` it
/// borrows moves between the two halves, and this fails with the edit to make.
#[test]
fn the_unreachable_four_are_the_family_files_that_borrow_this_modules_rig() {
    for f in MECHANISM_UNREACHABLE {
        assert!(UNTIL_FAMILY.contains(f), "{f} is not in the until family");
    }
    let reachable: Vec<&&str> = UNTIL_FAMILY
        .iter()
        .filter(|f| !MECHANISM_UNREACHABLE.contains(f))
        .collect();
    // Both halves counted, so an empty family cannot satisfy the bar.
    assert_eq!(MECHANISM_UNREACHABLE.len(), 4, "{MECHANISM_UNREACHABLE:?}");
    assert_eq!(reachable.len(), 7, "{reachable:?}");
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let read = |f: &str| std::fs::read_to_string(src.join(f)).expect("a family file");
    let mut borrowed = 0;
    for f in UNTIL_FAMILY {
        let text = read(f);
        if OWN_BINDINGS.contains(f) {
            assert!(
                text.contains("fn bindings("),
                "{f} is listed in OWN_BINDINGS and writes no `bindings`"
            );
            continue;
        }
        assert!(
            !text.contains("fn bindings("),
            "{f} now writes its own `bindings`: add it to OWN_BINDINGS and run it above"
        );
        if *f == "tests_task_react.rs" {
            // The child module: it takes this module's rig through `super::*`.
            assert!(!imports_from(&text, "tests_task_threads", "bindings"));
            assert!(MECHANISM_UNREACHABLE.contains(f));
            continue;
        }
        let from_forum = imports_from(&text, "tests_default_forum", "bindings");
        let from_here = imports_from(&text, "tests_task_threads", "bindings");
        assert!(
            from_forum ^ from_here,
            "{f} borrows neither `bindings` or both: forum {from_forum}, here {from_here}"
        );
        borrowed += 1;
        assert_eq!(
            from_here,
            MECHANISM_UNREACHABLE.contains(f),
            "{f} borrows {}'s rig: move it {} MECHANISM_UNREACHABLE",
            if from_here {
                "this module"
            } else {
                "the trusted"
            },
            if from_here { "into" } else { "out of" }
        );
    }
    assert_eq!(
        borrowed, 7,
        "the scan classed {borrowed} files by their import"
    );
}

/// `an_acting_call_waits_and_the_cli_approves_it` runs with Discord disabled:
/// the fixture every theseusd test shares turns it off, and that test's own
/// config never puts it back. Read from the source, since no Discord code of
/// ours can be asked about a daemon that never starts the binding.
#[test]
fn theseusds_acting_call_test_runs_with_discord_disabled() {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../theseusd/tests");
    let fixture = std::fs::read_to_string(d.join("common/mod.rs")).expect("theseusd's fixture");
    assert!(
        fixture.contains(r#"table(&mut t, "discord").insert("enabled".into(), false.into());"#),
        "theseusd's shared fixture no longer disables Discord: re-derive the recipe"
    );
    let acting = std::fs::read_to_string(d.join("mcp_server.rs")).expect("theseusd's mcp_server");
    // The positive, so a file that lost the test cannot pass on the negative.
    assert!(acting.contains("fn an_acting_call_waits_and_the_cli_approves_it"));
    assert!(
        !acting.contains(r#""discord""#) && !acting.contains("theseus_discord"),
        "mcp_server.rs now names Discord: it may no longer be outside the family"
    );
}

/// Whether `text` reaches `item` of `module` by any written form: the brace
/// import (`tests_task_threads::{bound, until}`), including its multi-line
/// form, the plain `use` (`use crate::tests_task_threads::until;`) or a
/// path-qualified call (`crate::tests_task_threads::until(..)`). A path that
/// names another item of the module (`::bound(`) is not a reference to it.
#[cfg(test)]
fn imports_from(text: &str, module: &str, item: &str) -> bool {
    let at = format!("{module}::");
    text.split(at.as_str()).skip(1).any(|rest| {
        if let Some(list) = rest.strip_prefix('{') {
            let list = list.split('}').next().unwrap_or("");
            list.split(',').any(|w| w.trim() == item)
        } else {
            let word: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            word == item
        }
    })
}

/// Whether `text` reaches this module's `until`: `imports_from` at the one
/// pair the scan below asks about.
#[cfg(test)]
fn reaches_until(text: &str) -> bool {
    imports_from(text, "tests_task_threads", "until")
}

/// Every `.rs` file under `dir`, recursively, as its path relative to `root`
/// with `/` separators (a top-level file is its bare name).
#[cfg(test)]
fn rs_files_under(root: &Path, dir: &Path, out: &mut Vec<(String, std::path::PathBuf)>) {
    for e in std::fs::read_dir(dir).expect("read a source directory") {
        let p = e.expect("a source entry").path();
        if p.is_dir() {
            rs_files_under(root, &p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            let rel = p.strip_prefix(root).expect("under src");
            let rel: Vec<_> = rel.iter().map(|c| c.to_string_lossy()).collect();
            out.push((rel.join("/"), p));
        }
    }
}

#[test]
fn the_rule_reads_every_way_a_file_reaches_until() {
    assert!(reaches_until(
        "use crate::tests_task_threads::{bound, until};"
    ));
    assert!(reaches_until(
        "use crate::tests_task_threads::{\n    bound,\n    until,\n};"
    ));
    assert!(reaches_until("use crate::tests_task_threads::until;"));
    assert!(reaches_until(
        "let x = crate::tests_task_threads::until(&r, f).await;"
    ));
    // Negatives, so the predicate cannot be constant-true.
    assert!(!reaches_until(
        "crate::tests_task_threads::bound(&r, post.id);"
    ));
    assert!(!reaches_until(
        "use crate::tests_task_threads::{bound, until_not};"
    ));
    assert!(!reaches_until("use crate::tests_task_threads::untilx;"));
    assert!(!reaches_until("nothing here"));
}

#[test]
fn the_until_family_is_exactly_the_files_that_share_its_guard() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found: Vec<String> = vec!["tests_task_threads.rs".into(), "tests_task_react.rs".into()];
    let mut files = Vec::new();
    rs_files_under(&src, &src, &mut files);
    assert!(
        files.iter().any(|(rel, _)| rel.contains('/')),
        "the walk did not descend into a subdirectory"
    );
    for (name, p) in files {
        let text = std::fs::read_to_string(&p).expect("read a source file");
        if reaches_until(&text) && !found.contains(&name) {
            found.push(name);
        }
    }
    found.sort();
    assert!(
        found.len() >= 10,
        "the scan found only {found:?}: it read the wrong tree"
    );
    assert_eq!(
        found, UNTIL_FAMILY,
        "a file started or stopped sharing `until`: update UNTIL_FAMILY and its doc"
    );
    // The members the bead names, in the files that make them members.
    for (file, test) in [
        (
            "tests_walk1b.rs",
            "a_dm_message_routed_to_a_placeless_thread_still_sees_its_tasks",
        ),
        (
            "tests_task_react.rs",
            "pause_parks_a_running_task_without_a_report_and_resume_lets_it_report",
        ),
    ] {
        let text = std::fs::read_to_string(src.join(file)).expect("a family file");
        assert!(
            text.contains(&format!("fn {test}")),
            "{test} is not in {file}"
        );
        assert!(
            found.iter().any(|f| f == file),
            "{file} was not found by the scan"
        );
    }
    // The ones that are not: neither mentions this module at all.
    let horizon = std::fs::read_to_string(src.join("tests_gateway/horizon.rs")).expect("horizon");
    assert!(!horizon.contains("tests_task_threads"));
    let acting = Path::new(env!("CARGO_MANIFEST_DIR")).join("../theseusd/tests/mcp_server.rs");
    let acting = std::fs::read_to_string(acting).expect("theseusd's mcp_server test");
    assert!(acting.contains("fn an_acting_call_waits_and_the_cli_approves_it"));
    assert!(!acting.contains("tests_task_threads") && !acting.contains("theseus_discord"));
}

#[path = "tests_task_react.rs"]
mod react;
