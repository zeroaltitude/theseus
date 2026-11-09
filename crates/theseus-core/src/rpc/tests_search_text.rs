//! A search's words reach only a surface that reads private text (theseus-snb3,
//! the w9qv report's lane E). `index.query`, `memory.search` and
//! `memory.recalls` are asked over a real protocol connection of each
//! surface, so the test fails if a dispatch arm forgets to pass the surface on:
//! the CLI and the web UI read every hit's words, Discord's binding and a
//! connection no listener named read the same hits with the words of a private
//! or imported session's gone. Each side asserts how many hits it holds, so an
//! empty answer cannot pass.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::index::{IndexHit, IndexQueryResult, IndexSourceRank};
use theseus_protocol::memory::{MemoryRecallsResult, RecallManifest};
use theseus_protocol::{Request, Response, SessionKind};

use super::search_text::WITHHELD;
use crate::approval::{Client, Surface};
use crate::config::MemoryMode;
use crate::node::Node;
use crate::session::SessionRecord;
use crate::tests_m3::rpc_as;
use crate::tests_recall::{index_of, rig, session, PIER};
use crate::Core;

/// Words that belong to one session each, so a leak names its source.
const IMPORTED: &str = "the acquisition target is Harbourlight";
const PRIVATE: &str = "the heron nests by the weir";
const SHARED: &str = "the pier's heron is grey";

/// The surfaces that read private text, and those that do not.
const READS: [Surface; 2] = [Surface::Cli, Surface::Web];
const DOES_NOT: [Surface; 2] = [Surface::Discord, Surface::Unnamed];

/// An imported session: the owner's own history, private whatever place its
/// episode names.
fn imported(core: &Core, tail: &str, said: &[&str]) -> String {
    let mut rec = SessionRecord::new(SessionKind::Conversation, None);
    let id = format!("{}{}", crate::import::SESSION_PREFIX, tail);
    rec.session_id = id.clone();
    core.store.put_session(&id, &rec).unwrap();
    for s in said {
        let n = Node::user(&id, None, "test", s);
        core.store.append(&[n.record().unwrap()]).unwrap();
    }
    id
}

async fn ask(core: &Arc<Core>, surface: Surface, method: &str, params: Value) -> Value {
    let client = Client::new(format!("{}#1", surface.as_str()), surface);
    rpc_as(core, client, method, params)
        .await
        .unwrap_or_else(|e| panic!("{method} from {}: {e:?}", surface.as_str()))
}

/// The three sessions every test searches: an imported one, one the CLI
/// spoke in, and one in a shared place.
fn sessions(core: &Core) -> (String, String, String) {
    (
        imported(core, "00ab12cd", &[IMPORTED]),
        session(core, None, &[PRIVATE]),
        session(core, Some(&format!("channel:{PIER}")), &[SHARED]),
    )
}

/// What may be in an answer a surface that reads no private text was given.
fn assert_no_private_words(answer: &Value) {
    let said = answer.to_string();
    for word in ["acquisition", "Harbourlight", "weir", "org:Harbourlight"] {
        assert!(!said.contains(word), "{word} leaked: {said}");
    }
}

