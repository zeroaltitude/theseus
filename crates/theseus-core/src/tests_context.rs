//! `context.explain` (theseus-7n3e): a turn's parts are the bytes its
//! request carried, they say when they changed since, a place that is not
//! private reads their sizes alone, and the read warns of nothing a turn
//! would.

use std::path::Path;
use std::sync::Arc;

use theseus_protocol::context::{ContextExplainParams, ContextExplainResult};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    _dir: tempfile::TempDir,
}

/// A rig whose config carries one context file, `RULES.md` in its projects
/// directory, named by the path it returns.
fn rig(script: Vec<Scripted>, file: &str) -> (Rig, String) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("work");
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let path = format!("{}/{file}", root.display());
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.tools.projects_dir = Some(root.to_string_lossy().into_owned());
    cfg.tools.roots = vec![];
    cfg.context.files = vec![path.clone().into()];
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    (
        Rig {
            core,
            fake,
            _dir: dir,
        },
        path,
    )
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

fn explain(core: &Core, session: &str, turn: Option<&str>, private: bool) -> ContextExplainResult {
    core.context_explain(
        &ContextExplainParams {
            session_id: session.into(),
            turn_id: turn.map(str::to_string),
        },
        private,
    )
    .unwrap()
}

/// The texts of a block's parts, joined as the system block joins them.
fn joined(x: &ContextExplainResult, block: &str) -> String {
    x.parts
        .iter()
        .filter(|p| p.block == block)
        .map(|p| p.text.clone().unwrap())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn aged(path: &Path) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(60))
        .unwrap();
}

/// The header's parts and the context file's part, joined, are the two
/// system blocks the provider was sent, byte for byte; their digest is the
/// turn's compilation's; and every part has its tokens.
#[tokio::test]
async fn a_turn_s_parts_are_the_system_blocks_its_request_carried() {
    let (r, path) = rig(
        vec![Scripted::text("Four."), Scripted::text("Five.")],
        "RULES.md",
    );
    std::fs::write(&path, "Answer in one word.\n").unwrap();
    aged(Path::new(&path));
    let res = turn(&r.core, None, "what is 2 + 2?").await;
    let x = explain(&r.core, &res.session_id, None, true);
    let sent = &r.fake.requests()[0].system;
    assert_eq!(joined(&x, "header"), sent[0]["text"].as_str().unwrap());
    assert_eq!(joined(&x, "context"), sent[1]["text"].as_str().unwrap());
    let names: Vec<&str> = x
        .parts
        .iter()
        .filter(|p| p.block == "header")
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(&names[..3], ["persona", "assembly", "precedence"]);
    assert_eq!(
        x.unchanged,
        Some(true),
        "{} against {:?}",
        x.digest_now,
        x.digest_then
    );
    assert_eq!(x.turns.len(), 1);
    assert_eq!(x.turn_id.as_deref(), Some(x.turns[0].turn_id.as_str()));
    let file = x.parts.iter().find(|p| p.block == "context").unwrap();
    assert_eq!(
        (file.name.as_str(), file.then.as_deref()),
        (path.as_str(), Some("same"))
    );
    assert!(x.parts.iter().all(|p| p.tokens > 0), "{:?}", x.parts);
    let tools = x.parts.iter().find(|p| p.block == "tools").unwrap();
    assert_eq!(tools.then.as_deref(), Some("same"));
    // The conversation is the rest of the turn's estimate.
    let est = x.compiled.as_ref().unwrap().est_tokens;
    let rest: u64 = x
        .parts
        .iter()
        .filter(|p| p.block != "conversation")
        .map(|p| p.tokens)
        .sum();
    let conversation = x.parts.iter().find(|p| p.block == "conversation").unwrap();
    assert_eq!(conversation.tokens, est.saturating_sub(rest));
    assert_eq!(
        x.compilation.as_ref().unwrap().compilation_id,
        x.compiled.as_ref().unwrap().compilation_id
    );
    assert_eq!(x.class, "private");

    // The file changes: the parts are today's, and say so.
    std::fs::write(&path, "Answer in one word, in capitals.\n").unwrap();
    aged(Path::new(&path));
    let after = explain(&r.core, &res.session_id, None, true);
    assert_eq!(after.unchanged, Some(false));
    let file = after.parts.iter().find(|p| p.block == "context").unwrap();
    assert_eq!(file.then.as_deref(), Some("changed"));
    assert!(file.text.as_deref().unwrap().contains("capitals"));
    // The next turn carries them: its parts are its request's again.
    turn(&r.core, Some(&res.session_id), "and 2 + 3?").await;
    let next = explain(&r.core, &res.session_id, None, true);
    assert_eq!(next.unchanged, Some(true));
    assert_eq!(
        joined(&next, "context"),
        r.fake.requests()[1].system[1]["text"].as_str().unwrap()
    );
    assert_eq!(next.turns.len(), 2);
    // The first turn, asked by its id, against its own compilation.
    let back = explain(&r.core, &res.session_id, Some(&x.turns[0].turn_id), true);
    assert_eq!(back.turn_id.as_deref(), Some(x.turns[0].turn_id.as_str()));
    assert_eq!(
        back.unchanged,
        Some(false),
        "the first turn's file was the one-word rule"
    );
}

/// A place that is not private reads the sizes and digests, and no text.
#[tokio::test]
async fn a_place_that_is_not_private_reads_no_text() {
    let (r, path) = rig(vec![Scripted::text("Four.")], "RULES.md");
    std::fs::write(&path, "Answer in one word.\n").unwrap();
    aged(Path::new(&path));
    let res = turn(&r.core, None, "what is 2 + 2?").await;
    let x = explain(&r.core, &res.session_id, None, false);
    assert!(x.withheld.is_some());
    assert!(x.title.is_none());
    assert!(x.parts.iter().all(|p| p.text.is_none()), "{:?}", x.parts);
    assert!(x
        .parts
        .iter()
        .any(|p| p.block == "context" && p.bytes > 0 && p.digest.is_some()));
    let private = explain(&r.core, &res.session_id, None, true);
    assert_eq!(x.digest_now, private.digest_now);
}

/// The read warns of nothing: a missing context file is still the turn's
/// first warning, and the read says the file is missing.
#[tokio::test]
async fn the_read_leaves_a_missing_file_s_warning_to_the_turn() {
    let (r, path) = rig(vec![Scripted::text("Four.")], "MISSING.md");
    let s = SessionRecord::new(SessionKind::Conversation, None);
    r.core.store.put_session(&s.session_id, &s).unwrap();
    let x = explain(&r.core, &s.session_id, None, true);
    assert_eq!(x.unchanged, None, "no turn yet");
    let file = x.parts.iter().find(|p| p.block == "context").unwrap();
    assert!(
        file.note.as_deref().unwrap().starts_with("missing"),
        "{file:?}"
    );
    turn(&r.core, Some(&s.session_id), "what is 2 + 2?").await;
    let warned: Vec<_> = r
        .core
        .store
        .ledger_tail::<crate::ledger::LedgerRow>(10_000)
        .unwrap()
        .into_iter()
        .filter(|(_, row)| row.kind == "context.file_missing")
        .collect();
    assert_eq!(warned.len(), 1, "the turn warned of {path}");
}

/// A session that does not exist is refused, by its id.
#[tokio::test]
async fn an_unknown_session_is_refused() {
    let (r, _) = rig(vec![], "RULES.md");
    let e = r
        .core
        .context_explain(
            &ContextExplainParams {
                session_id: "ses_nope".into(),
                turn_id: None,
            },
            true,
        )
        .unwrap_err();
    assert!(e.to_string().contains("no session"), "{e}");
}
