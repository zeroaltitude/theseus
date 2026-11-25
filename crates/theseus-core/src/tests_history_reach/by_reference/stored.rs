//! A look back's answer is stored as references only when every range it would store begins and ends outside every
//! known value, read on the whole field the range points into (verbatim or encoded); otherwise that answer is
//! stored as words, exactly as a build before references stored it (issue 49). A cap through a value written over
//! lines keeps the value's lines one quote, so that answer is stored as references (issue 52). Through the history
//! tool and the runtime, each with a made-up value placed so a range would begin or end inside it. All values and
//! words invented.

use std::sync::Arc;

use theseus_protocol::index::{IndexHit, IndexQueryResult, IndexSourceRank};

use super::*;
use crate::quotes::{cut_through, no_part_of, read_as_written, render_with, Part, Read};
use crate::recall::{Ask, AskFuture};
use crate::tests_cut_sites::{knowing_core, whole_or_absent, V};

/// A value written over two lines, as a key file's is.
const KEY: &str = "kelp-ferry-0417-north\nvault-token-9932-south";

/// A core that knows `value`, whose model calls the history tool as [`script`] says, its results capped at 2,000
/// characters.
fn knowing(value: &str) -> Rig {
    knowing_core(value, |c| c.tools.result_max_chars = 2_000, script)
}

/// `copy`'s stored record: its words and its parts.
fn stored(core: &Core, copy: &str) -> (String, Vec<Part>) {
    let (_, n) = core.store.get_node(copy).unwrap().expect("the result");
    match n.body {
        Body::ToolResult { content, parts, .. } => (content, parts),
        b => panic!("not a result: {b:?}"),
    }
}

/// That `copy` is stored as references: no words of its own, no range that begins or ends inside `value`, `value`
/// whole in one quote, and no part of it in what is stored; its rendering the text the reader was given, each line
/// of `value` in it; and that the reader's next request lays it.
async fn as_parts(r: &Rig, reader: &str, copy: &str, kept: &str, value: &str) {
    let (content, parts) = stored(&r.core, copy);
    assert!(
        content.is_empty() && !parts.is_empty(),
        "stored as words: {content}"
    );
    let read = |q: &crate::quotes::Quote| read_as_written(&r.core.store, q);
    let through = cut_through(&parts, read, &[value]);
    assert!(through.is_empty(), "a range cuts the value: {through:#?}");
    let whole = |p: &Part| match p {
        Part::Quote(q) => match read(q) {
            Read::Field(f) => f.get(q.from as usize..q.to as usize) == Some(value),
            _ => false,
        },
        Part::Code(_) => false,
    };
    assert!(
        parts.iter().any(whole),
        "the value is one quote: {parts:#?}"
    );
    let record = serde_json::to_string(&parts).unwrap();
    for line in value.lines() {
        // `no_part_of` allows a whole occurrence by design; neither is stored here.
        assert!(!record.contains(line), "{line:?} is stored: {record}");
        assert!(
            no_part_of(&record, line),
            "part of {line:?} is stored: {record}"
        );
    }
    assert_eq!(render_with(&parts, read), kept);
    for line in value.lines() {
        assert!(kept.contains(line), "{line:?} is shown: {kept}");
    }
    turn(&r.core, reader, "and then?").await;
    let (_, laid) = laid_result(r);
    let laid = laid.to_string();
    assert!(laid.contains("the keeper's note"), "laid as stored: {laid}");
}

/// A message holding [`KEY`] over two lines, then an empty line, then one long line: an open of it keeps its
/// whole lines before the cap's head cut, which falls in the long line, and the run of whole lines ends in the
/// empty one, which one quote of its bytes would drop; the cap keeps the key's two lines one quote.
fn keyed() -> String {
    format!(
        "the keeper's note\n{KEY}\n\n{}",
        "the tide table ".repeat(200)
    )
}

/// An exchange open, capped by the runtime through a value written over lines: stored as references, the value
/// whole in one quote.
#[tokio::test]
async fn an_exchange_open_capped_through_a_value_is_stored_as_parts() {
    let r = knowing(KEY);
    let sid = session(&r.core, None);
    let said = turn(&r.core, &sid, &keyed()).await.turn_id;
    let reader = session(&r.core, Some("channel:lab"));
    let (copy, kept) = read_in(&r.core, &reader, json!({ "open": said })).await;
    assert!(kept.contains(" not shown: "), "capped: {kept}");
    as_parts(&r, &reader, &copy, &kept, KEY).await;
}

/// A session open, capped by the runtime through a value written over lines: stored as references, the value
/// whole in one quote.
#[tokio::test]
async fn a_session_open_capped_through_a_value_is_stored_as_parts() {
    let r = knowing(KEY);
    let sid = session(&r.core, None);
    turn(&r.core, &sid, &keyed()).await;
    let reader = session(&r.core, Some("channel:lab"));
    let (copy, kept) = read_in(&r.core, &reader, json!({ "open": sid })).await;
    assert!(kept.contains(" not shown: "), "capped: {kept}");
    as_parts(&r, &reader, &copy, &kept, KEY).await;
}