/// `memory.search` over the three sessions: the CLI and the web UI read all
/// three items' words, the others read the shared place's and not the other
/// two's, and every other field of every item is what the CLI read.
#[tokio::test]
async fn memory_search_withholds_private_words_from_a_surface_that_reads_none() {
    let r = rig(MemoryMode::Off);
    let c = &r.core;
    let (imp, cli, pier) = sessions(c);
    c.runner
        .memory
        .set_ask(index_of(c, vec![imp.clone(), cli.clone(), pier.clone()]));
    let params = json!({"query": "heron"});
    let mut private = Vec::new();
    for surface in READS {
        let m: RecallManifest =
            serde_json::from_value(ask(c, surface, "memory.search", params.clone()).await).unwrap();
        assert_eq!(m.admitted.len(), 3, "{}: every item", surface.as_str());
        let words: Vec<_> = m
            .admitted
            .iter()
            .filter_map(|i| i.text.as_deref())
            .collect();
        for want in [IMPORTED, PRIVATE, SHARED] {
            assert!(
                words.contains(&want),
                "{}: {want} in {words:?}",
                surface.as_str()
            );
        }
        private = m.admitted;
    }
    for surface in DOES_NOT {
        let answer = ask(c, surface, "memory.search", params.clone()).await;
        let m: RecallManifest = serde_json::from_value(answer.clone()).unwrap();
        assert_eq!(m.admitted.len(), private.len(), "{}", surface.as_str());
        let mut held = 0;
        for item in &m.admitted {
            let was = private.iter().find(|p| p.node_id == item.node_id).unwrap();
            match item.session_id == pier {
                true => {
                    held += 1;
                    assert_eq!(item, was, "the shared place's own item is left alone");
                }
                false => {
                    assert_eq!(item.text.as_deref(), Some(WITHHELD));
                    let mut same = item.clone();
                    same.text = was.text.clone();
                    assert_eq!(&same, was, "only the words differ");
                }
            }
        }
        assert_eq!(held, 1, "{}: one item kept its words", surface.as_str());
        let withheld = m
            .admitted
            .iter()
            .filter(|i| i.text.as_deref() == Some(WITHHELD));
        assert_eq!(withheld.count(), 2, "{}", surface.as_str());
        assert_no_private_words(&answer);
    }
}

/// A hit as the tender answers it: every field a withheld hit must keep, and
/// the text and entities it must lose.
fn hit(session: &str, node: &str, position: u64, text: &str, entity: &str) -> IndexHit {
    IndexHit {
        node_id: node.into(),
        chunk: 0,
        session_id: session.into(),
        position,
        kind: "user_message".into(),
        origin: "operator".into(),
        author: Some("test".into()),
        place: Some("discord:channel:1".into()),
        tool: None,
        time_ms: 1_700_000_000_000 + position,
        external: false,
        text: text.into(),
        entities_matched: vec![entity.into()],
        entities: vec![entity.into(), "kind:note".into()],
        sources: [(
            "bm25".to_string(),
            IndexSourceRank {
                rank: 1,
                score: 2.5,
            },
        )]
        .into(),
        fused: 0.03,
    }
}

