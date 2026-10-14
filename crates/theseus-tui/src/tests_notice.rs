//! The TUI's notices by the one policy (theseus-753z), on the harbour rig: a
//! task's question rings once, with its words; a spend change rings nothing;
//! a wake's turn on a root rings once; and a burst of questions rings once,
//! reading `3 questions need you`.

use theseus_protocol::{ExecutionView, PendingConfirm, SessionKind, WaitingOn};

use crate::notice::Delivery;
use crate::tests::{conv, harbour_rig, task, view, Captured, Rig, NOW, T0};

/// The harbour rig, its terminal's sequences captured, delivering by OSC 9
/// so each ping's words are in the output.
async fn rig() -> (Rig, Captured) {
    let mut rig = harbour_rig(80, 24);
    let out = Captured::default();
    rig.runner.out = Box::new(out.clone());
    rig.runner.delivery = Delivery::Osc9;
    rig.shows("check the tide tables").await;
    (rig, out)
}

/// The pings written so far: each OSC 9's text.
fn pings(out: &Captured) -> Vec<String> {
    out.text()
        .split("\x1b]9;theseus: ")
        .skip(1)
        .map(|s| s.split('\x07').next().unwrap_or_default().to_string())
        .collect()
}

fn push(rig: &Rig, v: &ExecutionView) {
    rig.daemon()
        .notify("execution.changed", serde_json::to_value(v).unwrap());
}

/// Let the clock reach `ms` after T0, and the loop take what is due.
async fn at(rig: &mut Rig, ms: u64) {
    NOW.with(|n| n.set(T0 + ms));
    rig.settle().await;
}

/// A task of the DM's asking `cor` its question, expiring at 14:23.
fn asking(position: u64, sid: &str, cor: &str) -> ExecutionView {
    view(
        position,
        sid,
        SessionKind::Task,
        Some("ses_dm0001"),
        "waiting",
        Some(WaitingOn::Confirm {
            confirm_id: cor.into(),
        }),
        vec![PendingConfirm {
            correlation_id: cor.into(),
            tool: "proc.run".into(),
            reason: "run the tide".into(),
            floor: false,
            budget: false,
            expires_at_ms: T0 + 600_000,
        }],
        2,
        0.10,
    )
}

#[tokio::test]
async fn a_tasks_question_rings_once_with_its_words() {
    let (mut rig, out) = rig().await;
    push(&rig, &asking(141, "ses_tide01", "cor_tide01"));
    rig.settle().await;
    assert!(pings(&out).is_empty(), "not before its second");
    at(&mut rig, 1_000).await;
    rig.until("the ping", |_| !pings(&out).is_empty()).await;
    assert_eq!(
        pings(&out),
        ["● check the tide tables asks: proc.run run the tide? · expires 14:23"]
    );
    assert!(
        rig.screen()[23].contains("check the tide tables asks"),
        "{:?}",
        rig.screen()
    );
    at(&mut rig, 5_000).await;
    assert_eq!(pings(&out).len(), 1, "once");
}

#[tokio::test]
async fn a_spend_change_rings_nothing() {
    let (mut rig, out) = rig().await;
    push(
        &rig,
        &task(141, "ses_tide01", "ses_dm0001", "running", 0.80),
    );
    push(
        &rig,
        &task(142, "ses_tide01", "ses_dm0001", "running", 1.60),
    );
    at(&mut rig, 3_000).await;
    assert!(pings(&out).is_empty(), "{:?}", out.text());
    let bells = out.text().matches('\x07').count() - out.text().matches("\x1b]0;").count();
    assert_eq!(bells, 0, "{:?}", out.text());
}

#[tokio::test]
async fn a_wakes_turn_on_a_root_rings_once() {
    let (mut rig, out) = rig().await;
    let mut queued = conv(141, "ses_dm0001", "queued", None, 0.42);
    queued.why = Some("wake".into());
    queued.attention = theseus_protocol::attention(&queued, &theseus_protocol::utc_hm);
    push(&rig, &queued);
    push(&rig, &conv(142, "ses_dm0001", "running", None, 0.42));
    push(
        &rig,
        &conv(143, "ses_dm0001", "waiting", Some(WaitingOn::Input), 0.43),
    );
    rig.settle().await;
    at(&mut rig, 1_000).await;
    rig.until("the ping", |_| !pings(&out).is_empty()).await;
    assert_eq!(pings(&out), ["⏰ DM · its wake fired"]);
    at(&mut rig, 5_000).await;
    assert_eq!(pings(&out).len(), 1, "once: the turn's end only informs");
}

#[tokio::test]
async fn a_burst_of_questions_rings_once() {
    let (mut rig, out) = rig().await;
    for (i, sid) in ["ses_burst1", "ses_burst2", "ses_burst3"]
        .iter()
        .enumerate()
    {
        push(&rig, &asking(141 + i as u64, sid, &format!("cor_b{i}")));
    }
    rig.settle().await;
    at(&mut rig, 1_000).await;
    rig.until("the ping", |_| !pings(&out).is_empty()).await;
    assert_eq!(pings(&out), ["3 questions need you"]);
    // A fourth inside the window: the footer, no ping; one after it: its own.
    push(&rig, &asking(150, "ses_burst4", "cor_b4"));
    rig.settle().await;
    at(&mut rig, 3_000).await;
    assert_eq!(pings(&out).len(), 1, "{:?}", pings(&out));
    assert!(
        rig.screen()[23].contains("4 questions need you"),
        "{:?}",
        rig.screen()
    );
    push(&rig, &asking(160, "ses_burst5", "cor_b5"));
    rig.settle().await;
    at(&mut rig, 12_000).await;
    rig.until("the second ping", |_| pings(&out).len() == 2)
        .await;
    assert!(
        pings(&out)[1].contains("asks: proc.run run the tide"),
        "{:?}",
        pings(&out)
    );
}

