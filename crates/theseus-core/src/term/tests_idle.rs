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

/// The wait's own count, "after N ms", from a read's text.
fn waited_ms(text: &str) -> u64 {
    let at = text.find(" after ").expect("a wait's count") + " after ".len();
    text[at..]
        .split(' ')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("no count in {text}"))
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
    let terms = terms();
    let id = bash(&terms, d.path()).await;
    // At an idle prompt, at once.
    tokio::time::sleep(IDLE_SETTLE).await;
    let text = idle_read(&terms, &id, 5_000, d.path()).await;
    assert!(text.contains("Waited: its program is idle"), "{text}");
    assert!(waited_ms(&text) < 100, "{text}");
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
    let began = std::time::SystemTime::now();
    let text = idle_read(&terms, &id, 10_000, d.path()).await;
    assert!(text.contains("Waited: its program is idle"), "{text}");
    let finished = std::fs::metadata(&marker)
        .expect("the read returned before the command finished")
        .modified()
        .unwrap();
    // When the wait ended, by its own count from its start: the time the
    // test then takes to get its answer is the scheduler's, not the wait's,
    // and on a loaded machine it is not small (117 ms seen under four busy
    // loops at nice 19).
    let ended = began + Duration::from_millis(waited_ms(&text));
    let late = ended.duration_since(finished).unwrap_or_default();
    assert!(
        late < Duration::from_millis(100),
        "{late:?} after the command:\n{text}"
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
    let terms = terms();
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
    let t = Terms::new(Vec::new()).configured(true, 900);
    assert_eq!(t.idle_max_ms, 900_000);
    let t = Terms::new(Vec::new()).configured(true, 10);
    assert_eq!(t.idle_max_ms, MAX_WAIT_MS);
}

/// In front is not idle (the join's review): a shell between the commands
/// of a list holds the front for a moment, so a look then read idle with
/// the list half run (1 of 5 lists of 60 `sleep 0.01`s). Here each command
/// is followed by a builtin's output, which the shell writes in front and
/// which wakes the wait at once. Idle is also waiting for input.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_list_between_its_commands_is_not_idle() {
    let d = tempfile::tempdir().unwrap();
    let terms = terms();
    let id = bash(&terms, d.path()).await;
    for round in 0..5 {
        let done = d.path().join(format!("done{round}"));
        let text = format!(
            "{}touch {}\n",
            "sleep 0.01; echo -n .; ".repeat(60),
            done.display()
        );
        call(
            &terms,
            "s1",
            SEND,
            json!({"terminal": id, "text": text}),
            d.path(),
        )
        .await;
        let text = idle_read(&terms, &id, 20_000, d.path()).await;
        assert!(text.contains("Waited: its program is idle"), "{text}");
        assert!(
            done.exists(),
            "round {round}: idle before the list ended:\n{text}"
        );
    }
    terms.close_session("s1", BY_TOOL);
}

/// A REPL computing never gives up the front of its pty, so in front alone
/// read idle once the settle passed (124 ms into a 2 s sleep, the join's
/// review); waiting for input is what says its line is done.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_repl_computing_is_not_idle() {
    let d = tempfile::tempdir().unwrap();
    let terms = terms();
    let (o, _) = call(
        &terms,
        "s1",
        OPEN,
        json!({"argv": ["python3", "-q"]}),
        d.path(),
    )
    .await;
    let py = id_of(&o);
    read_until(&terms, "s1", &py, ">>> ", d.path()).await;
    let text = "import time; time.sleep(1); print('sl' + 'ept')\n";
    // From before the send, not by the wait's own count: that starts when the
    // read does, which on a loaded machine is long after python began its
    // second (882 ms seen), while python's sleep can only end a second after
    // the send.
    let sent = Instant::now();
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": py, "text": text}),
        d.path(),
    )
    .await;
    let text = idle_read(&terms, &py, 10_000, d.path()).await;
    assert!(text.contains("Waited: its program is idle"), "{text}");
    assert!(
        text.contains("\nslept\n") || text.contains("|slept"),
        "{text}"
    );
    assert!(sent.elapsed() >= Duration::from_millis(1_000), "{text}");
    terms.close_session("s1", BY_TOOL);
}

/// The syscall numbers `waits_in` reads, by architecture: read, then the
/// select, poll and epoll calls (aarch64 has no `select` or `poll` of its own,
/// so its first two are the `pselect6` and `ppoll` of the other columns).
#[cfg(target_arch = "x86_64")]
const NRS: [i64; 7] = [0, 23, 270, 7, 271, 232, 61];
#[cfg(target_arch = "aarch64")]
const NRS: [i64; 7] = [63, 72, 72, 73, 73, 22, 260];

/// A `/proc/<pid>/syscall` line: the number, six arguments, the stack
/// pointer and the program counter, as the kernel prints them.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn syscall_line(nr: i64, args: [u64; 6]) -> String {
    let args: Vec<String> = args.iter().map(|a| format!("0x{a:x}")).collect();
    format!("{nr} {} 0x7ffc755ca158 0x7f131cf1e9dd", args.join(" "))
}

