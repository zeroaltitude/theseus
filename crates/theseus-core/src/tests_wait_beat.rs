//! A `job_wait` on a job that prints nothing returns at `[tools]
//! job_wait_beat_secs` with what it saw (theseus-53ujg.6), on tokio's paused
//! clock, with the jobs held by the test as `tests_wait_cost.rs` holds them
//! (this file is its child: `super::*` is its rig).

use super::*;

/// The correlation id of the session's one job.
fn the_job(r: &Rig) -> String {
    r.core
        .kernel
        .open_actions()
        .unwrap()
        .into_iter()
        .find(|a| a.tool == "proc.run")
        .expect("a job")
        .correlation_id
}

/// Put `text` in the job's spool result, where its wrapper writes its output.
fn print(r: &Rig, corr: &str, text: &str) {
    use std::io::Write;
    let path = r.core.spool.result_path(corr);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    f.write_all(text.as_bytes()).unwrap();
}

fn beat(secs: u64) -> impl FnOnce(&mut Config) {
    move |c| c.tools.job_wait_beat_secs = secs
}

/// A job that prints nothing returns a wait asked for 1,500 s at 600 s, and
/// the result says what the wait saw.
#[tokio::test(start_paused = true)]
async fn a_silent_jobs_wait_returns_at_the_beat_with_what_it_saw() {
    let r = rig(script(1500), beat(600));
    let s = r.session();
    let turn = r.submit(&s, "search it");
    r.until_calls(2).await;
    let t0 = Instant::now();
    turn.await.unwrap();
    let took = t0.elapsed();
    assert!(
        took >= Duration::from_secs(599) && took < Duration::from_secs(603),
        "returned after {took:?}"
    );
    let (_, text, meta) = r.result(&s.session_id, "t2");
    assert!(text.contains("No output since it started."), "{text}");
    assert!(
        text.contains("the job printed nothing in that time"),
        "{text}"
    );
    assert_eq!(
        (&meta["beat"], &meta["state"]),
        (&json!(true), &json!("running"))
    );
    assert!(meta["silent_secs"].as_u64().unwrap() >= 599, "{meta}");
    let next = serde_json::to_string(&r.turns.requests()[2].messages).unwrap();
    assert!(
        next.contains("printed nothing in that time"),
        "the next request carries it"
    );
}

/// A job that prints every 100 s keeps a 900 s wait to 900 s.
#[tokio::test(start_paused = true)]
async fn a_job_that_prints_keeps_its_wait() {
    let r = rig(script(900), beat(600));
    let s = r.session();
    let turn = r.submit(&s, "build it");
    r.until_calls(2).await;
    let corr = the_job(&r);
    let t0 = Instant::now();
    let core = r.core.clone();
    let printer = tokio::spawn(async move {
        for n in 1..=8 {
            tokio::time::sleep(Duration::from_secs(100)).await;
            let path = core.spool.result_path(&corr);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let old = std::fs::read_to_string(&path).unwrap_or_default();
            std::fs::write(&path, format!("{old}line {n}\n")).unwrap();
        }
    });
    turn.await.unwrap();
    printer.abort();
    let took = t0.elapsed();
    assert!(
        took >= Duration::from_secs(899) && took < Duration::from_secs(903),
        "returned after {took:?}"
    );
    let (_, text, meta) = r.result(&s.session_id, "t2");
    assert!(text.starts_with("Still running after 900 s."), "{text}");
    assert!(meta.get("beat").is_none(), "{meta}");
    assert!(text.contains("8 lines so far"), "{text}");
}

/// With the beat off the wait is today's, word for word and key for key.
#[tokio::test(start_paused = true)]
async fn the_beat_off_is_todays_wait() {
    let r = rig(script(1500), beat(0));
    let s = r.session();
    let turn = r.submit(&s, "search it");
    r.until_calls(2).await;
    let t0 = Instant::now();
    turn.await.unwrap();
    let took = t0.elapsed();
    assert!(
        took >= Duration::from_secs(1499) && took < Duration::from_secs(1503),
        "returned after {took:?}"
    );
    let (_, text, meta) = r.result(&s.session_id, "t2");
    assert_eq!(
        text,
        "Still running after 1500 s. job_read j1 shows its latest output; its result also \
         arrives by itself when it ends."
    );
    let mut keys: Vec<&String> = meta.as_object().unwrap().keys().collect();
    keys.sort();
    assert_eq!(keys, ["correlation_id", "job", "state", "warm_reads"]);
}