/// A snapshot is the board as it stands, not a transition seen as it
/// happened: a question asked while the TUI was behind (`events.lost`, the
/// board read again) rings nothing, since a snapshot's views go through
/// `Notices::saw`.
#[tokio::test]
async fn a_question_read_from_a_snapshot_rings_nothing() {
    let world = crate::tests::harbour_world();
    let mut rig = Rig::new(80, 24, crate::tests::script(world.clone()));
    let out = Captured::default();
    rig.runner.out = Box::new(out.clone());
    rig.runner.delivery = Delivery::Osc9;
    rig.shows("check the tide tables").await;
    {
        let mut w = world.lock().unwrap();
        crate::tests::replace_view(&mut w.board, &asking(141, "ses_tide01", "cor_tide01"));
        w.board["position"] = serde_json::json!(141);
    }
    rig.daemon().notify(
        "events.lost",
        serde_json::json!({"dropped": 3, "streams": ["executions"]}),
    );
    rig.asked("executions.watch", 2).await;
    at(&mut rig, 2_000).await;
    rig.settle().await;
    assert!(pings(&out).is_empty(), "{:?}", pings(&out));
}

/// A toast's command line, recorded and never run (theseus-f0bi9).
#[derive(Default)]
struct Argvs(std::sync::Mutex<Vec<Vec<String>>>);

impl theseus_client::notify::Spawner for Argvs {
    fn run(&self, argv: &[String]) -> std::io::Result<()> {
        self.0.lock().unwrap().push(argv.to_vec());
        Ok(())
    }
}

/// What a toast word says (`x` and the text's UTF-8 as hex).
fn unword(w: &str) -> String {
    let hex = &w[1..];
    let bytes: Vec<u8> = (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
        .collect();
    String::from_utf8(bytes).unwrap()
}

#[tokio::test]
async fn under_toast_a_question_is_a_toast_titled_by_the_task_and_the_terminal_hears_nothing() {
    let (mut rig, out) = rig().await;
    let spawner = std::sync::Arc::new(Argvs::default());
    rig.runner.delivery = Delivery::Toast;
    rig.runner.notifier = theseus_client::notify::Notifier::with(
        Box::new(theseus_client::notify::Env::default),
        spawner.clone(),
        std::sync::Arc::new(|_| {}),
        std::sync::Arc::new(|_| {}),
    );
    push(&rig, &asking(141, "ses_tide01", "cor_tide01"));
    rig.settle().await;
    at(&mut rig, 1_000).await;
    rig.until("the toast", |_| !spawner.0.lock().unwrap().is_empty())
        .await;
    let argv = spawner.0.lock().unwrap()[0].clone();
    assert_eq!(argv[0], "powershell.exe");
    assert_eq!(argv[4], theseus_client::notify::SCRIPT);
    assert_eq!(unword(&argv[5]), "check the tide tables");
    assert_eq!(
        unword(&argv[6]),
        "● check the tide tables asks: proc.run run the tide? · expires 14:23"
    );
    assert_eq!(unword(&argv[7]), "sound", "a question is an Interrupt");
    assert!(pings(&out).is_empty(), "no OSC 9 beside the toast");
    // The title's OSC ends in a BEL too: take the sequences out, and no bell is left.
    let mut text = out.text();
    while let Some(a) = text.find("\x1b]") {
        let end = text[a..].find('\x07').map_or(text.len(), |b| a + b + 1);
        text.replace_range(a..end, "");
    }
    assert!(!text.contains('\x07'), "nor a bell");
}

#[tokio::test]
async fn a_burst_under_toast_is_one_toast_with_the_folded_words() {
    let (mut rig, _out) = rig().await;
    let spawner = std::sync::Arc::new(Argvs::default());
    rig.runner.delivery = Delivery::Toast;
    rig.runner.notifier = theseus_client::notify::Notifier::with(
        Box::new(theseus_client::notify::Env::default),
        spawner.clone(),
        std::sync::Arc::new(|_| {}),
        std::sync::Arc::new(|_| {}),
    );
    for (i, sid) in ["ses_burst1", "ses_burst2", "ses_burst3"]
        .iter()
        .enumerate()
    {
        push(&rig, &asking(141 + i as u64, sid, &format!("cor_b{i}")));
    }
    rig.settle().await;
    at(&mut rig, 1_000).await;
    rig.until("the toast", |_| !spawner.0.lock().unwrap().is_empty())
        .await;
    at(&mut rig, 1_500).await;
    let runs = spawner.0.lock().unwrap();
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert_eq!(unword(&runs[0][6]), "3 questions need you");
}
