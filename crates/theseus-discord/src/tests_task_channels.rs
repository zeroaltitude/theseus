//! A private text channel per qualifying task (theseus-bh3t.1), end to end:
//! a real core with the work engine, the binding on the stand-in's REST and
//! gateway (its channels, `theseus_sim::fake_discord::FakeChannel`), and a
//! stand-in model that starts tasks (the rig of `tests_task_threads.rs`).

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use theseus_core::provider::Scripted;
use theseus_protocol::task_start::Expect;
use theseus_protocol::work::{Progress, WorkState, WorkView};
use theseus_sim::fake_discord::commands::Sender;
use theseus_sim::fake_discord::{FakeChannel, Member, Overwrite, Role, BOT_ID, DEFAULT_GUILD};
use twilight_model::channel::permission_overwrite::{PermissionOverwrite, PermissionOverwriteType};
use twilight_model::channel::ChannelType;
use twilight_model::guild::Permissions;
use twilight_model::id::Id;

use crate::runtime::task_channels::{channel_name, qualifies, work_view, Why};
use crate::runtime::threads::{Made, Mark, PlaceKind};
use crate::tests_gateway::{Rig, ANA, ANA_DM, BEN, CY, LAB};
use crate::tests_task_threads::{
    answers, gate, guild, held, model, open_gate, said, sleeps, start, task_session, tweak, until,
    ASK, BUOY, REEF,
};
use crate::tests_war_room::answered;

const VIEW_CHANNEL: u64 = 1 << 10;
const READ_HISTORY: u64 = 1 << 16;
const SEND_MESSAGES: u64 = 1 << 11;
const MANAGE_CHANNELS: u64 = 1 << 4;
const MINUTE: u64 = 60_000;

/// A trusted guild (format 1's `private` beside `guild_id`): `#lab` is
/// private in it unless it says not, and ana has a DM.
fn bindings(lab_private: bool) -> String {
    format!(
        "guild_id = \"{DEFAULT_GUILD}\"\nprivate = true\n\
         [[channel]]\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{ANA}\"]\nmention_only = false\n\
         private = {lab_private}\n\
         [[dm]]\nuser = \"{ANA}\"\nname = \"ana\"\n"
    )
}

/// The channels (not categories) the bot made and kept.
fn made(r: &Rig) -> Vec<FakeChannel> {
    let all = r.fake.made_channels();
    all.into_iter()
        .filter(|c| c.kind == 0 && !c.deleted)
        .collect()
}

fn categories(r: &Rig) -> Vec<FakeChannel> {
    r.fake
        .made_channels()
        .into_iter()
        .filter(|c| c.kind == 4)
        .collect()
}

/// A response that plans two steps.
fn plan_steps() -> Scripted {
    Scripted::tools(
        "",
        &[
            (
                "s1",
                "task_create",
                json!({"title": "Chart the north reef"}),
            ),
            (
                "s2",
                "task_create",
                json!({"title": "Chart the south reef"}),
            ),
        ],
    )
}

/// A task whose first words plan two steps, then work until the gate opens.
fn plans(dir: &std::path::Path, last: &str) -> Vec<Scripted> {
    [vec![plan_steps(), gate(dir)], sleeps(1, last)].concat()
}

/// The parent starts a task that says it will run long.
fn start_long(brief: &str) -> Scripted {
    let quote = "run this as a background task, please";
    let input = json!({"brief": brief, "expect": "long",
        "arrangement": {"pieces": [{"quote": quote, "role": "objective"}]}});
    Scripted::tools("Starting.", &[("t_task0", "task_create", input)])
}

