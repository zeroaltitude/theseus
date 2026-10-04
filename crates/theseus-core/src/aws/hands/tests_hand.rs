//! The `hand` role against a local fake of CloudWatch Logs, S3, and SQS:
//! the job runs with its input and without its key, its output reaches the
//! logs, its result and files reach its prefix, and its envelope reaches
//! the queue signed with its key; past its deadline it is stopped and
//! fails.

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_aws::Credentials;

use super::envelope::{derive_key, Envelope, HandSpec, VERSION};
use super::hand::{run, Creds};
use crate::aws::tests::{Fake, Reply, Seen};

/// What the fake answers: `{}` to a JSON call, an ETag to an S3 put, and
/// a message id to SQS.
pub(super) fn answer(s: &Seen, n: usize) -> Reply {
    let id = format!("req-{n}");
    let target = s.header("x-amz-target").unwrap_or_default().to_string();
    if target.ends_with(".SendMessage") {
        return (
            200,
            vec![
                ("x-amzn-requestid", id),
                ("content-type", "application/x-amz-json-1.0".into()),
            ],
            json!({"MessageId": format!("m-{n}")}).to_string(),
        );
    }
    if !target.is_empty() {
        return (
            200,
            vec![
                ("x-amzn-requestid", id),
                ("content-type", "application/x-amz-json-1.1".into()),
            ],
            "{}".into(),
        );
    }
    (
        200,
        vec![("x-amz-request-id", id), ("etag", "\"e\"".into())],
        String::new(),
    )
}

/// The secret the tests derive keys from.
const SECRET: &str = "test-secret-not-a-key-0001";

pub(super) fn spec(url: &str, corr: &str, argv: &[&str], deadline_secs: u64) -> HandSpec {
    HandSpec {
        v: VERSION,
        correlation_id: corr.into(),
        group: "act_group_example".into(),
        index: 2,
        input: json!("alpha"),
        argv: argv.iter().map(|s| s.to_string()).collect(),
        deadline_secs,
        region: "us-west-2".into(),
        bucket: "example-bucket".into(),
        queue_url: format!("{url}/111122223333/theseus-completions"),
        log_group: "/theseus/hands".into(),
        backend: "lambda".into(),
        key: hex::encode(*derive_key(SECRET.as_bytes(), corr)),
        endpoint: Some(url.into()),
    }
}

fn creds() -> Creds {
    Creds::fixed(Credentials::new(
        "AKIDHANDEXAMPLE",
        "hand-secret-example",
        None,
        None,
    ))
}

fn work_dir() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    // A process of its own per test (nextest), so this is the test's alone.
    std::env::set_var("THESEUS_HAND_DIR", d.path());
    d
}

/// The S3 puts the fake saw: each key under the bucket, and its body.
fn puts(seen: &[Seen]) -> Vec<(String, String)> {
    seen.iter()
        .filter(|s| s.method == "PUT")
        .map(|s| {
            let key = s.target.trim_start_matches("/example-bucket/").to_string();
            (key, s.body.clone())
        })
        .collect()
}

fn sent(seen: &[Seen], op: &str) -> Vec<Value> {
    seen.iter()
        .filter(|s| s.header("x-amz-target").is_some_and(|t| t.ends_with(op)))
        .map(|s| serde_json::from_str(&s.body).unwrap())
        .collect()
}