/// The detector's own check, with no program in it, so that it does not
/// depend on which `python3` is first in PATH (theseus-yr2g3): CPython before
/// 3.11 sleeps in `time.sleep` with a `pselect6` of zero descriptors, and
/// `a_repl_computing_is_not_idle` read idle on such a host, 125 ms into a 1 s
/// sleep. The arguments are the ones the kernel printed for processes parked in
/// each call (x86_64, 2026-10-26). A select or poll that watches nothing is a sleep, not a wait for
/// input; one that watches a descriptor is.
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
#[test]
fn a_select_or_poll_that_watches_nothing_is_not_a_wait_for_input() {
    use super::pty::waits_in;
    let [read, select, pselect, poll, ppoll, epoll, wait] = NRS;
    let set = 0x7209_fa31_7f30;
    let fds = 0x79f5_01f1_7f10;
    let timeout = 0x7d3d_286a_7690;
    let waits = |nr, args| waits_in(&syscall_line(nr, args));
    // time.sleep under CPython 3.10: pselect6(0, NULL, NULL, NULL, &ts, 0).
    assert_eq!(waits(pselect, [0, 0, 0, 0, timeout, 0]), Some(false));
    assert_eq!(waits(select, [0, 0, 0, 0, timeout, 0]), Some(false));
    // A count with no set is a sleep too.
    assert_eq!(waits(select, [5, 0, 0, 0, timeout, 0]), Some(false));
    // The REPL's and the shell's prompt: pselect6(1, [0], NULL, NULL, ..).
    assert_eq!(waits(pselect, [1, set, 0, 0, 0, 0]), Some(true));
    assert_eq!(waits(select, [4, set, 0, 0, timeout, 0]), Some(true));
    assert_eq!(waits(select, [4, 0, set, 0, timeout, 0]), Some(true));
    // poll(fds, nfds, ms) and ppoll(fds, nfds, ts, ..): the count is arg 1.
    assert_eq!(waits(poll, [fds, 0, 2000, 0, 0, 0]), Some(false));
    assert_eq!(waits(poll, [fds, 1, 2000, 0, 0, 0]), Some(true));
    assert_eq!(waits(ppoll, [fds, 0, timeout, 0, 0, 0]), Some(false));
    assert_eq!(waits(ppoll, [fds, 1, timeout, 0, 0, 0]), Some(true));
    // An epoll wait names its epoll descriptor in arg 0: a watch set already.
    assert_eq!(waits(epoll, [3, fds, 64, 0, 0, 0]), Some(true));
    // read: the terminal's input, fd 0, and nothing else.
    assert_eq!(waits(read, [0, fds, 1024, 0, 0, 0]), Some(true));
    assert_eq!(waits(read, [3, fds, 1024, 0, 0, 0]), Some(false));
    // A child's wait, and a process not in a syscall.
    assert_eq!(waits(wait, [0xffff_ffff, fds, 0xa, 0, 0, 0]), Some(false));
    assert_eq!(waits_in("running\n"), Some(false));
}

/// A read's bound (the join's review: no test held it): `until_idle` alone
/// may wait up to `[tools] proc_sync_secs`; with `quiet_ms` or `until`, or
/// without `until_idle`, at most `MAX_WAIT_MS`; 10 s when no timeout is given.
#[test]
fn a_reads_bound_is_proc_runs_only_for_until_idle_alone() {
    let t = Terms::new(Vec::new()).configured(true, 900);
    let bound = |ms, alone| tools::bound_of(&t, ms, alone).as_millis();
    assert_eq!(bound(Some(900_000), true), 900_000);
    assert_eq!(bound(Some(2_000_000), true), 900_000);
    assert_eq!(bound(Some(900_000), false), u128::from(MAX_WAIT_MS));
    assert_eq!(bound(None, true), 10_000);
}

/// `until_idle` with `quiet_ms`: whichever comes first. A shell running a
/// silent command in front is not idle, and its quiet ends the read.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn until_idle_with_quiet_ends_at_the_quiet() {
    let d = tempfile::tempdir().unwrap();
    let terms = terms();
    let id = bash(&terms, d.path()).await;
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "text": "sleep 30\n"}),
        d.path(),
    )
    .await;
    let (o, _) = call(
        &terms,
        "s1",
        READ,
        json!({"terminal": id, "until_idle": true, "quiet_ms": 300, "timeout_ms": 20_000}),
        d.path(),
    )
    .await;
    assert!(
        o.text.contains("Waited: quiet for 300 ms after "),
        "{}",
        o.text
    );
    assert!(waited_ms(&o.text) < 10_000, "{}", o.text);
    terms.close_session("s1", BY_TOOL);
}

/// A person's message queued behind the turn ends a terminal's wait at once,
/// the program untouched (theseus-qu1lc): the read says a message arrived,
/// its meta names it, and the terminal still runs what was typed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_message_queued_behind_the_turn_ends_a_terminals_wait() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().canonicalize().unwrap();
    let terms = terms();
    let id = bash(&terms, &dir).await;
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "text": "sleep 20\n"}),
        &dir,
    )
    .await;
    let steers = terms.steers.clone();
    let turning = steers.open("s1", true);
    let waiting = {
        let (terms, id, dir) = (terms.clone(), id.clone(), dir.clone());
        tokio::spawn(async move {
            let t0 = Instant::now();
            let (o, _) = call(
                &terms,
                "s1",
                READ,
                json!({"terminal": id, "until_idle": true, "timeout_ms": 20_000}),
                &dir,
            )
            .await;
            (o, t0.elapsed())
        })
    };
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    let offer = steers
        .offer(
            "s1",
            crate::toolrun::waits::steer::Steered {
                text: "stop it".into(),
                attachments: vec![],
                author: "test".into(),
                owner: false,
                surface: None,
                reply_to: None,
                arrived: Instant::now(),
            },
        )
        .unwrap();
    let (o, took) = waiting.await.unwrap();
    assert!(
        took < std::time::Duration::from_secs(3),
        "{took:?}: {}",
        o.text
    );
    assert!(o.text.contains("a message arrived"), "{}", o.text);
    assert_eq!(o.meta["interrupted_by"], "message");
    // The program was left alone: still not back at its prompt, once the
    // message is no longer waiting.
    drop(offer);
    assert!(idle_read(&terms, &id, 300, &dir).await.contains("not idle"));
    drop(turning);
}
