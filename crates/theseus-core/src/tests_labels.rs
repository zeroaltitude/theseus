//! Confidentiality labels through the whole core (M4 19a; design
//! m4-boundaries §2.5 and §2.7): each node is labeled as it is written; a
//! session in a guild channel that someone besides the owner can view is
//! given an owner-only result as a placeholder that keeps its call paired; a
//! DM with the owner and the CLI admit everything; a change of who can view
//! the channel recompiles; a context file the audience may not read is its
//! header and why; and a store from before labels compiles as it did.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::{Audience, Integrity, Readers, SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::compiler::{compile, Compilation, CompileInput, RequestSpec};
use crate::labels::Judge;
use crate::node::{Body, Node};
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};
use theseus_store::Store as _;

/// The owner on Discord, and a second person who can view a channel.
const OWNER: u64 = 271_828_182_845_904_523;
const ALICE: u64 = 222_222_222_222_222_222;
/// A guild channel the bindings file binds.
const LAB: u64 = 314_159_265_358_979_323;

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn rig(script: Vec<Scripted>, tweak: impl FnOnce(&mut Config, &std::path::Path)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.labels.owner = Some(vec![format!("discord:{OWNER}")]);
    tweak(&mut cfg, &root);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    Rig {
        core,
        fake,
        root,
        _dir: dir,
    }
}

/// A session, posting to `place` (`channel:<id>`, `dm:<user>`) when given.
fn session(core: &Core, place: Option<&str>) -> String {
    let r = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&r.session_id, &r).unwrap();
    if let Some(p) = place {
        core.outbox.bind_place(p, &r.session_id).unwrap();
    }
    r.session_id
}

/// A turn, typed through Discord or not.
async fn turn(core: &Arc<Core>, sid: &str, input: &str, from_discord: bool) -> TurnSubmitResult {
    let rec = core
        .store
        .get_session::<SessionRecord>(sid)
        .unwrap()
        .unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), sid, None);
    core.runner
        .run(TurnRequest {
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: if from_discord {
                format!("discord:{OWNER}")
            } else {
                "test".into()
            },
            recompile: None,
            attachments: vec![],
            arrived: None,
            config_wait_us: 0,
            reply_to: None,
            from_discord,
        })
        .await
        .unwrap()
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

fn people(ids: &[u64]) -> BTreeSet<String> {
    ids.iter().map(|u| format!("discord:{u}")).collect()
}

/// Each node's kind and readers, in order.
fn labels(core: &Core, sid: &str) -> Vec<(String, Option<Readers>)> {
    core.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .map(|(_, n)| (n.kind_str().to_string(), n.label.map(|l| l.readers)))
        .collect()
}

/// The `tool_result` block that answers `id`, in a request's messages.
fn result_block(messages: &[Value], id: &str) -> Value {
    messages
        .iter()
        .flat_map(|m| m["content"].as_array().cloned().unwrap_or_default())
        .find(|b| b["type"] == "tool_result" && b["tool_use_id"] == id)
        .unwrap_or_else(|| panic!("no result for {id}: {messages:?}"))
}

/// Whether an assistant message in the request still calls `id`.
fn calls(messages: &[Value], id: &str) -> bool {
    messages
        .iter()
        .filter(|m| m["role"] == "assistant")
        .flat_map(|m| m["content"].as_array().cloned().unwrap_or_default())
        .any(|b| b["type"] == "tool_use" && b["id"] == id)
}

fn read_hello() -> Vec<Scripted> {
    vec![
        Scripted::tools(
            "Reading it.",
            &[("t1", "fs_read", json!({"path": "hello.txt"}))],
        ),
        Scripted::text("Done."),
    ]
}

