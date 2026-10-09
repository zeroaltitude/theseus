//! What a terminal's close leaves running (theseus-ggqf): at a session's end
//! and at the daemon's stop, with `keep_background`, the program and the
//! pty's foreground process group end and the background runs on; at
//! `term.close`, a cancel and a `/stop`, or with `keep_background` off,
//! everything ends. Each marker holds this run's pid, so another run's
//! processes are never taken for this one's (theseus-d006).

use std::time::Instant;

use serde_json::json;

use super::tests::{call, described, id_of, marked, read_until, terms};
use super::*;

/// The shapes a model starts in a shell, by their markers' tags.
const NOHUP: &str = "11";
const AMPERSAND: &str = "12";
const SETSID: &str = "13";
const DAEMON: &str = "14";
const FRONT: &str = "15";
const DEAF: &str = "16";

/// A marker: a sleep's seconds that name this run and the shape.
fn mark(run: &str, tag: &str) -> String {
    format!("47{tag}.{run}")
}

/// Wait until `pred` holds, failing with `what` after 10 s.
async fn until(what: &str, mut pred: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !pred() {
        assert!(t0.elapsed() < Duration::from_secs(10), "{what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// The processes of these markers alive now, described.
fn alive(marks: &[String]) -> Vec<String> {
    marks
        .iter()
        .flat_map(|m| marked(m))
        .map(described)
        .collect()
}

/// A bash in session `s1` that starts a `nohup` job, a bare `&` job, a
/// `setsid` child and a daemon that left its tree holding the pty; then, in
/// the foreground, a forker that ignores SIGTERM and SIGHUP, with a sleep of
/// its own and a sleep started in its group every 5 ms, so a scan never has
/// the whole foreground (theseus-z0kk).
async fn start(terms: &Arc<Terms>, run: &str, dir: &std::path::Path) -> String {
    let (o, _) = call(
        terms,
        "s1",
        OPEN,
        json!({"argv": ["bash", "--norc", "--noprofile", "-i"], "cols": 200}),
        dir,
    )
    .await;
    let sh = id_of(&o);
    let text = format!(
        "PS1='k''eep# '; nohup sleep {} >/dev/null 2>&1 & sleep {} & setsid sleep {}; \
         sh -c '(setsid sleep {} &)'; echo started\n",
        mark(run, NOHUP),
        mark(run, AMPERSAND),
        mark(run, SETSID),
        mark(run, DAEMON),
    );
    call(
        terms,
        "s1",
        SEND,
        json!({"terminal": sh, "text": text}),
        dir,
    )
    .await;
    read_until(terms, "s1", &sh, "started\nkeep# ", dir).await;
    let front = format!(
        "sh -c \"trap '' TERM HUP; sleep {} & while :; do sleep {} & sleep 0.005; done\"\n",
        mark(run, FRONT),
        mark(run, DEAF)
    );
    call(
        terms,
        "s1",
        SEND,
        json!({"terminal": sh, "text": front}),
        dir,
    )
    .await;
    let all: Vec<String> = [NOHUP, AMPERSAND, SETSID, DAEMON, FRONT, DEAF]
        .iter()
        .map(|t| mark(run, t))
        .collect();
    until("every shape started", || {
        all.iter().all(|m| !marked(m).is_empty())
    })
    .await;
    sh
}

fn run_tag(n: u32) -> String {
    // Seven digits of this run's pid, and the case: no marker holds another's.
    format!("{:07}{n}", std::process::id())
}

/// The keeping close, at a session's end and at the daemon's stop: the
/// background shapes survive and are named, why included; the foreground
/// forker, which ignores SIGTERM, and every sleep it started are gone after
/// its grace's SIGKILL to the group. A cancel's end of what was left then
/// ends the survivors too.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_end_and_a_daemon_stop_leave_the_background_running() {
    for (n, by) in [(1, BY_SESSION_END), (2, BY_DAEMON)] {
        let d = tempfile::tempdir().unwrap();
        let terms = terms(&[]);
        assert!(terms.keep_background, "the default keeps");
        let run = run_tag(n);
        let sh = start(&terms, &run, d.path()).await;
        let t0 = Instant::now();
        let closed = tokio::task::spawn_blocking({
            let terms = terms.clone();
            move || terms.close_where(by, |t| t.session == "s1")
        })
        .await
        .unwrap();
        let took = t0.elapsed();
        assert_eq!(closed.len(), 1, "{by}");
        let c = &closed[0];
        let gone: Vec<String> = [FRONT, DEAF].iter().map(|t| mark(&run, t)).collect();
        // Gone: the group's SIGKILL reached every member, one forked after
        // any scan included. A killed process takes a moment to die.
        let t1 = Instant::now();
        loop {
            let left = alive(&gone);
            if left.is_empty() {
                break;
            }
            assert!(
                t1.elapsed() < Duration::from_secs(3),
                "{by}: the foreground outlived the close:\n{}",
                left.join("\n")
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(c.killed > 0, "{c:?}");
        assert!(took >= CLOSE_GRACE, "the deaf foreground's grace: {took:?}");
        let kept: Vec<String> = [NOHUP, AMPERSAND, SETSID, DAEMON]
            .iter()
            .map(|t| mark(&run, t))
            .collect();
        // Still running, a moment after.
        tokio::time::sleep(Duration::from_millis(200)).await;
        for m in &kept {
            assert_eq!(marked(m).len(), 1, "{by}: {m} did not survive");
        }
        let named: Vec<(u32, &str)> = c.left.iter().map(|k| (k.proc.pid, k.why)).collect();
        for (tag, why) in [
            (NOHUP, pty::WHY_BACKGROUND),
            (AMPERSAND, pty::WHY_BACKGROUND),
            (SETSID, pty::WHY_SESSION),
            (DAEMON, pty::WHY_SESSION),
        ] {
            let pid = marked(&mark(&run, tag))[0];
            assert!(
                named.contains(&(pid, why)),
                "{by}: {tag} as {why:?} in {:?}",
                c.left
            );
        }
        assert_eq!(c.left.len(), 4, "{:?}", c.left);
        assert!(c.left.iter().all(|k| k.program == "sleep"));
        assert_eq!(c.meta()["left"].as_array().unwrap().len(), 4);
        // Health lists them while they run.
        let listed = terms.left_info();
        assert_eq!(listed.len(), 4, "{listed:?}");
        assert!(listed
            .iter()
            .all(|l| l.terminal == sh && l.session_id == "s1"));
        // A cancel's end of what was left: each, with its tree.
        let ended = tokio::task::spawn_blocking({
            let terms = terms.clone();
            move || terms.end_left("s1")
        })
        .await
        .unwrap();
        assert_eq!(ended.len(), 1);
        assert_eq!((ended[0].0.as_str(), ended[0].2.len()), (sh.as_str(), 4));
        until(&format!("{by}: what was left outlived its end"), || {
            alive(&kept).is_empty()
        })
        .await;
        assert!(terms.left_info().is_empty());
    }
}

/// `term.close`, a cancel and a `/stop` end everything a terminal started,
/// as before; so does any close with `keep_background` off.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_close_a_cancel_a_stop_and_keep_off_end_everything() {
    for (n, by, keep) in [
        (3, BY_TOOL, true),
        (4, BY_CANCEL, true),
        (5, BY_STOP, true),
        (6, BY_SESSION_END, false),
        (7, BY_DAEMON, false),
    ] {
        let d = tempfile::tempdir().unwrap();
        let terms = Arc::new(
            Arc::try_unwrap(terms(&[]))
                .ok()
                .unwrap()
                .configured(keep, 60),
        );
        let run = run_tag(n);
        start(&terms, &run, d.path()).await;
        let closed = tokio::task::spawn_blocking({
            let terms = terms.clone();
            move || terms.close_where(by, |t| t.session == "s1")
        })
        .await
        .unwrap();
        assert_eq!(closed.len(), 1);
        assert!(closed.iter().all(|c| c.left.is_empty()), "{closed:?}");
        let all: Vec<String> = [NOHUP, AMPERSAND, SETSID, DAEMON, FRONT, DEAF]
            .iter()
            .map(|t| mark(&run, t))
            .collect();
        let t0 = Instant::now();
        loop {
            let left = alive(&all);
            if left.is_empty() {
                break;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(5),
                "{by} (keep {keep}) left:\n{}",
                left.join("\n")
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(terms.left_info().is_empty());
    }
}

/// A terminal with nothing in the background closes as before: by the
/// hang-up, with nothing left. (Its time is `bench.rs`'s measure, not a
/// bound here: the suite runs under any load.)
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nothing_in_the_background_closes_as_before() {
    let d = tempfile::tempdir().unwrap();
    let terms = terms(&[]);
    let (o, _) = call(
        &terms,
        "s1",
        OPEN,
        json!({"argv": ["bash", "--norc", "--noprofile", "-i"]}),
        d.path(),
    )
    .await;
    let id = id_of(&o);
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "text": "PS1='q''uiet# '\n"}),
        d.path(),
    )
    .await;
    read_until(&terms, "s1", &id, "quiet# ", d.path()).await;
    let closed = tokio::task::spawn_blocking({
        let terms = terms.clone();
        move || terms.close_session("s1", BY_SESSION_END)
    })
    .await
    .unwrap();
    assert!(closed[0].left.is_empty(), "{:?}", closed[0]);
    assert!(terms.left_info().is_empty());
    // Ended by the hang-up, or killed after the grace on a loaded machine.
    assert!(
        closed[0].signal == Some(libc::SIGHUP) || closed[0].killed > 0,
        "{:?}",
        closed[0]
    );
}

/// In a child subreaper, as the daemon is, a keeping close of a bash with a
/// job in front and one behind does not wait out its grace: the foreground
/// job ends on its SIGTERM and stays the subreaper's zombie until a sweep,
/// and a zombie is no member that runs (the join's review: the close took
/// the whole grace, and the daemon's stop with it). The grace here is 10 s,
/// so a close that waits it out is told from a slow one on a loaded machine.
/// Each nextest test is a process of its own, so the subreaper is this
/// test's alone.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_keeping_close_in_a_subreaper_does_not_wait_out_a_zombies_grace() {
    // SAFETY: marks this test's own process a child subreaper.
    unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) };
    let d = tempfile::tempdir().unwrap();
    let terms = terms(&[]);
    let (o, _) = call(
        &terms,
        "s1",
        OPEN,
        json!({"argv": ["bash", "--norc", "--noprofile", "-i"], "cols": 200}),
        d.path(),
    )
    .await;
    let id = id_of(&o);
    let run = run_tag(8);
    let (back, front) = (mark(&run, AMPERSAND), mark(&run, FRONT));
    let text = format!("sleep {back} & sleep {front}\n");
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "text": text}),
        d.path(),
    )
    .await;
    until("both sleeps started", || {
        !marked(&back).is_empty() && !marked(&front).is_empty()
    })
    .await;
    let t = terms.get(&id).unwrap();
    let grace = Duration::from_secs(10);
    let t0 = Instant::now();
    let closed = tokio::task::spawn_blocking({
        let terms = terms.clone();
        move || terms.close_one(&t, BY_DAEMON, grace).unwrap()
    })
    .await
    .unwrap();
    let took = t0.elapsed();
    let left: Vec<u32> = closed.left.iter().map(|k| k.proc.pid).collect();
    assert_eq!(left, marked(&back), "{closed:?}");
    assert!(
        marked(&front).is_empty(),
        "the foreground outlived the close"
    );
    assert!(took < grace / 2, "the close waited out its grace: {took:?}");
    tokio::task::spawn_blocking(move || terms.end_left("s1"))
        .await
        .unwrap();
    until("what was left outlived its end", || {
        marked(&back).is_empty()
    })
    .await;
}

