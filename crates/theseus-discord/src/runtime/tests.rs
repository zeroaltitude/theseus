//! The runtime module's own tests: the slash command table, the control
//! words, the confirm ids, the places and their routes, and the helpers the
//! rest of the crate's tests build their cores and places with.
//!
//! Moved out of `runtime.rs` by theseus-st2wz: that file sat two lines under
//! its 3,500-line ceiling in `scripts/long-files.txt`, so every row that
//! wanted a line there had to raise the ceiling first. The module path is
//! unchanged (`crate::runtime::tests`), so every `use` of it across the crate
//! still resolves.

use super::*;
use theseus_protocol::TurnSubmitParams;

#[test]
fn the_slash_commands_list_tasks_and_wakes_and_cancel_one_by_name() {
    let cmds = commands();
    let names: Vec<&str> = cmds.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names[..8],
        ["stop", "new", "status", "tasks", "wakes", "trust", "cancel", "publish"]
    );
    // One required option names a task or a wake (DD8; DD7 called it `task`).
    let cancel = cmds.iter().find(|c| c.name == "cancel").unwrap();
    assert_eq!(cancel.options.len(), 1);
    assert_eq!(cancel.options[0].name, "id");
    assert_eq!(cancel.options[0].required, Some(true));
    assert!(cancel.description.contains("cancel a wake"));
    // One command, one effect (W1): `/stop` halts and keeps the
    // conversation; `/new` alone starts fresh.
    let desc = |n: &str| {
        cmds.iter()
            .find(|c| c.name == n)
            .unwrap()
            .description
            .clone()
    };
    assert_eq!(
        desc("stop"),
        "Stop what Theseus is doing here: its turn, jobs, and queued messages. The \
         conversation goes on"
    );
    assert_eq!(
        desc("new"),
        "Start a fresh session here; the old one stays in the web UI"
    );
    // `/trust`: its one effect.
    assert_eq!(
        desc("trust"),
        "Remove the injection label: this conversation's calls that change things run again"
    );
    assert!(cmds
        .iter()
        .find(|c| c.name == "trust")
        .unwrap()
        .options
        .is_empty());
    // Discord refuses a description over 100 characters.
    assert!(cmds.iter().all(|c| c.description.chars().count() <= 100));
}

/// What a bound place says first, and what `/stop` answers (W1).
#[test]
fn the_bind_notice_and_the_stops_answer_name_each_control_with_its_one_effect() {
    assert_eq!(
        succession::bind_notice(false),
        "🔗 Theseus is bound here; a fresh session starts with your next message. Talk to me \
         in this place. `/stop` halts what I am doing and keeps the conversation, `/new` \
         starts a fresh one, `/trust` trusts the conversation again after it reads web text, \
         and `/status`, `/tasks` and `/wakes` show this place's; everything shows in the web \
         UI."
    );
    assert!(succession::bind_notice(true).contains(". @mention me or reply"));
    let exec = theseus_protocol::ExecutionInfo {
        execution_id: "exe_1".into(),
        session_id: "ses_1".into(),
        kind: "conversation".into(),
        state: "waiting".into(),
        turns: 2,
        interrupted: 0,
        outstanding: 0,
        queued_results: 0,
        budget: Default::default(),
        wake: Value::Null,
        waiting_on: None,
        reports_to: None,
        ended_reason: None,
        created_at_ms: 0,
        updated_at_ms: 0,
        attention: None,
    };
    let mut r = theseus_protocol::ExecutionStopResult {
        execution: exec,
        stopped: true,
        stopped_actions: vec!["act_1".into()],
        verdicts: vec![],
        declined: vec![],
        turn_running: true,
        tasks_running: 0,
        wakes_pending: 0,
        cancelled: vec![],
        left_ended: vec![],
        left_kept: vec![],
    };
    assert_eq!(
        stop_answer(&r),
        "⏹️ Stopped this session's work (1 running action(s) told to stop). The conversation \
         goes on; `/new` starts a fresh one."
    );
    (r.tasks_running, r.wakes_pending) = (2, 1);
    assert!(stop_answer(&r).ends_with(
        " Its 2 task(s) and 1 wake(s) go on: `/tasks` and `/wakes` list them, and \
         `/cancel <id>` stops one."
    ));
    r.stopped = false;
    r.execution.state = "cancelled".into();
    assert_eq!(
        stop_answer(&r),
        "Nothing to stop: this session's execution has ended (cancelled). `/new` starts a \
         fresh one."
    );
}

