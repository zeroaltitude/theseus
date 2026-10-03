//! Graduation through the whole core (M4 19c; design m4-boundaries §2.7): a
//! withheld result graduated to its place is admitted at the next compile, as
//! an append, its placeholder still paired; a job's process cannot graduate
//! (J1), and neither can an untrusted channel; and a graduation's refusals
//! write nothing.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::{Integrity, Readers, SessionKind, TurnSubmitResult};

use crate::approval::Surface;
use crate::bus::EventSink;
use crate::node::Body;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

/// The owner on Discord, and a second person who can view the channel.
const OWNER: u64 = 271_828_182_845_904_523;
const ALICE: u64 = 222_222_222_222_222_222;
const LAB: u64 = 314_159_265_358_979_323;

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

fn rig(script: Vec<Scripted>, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.labels.owner = Some(vec![format!("discord:{OWNER}")]);
    tweak(&mut cfg);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    std::fs::write(root.join("hello.txt"), "the vault code is 4417\n").unwrap();
    Rig {
        core,
        fake,
        _dir: dir,
    }
}

/// A session in `#lab`, which the owner and alice can view.
fn lab_session(core: &Core) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    core.outbox
        .bind_place(&format!("channel:{LAB}"), &r.session_id)
        .unwrap();
    core.place_viewers(LAB, Some("lab".into()), Some(vec![OWNER, ALICE]), None);
    r.session_id
}

async fn turn(core: &Arc<Core>, sid: &str, input: &str) -> TurnSubmitResult {
    let rec = core
        .store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    core.runner
        .run(TurnRequest {
            session: rec,
            input: Some(input.into()),
            target,
            sink: EventSink::new(core.bus.clone(), sid, None),
            author: format!("discord:{OWNER}"),
            recompile: None,
            attachments: vec![],
            arrived: None,
            config_wait_us: 0,
            reply_to: None,
            from_discord: true,
        })
        .await
        .unwrap()
}

fn read_hello() -> Vec<Scripted> {
    vec![
        Scripted::tools(
            "Reading it.",
            &[("t1", "fs_read", json!({"path": "hello.txt"}))],
        ),
        Scripted::text("Done."),
        Scripted::text("Now I can say it."),
    ]
}

fn ledgered(core: &Core, kind: &str) -> Vec<Value> {
    core.store
        .ledger_tail::<crate::ledger::LedgerRow>(10_000)
        .unwrap()
        .into_iter()
        .filter(|(_, row)| row.kind == kind)
        .map(|(_, row)| row.data)
        .collect()
}

/// The id of the session's tool result.
fn result_id(core: &Core, sid: &str) -> String {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .find(|(_, n)| matches!(n.body, Body::ToolResult { .. }))
        .map(|(_, n)| n.id)
        .unwrap()
}

/// The `tool_result` block that answers `id`, in a request's messages.
fn result_block(messages: &[Value], id: &str) -> Value {
    messages
        .iter()
        .flat_map(|m| m["content"].as_array().cloned().unwrap_or_default())
        .find(|b| b["type"] == "tool_result" && b["tool_use_id"] == id)
        .unwrap_or_else(|| panic!("no result for {id}: {messages:?}"))
}

/// One request over a real protocol connection, accepted as `client`.
async fn rpc_as(
    core: &Arc<Core>,
    client: crate::approval::Client,
    method: &str,
    params: Value,
) -> Result<Value, theseus_protocol::RpcError> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (ours, theirs) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(theirs);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, client));
    let (cr, mut cw) = tokio::io::split(ours);
    let req = theseus_protocol::Request::new(theseus_protocol::Id::Num(1), method, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    let out = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let theseus_protocol::Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break match (r.result, r.error) {
                (Some(v), _) => Ok(v),
                (None, e) => Err(e.unwrap()),
            };
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    out
}