async fn rig(
    parent: Vec<Scripted>,
    kids: impl FnOnce(&std::path::Path) -> Vec<(&'static str, Vec<Scripted>)>,
    lab_private: bool,
    more: impl FnOnce(&mut theseus_core::Config),
) -> Rig {
    Rig::start_tweaked(
        |dir, _| model(parent, kids(dir)),
        guild(),
        &bindings(lab_private),
        &[],
        |c| {
            tweak(c);
            // A task plans its steps with `task_create`, a deferred tool.
            c.tools.defer.always_loaded.push("tasks".into());
            // Channels alone: the default forum's own tests join the two, and
            // the task home's (`tests_task_home.rs`) the report's place.
            c.discord.forum = false;
            c.discord.task_home = String::new();
            more(c);
        },
    )
    .await
}

fn wv(id: &str, state: &str) -> WorkView {
    work_view(id, state)
}

fn steps(total: u32) -> Option<Progress> {
    Some(Progress {
        steps_done: 0,
        steps_total: total,
        current: None,
        said: None,
        at_ms: 1,
    })
}

/// The pure rule: two steps, `long`, or the minutes; never an ended task.
#[test]
fn a_task_qualifies_by_steps_by_a_long_expectation_or_by_the_minutes_and_never_once_over() {
    let after = 10 * MINUTE;
    let t0 = 1_000;
    let mut v = wv("exe_reef", "running");
    assert_eq!(qualifies(&v, t0 + 1, after), None, "a new short task");
    assert_eq!(qualifies(&v, t0 + after - 1, after), None, "a minute short");
    assert_eq!(qualifies(&v, t0 + after, after), Some(Why::Promoted));
    v.progress = steps(1);
    assert_eq!(qualifies(&v, t0 + 1, after), None, "one step is no plan");
    v.progress = steps(2);
    assert_eq!(qualifies(&v, t0 + 1, after), Some(Why::Steps));
    v.progress = None;
    v.expect = Some(Expect::Long);
    assert_eq!(qualifies(&v, t0 + 1, after), Some(Why::Long));
    v.expect = Some(Expect::Short);
    assert_eq!(qualifies(&v, t0 + 1, after), None);
    // A deferred task asleep has not begun, and a conversation is no task.
    let mut asleep = wv("exe_late", "waiting");
    asleep.next_at_ms = Some(t0 + 5 * after);
    assert_eq!(qualifies(&asleep, t0 + 3 * after, after), None);
    let mut waiting = wv("exe_wait", "waiting");
    assert_eq!(qualifies(&waiting, t0 + after, after), Some(Why::Promoted));
    waiting.state = WorkState::Done;
    waiting.progress = steps(3);
    waiting.expect = Some(Expect::Long);
    assert_eq!(qualifies(&waiting, t0 + 9 * after, after), None, "over");
}

#[test]
fn a_channels_name_is_the_short_id_and_the_slug_within_a_hundred_with_the_mark_at_the_end() {
    let id = "exe_01J00000000000000000A1B2C3";
    assert_eq!(
        channel_name(Mark::Running, id, "Fix the  flaky lifecycle test!"),
        "a1b2c3-fix-the-flaky-lifecycle-test"
    );
    assert_eq!(channel_name(Mark::Done, id, "x"), "✅-a1b2c3-x");
    assert_eq!(channel_name(Mark::Failed, id, "x"), "❌-a1b2c3-x");
    assert_eq!(channel_name(Mark::Running, id, "  "), "a1b2c3-task");
    let long = channel_name(Mark::Failed, id, &"y".repeat(300));
    assert!(long.chars().count() <= 100, "{long}");
}

/// (a) A task with two plan steps gets a private channel under the category,
/// from the DM, which a DM has no thread for; the owner and the bot can see
/// it and nobody else; the DM gets one quiet line and the first line of the
/// channel says where it came from.
#[tokio::test]
async fn a_task_with_two_plan_steps_gets_a_private_channel_under_the_category() {
    let r = rig(
        vec![start(&[REEF]), Scripted::text("Started.")],
        |dir| vec![(REEF, plans(dir, "Reef charted, all of it."))],
        true,
        |_| {},
    )
    .await;
    r.say((ANA, "ana"), None, ASK);
    until(&r, "a channel for the task", || made(&r).len() == 1).await;
    let c = made(&r).remove(0);
    let cat = categories(&r);
    assert_eq!(
        cat.len(),
        1,
        "the open category alone, the done one when it is needed: {cat:#?}"
    );
    let open = cat
        .iter()
        .find(|k| k.name == "⚙️ Theseus tasks")
        .expect("the open category");
    assert_eq!(c.parent, Some(open.id), "{c:#?}");
    assert!(
        c.name.contains("chart-the-reef") || c.name.ends_with("-chart-the-reef"),
        "{}",
        c.name
    );
    // Private: @everyone denied, the owner and the bot allowed, no one else.
    let everyone = c
        .overwrites
        .iter()
        .find(|o| o.id == DEFAULT_GUILD)
        .expect("@everyone");
    assert_eq!(
        (everyone.kind, everyone.deny & VIEW_CHANNEL),
        (0, VIEW_CHANNEL)
    );
    let ana = c
        .overwrites
        .iter()
        .find(|o| o.id == ANA)
        .expect("the owner");
    assert_eq!(
        ana.allow & (VIEW_CHANNEL | SEND_MESSAGES),
        VIEW_CHANNEL | SEND_MESSAGES
    );
    let bot = c
        .overwrites
        .iter()
        .find(|o| o.id == theseus_sim::fake_discord::BOT_ID)
        .expect("the bot");
    assert_eq!(bot.allow & MANAGE_CHANNELS, MANAGE_CHANNELS);
    // ana and ben are the owners of this rig; nobody else is named.
    let mut named: Vec<u64> = c.overwrites.iter().map(|o| o.id).collect();
    named.sort_unstable();
    let mut want = vec![DEFAULT_GUILD, ANA, BEN, theseus_sim::fake_discord::BOT_ID];
    want.sort_unstable();
    assert_eq!(named, want, "{:#?}", c.overwrites);
    assert!(c.topic.contains("Reef-chart"), "{}", c.topic);
    // The DM got one quiet line; the channel says where it came from.
    until(&r, "the pointer in the DM", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains(&format!("<#{}>", c.id)))
    })
    .await;
    let pointers: Vec<_> = r
        .posted(ANA_DM)
        .into_iter()
        .filter(|m| m.content.contains(&format!("<#{}>", c.id)))
        .collect();
    assert_eq!(pointers.len(), 1, "{pointers:#?}");
    assert!(pointers[0].silent() && pointers[0].mentions.is_empty());
    until(&r, "the channel's first line", || {
        said(&r, c.id).contains("↩ from the DM")
    })
    .await;
    // The stream lands in the channel; there is no thread to be made.
    open_gate(&r);
    until(&r, "the task's tool line in its channel", || {
        said(&r, c.id).contains("sleep 1")
    })
    .await;
    assert!(r.fake.threads().is_empty(), "{:#?}", r.fake.threads());
}

