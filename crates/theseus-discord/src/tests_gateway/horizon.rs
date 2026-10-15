//! The future in Discord (theseus-j90w2; horizon HZ9), end to end on the
//! stand-ins: `/agenda`, `/routine`, `/snooze`, `/watch`, the board's Coming
//! up and Routines, a routine's report and failure, a met watch, and what a
//! shared place is never shown.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use theseus_core::approval::{Client, Surface};
use theseus_core::provider::{
    DeltaSink, FakeProvider, Provider, ProviderError, ProviderFuture, ProviderRequest, Scripted,
};
use theseus_protocol::routines::{RoutineInfo, RoutineListResult};
use theseus_protocol::watches::{WatchCheck, WatchCreateParams, WatchCreateResult};
use theseus_sim::fake_discord::{Command, Modal, Msg, IS_COMPONENTS_V2};

use super::{Rig, ANA, ANA_DM, LAB};
use crate::render::board_v2;
use crate::rpc_client::RpcClient;

/// A model that says one thing, and fails every request while `fail` is set.
struct Model {
    fail: AtomicBool,
}

impl Provider for Model {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            if self.fail.load(Ordering::SeqCst) {
                return Err(ProviderError::Auth {
                    status: 401,
                    message: "Gmail needs a login".into(),
                }
                .into());
            }
            FakeProvider::scripted(vec![Scripted::text("Tide at 06:12.")])
                .stream_message(req, on_delta)
                .await
        })
    }
}

/// A rig whose lab is private (the owner's), the model `model`, and ana's DM
/// opened by a first word (the place the owner's Home is).
async fn rig(model: Arc<Model>, shared_lab: bool) -> Rig {
    let m = model.clone();
    let r = Rig::start_with(move |_, _| m as Arc<dyn Provider>, true, !shared_lab).await;
    r.say((ANA, "ana"), None, "hello");
    r.until("the DM answered", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains("Tide at 06:12."))
    })
    .await;
    r
}

fn owner(r: &Rig) -> Arc<RpcClient> {
    RpcClient::connect(r.core.clone(), Client::new("test", Surface::Cli)).0
}

