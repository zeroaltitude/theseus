//! What a terminal's close leaves running (theseus-ggqf): at a session's end
//! and at the daemon's stop, with `keep_background`, the program and the
//! pty's foreground process group end and the background runs on; at
//! `term.close`, a cancel and a `/stop`, or with `keep_background` off,
//! everything ends. Each marker holds this run's pid, so another run's
//! processes are never taken for this one's (theseus-d006).
//!
//! A pty test that leaves a job in the shell's own group cannot use
//! `bash -c 'sleep N & exit'` alone: the shell's exit hangs up the tty's
//! foreground group, which is that group, and the sleep dies before it can be
//! counted (observed: `marked=[]`, `holders=0`), so those tests run
//! `trap '' HUP` first.

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
        let terms = terms();
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
        let terms = Arc::new(Arc::try_unwrap(terms()).ok().unwrap().configured(keep, 60));
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
    let terms = terms();
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
    let terms = terms();
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
    let terms = terms();
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
    let terms = terms();
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

/// A shell that has already exited, with a background job it started still
/// running: the close finds the job by the pty it holds, and names it as it
/// names a job of a live shell, so the book tracks it (theseus-d1zw). The
/// positive is the job's own pid, `sleep` and why it was left.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shell_that_exited_leaves_its_background_job_tracked() {
    let d = tempfile::tempdir().unwrap();
    let terms = terms();
    let run = run_tag(8);
    let job = mark(&run, AMPERSAND);
    let (o, _) = call(
        &terms,
        "s1",
        OPEN,
        json!({"argv": ["bash", "--norc", "--noprofile", "-i"], "cols": 200}),
        d.path(),
    )
    .await;
    let sh = id_of(&o);
    let text = format!("PS1='k''eep# '; sleep {job} & exit\n");
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": sh, "text": text}),
        d.path(),
    )
    .await;
    until("the job started and the shell exited", || {
        !marked(&job).is_empty()
            && terms
                .all()
                .iter()
                .any(|t| t.id == sh && t.pty.exited().is_some())
    })
    .await;
    let pid = marked(&job)[0];
    let closed = tokio::task::spawn_blocking({
        let terms = terms.clone();
        move || terms.close_where(BY_SESSION_END, |t| t.session == "s1")
    })
    .await
    .unwrap();
    assert_eq!(closed.len(), 1);
    let c = &closed[0];
    let named: Vec<(u32, &str, &str)> = c
        .left
        .iter()
        .map(|k| (k.proc.pid, k.program.as_str(), k.why))
        .collect();
    assert_eq!(
        named,
        vec![(pid, "sleep", pty::WHY_BACKGROUND)],
        "the job of the exited shell is named"
    );
    assert_eq!(terms.left_info().len(), 1);
    assert_eq!(marked(&job).len(), 1, "it was left running, not killed");
    let ended = tokio::task::spawn_blocking({
        let terms = terms.clone();
        move || terms.end_left("s1")
    })
    .await
    .unwrap();
    assert_eq!(ended.len(), 1);
    until("the left job ended", || marked(&job).is_empty()).await;
}

/// Closes a terminal whose foreground forks a child into a session of its own
/// every 2 ms and ignores SIGTERM and SIGHUP, so the forks go on through the
/// grace and across the close's last scan (theseus-z0kk: 2 of 23 closes left
/// a child). A child that lands in a new session between the scan and the
/// kill is in no group the close signals. Returns how many of `n` closes left
/// a child alive, each child named by its pid.
async fn closes_leaving_a_child(
    n: u32,
    tag: &str,
    keep: bool,
    by: &'static str,
) -> (u32, Vec<String>) {
    closes_leaving(n, tag, keep, by, false).await
}

