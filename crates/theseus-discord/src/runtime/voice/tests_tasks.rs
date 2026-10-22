//! A task's question and report in the call, and the owner's spoken answer
//! (theseus-bh3t.3): what is said, once, at the next pause; what a spoken
//! "approve", "the second one" or "stop it" does in the core; and what is
//! never said (no call, a shared voice place, a task of a shared place, or
//! the voice place's own report).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_core::approval::{Answerer, Client, Surface};
use theseus_core::places::BoundPlace;
use theseus_core::provider::Scripted;
use theseus_core::push::WorkOut;
use theseus_core::Core;
use theseus_protocol::task_start::TaskStartParams;
use theseus_protocol::work::{answer_options, AnswerOption, Question, WorkView};
use theseus_protocol::{TurnSubmitParams, TurnSubmitResult};
use theseus_voice::{Command, TurnId};
use tokio::sync::mpsc;
use twilight_model::id::Id;

use super::super::tests::core_edited;
use super::super::{Place, PlaceMsg};
use super::tasks::{matched, Pending, Said, Tasks};
use super::tests::{heard, joined, place, LOUNGE, OWNER};
use super::{VoiceTurn, FRAMING};
use crate::rpc_client::RpcClient;
use crate::runtime::voice_status::Handle;

const GUILD: &str = "100000000000000001";

/// A call in the lounge with the tasks' feed on the one work watch.
struct Rig {
    _dir: tempfile::TempDir,
    core: Arc<Core>,
    sid: String,
    place: Place,
    _rx: mpsc::UnboundedReceiver<PlaceMsg>,
    spoken: mpsc::UnboundedReceiver<Command>,
    tasks: Arc<Mutex<Tasks>>,
}

fn bound(lounge_private: bool) -> Vec<BoundPlace> {
    vec![
        BoundPlace {
            target: format!("discord:dm:{OWNER}"),
            name: "DM".into(),
            private: false,
            ..Default::default()
        },
        BoundPlace {
            target: format!("discord:channel:{LOUNGE}"),
            name: "#lounge".into(),
            private: lounge_private,
            guild: Some(GUILD.into()),
            ..Default::default()
        },
    ]
}

