//! The voice channel's status and the board's line (theseus-arp9l): the
//! status loop against a recording sink on tokio's paused clock (the pace:
//! 2 s, one a minute, the last state, a clear at the end, a 403 told once),
//! against the fake Discord over its REST path, and the spoken line in a
//! private place's voice turn and never in a shared one.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use theseus_core::provider::Scripted;
use theseus_core::push::WorkOut;
use theseus_core::Core;
use theseus_protocol::work::WorkView;
use theseus_voice::Utterance;
use tokio::time::Instant;

use super::super::tests::core_scripted;
use super::super::voice_status::tests::view;
use super::super::voice_status::{Handle, Put, PutError, PutFuture, Rest, Tally};
use super::tests::{heard, LOUNGE};
use super::tests_heard::lounge_scripted;

const BETA: &str = "fs.write create projects/beta-summary.txt (40 B)";

/// A sink that keeps every write, and can answer 403.
#[derive(Default)]
struct Record {
    puts: Mutex<Vec<(Instant, String)>>,
    attempts: AtomicUsize,
    forbid: std::sync::atomic::AtomicBool,
}

impl Record {
    fn texts(&self) -> Vec<String> {
        self.puts
            .lock()
            .unwrap()
            .iter()
            .map(|(_, t)| t.clone())
            .collect()
    }
}

impl Put for Record {
    fn put<'a>(&'a self, _channel: u64, status: &'a str) -> PutFuture<'a> {
        Box::pin(async move {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            if self.forbid.load(Ordering::SeqCst) {
                return Err(PutError::Forbidden);
            }
            self.puts
                .lock()
                .unwrap()
                .push((Instant::now(), status.to_string()));
            Ok(())
        })
    }
}

/// Views as one committed frame, to every `work.watch` connection.
fn publish(core: &Core, views: Vec<WorkView>, gone: &[&str]) {
    let out = WorkOut {
        position: 1,
        at_ms: 1,
        views,
        gone: gone
            .iter()
            .map(|id| (id.to_string(), "exe_root".to_string()))
            .collect(),
        notices: Vec::new(),
        threads: Vec::new(),
    };
    core.bus.publish_work(&out, &core.tools.scrubber);
}