/// `closes_leaving_a_child`, with the forker either in the foreground or, with
/// `shell_exits`, left as a background job of a shell that has exited by the
/// time of the close (the close then takes the exited-shell path). In that
/// mode each close also holds, hard, what is deterministic: the forker
/// forked, one `Closed`, nothing left, and something killed.
async fn closes_leaving(
    n: u32,
    tag: &str,
    keep: bool,
    by: &'static str,
    shell_exits: bool,
) -> (u32, Vec<String>) {
    let mut left_by = 0;
    let mut why = Vec::new();
    for i in 0..n {
        let d = tempfile::tempdir().unwrap();
        let terms = Arc::new(Arc::try_unwrap(terms()).ok().unwrap().configured(keep, 60));
        let run = format!("{}{i:02}", run_tag(0));
        let m = mark(&run, tag);
        let (o, _) = call(
            &terms,
            "s1",
            OPEN,
            json!({"argv": ["bash", "--norc", "--noprofile", "-i"], "cols": 200}),
            d.path(),
        )
        .await;
        let sh = id_of(&o);
        let tail = if shell_exits { " & exit" } else { "" };
        let text = format!(
            "PS1='k''eep# '; sh -c \"trap '' TERM HUP; while :; do setsid sleep {m} & sleep 0.002; done\"{tail}\n"
        );
        call(
            &terms,
            "s1",
            SEND,
            json!({"terminal": sh, "text": text}),
            d.path(),
        )
        .await;
        until("the forker forked", || {
            marked(&m).len() > 3
                && (!shell_exits
                    || terms
                        .all()
                        .iter()
                        .any(|t| t.id == sh && t.pty.exited().is_some()))
        })
        .await;
        let closed = tokio::task::spawn_blocking({
            let terms = terms.clone();
            move || terms.close_where(by, |t| t.session == "s1")
        })
        .await
        .unwrap();
        if shell_exits {
            assert_eq!(closed.len(), 1, "close {i}: one Closed");
            assert!(closed[0].left.is_empty(), "close {i}: {:?}", closed[0]);
            assert!(closed[0].killed >= 1, "close {i}: killed {:?}", closed[0]);
        }
        // A killed process takes a moment to die.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let alive = marked(&m);
        let named: Vec<u32> = closed[0].left.iter().map(|k| k.proc.pid).collect();
        // Under a keeping close a survivor is acceptable only when named.
        let unnamed: Vec<&u32> = alive.iter().filter(|p| !named.contains(p)).collect();
        if !unnamed.is_empty() {
            left_by += 1;
            why.push(format!("close {i}: {} unnamed survivors", unnamed.len()));
        }
        for p in alive {
            // SAFETY: a test's own stray sleep, found by its marker.
            unsafe { libc::kill(p as libc::pid_t, libc::SIGKILL) };
        }
        terms.end_left("s1");
    }
    (left_by, why)
}

/// The rate, measured: how often a close leaves a child forked in the window.
/// Run with `--nocapture` to read it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_close_leaves_no_child_forked_into_a_new_session_during_it() {
    let n = 30;
    let (bad, why) = closes_leaving_a_child(n, "17", false, BY_TOOL).await;
    println!("z0kk: {bad} of {n} closes left a child: {why:?}");
    assert_eq!(bad, 0, "{bad} of {n} closes left a child: {why:?}");
}

/// The same under a keeping close: what it leaves is named, and a child
/// forked into a session of its own during the close is left too, so it is
/// named as the others are, never silently (a close that under-reports is
/// worse than one that leaks).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_keeping_close_names_a_child_forked_into_a_new_session_during_it() {
    let n = 20;
    let (bad, why) = closes_leaving_a_child(n, "18", true, BY_SESSION_END).await;
    println!("z0kk keep: {bad} of {n} closes left an unnamed child: {why:?}");
    assert_eq!(bad, 0, "{bad} of {n} closes left an unnamed child: {why:?}");
}

/// The same race on the exited-shell path: the forker is a background job of a
/// shell that has already exited, so the close finds it by the pty it holds
/// (`after_exit`). The close repeats its scan and kill as the live path does.
/// A measured rate over a few closes with a bound of one, not a hard zero: the
/// residual of the race is unmeasured, and the two tests above are the hard
/// ones.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_close_of_an_exited_shell_leaves_no_child_forked_into_a_new_session() {
    let n = 20;
    let (bad, why) = closes_leaving(n, "19", false, BY_TOOL, true).await;
    println!("exited shell: {bad} of {n} closes left a child: {why:?}");
    assert!(bad <= 1, "{bad} of {n} closes left a child: {why:?}");
}