async fn rig(script: Vec<Scripted>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let core = core_edited(dir.path(), script, |c| {
        // The model's owner_ask is deferred by default: these scripts call it.
        c.tools.defer.always_loaded.push("ask".into());
    });
    core.bind_places(bound(true));
    let key = format!("channel:{LOUNGE}");
    let (mut p, rx) = place(&core, "");
    let sid = p.shared.open_session(&key, "#lounge").await.unwrap();
    p.session_id = sid.clone();
    p.key = key;
    p.target = format!("discord:channel:{LOUNGE}");
    p.channel = Some(Id::new(LOUNGE));
    p.shared
        .voice
        .names
        .lock()
        .unwrap()
        .insert(OWNER, "zeroaltitude".into());
    let (commands, spoken) = mpsc::unbounded_channel();
    joined(&p, commands.clone());
    let tasks = Arc::new(Mutex::new(Tasks::new(commands, LOUNGE, 0)));
    let handle = Handle::start(
        Arc::clone(&core),
        None,
        Some(Arc::clone(&tasks)),
        LOUNGE,
        1,
        |_| {},
    );
    p.shared.voice.call.lock().unwrap().as_mut().unwrap().status = Some(handle);
    while core.bus.work_watchers() == 0 {
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    Rig {
        _dir: dir,
        core,
        sid,
        place: p,
        _rx: rx,
        spoken,
        tasks,
    }
}

impl Rig {
    /// Views as one committed frame, to the watch.
    fn publish(&self, views: Vec<WorkView>) {
        let out = WorkOut {
            position: 1,
            at_ms: 1,
            views,
            gone: Vec::new(),
            notices: Vec::new(),
            threads: Vec::new(),
        };
        self.core.bus.publish_work(&out, &self.core.tools.scrubber);
    }

    /// The next thing the call is told to say, in a few seconds.
    async fn said(&mut self) -> String {
        match tokio::time::timeout(Duration::from_secs(5), self.spoken.recv()).await {
            Ok(Some(Command::Report { text })) => text,
            other => panic!("a report was expected: {other:?}"),
        }
    }

    /// Everything said so far, after a sentinel report has gone through the
    /// same watch: the watch hears views in order, so nothing earlier is late.
    async fn all_said(&mut self) -> Vec<String> {
        self.publish(vec![task(
            "exe_sentinel",
            "Sentinel",
            "done",
            Some("done"),
            "s",
            None,
        )]);
        let mut out = Vec::new();
        loop {
            let text = self.said().await;
            if text.starts_with("Sentinel") {
                return out;
            }
            out.push(text);
        }
    }

    /// The owner says `text` in the call.
    fn say(&mut self, text: &str) {
        self.place.voice_turn(VoiceTurn {
            serial: 1,
            turn: TurnId(0),
            utterances: vec![heard(text)],
        });
    }
}

/// A task's view, ended or not, with its report.
fn task(
    id: &str,
    title: &str,
    state: &str,
    outcome: Option<&str>,
    line: &str,
    session: Option<&str>,
) -> WorkView {
    let mut v = json!({
        "id": id, "kind": "task", "position": 1, "at_ms": 1, "root": "exe_root",
        "title": title, "state": state,
        "attention": {"level": "idle", "label": "", "since_ms": 1},
        "started_at_ms": 1, "state_since_ms": u64::MAX / 2, "ended_at_ms": u64::MAX / 2,
    });
    if let Some(o) = outcome {
        v["report"] = json!({"outcome": o, "first_line": line});
    }
    if let Some(s) = session {
        v["session_id"] = json!(s);
    }
    serde_json::from_value(v).expect("a work view")
}

/// A question a task asks, as the board shows it.
fn question(
    id: &str,
    by: &str,
    kind: &str,
    prompt: &str,
    tool: Option<&str>,
    choices: &[&str],
) -> WorkView {
    let mut q = json!({
        "id": id, "kind": kind,
        "asked_by": {"id": by, "kind": "task", "title": "Harbour survey"},
        "prompt": prompt, "options": [], "floor": false, "may_answer": true,
        "asked_at_ms": 5, "choices": choices,
    });
    if let Some(t) = tool {
        q["tool"] = json!(t);
    }
    serde_json::from_value(json!({
        "id": id, "kind": "question", "position": 1, "at_ms": 1, "root": "exe_root",
        "title": prompt, "state": "needs_you",
        "attention": {"level": "needs_you", "label": "", "since_ms": 1},
        "started_at_ms": 1, "state_since_ms": 1, "question": q,
    }))
    .expect("a work view")
}

/// A question asked by a CLI turn, for real: its id waits in the core.
async fn asked(core: &Arc<Core>) -> String {
    let rpc = RpcClient::connect(core.clone(), Client::new("test", Surface::Cli)).0;
    let res: TurnSubmitResult = rpc
        .call(
            theseus_protocol::method::TURN_SUBMIT,
            TurnSubmitParams {
                dir: None,
                carried: false,
                prompt: None,
                session_id: None,
                input: "decide".into(),
                profile: None,
                provider: None,
                model: None,
                author: Some("test".into()),
                attachments: vec![],
                reply_to: None,
                opened_from: None,
                thread: None,
                speaking: None,
                in_reply_to: None,
                route_at_ms: None,
                surface: None,
                via: None,
            },
        )
        .await
        .unwrap();
    res.awaiting_confirm.expect("the question waits")
}

fn owner_asks() -> Vec<Scripted> {
    let ask = json!({
        "question": "Which plan for the harbour survey?",
        "options": [{"label": "Careful"}, {"label": "Ship it behind the flag"}],
        "default": 1,
    });
    vec![
        Scripted::tools("", &[("t1", "owner_ask", ask)]),
        Scripted::text("Going on."),
    ]
}

/// The answers the core has recorded, by option.
fn answered(core: &Core) -> Vec<String> {
    core.store
        .ledger_tail::<theseus_core::ledger::LedgerRow>(4000)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == "action.confirm_answered")
        .filter_map(|r| r.data["option"].as_str().map(str::to_string))
        .collect()
}