/// An end stops what a process it ends started after the close that left
/// it (`tree::stop`; the join's review: no test held it). The job left at a
/// daemon's stop ignores SIGTERM, as its later child does by inheritance,
/// so only the stop of its tree ends the child once the grace runs out.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_end_stops_what_a_left_job_started_after_its_close() {
    let d = tempfile::tempdir().unwrap();
    let terms = terms(&[]);
    let (o, _) = call(
        &terms,
        "s1",
        OPEN,
        json!({"argv": ["bash", "--norc", "--noprofile", "-i"], "cols": 200}),
        d.path(),
    )
    .await;
    let id = id_of(&o);
    let run = run_tag(9);
    let (first, later) = (mark(&run, SETSID), mark(&run, DAEMON));
    // The sleeps' seconds are spelt out by the shell, so the job's own
    // command line holds neither marker.
    let (f, t) = (first.split_at(2), later.split_at(2));
    let text = format!(
        "PS1='e''nd# '; sh -c 'trap \"\" TERM; f={}; t={}; sleep ${{f}}{}; sleep ${{t}}{}' & \
         echo started\n",
        f.0, t.0, f.1, t.1
    );
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "text": text}),
        d.path(),
    )
    .await;
    read_until(&terms, "s1", &id, "started\nend# ", d.path()).await;
    until("the job's first sleep", || !marked(&first).is_empty()).await;
    let closed = tokio::task::spawn_blocking({
        let terms = terms.clone();
        move || terms.close_where(BY_DAEMON, |t| t.session == "s1")
    })
    .await
    .unwrap();
    assert_eq!(
        closed[0].left.len(),
        2,
        "the job and its first sleep: {closed:?}"
    );
    assert!(marked(&later).is_empty(), "the later sleep started early");
    // The first sleep ends; the job, left, starts the later one.
    for p in marked(&first) {
        // SAFETY: a signal to this test's own sleep.
        unsafe { libc::kill(p as libc::pid_t, libc::SIGKILL) };
    }
    until("the job's later sleep", || !marked(&later).is_empty()).await;
    tokio::task::spawn_blocking({
        let terms = terms.clone();
        move || terms.end_left("s1")
    })
    .await
    .unwrap();
    until("what the left job started outlived its end", || {
        marked(&later).is_empty()
    })
    .await;
    assert!(terms.left_info().is_empty());
}

