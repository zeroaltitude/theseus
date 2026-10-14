//! The follow pane (theseus-e1hu4): `l` on a session row finds its running
//! job (`action.list`), peeks it (`work.peek`), follows from the peek's end
//! (`action.output`), shows the chunks the daemon sends and the job's end, and
//! `esc` lets it go (`action.unfollow`). A stream that fell behind is peeked
//! again; a session with no job running says so.

use crossterm::event::KeyCode;
use serde_json::{json, Value};

use crate::tests::{clock, harbour_world, script, Rig};

const JOB: &str = "act_k5";

/// The daemon's answers: the tide task has one job running, 12 bytes in.
fn world() -> std::sync::Arc<std::sync::Mutex<crate::tests::World>> {
    let w = harbour_world();
    let started = clock() - 5_400;
    {
        let mut g = w.lock().unwrap();
        g.answers.insert(
            "action.list".into(),
            Ok(json!({"actions": [{
                "correlation_id": JOB, "execution_id": "exe_ses_tide01",
                "session_id": "ses_tide01", "tool": "proc.run", "state": "dispatched",
                "retry_class": "never", "planned_at_ms": started, "dispatched_at_ms": started,
                "deadline_at_ms": started + 600_000, "confirmed": false,
                "completions_seen": 0
            }], "total": 1})),
        );
        g.answers.insert(
            "work.peek".into(),
            Ok(
                json!({"id": JOB, "bytes": 12, "lines": ["leg 1", "leg 2"], "at_ms": clock(),
                "complete": false, "started_at_ms": started, "usual_ms": 31_000}),
            ),
        );
        g.answers.insert(
            "action.output".into(),
            Ok(json!({"id": JOB, "offset": 12, "bytes": 12, "complete": false})),
        );
    }
    w
}

async fn on_the_tide_task(rig: &mut Rig) {
    rig.shows("turn 2  check the tide").await;
    rig.press(&[KeyCode::Char('j'), KeyCode::Char('j'), KeyCode::Char('j')])
        .await;
    assert_eq!(
        rig.runner.app.selected.as_deref(),
        Some("ses_tide01"),
        "{:?}",
        rig.screen()
    );
}

fn chunk(offset: u64, text: &str) -> Value {
    json!({"jsonrpc": "2.0", "method": "action.output.chunk",
        "params": {"id": JOB, "offset": offset, "text": text}})
}

/// `l` follows the running job: the calls come in order, the pane shows its
/// time against its usual and its lines, each chunk lands in order, the end
/// is said, and `esc` unfollows.
#[tokio::test]
async fn l_follows_a_sessions_running_job_and_esc_lets_it_go() {
    let mut rig = Rig::new(120, 30, script(world()));
    on_the_tide_task(&mut rig).await;
    rig.press(&[KeyCode::Char('l')]).await;
    rig.asked("action.output", 1).await;
    let d = rig.daemon();
    assert_eq!(
        d.asked("action.list"),
        [json!({"execution_id": "exe_ses_tide01", "unsettled": true})]
    );
    assert_eq!(d.asked("work.peek"), [json!({"id": JOB})]);
    assert_eq!(
        d.asked("action.output"),
        [json!({"id": JOB, "from": 12})],
        "it follows from the peek's end"
    );
    rig.shows("job 0:05 of usual 0:31").await;
    rig.shows("leg 2").await;
    // The chunks, in order, a line whole.
    d.push.send(chunk(12, "leg 3\nle")).unwrap();
    d.push.send(chunk(20, "g 4\n")).unwrap();
    rig.shows("leg 4").await;
    let lines: Vec<String> = rig
        .screen()
        .iter()
        .filter(|l| l.contains("leg "))
        .map(|l| l.trim().to_string())
        .map(|l| l[l.find("leg ").unwrap()..].to_string())
        .collect();
    assert!(
        lines
            .windows(4)
            .any(|w| w == ["leg 1", "leg 2", "leg 3", "leg 4"]),
        "{lines:?}"
    );
    d.push
        .send(json!({"jsonrpc": "2.0", "method": "action.output.end",
            "params": {"id": JOB, "outcome": "succeeded"}}))
        .unwrap();
    rig.shows("job succeeded").await;
    // esc closes the pane and tells the daemon.
    rig.press(&[KeyCode::Esc]).await;
    rig.asked("action.unfollow", 1).await;
    assert_eq!(d.asked("action.unfollow"), [json!({"id": JOB})]);
    assert!(rig.runner.app.follow.is_none());
    assert!(!rig.screen().iter().any(|l| l.contains("job succeeded")));
}

/// A stream that fell behind (`events.lost` naming the job) is read again: a
/// new peek, which replaces the lines.
#[tokio::test]
async fn a_stream_that_fell_behind_is_peeked_again() {
    let mut rig = Rig::new(120, 30, script(world()));
    on_the_tide_task(&mut rig).await;
    rig.press(&[KeyCode::Char('l')]).await;
    rig.asked("action.output", 1).await;
    let d = rig.daemon();
    d.push
        .send(json!({"jsonrpc": "2.0", "method": "events.lost",
            "params": {"dropped": 3, "streams": [format!("output:{JOB}")]}}))
        .unwrap();
    rig.asked("work.peek", 2).await;
    rig.asked("action.output", 2).await;
    rig.shows("fell behind; read again").await;
}

/// A session with no job running is told so, and no pane opens.
#[tokio::test]
async fn l_on_a_session_with_no_running_job_says_so() {
    let w = world();
    w.lock()
        .unwrap()
        .answers
        .insert("action.list".into(), Ok(json!({"actions": [], "total": 0})));
    let mut rig = Rig::new(120, 30, script(w));
    on_the_tide_task(&mut rig).await;
    rig.press(&[KeyCode::Char('l')]).await;
    rig.shows("no job is running in").await;
    assert!(rig.runner.app.follow.is_none());
    assert!(rig.daemon().asked("work.peek").is_empty());
}

/// A refusal closes the pane and says why.
#[tokio::test]
async fn a_refused_peek_closes_the_pane_with_its_reason() {
    let w = world();
    w.lock().unwrap().answers.insert(
        "work.peek".into(),
        Err((-32002, "no job act_k5".to_string())),
    );
    let mut rig = Rig::new(120, 30, script(w));
    on_the_tide_task(&mut rig).await;
    rig.press(&[KeyCode::Char('l')]).await;
    rig.shows("work.peek: no job act_k5").await;
    assert!(rig.runner.app.follow.is_none());
}