/// A still-running wait that is not the beat's says the job's last lines and
/// how long it has been quiet, and its CPU only where a cgroup gave it.
#[tokio::test(start_paused = true)]
async fn a_still_running_wait_says_its_output_and_cpu() {
    let r = rig(script(120), beat(600));
    let s = r.session();
    let turn = r.submit(&s, "build it");
    r.until_calls(2).await;
    print(&r, &the_job(&r), "compiling a\ncompiling b\ndone linking\n");
    turn.await.unwrap();
    let (_, text, meta) = r.result(&s.session_id, "t2");
    assert!(text.starts_with("Still running after 120 s."), "{text}");
    // It printed at the wait's first look, a second in: quiet for the rest.
    assert!(
        text.contains(
            "No new output for 119 s; 3 lines so far:\ncompiling a\ncompiling b\ndone linking"
        ),
        "{text}"
    );
    assert!(
        !text.contains("CPU"),
        "no cgroup in a test, so no CPU words: {text}"
    );
    assert!(meta.get("beat").is_none(), "{meta}");
}

/// A wait returned at the beat had its keep-warm reads before it.
#[tokio::test(start_paused = true)]
async fn the_beat_does_not_starve_the_cache() {
    let r = rig(script(1500), beat(600));
    let s = r.session();
    let turn = r.submit(&s, "search it");
    r.until_calls(2).await;
    turn.await.unwrap();
    assert_eq!(r.warm_reads().len(), 2, "reads at 240 s and 480 s");
    let (_, _, meta) = r.result(&s.session_id, "t2");
    assert_eq!(meta["warm_reads"], 2);
    assert_eq!(meta["beat"], true);
}

// ---- the status line at each beat (theseus-5d204)

/// The place the bound session's posts go to.
const PLACE: &str = "discord:dm:7";

/// Bind the session to a private place, so its notices are outbox posts.
fn bind(r: &Rig, s: &SessionRecord) {
    r.core.outbox.bind_place("dm:7", &s.session_id).unwrap();
    r.core
        .runner
        .place_rule
        .bind_one(crate::places::BoundPlace {
            target: PLACE.into(),
            name: "DM".into(),
            private: true,
            ..Default::default()
        });
}