async fn until(what: &str, mut ok: impl FnMut() -> bool) {
    for _ in 0..500 {
        if ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("never happened: {what}");
}

fn options() -> Vec<AnswerOption> {
    let q: Question = serde_json::from_value(json!({
        "id": "q", "kind": "approval",
        "asked_by": {"id": "exe_t", "kind": "task"},
        "prompt": "", "options": [], "floor": false, "may_answer": true,
    }))
    .unwrap();
    answer_options(&q)
}

/// The words that answer a question, and those that do not: the table.
#[test]
fn the_matcher_reads_whole_utterances_only() {
    let o = options();
    let ans = |s: &str| match matched(s, &o) {
        Some(Said::Answer { option, ack }) => Some((option, ack)),
        _ => None,
    };
    for s in ["Approve.", "approved", "Go ahead!", "do it", "approve it"] {
        assert_eq!(ans(s), Some(("approve".into(), "Approved.".into())), "{s}");
    }
    for s in ["Deny", "no", "Don't.", "reject", "decline"] {
        assert_eq!(ans(s), Some(("decline".into(), "Declined.".into())), "{s}");
    }
    // By position: "the second one" is the second option, whatever its id.
    for s in ["first", "one", "1", "The first one.", "option one"] {
        assert_eq!(ans(s).map(|a| a.0), Some("approve".into()), "{s}");
    }
    for s in [
        "second",
        "two",
        "2",
        "the second one",
        "Option two",
        "number 2",
    ] {
        assert_eq!(
            ans(s),
            Some(("decline".into(), "Option two.".into())),
            "{s}"
        );
    }
    // Past the options, or among other words, it is no answer.
    for s in [
        "third",
        "the third one",
        "approve the deploy and then tell me why",
        "one thing",
        "what's the weather",
        "yes",
        "",
    ] {
        assert_eq!(matched(s, &o), None, "{s}");
    }
    for s in ["stop it", "Cancel it.", "stop the task", "cancel the task"] {
        assert_eq!(matched(s, &o), Some(Said::Stop), "{s}");
    }
    // Deny needs an option that declines.
    let reset = [AnswerOption {
        id: "reset".into(),
        ..o[0].clone()
    }];
    assert_eq!(matched("deny", &reset), None);
    assert!(matches!(
        matched("approve", &reset),
        Some(Said::Answer { .. })
    ));
}

/// A task that ends during the call is told once: its title and a sentence,
/// no code, link or path; the same end seen again is not told again.
#[tokio::test]
async fn a_report_during_the_call_is_spoken_once_and_short() {
    let mut r = rig(vec![]).await;
    let ended = task(
        "exe_t1",
        "Harbour survey",
        "done",
        Some("done"),
        "Counted 14 boats and wrote the notes to `notes.md`. It also fixed the link at https://example.test/x. A third sentence.",
        None,
    );
    r.publish(vec![ended.clone()]);
    let text = r.said().await;
    assert!(
        text.starts_with("Harbour survey is done: Counted 14 boats and wrote the notes to"),
        "{text}"
    );
    assert!(
        !text.contains('`') && !text.contains('/') && !text.contains("example"),
        "{text}"
    );
    assert!(!text.contains("third"), "two sentences at most: {text}");
    // Seen twice, and a restamp of it: still one.
    r.publish(vec![ended.clone(), ended]);
    let rest = r.all_said().await;
    assert!(rest.is_empty(), "{rest:?}");
    // A failure says why.
    r.publish(vec![task(
        "exe_t2",
        "Dock audit",
        "failed",
        Some("failed"),
        "x",
        None,
    )]);
    assert_eq!(r.said().await, "Dock audit failed: x");
}

/// What was there when the call began, and an end from before it, is the past.
#[tokio::test]
async fn the_past_is_not_spoken() {
    let mut r = rig(vec![]).await;
    r.tasks.lock().unwrap().seed(&[task(
        "exe_old",
        "Old job",
        "done",
        Some("done"),
        "Done long ago.",
        None,
    )]);
    let mut stale = task(
        "exe_old2",
        "Older job",
        "done",
        Some("done"),
        "Done before the call.",
        None,
    );
    stale.ended_at_ms = Some(1);
    stale.state_since_ms = 1;
    r.tasks.lock().unwrap().since_ms = 1_000;
    r.publish(vec![
        task(
            "exe_old",
            "Old job",
            "done",
            Some("done"),
            "Done long ago.",
            None,
        ),
        stale,
    ]);
    let said = r.all_said().await;
    assert!(said.is_empty(), "{said:?}");
}

/// A question is told once, with how to answer; "the second one" settles it
/// in the core as the owner, and no voice turn is made of those words.
#[tokio::test]
async fn a_spoken_choice_settles_the_question_as_the_owner() {
    let mut r = rig(owner_asks()).await;
    let corr = asked(&r.core).await;
    let view = question(
        &corr,
        "exe_t1",
        "ask",
        "Which plan for the harbour survey?",
        None,
        &["Careful", "Ship it behind the flag"],
    );
    r.publish(vec![view.clone()]);
    let text = r.said().await;
    assert_eq!(
        text,
        "Harbour survey needs you: Which plan for the harbour survey. one, Careful. two, Ship it behind the flag. Say the option's number."
    );
    r.publish(vec![view]);
    assert!(r.all_said().await.is_empty(), "told once");
    r.say("the second one");
    assert!(!r.place.inflight, "no turn was made of it");
    until("the answer", || !answered(&r.core).is_empty()).await;
    assert_eq!(answered(&r.core), ["choice:1"]);
    assert!(r.core.confirm_list().unwrap().is_empty(), "it is settled");
    assert_eq!(r.said().await, "Option two.");
    // The transcript is in the place's text, as any voice turn's is.
    let posts = super::tests::posts(&r.core, &format!("discord:channel:{LOUNGE}"));
    assert!(
        posts
            .iter()
            .any(|p| p.contains("🎙️ **zeroaltitude**: the second one")),
        "{posts:?}"
    );
    // It was answered once: the same words now are an ordinary turn.
    r.say("the second one");
    assert!(r.place.inflight);
}

/// An approval is told as the tool it wants, not its arguments; "approve" is
/// the approving option.
#[tokio::test]
async fn approve_answers_an_approval_with_its_approving_option() {
    let mut r = rig(vec![Scripted::tools(
        "",
        &[(
            "t1",
            "fs_write",
            json!({"path": "/tmp/x.txt", "content": "hi"}),
        )],
    )])
    .await;
    let corr = asked(&r.core).await;
    r.publish(vec![question(
        &corr,
        "exe_t1",
        "approval",
        "fs.write create projects/beta-summary.txt (40 B)",
        Some("fs.write"),
        &[],
    )]);
    let text = r.said().await;
    assert_eq!(
        text,
        "Harbour survey needs you: wants to run fs write. Say approve or deny."
    );
    assert!(!text.contains("beta-summary"), "{text}");
    r.say("approve");
    until("the answer", || !answered(&r.core).is_empty()).await;
    assert_eq!(answered(&r.core), ["approve"]);
    assert_eq!(r.said().await, "Approved.");
}

/// "stop it" while a question is pending cancels the task that asked.
#[tokio::test]
async fn stop_it_cancels_the_task_that_asked() {
    let mut r = rig(vec![]).await;
    let owner = Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    };
    let started = r
        .core
        .task_start(
            &TaskStartParams {
                brief: "Count the boats".into(),
                ..Default::default()
            },
            owner,
        )
        .unwrap();
    let exec = started.session_id.replacen("ses_", "exe_", 1);
    r.publish(vec![question(
        "act_q1",
        &exec,
        "approval",
        "x",
        Some("proc.run"),
        &[],
    )]);
    let _ = r.said().await;
    r.say("stop it");
    assert!(!r.place.inflight);
    assert_eq!(r.said().await, "Stopped.");
    assert!(
        r.all_said().await.is_empty(),
        "the cancelled task's end is not told again"
    );
    let state = r
        .core
        .kernel
        .tasks(None)
        .unwrap()
        .into_iter()
        .find(|e| e.id == exec)
        .map(|e| format!("{:?}", e.state));
    assert_eq!(state.as_deref(), Some("Cancelled"));
}

