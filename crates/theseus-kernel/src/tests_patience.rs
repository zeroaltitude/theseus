//! A job's limit is a bound on its silence, with a ceiling (theseus-x1zz8):
//! a job still printing past the limit runs on and is told; a quiet one is
//! stopped; a talkative one is stopped at the ceiling.

use std::time::{Duration, Instant};

use crate::job::{run_wrapper, WrapperArgs, DEFAULT_OUTPUT_MAX_BYTES};
use crate::patience::CEILING_FACTOR;
use crate::spool::Spool;
use crate::types::Outcome;

fn run_sh(id: &str, deadline_ms: u64, script: &str) -> (Outcome, serde_json::Value, Duration) {
    run_in("sh", id, deadline_ms, script)
}

fn run_in(
    shell: &str,
    id: &str,
    deadline_ms: u64,
    script: &str,
) -> (Outcome, serde_json::Value, Duration) {
    let d = tempfile::tempdir().unwrap();
    let spool = Spool::open(d.path()).unwrap();
    let args = WrapperArgs {
        spool_dir: d.path().to_path_buf(),
        correlation_id: id.into(),
        deadline_ms,
        notify_socket: None,
        argv: vec![shell.into(), "-c".into(), script.into()],
        cwd: None,
        env: vec![("PATH".into(), std::env::var("PATH").unwrap_or_default())],
        umask: None,
        redact: vec![],
        output_max_bytes: DEFAULT_OUTPUT_MAX_BYTES,
        sandbox: None,
        cgroup: None,
    };
    let t0 = Instant::now();
    run_wrapper(&args).unwrap();
    let took = t0.elapsed();
    let c = spool.read_completion(id).unwrap().unwrap();
    (c.outcome, c.detail.unwrap(), took)
}

/// Twelve lines over 1.2 s against a 0.4 s limit: it is told, not stopped.
#[test]
fn a_job_still_printing_past_its_limit_runs_on_and_is_told() {
    let (outcome, detail, _) = run_sh(
        "act_talks",
        400,
        "i=0; while [ $i -lt 12 ]; do echo line; sleep 0.1; i=$((i+1)); done",
    );
    assert_eq!(outcome, Outcome::Succeeded, "{detail}");
    assert!(detail.get("timed_out").is_none(), "{detail}");
    let told = &detail["ran_past_limit_ms"];
    assert_eq!(told["limit_ms"], 400, "{detail}");
    assert_eq!(told["ceiling_ms"], 400 * CEILING_FACTOR as u64, "{detail}");
    assert!(told["noticed_at_ms"].as_u64().unwrap() >= 400, "{detail}");
}

/// A job that prints once and then nothing is stopped at the limit, as before.
#[test]
fn a_job_that_goes_quiet_is_stopped_at_its_limit() {
    let (outcome, detail, took) = run_sh("act_quiet", 400, "echo hi; sleep 30");
    assert_eq!(outcome, Outcome::Failed, "{detail}");
    assert_eq!(detail["timed_out"], true, "{detail}");
    assert!(detail["silent_ms"].as_u64().unwrap() >= 400, "{detail}");
    assert!(detail.get("ran_past_limit_ms").is_none(), "{detail}");
    assert!(took < Duration::from_secs(10), "{took:?}");
}

/// The window is measured from the LAST byte, not from the start.
///
/// The join review (w77) asked for this one, and it is the test that
/// discriminates the change: every other test here passes just as well if the
/// window is measured from `t0`. A job that is quiet, prints once at ~300 ms,
/// then goes quiet again against a 400 ms limit must be stopped at about
/// 700 ms, not at 400 ms - and must still be stopped, so this is not the
/// "runs on" case either. The lower bound is the real assertion; the upper one
/// is loose on purpose, because a loaded host may be late.
#[test]
fn the_silence_window_runs_from_the_last_byte_not_from_the_start() {
    let (outcome, detail, took) = run_sh("act_late", 400, "sleep 0.3; echo hi; sleep 30");
    assert_eq!(outcome, Outcome::Failed, "{detail}");
    assert_eq!(detail["timed_out"], true, "{detail}");
    assert!(detail["silent_ms"].as_u64().unwrap() >= 400, "{detail}");
    // From the start it would be ~400 ms; from the last byte, ~700 ms.
    assert!(took >= Duration::from_millis(650), "{took:?} {detail}");
    assert!(took < Duration::from_secs(10), "{took:?}");
}