#[test]
fn controls_and_confirm_ids_parse() {
    assert_eq!(parse_control("/stop"), Some(Control::Stop));
    // `/cancel` names a task now (DD7); alone, it asks which.
    assert_eq!(
        parse_control("  /cancel a1b2c3"),
        Some(Control::Cancel(Some("a1b2c3".into())))
    );
    assert_eq!(parse_control("/cancel"), Some(Control::Cancel(None)));
    assert_eq!(parse_control("/tasks"), Some(Control::Tasks));
    assert_eq!(parse_control("/wakes"), Some(Control::Wakes));
    assert_eq!(parse_control("/new"), Some(Control::New));
    assert_eq!(parse_control("/status"), Some(Control::Status));
    // A typed `/trust`: the place fills in who typed it, and where.
    assert_eq!(parse_control("/trust"), Some(Control::Trust(None)));
    assert_eq!(parse_control("/trusted"), None);
    assert_eq!(parse_control("please /stop"), None);
    assert_eq!(parse_control("/stopper"), None);
    let press = |approve, corr: &str| Press {
        approve,
        corr: corr.into(),
    };
    assert_eq!(
        parse_confirm_id("confirm:approve:act_1"),
        Some(press(true, "act_1"))
    );
    assert_eq!(
        parse_confirm_id("confirm:decline:act_2"),
        Some(press(false, "act_2"))
    );
    // "Approve + trust session" is no button any more (R6, theseus-gyjyo.8).
    assert_eq!(parse_confirm_id("confirm:trust:act_3"), None);
    assert_eq!(parse_confirm_id("confirm:maybe:x"), None);
    assert_eq!(parse_confirm_id("other"), None);
    assert!(terminal(Some("cancelled")));
    assert!(!terminal(Some("waiting")));
}

#[test]
fn mention_only_channels_answer_mentions_and_replies_only() {
    let (bot, role) = (1618033988749894848u64, 42u64);
    assert!(addressed("hi", &[bot], &[], None, bot, &[role]));
    assert!(addressed(
        "<@1618033988749894848> hi",
        &[],
        &[],
        None,
        bot,
        &[role]
    ));
    assert!(addressed(
        "<@!1618033988749894848> hi",
        &[],
        &[],
        None,
        bot,
        &[role]
    ));
    assert!(addressed("<@&42> hi", &[], &[role], None, bot, &[role]));
    assert!(addressed("thanks", &[], &[], Some(bot), bot, &[role]));
    assert!(!addressed(
        "@Tabitha/Claude hi",
        &[7],
        &[],
        None,
        bot,
        &[role]
    ));
    assert!(!addressed("hi all", &[], &[9], Some(7), bot, &[role]));
    assert_eq!(
        strip_mentions("<@1618033988749894848>  run the tests", bot, &[role]),
        "run the tests"
    );
    assert_eq!(strip_mentions("hey <@&42> look", bot, &[role]), "hey look");
}

#[test]
fn confirm_buttons_carry_the_correlation_id() {
    let c = confirm_buttons("act_9");
    let Component::ActionRow(row) = &c[0] else {
        panic!()
    };
    let ids: Vec<(String, String)> = row
        .components
        .iter()
        .map(|b| match b {
            Component::Button(b) => (b.custom_id.clone().unwrap(), b.label.clone().unwrap()),
            _ => panic!(),
        })
        .collect();
    assert_eq!(
        ids,
        vec![
            ("confirm:approve:act_9".to_string(), "Approve".to_string()),
            ("confirm:decline:act_9".to_string(), "Decline".to_string()),
        ]
    );
}

/// A "should have asked" choice carries the tool and its call, in the
/// menu's option value or the card button's custom id, within Discord's
/// 100 characters (theseus-sgh).
#[test]
fn a_should_have_asked_value_carries_the_tool_and_its_call() {
    let a = Asked {
        tool: "proc.run".into(),
        correlation_id: Some("act_019".into()),
    };
    assert_eq!(asked_value(&a), "proc.run|act_019");
    let tool_only = Asked {
        tool: "fs.write".into(),
        correlation_id: None,
    };
    for (id, values, want) in [
        ("tighten", vec!["proc.run|act_019"], Some(a.clone())),
        ("tighten:proc.run|act_019", vec![], Some(a.clone())),
        ("tighten", vec!["fs.write"], Some(tool_only.clone())),
        ("tighten", vec!["fs.write|"], Some(tool_only.clone())),
        ("tighten", vec![], None),
        ("tightening", vec!["x"], None),
        ("confirm:approve:act_1", vec![], None),
    ] {
        let values: Vec<String> = values.into_iter().map(String::from).collect();
        assert_eq!(parse_asked_pick(id, &values), want, "{id} {values:?}");
    }
    let long = Asked {
        tool: format!("mcp:{}/tool", "s".repeat(60)),
        correlation_id: Some(format!("act_{}", "0".repeat(32))),
    };
    assert_eq!(
        asked_value(&long),
        long.tool,
        "past 100 characters, the tool alone"
    );

    let menu = asked_menu(&[a.clone(), tool_only]);
    let Component::ActionRow(row) = &menu[0] else {
        panic!()
    };
    let Component::SelectMenu(m) = &row.components[0] else {
        panic!()
    };
    assert_eq!(m.custom_id, "tighten");
    assert_eq!(m.placeholder.as_deref(), Some("Should I have asked?"));
    assert_eq!((m.min_values, m.max_values), (Some(1), Some(1)));
    let opts: Vec<(&str, Option<&str>, &str)> = m
        .options
        .as_ref()
        .unwrap()
        .iter()
        .map(|o| (o.label.as_str(), o.description.as_deref(), o.value.as_str()))
        .collect();
    assert_eq!(
        opts,
        [
            (
                "Make actions like this ask in the future",
                Some("Every proc.run call asks you first"),
                "proc.run|act_019"
            ),
            (
                "Make actions like this ask in the future",
                Some("Every fs.write call asks you first"),
                "fs.write"
            )
        ]
    );
    assert!(asked_button(None).is_empty());
    let b = asked_button(Some(&a));
    let Component::ActionRow(row) = &b[0] else {
        panic!()
    };
    let Component::Button(b) = &row.components[0] else {
        panic!()
    };
    assert_eq!(b.custom_id.as_deref(), Some("tighten:proc.run|act_019"));
    assert_eq!(
        b.label.as_deref(),
        Some("Make actions like this ask in the future")
    );
    assert!(
        everywhere(&CoreEvent::PolicyTightened(Default::default()))
            && everywhere(&CoreEvent::PolicyUntightened(Default::default()))
    );
    assert!(!everywhere(&CoreEvent::PolicyNotified(Default::default())));
}

