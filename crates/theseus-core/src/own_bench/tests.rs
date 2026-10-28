//! Your own benchmark through the whole core (own-benchmark, theseus-pw1q.12):
//! a block written once and resolved by digest; a turn's first request
//! rebuilt byte for byte (and refused, naming the part, when a blob is gone or
//! the bytes differ); `turn.replay` writing a row and nothing else, to the
//! profile it names, with the original's system, tools and messages;
//! `compare.v1` through the fake Jev; and the place rule over a replay.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::own_bench::TurnReplayParams;
use theseus_protocol::LedgerKind;

use super::rebuild::NotRebuildable;
use crate::config::MemoryMode;
use crate::node::Body;
use crate::provider::Scripted;
use crate::secrets::{Secret, SecretBoard};
use crate::tests_recall::{session, turn, Rig};
use crate::Core;

/// The invented request and the answers it gets.
const ASK: &str = "Which port does the kestrel relay listen on?";
const ORIGINAL: &str = "The kestrel relay listens on port 7714.";
const REPLAYED: &str = "Port 7714.";

pub(super) fn board() -> Arc<SecretBoard> {
    let board = SecretBoard::new(["jev_api_key".to_string()], Instant::now());
    board.publish(
        BTreeMap::from([(
            "jev_api_key".to_string(),
            Ok(Secret::new("jev-test-key-0123456789".into())),
        )]),
        "test",
    );
    board
}

pub(super) fn rig(jev: Option<&FakeJev>) -> Rig {
    let board = board();
    let base = jev.map(FakeJev::base);
    crate::tests_recall::rig_with_secrets(MemoryMode::Off, board, |c| {
        if let Some(base) = base {
            c.judge.enabled = true;
            c.judge.api_base = base;
            c.judge.connect_secs = 1;
            c.judge.total_secs = 2;
        }
    })
}

pub(super) fn script(rig: &Rig, answers: &[&str]) {
    let mut s = rig.model.script.lock().unwrap();
    for a in answers {
        s.push_back(Scripted::text(a));
    }
}

/// The user's turn in a fresh session: its id and the turn's.
pub(super) async fn asked(rig: &Rig) -> (String, String) {
    let sid = session(&rig.core, None, &[]);
    let r = turn(&rig.core, &sid, ASK).await;
    (sid, r.turn_id)
}

fn params(sid: &str, turn: &str, profile: &str) -> TurnReplayParams {
    TurnReplayParams {
        session: sid.into(),
        turn: turn.into(),
        profile: profile.into(),
        effort: None,
        scope: None,
        compare: Some(false),
    }
}

fn rows(core: &Core, kind: LedgerKind) -> Vec<crate::ledger::LedgerRow> {
    core.rows_since(kind, 0).unwrap()
}

fn compilation_of(core: &Core, sid: &str) -> crate::compiler::Compilation {
    let rec: crate::session::SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    core.store
        .get_compilation(rec.compilation_id.as_deref().unwrap())
        .unwrap()
        .unwrap()
}

/// Each distinct system block is one blob named by its digest: two sessions
/// of a profile share the header's, which the second compile does not write
/// again, and every blob reads back as the text the request carried.
#[tokio::test]
async fn a_block_is_written_once_per_distinct_text_and_resolves_by_digest() {
    let rig = rig(None);
    script(&rig, &[ORIGINAL, ORIGINAL]);
    let (a, _) = asked(&rig).await;
    let syncs = rig.core.store.blobs().syncs();
    let (b, _) = asked(&rig).await;
    let (ca, cb) = (compilation_of(&rig.core, &a), compilation_of(&rig.core, &b));
    let digests = |c: &crate::compiler::Compilation| -> Vec<String> {
        c.manifest
            .cache
            .as_ref()
            .unwrap()
            .blocks
            .iter()
            .map(|b| b.digest.clone().expect("a block is stamped"))
            .collect()
    };
    let (da, db) = (digests(&ca), digests(&cb));
    assert_eq!(da, db, "the same profile and no files: the same blocks");
    assert!(!da.is_empty());
    assert_eq!(
        rig.core.store.blobs().syncs(),
        syncs,
        "the second session's compile wrote no block again"
    );
    let sent = rig.model.requests();
    for (i, d) in da.iter().enumerate() {
        let text = rig.core.store.blobs().read(d).expect("the blob");
        assert_eq!(crate::blobs::digest(&text), *d);
        assert_eq!(
            String::from_utf8(text).unwrap(),
            sent[0].system[i]["text"].as_str().unwrap(),
            "block {i} is the text the request carried"
        );
    }
}