/// The brief's first test: in a channel two people can view, the owner's
/// file's result is a placeholder that names why, its call still paired,
/// while what was said in the channel goes in whole. The model's answers are
/// read by the channel; the result by the owner alone.
#[tokio::test]
async fn a_two_viewer_channel_withholds_an_owner_only_result_and_keeps_its_call_paired() {
    let r = rig(read_hello(), |_, _| {});
    std::fs::write(r.root.join("hello.txt"), "the vault code is 4417\n").unwrap();
    let sid = session(&r.core, Some(&format!("channel:{LAB}")));
    r.core
        .place_viewers(LAB, Some("lab".into()), Some(vec![OWNER, ALICE]), None);
    let res = turn(&r.core, &sid, "read hello.txt", true).await;
    assert_eq!(res.loops, 2, "{res:?}");

    let reqs = r.fake.requests();
    let second = &reqs[1].messages;
    assert!(calls(second, "t1"), "the call is still there: {second:?}");
    let block = result_block(second, "t1");
    let text = block["content"].as_str().unwrap();
    assert_eq!(
        text,
        "[withheld: fs.read's result is labeled owner-only, and this session's audience is \
         #lab (2 people)]"
    );
    assert!(!serde_json::to_string(second).unwrap().contains("4417"));
    assert!(serde_json::to_string(&reqs[0].messages)
        .unwrap()
        .contains("read hello.txt"));

    let place = Readers::Place(format!("discord:{LAB}"));
    assert_eq!(
        labels(&r.core, &sid),
        [
            ("user_message".to_string(), Some(place.clone())),
            ("assistant_message".to_string(), Some(place.clone())),
            ("tool_call".to_string(), Some(place.clone())),
            ("tool_result".to_string(), Some(Readers::Owner)),
            ("assistant_message".to_string(), Some(place)),
        ],
        "the second answer drew on the channel alone"
    );
    let compiled = ledgered(&r.core, "context.compiled");
    assert_eq!(compiled[1]["withheld"], 1, "{compiled:?}");
    assert_eq!(compiled[1]["audience"]["viewers"], 2);
    assert!(
        compiled[0].get("withheld").is_none(),
        "nothing withheld at first"
    );
    let row = &ledgered(&r.core, "label.withheld")[0];
    assert_eq!(row["reasons"]["owner-only"]["nodes"], 1, "{row}");
    assert_eq!(ledgered(&r.core, "label.audience")[0]["viewers"], 2);
}

/// A DM with the owner, and the CLI, admit everything: the owner is their
/// whole audience. The DM's manifest names its person.
#[tokio::test]
async fn a_dm_with_the_owner_and_the_cli_admit_everything() {
    let mut script = read_hello();
    script.extend(read_hello());
    let r = rig(script, |_, _| {});
    std::fs::write(r.root.join("hello.txt"), "the vault code is 4417\n").unwrap();
    let dm = session(&r.core, Some(&format!("dm:{OWNER}")));
    turn(&r.core, &dm, "read hello.txt", true).await;
    let cli = session(&r.core, None);
    turn(&r.core, &cli, "read hello.txt", false).await;
    let reqs = r.fake.requests();
    for i in [1, 3] {
        let block = result_block(&reqs[i].messages, "t1");
        assert!(
            block["content"].as_str().unwrap().contains("4417"),
            "{block}"
        );
    }
    assert!(ledgered(&r.core, "label.withheld").is_empty());
    let manifest = |sid: &str| {
        r.core.store.session_compilations(sid).unwrap()[0]
            .manifest
            .clone()
    };
    let m = manifest(&dm);
    assert_eq!(
        m.audience,
        Some(Audience::People {
            people: people(&[OWNER])
        })
    );
    assert!(m.withheld.is_empty());
    assert_eq!(
        m.readers,
        Some(Readers::People(people(&[OWNER]))),
        "the prefix held the DM's message alone"
    );
    assert_eq!(manifest(&cli).audience, Some(Audience::Owner));
    // The CLI's words are the owner's; the DM's are its person's.
    assert_eq!(labels(&r.core, &cli)[0].1, Some(Readers::Owner));
    assert_eq!(
        labels(&r.core, &dm)[0].1,
        Some(Readers::People(people(&[OWNER])))
    );
}