type Mailbox = (
    String,
    mpsc::UnboundedSender<PlaceMsg>,
    mpsc::UnboundedReceiver<PlaceMsg>,
);

/// Three places bound with no session yet, each its own mailbox.
fn fresh_places(shared: &Shared) -> Vec<Mailbox> {
    ["channel:11", "channel:12", "dm:13"]
        .iter()
        .map(|key| {
            let (tx, rx) = mpsc::unbounded_channel();
            shared.routes.lock().unwrap().enrol(key, "", tx.clone());
            (key.to_string(), tx, rx)
        })
        .collect()
}

/// How many messages each mailbox holds now.
fn delivered(places: &mut [Mailbox]) -> Vec<usize> {
    places
        .iter_mut()
        .map(|(_, _, rx)| {
            let mut n = 0;
            while rx.try_recv().is_ok() {
                n += 1;
            }
            n
        })
        .collect()
}

/// Every place with no session yet was bound under the same empty id, so a
/// broadcast reached only the last of them (theseus-2xr6x): N places, N
/// deliveries, one each.
#[tokio::test]
async fn a_broadcast_reaches_every_fresh_place() {
    let d = tempfile::tempdir().unwrap();
    let shared = shared_for_tests(&core_for_tests(d.path()));
    let mut places = fresh_places(&shared);
    shared.broadcast(|| PlaceMsg::Tasks);
    let got = delivered(&mut places);
    assert_eq!(
        got,
        vec![1, 1, 1],
        "each of three fresh places hears it once"
    );
    assert_eq!(got.iter().sum::<usize>(), 3);
}

/// A fresh place that opens its session moves under it and stays one
/// target: the others are still fresh, and still heard.
#[tokio::test]
async fn a_broadcast_reaches_fresh_and_adopted_places_once_each() {
    let d = tempfile::tempdir().unwrap();
    let shared = shared_for_tests(&core_for_tests(d.path()));
    let mut places = fresh_places(&shared);
    {
        let (key, tx, _) = &places[1];
        let mut r = shared.routes.lock().unwrap();
        r.enrol(key, "ses_one", tx.clone());
        assert_eq!(r.by_session.len(), 1);
    }
    shared.broadcast(|| PlaceMsg::Board);
    assert_eq!(delivered(&mut places), vec![1, 1, 1]);
    // A place that goes back to no session is still one target.
    {
        let (key, tx, _) = &places[1];
        let mut r = shared.routes.lock().unwrap();
        r.release(key, "ses_one");
        r.enrol(key, "", tx.clone());
    }
    shared.broadcast(|| PlaceMsg::Board);
    assert_eq!(delivered(&mut places), vec![1, 1, 1]);
    // An unbound place is no longer one.
    shared.routes.lock().unwrap().release("channel:11", "");
    shared.broadcast(|| PlaceMsg::Board);
    assert_eq!(delivered(&mut places), vec![0, 1, 1]);
}

/// A core over a scratch store, with the template's tools and posture,
/// and a provider that is never called.
fn core_for_tests(dir: &std::path::Path) -> Arc<Core> {
    core_with_secrets(dir, theseus_core::secrets::SecretBoard::empty())
}

fn core_with_secrets(
    dir: &std::path::Path,
    secrets: Arc<theseus_core::secrets::SecretBoard>,
) -> Arc<Core> {
    core_with(dir, secrets, |_| {})
}