/// A shell that ends on SIGTERM (a trap; an interactive bash ignores it) is
/// not killed at once, and gets no hang-up, which a shell forwards to every
/// job it holds: its `&` job is kept (the join's review: with bash the
/// at-once SIGKILL hid a hang-up sent to the program).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shell_that_ends_on_sigterm_gets_no_hang_up_and_its_job_is_kept() {
    // A run under `nohup` ignores the hang-up, and so do the shell and its
    // jobs, which would hide one: this test's process takes the default.
    // SAFETY: a disposition of this test's own process (nextest runs each
    // test as a process of its own).
    unsafe { libc::signal(libc::SIGHUP, libc::SIG_DFL) };
    let d = tempfile::tempdir().unwrap();
    let terms = terms(&[]);
    let (o, _) = call(
        &terms,
        "s1",
        OPEN,
        json!({"argv": ["bash", "--norc", "--noprofile", "-i"], "cols": 200}),
        d.path(),
    )
    .await;
    let id = id_of(&o);
    let back = mark(&run_tag(10), AMPERSAND);
    let text = format!("trap 'exit 0' TERM; PS1='t''erm# '; sleep {back} & echo started\n");
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": id, "text": text}),
        d.path(),
    )
    .await;
    read_until(&terms, "s1", &id, "started\nterm# ", d.path()).await;
    let closed = tokio::task::spawn_blocking({
        let terms = terms.clone();
        move || terms.close_where(BY_SESSION_END, |t| t.session == "s1")
    })
    .await
    .unwrap();
    let c = &closed[0];
    assert_eq!(c.exit, Some(0), "the shell ended on its trap: {c:?}");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(marked(&back).len(), 1, "the job did not survive: {c:?}");
    assert_eq!(
        c.left.iter().map(|k| k.proc.pid).collect::<Vec<_>>(),
        marked(&back)
    );
    tokio::task::spawn_blocking(move || terms.end_left("s1"))
        .await
        .unwrap();
    until("what was left outlived its end", || {
        marked(&back).is_empty()
    })
    .await;
}