/// A job that prints without end is stopped at the ceiling, never run on
/// for ever.
///
/// The limit is 250 ms rather than 100 ms on the join review's advice: the
/// script prints every 10 ms, so at a 100 ms limit a loaded host that loses
/// the scheduler for a tenth of a second delivers `Cut::Silent` instead of
/// `Cut::Ceiling` and reds the test. There is no flaky list to catch that.
/// 250 ms keeps the same assertions with 25 prints of margin. (Held in proportion on a busy
/// machine since: see the test's first lines, theseus-20.)
#[test]
fn a_job_that_never_stops_printing_is_stopped_at_the_ceiling() {
    // The limit is an input, held in proportion on a busy machine (theseus-20): under 64 busy threads a
    // stall of the script past 250 ms cut it as silent, 1768 ms in, and past 500 ms (twice what the
    // probe read) once in fifteen runs. So it is twice the factor, which is 2 on a quiet machine, where
    // the test then takes 12 s and not 6 s.
    let busy = 2 * theseus_tools::busy_factor();
    // It pauses with bash's own `read -t` on a pipe nobody writes to, not with a `sleep` of its own: a fork and
    // an exec every 10 ms for 24 limits is where a crowded machine stalls it for seconds.
    let (outcome, detail, took) = run_in(
        "bash",
        "act_ceiling",
        250 * busy,
        "exec 3<> <(:); while :; do echo line; read -t 0.01 -u 3; done",
    );
    assert_eq!(outcome, Outcome::Failed, "{detail}");
    assert_eq!(detail["timed_out"], true, "{detail}");
    assert_eq!(detail["ceiling_hit"], true, "{detail}");
    assert!(detail.get("silent_ms").is_none(), "{detail}");
    assert!(
        took >= Duration::from_millis(250 * busy * CEILING_FACTOR as u64),
        "{took:?}"
    );
}

/// The wrapper's sleep between two looks at the output is never longer than one look, whatever the limit: an
/// hour's limit is still looked at every second. (Its schedule alone, no clock: a sleep as long as the limit
/// was what let a byte that came early wait a whole limit to be seen.)
#[test]
fn the_wrapper_never_sleeps_longer_than_one_look_of_the_output() {
    use crate::patience::Patience;
    for limit_ms in [50, 400, 2_000, 3_600_000] {
        let p = Patience::silence(limit_ms, Instant::now(), || 0);
        assert!(
            p.look() <= Duration::from_secs(1),
            "{limit_ms}: {:?}",
            p.look()
        );
        assert!(
            p.sleep() <= p.look(),
            "a {limit_ms} ms limit: it sleeps {:?}, a look is {:?}",
            p.sleep(),
            p.look()
        );
    }
    let clock = Patience::clock(3_600_000, Instant::now());
    assert!(
        clock.sleep() > Duration::from_secs(3_000),
        "a limit on the clock needs no look of the output"
    );
}

/// The silence runs from the byte, not from the look that saw it: a job that prints once, half way to its
/// limit, is cut one limit after that byte and one look at most, never a whole limit later. The window is held
/// open by hand: the byte lands at a moment the test chooses, the loop is the wrapper's own (`check`, then
/// sleep as long as `sleep` says), and the cut is read against the byte's own instant. Before the look was
/// bounded the first look came a whole limit in, saw the byte there, and the cut came a limit after THAT: half
/// a limit late, 300 ms of 600.
#[test]
fn a_byte_printed_early_starts_the_silence_at_the_byte_not_at_the_next_look() {
    use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
    use std::sync::{Arc, Mutex};

    use crate::patience::Patience;

    let limit = Duration::from_millis(600);
    let bytes = Arc::new(AtomicU64::new(0));
    let counted = bytes.clone();
    let mut p = Patience::silence(600, Instant::now(), move || counted.load(SeqCst));
    let look = p.look();
    let printed: Arc<Mutex<Option<Instant>>> = Arc::default();
    let (when, put) = (printed.clone(), bytes);
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        *when.lock().unwrap() = Some(Instant::now());
        put.store(1, SeqCst);
    });
    let busy = theseus_tools::Busy::before();
    let cut = loop {
        if let Some(cut) = p.check() {
            break cut;
        }
        std::thread::sleep(p.sleep());
    };
    let seen_at = Instant::now();
    writer.join().unwrap();
    let byte = printed
        .lock()
        .unwrap()
        .expect("the byte was written before the cut");
    assert!(matches!(cut, crate::patience::Cut::Silent(_)), "{cut:?}");
    let quiet = seen_at - byte;
    assert!(
        quiet >= limit,
        "cut after {quiet:?} of silence, under its {limit:?}"
    );
    let late = quiet - limit;
    assert!(
        busy.keeps(late, 2 * look),
        "cut {late:?} past the limit counted from the byte; a look is {look:?}: {busy}"
    );
}
