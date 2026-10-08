//! `term.read`'s `until_idle` (theseus-ggqf): a read that waits for the
//! command typed to finish, by the pty's foreground process group: back to
//! the program (a shell at its prompt, a REPL waiting for its line).

use std::time::Instant;

use serde_json::json;

use super::tests::{call, id_of, read_until, terms};
use super::*;

/// A bash with a known prompt, read until it shows.
async fn bash(terms: &Arc<Terms>, dir: &std::path::Path) -> String {
    let (o, _) = call(
        terms,
        "s1",
        OPEN,
        json!({"argv": ["bash", "--norc", "--noprofile", "-i"], "rows": 10, "cols": 100}),
        dir,
    )
    .await;
    let id = id_of(&o);
    call(
        terms,
        "s1",
        SEND,
        json!({"terminal": id, "text": "PS1='i''dle# '\n"}),
        dir,
    )
    .await;
    read_until(terms, "s1", &id, "idle# ", dir).await;
    id
}

async fn idle_read(terms: &Arc<Terms>, id: &str, timeout_ms: u64, dir: &std::path::Path) -> String {
    let (o, _) = call(
        terms,
        "s1",
        READ,
        json!({"terminal": id, "until_idle": true, "timeout_ms": timeout_ms}),
        dir,
    )
    .await;
    o.text
}

/// `sleep 1` in bash: the read returns within 100 ms of the sleep's end
/// (the shell back in front), and at once at an idle prompt; `sleep 5 |
/// cat` (its own group) reads busy and times out cleanly, naming what is
/// in front; and `less` (its own group, full-screen) reads busy.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn until_idle_waits_for_the_typed_command() {
    let d = tempfile::tempdir().unwrap();
    let terms = terms(&[]);
    let id = bash(&terms, d.path()).await;
    // At an idle prompt, at once.
    tokio::time::sleep(IDLE_SETTLE).await;
    let t0 = Instant::now();
    let text = idle_read(&terms, &id, 5_000, d.path()).await;
    assert!(text.contains("Waited: its program is idle"), "{text}");
    assert!(
        t0.elapsed() < Duration::from_millis(100),
        "{:?}",
        t0.elapsed()
    );
    // A command of a second: the read ends once it does, and no sooner.
    let marker = d.path().join("slept");
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "text": format!("sleep 1; touch {}\n", marker.display())}),
        d.path(),
    )
    .await;
    let text = idle_read(&terms, &id, 10_000, d.path()).await;
    let returned = std::time::SystemTime::now();
    assert!(text.contains("Waited: its program is idle"), "{text}");
    let finished = std::fs::metadata(&marker)
        .expect("the read returned before the command finished")
        .modified()
        .unwrap();
    let late = returned.duration_since(finished).unwrap_or_default();
    assert!(
        late < Duration::from_millis(100),
        "{late:?} after the command"
    );
    // A pipeline in its own group: busy, and the bound ends the wait.
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "text": "sleep 5 | cat\n"}),
        d.path(),
    )
    .await;
    let t0 = Instant::now();
    let text = idle_read(&terms, &id, 700, d.path()).await;
    assert!(
        text.contains("Waited: its program was not idle in 700 ms (sleep is in front)")
            || text.contains("Waited: its program was not idle in 700 ms (cat is in front)"),
        "{text}"
    );
    assert!(t0.elapsed() >= Duration::from_millis(700));
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "keys": ["Ctrl-C"]}),
        d.path(),
    )
    .await;
    let text = idle_read(&terms, &id, 10_000, d.path()).await;
    assert!(text.contains("Waited: its program is idle"), "{text}");
    // A pager in front: busy until it quits.
    std::fs::write(d.path().join("f"), "line\n".repeat(50)).unwrap();
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "text": "less f\n"}),
        d.path(),
    )
    .await;
    let text = idle_read(&terms, &id, 500, d.path()).await;
    assert!(text.contains("(less is in front)"), "{text}");
    let (o, _) = call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "text": "q", "until_idle": true, "timeout_ms": 10_000}),
        d.path(),
    )
    .await;
    assert!(
        o.text
            .contains("Sent 1 characters of text; waited: its program is idle"),
        "{}",
        o.text
    );
    assert!(o.text.contains("|idle# less f\n"), "{}", o.text);
    terms.close_session("s1", BY_TOOL);
}

/// Python's REPL waiting at `>>>` reads idle, and a program that ends
/// ends a send's wait.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_repl_at_its_prompt_reads_idle_and_an_end_ends_the_wait() {
    let d = tempfile::tempdir().unwrap();
    let terms = terms(&[]);
    let (o, _) = call(
        &terms,
        "s1",
        OPEN,
        json!({"argv": ["python3", "-q"]}),
        d.path(),
    )
    .await;
    let id = id_of(&o);
    read_until(&terms, "s1", &id, ">>> ", d.path()).await;
    let text = idle_read(&terms, &id, 5_000, d.path()).await;
    assert!(text.contains("Waited: its program is idle"), "{text}");
    let (o, _) = call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "keys": ["Ctrl-D"], "until_idle": true, "timeout_ms": 5_000}),
        d.path(),
    )
    .await;
    assert!(o.text.contains("waited: its program ended"), "{}", o.text);
    terms.close_session("s1", BY_SESSION_END);
}

/// The bound: past `MAX_WAIT_MS` only for `until_idle` alone, and only as
/// far as `[tools] proc_sync_secs`.
#[test]
fn until_idle_alone_may_wait_as_long_as_proc_run_does() {
    let t = Terms::new(Vec::new(), Vec::new()).configured(true, 900);
    assert_eq!(t.idle_max_ms, 900_000);
    let t = Terms::new(Vec::new(), Vec::new()).configured(true, 10);
    assert_eq!(t.idle_max_ms, MAX_WAIT_MS);
}