/// Words that are no answer are an ordinary voice turn, and the question
/// stays pending; so do the words of someone who is not an owner.
#[tokio::test]
async fn anything_else_is_an_ordinary_voice_turn() {
    let mut r = rig(vec![Scripted::text("It is sunny.")]).await;
    r.publish(vec![question(
        "act_q1",
        "exe_t1",
        "approval",
        "x",
        Some("proc.run"),
        &[],
    )]);
    let _ = r.said().await;
    r.say("what's the weather");
    assert!(r.place.inflight, "a voice turn");
    assert!(
        r.tasks.lock().unwrap().pending.is_some(),
        "still waiting for its answer"
    );
    // The framing is there: it went the way every voice turn goes.
    let _ = FRAMING;
    // Not an owner: the same words are no answer.
    let mut r = rig(vec![Scripted::text("Sure.")]).await;
    r.publish(vec![question(
        "act_q1",
        "exe_t1",
        "approval",
        "x",
        Some("proc.run"),
        &[],
    )]);
    let _ = r.said().await;
    r.core
        .bind_places(bound(true).into_iter().skip(1).collect());
    r.say("approve");
    assert!(r.place.inflight, "a stranger's approve is a voice turn");
}

/// The answer goes stale: five minutes on, "approve" is an ordinary turn.
#[test]
fn a_question_can_be_answered_by_voice_for_five_minutes() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut t = Tasks::new(tx, LOUNGE, 0);
    let pend = || Pending {
        question: "q".into(),
        task: "exe_t".into(),
        options: options(),
        at: Instant::now(),
    };
    t.pending = Some(pend());
    let now = Instant::now();
    assert!(t.take("approve", false, now).is_none(), "an owner's alone");
    assert!(t
        .take("approve", true, now + Duration::from_secs(301))
        .is_none());
    assert!(t.take("approve", true, now).is_none(), "gone once stale");
    t.pending = Some(pend());
    assert!(t.take("approve", true, now).is_some());
    assert!(t.take("approve", true, now).is_none(), "taken once");
}

