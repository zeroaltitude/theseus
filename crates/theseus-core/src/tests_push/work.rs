//! The work board over real connections (theseus-hkx30): what a client that
//! lost events does, who counts as a screen a person is looking at, and what a
//! connection that reads no private text may see.

use theseus_protocol::{
    work::{WorkKind, WorkView},
    WorkWatchResult,
};

use super::*;
use crate::approval::{Client, Surface};

/// A connection that reads only when told to, as `surface`.
fn connect(core: &Arc<Core>, name: &str, surface: Surface, buffer: usize) -> Raw {
    let (client, server) = tokio::io::duplex(buffer);
    let (sr, sw) = tokio::io::split(server);
    tokio::spawn(
        core.clone()
            .serve_connection(sr, sw, Client::new(name, surface)),
    );
    let (cr, w) = tokio::io::split(client);
    Raw {
        lines: BufReader::new(cr).lines(),
        w,
        notes: Vec::new(),
    }
}

fn snapshot(r: Response) -> WorkWatchResult {
    serde_json::from_value(r.result.expect("a result")).unwrap()
}

/// What a snapshot says of each view, by id.
fn key(s: &WorkWatchResult) -> BTreeMap<String, (u64, String, WorkKind)> {
    s.views
        .iter()
        .map(|v| {
            (
                v.id.clone(),
                (v.position, v.attention.label.clone(), v.kind),
            )
        })
        .collect()
}

/// A client that stops reading hears what it lost, and its re-snapshot equals
/// a fresh client's: the board's frames are streamed as `work`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_client_that_loses_work_events_catches_up_by_a_snapshot() {
    const CAP: usize = 64;
    const SESSIONS: usize = 120;
    let r = rig_full(vec![], false, true);
    r.core
        .push
        .backlog_cap
        .store(CAP, std::sync::atomic::Ordering::Relaxed);
    let mut slow = connect(&r.core, "slow", Surface::Cli, 16 * 1024);
    let first = snapshot(
        slow.call(1, method::WORK_WATCH, json!({"limit": 10_000}))
            .await,
    );
    assert_eq!(first.total, 0);
    // It stops reading. Each conversation's turn is several frames.
    for _ in 0..SESSIONS {
        turn(&r.core, None).await;
    }
    let last = last_kernel_position(&r.core);
    r.core
        .push
        .feed()
        .wait_for(|&p| p >= last)
        .await
        .expect("the board's feed");
    assert!(r.core.push.status(0).lost > 0, "the cap dropped some");
    let lost = loop {
        let line = tokio::time::timeout(Duration::from_secs(30), slow.lines.next_line())
            .await
            .expect("a line in 30 s")
            .unwrap()
            .unwrap();
        let Message::Notification(n) = serde_json::from_str::<Message>(&line).unwrap() else {
            continue;
        };
        match n.method.as_str() {
            notify::WORK_CHANGED | notify::WORK_NOTICE => {}
            notify::EVENTS_LOST => break n.params,
            other => panic!("unexpected {other}"),
        }
    };
    assert_eq!(lost["streams"], json!(["work"]));
    assert!(lost["dropped"].as_u64().unwrap() > 0);
    let again = snapshot(
        slow.call(2, method::WORK_WATCH, json!({"limit": 10_000}))
            .await,
    );
    let mut fresh = connect(&r.core, "fresh", Surface::Cli, 1 << 20);
    let fresh = snapshot(
        fresh
            .call(1, method::WORK_WATCH, json!({"limit": 10_000}))
            .await,
    );
    assert_eq!(again.views.len(), SESSIONS);
    assert_eq!(key(&again), key(&fresh));
    assert_eq!(again.position, fresh.position);
}

