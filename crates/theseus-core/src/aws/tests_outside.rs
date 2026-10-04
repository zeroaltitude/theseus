//! Data-plane reads hold their session (AWS design §3.9, T1; C3 = 14c): an
//! object's text, log lines, and CloudTrail's events are outside text, so
//! the result is marked as a fetched page's is, and the session's hold is
//! written with it; an object written to a file, and a control-plane read,
//! hold nothing.

use serde_json::json;
use theseus_tools::ToolCtx;

use super::tests::{board, core, layer, ledgered, plain_ctx, turn};
use super::tests_c3::{call_in, fake};
use crate::provider::Scripted;
use crate::session::SessionRecord;

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

fn held(core: &crate::Core, sid: &str) -> Option<theseus_protocol::ExternalText> {
    core.store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap()
        .external
}

/// Through the whole core: an object read as text holds the session, with
/// its `session.external_read` row naming the object, and the next call
/// that acts (a put) waits for the operator; a session that only wrote an
/// object to a file holds nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_objects_text_holds_the_session_and_a_put_then_waits() {
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
            Scripted::text("The put waits for you."),
        ],
    );
    let r = turn(&core, "read the note, then write that it is done").await;
    let h = held(&core, &r.session_id).expect("the session holds outside text");
    assert_eq!(
        (h.tool.as_str(), h.url.as_str()),
        ("aws.s3.get", "s3://example-bucket/notes/a.txt")
    );
    let read = ledgered(&core, "session.external_read");
    assert_eq!(read.len(), 1, "{read:?}");
    assert!(
        read[0]
            .to_string()
            .contains("s3://example-bucket/notes/a.txt"),
        "{}",
        read[0]
    );
    // The put asked, so nothing was put.
    let puts = fake.seen().iter().filter(|s| s.method == "PUT").count();
    assert_eq!(puts, 0, "the put waits for the operator");
    assert!(
        r.awaiting_confirm.is_some(),
        "the put waits for the operator"
    );
    let asked = ledgered(&core, "tool.confirm_requested");
    assert_eq!(asked.len(), 1, "{asked:?}");
    let reason = asked[0]["reason"].as_str().unwrap_or_default();
    assert!(
        reason.contains(
            "this session read external text (aws.s3.get s3://example-bucket/notes/a.txt"
        ),
        "{reason}"
    );
    let pending = core.pending_confirms(&r.session_id).unwrap();
    assert_eq!(
        pending[0].external_text.as_ref().unwrap().url,
        "s3://example-bucket/notes/a.txt"
    );
}

/// A session that only wrote an object to a file holds nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_object_written_to_a_file_holds_nothing() {
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
    assert!(held(&core, &r.session_id).is_none());
    assert!(ledgered(&core, "session.external_read").is_empty());
}