/// (b) A task started `expect = long` gets one at once, with no plan.
#[tokio::test]
async fn a_task_started_expecting_to_run_long_gets_a_channel_at_once() {
    let r = rig(
        vec![start_long(REEF), Scripted::text("Started.")],
        |dir| vec![(REEF, [vec![gate(dir)], sleeps(1, "Charted.")].concat())],
        true,
        |_| {},
    )
    .await;
    r.say((ANA, "ana"), None, ASK);
    until(&r, "a channel for the long task", || made(&r).len() == 1).await;
    let c = made(&r).remove(0);
    until(&r, "the first line", || {
        said(&r, c.id).contains("↩ from the DM")
    })
    .await;
    assert!(
        !said(&r, c.id).contains("**"),
        "a long task's first line is the pointer alone: {}",
        said(&r, c.id)
    );
    open_gate(&r);
}

/// (c) A working task that has run the config's minutes gets a channel with
/// what has happened so far (the minutes, set to none here, are the rule's).
#[tokio::test]
async fn a_task_still_working_after_the_minutes_is_moved_to_a_channel_with_what_it_did() {
    let r = rig(
        vec![start(&[REEF]), Scripted::text("Started.")],
        |dir| vec![(REEF, [vec![gate(dir)], sleeps(1, "Charted.")].concat())],
        true,
        |c| c.discord.task_channel_after_minutes = 0,
    )
    .await;
    r.say((ANA, "ana"), None, ASK);
    until(&r, "a channel for the promoted task", || {
        made(&r).len() == 1
    })
    .await;
    let c = made(&r).remove(0);
    until(&r, "the summary", || {
        let said = said(&r, c.id);
        said.contains("↩ from the DM") && said.contains("**Reef-chart")
    })
    .await;
    open_gate(&r);
}