/// A new compilation's request never waits on its blocks' syncs
/// (theseus-nsp9l.4): two files and the directory, ~7 ms each on the owner's
/// disk, were ~21 ms of the send path, between the input's frame and the
/// request. Held here, the request still reaches the model, and the turn does
/// not end; once the writes land it ends, and every block reads back.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_compilations_request_never_waits_on_its_blocks_syncs() {
    let rig = rig(None);
    script(&rig, &[ORIGINAL]);
    let sid = session(&rig.core, None, &[]);
    let syncs = rig.core.store.blobs().syncs();
    let release = rig.core.store.blobs().hold_puts();
    let asked = {
        let (core, sid) = (rig.core.clone(), sid.clone());
        tokio::spawn(async move { turn(&core, &sid, ASK).await })
    };
    let t0 = Instant::now();
    while rig.model.requests().is_empty() {
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "the request waited on the blocks' syncs"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        rig.core.store.blobs().syncs(),
        syncs,
        "the request left before any block was synced"
    );
    // The model has answered; the turn waits on its blocks, not past them.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        !asked.is_finished(),
        "the turn ended before its blocks were kept"
    );
    drop(release);
    asked.await.unwrap();
    let c = compilation_of(&rig.core, &sid);
    let blocks = &c.manifest.cache.as_ref().unwrap().blocks;
    assert!(!blocks.is_empty());
    for b in blocks {
        let d = b.digest.as_deref().expect("a block is stamped");
        assert!(
            rig.core.store.blobs().read(d).is_some(),
            "block {d} is kept once the turn has ended"
        );
    }
}

/// The first request comes back byte for byte: the request the provider
/// received, and its recorded digest. The messages and settings are held in a
/// golden (`THESEUS_GOLDEN=write` rewrites it); the system is held to its
/// blobs and the tools to the manifest's digest, whatever their text.
#[tokio::test]
async fn a_turns_first_request_is_rebuilt_byte_for_byte() {
    let rig = rig(None);
    script(&rig, &[ORIGINAL]);
    let (sid, t) = asked(&rig).await;
    let got = rig.core.rebuild_first(&sid, &t).expect("rebuildable");
    let sent = &rig.model.requests()[0];
    assert_eq!(&got.request, sent, "the rebuilt request is the one sent");
    assert_eq!(got.digest, sent.digest());
    assert_eq!(got.ask, ASK);
    assert_eq!(got.said.text, ORIGINAL);
    let shape = json!({
        "model": got.request.model,
        "max_tokens": got.request.max_tokens,
        "messages": got.request.messages,
        "thinking": got.request.thinking,
        "output_config": got.request.output_config,
        "cache_control": got.request.cache_control,
        "betas": got.request.betas,
        "system_blocks": got.request.system.iter().map(|b| json!({
            "bytes": b["text"].as_str().unwrap().len() > 100,
            "cache_control": b.get("cache_control"),
        })).collect::<Vec<_>>(),
    });
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/own_bench/first_request.golden.json"
    );
    let text = serde_json::to_string_pretty(&shape).unwrap() + "\n";
    if std::env::var("THESEUS_GOLDEN").as_deref() == Ok("write") {
        std::fs::write(path, &text).unwrap();
    }
    assert_eq!(std::fs::read_to_string(path).unwrap(), text);
}

/// With deferred tools on (theseus-7m7w.2), a turn's first request is still
/// rebuilt byte for byte: the tools as its compilation loaded them, deferred
/// as they were sent.
#[tokio::test]
async fn a_turns_first_request_is_rebuilt_with_its_tools_deferred() {
    let rig = crate::tests_recall::rig_built(
        MemoryMode::Off,
        |_| {},
        |p| {
            p.secrets = board();
            p.cfg.tools.defer.enabled = true;
        },
    );
    script(&rig, &[ORIGINAL]);
    let (sid, t) = asked(&rig).await;
    let sent = &rig.model.requests()[0];
    assert!(
        sent.tools
            .iter()
            .any(|t| t.get("defer_loading") == Some(&serde_json::Value::Bool(true))),
        "the turn deferred some tools"
    );
    let got = rig.core.rebuild_first(&sid, &t).expect("rebuildable");
    assert_eq!(&got.request, sent, "the rebuilt request is the one sent");
}