/// A core's refusal is spoken in few words, with the core's own in the text.
#[tokio::test]
async fn a_refused_answer_is_spoken_briefly_and_the_core_says_why() {
    let mut r = rig(owner_asks()).await;
    let corr = asked(&r.core).await;
    r.publish(vec![question(
        &corr,
        "exe_t1",
        "ask",
        "Which plan?",
        None,
        &["A", "B"],
    )]);
    let _ = r.said().await;
    // The channel stops being private after the question was asked.
    r.core.bind_places(bound(false));
    r.say("first");
    assert_eq!(r.said().await, "I can't take that here.");
    assert_eq!(
        r.core.confirm_list().unwrap().len(),
        1,
        "the question waits"
    );
    let posts = super::tests::posts(&r.core, &format!("discord:channel:{LOUNGE}"));
    assert!(
        posts
            .iter()
            .any(|p| p.starts_with("🔐 Your answer did not count")),
        "{posts:?}"
    );
}

/// A shared voice place hears nothing of the owner's tasks, nor does the
/// call about a task of a shared place.
#[tokio::test]
async fn nothing_is_said_in_a_shared_voice_place() {
    let mut r = rig(vec![]).await;
    r.core.bind_places(bound(false));
    r.publish(vec![
        task(
            "exe_t1",
            "Harbour survey",
            "done",
            Some("done"),
            "Done.",
            None,
        ),
        question("act_q1", "exe_t1", "approval", "x", Some("proc.run"), &[]),
    ]);
    // The sentinel is told in no shared place either: nothing arrives at all.
    r.publish(vec![task(
        "exe_t9",
        "Last",
        "done",
        Some("done"),
        "Done.",
        None,
    )]);
    let heard = tokio::time::timeout(Duration::from_millis(300), r.spoken.recv()).await;
    assert!(heard.is_err(), "{heard:?}");
    assert!(r.tasks.lock().unwrap().pending.is_none());
}

