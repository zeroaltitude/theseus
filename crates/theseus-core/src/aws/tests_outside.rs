//! Data-plane reads are outside text (AWS design §3.9, T1; C3 = 14c): an
//! object's text, log lines, and CloudTrail's events are marked on their
//! result as a fetched page's is, and hold nothing; an object written to a
//! file, and a control-plane read, are not marked.

use serde_json::json;
use theseus_tools::ToolCtx;

use super::tests::{board, core, layer, ledgered, plain_ctx, turn};
use super::tests_c3::{call_in, fake};
use crate::provider::Scripted;

/// Each data-plane read returns the marker, naming where its text came
/// from; a get to a file and a listing of buckets return none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_data_plane_read_is_marked_and_a_file_is_not() {
    let fake = fake();
    let aws = layer(&fake, board());
    for (name, input, url) in [
        (
            "aws.s3.get",
            json!({"path": "s3://example-bucket/notes/a.txt"}),
            "s3://example-bucket/notes/a.txt",
        ),
        (
            "aws.logs.tail",
            json!({"group": "/example/app-a"}),
            "logs:us-west-2:/example/app-a",
        ),
        (
            "aws.logs.query",
            json!({"query": "fields @message", "groups": ["/example/app-a"]}),
            "logs:us-west-2:query/q-1",
        ),
        (
            "aws.trail",
            json!({"execution": "exe_test"}),
            "cloudtrail:us-west-2:LookupEvents",
        ),
    ] {
        let (r, _, _) = call_in(&aws, name, input, plain_ctx()).await;
        let (_, marked) = r.expect(name);
        assert_eq!(marked.map(|m| m.url).as_deref(), Some(url), "{name}");
    }
    let dir = tempfile::tempdir().unwrap();
    let (r, _, _) = call_in(
        &aws,
        "aws.s3.get",
        json!({"path": "s3://example-bucket/notes/a.txt", "to": "a.txt"}),
        ToolCtx::for_tests(dir.path()),
    )
    .await;
    assert!(
        r.unwrap().1.is_none(),
        "a file's bytes never reach the context"
    );
}

/// Through the whole core: an object read as text is marked outside text on
/// its result (naming the object), and the next call that acts (a put) is
/// not held for it: it runs at its own posture; a session that only wrote an
/// object to a file read none.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_objects_text_is_marked_and_a_put_then_runs_at_its_posture() {
    let fake = fake();
    let dir = tempfile::tempdir().unwrap();
    let get = json!({"path": "s3://example-bucket/notes/a.txt"});
    let put = json!({"path": "s3://example-bucket/out/b.txt", "text": "done"});
    let core = core(
        &fake,
        dir.path(),
        vec![
            Scripted::tools("", &[("t_get", "aws_s3_get", get)]),
            Scripted::tools("", &[("t_put", "aws_s3_put", put)]),
            Scripted::text("The put ran."),
        ],
    );
    let r = turn(&core, "read the note, then write that it is done").await;
    let nodes = core.store.session_nodes(&r.session_id).unwrap();
    let first = crate::external::first(&nodes.into_iter().map(|(_, n)| n).collect::<Vec<_>>())
        .expect("the object's text is marked outside text");
    assert_eq!(
        (first.tool.as_str(), first.url.as_str()),
        ("aws.s3.get", "s3://example-bucket/notes/a.txt")
    );
    assert!(ledgered(&core, "session.external_read").is_empty());
    // The put is not held for the read.
    let puts = fake.seen().iter().filter(|s| s.method == "PUT").count();
    assert_eq!(puts, 1, "the put ran");
    assert!(r.awaiting_confirm.is_none(), "the put is not held");
    let asked = ledgered(&core, "tool.confirm_requested");
    assert!(asked.is_empty(), "{asked:?}");
}

/// A session that only wrote an object to a file read no outside text.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_object_written_to_a_file_is_not_outside_text() {
    let fake = fake();
    let dir = tempfile::tempdir().unwrap();
    let get = json!({"path": "s3://example-bucket/notes/a.txt", "to": "a.txt"});
    let core = core(
        &fake,
        dir.path(),
        vec![
            Scripted::tools("", &[("t_get", "aws_s3_get", get)]),
            Scripted::text("Saved."),
        ],
    );
    let r = turn(&core, "save the note").await;
    assert_eq!(r.tool_calls, 1);
    assert!(!crate::external::read_any(&core.store, &r.session_id));
    assert!(ledgered(&core, "session.external_read").is_empty());
}