/// `work.watch` with `interactive` is counted once, apart from the rest, and
/// uncounted when the connection goes or watches again as a reader; health's
/// push line says both.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interactive_watchers_count_and_uncount() {
    let r = rig();
    let mut screen = connect(&r.core, "screen", Surface::Cli, 1 << 16);
    let mut digest = connect(&r.core, "digest", Surface::Cli, 1 << 16);
    snapshot(
        screen
            .call(1, method::WORK_WATCH, json!({"interactive": true}))
            .await,
    );
    snapshot(digest.call(1, method::WORK_WATCH, json!({})).await);
    assert_eq!(
        (
            r.core.bus.work_watchers(),
            r.core.bus.interactive_watchers()
        ),
        (2, 1)
    );
    let push = r.core.health().push.unwrap();
    assert_eq!(
        (push.work_watchers, push.interactive),
        (2, 1),
        "health says both"
    );
    // The same connection watches again, no longer a screen.
    snapshot(screen.call(2, method::WORK_WATCH, json!({})).await);
    assert_eq!(r.core.bus.interactive_watchers(), 0);
    snapshot(
        digest
            .call(2, method::WORK_WATCH, json!({"interactive": true}))
            .await,
    );
    assert_eq!(r.core.bus.interactive_watchers(), 1);
    // It goes away.
    drop(digest);
    for _ in 0..400 {
        if r.core.bus.work_watchers() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        (
            r.core.bus.work_watchers(),
            r.core.bus.interactive_watchers()
        ),
        (1, 0),
        "a closed connection is no screen"
    );
    let off = screen.call(3, method::WORK_UNWATCH, json!({})).await;
    assert_eq!(off.result.unwrap()["watching"], false);
    assert_eq!(r.core.bus.work_watchers(), 0);
}

/// What a connection that reads no private text hears: it must name its
/// conversations, a private one named is left out without a word, the title it
/// gets passes the scrubber, and it hears nothing of the others as they
/// change. The owner's own surface sees the whole board.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_shared_places_watch_sees_no_private_title() {
    use crate::places::BoundPlace;
    use crate::tests_places::{session, turn as say, LAB};
    let r = rig();
    r.core.bind_places(vec![BoundPlace {
        target: format!("discord:channel:{LAB}"),
        name: "#lab".into(),
        private: false,
        ..Default::default()
    }]);
    let shared = session(&r.core, Some(&format!("channel:{LAB}")));
    let private = session(&r.core, None);
    // A token, built from parts so this file holds none whole.
    let token = ["ghp_", "a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8"].concat();
    say(&r.core, &shared, &format!("deploy with {token} today")).await;
    say(
        &r.core,
        &private,
        "the owner's private plan for the quarter",
    )
    .await;

    // A binding says which conversations it asks for.
    let mut binding = Raw::connect(&r.core, "binding", 1 << 20);
    let refused = binding.call(1, method::WORK_WATCH, json!({})).await;
    assert!(
        refused.error.is_some(),
        "a surface that reads no private text names its roots"
    );
    let snap = snapshot(
        binding
            .call(2, method::WORK_WATCH, json!({"roots": [shared, private]}))
            .await,
    );
    assert_eq!(
        snap.views.len(),
        1,
        "the private root is left out: {:?}",
        snap.views
    );
    let seen = &snap.views[0];
    assert_eq!(seen.kind, WorkKind::Conversation);
    assert!(seen.title.starts_with("deploy with "), "{}", seen.title);
    assert!(
        !seen.title.contains(&token),
        "the scrubber ran: {}",
        seen.title
    );
    let every_word = serde_json::to_string(&snap.views).unwrap();
    assert!(!every_word.contains("private plan"), "{every_word}");

    // The owner's surface sees both, and the token as typed.
    let mut cli = connect(&r.core, "cli", Surface::Cli, 1 << 20);
    let whole = snapshot(cli.call(1, method::WORK_WATCH, json!({})).await);
    assert_eq!(whole.views.len(), 2);
    assert!(whole.views.iter().any(|v| v.title.contains("private plan")));

    // The private conversation changes: the binding hears nothing of it.
    say(&r.core, &private, "another private thing").await;
    say(&r.core, &shared, "and a public one").await;
    let mut heard: Vec<String> = Vec::new();
    while let Ok(Ok(Some(l))) =
        tokio::time::timeout(Duration::from_millis(600), binding.lines.next_line()).await
    {
        heard.push(l);
    }
    let heard = heard.join("\n");
    assert!(
        heard.contains(notify::WORK_CHANGED),
        "it hears its own root change"
    );
    assert!(!heard.contains("private"), "{heard}");
    assert!(!heard.contains(&token), "{heard}");
}