/// A channel the owner alone can view admits the owner's file; once a
/// second person can view it, the next turn recompiles (`audience`), and
/// the result in the prefix becomes its placeholder, named in the manifest;
/// once she cannot, it recompiles again and the result is back.
#[tokio::test]
async fn an_audience_change_recompiles_and_withholds_what_the_prefix_held() {
    let mut script = read_hello();
    script.push(Scripted::text("Hello again."));
    script.push(Scripted::text("And again."));
    let r = rig(script, |_, _| {});
    std::fs::write(r.root.join("hello.txt"), "the vault code is 4417\n").unwrap();
    let sid = session(&r.core, Some(&format!("channel:{LAB}")));
    r.core
        .place_viewers(LAB, Some("lab".into()), Some(vec![OWNER]), None);
    turn(&r.core, &sid, "read hello.txt", true).await;
    let first = r.fake.requests();
    assert!(result_block(&first[1].messages, "t1")["content"]
        .as_str()
        .unwrap()
        .contains("4417"));

    r.core
        .place_viewers(LAB, Some("lab".into()), Some(vec![OWNER, ALICE]), None);
    turn(&r.core, &sid, "hello", true).await;
    let comps = r.core.store.session_compilations(&sid).unwrap();
    assert_eq!(comps.len(), 2, "{comps:?}");
    let now = &comps[1];
    assert_eq!(now.trigger, "audience");
    assert!(now.manifest.strip_thinking, "a change beneath the thinking");
    // The result, and the answer that drew on it: both the owner's alone.
    let nodes = r.core.store.session_nodes(&sid).unwrap();
    let owners: Vec<(String, String)> = nodes
        .iter()
        .filter(|(_, n)| {
            n.label
                .as_ref()
                .is_some_and(|l| l.readers == Readers::Owner)
        })
        .map(|(_, n)| (n.id.clone(), "owner-only".to_string()))
        .collect();
    assert_eq!(owners.len(), 2, "{nodes:?}");
    assert!(matches!(nodes[3].1.body, Body::ToolResult { .. }));
    assert!(matches!(nodes[4].1.body, Body::AssistantMessage { .. }));
    let withheld: Vec<(String, String)> = now
        .manifest
        .withheld
        .iter()
        .map(|w| (w.node_id.clone(), w.reason.clone()))
        .collect();
    assert_eq!(withheld, owners);
    let reqs = r.fake.requests();
    let block = result_block(&reqs[2].messages, "t1");
    assert!(block["content"].as_str().unwrap().starts_with("[withheld:"));
    assert!(calls(&reqs[2].messages, "t1"));

    r.core
        .place_viewers(LAB, Some("lab".into()), Some(vec![OWNER]), None);
    turn(&r.core, &sid, "again", true).await;
    let comps = r.core.store.session_compilations(&sid).unwrap();
    assert_eq!(comps.len(), 3);
    assert_eq!(comps[2].trigger, "audience");
    assert!(comps[2].manifest.withheld.is_empty());
    let reqs = r.fake.requests();
    assert!(result_block(&reqs[3].messages, "t1")["content"]
        .as_str()
        .unwrap()
        .contains("4417"));
}

/// A context file is the owner's unless its entry says it is public: in a
/// channel two people can view, the owner's file is its header and why, and
/// the manifest records it as withheld; the public one goes in whole.
#[tokio::test]
async fn a_context_file_the_audience_may_not_read_is_withheld_with_its_reason() {
    let (mut own, mut open) = (String::new(), String::new());
    let r = rig(vec![Scripted::text("Hi.")], |cfg, root| {
        own = root.join("NOTES.md").to_string_lossy().into_owned();
        open = root.join("README.md").to_string_lossy().into_owned();
        cfg.context.files = vec![
            own.clone().into(),
            crate::context_files::ContextEntry::Table(crate::context_files::ContextFileEntry {
                path: open.clone(),
                readers: crate::context_files::ContextReaders::Public,
            }),
        ];
    });
    std::fs::write(&own, "the vault code is 4417\n").unwrap();
    std::fs::write(&open, "a public readme\n").unwrap();
    let sid = session(&r.core, Some(&format!("channel:{LAB}")));
    r.core
        .place_viewers(LAB, Some("lab".into()), Some(vec![OWNER, ALICE]), None);
    turn(&r.core, &sid, "hi", true).await;
    let context = r.fake.requests()[0].system[1]["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        context.starts_with(&format!(
            "# Context file (system): {own} — withheld: owner-only, and this session's audience \
             is #lab (2 people)"
        )),
        "{context}"
    );
    assert!(!context.contains("4417"));
    assert!(context.contains("a public readme"));
    let m = &r.core.store.session_compilations(&sid).unwrap()[0].manifest;
    assert_eq!(m.context_files[0].withheld.as_deref(), Some("owner-only"));
    assert_eq!(m.context_files[0].digest, None);
    assert_eq!(m.context_files[1].withheld, None);
    assert_eq!(m.context_files[1].readers, Some(Readers::Public));
    assert_eq!(ledgered(&r.core, "context.compiled")[0]["withheld"], 1);
}

