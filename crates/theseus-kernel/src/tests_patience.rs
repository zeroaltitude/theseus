//! A job's limit is a bound on its silence, with a ceiling (theseus-x1zz8):
//! a job still printing past the limit runs on and is told; a quiet one is
//! stopped; a talkative one is stopped at the ceiling.

use std::time::{Duration, Instant};

use crate::job::{run_wrapper, WrapperArgs, DEFAULT_OUTPUT_MAX_BYTES};
use crate::patience::CEILING_FACTOR;
use crate::spool::Spool;
use crate::types::Outcome;

fn run_sh(id: &str, deadline_ms: u64, script: &str) -> (Outcome, serde_json::Value, Duration) {
    let d = tempfile::tempdir().unwrap();
    let spool = Spool::open(d.path()).unwrap();
    let args = WrapperArgs {
        spool_dir: d.path().to_path_buf(),
        correlation_id: id.into(),
        deadline_ms,
        notify_socket: None,
        argv: vec!["sh".into(), "-c".into(), script.into()],
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

/// A job that prints without end is stopped at the ceiling, never run on
/// for ever.
#[test]
fn a_job_that_never_stops_printing_is_stopped_at_the_ceiling() {
    let (outcome, detail, took) = run_sh(
        "act_ceiling",
        100,
        "while :; do echo line; sleep 0.01; done",
    );
    assert_eq!(outcome, Outcome::Failed, "{detail}");
    assert_eq!(detail["timed_out"], true, "{detail}");
    assert_eq!(detail["ceiling_hit"], true, "{detail}");
    assert!(detail.get("silent_ms").is_none(), "{detail}");
    assert!(
        took >= Duration::from_millis(100 * CEILING_FACTOR as u64),
        "{took:?}"
    );
}