/// A guarded scope cuts a question's power to answer and scrubs its words;
/// the whole scope hands a view over as it is.
#[test]
fn a_guarded_scope_scrubs_and_cannot_answer() {
    use crate::push::WorkScope;
    use theseus_protocol::work::{AnswerOption, Question, QuestionKind, Style, WorkRef, WorkState};
    let token = ["ghp_", "a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8"].concat();
    let scrubber = crate::scrub::Scrubber::default();
    let who = WorkRef {
        id: "exe_1".into(),
        kind: WorkKind::Task,
        session_id: None,
        title: String::new(),
    };
    let mut view: WorkView = serde_json::from_value(json!({
        "id": "act_1", "kind": "question", "position": 5, "at_ms": 1, "root": "exe_1",
        "title": format!("curl -H {token}"), "state": "needs_you",
        "attention": {"level": "needs_you", "label": "confirm", "since_ms": 1},
        "started_at_ms": 1, "state_since_ms": 1
    }))
    .unwrap();
    assert_eq!(view.state, WorkState::NeedsYou);
    view.question = Some(Question {
        id: "act_1".into(),
        kind: QuestionKind::Approval,
        asked_by: who,
        prompt: format!("proc.run curl -H {token}"),
        detail: None,
        input: None,
        options: vec![AnswerOption {
            id: "approve".into(),
            label: "Approve".into(),
            style: Style::Primary,
            takes_note: false,
            effect: String::new(),
        }],
        on_expiry: None,
        expires_at_ms: None,
        floor: false,
        may_answer: true,
        tool: Some("proc.run".into()),
        asked_at_ms: 1,
        choices: vec![],
    });
    let guarded = WorkScope {
        roots: None,
        guarded: true,
    };
    let shown = guarded.show(&view, &scrubber);
    let words = serde_json::to_string(&shown).unwrap();
    assert!(!words.contains(&token), "{words}");
    assert!(!shown.question.as_ref().unwrap().may_answer);
    let whole = WorkScope::default().show(&view, &scrubber);
    assert!(whole.question.as_ref().unwrap().may_answer);
    assert!(serde_json::to_string(&whole).unwrap().contains(&token));
}