/// (d) A short, one-step task in a private guild channel goes on as it did: a
/// public thread there, and no channel.
#[tokio::test]
async fn a_short_task_gets_the_thread_it_always_did_and_no_channel() {
    let r = rig(
        vec![start(&[REEF]), Scripted::text("Started.")],
        |_| vec![(REEF, answers("Reef charted."))],
        true,
        |_| {},
    )
    .await;
    r.say((ANA, "ana"), Some(LAB), ASK);
    until(&r, "its thread, or a channel", || {
        r.fake.threads().iter().any(|t| t.parent == LAB) || !made(&r).is_empty()
    })
    .await;
    assert!(made(&r).is_empty(), "{:#?}", made(&r));
    assert!(r.fake.threads().iter().any(|t| t.parent == LAB));
    assert!(
        categories(&r).is_empty(),
        "no category for a task with no channel"
    );
}

/// A task in a channel that is shared keeps its public thread, plan or not:
/// its guests are there.
#[tokio::test]
async fn a_shared_channels_task_keeps_its_thread_whatever_its_plan() {
    let r = rig(
        vec![start(&[REEF]), Scripted::text("Started.")],
        |dir| vec![(REEF, plans(dir, "Reef charted."))],
        false,
        |_| {},
    )
    .await;
    r.say((ANA, "ana"), Some(LAB), ASK);
    until(&r, "its thread", || {
        r.fake.threads().iter().any(|t| t.parent == LAB)
    })
    .await;
    open_gate(&r);
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(made(&r).is_empty(), "{:#?}", made(&r));
}

/// The model, with the answer after a task's plan held back `SLOW`: a shared
/// place runs no `proc_run`, so this keeps its task working with its plan.
struct SlowAfterPlan(Arc<dyn theseus_core::provider::Provider>);

const SLOW: Duration = Duration::from_secs(4);

impl theseus_core::provider::Provider for SlowAfterPlan {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a theseus_core::provider::ProviderRequest,
        on_delta: theseus_core::provider::DeltaSink<'a>,
    ) -> theseus_core::provider::ProviderFuture<'a> {
        let after_plan = req
            .messages
            .last()
            .is_some_and(|m| m.to_string().contains("as a plan item"));
        Box::pin(async move {
            if after_plan {
                tokio::time::sleep(SLOW).await;
            }
            self.0.stream_message(req, on_delta).await
        })
    }
}

/// A shared channel's task that plans two steps and goes on working keeps its
/// public thread: no channel while its plan is on the board, nor after.
#[tokio::test]
async fn a_shared_channels_working_task_with_a_plan_gets_no_channel() {
    let r = Rig::start_tweaked(
        |_, _| {
            Arc::new(SlowAfterPlan(model(
                vec![start(&[REEF]), Scripted::text("Started.")],
                vec![(REEF, vec![plan_steps(), Scripted::text("Reef charted.")])],
            )))
        },
        guild(),
        &bindings(false),
        &[],
        |c| {
            tweak(c);
            c.tools.defer.always_loaded.push("tasks".into());
            c.discord.task_home = String::new();
        },
    )
    .await;
    r.say((ANA, "ana"), Some(LAB), ASK);
    until(&r, "its thread", || {
        r.fake.threads().iter().any(|t| t.parent == LAB)
    })
    .await;
    let task = task_session(&r);
    until(&r, "its plan of two steps", || {
        held(&r, &task)
            .iter()
            .filter(|b| b.contains("as a plan item"))
            .count()
            == 2
    })
    .await;
    // The task works on, its plan on the board, for SLOW.
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(made(&r).is_empty(), "{:#?}", made(&r));
    until(&r, "it ends", || {
        held(&r, &task).iter().any(|b| b.contains("Reef charted."))
    })
    .await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(made(&r).is_empty(), "{:#?}", made(&r));
}