async fn routine(rpc: &RpcClient, title: &str) -> RoutineInfo {
    let made: theseus_protocol::routines::RoutineEditResult = rpc
        .call(
            theseus_protocol::method::ROUTINE_CREATE,
            theseus_protocol::routines::RoutineCreateParams {
                title: title.into(),
                schedule: "weekdays 08:00".into(),
                brief: "Read the tide table and report it.".into(),
                budget_usd: Some(0.5),
                month_usd: Some(10.0),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    made.routine
}

async fn routines(rpc: &RpcClient) -> Vec<RoutineInfo> {
    let l: RoutineListResult = rpc
        .call(theseus_protocol::method::ROUTINE_LIST, json!({}))
        .await
        .unwrap();
    l.routines
}

/// The routines now, read from the core directly (for a closure `until` polls).
fn now(r: &Rig) -> Vec<RoutineInfo> {
    r.core.routine_list().unwrap().routines
}

/// A slash command typed by ana in `channel`, and what the binding answered.
async fn slash(
    r: &Rig,
    channel: u64,
    command: &str,
    sub: &str,
    options: &[(&str, &str)],
) -> (String, String) {
    let id = r
        .fake
        .command(&Command {
            user: ANA,
            name: "ana",
            channel,
            parent: None,
            command,
            sub,
            options,
        })
        .unwrap();
    let answer = |r: &Rig| {
        r.fake
            .replies()
            .into_iter()
            .find(|x| x.interaction.as_deref() == Some(id.as_str()) && x.content.is_some())
            .and_then(|x| x.content)
    };
    r.until("the command's answer", || answer(r).is_some())
        .await;
    (id.clone(), answer(r).unwrap())
}

/// The board in `channel`: its V2 message with the Clear button.
fn board(r: &Rig, channel: u64) -> Option<Msg> {
    r.posted(channel).into_iter().find(|m| {
        m.flags & IS_COMPONENTS_V2 != 0 && m.buttons.iter().any(|b| b.custom_id == board_v2::CLEAR)
    })
}

fn model(fail: bool) -> Arc<Model> {
    Arc::new(Model {
        fail: AtomicBool::new(fail),
    })
}

/// A reminder in Home, due in `secs`.
fn remind(r: &Rig, secs: u64, note: &str) -> String {
    let home = r.core.home().unwrap();
    let due = r.core.kernel.now_ms() + secs * 1000;
    r.core
        .kernel
        .create_wake(
            &home.execution_id,
            None,
            theseus_kernel::wake_owner::reminder(due, note),
            "the test",
        )
        .unwrap()
        .wake
        .id
}

#[tokio::test]
async fn agenda_renders_its_sections_for_the_owner_in_a_private_place() {
    let r = rig(model(false), false).await;
    let rpc = owner(&r);
    remind(&r, 3 * 3600, "stretch");
    routine(&rpc, "Morning digest").await;
    let (_, said) = slash(&r, LAB, "agenda", "", &[]).await;
    assert!(said.contains("**⏰ Coming up**"), "{said}");
    assert!(said.contains("stretch"), "{said}");
    assert!(said.contains("**🔁 Routines**"), "{said}");
    assert!(said.contains("Morning digest"), "{said}");
    assert!(said.contains("$0 of $10"), "{said}");
    let (_, today) = slash(&r, LAB, "agenda", "", &[("when", "today")]).await;
    assert!(
        today.contains("today") && today.contains("stretch"),
        "{today}"
    );
}

#[tokio::test]
async fn a_shared_place_is_shown_no_routine_and_no_agenda() {
    let r = rig(model(false), true).await;
    let rpc = owner(&r);
    remind(&r, 3600, "stretch");
    routine(&rpc, "Morning digest").await;
    let (_, said) = slash(&r, LAB, "agenda", "", &[]).await;
    assert!(said.contains("yours alone"), "{said}");
    assert!(
        !said.contains("Morning digest") && !said.contains("stretch"),
        "{said}"
    );
    let (_, list) = slash(&r, LAB, "routine", "list", &[]).await;
    assert!(list.contains("yours alone"), "{list}");
    // Autocomplete there offers nothing of the owner's.
    let id = r
        .fake
        .autocomplete(&Command {
            user: ANA,
            name: "ana",
            channel: LAB,
            parent: None,
            command: "routine",
            sub: "run",
            options: &[("name", "")],
        })
        .unwrap();
    r.until("the autocomplete answer", || {
        r.fake
            .replies()
            .iter()
            .any(|x| x.interaction.as_deref() == Some(id.as_str()))
    })
    .await;
    let reply = r
        .fake
        .replies()
        .into_iter()
        .find(|x| x.interaction.as_deref() == Some(id.as_str()))
        .unwrap();
    assert!(reply.choices.is_empty(), "{reply:?}");
    // And no board of the shared place names the routine.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(
        r.fake
            .messages(LAB)
            .iter()
            .all(|m| !m.content.contains("Morning digest") && !m.content.contains("Routines")),
        "{:#?}",
        r.fake.messages(LAB)
    );
}

#[tokio::test]
async fn the_dms_board_has_coming_up_and_routines_with_their_buttons() {
    let r = rig(model(false), false).await;
    let rpc = owner(&r);
    remind(&r, 3600, "stretch");
    let made = routine(&rpc, "Morning digest").await;
    r.until("the board with the future", || {
        board(&r, ANA_DM)
            .is_some_and(|b| b.content.contains("Coming up") && b.content.contains("Routines"))
    })
    .await;
    let b = board(&r, ANA_DM).unwrap();
    assert!(
        b.content.contains("stretch") && b.content.contains("Morning digest"),
        "{}",
        b.content
    );
    let ids: Vec<&str> = b.buttons.iter().map(|x| x.custom_id.as_str()).collect();
    assert!(
        ids.contains(&format!("hr:run:{}", made.id).as_str()),
        "{ids:?}"
    );
    assert!(
        ids.contains(&format!("hr:pause:{}", made.id).as_str()),
        "{ids:?}"
    );
    assert!(b.content.contains("$0 of $10"), "{}", b.content);
    // Within Discord's bounds, whatever it holds.
    let comps = b.components_json.clone();
    let flat = comps.as_array().cloned().unwrap_or_default();
    assert!(crate::render::v2::count(&flat) <= 40);
    assert!(crate::render::v2::text_len(&flat) <= 4000);
}

#[tokio::test]
async fn routine_add_makes_a_routine_that_runs_and_speaks_as_the_routine() {
    let r = rig(model(false), false).await;
    let id = r
        .fake
        .modal(&Modal {
            user: ANA,
            name: "ana",
            channel: LAB,
            custom_id: "hzm:routine",
            fields: &[
                ("title", "Tide check"),
                ("schedule", "weekdays 08:00"),
                ("brief", "Read the tide table and report it."),
                ("budget", "0.50 / 5"),
                ("where", "dm"),
            ],
        })
        .unwrap();
    r.until("the routine made", || {
        r.fake.replies().iter().any(|x| {
            x.interaction.as_deref() == Some(id.as_str())
                && x.content
                    .as_deref()
                    .is_some_and(|c| c.contains("Tide check"))
        })
    })
    .await;
    let rpc = owner(&r);
    let all = routines(&rpc).await;
    assert_eq!(all.len(), 1);
    assert_eq!((all[0].run_usd, all[0].month_usd), (0.5, 5.0));
    // /routine run, by its title.
    let (_, said) = slash(&r, LAB, "routine", "run", &[("name", "tide")]).await;
    assert!(said.contains("is running now"), "{said}");
    r.until("the run's report in the DM", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.starts_with("🔁 **Tide check**"))
    })
    .await;
    let post = r
        .posted(ANA_DM)
        .into_iter()
        .find(|m| m.content.starts_with("🔁 **Tide check**"))
        .unwrap();
    assert!(post.content.contains("run 1"), "{}", post.content);
    assert!(post.content.contains("Tide at 06:12."), "{}", post.content);
}