/// A fixture history imported in four numbered batches (theseus-jn17m): a
/// watcher hears the daemon's own job, `batch 1 of 4` to `batch 3 of 4`,
/// then its end, on `work.changed`, under the one root, `Theseus`. A fresh
/// watcher's snapshot has it for the owner's surface, a binding's none of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_import_shows_its_progress_on_work_changed_and_ends() {
    use crate::import::tests::{fixture, lines};
    use theseus_protocol::WorkChanged;
    let r = rig();
    let mut cli = connect(&r.core, "cli", Surface::Cli, 1 << 20);
    snapshot(cli.call(1, method::WORK_WATCH, json!({})).await);
    let all = lines(&fixture());
    for (i, chunk) in all.chunks(3).enumerate() {
        let p = json!({"file": "reef.jsonl", "lines": chunk, "batch": i + 1, "batches": 4});
        let got = cli.call(10 + i as u64, method::IMPORT_EPISODES, p).await;
        assert!(got.error.is_none(), "{:?}", got.error);
    }
    let mut labels: Vec<String> = Vec::new();
    let mut root_seen = false;
    let mut take = |n: &theseus_protocol::Notification, labels: &mut Vec<String>| {
        if n.method != notify::WORK_CHANGED {
            return;
        }
        let c: WorkChanged = serde_json::from_value(n.params.clone()).unwrap();
        for v in c.views {
            if v.kind == WorkKind::System && v.parent.is_some() {
                labels.push(v.attention.label.clone());
            }
            root_seen |= v.id == "system";
        }
    };
    for n in std::mem::take(&mut cli.notes) {
        take(&n, &mut labels);
    }
    while labels.last().map(String::as_str) != Some("done · 12 sessions") {
        let line = tokio::time::timeout(Duration::from_secs(30), cli.lines.next_line())
            .await
            .expect("the job's end in 30 s")
            .unwrap()
            .expect("open");
        if let Ok(Message::Notification(n)) = serde_json::from_str::<Message>(&line) {
            take(&n, &mut labels);
        }
    }
    assert_eq!(
        labels,
        [
            "batch 1 of 4",
            "batch 2 of 4",
            "batch 3 of 4",
            "done · 12 sessions"
        ]
    );
    assert!(root_seen, "the root arrived with its job");

    // A new watcher on the owner's surface has the root and its job; a
    // binding that names a conversation of a shared place has neither.
    let mut fresh = connect(&r.core, "fresh", Surface::Cli, 1 << 20);
    let owner = snapshot(fresh.call(1, method::WORK_WATCH, json!({})).await);
    assert!(owner.views.iter().any(|v| v.id == "system"));
    assert!(owner
        .views
        .iter()
        .any(|v| v.kind == WorkKind::System && v.state == theseus_protocol::work::WorkState::Done));
    let mut binding = Raw::connect(&r.core, "binding", 1 << 20);
    let theirs = snapshot(
        binding
            .call(1, method::WORK_WATCH, json!({"roots": ["ses_nothing"]}))
            .await,
    );
    assert!(theirs.views.is_empty(), "{:?}", theirs.views);
}

/// The Discord binding is the one guarded surface that watches a conversation
/// of a private place, when the place is a Discord place it speaks in
/// (theseus-2j2ka): it shows a place its own session's tasks in that place
/// alone, as it shows its cards. A private session with no Discord place is
/// still left out, and so is the same room to any other surface. What it hears
/// is scrubbed and offers no answer.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_discord_binding_watches_the_private_places_it_speaks_in_and_no_other_private_session()
{
    use crate::places::BoundPlace;
    use crate::tests_places::{session, turn as say, LAB};
    let r = rig();
    r.core.bind_places(vec![BoundPlace {
        target: format!("discord:channel:{LAB}"),
        name: "#lab".into(),
        private: true,
        ..Default::default()
    }]);
    let lab = session(&r.core, Some(&format!("channel:{LAB}")));
    let cli = session(&r.core, None);
    say(&r.core, &lab, "the lab's private plan for the quarter").await;
    say(&r.core, &cli, "a command line session's words").await;

    let mut binding = connect(&r.core, "binding", Surface::Discord, 1 << 20);
    let snap = snapshot(
        binding
            .call(1, method::WORK_WATCH, json!({"roots": [lab, cli]}))
            .await,
    );
    assert_eq!(
        snap.views.len(),
        1,
        "only its own place's: {:?}",
        snap.views
    );
    assert!(
        snap.views[0].title.contains("private plan"),
        "{}",
        snap.views[0].title
    );
    assert!(
        snap.views
            .iter()
            .all(|v| v.question.as_ref().is_none_or(|q| !q.may_answer)),
        "it offers no answer"
    );

    // Any other surface that reads no private text hears nothing of the room.
    let mut mcp = connect(&r.core, "other", Surface::Unnamed, 1 << 20);
    let none = snapshot(
        mcp.call(1, method::WORK_WATCH, json!({"roots": [lab]}))
            .await,
    );
    assert!(none.views.is_empty(), "{:?}", none.views);
}