/// (e) The owner's message in the channel steers the task, as in a thread.
#[tokio::test]
async fn the_owners_message_in_a_tasks_channel_steers_the_task() {
    let r = rig(
        vec![start(&[REEF]), Scripted::text("Started.")],
        |dir| vec![(REEF, plans(dir, "Reef charted."))],
        true,
        |_| {},
    )
    .await;
    r.say((ANA, "ana"), None, ASK);
    until(&r, "a channel", || made(&r).len() == 1).await;
    let c = made(&r).remove(0);
    until(&r, "the channel's first line", || {
        said(&r, c.id).contains("↩ from the DM")
    })
    .await;
    let task = task_session(&r);
    r.say((ANA, "ana"), Some(c.id), "chart the north reef too");
    until(&r, "the owner's message reached the task", || {
        held(&r, &task)
            .iter()
            .any(|b| b.contains("chart the north reef too"))
    })
    .await;
    until(&r, "the core counts it as the owner's steer", || {
        r.core.runner.tools.job_waits.steers.joined(&task) == 1
    })
    .await;
    open_gate(&r);
}

/// (f) A bot without Manage Channels is refused: the tasks go on in threads,
/// channels stop, and the owner is told once.
#[tokio::test]
async fn a_refusal_for_want_of_permission_falls_back_to_threads_and_tells_the_owner_once() {
    let r = rig(
        vec![start(&[REEF, BUOY]), Scripted::text("Started.")],
        |dir| {
            vec![
                (REEF, plans(dir, "Reef charted.")),
                (BUOY, plans(dir, "Buoy painted.")),
            ]
        },
        true,
        |_| {},
    )
    .await;
    r.fake.refuse_channels(true);
    r.say((ANA, "ana"), Some(LAB), ASK);
    until(&r, "a thread for each task", || {
        r.fake.threads().iter().filter(|t| t.parent == LAB).count() == 2
    })
    .await;
    until(&r, "the owner's line", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains("I can't make task channels"))
    })
    .await;
    open_gate(&r);
    tokio::time::sleep(Duration::from_secs(1)).await;
    let lines: Vec<_> = r
        .posted(ANA_DM)
        .into_iter()
        .filter(|m| m.content.contains("I can't make task channels"))
        .collect();
    assert_eq!(lines.len(), 1, "once, across two tasks: {lines:#?}");
    assert!(lines[0]
        .content
        .contains("Manage Channels and Manage Roles"));
    assert!(lines[0].content.contains("permissions=326686067792"));
    assert!(made(&r).is_empty());
}