/// A part that cannot be rebuilt is named, never guessed: a block whose blob
/// is gone; a compilation from before blocks were kept by digest; blobs that
/// are not the text they are named for.
#[tokio::test]
async fn a_replay_is_refused_naming_the_part_it_cannot_rebuild() {
    let rig = rig(None);
    script(&rig, &[ORIGINAL]);
    let (sid, t) = asked(&rig).await;
    let c = compilation_of(&rig.core, &sid);
    let first = c.manifest.cache.as_ref().unwrap().blocks[0]
        .digest
        .clone()
        .unwrap();
    // The blob corrupted: not the text it is named for.
    let path = rig.core.store.blobs().path(&first);
    let good = std::fs::read(&path).unwrap();
    std::fs::write(&path, b"another text").unwrap();
    let no = |r: Result<_, NotRebuildable>| match r {
        Ok(_) => panic!("rebuilt"),
        Err(e) => e,
    };
    let e = no(rig.core.rebuild_first(&sid, &t).map(|_| ()));
    assert_eq!(e.part, "system", "{e}");
    assert!(e.why.contains("missing or does not hash"), "{e}");
    // The blob gone.
    std::fs::remove_file(&path).unwrap();
    let e = no(rig.core.rebuild_first(&sid, &t).map(|_| ()));
    assert_eq!(e.part, "system", "{e}");
    assert!(
        e.why.contains("missing or does not hash") && e.why.contains(&first),
        "{e}"
    );
    // And back: it rebuilds.
    std::fs::write(&path, good).unwrap();
    assert!(rig.core.rebuild_first(&sid, &t).is_ok());
    // A compilation from before the digests.
    let mut old = c;
    for b in &mut old.manifest.cache.as_mut().unwrap().blocks {
        b.digest = None;
    }
    rig.core
        .store
        .append(&[theseus_store::NewRecord::json(
            theseus_store::kinds::COMPILATION,
            Some(&old.id),
            &old,
        )
        .unwrap()
        .scoped(&old.session_id)])
        .unwrap();
    let e = no(rig.core.rebuild_first(&sid, &t).map(|_| ()));
    assert_eq!(e.part, "system", "{e}");
    assert!(e.why.contains("predates"), "{e}");
}

/// A turn that is not there, or whose conversation changed, says so too.
#[tokio::test]
async fn a_turn_it_cannot_find_is_refused() {
    let rig = rig(None);
    script(&rig, &[ORIGINAL]);
    let (sid, _) = asked(&rig).await;
    let e = rig.core.rebuild_first(&sid, "trn_nothing").err().unwrap();
    assert_eq!(e.part, "answer");
    let e = rig
        .core
        .rebuild_first("ses_nothing", "trn_x")
        .err()
        .unwrap();
    assert_eq!(e.part, "session");
}