/// The brief's first test: in `#lab`, which alice can view too, the owner's
/// file is a placeholder. Graduated to the place, with a reason, the next
/// turn's compile appends a new node that carries it, readable by the place,
/// trusted as its source was, with the warrant; the placeholder stays where
/// it was, its call still paired, and the source keeps its own label.
#[tokio::test]
async fn a_graduated_result_is_admitted_at_the_next_compile_and_its_placeholder_stays_paired() {
    let r = rig(read_hello(), |_| {});
    let sid = lab_session(&r.core);
    turn(&r.core, &sid, "read hello.txt").await;
    let trs = result_id(&r.core, &sid);
    let placeholder = result_block(&r.fake.requests()[1].messages, "t1");
    let line = placeholder["content"].as_str().unwrap().to_string();
    assert!(
        line.starts_with("[withheld: fs.read's result is labeled owner-only")
            && line.ends_with(&format!(
                "The operator can graduate it: theseus graduate {trs} --to place]"
            )),
        "the placeholder names the command: {line}"
    );

    let g = r
        .core
        .graduate(&trs, "place", "alice works on the vault too", "cli")
        .unwrap();
    assert!(g.covers, "the place's readers cover its audience: {g:?}");
    assert_eq!(g.readers, Readers::Place(format!("discord:{LAB}")));
    assert_eq!(g.warrant.graduated_from, trs);
    let rows = ledgered(&r.core, "label.graduated");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["graduated_from"], trs.as_str());
    assert_eq!(rows[0]["why"], "alice works on the vault too");

    turn(&r.core, &sid, "and now?").await;
    let reqs = r.fake.requests();
    let third = &reqs[2].messages;
    let text = serde_json::to_string(third).unwrap();
    assert!(
        text.contains("4417"),
        "the graduated node is admitted: {text}"
    );
    assert!(text.contains("[Graduated by the operator (cli): fs.read's result"));
    assert_eq!(
        result_block(third, "t1")["content"],
        placeholder["content"],
        "the placeholder stays where it was, paired"
    );
    let comps = r.core.store.session_compilations(&sid).unwrap();
    assert_eq!(comps.len(), 1, "an append, not a recompile: {comps:?}");

    let nodes = r.core.store.session_nodes(&sid).unwrap();
    let source = nodes.iter().find(|(_, n)| n.id == trs).unwrap();
    assert_eq!(
        source.1.label.as_ref().unwrap().readers,
        Readers::Owner,
        "the source keeps its own label"
    );
    let copy = nodes.iter().find(|(_, n)| n.id == g.node_id).unwrap();
    let l = copy.1.label.as_ref().unwrap();
    assert_eq!(l.readers, Readers::Place(format!("discord:{LAB}")));
    assert_eq!(l.integrity, Integrity::Trusted, "the source's integrity");
    assert_eq!(
        l.warrant.as_ref().unwrap().why,
        "alice works on the vault too"
    );
    assert_eq!(copy.1.origin, crate::node::Origin::Operator);
}

/// Graduation never touches integrity: a fetched page's result, untrusted,
/// stays untrusted when its copy is graduated; and a refusal writes nothing:
/// a call, readers that widen nothing, an empty reason, a node that is not
/// there.
#[tokio::test]
async fn a_graduation_keeps_integrity_and_its_refusals_write_nothing() {
    let r = rig(read_hello(), |_| {});
    let sid = lab_session(&r.core);
    turn(&r.core, &sid, "read hello.txt").await;
    let trs = result_id(&r.core, &sid);
    let call = r
        .core
        .store
        .session_nodes(&sid)
        .unwrap()
        .into_iter()
        .find(|(_, n)| matches!(n.body, Body::ToolCall { .. }))
        .map(|(_, n)| n.id)
        .unwrap();
    let before = r.core.store.stats().unwrap().frames_appended;
    for (node, to, why, says) in [
        (call.as_str(), "place", "w", "graduate its result"),
        (trs.as_str(), "owner", "w", "none of public, place"),
        (trs.as_str(), "place", "  ", "needs its reason"),
        ("trs_nope", "place", "w", "no node is named"),
        (trs.as_str(), "people:alice", "w", "not a Discord user id"),
    ] {
        let e = format!("{:#}", r.core.graduate(node, to, why, "cli").unwrap_err());
        assert!(e.contains(says), "{node} {to}: {e}");
    }
    assert_eq!(
        r.core.store.stats().unwrap().frames_appended,
        before,
        "a refused graduation writes nothing"
    );

    let mut source = r.core.store.get_node(&trs).unwrap().unwrap().1;
    let page = theseus_protocol::ExternalText {
        since_ms: 1,
        tool: "http.fetch".into(),
        url: "https://example.invalid/tides".into(),
        node_id: trs.clone(),
        from_session: None,
        via: None,
        query: None,
    };
    source.label = Some(theseus_protocol::Label::untrusted(page, Readers::Owner));
    let l = crate::labels::graduated(
        source.label.as_ref(),
        None,
        Readers::Public,
        theseus_protocol::Warrant {
            graduated_from: trs,
            who: "cli".into(),
            how: "cli".into(),
            why: "w".into(),
            at_ms: 1,
        },
    );
    assert_eq!(
        (l.integrity, l.source.as_ref().map(|s| s.url.as_str())),
        (Integrity::Untrusted, Some("https://example.invalid/tides"))
    );
}