/// (g) At its end the channel moves under the done category, is named with
/// its mark, and the owner can read it and no longer write in it.
#[tokio::test]
async fn an_ended_tasks_channel_moves_to_the_done_category_and_closes_to_the_owner() {
    let r = rig(
        vec![start(&[REEF]), Scripted::text("Started.")],
        |dir| vec![(REEF, plans(dir, "Reef charted, all of it."))],
        true,
        |_| {},
    )
    .await;
    r.say((ANA, "ana"), None, ASK);
    until(&r, "a channel", || made(&r).len() == 1).await;
    let id = made(&r)[0].id;
    open_gate(&r);
    until(&r, "the channel moved", || {
        made(&r).first().is_some_and(|c| c.names.len() == 2)
    })
    .await;
    let c = made(&r).remove(0);
    let done = categories(&r)
        .into_iter()
        .find(|k| k.name == "⚙️ Theseus tasks · done")
        .expect("the done category");
    assert_eq!((c.id, c.parent), (id, Some(done.id)), "{c:#?}");
    assert!(c.names[1].starts_with("✅-"), "{:?}", c.names);
    let ana = c
        .overwrites
        .iter()
        .find(|o| o.id == ANA)
        .expect("the owner");
    assert_ne!(ana.deny & SEND_MESSAGES, 0, "read-only: {ana:?}");
    assert_ne!(ana.allow & VIEW_CHANNEL, 0, "still readable: {ana:?}");
    // The report's first line links the channel (theseus-ypnxt.5).
    until(&r, "the report in the channel", || {
        said(&r, id).contains("done")
    })
    .await;
    until(&r, "the report in the DM", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains("finished"))
    })
    .await;
    let report = r
        .posted(ANA_DM)
        .into_iter()
        .find(|m| m.content.contains("finished"))
        .unwrap();
    let first = report.content.lines().next().unwrap_or_default();
    assert!(first.contains(&format!("<#{id}>")), "{}", report.content);
}

/// (h) At the cap, the next qualifier goes on as before, silently.
#[tokio::test]
async fn at_the_cap_a_qualifying_task_goes_on_without_a_channel() {
    let r = rig(
        vec![start(&[REEF, BUOY]), Scripted::text("Started.")],
        |dir| {
            vec![
                (REEF, plans(dir, "Reef charted.")),
                (BUOY, plans(dir, "Buoy painted.")),
            ]
        },
        true,
        |c| c.discord.task_channel_cap = 1,
    )
    .await;
    r.say((ANA, "ana"), None, ASK);
    until(&r, "the first channel", || made(&r).len() == 1).await;
    until(&r, "both tasks running", || {
        r.core.kernel.tasks(None).map(|t| t.len()).unwrap_or(0) == 2
    })
    .await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(made(&r).len(), 1, "{:#?}", made(&r));
    assert!(
        !r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains("I can't make task channels")),
        "the cap is silent"
    );
    open_gate(&r);
}

/// (i) A channel that ended `task_channel_keep_days` ago is deleted by the
/// sweep, with its row; a newer one stays.
#[tokio::test]
async fn the_sweep_deletes_a_channel_that_ended_long_ago_and_keeps_a_newer_one() {
    let r = rig(vec![Scripted::text("Hello.")], |_| vec![], true, |_| {}).await;
    let shared = crate::runtime::shared_for_tests(&r.core);
    shared.task_threads.load(&r.core.store);
    let day = 24 * 3_600_000;
    let now = theseus_protocol::now_unix_ms();
    for (task, ended) in [("exe_old", now - 20 * day), ("exe_new", now - day)] {
        let v = wv(task, "done");
        let channel = shared
            .task_channel_create(DEFAULT_GUILD, &channel_name(Mark::Done, task, "old"), &v)
            .await
            .expect("a channel");
        shared.task_threads.add(Made {
            task: task.into(),
            session: format!("ses_{task}"),
            thread: channel,
            kind: PlaceKind::Channel,
            parent: LAB,
            parent_session: "ses_parent".into(),
            name: "x".into(),
            guild: Some(DEFAULT_GUILD),
            private: true,
            ceiling: None,
            created_ms: ended - day,
            ended_ms: Some(ended),
        });
    }
    shared.sweep_once().await;
    let live: Vec<_> = made(&r);
    assert_eq!(live.len(), 1, "{live:#?}");
    let rows = shared.task_threads.all();
    assert_eq!(
        rows.iter().map(|m| m.task.as_str()).collect::<Vec<_>>(),
        ["exe_new"]
    );
    assert_eq!(rows[0].thread, live[0].id);
}