/// `turn.replay` sends the original's system, tools and messages to the
/// profile it names, returns the reply and its price, and writes a
/// `turn.replayed` row and nothing else: no node, no session.
#[tokio::test]
async fn a_replay_writes_a_row_and_nothing_else() {
    let rig = rig(None);
    script(&rig, &[ORIGINAL, REPLAYED]);
    let (sid, t) = asked(&rig).await;
    let nodes = rig.core.store.session_nodes(&sid).unwrap().len();
    let sessions = rig
        .core
        .store
        .live_sessions::<crate::session::SessionRecord>()
        .unwrap()
        .len();
    let r = rig
        .core
        .turn_replay(params(&sid, &t, "haiku"), "cli")
        .await
        .unwrap();
    assert_eq!(r.replay.text, REPLAYED);
    assert_eq!(r.original.text, ORIGINAL);
    assert_eq!(r.profile, "haiku");
    assert!(r.unjudged.is_none() && r.verdict.is_none(), "not asked");
    assert!(r.cost_usd >= 0.0 && r.scope == "first_loop");
    let sent = rig.model.requests();
    assert_eq!(sent.len(), 2, "one call for the replay");
    let (orig, re) = (&sent[0], &sent[1]);
    assert_ne!(re.model, orig.model, "the other profile's model");
    assert_eq!(re.messages, orig.messages, "the same messages");
    assert_eq!(re.tools, orig.tools, "the same tools");
    assert_eq!(
        re.system
            .iter()
            .map(|b| b["text"].clone())
            .collect::<Vec<_>>(),
        orig.system
            .iter()
            .map(|b| b["text"].clone())
            .collect::<Vec<_>>(),
        "the same system"
    );
    let replayed = rows(&rig.core, LedgerKind::TurnReplayed);
    assert_eq!(replayed.len(), 1);
    assert_eq!(replayed[0].turn_id.as_deref(), Some(t.as_str()));
    assert_eq!(replayed[0].data["by"], "owner");
    assert_eq!(replayed[0].data["profile"], "haiku");
    assert_eq!(rig.core.store.session_nodes(&sid).unwrap().len(), nodes);
    assert_eq!(
        rig.core
            .store
            .live_sessions::<crate::session::SessionRecord>()
            .unwrap()
            .len(),
        sessions
    );
    assert!(!rig.core.store.session_nodes(&sid).unwrap().iter().any(
        |(_, n)| matches!(&n.body, Body::AssistantMessage { blocks, .. }
            if crate::provider::text_of(blocks) == REPLAYED)
    ));
}

/// A replay is the owner's, from a private surface: the MCP server's is
/// refused before a model is called.
#[tokio::test]
async fn a_replay_from_the_mcp_surface_is_refused_and_calls_no_model() {
    let rig = rig(None);
    script(&rig, &[ORIGINAL, REPLAYED]);
    let (sid, t) = asked(&rig).await;
    let who = crate::approval::Answerer {
        label: "mcp#1".into(),
        surface: crate::approval::Surface::Mcp,
        discord: None,
        agent: None,
    };
    let err = rig
        .core
        .turn_replay(params(&sid, &t, "haiku"), who)
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("does not count"), "{err:#}");
    assert_eq!(rig.model.requests().len(), 1, "no call was made");
}

/// A shared place's turn is never replayed.
#[tokio::test]
async fn a_shared_places_turn_is_never_replayed() {
    let rig = rig(None);
    script(&rig, &[ORIGINAL, REPLAYED]);
    let sid = session(
        &rig.core,
        Some(&format!("channel:{}", crate::tests_recall::PIER)),
        &[],
    );
    let t = turn(&rig.core, &sid, ASK).await.turn_id;
    let err = rig
        .core
        .turn_replay(params(&sid, &t, "haiku"), "cli")
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("shared place"), "{err:#}");
    assert_eq!(rig.model.requests().len(), 1);
}

/// `compare.v1` through the fake Jev: the replay is judged, the verdict is
/// Jev's, the pack stands in shadow, and its state holds both replies.
#[tokio::test]
async fn compare_judges_the_two_replies_in_shadow() {
    let jev = FakeJev::start().unwrap();
    jev.script(
        "compare.v1/verdict",
        Jev::Choice {
            option: "same".into(),
            confidence: 0.92,
        },
    );
    let rig = rig(Some(&jev));
    script(&rig, &[ORIGINAL, REPLAYED]);
    let (sid, t) = asked(&rig).await;
    let mut p = params(&sid, &t, "haiku");
    p.compare = None;
    let r = rig.core.turn_replay(p, "cli").await.unwrap();
    let v = r.verdict.expect("judged");
    assert_eq!(v.verdict, "same");
    assert!(v.p > 0.9 && v.same_or_better > 0.9, "{v:?}");
    assert_eq!(v.mode, "shadow");
    let seen = jev.seen();
    let body = seen
        .iter()
        .map(|s| s.body.to_string())
        .find(|b| b.contains("compare.v1/verdict"))
        .expect("the comparison was asked");
    assert!(
        body.contains(ASK) && body.contains(ORIGINAL) && body.contains(REPLAYED),
        "{body}"
    );
}