#[tokio::test]
async fn run_now_and_pause_from_the_routines_list_act_and_settle() {
    let r = rig(model(false), false).await;
    let rpc = owner(&r);
    let made = routine(&rpc, "Morning digest").await;
    let (iid, list) = slash(&r, LAB, "routine", "list", &[]).await;
    assert!(list.contains("Morning digest"), "{list}");
    r.fake
        .press_reply(&iid, "▶ Run now · Morning digest", ANA, "ana")
        .unwrap();
    r.until("the run in the DM", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.starts_with("🔁 **Morning digest**"))
    })
    .await;
    r.fake
        .press_reply(&iid, "⏸ Pause · Morning digest", ANA, "ana")
        .unwrap();
    r.until("the routine paused", || {
        now(&r).iter().any(|x| x.id == made.id && x.paused)
    })
    .await;
    // Each press was answered in words.
    r.until("both presses answered", || {
        let said: Vec<String> = r
            .fake
            .replies()
            .into_iter()
            .filter_map(|x| x.content)
            .collect();
        said.iter().any(|c| c.contains("is running now"))
            && said.iter().any(|c| c.contains("is paused"))
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_routines_failure_posts_with_buttons_once_a_day_and_the_buttons_act() {
    let m = model(false);
    let r = rig(m.clone(), false).await;
    let rpc = owner(&r);
    let made = routine(&rpc, "Morning digest").await;
    m.fail.store(true, Ordering::SeqCst);
    let run = |r: &Rig| {
        let rpc = owner(r);
        let id = made.id.clone();
        async move {
            let _: theseus_protocol::routines::RoutineEditResult = rpc
                .call(
                    theseus_protocol::method::ROUTINE_RUN_NOW,
                    theseus_protocol::routines::RoutineNameParams {
                        id,
                        author: None,
                        discord: None,
                    },
                )
                .await
                .unwrap();
        }
    };
    run(&r).await;
    let is_it = |m: &Msg| m.content.contains("✗ Morning digest failed");
    r.until("the failure post", || r.posted(ANA_DM).iter().any(is_it))
        .await;
    let post = r.posted(ANA_DM).into_iter().find(is_it).unwrap();
    let labels: Vec<&str> = post.buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, ["▶ Run now", "⏸ Pause", "Details"], "{post:?}");
    assert!(!post.silent(), "a failure pings: {post:?}");
    assert!(
        post.content.contains("Gmail needs a login"),
        "{}",
        post.content
    );
    // A second failure the same day posts nothing more.
    r.until("the first run settled", || {
        now(&r).iter().any(|x| x.id == made.id && !x.running)
    })
    .await;
    run(&r).await;
    r.until("the second run settled", || {
        let l = now(&r);
        l.iter()
            .any(|x| x.id == made.id && !x.running && x.runs >= 2 && x.streak >= 2)
    })
    .await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let posts = r.posted(ANA_DM).into_iter().filter(is_it).count();
    assert_eq!(posts, 1, "once a day: {:#?}", r.posted(ANA_DM));
    // Pause on the post: the routine is held and the post becomes the answer.
    r.press(&post.id, "⏸ Pause", (ANA, "ana"));
    r.until("the routine paused by the press", || {
        now(&r).iter().any(|x| x.id == made.id && x.paused)
    })
    .await;
    r.until("the post settled", || {
        r.fake.replies().iter().any(|x| {
            x.response_type == Some(7)
                && x.content
                    .as_deref()
                    .is_some_and(|c| c.contains("✗ Morning digest") && c.ends_with("⏸ Paused"))
        })
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_met_watch_posts_once_and_pings() {
    let r = rig(model(false), false).await;
    let posts = met_watch(&r).await;
    assert_eq!(posts.len(), 1, "once: {posts:#?}");
    assert!(!posts[0].silent(), "a met watch pings: {:?}", posts[0]);
    assert!(
        posts[0].buttons.is_empty(),
        "no verb answers a met watch yet: {:?}",
        posts[0]
    );
}

/// The met watch's post is a reminder's kind, not a notice's: a place that
/// silences notices still hears it (the core's `purpose: watch.met`).
#[tokio::test]
async fn a_met_watch_pings_where_notices_are_silent() {
    let m = model(false);
    let r = Rig::start_tweaked(
        move |_, _| m as Arc<dyn Provider>,
        super::guild(true),
        &super::bindings(true),
        &[],
        |c| c.discord.silent = vec![theseus_core::config::discord::Category::Notices],
    )
    .await;
    r.say((ANA, "ana"), None, "hello");
    r.until("the DM answered", || {
        r.posted(ANA_DM)
            .iter()
            .any(|m| m.content.contains("Tide at 06:12."))
    })
    .await;
    let posts = met_watch(&r).await;
    assert_eq!(posts.len(), 1, "once: {posts:#?}");
    assert!(
        !posts[0].silent(),
        "a met watch pings though notices are silent: {:?}",
        posts[0]
    );
}

/// A file watch made and met: its posts in the owner's DM.
async fn met_watch(r: &Rig) -> Vec<Msg> {
    let rpc = owner(r);
    let file = r.dir.path().join("build.flag");
    let made: WatchCreateResult = rpc
        .call(
            theseus_protocol::method::WATCH_CREATE,
            WatchCreateParams {
                title: "build finished?".into(),
                check: WatchCheck::File {
                    path: file.display().to_string(),
                },
                until: "exists".into(),
                every: Some("1m".into()),
                for_at_most: None,
                then: None,
                session_id: None,
                author: None,
                discord: None,
            },
        )
        .await
        .unwrap();
    r.core
        .watches
        .with(&made.watch.wake_id, |e| e.spec.every_ms = 700)
        .unwrap();
    std::fs::write(&file, "done").unwrap();
    let is_it = |m: &Msg| m.content.contains("build finished") && m.content.contains("👁");
    r.until("the watch met", || r.posted(ANA_DM).iter().any(is_it))
        .await;
    tokio::time::sleep(Duration::from_millis(2500)).await;
    r.posted(ANA_DM).into_iter().filter(is_it).collect()
}

#[tokio::test]
async fn snooze_autocomplete_offers_the_pending_wakes_from_memory_and_the_snooze_acts() {
    let r = rig(model(false), false).await;
    let wake = remind(&r, 7200, "stretch");
    // The snapshot is read soon after the wake is made: ask until it holds it.
    let mut choices = Vec::new();
    for _ in 0..200 {
        let id = r
            .fake
            .autocomplete(&Command {
                user: ANA,
                name: "ana",
                channel: LAB,
                parent: None,
                command: "snooze",
                sub: "",
                options: &[("wake", "")],
            })
            .unwrap();
        r.until("the choices", || {
            r.fake
                .replies()
                .iter()
                .any(|x| x.interaction.as_deref() == Some(id.as_str()))
        })
        .await;
        choices = r
            .fake
            .replies()
            .into_iter()
            .find(|x| x.interaction.as_deref() == Some(id.as_str()))
            .unwrap()
            .choices;
        if !choices.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        choices
            .iter()
            .any(|(n, v)| v == &wake && n.contains("stretch")),
        "{choices:?}"
    );
    // /snooze moves it.
    let (_, said) = slash(&r, LAB, "snooze", "", &[("wake", &wake), ("when", "in 3h")]).await;
    assert!(said.contains("is set again"), "{said}");
}

#[tokio::test]
async fn a_command_check_is_offered_from_the_dm_only() {
    let r = rig(model(false), false).await;
    let (_, said) = slash(&r, LAB, "watch", "add", &[("kind", "command")]).await;
    assert!(said.contains("DM only"), "{said}");
}