/// The notices the place has been sent: what the status line posts.
fn statuses(r: &Rig) -> Vec<String> {
    r.core
        .outbox
        .open_for(PLACE)
        .iter()
        .filter(|a| crate::outbox::kind_of(a) == "notice")
        .map(|a| {
            crate::outbox::body_of(a)["text"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

fn status_off(c: &mut Config) {
    c.tools.job_wait_beat_secs = 600;
    c.tools.job_wait_status = false;
}

/// The key test: a silent job's beat posts one plain line to the place, from
/// facts alone, and the model's requests and results are as they were.
#[tokio::test(start_paused = true)]
async fn a_silent_jobs_beat_posts_one_status_line_to_its_place() {
    let r = rig(script(1500), beat(600));
    let s = r.session();
    bind(&r, &s);
    let turn = r.submit(&s, "search it");
    r.until_calls(2).await;
    print(&r, &the_job(&r), "round 1\nround 2\n");
    turn.await.unwrap();
    let lines = statuses(&r);
    assert_eq!(lines.len(), 1, "{lines:?}");
    let l = &lines[0];
    for want in [
        "j1",
        "still running",
        "10 min",
        "no new output",
        "\"round 2\"",
    ] {
        assert!(l.contains(want), "{want:?} in {l}");
    }
    assert!(!l.contains('\n'), "{l:?}");
    assert_eq!(r.turns.requests().len(), 3, "no model call of its own");
    let next = serde_json::to_string(&r.turns.requests()[2].messages).unwrap();
    assert!(!next.contains("still running:"), "the model never reads it");
}

/// A wait that does not beat posts nothing: the job prints, the beat is off,
/// or the switch is.
#[tokio::test(start_paused = true)]
async fn no_status_line_without_a_beat() {
    // A job that prints every 100 s keeps its 900 s wait.
    let r = rig(script(900), beat(600));
    let s = r.session();
    bind(&r, &s);
    let turn = r.submit(&s, "build it");
    r.until_calls(2).await;
    let corr = the_job(&r);
    let core = r.core.clone();
    let printer = tokio::spawn(async move {
        for n in 1..=8 {
            tokio::time::sleep(Duration::from_secs(100)).await;
            let path = core.spool.result_path(&corr);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let old = std::fs::read_to_string(&path).unwrap_or_default();
            std::fs::write(&path, format!("{old}line {n}\n")).unwrap();
        }
    });
    turn.await.unwrap();
    printer.abort();
    assert_eq!(statuses(&r), Vec::<String>::new(), "it printed");

    // The beat off fires nothing.
    let r = rig(script(1500), beat(0));
    let s = r.session();
    bind(&r, &s);
    r.submit(&s, "search it").await.unwrap();
    assert_eq!(statuses(&r), Vec::<String>::new(), "no beat");

    // The switch off leaves the beat as it is, and posts nothing.
    let on = rig(script(1500), beat(600));
    let s = on.session();
    on.submit(&s, "search it").await.unwrap();
    let with = on.result(&s.session_id, "t2");
    let off = rig(script(1500), status_off);
    let s = off.session();
    bind(&off, &s);
    off.submit(&s, "search it").await.unwrap();
    let without = off.result(&s.session_id, "t2");
    assert_eq!(with.1, without.1, "the beat's text is the same");
    assert_eq!(without.2["beat"], true);
    assert_eq!(statuses(&off), Vec::<String>::new(), "the switch is off");
}

/// Each beat posts its own line: a second wait on the same silent job beats
/// again, and the place has two.
#[tokio::test(start_paused = true)]
async fn each_beat_posts_its_own_line() {
    let mut steps = script(1500);
    steps.insert(
        2,
        Scripted::tools(
            "Waiting more.",
            &[("t3", "job_wait", json!({"job": "j1", "timeout_secs": 1500}))],
        ),
    );
    let r = rig(steps, beat(600));
    let s = r.session();
    bind(&r, &s);
    r.submit(&s, "search it").await.unwrap();
    let lines = statuses(&r);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(
        lines.iter().all(|l| l.contains("still running")),
        "{lines:?}"
    );
}

/// A job past its file's head is not silent, and nothing is posted.
#[tokio::test(start_paused = true)]
async fn a_held_job_posts_nothing() {
    let r = rig(script(900), |c| {
        c.tools.job_wait_beat_secs = 600;
        c.tools.job_output_max_bytes = 1 << 20;
    });
    let s = r.session();
    bind(&r, &s);
    let turn = r.submit(&s, "build it");
    r.until_calls(2).await;
    let (head, _) = theseus_kernel::redact::split(r.core.tools.output_max_bytes);
    print(&r, &the_job(&r), &"x".repeat(head as usize));
    turn.await.unwrap();
    let (_, text, meta) = r.result(&s.session_id, "t2");
    assert!(meta.get("beat").is_none(), "{text}");
    assert_eq!(statuses(&r), Vec::<String>::new());
}

/// A session with no place gets its beat result and no post.
#[tokio::test(start_paused = true)]
async fn an_unbound_session_posts_nothing() {
    let r = rig(script(1500), beat(600));
    let s = r.session();
    r.submit(&s, "search it").await.unwrap();
    let (_, _, meta) = r.result(&s.session_id, "t2");
    assert_eq!(meta["beat"], true);
    assert_eq!(statuses(&r), Vec::<String>::new());
}

const SECRET: &str = "invented-secret-1a2b3c4d5e6f7a8b";

/// The one status line posted after the job printed `printed` and went quiet,
/// with SECRET in the vault.
async fn posted_after_printing(printed: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path(), false, beat(600));
    let store = Store::open(&dir.path().join("store")).unwrap();
    let turns = Arc::new(FakeProvider::scripted(script(1500)));
    let reads = Arc::new(FakeProvider::default());
    let split = Arc::new(Split {
        turns: turns.clone(),
        reads: reads.clone(),
    });
    let gate = Arc::new(Gated::default());
    let mut parts = crate::rpc::Parts::for_tests(cfg, split, store);
    parts.launcher = gate.clone();
    parts.scrubber = Arc::new(crate::scrub::Scrubber::with_values(vec![(
        SECRET.into(),
        "demo_secret".into(),
    )]));
    let r = Rig {
        core: Core::build(parts).unwrap(),
        turns,
        reads,
        gate,
        _dir: dir,
    };
    let s = r.session();
    bind(&r, &s);
    let turn = r.submit(&s, "search it");
    r.until_calls(2).await;
    print(&r, &the_job(&r), &format!("{printed}\n"));
    turn.await.unwrap();
    let lines = statuses(&r);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("Last line:"), "{}", lines[0]);
    lines[0].clone()
}

/// A secret the job printed is not in the line that leaves.
#[tokio::test(start_paused = true)]
async fn the_status_line_is_scrubbed() {
    let line = posted_after_printing(&format!("token {SECRET}")).await;
    assert!(!line.contains(SECRET), "{line}");
}

/// A secret the 160-char cut would split leaks no prefix: the line is
/// scrubbed before it is cut (the join fix of theseus-5d204).
#[tokio::test(start_paused = true)]
async fn a_secret_split_by_the_cut_leaks_no_prefix() {
    let line = posted_after_printing(&format!("{}{SECRET}", "x".repeat(150))).await;
    for n in 8..=SECRET.len() {
        assert!(!line.contains(&SECRET[..n]), "{n} chars leaked: {line}");
    }
}