/// A guild channel whose viewers cannot be read counts as public: its own
/// words go in, the owner's do not, and the manifest says why.
#[tokio::test]
async fn a_channel_whose_viewers_cannot_be_read_counts_as_public() {
    let r = rig(read_hello(), |_, _| {});
    std::fs::write(r.root.join("hello.txt"), "the vault code is 4417\n").unwrap();
    let sid = session(&r.core, Some(&format!("channel:{LAB}")));
    r.core.place_viewers(
        LAB,
        Some("lab".into()),
        None,
        Some("the bot's Server Members intent is off"),
    );
    turn(&r.core, &sid, "read hello.txt", true).await;
    let reqs = r.fake.requests();
    let block = result_block(&reqs[1].messages, "t1");
    assert_eq!(
        block["content"],
        "[withheld: fs.read's result is labeled owner-only, and this session's audience is \
         #lab (public: its viewers cannot be read)]"
    );
    assert!(serde_json::to_string(&reqs[0].messages)
        .unwrap()
        .contains("read hello.txt"));
    let m = &r.core.store.session_compilations(&sid).unwrap()[0].manifest;
    assert_eq!(
        m.audience,
        Some(Audience::Place {
            place: format!("discord:{LAB}"),
            name: Some("lab".into()),
            viewers: None,
            digest: None,
        })
    );
    let row = &ledgered(&r.core, "label.audience")[0];
    assert_eq!(row["why"], "the bot's Server Members intent is off");
}

/// A file inside a `[labels] public_paths` tree is anyone's: the channel's
/// session reads it whole.
#[tokio::test]
async fn a_file_in_a_public_tree_is_anyones() {
    let r = rig(read_hello(), |cfg, root| {
        cfg.labels.public_paths = vec![root.to_string_lossy().into_owned()];
    });
    std::fs::write(r.root.join("hello.txt"), "a public line\n").unwrap();
    let sid = session(&r.core, Some(&format!("channel:{LAB}")));
    r.core
        .place_viewers(LAB, Some("lab".into()), Some(vec![OWNER, ALICE]), None);
    turn(&r.core, &sid, "read hello.txt", true).await;
    let block = result_block(&r.fake.requests()[1].messages, "t1");
    assert!(block["content"].as_str().unwrap().contains("a public line"));
    let l = &labels(&r.core, &sid)[3];
    assert_eq!(l.1, Some(Readers::Public));
}

/// The spec a compile of the fixture's session renders with.
fn spec() -> RequestSpec {
    RequestSpec {
        profile: "p".into(),
        provider: "anthropic".into(),
        model: "claude-sonnet-5-5".into(),
        max_tokens: 1000,
        system_text: "You are Theseus.".into(),
        context_text: String::new(),
        context_files: vec![],
        persona: None,
        tools: vec![
            json!({"name": "proc_run", "description": "d", "input_schema": {"type": "object"}}),
        ],
        effort: None,
        thinking_display: crate::config::ThinkingDisplay::Summarized,
        refusal_fallbacks: false,
        first_party: true,
        cache_ttl: crate::config::CacheTtl::FiveMinutes,
        conversation_ttl: crate::config::CacheTtl::FiveMinutes,
    }
}