/// The deterministic half of `after_exit`'s kill branch: a close that keeps
/// nothing ends the job of a shell that had exited, counts it once, and names
/// nothing as left.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_close_that_keeps_nothing_kills_the_job_of_an_exited_shell() {
    let d = tempfile::tempdir().unwrap();
    let terms = terms();
    let run = run_tag(11);
    let job = mark(&run, AMPERSAND);
    let (o, _) = call(
        &terms,
        "s1",
        OPEN,
        json!({"argv": ["bash", "--norc", "--noprofile", "-i"], "cols": 200}),
        d.path(),
    )
    .await;
    let sh = id_of(&o);
    let text = format!("PS1='k''eep# '; sleep {job} & exit\n");
    call(
        &terms,
        "s1",
        SEND,
        json!({"terminal": sh, "text": text}),
        d.path(),
    )
    .await;
    until("the job started and the shell exited", || {
        !marked(&job).is_empty()
            && terms
                .all()
                .iter()
                .any(|t| t.id == sh && t.pty.exited().is_some())
    })
    .await;
    let closed = tokio::task::spawn_blocking({
        let terms = terms.clone();
        move || terms.close_where(BY_TOOL, |t| t.session == "s1")
    })
    .await
    .unwrap();
    assert_eq!(closed.len(), 1);
    assert!(closed[0].left.is_empty(), "{:?}", closed[0]);
    assert_eq!(
        closed[0].killed, 1,
        "the one job is counted: {:?}",
        closed[0]
    );
    until("the job is gone", || marked(&job).is_empty()).await;
}

/// The kill's count is of distinct processes: one signalled again in a later
/// round is not counted again. (The double count cannot be forced with real
/// processes: it needs one still running after its SIGKILL.)
#[test]
fn killed_counts_distinct_processes() {
    let p = |pid| theseus_kernel::tree::Proc { pid, start: 7 };
    let (a, b, c) = (p(10), p(11), p(12));
    let mut seen = std::collections::BTreeSet::new();
    assert_eq!(pty::count_new(&mut seen, [a, b]), 2);
    assert_eq!(pty::count_new(&mut seen, [b, c]), 1);
    assert_eq!(pty::count_new(&mut seen, []), 0);
    assert_eq!(seen.len(), 3);
}

/// Open a terminal running `bash -c <script>` and wait until its program has
/// been reaped; the terminal, for the predicate's tests below.
async fn exited_bash(terms: &Arc<Terms>, dir: &std::path::Path, script: &str) -> Arc<Terminal> {
    let (o, _) = call(
        terms,
        "s1",
        OPEN,
        json!({"argv": ["bash", "--norc", "--noprofile", "-c", script], "cols": 200}),
        dir,
    )
    .await;
    let id = id_of(&o);
    let t = terms.map.lock().unwrap().get(&id).unwrap().clone();
    until("the shell exited", || t.pty.exited().is_some()).await;
    t
}

/// `close` signals the group of a program it found already reaped only while
/// a holder of the pty is still in that group (theseus-3i6d1, finding F5 of
/// the file-tools review). A race cannot be forced (the kernel will not hand
/// out a chosen pgid), so this asserts the predicate, not the race. Without
/// `-i` there is no job control, so the background sleep stays in the shell's
/// own group and keeps the pty. The shell ignores SIGHUP first: its exit hangs
/// up the tty's foreground group, which is that group, and would kill the
/// sleep before it is counted. Real processes and `until`'s real 10 s bound;
/// no paused clock.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_group_of_an_exited_program_is_ours_only_while_a_pty_holder_is_in_it() {
    let d = tempfile::tempdir().unwrap();
    let terms = terms();
    let job = mark(&run_tag(12), AMPERSAND);
    let _cleanup = KillMarked(job.clone());
    let t = exited_bash(
        &terms,
        d.path(),
        &format!("trap '' HUP; sleep {job} & exit"),
    )
    .await;
    until("the sleep holds the pty in the shell's group", || {
        !marked(&job).is_empty() && t.pty.holders_in_group() == 1
    })
    .await;
    assert_eq!(
        t.pty.holders_in_group(),
        1,
        "holders in the program's group"
    );
    assert!(
        t.pty.group_still_ours(),
        "the group is ours with {} holder in it",
        t.pty.holders_in_group()
    );
}