/// `core_with_secrets`, with `tweak` applied to the config first.
pub(crate) fn core_with(
    dir: &std::path::Path,
    secrets: Arc<theseus_core::secrets::SecretBoard>,
    tweak: impl FnOnce(&mut theseus_core::Config),
) -> Arc<Core> {
    let mut cfg = theseus_core::Config::example();
    cfg.discord.digest_at = vec![];
    cfg.discord.app_emojis = false;
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    cfg.tools.projects_dir = Some(work.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    tweak(&mut cfg);
    let store = theseus_core::store::Store::open(&dir.join("store")).unwrap();
    let fake: Arc<dyn theseus_core::provider::Provider> =
        Arc::new(theseus_core::provider::FakeProvider::scripted(vec![]));
    let providers = [(cfg.model.provider.clone(), fake)].into_iter().collect();
    Core::build(theseus_core::rpc::Parts {
        cfg,
        providers,
        store,
        secrets,
        startup_log: Arc::default(),
        telemetry: Some(theseus_core::telemetry::Telemetry::disabled()),
        scrubber: Arc::new(theseus_core::scrub::Scrubber::default()),
        launcher: Arc::new(theseus_core::toolrun::InlineLauncher),
        config_gate: theseus_core::config_gate::ConfigGate::file("test"),
        toollets: vec![],
        cpu_cores: None,
    })
    .unwrap()
}

/// A core whose model answers from `script`, whose tools ask first, and
/// whose Discord REST points at a port nothing listens on.
pub(super) fn core_scripted(
    dir: &std::path::Path,
    script: Vec<theseus_core::provider::Scripted>,
) -> Arc<Core> {
    core_edited(dir, script, |_| {})
}

/// `core_scripted`, with the config edited first.
pub(super) fn core_edited(
    dir: &std::path::Path,
    script: Vec<theseus_core::provider::Scripted>,
    edit: impl FnOnce(&mut theseus_core::Config),
) -> Arc<Core> {
    let mut cfg = theseus_core::Config::example();
    edit(&mut cfg);
    cfg.server.state_dir = dir.to_string_lossy().into_owned();
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    cfg.tools.projects_dir = Some(work.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.policy.enforcement = theseus_core::policy::Posture::Approve;
    cfg.discord.rest_proxy = Some("127.0.0.1:9".into());
    cfg.discord.gateway_proxy = Some("ws://127.0.0.1:9".into());
    let store = theseus_core::store::Store::open(&dir.join("store")).unwrap();
    let fake: Arc<dyn theseus_core::provider::Provider> =
        Arc::new(theseus_core::provider::FakeProvider::scripted(script));
    let providers = [(cfg.model.provider.clone(), fake)].into_iter().collect();
    Core::build(theseus_core::rpc::Parts {
        cfg,
        providers,
        store,
        secrets: theseus_core::secrets::SecretBoard::empty(),
        startup_log: Arc::default(),
        telemetry: Some(theseus_core::telemetry::Telemetry::disabled()),
        scrubber: Arc::new(theseus_core::scrub::Scrubber::default()),
        launcher: Arc::new(theseus_core::toolrun::InlineLauncher),
        config_gate: theseus_core::config_gate::ConfigGate::file("test"),
        toollets: vec![],
        cpu_cores: None,
    })
    .unwrap()
}

/// A DM place on `core`'s session `sid`, driven directly: the gateway is
/// not faked, so a test calls its handlers. Its mailbox comes back with it.
pub(super) fn place_for_tests(
    core: &Arc<Core>,
    sid: &str,
) -> (Place, mpsc::UnboundedReceiver<PlaceMsg>) {
    let shared = shared_for_tests(core);
    let (tx, rx) = mpsc::unbounded_channel();
    let (lane, _) = mpsc::unbounded_channel();
    shared
        .routes
        .lock()
        .unwrap()
        .by_session
        .insert(sid.to_string(), tx.clone());
    let place = Place {
        renderer: shared.renderer(),
        view: show::View::new(show::Show::default()),
        shared,
        key: "dm:42".into(),
        target: "discord:dm:42".into(),
        kind: "dm",
        label: "zeroaltitude".into(),
        channel: None,
        users: vec![42],
        mention_only: false,
        session_id: sid.to_string(),
        lane,
        inflight: false,
        steering: Default::default(),
        queued: Vec::new(),
        saw_failure: false,
        stopped_turn: None,
        stopping: false,
        last_activity_ms: 0,
        fresh: false,
        tx,
    };
    (place, rx)
}

/// What the binding logs at info or louder (theseus-j7xi): a subscriber
/// for this thread alone, which a current-thread test's tasks share.
#[derive(Clone, Default)]
struct Loud(Arc<Mutex<Vec<String>>>);

impl tracing::Subscriber for Loud {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, e: &tracing::Event<'_>) {
        let m = e.metadata();
        if matches!(
            *m.level(),
            tracing::Level::ERROR | tracing::Level::WARN | tracing::Level::INFO
        ) {
            self.0
                .lock()
                .unwrap()
                .push(format!("{} {}", m.level(), m.target()));
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// theseus-j7xi: the binding's live progress is best-effort, so it
/// ignores `events.lost` and `execution.changed` (theseus-in3, 9c). The
/// route drops `events.lost`, which names no session, and hands
/// `execution.changed` to its session's place, whose renderer draws
/// nothing for it. The place sends its lane nothing for either, nothing
/// fails, and nothing is logged above debug. A turn's start, sent after
/// them, is still drawn: the place goes on as before.
#[tokio::test]
async fn a_place_draws_nothing_for_events_lost_or_execution_changed() {
    let loud = Loud::default();
    let _guard = tracing::subscriber::set_default(loud.clone());
    let d = tempfile::tempdir().unwrap();
    let core = core_for_tests(d.path());
    let rec = theseus_core::session::SessionRecord::new(
        theseus_protocol::SessionKind::Conversation,
        None,
    );
    let sid = rec.session_id.clone();
    core.store.put_session(&sid, &rec).unwrap();
    let (mut place, mut mailbox) = place_for_tests(&core, &sid);
    let (lane_tx, mut lane) = mpsc::unbounded_channel();
    place.lane = lane_tx;
    let (notes, rx) = mpsc::unbounded_channel();
    tokio::spawn(route(place.shared.clone(), rx));
    let view: theseus_protocol::ExecutionView = serde_json::from_value(json!({
        "position": 7, "at_ms": 1, "execution_id": "exec_j7xi", "session_id": sid,
        "kind": "conversation", "state": "waiting", "previous": "running",
        "attention": {"level": "ready", "label": "ready", "since_ms": 1}
    }))
    .unwrap();
    // What the core logged as it was built is not the binding's.
    loud.0.lock().unwrap().clear();
    for e in [
        CoreEvent::EventsLost(theseus_protocol::EventsLost {
            dropped: 865,
            streams: vec!["executions".into(), format!("session:{sid}")],
        }),
        CoreEvent::ExecutionChanged(view),
        CoreEvent::TurnStarted(theseus_protocol::TurnStarted {
            session_id: sid.clone(),
            turn_id: "turn_j7xi".into(),
            ..Default::default()
        }),
    ] {
        notes.send(e.notification()).unwrap();
    }
    // The route hands the place what names its session, in order; the
    // turn's start comes last.
    let mut got = vec![];
    loop {
        let m = tokio::time::timeout(Duration::from_secs(5), mailbox.recv())
            .await
            .expect("the route handed the place its events")
            .unwrap();
        let PlaceMsg::Event(e) = &m else {
            continue;
        };
        got.push(e.method());
        let last = matches!(**e, CoreEvent::TurnStarted(_));
        place.handle(m).await;
        if last {
            break;
        }
    }
    assert_eq!(
        got,
        ["execution.changed", "turn.started"],
        "events.lost names no session, and the route drops it"
    );
    // Only the turn's start reached the lane: typing, and the held turn.
    let mut sent = vec![];
    while let Ok(m) = lane.try_recv() {
        sent.push(match m {
            LaneMsg::Live(crate::render::Op::Typing) => "typing".to_string(),
            LaneMsg::Live(op) => format!("live {:?}", op.key()),
            LaneMsg::Held(turns) => format!("held {turns:?}"),
            _ => "other".to_string(),
        });
    }
    assert_eq!(sent, ["typing", r#"held ["turn_j7xi"]"#]);
    assert!(!place.saw_failure);
    assert!(
        loud.0.lock().unwrap().is_empty(),
        "logged above debug: {:?}",
        loud.0.lock().unwrap()
    );
}

/// `/stop` (W1) halts the session's work and keeps the place on the same
/// session: the approval it waited on is declined, and the next message
/// continues it. `/new` alone starts a fresh session.
#[tokio::test]
async fn stop_keeps_the_places_session_and_new_alone_starts_a_fresh_one() {
    use theseus_core::provider::Scripted;
    let d = tempfile::tempdir().unwrap();
    let core = core_scripted(
        d.path(),
        vec![
            Scripted::tools(
                "Writing.",
                &[("w1", "fs_write", json!({"path": "a.txt", "content": "x\n"}))],
            ),
            Scripted::text("Understood: not written."),
        ],
    );
    let rec = theseus_core::session::SessionRecord::new(
        theseus_protocol::SessionKind::Conversation,
        None,
    );
    let sid = rec.session_id.clone();
    core.store.put_session(&sid, &rec).unwrap();
    core.outbox.bind_place("dm:42", &sid).unwrap();
    // The binding binds the DM, so its person is its owner (the place
    // rule, theseus-zmgb).
    core.bind_places(vec![theseus_core::places::BoundPlace {
        target: "discord:dm:42".into(),
        name: "DM".into(),
        private: false,
        ..Default::default()
    }]);
    let (rpc, _notes) = RpcClient::connect(core.clone(), Client::new("test", Surface::Cli));
    let first: TurnSubmitResult = rpc
        .call(
            theseus_protocol::method::TURN_SUBMIT,
            TurnSubmitParams {
                session_id: Some(sid.clone()),
                input: "FIRST write a file".into(),
                author: Some("test".into()),
                attachments: vec![],
                reply_to: None,
                speaking: None,
                in_reply_to: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(first.awaiting_confirm.is_some(), "it asks first");
    let (mut place, mut rx) = place_for_tests(&core, &sid);
    let answer = place.control(Control::Stop, "discord:zeroaltitude").await;
    assert_eq!(
        answer,
        "⏹️ Stopped this session's work (0 running action(s) told to stop). The \
         conversation goes on; `/new` starts a fresh one."
    );
    assert_eq!(place.session_id, sid, "the same session");
    assert!(core.kernel.pending_confirms().unwrap().is_empty());
    // The next message continues it.
    place.submit(vec![Inbound {
        author: "zeroaltitude".into(),
        author_id: 42,
        text: "SECOND never mind".into(),
        files: None,
        message: Id::new(7),
        channel: Id::new(43),
        guild: None,
        thread: None,
        reply_turn: None,
        task: None,
    }]);
    let done = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(PlaceMsg::SubmitDone(r)) = rx.recv().await {
                break r;
            }
        }
    })
    .await
    .unwrap();
    done.unwrap();
    let rec: theseus_core::session::SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
    assert_eq!(rec.turns, 2, "both turns in the one session");
    // `/new` alone starts a fresh one, with the next message
    // (theseus-emqx: `runtime/tests_succession.rs`).
    let answer = place.control(Control::New, "discord:zeroaltitude").await;
    assert!(answer.starts_with("🆕 A fresh session starts"), "{answer}");
    assert!(place.ready().await);
    assert_ne!(place.session_id, sid);
    assert_eq!(
        core.outbox.place_session("dm:42").unwrap().as_deref(),
        Some(place.session_id.as_str())
    );
}

/// Fail closed (theseus-qa0): with its token resolving the binding
/// waits, and with it failed the binding says why and lets the driver go
/// on. It never starts connecting either way.
#[tokio::test]
async fn the_binding_never_connects_without_its_token() {
    use theseus_core::secrets::{Secret, SecretBoard};
    let d = tempfile::tempdir().unwrap();
    let name = "discord_bot_token".to_string();
    let board = SecretBoard::new([name.clone()], std::time::Instant::now());
    let core = core_with_secrets(d.path(), board.clone());
    let path = d.path().join("bindings.toml");
    std::fs::write(&path, crate::EXAMPLE_BINDINGS).unwrap();
    let cfg = core.cfg.discord.clone();
    assert!(cfg.enabled && cfg.token_secret == name);
    let binding = tokio::spawn(run(core.clone(), cfg, path));
    let state = |core: &Arc<Core>| {
        let b = core.bindings.all().pop().unwrap();
        (b.state, b.detail.unwrap_or_default())
    };
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let (s, detail) = state(&core);
    assert_eq!(s, "waiting", "{detail}");
    assert!(detail.contains("discord_bot_token"), "{detail}");
    board.publish(
        [(name.clone(), Err::<Secret, _>("could not find item".into()))].into(),
        "fake",
    );
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let (s, detail) = state(&core);
    assert_eq!(s, "failed");
    assert!(
        detail.contains("discord_bot_token") && detail.contains("could not find item"),
        "{detail}"
    );
    assert!(!binding.is_finished(), "it waits for a retry");
    binding.abort();
    let phases = core.startup_log.snapshot();
    let p = phases.iter().find(|p| p.name == "discord.token").unwrap();
    assert_eq!(p.detail["outcome"], "failed");
}

/// A pick sends `policy.tighten` for its tool and call, as the presser
/// in their place, through the binding's own connection; the core
/// records it, and the presser alone hears how it went and how to undo
/// it (theseus-sgh).
#[tokio::test]
async fn a_pick_sends_the_tighten_request_and_the_presser_hears_how_to_undo_it() {
    let d = tempfile::tempdir().unwrap();
    let core = core_for_tests(d.path());
    let (rpc, _notes) = RpcClient::connect(core.clone(), Client::new(CLIENT, Surface::Discord));
    let pick = parse_asked_pick(ASKED_MENU, &["proc.run".into()]).unwrap();
    // The binding names who pressed, and where, on every press: a press
    // from it that names none is refused.
    let dm = || {
        Some(DiscordOrigin {
            user_id: "271828182845904523".into(),
            channel_id: "444444444444444444".into(),
            guild_id: None,
        })
    };
    let r = send_tighten(&rpc, &pick, "discord:zeroaltitude", dm()).await;
    let t = r.as_ref().unwrap();
    assert_eq!(
        (t.tool.as_str(), t.posture.as_str(), t.changed),
        ("proc.run", "approve", true)
    );
    assert_eq!(
        (t.tightening.by.as_str(), t.tightening.via.as_str()),
        ("discord:zeroaltitude", "discord:dm")
    );
    assert_eq!(core.health().tightenings[0].tool, "proc.run");
    assert_eq!(
        tightened_reply("proc.run", &r),
        "🔒 `proc.run` asks first from now on. Undo it from the cockpit's Actions view, or \
         `theseus policy untighten proc.run`."
    );
    let again = send_tighten(&rpc, &pick, "discord:zeroaltitude", dm()).await;
    assert_eq!(
        tightened_reply("proc.run", &again),
        "🔒 `proc.run` already asks first: tightened by discord:zeroaltitude."
    );
    let bad = Asked {
        tool: "fs.read".into(),
        correlation_id: Some("act_nope".into()),
    };
    let e = send_tighten(&rpc, &bad, "discord:zeroaltitude", dm()).await;
    assert_eq!(
        tightened_reply("fs.read", &e),
        "⚠️ Could not tighten `fs.read`: no call act_nope"
    );
    let refused: Result<TightenResult, CallError> = Err(CallError {
        code: theseus_protocol::error_code::REFUSED,
        message: "the press from x does not count".into(),
        data: json!({"why": "it came through a connection no listener named"}),
    });
    assert_eq!(
        tightened_reply("proc.run", &refused),
        "🔐 Your press did not count: it came through a connection no listener named."
    );
}

/// The owner's user id, and a user of a place who is not the owner.
pub(super) const OWNER: u64 = 271_828_182_845_904_523;
pub(super) const MALLORY: u64 = 222_222_222_222_222_222;

/// R6 (theseus-gyjyo.8): `/trust` is the Remove press for the `injection` label, and nothing
/// else. It removes that one label as the presser (`content.unlabel`), leaves the session's
/// other labels alone, tells a session without one so and writes nothing, and a person who is
/// not the owner is refused with the reason and the label stays.
#[tokio::test]
async fn trust_removes_the_injection_label_and_nothing_else() {
    use theseus_core::content::{ContentFlags, Flag, ItemKind, Label};
    let d = tempfile::tempdir().unwrap();
    let core = core_with(d.path(), theseus_core::secrets::SecretBoard::empty(), |c| {
        c.discord.rest_proxy = Some("127.0.0.1:9".into());
        c.places.owner = Some(vec![format!("discord:{OWNER}")]);
    });
    let mut rec = theseus_core::session::SessionRecord::new(
        theseus_protocol::SessionKind::Conversation,
        None,
    );
    let sid = rec.session_id.clone();
    let label = |flag: Flag, at_ms: u64| Label {
        flag,
        node: Some("nod_e1".into()),
        kind: ItemKind::FetchedPage,
        source: "http.fetch notes.example/today".into(),
        confidence: 0.93,
        band: flag.bar(),
        at_ms,
        judgment: String::new(),
    };
    let mut flags = ContentFlags::default();
    assert!(flags.raise(label(Flag::Injection, 11)));
    assert!(flags.raise(label(Flag::Secret, 12)));
    rec.data = Some(Box::new(flags));
    core.store.put_session(&sid, &rec).unwrap();
    let (mut place, _rx) = place_for_tests(&core, &sid);
    let dm = |user: u64| {
        Some(DiscordOrigin {
            user_id: user.to_string(),
            channel_id: "444444444444444444".into(),
            guild_id: None,
        })
    };
    let ledger = || {
        core.store
            .ledger_tail::<theseus_core::ledger::LedgerRow>(100_000)
            .unwrap()
    };
    let rows = |kind: &str| -> Vec<Value> {
        ledger()
            .into_iter()
            .filter(|(_, r)| r.kind == kind)
            .map(|(_, r)| r.data)
            .collect()
    };
    let flags_now = || -> ContentFlags {
        let rec: theseus_core::session::SessionRecord =
            core.store.get_session(&sid).unwrap().unwrap();
        rec.data.map(|b| *b).unwrap_or_default()
    };

    // Someone who is not the owner: refused with the reason, and the label stays.
    let answer = place
        .control(Control::Trust(dm(MALLORY)), "discord:mallory")
        .await;
    assert_eq!(
        answer,
        format!(
            "🔐 Your /trust did not count: discord:{MALLORY} is not an owner. The `injection` \
             label stays."
        )
    );
    assert!(flags_now().has(Flag::Injection));
    assert!(rows("content.removed").is_empty());

    // The owner: the injection label goes, the secret one stays, one row says who.
    let answer = place
        .control(Control::Trust(dm(OWNER)), "discord:zeroaltitude")
        .await;
    assert_eq!(
        answer,
        "Removed the `injection` label (by discord:zeroaltitude): calls that act run again."
    );
    let now = flags_now();
    assert!(!now.has(Flag::Injection) && now.has(Flag::Secret));
    let removed = rows("content.removed");
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0]["flag"], "injection");

    // Again: no injection label, so it says so and writes nothing.
    let before = ledger().len();
    let answer = place
        .control(Control::Trust(dm(OWNER)), "discord:zeroaltitude")
        .await;
    assert_eq!(
        answer,
        "This conversation carries no `injection` label, so there is nothing to remove."
    );
    assert_eq!(ledger().len(), before);
    assert!(flags_now().has(Flag::Secret));
}

/// A slash command as the gateway delivers one, from `user`, in
/// `channel`, in `guild` (None: a DM).
pub(super) fn slash(guild: Option<&str>, channel: u64, user: u64, command: &str) -> Interaction {
    let mut v = json!({
        "id": "1000000000000000001",
        "application_id": "1000000000000000002",
        "type": 2,
        "token": "not-a-token",
        "version": 1,
        "authorizing_integration_owners": {},
        "channel": {"id": channel.to_string(), "type": if guild.is_some() { 0 } else { 1 }},
        "user": {"id": user.to_string(), "username": if user == OWNER { "zeroaltitude" } else { "mallory" },
                 "discriminator": "0", "avatar": null},
        "data": {"id": "1000000000000000003", "name": command, "type": 1, "options": []},
    });
    if let Some(g) = guild {
        v["guild_id"] = json!(g);
    }
    serde_json::from_value(v).unwrap()
}

/// Interactions find their place as messages do (theseus-e89): a guild
/// interaction in a channel this daemon does not bind is left alone, with
/// no answer and nothing acted on, even from a user whose DM it binds; a
/// DM's reaches the DM's place; a bound channel's reaches its place, with
/// the presser's ids for `/trust`; and a bound place still refuses a user
/// it does not list.
#[tokio::test]
async fn interactions_find_their_place_as_messages_do_and_leave_the_rest_alone() {
    const GUILD: &str = "314159265358979323";
    const BOUND: u64 = 900_000_000_000_000_001;
    const UNBOUND: u64 = 900_000_000_000_000_002;
    let fake = theseus_sim::fake_discord::FakeDiscord::start();
    let d = tempfile::tempdir().unwrap();
    let addr = fake.addr.clone();
    let core = core_with(d.path(), theseus_core::secrets::SecretBoard::empty(), |c| {
        c.discord.rest_proxy = Some(addr)
    });
    let shared = shared_for_tests(&core);
    let (dm_tx, mut dm_rx) = mpsc::unbounded_channel();
    let (ch_tx, mut ch_rx) = mpsc::unbounded_channel();
    {
        let mut r = shared.routes.lock().unwrap();
        r.by_dm_user.insert(OWNER, dm_tx);
        r.by_channel.insert(BOUND, ch_tx);
        r.users.insert(BOUND, vec![OWNER]);
    }
    // Each answer to an interaction is a POST to its callback.
    let answered = || {
        fake.seen()
            .iter()
            .filter(|s| s.path.contains("/interactions/"))
            .count()
    };
    let commands = || {
        core.store.flush_waiting(); // its rows ride a frame (theseus-ht8b)
        core.store
            .ledger_tail::<theseus_core::ledger::LedgerRow>(100_000)
            .unwrap()
            .into_iter()
            .filter(|(_, r)| r.kind == "discord.command")
            .count()
    };
    let control = |m: Option<PlaceMsg>| match m {
        Some(PlaceMsg::Control { cmd, by, reply }) => {
            let _ = reply.send("done".into());
            (cmd, by)
        }
        _ => panic!("not a control"),
    };

    // A guild channel no place of ours binds, from a user whose DM is
    // bound: no answer, and nothing acted on.
    for cmd in ["stop", "trust"] {
        shared
            .clone()
            .on_interaction(slash(Some(GUILD), UNBOUND, OWNER, cmd))
            .await;
    }
    assert!(dm_rx.try_recv().is_err() && ch_rx.try_recv().is_err());
    assert_eq!((answered(), commands()), (0, 0), "{:?}", fake.seen());
    assert_eq!(shared.board.st.lock().unwrap().interactions, 0);

    // A bound channel still refuses a user it does not list.
    shared
        .clone()
        .on_interaction(slash(Some(GUILD), BOUND, MALLORY, "stop"))
        .await;
    assert!(ch_rx.try_recv().is_err());
    assert_eq!((answered(), commands()), (1, 0));

    // A DM's reaches the DM's place.
    let h = tokio::spawn(shared.clone().on_interaction(slash(
        None,
        444_444_444_444_444_444,
        OWNER,
        "status",
    )));
    let got = tokio::time::timeout(Duration::from_secs(10), dm_rx.recv())
        .await
        .unwrap();
    assert_eq!(
        control(got),
        (Control::Status, "discord:zeroaltitude".into())
    );
    h.await.unwrap();

    // A bound channel's reaches its place, and `/trust` carries who
    // pressed it, and where.
    let h = tokio::spawn(
        shared
            .clone()
            .on_interaction(slash(Some(GUILD), BOUND, OWNER, "trust")),
    );
    let got = tokio::time::timeout(Duration::from_secs(10), ch_rx.recv())
        .await
        .unwrap();
    let origin = DiscordOrigin {
        user_id: OWNER.to_string(),
        channel_id: BOUND.to_string(),
        guild_id: Some(GUILD.into()),
    };
    assert_eq!(
        control(got),
        (Control::Trust(Some(origin)), "discord:zeroaltitude".into())
    );
    h.await.unwrap();
    assert_eq!((answered(), commands()), (3, 2));
    assert!(dm_rx.try_recv().is_err() && ch_rx.try_recv().is_err());
}

/// One lookup for a message and an interaction (theseus-0g4): a DM by its
/// author's DM binding, a guild channel by the channel with its author
/// checked, a mention-only channel says so, and an unbound place is none.
#[test]
fn a_message_or_an_interaction_resolves_to_its_place() {
    let (dm, _dm_rx) = mpsc::unbounded_channel::<PlaceMsg>();
    let (lab, _lab_rx) = mpsc::unbounded_channel::<PlaceMsg>();
    let (talk, _talk_rx) = mpsc::unbounded_channel::<PlaceMsg>();
    let mut r = Routes::default();
    r.by_dm_user.insert(7, dm.clone());
    r.by_channel.insert(10, lab.clone());
    r.users.insert(10, vec![7]);
    r.by_channel.insert(20, talk.clone());
    r.users.insert(20, vec![7, 8]);
    r.mention_only.insert(20);
    let found = |f: Option<Resolved>| f.map(|f| (f.place, f.allowed, f.mention_only)).unwrap();

    // A DM: its author's binding, whatever channel it came through.
    let (place, allowed, mention_only) = found(r.resolve(Some(99), false, Some(7)));
    assert!(place.same_channel(&dm) && allowed && !mention_only);
    assert!(
        r.resolve(Some(99), false, Some(8)).is_none(),
        "8 has no DM binding"
    );
    assert!(r.resolve(Some(99), false, None).is_none());

    // A channel: by the channel; its listed user drives it, another not.
    let (place, allowed, mention_only) = found(r.resolve(Some(10), true, Some(7)));
    assert!(place.same_channel(&lab) && allowed && !mention_only);
    let (place, allowed, _) = found(r.resolve(Some(10), true, Some(8)));
    assert!(place.same_channel(&lab) && !allowed);
    let (_, allowed, _) = found(r.resolve(Some(10), true, None));
    assert!(!allowed, "no author drives nothing");

    // A mention-only channel says so.
    let (place, allowed, mention_only) = found(r.resolve(Some(20), true, Some(8)));
    assert!(place.same_channel(&talk) && allowed && mention_only);

    // An unbound channel, and a guild interaction with no channel: none.
    assert!(r.resolve(Some(30), true, Some(7)).is_none());
    assert!(r.resolve(None, true, Some(7)).is_none());
    // A guild channel never reaches a DM binding (theseus-e89).
    assert!(r.resolve(Some(7), true, Some(7)).is_none());
}