/// A task that began as a thread and then plans two steps is moved: the
/// thread says where it went, and the session's posts go to the channel.
#[tokio::test]
async fn a_task_with_a_thread_that_comes_to_qualify_moves_to_a_channel() {
    let r = rig(
        vec![start(&[REEF]), Scripted::text("Started.")],
        |dir| {
            let late = [vec![gate(dir), plan_steps()], sleeps(3, "Reef charted.")].concat();
            vec![(REEF, late)]
        },
        true,
        |_| {},
    )
    .await;
    r.say((ANA, "ana"), Some(LAB), ASK);
    until(&r, "its thread", || {
        r.fake.threads().iter().any(|t| t.parent == LAB)
    })
    .await;
    assert!(made(&r).is_empty(), "no plan yet: a thread, as before");
    let thread = r.fake.threads().remove(0);
    open_gate(&r);
    until(&r, "its channel", || made(&r).len() == 1).await;
    let c = made(&r).remove(0);
    until(&r, "the thread says where it went", || {
        said(&r, thread.id).contains(&format!("moved to <#{}>", c.id))
    })
    .await;
    let task = task_session(&r);
    until(&r, "the session's posts go to the channel", || {
        r.core.outbox.target(&task).as_deref() == Some(format!("discord:channel:{}", c.id).as_str())
    })
    .await;
    until(&r, "the first line says where it came from", || {
        said(&r, c.id).contains(&format!("↩ from <#{LAB}>"))
    })
    .await;
    // Live progress is best effort: what the task did before its channel
    // stood is in the thread. Its end is in the channel.
    until(&r, "the report in the channel", || {
        said(&r, c.id).contains("done")
    })
    .await;
    until(&r, "the channel moved to the done category", || {
        made(&r).first().is_some_and(|c| c.names.len() == 2)
    })
    .await;
}

const DAN: u64 = 900_000_000_000_000_404;
const HELPERS: u64 = 900_000_000_000_000_777;

/// A task with a channel, shared with cy through `/task share:`, and a role
/// that a person gave View and Send by hand; then the task ends. Gives the rig,
/// the channel's id and the task's id.
async fn ended_with_a_guest_and_a_role() -> (Rig, u64, String) {
    let mut g = guild();
    g.roles.push(Role {
        id: HELPERS,
        name: "helpers".into(),
        permissions: VIEW_CHANNEL | SEND_MESSAGES,
    });
    g.members.push(Member {
        id: DAN,
        name: "dan".into(),
        roles: vec![HELPERS],
        bot: false,
    });
    let parent = vec![start(&[REEF]), Scripted::text("Started.")];
    let r = Rig::start_tweaked(
        |dir, _| model(parent, vec![(REEF, plans(dir, "Reef charted, all of it."))]),
        g,
        &bindings(true),
        &[],
        |c| {
            tweak(c);
            c.tools.defer.always_loaded.push("tasks".into());
            c.discord.forum = false;
            c.discord.task_home = String::new();
        },
    )
    .await;
    r.say((ANA, "ana"), None, ASK);
    until(&r, "a channel", || made(&r).len() == 1).await;
    let id = made(&r)[0].id;
    let task = r.core.kernel.tasks(None).unwrap()[0].id.to_string();
    let who = Sender {
        user: ANA,
        name: "ana",
        channel: None,
    };
    let short = task[task.len() - 6..].to_string();
    let iid = r
        .fake
        .slash_with(&who, "task", &[("name", &short)], &[("share", CY)])
        .unwrap();
    assert!(answered(&r, &iid).await.contains("can watch and comment"));
    let by_hand = Overwrite {
        id: HELPERS,
        kind: 0,
        allow: VIEW_CHANNEL | SEND_MESSAGES,
        deny: 0,
    };
    assert!(r.fake.put_overwrite(id, by_hand));
    assert!(r.fake.overwrites_of(id).iter().any(|o| o.id == CY));
    open_gate(&r);
    until(&r, "the channel closed", || {
        made(&r).first().is_some_and(|c| c.names.len() == 2)
    })
    .await;
    (r, id, task)
}