/// J1's rule: a Theseus job's process cannot graduate. `label.graduate` from
/// a process under a live job wrapper is refused with the job, ledgered as
/// `approval.refused`, and nothing is graduated; the operator's own, from
/// outside every job, counts.
#[tokio::test]
async fn a_jobs_process_cannot_graduate() {
    let job = crate::peer::Standin::start("act_graduater");
    let r = rig(read_hello(), |_| {});
    let sid = lab_session(&r.core);
    turn(&r.core, &sid, "read hello.txt").await;
    let trs = result_id(&r.core, &sid);
    let params = json!({"node_id": trs, "to": "place", "why": "the lab may read it"});
    let client = crate::approval::Client::new("sock#7", Surface::Cli)
        .with_peer(crate::peer::Peer::process(job.child));
    let e = rpc_as(
        &r.core,
        client,
        theseus_protocol::method::LABEL_GRADUATE,
        params.clone(),
    )
    .await
    .expect_err("refused");
    assert_eq!(e.code, theseus_protocol::error_code::REFUSED, "{e:?}");
    assert!(
        e.message
            .contains("from a Theseus job's process (job act_graduater"),
        "{}",
        e.message
    );
    let refused = ledgered(&r.core, "approval.refused");
    assert_eq!(refused.len(), 1);
    assert_eq!(
        (refused[0]["act"].as_str(), refused[0]["node_id"].as_str()),
        (Some("label.graduate"), Some(trs.as_str()))
    );
    assert_eq!(refused[0]["from_job"], true);
    assert!(ledgered(&r.core, "label.graduated").is_empty());

    if crate::peer::tests_support::inside_a_job() {
        return;
    }
    let client = crate::approval::Client::new("sock#1", Surface::Cli)
        .with_peer(crate::peer::Peer::process(std::process::id()));
    rpc_as(
        &r.core,
        client,
        theseus_protocol::method::LABEL_GRADUATE,
        params,
    )
    .await
    .unwrap();
    assert_eq!(ledgered(&r.core, "label.graduated").len(), 1);
}

/// Under `[approval]`, graduation counts only through a trusted channel, as
/// an approval does: the web UI, unlisted, is refused, and nothing is
/// graduated; the CLI, listed, counts.
#[tokio::test]
async fn an_untrusted_channel_cannot_graduate() {
    let r = rig(read_hello(), |c| {
        c.approval = Some(crate::config::ApprovalConfig {
            trusted_users: vec![format!("discord:{OWNER}")],
            channels: vec!["cli".into()],
        });
    });
    let sid = lab_session(&r.core);
    turn(&r.core, &sid, "read hello.txt").await;
    let trs = result_id(&r.core, &sid);
    let web = crate::approval::Answerer {
        label: "web#3".into(),
        surface: Surface::Web,
        discord: None,
        peer: crate::peer::Peer::process(std::process::id()),
    };
    let e = r
        .core
        .graduate(&trs, "place", "the lab may read it", web)
        .unwrap_err();
    assert!(
        e.downcast_ref::<crate::approval::Refusal>().is_some(),
        "{e:#}"
    );
    assert!(ledgered(&r.core, "label.graduated").is_empty());
    assert_eq!(ledgered(&r.core, "approval.refused").len(), 1);
    r.core
        .graduate(&trs, "place", "the lab may read it", "cli")
        .unwrap();
    assert_eq!(ledgered(&r.core, "label.graduated").len(), 1);
}