/// The status loop has begun its watch (and so its first want).
async fn watching(core: &Core) {
    while core.bus.work_watchers() == 0 {
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

/// Move the paused clock on `s` seconds a second at a time, letting the loop
/// run between: a blocking task (the push's applier) keeps tokio from
/// advancing a paused clock by itself.
async fn pass(s: u64) {
    // What was just published is taken in at this instant, before time moves.
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
    for _ in 0..s {
        tokio::time::advance(Duration::from_secs(1)).await;
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
    }
}

fn tasks(n: usize) -> Vec<WorkView> {
    (0..n)
        .map(|i| {
            view(
                &format!("exe_t{i}"),
                "task",
                "running",
                &format!("Task {i}"),
                None,
            )
        })
        .collect()
}

fn question(id: &str) -> WorkView {
    view(id, "question", "needs_you", "Beta summary", Some(BETA))
}

fn start(core: &Arc<Core>, put: &Arc<Record>, told: &Arc<AtomicU32>) -> Handle {
    let told = Arc::clone(told);
    Handle::start(Arc::clone(core), put.clone(), LOUNGE, 1, move |_| {
        told.fetch_add(1, Ordering::SeqCst);
    })
}

fn bare_core() -> (tempfile::TempDir, Arc<Core>) {
    let d = tempfile::tempdir().unwrap();
    let core = core_scripted(d.path(), vec![]);
    (d, core)
}

/// Joining sets the status; a task that needs you changes it within the
/// debounce once a minute has passed; two changes inside a minute write once,
/// the last state; leaving clears it.
#[tokio::test]
async fn the_status_follows_the_boards_counts_at_the_pace() {
    let (_d, core) = bare_core();
    let (put, told) = (Arc::new(Record::default()), Arc::new(AtomicU32::new(0)));
    let handle = start(&core, &put, &told);
    watching(&core).await;
    tokio::time::pause();
    let secs = pass;

    // Joined: the status is set, after the debounce, with nothing working.
    secs(1).await;
    assert!(put.texts().is_empty(), "not before the debounce");
    secs(2).await;
    assert_eq!(put.texts(), ["🎧 Theseus"]);

    // Two changes inside the minute: nothing at once, then one write of the
    // last state (the three tasks alone were never written).
    publish(&core, tasks(3), &[]);
    secs(10).await;
    publish(&core, vec![question("act_q1")], &[]);
    secs(30).await;
    assert_eq!(put.texts(), ["🎧 Theseus"], "the minute has not passed");
    secs(25).await;
    assert_eq!(
        put.texts(),
        ["🎧 Theseus", "🎧 Theseus · ⚙️ 3 tasks · 🔴 1 needs you"]
    );

    // A quiet minute later, a task needing you lands inside the debounce.
    secs(70).await;
    publish(&core, vec![question("act_q2")], &[]);
    secs(1).await;
    assert_eq!(put.texts().len(), 2, "not before the debounce");
    secs(2).await;
    assert_eq!(
        put.texts().last().unwrap(),
        "🎧 Theseus · ⚙️ 3 tasks · 🔴 2 needs you"
    );
    // The same counts again write nothing, however long.
    publish(&core, tasks(3), &[]);
    secs(200).await;
    assert_eq!(put.texts().len(), 3);

    // Leave: cleared.
    handle.finish().await;
    assert_eq!(put.texts().last().unwrap(), "");
    assert_eq!(put.texts().len(), 4);
    assert_eq!(told.load(Ordering::SeqCst), 0);
    assert_eq!(core.bus.work_watchers(), 0, "the watch ended with the call");
}

/// A 403 is told once and the call never writes again, a clear included.
#[tokio::test]
async fn a_403_is_told_once_and_not_retried() {
    let (_d, core) = bare_core();
    let (put, told) = (Arc::new(Record::default()), Arc::new(AtomicU32::new(0)));
    put.forbid.store(true, Ordering::SeqCst);
    let handle = start(&core, &put, &told);
    watching(&core).await;
    tokio::time::pause();
    pass(3).await;
    assert_eq!(told.load(Ordering::SeqCst), 1);
    assert_eq!(put.attempts.load(Ordering::SeqCst), 1);
    // More changes, a long time: no second try, and no second word.
    for i in 0..3 {
        publish(&core, tasks(i + 1), &[]);
        pass(120).await;
    }
    assert_eq!(put.attempts.load(Ordering::SeqCst), 1, "never retried");
    handle.finish().await;
    assert_eq!(
        put.attempts.load(Ordering::SeqCst),
        1,
        "no clear of what was never set"
    );
    assert_eq!(told.load(Ordering::SeqCst), 1, "told once");
}

/// The daemon's stop clears the status, with no `/leave`.
#[tokio::test]
async fn the_binding_stopping_clears_the_status() {
    let (_d, core) = bare_core();
    let (put, told) = (Arc::new(Record::default()), Arc::new(AtomicU32::new(0)));
    let handle = start(&core, &put, &told);
    watching(&core).await;
    tokio::time::pause();
    pass(3).await;
    assert_eq!(put.texts(), ["🎧 Theseus"]);
    core.outbox.stop_sending();
    pass(1).await;
    assert_eq!(put.texts().last().unwrap(), "", "cleared at the stop");
    drop(handle);
}

/// The REST path: a status the fake Discord receives as Discord would, the
/// counts of a board that changed inside the debounce as one write, the clear
/// at the leave, and a 403 told once.
#[tokio::test]
async fn the_status_reaches_discords_route_and_clears_at_the_leave() {
    let fake = theseus_sim::fake_discord::FakeDiscord::start();
    let (_d, core) = bare_core();
    let base = format!("http://{}/api/v10", fake.addr);
    let rest = Rest::new(base, "fake-token-not-a-secret".into()).unwrap();
    let told = Arc::new(AtomicU32::new(0));
    let t2 = Arc::clone(&told);
    let handle = Handle::start(Arc::clone(&core), Arc::new(rest), LOUNGE, 1, move |_| {
        t2.fetch_add(1, Ordering::SeqCst);
    });
    watching(&core).await;
    // Three tasks, one asking, inside the debounce: one write of all of it.
    publish(&core, tasks(3), &[]);
    publish(&core, vec![question("act_q1")], &[]);
    let wrote = async {
        while fake.voice_statuses().is_empty() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(8), wrote)
        .await
        .expect("a status write");
    let channel = LOUNGE.to_string();
    assert_eq!(
        fake.voice_statuses()
            .iter()
            .map(|(_, c, s)| (c.as_str(), s.as_str()))
            .collect::<Vec<_>>(),
        [(channel.as_str(), "🎧 Theseus · ⚙️ 3 tasks · 🔴 1 needs you")]
    );
    let seen = fake.seen();
    let put = seen.iter().find(|s| s.outcome == "voice_status").unwrap();
    assert_eq!(put.method, "PUT");
    assert_eq!(put.path, format!("/channels/{LOUNGE}/voice-status"));
    handle.finish().await;
    let all = fake.voice_statuses();
    assert_eq!(all.len(), 2);
    assert_eq!(all[1].2, "", "cleared at the leave");
    assert_eq!(told.load(Ordering::SeqCst), 0);

    // A bot without the permission: 403, said once, nothing recorded.
    fake.refuse_voice_status(true);
    let rest = Rest::new(format!("http://{}/api/v10", fake.addr), "t".into()).unwrap();
    let t3 = Arc::clone(&told);
    let handle = Handle::start(Arc::clone(&core), Arc::new(rest), LOUNGE, 2, move |why| {
        assert!(why.contains("Set Voice Channel Status"), "{why}");
        t3.fetch_add(1, Ordering::SeqCst);
    });
    let refused = async {
        while told.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(8), refused)
        .await
        .expect("told");
    handle.finish().await;
    assert_eq!(told.load(Ordering::SeqCst), 1);
    assert_eq!(
        fake.voice_statuses().len(),
        2,
        "the refused call set nothing"
    );
    let refusals = fake
        .seen()
        .iter()
        .filter(|s| s.outcome == "refused")
        .count();
    assert_eq!(refusals, 1, "one try, never retried");
}

fn utterance(text: &str) -> Utterance {
    heard(text)
}

/// The call's tally holds the open work: waits for the loop to apply what
/// was published.
async fn settled(tally: &Mutex<Tally>, tasks: u32, needs: u32) {
    for _ in 0..200 {
        let c = tally.lock().unwrap().counts();
        if (c.tasks, c.needs) == (tasks, needs) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the tally never reached {tasks} tasks, {needs} asking");
}

/// A lounge whose call keeps the status watch over a recording sink, with
/// three tasks and one question on the board.
async fn with_board(dir: &std::path::Path, private: bool) -> super::tests_heard::Lounge {
    let l = lounge_scripted(
        dir,
        vec![Scripted::text("Three tasks."), Scripted::text("Done.")],
    );
    let core = Arc::clone(&l.place.shared.core);
    if private {
        core.bind_places(vec![theseus_core::places::BoundPlace {
            target: l.place.target.clone(),
            name: "#lounge".into(),
            private: true,
            guild: Some("100000000000000001".into()),
            ..Default::default()
        }]);
    }
    let put = Arc::new(Record::default());
    let h = Handle::start(Arc::clone(&core), put, LOUNGE, 1, |_| {});
    watching(&core).await;
    let mut views = tasks(3);
    views.push(question("act_q1"));
    publish(&core, views, &[]);
    settled(&h.tally, 3, 1).await;
    l.place
        .shared
        .voice
        .call
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .status = Some(h);
    l
}

/// In a private place the voice turn carries one line from the board, so a
/// question about what is going on is answered with no tool call.
#[tokio::test]
async fn a_private_places_voice_turn_carries_the_boards_line() {
    let d = tempfile::tempdir().unwrap();
    let mut l = with_board(d.path(), true).await;
    let input = l.turn(1, vec![], vec![utterance("What's going on?")]).await;
    let line = format!("[The board: 3 tasks working; 1 needs you: Beta summary asks {BETA}]");
    assert!(input.contains(&line), "{input}");
    assert!(input.ends_with("🎙️ What's going on?"), "{input}");
    assert!(line.chars().count() - "[The board: ]".chars().count() <= 200);
}

/// In a shared place the voice turn carries nothing of the board: a private
/// title is never named there.
#[tokio::test]
async fn a_shared_places_voice_turn_never_names_a_private_title() {
    let d = tempfile::tempdir().unwrap();
    let mut l = with_board(d.path(), false).await;
    let input = l.turn(1, vec![], vec![utterance("What's going on?")]).await;
    assert!(!input.contains("Beta summary"), "{input}");
    assert!(!input.contains("beta-summary"), "{input}");
    assert!(!input.contains("The board"), "{input}");
    assert_eq!(input, format!("{}\n🎙️ What's going on?", super::FRAMING));
}