/// (i) The owner's rule: at the end nobody but the operator keeps access. The
/// close replaces the whole overwrite set, so a guest's member overwrite and a
/// role someone added by hand are gone; `@everyone` cannot view, the owners
/// read and cannot write, the bot keeps its own. Who can view then, by the
/// crate's own `viewers` reckoning, is the owners.
#[tokio::test]
async fn a_closed_task_channel_is_the_operators_alone() {
    let (r, id, _) = ended_with_a_guest_and_a_role().await;
    let c = made(&r).remove(0);
    let mut named: Vec<u64> = c.overwrites.iter().map(|o| o.id).collect();
    named.sort_unstable();
    let mut want = vec![DEFAULT_GUILD, ANA, BEN, BOT_ID];
    want.sort_unstable();
    assert_eq!(
        named, want,
        "the guest and the role are gone: {:#?}",
        c.overwrites
    );
    let of = |who: u64| c.overwrites.iter().find(|o| o.id == who).unwrap();
    assert_eq!(of(DEFAULT_GUILD).deny & VIEW_CHANNEL, VIEW_CHANNEL);
    for owner in [ANA, BEN] {
        let o = of(owner);
        assert_eq!(
            o.allow & (VIEW_CHANNEL | READ_HISTORY),
            VIEW_CHANNEL | READ_HISTORY
        );
        assert_eq!(o.deny & SEND_MESSAGES, SEND_MESSAGES, "{o:?}");
    }
    // Who can view, over the guild's roles and the stored overwrites.
    let over: Vec<PermissionOverwrite> = c
        .overwrites
        .iter()
        .map(|o| PermissionOverwrite {
            allow: Permissions::from_bits_truncate(o.allow),
            deny: Permissions::from_bits_truncate(o.deny),
            id: Id::new(o.id),
            kind: if o.kind == 0 {
                PermissionOverwriteType::Role
            } else {
                PermissionOverwriteType::Member
            },
        })
        .collect();
    let everyone = Permissions::from_bits_truncate(VIEW_CHANNEL | SEND_MESSAGES | READ_HISTORY);
    let roles = [
        (Id::new(DEFAULT_GUILD), everyone),
        (
            Id::new(HELPERS),
            Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES,
        ),
    ];
    let guild = crate::viewers::Guild {
        id: Id::new(DEFAULT_GUILD),
        owner: Id::new(ANA),
        roles: &roles,
    };
    let member = |id: u64, roles: &[u64]| crate::viewers::Member {
        id,
        name: id.to_string(),
        roles: roles.iter().map(|r| Id::new(*r)).collect(),
    };
    let members = [
        member(ANA, &[]),
        member(BEN, &[]),
        member(CY, &[]),
        member(DAN, &[HELPERS]),
    ];
    let seen = crate::viewers::can_view(&guild, ChannelType::GuildText, &over, &members, BOT_ID);
    let mut seen: Vec<u64> = seen.iter().map(|m| m.id).collect();
    seen.sort_unstable();
    assert_eq!(seen, vec![ANA, BEN], "channel {id}");
}

/// (j) The end clears the live guest list (the report's count is the
/// comments', untouched), and a `share:` on the ended task is refused.
#[tokio::test]
async fn an_ended_task_lists_no_guests_and_refuses_a_share() {
    let (r, _, task) = ended_with_a_guest_and_a_role().await;
    until(&r, "the guest list cleared", || {
        let guests: Vec<u64> = r
            .core
            .store
            .get_meta(&format!("discord.task_guests.{task}"))
            .unwrap()
            .unwrap_or_default();
        guests.is_empty()
    })
    .await;
    let who = Sender {
        user: ANA,
        name: "ana",
        channel: None,
    };
    let short = task[task.len() - 6..].to_string();
    let iid = r
        .fake
        .slash_with(&who, "task", &[("name", &short)], &[("share", BEN)])
        .unwrap();
    let line = answered(&r, &iid).await;
    assert!(
        line.contains("has ended; its place is yours alone"),
        "{line}"
    );
    assert!(made(&r)[0]
        .overwrites
        .iter()
        .all(|o| o.id != BEN || o.deny & SEND_MESSAGES != 0));
}