/// A stand-in words index that answers a search of `words` with one hit on `node`, its text `excerpt`.
fn excerpt_of(core: &Core, node: String, words: &'static str, excerpt: String) -> Ask {
    let store = core.store.clone();
    Arc::new(move |p| -> AskFuture {
        let hits = match p.text.trim() == words {
            false => Vec::new(),
            true => {
                let (position, n) = store.get_node(&node).unwrap().expect("the message");
                vec![IndexHit {
                    text: excerpt.clone(),
                    time_ms: n.created_at_ms,
                    node_id: n.id,
                    chunk: 0,
                    session_id: n.session_id,
                    position,
                    kind: "imported".into(),
                    origin: "import".into(),
                    author: None,
                    place: None,
                    tool: None,
                    external: false,
                    entities_matched: vec![],
                    entities: Vec::new(),
                    sources: [(
                        "bm25".to_string(),
                        IndexSourceRank {
                            rank: 1,
                            score: 1.0,
                        },
                    )]
                    .into(),
                    fused: 1.0 / 61.0,
                }]
            }
        };
        Box::pin(async move {
            Ok(IndexQueryResult {
                hits,
                indexed_through: 0,
                lag: Default::default(),
                timings: Default::default(),
                skipped: Default::default(),
                weights: Default::default(),
            })
        })
    })
}

/// An import search whose hit the index began inside one known value and ended inside another: its quote would
/// begin and end inside them, so the page is stored as words, the words the search showed, as before.
#[tokio::test]
async fn an_import_search_whose_hit_begins_inside_a_value_is_stored_as_words() {
    use crate::tests_imports::{episode, import};
    let r = knowing(V);
    let text = format!("the keeper's note: {V} then the harbour lantern {V} and more");
    let ids = import(
        &r.core,
        &[episode(7, "personal", "2026-03-03T18:00:00Z", &text)],
    );
    let node = (r.core.store.session_nodes(&ids[0]).unwrap().into_iter())
        .map(|(_, n)| n)
        .find(|n| matches!(&n.body, Body::Imported { text: t, .. } if t.contains("harbour lantern") && !t.starts_with("Episode")))
        .expect("the imported message");
    let field = crate::recall::text_of(&node);
    let (a, b) = (field.find(V).unwrap() + 9, field.rfind(V).unwrap() + 9);
    let excerpt = format!("{}\u{2026}", &field[a..b]);
    (r.core.runner.memory).set_ask(excerpt_of(&r.core, node.id.clone(), "harbour", excerpt));
    let reader = session(&r.core, None);
    let (copy, kept) = read_in(
        &r.core,
        &reader,
        json!({ "place": "import", "words": "harbour" }),
    )
    .await;
    assert!(kept.contains("then the harbour lantern"), "{kept}");
    let (content, parts) = stored(&r.core, &copy);
    assert!(parts.is_empty(), "stored as references: {parts:#?}");
    assert_eq!(content, kept, "the words the search showed, as before");
}

/// A history page whose message runs past its line's cut through a known value: the page's own cut stops at the
/// value's edge, so every range it stores begins and ends outside the value and it is stored as references.
#[tokio::test]
async fn a_history_page_cut_at_a_value_stores_only_ranges_outside_it() {
    let r = knowing(V);
    let sid = session(&r.core, None);
    turn(
        &r.core,
        &sid,
        &format!("{}{V} and the rest", "x".repeat(290)),
    )
    .await;
    let (copy, kept) = read_in(&r.core, &sid, json!({})).await;
    assert!(
        kept.contains(&format!("{}\u{2026}", "x".repeat(290))),
        "{kept}"
    );
    let (content, parts) = stored(&r.core, &copy);
    assert!(content.is_empty() && !parts.is_empty(), "kept by reference");
    let read = |q: &crate::quotes::Quote| read_as_written(&r.core.store, q);
    assert!(cut_through(&parts, read, &[V]).is_empty(), "{parts:#?}");
    whole_or_absent("the page as kept", &kept, V);
}

/// A look back's result kept by reference, retracted itself: the gist the retraction states and stores is made
/// from its rendering, not from its stored words, which are none.
#[tokio::test]
async fn a_retracted_look_backs_gist_is_its_rendering() {
    let r = rig();
    let said = say(&r.core).await;
    let reader = session(&r.core, Some("channel:lab"));
    let (copy, kept) = read_in(&r.core, &reader, json!({ "open": said.turn })).await;
    let (content, parts) = stored(&r.core, &copy);
    assert!(content.is_empty() && !parts.is_empty(), "kept by reference");
    let first = kept
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");
    let out = retract(&r.core, &copy);
    let gist = out.gist.expect("a gist");
    assert!(gist.starts_with(&first), "{gist:?} from {kept:?}");
    // Asked again, the retraction is read back as it was stored.
    let again = retract(&r.core, &copy);
    assert_eq!(again.gist.as_deref(), Some(gist.as_str()));
}