/// A task that reports to the voice place's own session is not told here:
/// its report wakes that session, and that turn's reply is spoken.
#[tokio::test]
async fn a_report_to_the_voice_places_own_session_is_left_to_its_turn() {
    let mut r = rig(vec![]).await;
    let owner = Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    };
    let here = r
        .core
        .task_start(
            &TaskStartParams {
                brief: "Count the boats".into(),
                home: Some(r.sid.clone()),
                ..Default::default()
            },
            owner.clone(),
        )
        .unwrap();
    let elsewhere = r
        .core
        .task_start(
            &TaskStartParams {
                brief: "Count the gulls".into(),
                ..Default::default()
            },
            owner,
        )
        .unwrap();
    r.publish(vec![
        task(
            "exe_a",
            "Boats",
            "done",
            Some("done"),
            "Counted 14.",
            Some(&here.session_id),
        ),
        task(
            "exe_b",
            "Gulls",
            "done",
            Some("done"),
            "Counted 9.",
            Some(&elsewhere.session_id),
        ),
    ]);
    assert_eq!(r.said().await, "Gulls is done: Counted 9");
    assert!(r.all_said().await.is_empty());
}

/// After the call ends its watch is gone: nothing more is said.
#[tokio::test]
async fn nothing_is_said_once_the_call_has_ended() {
    let mut r = rig(vec![]).await;
    let handle = r
        .place
        .shared
        .voice
        .call
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .status
        .take()
        .unwrap();
    handle.finish().await;
    assert_eq!(
        r.core.bus.work_watchers(),
        0,
        "the watch ended with the call"
    );
    r.publish(vec![task(
        "exe_t1",
        "Harbour survey",
        "done",
        Some("done"),
        "Done.",
        None,
    )]);
    let heard = tokio::time::timeout(Duration::from_millis(300), r.spoken.recv()).await;
    assert!(heard.is_err(), "{heard:?}");
}

/// The ledger rows of `kind` that name `session`.
fn rows_of(core: &Core, kind: &str, session: &str) -> usize {
    core.store
        .ledger_tail::<theseus_core::ledger::LedgerRow>(4000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.kind == kind && r.data["session_id"] == session)
        .count()
}

/// A task started for real, and the execution id its view carries.
fn started_task(core: &Arc<Core>, brief: &str) -> (String, String) {
    let owner = Answerer {
        label: "the CLI".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    };
    let s = core
        .task_start(
            &TaskStartParams {
                brief: brief.into(),
                ..Default::default()
            },
            owner,
        )
        .unwrap();
    let exec = s.session_id.replacen("ses_", "exe_", 1);
    (s.session_id, exec)
}

/// "pause it" after a question pauses the task that asked, with no voice turn,
/// and the question stays pending; the RPC's word comes back once
/// (theseus-ypnxt.5).
#[tokio::test]
async fn pause_it_after_a_question_pauses_that_task() {
    let mut r = rig(vec![]).await;
    let (session, exec) = started_task(&r.core, "Count the boats");
    let (_other, _) = started_task(&r.core, "Count the gulls");
    r.publish(vec![question(
        "act_q1",
        &exec,
        "approval",
        "x",
        Some("proc.run"),
        &[],
    )]);
    let _ = r.said().await;
    r.say("pause it");
    assert!(!r.place.inflight, "no turn was made of it");
    assert_eq!(r.said().await, "Paused.");
    assert_eq!(rows_of(&r.core, "task.paused", &session), 1);
    assert!(
        r.tasks.lock().unwrap().pending.is_some(),
        "the question still waits for its answer"
    );
}

