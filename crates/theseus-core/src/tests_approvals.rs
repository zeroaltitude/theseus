//! The approval flow across a batch and across responses: a declined call
//! ends its batch's waits (theseus-6i0), and a call is found by its own
//! response, whatever id an earlier response's call had (theseus-w6uh).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::{SessionKind, Span, TurnSubmitResult};

use crate::bus::EventSink;
use crate::node::{Body, ResultStatus};
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    /// The workspace's root, where reads are open.
    root: PathBuf,
    /// Outside the roots and on the approve list: a write here waits.
    guarded: PathBuf,
    _dir: tempfile::TempDir,
}

/// A core whose provider follows the script `script` writes for the
/// workspace's root and the guarded directory, at the template's postures,
/// with that one approve-listed directory outside the roots. Since
/// theseus-ewi a write outside the roots takes the tool's posture (notify,
/// here), so the approve list is what makes these writes wait.
fn rig(script: impl FnOnce(&Path, &Path) -> Vec<Scripted>) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    let guarded = dir.path().join("guarded");
    for d in [&root, &guarded] {
        std::fs::create_dir_all(d).unwrap();
    }
    let (root, guarded) = (
        root.canonicalize().unwrap(),
        guarded.canonicalize().unwrap(),
    );
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.tools.approve_paths = vec![guarded.to_string_lossy().into_owned()];
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script(&root, &guarded)));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    Rig {
        core,
        fake,
        root,
        guarded,
        _dir: dir,
    }
}

async fn turn(core: &Arc<Core>, session: Option<&str>, input: &str) -> TurnSubmitResult {
    let rec = match session {
        Some(id) => core
            .store
            .get_session::<SessionRecord>(id)
            .unwrap()
            .unwrap(),
        None => {
            let r = SessionRecord::new(SessionKind::Conversation, None);
            core.store.put_session(&r.session_id, &r).unwrap();
            r
        }
    };
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), &rec.session_id, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
        .unwrap()
}

/// Answer the question `res` parked on, and run the continuation.
async fn answer(
    r: &Rig,
    res: &TurnSubmitResult,
    approve: bool,
    note: Option<&str>,
) -> TurnSubmitResult {
    let corr = res.awaiting_confirm.as_deref().expect("it asks");
    r.core.confirm_action(corr, approve, note, "test").unwrap();
    let exec = res.execution_id.as_deref().unwrap();
    r.core.continue_execution(exec).await.unwrap().unwrap()
}

fn write(id: &str, path: &Path, content: &str) -> (String, String, Value) {
    (
        id.into(),
        "fs_write".into(),
        json!({"path": path, "content": content}),
    )
}

fn read(id: &str, path: &Path) -> (String, String, Value) {
    (id.into(), "fs_read".into(), json!({"path": path}))
}

fn tools(calls: &[(String, String, Value)]) -> Scripted {
    let calls: Vec<(&str, &str, Value)> = calls
        .iter()
        .map(|(id, name, input)| (id.as_str(), name.as_str(), input.clone()))
        .collect();
    Scripted::tools("", &calls)
}