/// The empty case: nothing is left, so the group id is nobody's to signal,
/// the branch where the unguarded kill signalled a group it did not own.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_group_of_an_exited_program_with_nothing_left_is_not_ours() {
    let d = tempfile::tempdir().unwrap();
    let terms = terms();
    let t = exited_bash(&terms, d.path(), "exit 0").await;
    assert_eq!(
        t.pty.holders_in_group(),
        0,
        "holders in the program's group"
    );
    assert!(
        !t.pty.group_still_ours(),
        "the group is not ours with {} holders in it",
        t.pty.holders_in_group()
    );
}

/// The wiring of that guard, which the two tests above cannot see: they call
/// the predicate, and neither reaches `close`. Here a real `close` finds the
/// program already reaped with a job left in its own group that holds no pty,
/// so `group_still_ours()` is false and the group must NOT be signalled; the
/// job survives, and nothing is counted or named. Delete the `if` at the
/// guard in `Pty::close` and the unconditional kill ends the job, and this
/// fails. A race cannot be forced (the kernel will not hand out a chosen
/// pgid), so this asserts the branch, not the race. No `-i` (no job control,
/// the opposite of `a_close_that_keeps_nothing_kills_the_job_of_an_exited_shell`,
/// whose sleep is in its own group and meets `after_exit`), the sleep's stdio
/// is off the pty (so no holder is in the group), and `trap '' HUP` first (the
/// shell's exit hangs up the group, which is that one). Real processes and
/// `until`'s real 10 s bound; no paused clock.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_close_of_an_exited_program_leaves_a_group_member_that_holds_no_pty() {
    let d = tempfile::tempdir().unwrap();
    let terms = terms();
    let job = mark(&run_tag(13), AMPERSAND);
    let _cleanup = KillMarked(job.clone());
    let t = exited_bash(
        &terms,
        d.path(),
        &format!("trap '' HUP; sleep {job} >/dev/null 2>&1 </dev/null & exit"),
    )
    .await;
    until("the job started and the shell exited", || {
        marked(&job).len() == 1 && t.pty.exited().is_some()
    })
    .await;
    assert_eq!(
        t.pty.holders_in_group(),
        0,
        "the job holds no pty, so the guard's false branch is the one driven"
    );
    let closed = tokio::task::spawn_blocking({
        let terms = terms.clone();
        move || terms.close_where(BY_TOOL, |t| t.session == "s1")
    })
    .await
    .unwrap();
    assert_eq!(closed.len(), 1);
    assert_eq!(
        closed[0].killed, 0,
        "nothing was signalled: {:?}",
        closed[0]
    );
    assert!(closed[0].left.is_empty(), "{:?}", closed[0]);
    // Settle: a kill that did fire is seen, not raced past.
    let t0 = Instant::now();
    while marked(&job).len() == 1 && t0.elapsed() < Duration::from_secs(1) {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        marked(&job).len(),
        1,
        "the job in the exited program's group, holding no pty, survives the close: {:?}",
        marked(&job)
    );
}

/// Kills the processes of a marker when dropped, so a test that fails first
/// does not leave its sleep running.
struct KillMarked(String);

impl Drop for KillMarked {
    fn drop(&mut self) {
        for p in marked(&self.0) {
            // SAFETY: the test's own marked sleep.
            unsafe { libc::kill(p as libc::pid_t, libc::SIGKILL) };
        }
    }
}