/// F4a's old-store fixture compiles exactly as before: its nodes have no
/// labels, and are read in their own session whatever its audience, so a
/// compile with a judge, for the owner, a DM, or a public channel, renders
/// the same request as one without, and its compilation from before 19a
/// keeps appending (no `audience` trigger).
#[test]
fn the_old_store_fixture_compiles_exactly_as_before() {
    let d = crate::store::tests::older_store();
    let store = Store::open(d.path()).unwrap();
    let comps: Vec<Compilation> = store
        .inner()
        .latest_of_kind(theseus_store::kinds::COMPILATION)
        .unwrap()
        .iter()
        .map(|r| r.decode().unwrap())
        .collect();
    assert_eq!(comps.len(), 1, "the fixture's one compilation");
    let mut current = comps[0].clone();
    let sid = current.session_id.clone();
    let nodes = store.transcript(&sid).unwrap();
    assert!(
        nodes.iter().all(|(_, n)| n.label.is_none()),
        "written before labels"
    );
    let sp = spec();
    let catalog = crate::catalog::Catalog::builtin();
    // As if nothing else changed: the stored manifest names this spec.
    let now = crate::compiler::manifest_for(&sp, &catalog, None, false);
    current.manifest.provider.clone_from(&now.provider);
    current.manifest.model.clone_from(&now.model);
    current
        .manifest
        .system_digest
        .clone_from(&now.system_digest);
    current.manifest.tools_digest.clone_from(&now.tools_digest);
    let places = crate::labels::Places::default();
    let channel = Judge::new(
        Some("discord:channel:314159265358979323"),
        BTreeSet::new(),
        &places,
        &store,
        false,
    );
    let dm = Judge::new(
        Some("discord:dm:222222222222222222"),
        BTreeSet::new(),
        &places,
        &store,
        false,
    );
    let owner = Judge::owner_only();
    let run = |current: Option<&Compilation>, judge: Option<&Judge>| {
        compile(CompileInput {
            session_id: &sid,
            current,
            nodes: &nodes,
            last_position: store.last_position(),
            spec: &sp,
            catalog: &catalog,
            force: None,
            window_override: None,
            blobs: None,
            hidden: &[],
            strip: None,
            overflowed: None,
            judge,
        })
    };
    for current in [Some(&current), None] {
        let before = run(current, None);
        for judge in [&owner, &dm, &channel] {
            let after = run(current, Some(judge));
            assert_eq!(after.digest, before.digest, "{:?}", judge.audience);
            assert_eq!(after.decision(), before.decision());
            assert_eq!(after.trigger, before.trigger);
            assert_eq!(after.withheld, 0);
        }
    }
    assert_eq!(run(Some(&current), Some(&channel)).decision(), "append");
}

/// A node from before labels is read in its own session, whoever its
/// audience is, and adds the session's own readers to the meet; the same
/// node labeled the owner's is not read by a channel that counts as public.
/// What a job returns is the owner's in 19a, at L0 and L1 alike.
#[test]
fn a_node_from_before_labels_is_read_only_in_its_own_session() {
    use crate::labels::Verdict;
    let d = tempfile::tempdir().unwrap();
    let store = Store::open(d.path()).unwrap();
    let places = crate::labels::Places::default();
    let public = Judge::new(
        Some("discord:channel:314159265358979323"),
        BTreeSet::new(),
        &places,
        &store,
        false,
    );
    let n = Node::user("ses_a", None, "cli", "hello");
    assert_eq!(
        public.verdict(&n),
        Verdict::Admit(&Readers::Place("discord:314159265358979323".into()))
    );
    let owners = n.labeled(crate::labels::for_input(false, None));
    assert_eq!(owners.label.as_ref().unwrap().integrity, Integrity::Trusted);
    assert_eq!(
        public.verdict(&owners),
        Verdict::Withhold("owner-only".into())
    );
    let run = crate::labels::for_result("proc.run", "trs_1", None, None, &[], &[]);
    assert_eq!(run.readers, Readers::Owner);
}

