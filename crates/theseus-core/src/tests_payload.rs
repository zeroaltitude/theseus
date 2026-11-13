//! A large payload, flagged at ingest and compiled out to a marker
//! (theseus-czpp, row B4). The bar, end to end on a real job: a turn whose
//! tool output passes `[tools] large_payload_chars` compiles to a marker
//! line rather than the payload, the compiled context stays far under its
//! bound, and a later read of what the marker names returns the original
//! bytes unchanged.
//!
//! The rig is `tests_kept`'s, because a payload is kept by the same keep:
//! this file is its sibling and runs a real `bash` the same way.

use std::sync::Arc;

use serde_json::json;
use theseus_protocol::SessionKind;

use crate::node::Body;
use crate::payload::{Marked, PayloadKind};
use crate::policy::Posture;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::test_support::turn;
use crate::{Config, Core};

/// The threshold these tests set, well under the default so the command
/// stays quick: 40,000 characters, which the printer below passes and no
/// other result of the turn comes near.
const LINE: usize = 40_000;

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

fn rig(script: Vec<Scripted>, large: usize) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().canonicalize().unwrap();
    let root = state.join("work");
    std::fs::create_dir_all(&root).unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.proc_sync_secs = 10;
    cfg.tools.large_payload_chars = large;
    cfg.policy.enforcement = Posture::Open;
    cfg.policy.tools.insert("proc.run".into(), Posture::Open);
    let store = Store::open(&state.join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let p = crate::rpc::Parts::for_tests(cfg, fake.clone(), store);
    let core = Core::build(p).unwrap();
    Rig {
        core,
        fake,
        _dir: dir,
    }
}

fn session(core: &Core) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    r.session_id
}

/// A command that prints 1,200 lines of about 60 characters each: about
/// 74,000 characters, past [`LINE`] and past the 30,000-character result
/// cap, so its whole output is kept and it is a large payload.
fn printer() -> serde_json::Value {
    json!({"argv": ["bash", "-c",
        "for i in $(seq 1 1200); do echo \"line $i: a soundings row of the outer harbour survey\"; done"
    ]})
}

/// The payload on the session's one result, and the result's own text.
fn payload_of(core: &Core, sid: &str) -> (Option<Box<Marked>>, String) {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            Body::ToolResult {
                payload, content, ..
            } => Some((payload, content)),
            _ => None,
        })
        .expect("the session has a tool result")
}

/// Every `tool_result` block's content in the newest request the provider
/// was sent.
fn results_sent(r: &Rig) -> Vec<String> {
    let q = r.fake.requests().pop().expect("a request was sent");
    q.messages
        .iter()
        .filter_map(|m| m["content"].as_array())
        .flatten()
        .filter(|b| b["type"] == "tool_result")
        .map(|b| b["content"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// The whole bar in one run: the flag at ingest, the marker in the next
/// request in the payload's place, the context's size, and the bytes read
/// back unchanged.
#[tokio::test]
async fn a_large_output_compiles_to_its_marker_and_reads_back_unchanged() {
    let r = rig(
        vec![Scripted::text("Ran it."), Scripted::text("Done.")],
        LINE,
    );
    r.fake
        .script
        .lock()
        .unwrap()
        .push_front(Scripted::tools("", &[("t1", "proc_run", printer())]));
    let sid = session(&r.core);
    assert_eq!(
        turn(&r.core, &sid, "run the survey").await.output,
        "Ran it."
    );

    // 1. Flagged where it entered, with a guid and the description's parts.
    let (payload, content) = payload_of(&r.core, &sid);
    let p = payload.expect("the output passed the line, so it is a payload");
    assert!(p.guid.starts_with("pay_"), "{}", p.guid);
    assert_eq!(p.kind, PayloadKind::Program);
    assert_eq!(p.tool, "proc.run");
    assert_eq!(p.lines, 1_200, "every line of the output is counted");
    assert!(p.chars > LINE as u64, "{} characters", p.chars);
    assert!(p.first.starts_with("line 1:"), "{}", p.first);
    assert!(p.last.starts_with("line 1200:"), "{}", p.last);
    // The record still holds the capped text, as it always did: the marker
    // is what the compile renders, not what the store keeps.
    assert!(content.contains("line 1:"), "the node keeps its own text");

    // 2. The next request carries the marker, not the payload.
    assert_eq!(turn(&r.core, &sid, "and now?").await.output, "Done.");
    let sent = results_sent(&r);
    assert_eq!(sent.len(), 1, "one result in the request: {sent:?}");
    let block = &sent[0];
    assert_eq!(block, &p.line(), "the block is the marker, byte for byte");
    assert!(block.contains(&p.guid), "the marker carries the guid");
    assert!(
        block.contains("a program's whole output") && block.contains("Read it when you need"),
        "the marker carries the description: {block}"
    );
    assert!(
        !block.contains("line 600:"),
        "no line of the payload is in the request: {block}"
    );
    assert!(
        block.chars().count() < 1_000,
        "a 74,000-character output costs {} characters",
        block.chars().count()
    );

    // 3. The bytes are where the marker says, unchanged.
    let at = p.at.clone();
    let bytes = std::fs::read_to_string(&at).expect(&at);
    let expected: String = (1..=1200)
        .map(|i| format!("line {i}: a soundings row of the outer harbour survey\n"))
        .collect();
    assert_eq!(bytes, expected, "the kept bytes are what it printed");
    assert_eq!(
        bytes.chars().count() as u64,
        p.chars,
        "and the count is its"
    );
}

/// The positive control, which is also the planted revert: with the line at
/// 0 the same run is flagged as nothing and the same request carries the
/// capped text, lines of the payload and all. Without this a marker test
/// could pass on a run that produced no output at all.
#[tokio::test]
async fn with_the_line_off_the_same_output_is_carried_as_before() {
    let r = rig(vec![Scripted::text("Ran it."), Scripted::text("Done.")], 0);
    r.fake
        .script
        .lock()
        .unwrap()
        .push_front(Scripted::tools("", &[("t1", "proc_run", printer())]));
    let sid = session(&r.core);
    assert_eq!(
        turn(&r.core, &sid, "run the survey").await.output,
        "Ran it."
    );
    let (payload, content) = payload_of(&r.core, &sid);
    assert_eq!(payload, None, "nothing is a payload with the line off");
    assert!(content.contains("line 1:"), "it kept its own text");

    assert_eq!(turn(&r.core, &sid, "and now?").await.output, "Done.");
    let sent = results_sent(&r);
    assert_eq!(sent.len(), 1, "one result in the request: {sent:?}");
    assert!(
        !sent[0].contains("Large output held out"),
        "no marker: {}",
        &sent[0][..200.min(sent[0].len())]
    );
    assert!(sent[0].contains("line 1:"), "the payload's own lines");
    assert!(
        sent[0].chars().count() > 10_000,
        "and it costs the window {} characters",
        sent[0].chars().count()
    );
}