/// With no question pending, the place's only running task is the one; "go on"
/// lets the only paused one go.
#[tokio::test]
async fn pause_it_and_go_on_take_the_places_only_running_or_paused_task() {
    let mut r = rig(vec![]).await;
    let (session, _exec) = started_task(&r.core, "Count the boats");
    // The core's own board tells the watch of the task it started.
    let tasks = Arc::clone(&r.tasks);
    until("the task is open on the watch", move || {
        tasks.lock().unwrap().open_counts() == (1, 0)
    })
    .await;
    r.say("Pause it.");
    assert!(!r.place.inflight);
    assert_eq!(r.said().await, "Paused.");
    assert_eq!(rows_of(&r.core, "task.paused", &session), 1);
    // The board shows it held; "go on" lets it go.
    let tasks = Arc::clone(&r.tasks);
    until("the board shows it held", move || {
        tasks.lock().unwrap().open_counts() == (0, 1)
    })
    .await;
    r.say("go on");
    assert!(!r.place.inflight);
    assert_eq!(r.said().await, "Going on.");
    assert_eq!(rows_of(&r.core, "task.resumed", &session), 1);
}

/// Two running and none pending: there is no one task, so it is an ordinary
/// voice turn.
#[tokio::test]
async fn pause_it_with_two_running_and_none_pending_is_an_ordinary_turn() {
    let mut r = rig(vec![Scripted::text("Which one?")]).await;
    let (a, _) = started_task(&r.core, "Count the boats");
    let (b, _) = started_task(&r.core, "Count the gulls");
    let mut va = task("exe_a", "Boats", "running", None, "", Some(&a));
    let mut vb = task("exe_b", "Gulls", "running", None, "", Some(&b));
    va.ended_at_ms = None;
    vb.ended_at_ms = None;
    r.publish(vec![va, vb]);
    let _ = r.all_said().await;
    r.say("pause it");
    assert!(r.place.inflight, "a voice turn");
    assert_eq!(rows_of(&r.core, "task.paused", &a), 0);
    assert_eq!(rows_of(&r.core, "task.paused", &b), 0);
}

/// A guest's words hold nothing: only the owner pauses a task by voice.
#[tokio::test]
async fn a_guests_pause_it_does_nothing() {
    let mut r = rig(vec![Scripted::text("Sure.")]).await;
    let (session, exec) = started_task(&r.core, "Count the boats");
    r.publish(vec![question(
        "act_q1",
        &exec,
        "approval",
        "x",
        Some("proc.run"),
        &[],
    )]);
    let _ = r.said().await;
    r.core
        .bind_places(bound(true).into_iter().skip(1).collect());
    r.say("pause it");
    assert!(r.place.inflight, "a stranger's pause is a voice turn");
    assert_eq!(rows_of(&r.core, "task.paused", &session), 0);
}

/// The table of words, and the rule for the task.
#[test]
fn only_whole_utterances_pause_or_resume_and_the_pending_question_names_the_task() {
    for s in ["pause it", "Pause it.", "pause that", "pause the task"] {
        assert_eq!(super::tasks::held_words(s), Some(true), "{s}");
    }
    for s in ["go on", "Go on!", "resume it", "resume the task"] {
        assert_eq!(super::tasks::held_words(s), Some(false), "{s}");
    }
    for s in [
        "pause it and tell me why",
        "go on then",
        "pause",
        "please go on with the story",
        "",
    ] {
        assert_eq!(super::tasks::held_words(s), None, "{s}");
    }
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut t = Tasks::new(tx, LOUNGE, 0);
    let now = Instant::now();
    assert!(t.hold("pause it", true, now).is_none(), "no task at all");
    t.pending = Some(Pending {
        question: "q".into(),
        task: "exe_asked".into(),
        options: options(),
        at: now,
    });
    assert_eq!(
        t.hold("pause it", true, now),
        Some((true, "exe_asked".into()))
    );
    assert_eq!(
        t.hold("go on", true, now),
        Some((false, "exe_asked".into()))
    );
    assert!(t.hold("pause it", false, now).is_none(), "an owner's alone");
    assert!(
        t.hold("pause it", true, now + Duration::from_secs(301))
            .is_none(),
        "stale, and no other task"
    );
}