/// compare.v1 is wired in shadow, and the pack has no action.
#[test]
fn compare_is_wired_in_shadow_with_no_action() {
    assert_eq!(
        crate::judge::wired_mode(crate::judge::compare::COMPARE_PACK),
        crate::config::PackMode::Shadow
    );
    let p = theseus_judge::pack::by_name("compare.v1").unwrap();
    assert_eq!(p.action, theseus_judge::pack::Action::None);
    assert_eq!(p.point, theseus_judge::pack::Point::Replay);
    assert_eq!(p.questions.len(), 1);
}

/// A replay sends the system the turn saw, from its blobs, not the one the
/// config and the files build now: the context file changed after the turn,
/// and the replay's request still carries the old text.
#[tokio::test]
async fn a_replay_sends_the_system_the_turn_saw_not_the_one_built_now() {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let work = work.canonicalize().unwrap();
    let file = format!("{}/NOTES.md", work.display());
    std::fs::write(&file, "The relay listens on 7714.\n").unwrap();
    let rig = crate::tests_recall::rig_with(MemoryMode::Off, |c| {
        c.tools.projects_dir = Some(work.to_string_lossy().into_owned());
        c.context.files = vec![file.clone().into()];
    });
    script(&rig, &[ORIGINAL, REPLAYED]);
    let (sid, t) = asked(&rig).await;
    std::fs::write(&file, "The relay listens on 9000 now.\n").unwrap();
    rig.core
        .turn_replay(params(&sid, &t, "haiku"), "cli")
        .await
        .unwrap();
    let sent = rig.model.requests();
    let system = |r: &crate::provider::ProviderRequest| {
        r.system
            .iter()
            .map(|b| b["text"].to_string())
            .collect::<String>()
    };
    assert!(system(&sent[0]).contains("7714"));
    assert!(system(&sent[1]).contains("7714"), "the turn's own system");
    assert!(!system(&sent[1]).contains("9000"), "not the one built now");
}

#[test]
fn a_reply_is_the_wires_own_type_and_serialises_as_it_always_did() {
    let said = super::rebuild::Said {
        model: "m".into(),
        text: "t".into(),
        tool_calls: vec!["fs.read".into()],
        input_tokens: 3,
        output_tokens: 4,
        cost_usd: 0.5,
    };
    assert_eq!(
        serde_json::to_string(&said).unwrap(),
        r#"{"model":"m","text":"t","tool_calls":["fs.read"],"input_tokens":3,"output_tokens":4,"cost_usd":0.5}"#
    );
}

/// A replay to a profile of restricted origin is refused in words before the
/// rebuild, in both scopes: nothing is sent, no `turn.replayed` row and no
/// blocked row is written, and a replay of the same turn to a profile of US
/// origin still runs (theseus-a1wvq.10).
#[tokio::test]
async fn a_replay_to_a_restricted_profile_is_refused_in_words() {
    let zai = Arc::new(crate::provider::FakeProvider::default());
    let given = zai.clone();
    let rig = crate::tests_recall::rig_built(
        MemoryMode::Off,
        |c| c.copy_profile("dsflash", "glm"),
        move |p| {
            p.secrets = board();
            p.providers.insert("zai".into(), given);
        },
    );
    script(&rig, &[ORIGINAL, REPLAYED]);
    let (sid, t) = asked(&rig).await;
    let sent = rig.model.requests().len();
    for scope in [None, Some("whole_turn")] {
        let mut p = params(&sid, &t, "dsflash");
        p.scope = scope.map(Into::into);
        let err = rig.core.turn_replay(p, "cli").await.unwrap_err();
        let said = format!("{err:#}");
        assert!(said.contains("restricted origin"), "{scope:?}: {said}");
        assert!(said.contains("not replayed on `dsflash`"), "{said}");
        assert!(
            err.downcast_ref::<super::replay::Crossing>().is_some(),
            "refused as a crossing is"
        );
    }
    assert!(zai.requests().is_empty(), "nothing was sent");
    assert_eq!(rig.model.requests().len(), sent);
    assert!(rows(&rig.core, LedgerKind::TurnReplayed).is_empty());
    assert!(crate::tests_judge::kinds(&rig.core.store, "data_gate.blocked").is_empty());
    let ok = rig
        .core
        .turn_replay(params(&sid, &t, "haiku"), "cli")
        .await
        .unwrap();
    assert_eq!(ok.replay.text, REPLAYED);
    assert_eq!(rows(&rig.core, LedgerKind::TurnReplayed).len(), 1);
}