/// A hand's whole path: its job runs in its own directory with its index
/// and input, never its spec; its lines reach its log stream; its file
/// under `out/`, its output, and `result.json` reach its prefix; and its
/// envelope on the queue checks with the key derived for its correlation
/// id, and with no other.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hand_runs_its_job_uploads_its_result_and_signs_its_envelope() {
    let _dir = work_dir();
    let fake = Fake::start(answer);
    std::env::set_var(super::hand::SPEC_ENV, "the job must not see this");
    let s = spec(
        &fake.url,
        "act_hand_example_1",
        &[
            "sh",
            "-c",
            "echo hello {index} {input}; printf '%s' \"$THESEUS_HAND_INPUT\" > out/in.txt; \
             mkdir -p out/sub && echo deep > out/sub/b.txt; test -z \"$THESEUS_HAND\"",
        ],
        60,
    );
    let e = run(&s, Some("req-lambda-1".into()), &creds())
        .await
        .unwrap();
    assert_eq!(
        (e.outcome.as_str(), e.exit_code),
        ("succeeded", Some(0)),
        "{e:?}"
    );
    assert_eq!(e.external_op_id.as_deref(), Some("req-lambda-1"));
    assert_eq!(e.producer, "hand:lambda");
    assert_eq!(
        e.result_ref.as_deref(),
        Some("s3://example-bucket/hands/act_hand_example_1/")
    );
    assert!(e.tail.contains("hello 2 alpha"), "{}", e.tail);
    let key = derive_key(SECRET.as_bytes(), "act_hand_example_1");
    assert!(e.verify(key.as_ref()));

    let seen = fake.seen();
    let puts = puts(&seen);
    let keys: Vec<&str> = puts.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        keys,
        [
            "hands/act_hand_example_1/out/in.txt",
            "hands/act_hand_example_1/out/sub/b.txt",
            "hands/act_hand_example_1/output.txt",
            "hands/act_hand_example_1/result.json",
        ]
    );
    assert_eq!(puts[0].1, "alpha");
    let result: Value = serde_json::from_str(&puts[3].1).unwrap();
    assert_eq!(result["exit_code"], 0);
    assert_eq!(result["files"].as_array().unwrap().len(), 2, "{result}");

    let logs = sent(&seen, ".PutLogEvents");
    assert!(!logs.is_empty());
    assert_eq!(logs[0]["logStreamName"], "hands/act_group_example/2");
    assert!(logs[0]["logEvents"][0]["message"]
        .as_str()
        .unwrap()
        .contains("hello 2 alpha"));
    assert_eq!(sent(&seen, ".CreateLogStream").len(), 1);

    let msgs = sent(&seen, ".SendMessage");
    assert_eq!(msgs.len(), 1);
    assert!(msgs[0]["QueueUrl"]
        .as_str()
        .unwrap()
        .ends_with("/theseus-completions"));
    let on_queue: Envelope =
        serde_json::from_str(msgs[0]["MessageBody"].as_str().unwrap()).unwrap();
    assert_eq!(on_queue, e);
    assert!(on_queue.verify(key.as_ref()));
    assert!(!on_queue.verify(derive_key(SECRET.as_bytes(), "act_hand_example_2").as_ref()));
    // Every request names its call to AWS.
    for r in &seen {
        let ua = r.header("user-agent").unwrap_or_default();
        assert!(ua.contains("call/act_hand_example_1"), "{ua}");
    }
}

/// A job that fails is a failed hand with its exit code, signed all the
/// same; one past its deadline is stopped, with what it started, and fails
/// as timed out, long before its own end.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failing_or_overdue_job_is_a_failed_hand() {
    let _dir = work_dir();
    let fake = Fake::start(answer);
    let s = spec(
        &fake.url,
        "act_hand_fail",
        &["sh", "-c", "echo no; exit 3"],
        60,
    );
    let e = run(&s, None, &creds()).await.unwrap();
    assert_eq!(
        (e.outcome.as_str(), e.exit_code, e.timed_out),
        ("failed", Some(3), false)
    );
    assert!(e.verify(derive_key(SECRET.as_bytes(), "act_hand_fail").as_ref()));

    let s = spec(
        &fake.url,
        "act_hand_slow",
        &["sh", "-c", "sleep 300 & sleep 300; echo never"],
        1,
    );
    let t0 = Instant::now();
    let e = run(&s, None, &creds()).await.unwrap();
    assert!(t0.elapsed() < Duration::from_secs(30), "{:?}", t0.elapsed());
    assert_eq!((e.outcome.as_str(), e.timed_out), ("failed", true), "{e:?}");
    assert!(!e.tail.contains("never"));

    let s = spec(&fake.url, "act_hand_absent", &["/nonexistent/program"], 60);
    let e = run(&s, None, &creds()).await.unwrap();
    assert_eq!((e.outcome.as_str(), e.exit_code), ("failed", None));
    assert!(
        e.note
            .as_deref()
            .unwrap_or_default()
            .contains("could not start"),
        "{e:?}"
    );
}

/// A queue that does not answer: no envelope, and the hand says so (its
/// Lambda invocation then fails into the queue's failure destination).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hand_whose_envelope_is_not_sent_says_so() {
    let _dir = work_dir();
    let fake = Fake::start(|s, n| {
        if s.header("x-amz-target")
            .is_some_and(|t| t.ends_with(".SendMessage"))
        {
            return (
                400,
                vec![("x-amzn-requestid", format!("req-{n}"))],
                json!({"__type": "com.amazonaws.sqs#QueueDoesNotExist", "message": "no queue"})
                    .to_string(),
            );
        }
        answer(s, n)
    });
    let s = spec(&fake.url, "act_hand_noqueue", &["true"], 60);
    let err = run(&s, None, &creds()).await.unwrap_err();
    assert!(err.contains("the envelope was not sent"), "{err}");
}
