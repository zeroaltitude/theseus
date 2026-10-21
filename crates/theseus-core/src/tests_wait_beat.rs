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