/// The compile filter's cost (design §2.10: a compile of 1,000 nodes, under
/// 50 µs added). A 1,000-node session (250 exchanges: a channel's message,
/// an answer calling `fs.read`, its owner-only result, an answer), compiled
/// with no judge (before labels), for the owner (nothing withheld), and for
/// a channel two people view (the 250 results withheld): the median of 101
/// compiles each, in this build's profile. Run it alone:
/// `cargo nextest run --workspace --run-ignored only -E 'test(bench_the_compile_filter)' --no-capture`.
#[test]
#[ignore = "a bench: run it alone"]
fn bench_the_compile_filter() {
    use crate::labels::{PlaceViewers, Places};
    let d = tempfile::tempdir().unwrap();
    let store = Store::open(d.path()).unwrap();
    let place = "discord:channel:314159265358979323";
    let nodes = big_session(place);
    let places = Places::default();
    places
        .set(
            &store,
            "discord:314159265358979323",
            PlaceViewers {
                name: Some("lab".into()),
                viewers: Some(people(&[OWNER, ALICE])),
            },
        )
        .unwrap();
    let owner = Judge::owner_only();
    let two = Judge::new(Some(place), people(&[OWNER]), &places, &store, false);
    let sp = spec();
    let catalog = crate::catalog::Catalog::builtin();
    let run = |judge: Option<&Judge>| {
        let t = std::time::Instant::now();
        let c = compile(CompileInput {
            session_id: "s",
            current: None,
            nodes: &nodes,
            last_position: nodes.len() as u64,
            spec: &sp,
            catalog: &catalog,
            force: None,
            window_override: Some(10_000_000),
            blobs: None,
            hidden: &[],
            strip: None,
            overflowed: None,
            judge,
        });
        (t.elapsed().as_micros() as u64, c.withheld)
    };
    let median = |judge: Option<&Judge>| {
        let mut times: Vec<u64> = (0..101).map(|_| run(judge).0).collect();
        times.sort_unstable();
        times[50]
    };
    let (none, by_owner, by_two) = (median(None), median(Some(&owner)), median(Some(&two)));
    assert_eq!(run(Some(&two)).1, 250, "the 250 results withheld");
    println!(
        "compile filter, {} nodes: no judge {none} µs; the owner {by_owner} µs ({:+} µs); a channel of two \
         {by_two} µs ({:+} µs, 250 withheld)",
        nodes.len(),
        by_owner as i64 - none as i64,
        by_two as i64 - none as i64
    );
}

/// 250 exchanges in a guild channel, 1,000 nodes: a message, an answer that
/// calls `fs.read`, its owner-only result, and an answer.
fn big_session(place: &str) -> Vec<(u64, Arc<Node>)> {
    use crate::labels::{for_agent, for_input, for_result};
    let channel = Readers::Place("discord:314159265358979323".into());
    let mut nodes: Vec<(u64, Arc<Node>)> = Vec::new();
    for i in 0..250u64 {
        let id = format!("t{i}");
        let user = Node::user("s", None, "discord", &format!("read file {i}"))
            .labeled(for_input(true, Some(place)));
        let mut call = Node::assistant(
            "s",
            "t",
            0,
            Body::AssistantMessage {
                blocks: vec![
                    json!({"type": "text", "text": "Reading it."}),
                    json!({"type": "tool_use", "id": id, "name": "fs_read", "input": {"path": format!("f{i}.txt")}}),
                ],
                model: "claude-sonnet-5-5".into(),
                provider: "anthropic".into(),
                stop_reason: Some("tool_use".into()),
                usage: Default::default(),
                cost_usd: None,
                catalog_version: None,
                request_id: None,
                correlation_id: None,
                compilation_id: None,
                request_digest: None,
            },
        );
        call.label = Some(for_agent(channel.clone()));
        let mut answer = call.clone();
        answer.id = format!("msg_answer_{i}");
        if let Body::AssistantMessage { blocks, .. } = &mut answer.body {
            *blocks = vec![json!({"type": "text", "text": format!("File {i} says hello.")})];
        }
        let result = Node::tool_result(
            "s",
            None,
            None,
            Body::ToolResult {
                tool_use_id: id,
                tool: "fs.read".into(),
                status: crate::node::ResultStatus::Ok,
                is_error: false,
                content: format!("     1\thello from file {i}\n"),
                correlation_id: None,
                bytes_total: 24,
                truncated: false,
                full_ref: None,
                duration_ms: Some(1),
                late: false,
                meta: json!({}),
                image: None,
                external: None,
            },
        )
        .labeled(for_result("fs.read", "trs", None, None, &[], &[]));
        for n in [user, call, result, answer] {
            let at = nodes.len() as u64 + 1;
            nodes.push((at, Arc::new(n)));
        }
    }
    nodes
}