/// A stand-in tender on the core's own tender socket: every `index.query` is
/// answered with `hits`, and the core's supervisor is told it runs.
fn tender(core: &Core, hits: Vec<IndexHit>) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    std::fs::create_dir_all(core.index.dir()).unwrap();
    let listener = tokio::net::UnixListener::bind(core.index.socket()).unwrap();
    core.index.mark_running();
    let answer = IndexQueryResult {
        hits,
        indexed_through: 9,
        lag: Default::default(),
        timings: Default::default(),
        skipped: Default::default(),
        weights: Default::default(),
    };
    tokio::spawn(async move {
        loop {
            let Ok((conn, _)) = listener.accept().await else {
                return;
            };
            let answer = answer.clone();
            tokio::spawn(async move {
                let (r, mut w) = conn.into_split();
                let mut lines = BufReader::new(r).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let req: Request = serde_json::from_str(&line).unwrap();
                    assert_eq!(
                        req.method, "index.query",
                        "the tender is asked nothing else"
                    );
                    let mut out = serde_json::to_vec(&Response::ok(req.id, &answer)).unwrap();
                    out.push(b'\n');
                    if w.write_all(&out).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
}

/// `index.query` over the same three sessions, and a fourth the store does
/// not know (its place cannot be read as a shared one, so it is private): the
/// CLI and the web UI read every hit as the tender sent it; the others read
/// each private hit's ids, position, kind, author, place, date, sources and
/// scores, and no words and no entities.
#[tokio::test]
async fn index_query_withholds_private_words_from_a_surface_that_reads_none() {
    let r = rig(MemoryMode::Off);
    let c = &r.core;
    let (imp, cli, pier) = sessions(c);
    let hits = vec![
        hit(&imp, "nod_imp", 1, IMPORTED, "org:Harbourlight"),
        hit(&cli, "nod_cli", 2, PRIVATE, "bird:heron"),
        hit(&pier, "nod_pier", 3, SHARED, "bird:grey-heron"),
        hit(
            "ses_nobody",
            "nod_none",
            4,
            "an unplaced note about Harbourlight",
            "org:Harbourlight",
        ),
    ];
    tender(c, hits.clone());
    let params = json!({"text": "heron"});
    for surface in READS {
        let got: IndexQueryResult =
            serde_json::from_value(ask(c, surface, "index.query", params.clone()).await).unwrap();
        assert_eq!(
            got.hits,
            hits,
            "{}: the tender's answer as it came",
            surface.as_str()
        );
    }
    for surface in DOES_NOT {
        let answer = ask(c, surface, "index.query", params.clone()).await;
        let got: IndexQueryResult = serde_json::from_value(answer.clone()).unwrap();
        assert_eq!(got.hits.len(), hits.len(), "{}", surface.as_str());
        let mut kept = 0;
        for (got, sent) in got.hits.iter().zip(&hits) {
            if sent.session_id == pier {
                kept += 1;
                assert_eq!(got, sent, "the shared place's own hit is left alone");
                continue;
            }
            assert_eq!(got.text, WITHHELD, "{}", sent.node_id);
            assert!(got.entities.is_empty() && got.entities_matched.is_empty());
            let mut same = got.clone();
            same.text = sent.text.clone();
            same.entities = sent.entities.clone();
            same.entities_matched = sent.entities_matched.clone();
            assert_eq!(
                &same, sent,
                "{}: only the words and entities differ",
                sent.node_id
            );
        }
        assert_eq!(kept, 1, "{}", surface.as_str());
        assert_no_private_words(&answer);
        assert_eq!(
            got.indexed_through, 9,
            "the answer's own fields ride through"
        );
    }
}

/// `memory.recalls` gives a session's recalled notes the same way: a turn in
/// the CLI's place recalls the three sessions, and the web UI reads all three
/// notes while Discord's binding reads the shared place's alone.
#[tokio::test]
async fn memory_recalls_withholds_private_words_from_a_surface_that_reads_none() {
    let r = rig(MemoryMode::Shadow);
    let c = &r.core;
    let (imp, cli, pier) = sessions(c);
    let here = session(c, None, &[]);
    c.runner
        .memory
        .set_ask(index_of(c, vec![imp.clone(), cli.clone(), pier.clone()]));
    crate::test_support::turn(c, &here, "where does the heron nest?").await;
    let params = json!({"session_id": here});
    let recall = |v: Value| -> Vec<(String, Option<String>)> {
        let r: MemoryRecallsResult = serde_json::from_value(v).unwrap();
        assert_eq!(r.recalls.len(), 1, "one recall a turn");
        r.recalls[0]
            .admitted
            .iter()
            .map(|i| (i.session_id.clone(), i.text.clone()))
            .collect()
    };
    for surface in READS {
        let read = recall(ask(c, surface, "memory.recalls", params.clone()).await);
        assert_eq!(read.len(), 3, "{}", surface.as_str());
        assert!(read
            .iter()
            .all(|(_, t)| t.as_deref().is_some_and(|t| t != WITHHELD)));
    }
    for surface in DOES_NOT {
        let answer = ask(c, surface, "memory.recalls", params.clone()).await;
        let read = recall(answer.clone());
        assert_eq!(read.len(), 3, "{}", surface.as_str());
        for (session, text) in &read {
            match *session == pier {
                true => assert_eq!(text.as_deref(), Some(SHARED)),
                false => assert_eq!(text.as_deref(), Some(WITHHELD), "{session}"),
            }
        }
        assert_no_private_words(&answer);
    }
}