/// The `tool_result` blocks of the model's last request, in order: each
/// call's id and text.
fn last_results(fake: &FakeProvider) -> Vec<(String, String)> {
    let req = fake.requests().pop().expect("a request");
    let user = req.messages.last().unwrap();
    user["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|b| b["type"] == "tool_result")
        .map(|b| {
            let text = match &b["content"] {
                Value::String(s) => s.clone(),
                v => v.to_string(),
            };
            (b["tool_use_id"].as_str().unwrap().to_string(), text)
        })
        .collect()
}

/// The session's results, in order: id, status, text.
fn results(core: &Core, sid: &str) -> Vec<(String, ResultStatus, String)> {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .filter_map(|(_, n)| match n.body {
            Body::ToolResult {
                tool_use_id,
                status,
                content,
                ..
            } => Some((tool_use_id, status, content)),
            _ => None,
        })
        .collect()
}

/// How many questions the session's calls asked (`tool.confirm_requested`).
fn asked(r: &Rig) -> usize {
    r.core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(10_000)
        .unwrap()
        .into_iter()
        .filter(|(_, row)| row.kind == "tool.confirm_requested")
        .count()
}

/// Every span of the trace whose name starts `tool `, with its parent's name.
fn tool_spans(root: &Span) -> Vec<(String, Span)> {
    fn walk(s: &Span, out: &mut Vec<(String, Span)>) {
        for c in &s.children {
            if c.name.starts_with("tool ") {
                out.push((s.name.clone(), c.clone()));
            }
            walk(c, out);
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

const NOT_ASKED: &str = "Not run: an earlier call in this batch was declined.";

/// theseus-6i0: one response holds a write that waits, a read, and a second
/// write that would wait. The first is declined with a note: the
/// continuation asks nothing more, the read runs, the second write is
/// answered not run, and the model hears all three at once.
#[tokio::test]
async fn a_declined_call_ends_its_batchs_waits() {
    let r = rig(|root, guarded| {
        vec![
            tools(&[
                write("w1", &guarded.join("a.txt"), "first\n"),
                read("r1", &root.join("note.txt")),
                write("w2", &guarded.join("b.txt"), "second\n"),
            ]),
            Scripted::text("Understood: neither file."),
        ]
    });
    std::fs::write(r.root.join("note.txt"), "a quiet harbour\n").unwrap();
    let res = turn(&r.core, None, "write both files").await;
    assert_eq!(asked(&r), 1);
    let cont = answer(&r, &res, false, Some("wrong place")).await;

    // No card for the second write: nothing asked, nothing waits.
    assert_eq!(cont.output, "Understood: neither file.");
    assert!(cont.awaiting_confirm.is_none(), "{cont:?}");
    assert_eq!(asked(&r), 1, "the second write was never asked");
    assert!(r.core.kernel.pending_confirms().unwrap().is_empty());
    assert!(r.core.pending_confirms(&res.session_id).unwrap().is_empty());
    assert!(!r.guarded.join("a.txt").exists());
    assert!(!r.guarded.join("b.txt").exists());

    // The model's next request: the decline with its note, the read's
    // content, and the not-run line, in call order.
    let got = last_results(&r.fake);
    let ids: Vec<&str> = got.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["w1", "r1", "w2"], "{got:?}");
    assert!(
        got[0].1.contains("declined") && got[0].1.contains("wrong place"),
        "{got:?}"
    );
    assert!(got[1].1.contains("a quiet harbour"), "{got:?}");
    assert_eq!(got[2].1, NOT_ASKED, "{got:?}");

    // The second write is `Cancelled`: nobody declined it. It is answered at
    // its admission, as an invalid input is, so it is stored before the read
    // it follows; the request puts every result in call order.
    let rs = results(&r.core, &res.session_id);
    let mut statuses: Vec<_> = rs.iter().map(|(id, s, _)| (id.as_str(), *s)).collect();
    statuses.sort_by_key(|(id, _)| ["w1", "r1", "w2"].iter().position(|c| c == id));
    assert_eq!(
        statuses,
        [
            ("w1", ResultStatus::Declined),
            ("r1", ResultStatus::Ok),
            ("w2", ResultStatus::Cancelled)
        ]
    );
    // It leaves its gate's record on a node of its own, and no action.
    let w2 = r
        .core
        .store
        .session_nodes(&res.session_id)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match n.body {
            Body::ToolCall {
                tool_use_id,
                correlation_id,
                gate,
                ..
            } if tool_use_id == "w2" => Some((correlation_id, gate)),
            _ => None,
        })
        .expect("its call's node");
    assert_eq!(w2.0, None, "never planned");
    assert_eq!(w2.1.expect("its gate record").result.gate, "needs_confirm");

    // Each call the continuation answered is one span under it, once.
    let trace = cont.trace.as_ref().expect("the continuation's trace");
    let spans = tool_spans(trace);
    let mut named: Vec<_> = spans
        .iter()
        .map(|(parent, s)| {
            (
                parent.as_str(),
                s.attrs["tool_use_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                s.attrs["result"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    // In the order they were answered: the not-run line first, at admission.
    named.sort_by_key(|(_, id, _)| ["w1", "r1", "w2"].iter().position(|c| c == id));
    assert_eq!(
        named,
        [
            ("continuation", "w1".into(), "declined".into()),
            ("continuation", "r1".into(), "ok".into()),
            ("continuation", "w2".into(), "cancelled".into()),
        ],
        "{trace:#?}"
    );
}

/// theseus-6i0's other side: the first write approved, the second still
/// asks, one card at a time as before, and runs once approved too.
#[tokio::test]
async fn an_approved_call_leaves_the_next_to_ask() {
    let r = rig(|root, guarded| {
        vec![
            tools(&[
                write("w1", &guarded.join("a.txt"), "first\n"),
                read("r1", &root.join("note.txt")),
                write("w2", &guarded.join("b.txt"), "second\n"),
            ]),
            Scripted::text("Both written."),
        ]
    });
    std::fs::write(r.root.join("note.txt"), "a quiet harbour\n").unwrap();
    let res = turn(&r.core, None, "write both files").await;
    let cont = answer(&r, &res, true, None).await;
    assert!(r.guarded.join("a.txt").exists());
    let second = cont
        .awaiting_confirm
        .clone()
        .expect("the second write asks");
    assert_ne!(Some(second.as_str()), res.awaiting_confirm.as_deref());
    assert_eq!(asked(&r), 2);
    assert!(!r.guarded.join("b.txt").exists());
    let done = answer(&r, &cont, true, None).await;
    assert_eq!(done.output, "Both written.");
    assert!(r.guarded.join("b.txt").exists());
    let got = last_results(&r.fake);
    let ids: Vec<&str> = got.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["w1", "r1", "w2"], "{got:?}");
    assert!(got[1].1.contains("a quiet harbour"), "{got:?}");
}

/// theseus-w6uh, the issue's case: a provider that numbers its calls per
/// response gives two responses' first calls one id. The second, waiting,
/// is approved: it runs, and the model reads its own result.
#[tokio::test]
async fn an_approved_call_whose_id_an_earlier_response_used_runs() {
    let r = rig(|root, guarded| {
        vec![
            tools(&[read("toolu_fake_0", &root.join("note.txt"))]),
            Scripted::text("Read it."),
            tools(&[write("toolu_fake_0", &guarded.join("c.txt"), "tide\n")]),
            Scripted::text("Written."),
        ]
    });
    std::fs::write(r.root.join("note.txt"), "a quiet harbour\n").unwrap();
    let first = turn(&r.core, None, "read the note").await;
    assert_eq!(first.output, "Read it.");
    let res = turn(&r.core, Some(&first.session_id), "now write c").await;
    let done = answer(&r, &res, true, None).await;
    assert_eq!(done.output, "Written.");
    assert!(r.guarded.join("c.txt").exists());
    let got = last_results(&r.fake);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].0, "toolu_fake_0");
    assert!(
        !got[0].1.contains("a quiet harbour"),
        "its own result: {got:?}"
    );
    let rs = results(&r.core, &first.session_id);
    assert_eq!(rs.len(), 2, "{rs:?}");
    assert_eq!(rs[1].1, ResultStatus::Ok, "{rs:?}");
}

/// Two responses, the first's two reads answered; the second holds a write
/// that waits, then a read whose id repeats the first response's second
/// call. `[root, guarded]`'s script.
fn repeated_ids(root: &Path, guarded: &Path) -> Vec<Scripted> {
    vec![
        tools(&[
            read("toolu_fake_0", &root.join("note.txt")),
            read("toolu_fake_1", &root.join("note.txt")),
        ]),
        Scripted::text("Read it twice."),
        tools(&[
            write("toolu_fake_0", &guarded.join("c.txt"), "tide\n"),
            read("toolu_fake_1", &root.join("note.txt")),
        ]),
        Scripted::text("Written, and read again."),
    ]
}

/// theseus-w6uh, the case main missed: the second response's read was never
/// planned, and its id is the first response's answered read. Approved, the
/// write runs, and the read runs anew: its result is the note as it is now,
/// not the first read's, nor "settled but lost".
#[tokio::test]
async fn a_never_planned_call_whose_id_an_earlier_response_used_runs_anew() {
    let r = rig(repeated_ids);
    std::fs::write(r.root.join("note.txt"), "a quiet harbour\n").unwrap();
    let first = turn(&r.core, None, "read the note twice").await;
    assert_eq!(first.output, "Read it twice.");
    std::fs::write(r.root.join("note.txt"), "a rising tide\n").unwrap();
    let res = turn(&r.core, Some(&first.session_id), "write c, then read").await;
    let done = answer(&r, &res, true, None).await;
    assert_eq!(done.output, "Written, and read again.");
    assert!(r.guarded.join("c.txt").exists());
    let got = last_results(&r.fake);
    let ids: Vec<&str> = got.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["toolu_fake_0", "toolu_fake_1"], "{got:?}");
    assert!(got[1].1.contains("a rising tide"), "{got:?}");
    let rs = results(&r.core, &first.session_id);
    let statuses: Vec<_> = rs.iter().map(|(_, s, _)| *s).collect();
    assert_eq!(statuses, [ResultStatus::Ok; 4], "{rs:?}");
}

/// theseus-w6uh's cancel: the same two responses, the execution cancelled
/// before the write's answer. The cancel answers the second response's two
/// calls, as not run, and touches nothing of the first's.
#[tokio::test]
async fn a_cancel_answers_the_last_responses_calls_whatever_their_ids() {
    let r = rig(repeated_ids);
    std::fs::write(r.root.join("note.txt"), "a quiet harbour\n").unwrap();
    let first = turn(&r.core, None, "read the note twice").await;
    let res = turn(&r.core, Some(&first.session_id), "write c, then read").await;
    let corr = res.awaiting_confirm.clone().expect("the write asks");
    let exec = res.execution_id.clone().unwrap();
    r.core.cancel_execution(&exec, "operator").await.unwrap();
    assert!(!r.guarded.join("c.txt").exists());
    let rs = results(&r.core, &first.session_id);
    assert_eq!(rs.len(), 4, "{rs:?}");
    for (id, status, text) in &rs[2..] {
        assert_eq!(*status, ResultStatus::Cancelled, "{id}: {rs:?}");
        assert!(text.starts_with("Not run: "), "{id}: {rs:?}");
    }
    assert_eq!(
        (rs[2].0.as_str(), rs[3].0.as_str()),
        ("toolu_fake_0", "toolu_fake_1")
    );
    let a = r.core.kernel.action(&corr).unwrap().unwrap();
    assert_eq!(a.state, theseus_kernel::ActionState::Cancelled);
    // The write's result names its own action.
    let corrs: Vec<_> = r
        .core
        .store
        .session_nodes(&first.session_id)
        .unwrap()
        .into_iter()
        .filter_map(|(_, n)| match n.body {
            Body::ToolResult { correlation_id, .. } => Some(correlation_id),
            _ => None,
        })
        .collect();
    assert_eq!(corrs[2].as_deref(), Some(corr.as_str()), "{corrs:?}");
    assert_eq!(corrs[3], None, "the read was never planned");
}
