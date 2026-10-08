//! `memory.lookup` (theseus-w9qv, fix C) through the whole core: offered in
//! a private place and never in a shared one, refused there by the gate and
//! again by the tool itself; a personal or partner-confidential item's text
//! stays veiled, its labels shown; a month and a range keep to their
//! episodes, at the daemon's time zone's edges; small pages with a cursor;
//! and a search of words through recall's pipeline, its items dated.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::import::{ImportEpisodesParams, ImportLine};
use theseus_protocol::index::{IndexHit, IndexQueryResult, IndexSourceRank};

use crate::import::episode;
use crate::node::Node;
use crate::provider::{ProviderRequest, Scripted};
use crate::recall::{Ask, AskFuture};
use crate::tests_places::{
    answers_a_call, last_user, offered, refused, result_of, rig_scripted, session, turn, LAB,
};
use crate::Core;

/// What the veiled episodes say: no answer may carry it.
const HIDDEN: &str = "the lighthouse keeper's letters";
/// What the episodes the veil leaves alone say.
const SHOWN: &str = "the tide-table parser";

/// An episode of `sensitivity`, its one message at `at` (RFC 3339), its
/// summary saying `says`.
fn episode(n: usize, sensitivity: &str, at: &str, says: &str) -> Value {
    let mut v = json!({
        "format": 1, "import_tag": "kelp-2026", "episode_id": format!("ep_{:064x}", 0xbeef + n),
        "source": "claude-cli", "agent": "main",
        "place": {"kind": "cli", "name": format!("desk-{n}")},
        "as_of": {"start": at, "end": at},
        "labels": {"sensitivity": sensitivity, "topic": ["kelp/notes"], "book_hint": "casebook"},
        "summary": {"text": format!("Episode {n}: {says}."), "cites": [0], "model": "claude-opus-5-5"},
        "messages": [{"idx": 0, "time": at, "author": "wren", "integrity": "operator",
                      "text": format!("Message {n}: {says}."), "unit": format!("u-{n}"),
                      "sha256": "ab".repeat(32)}],
    });
    v["hash"] = json!(episode::hash_of(&v));
    v
}

/// Five episodes: four in March 2026 (one of each sensitivity, the veiled
/// ones saying `HIDDEN`), one in September 2026.
fn episodes() -> Vec<Value> {
    vec![
        episode(0, "personal", "2026-03-03T18:00:00Z", HIDDEN),
        episode(1, "company-confidential", "2026-03-10T18:00:00Z", SHOWN),
        episode(2, "partner-confidential", "2026-03-17T18:00:00Z", HIDDEN),
        episode(3, "public", "2026-03-24T18:00:00Z", SHOWN),
        episode(
            4,
            "public",
            "2026-09-15T18:00:00Z",
            "the September renderer",
        ),
    ]
}

fn import(core: &Core, episodes: &[Value]) -> Vec<String> {
    let p = ImportEpisodesParams {
        file: "kelp.jsonl".into(),
        lines: episodes
            .iter()
            .enumerate()
            .map(|(i, e)| ImportLine {
                line: i as u64 + 1,
                text: serde_json::to_string(e).unwrap(),
            })
            .collect(),
    };
    let r = crate::import::write::import_batch(&core.store, &p, "cli").unwrap();
    assert_eq!(r.imported, episodes.len() as u64, "{r:?}");
    episodes
        .iter()
        .map(|e| crate::import::session_id_of(e["episode_id"].as_str().unwrap()))
        .collect()
}

/// The model calls `memory_lookup` with the JSON after `LOOKUP `.
fn script(req: &ProviderRequest) -> Scripted {
    if answers_a_call(req) {
        return Scripted::text("Done.");
    }
    let last = last_user(req);
    match last.find("LOOKUP ") {
        Some(i) => {
            let rest = &last[i + 7..];
            let input: Value = serde_json::from_str(&rest[..=rest.rfind('}').unwrap()]).unwrap();
            Scripted::tools("Looking.", &[("m1", "memory_lookup", input)])
        }
        None => Scripted::text("Done."),
    }
}

async fn call(core: &Arc<Core>, sid: &str, input: Value) -> Result<String, String> {
    crate::memory_lookup::call(core.clone(), sid, input)
        .await
        .map(|o| o.text)
}

/// The tool is in a private place's catalog and absent from a shared one's;
/// called there anyway, the gate refuses it and nothing of memory comes back.
#[tokio::test]
async fn a_shared_place_is_never_offered_the_lookup_and_the_gate_refuses_it() {
    let r = rig_scripted(|_| {}, script);
    import(&r.core, &episodes());
    let private = session(&r.core, None);
    let shared = session(&r.core, Some(&format!("channel:{LAB}")));
    turn(&r.core, &private, r#"LOOKUP {"when": "March 2026"}"#).await;
    let reqs = r.requests();
    assert!(offered(&reqs[0]).contains(&"memory_lookup".to_string()));
    let got = result_of(&r.core, &private, "m1");
    assert!(got.contains("4 imported episodes in March 2026"), "{got}");
    assert!(got.contains(SHOWN), "{got}");

    let before = r.requests().len();
    turn(&r.core, &shared, r#"LOOKUP {"when": "March 2026"}"#).await;
    let reqs = r.requests();
    assert!(!offered(&reqs[before]).contains(&"memory_lookup".to_string()));
    let got = result_of(&r.core, &shared, "m1");
    assert!(
        got.starts_with("Not run: memory.lookup is not offered in a shared place"),
        "{got}"
    );
    assert!(refused(&r.core)
        .iter()
        .any(|(id, why)| id == "m1" && why.starts_with("place: ")));
    for req in &reqs[before..] {
        let all = serde_json::to_string(&req.messages).unwrap();
        assert!(!all.contains(SHOWN) && !all.contains(HIDDEN), "{all}");
    }
}

/// Past the gate, the tool checks the asking session's place itself: a
/// shared place's session gets nothing, whatever it asks.
#[tokio::test]
async fn the_tool_refuses_a_shared_session_itself() {
    let r = rig_scripted(|_| {}, script);
    import(&r.core, &episodes());
    let shared = session(&r.core, Some(&format!("channel:{LAB}")));
    for input in [
        json!({"when": "March 2026"}),
        json!({"words": "parser"}),
        json!({"book": "casebook"}),
    ] {
        let e = call(&r.core, &shared, input).await.unwrap_err();
        assert!(e.contains("this place is shared"), "{e}");
    }
}

/// An imported item whose episode the catalog does not name (its labels
/// unread) is veiled, not shown as a native one: the veil fails closed.
#[tokio::test]
async fn an_imported_item_without_its_labels_stays_veiled() {
    let r = rig_scripted(|_| {}, script);
    import(&r.core, &episodes());
    let stray = crate::import::session_id_of(&format!("ep_{:064x}", 0xdead));
    let item = theseus_protocol::memory::RecallItem {
        node_id: "nod_stray".into(),
        session_id: stray.clone(),
        kind: "imported".into(),
        text: Some(format!("Message 9: {HIDDEN}.")),
        ..Default::default()
    };
    let got = crate::memory_lookup::rendered(&r.core, &[item])
        .await
        .unwrap();
    assert!(got.contains(&stray), "{got}");
    assert!(!got.contains(HIDDEN), "{got}");
    assert!(got.contains("labels unread · veiled"), "{got}");
}

/// A personal or a partner-confidential episode's text is veiled, as the
/// cockpit's Context page veils it: its date, session, source and label
/// show, its text never does. The others' text shows.
#[tokio::test]
async fn veiled_text_stays_veiled() {
    let r = rig_scripted(|_| {}, script);
    let ids = import(&r.core, &episodes());
    let cli = session(&r.core, None);
    let got = call(&r.core, &cli, json!({"when": "March 2026"}))
        .await
        .unwrap();
    assert!(!got.contains(HIDDEN), "{got}");
    assert!(got.contains(SHOWN), "{got}");
    for (id, label) in [(&ids[0], "personal"), (&ids[2], "partner-confidential")] {
        let line = got
            .lines()
            .find(|l| l.contains(id.as_str()))
            .unwrap_or_else(|| panic!("{got}"));
        assert!(
            line.contains(label) && line.contains("imported from claude-cli"),
            "{line}"
        );
    }
    assert_eq!(
        got.matches("veiled: its text stays veiled").count(),
        2,
        "{got}"
    );
    // By book and by topic too.
    for input in [
        json!({"book": "casebook"}),
        json!({"topic": "kelp/notes", "limit": 25}),
    ] {
        let got = call(&r.core, &cli, input).await.unwrap();
        assert!(!got.contains(HIDDEN) && got.contains(SHOWN), "{got}");
    }
}

/// The local month an instant falls in, `2026-03`, in the daemon's zone.
fn local_month(ms: u64) -> String {
    crate::judge::spend::local_day(ms)[..7].to_string()
}

/// A month keeps to its episodes, by the daemon's time zone: an episode
/// near a month's edge is in the month it falls in there. A range by `from`
/// and `to` keeps to its days; `cursor` and `limit` page.
#[tokio::test]
async fn months_and_ranges_keep_to_their_episodes() {
    let r = rig_scripted(|_| {}, script);
    let mut eps = episodes();
    // 2026-04-01T03:30Z: March in the Americas, April at UTC and east of it.
    eps.push(episode(
        5,
        "public",
        "2026-04-01T03:30:00Z",
        "the edge of the month",
    ));
    let ids = import(&r.core, &eps);
    let cli = session(&r.core, None);
    let edge = crate::wake::parse_at("2026-04-01T03:30:00Z").unwrap();
    let in_march = local_month(edge) == "2026-03";
    let got = call(&r.core, &cli, json!({"when": "March 2026", "limit": 25}))
        .await
        .unwrap();
    let want = 4 + usize::from(in_march);
    assert!(
        got.contains(&format!("{want} imported episodes in March 2026")),
        "{got}"
    );
    assert_eq!(got.contains(&ids[5]), in_march, "{got}");
    assert!(!got.contains(&ids[4]), "September: {got}");
    let got = call(&r.core, &cli, json!({"when": "April 2026"}))
        .await
        .unwrap();
    assert_eq!(got.contains(&ids[5]), !in_march, "{got}");
    // A range of days: the 9th to the 18th holds episodes 1 and 2.
    let got = call(
        &r.core,
        &cli,
        json!({"from": "2026-03-09", "to": "2026-03-18"}),
    )
    .await
    .unwrap();
    assert!(got.contains("2 imported episodes"), "{got}");
    assert!(
        got.contains(&ids[1]) && got.contains(&ids[2]) && !got.contains(&ids[0]),
        "{got}"
    );
    // Pages of one, newest first, by cursor, short of the month's edge.
    let page = |cursor: u64| {
        let core = r.core.clone();
        let cli = cli.clone();
        async move {
            call(
                &core,
                &cli,
                json!({"from": "2026-03-01", "to": "2026-03-30", "limit": 1, "cursor": cursor}),
            )
            .await
            .unwrap()
        }
    };
    let first = page(0).await;
    assert!(
        first.contains(&ids[3]) && first.contains("More: call again with cursor 1"),
        "{first}"
    );
    let last = page(3).await;
    assert!(
        last.contains(&ids[0]) && last.contains("that is all"),
        "{last}"
    );
    // A span that ends before it begins, and words that name no time.
    let e = call(
        &r.core,
        &cli,
        json!({"from": "2026-03-20", "to": "2026-03-01"}),
    )
    .await
    .unwrap_err();
    assert!(e.contains("ends before it begins"), "{e}");
    let e = call(&r.core, &cli, json!({"when": "soon"}))
        .await
        .unwrap_err();
    assert!(e.contains("names no time"), "{e}");
    let e = call(&r.core, &cli, json!({})).await.unwrap_err();
    assert!(e.contains("name something to look up"), "{e}");
}

/// A budget ends a page early, and the cursor says where the next begins.
#[tokio::test]
async fn a_small_budget_ends_the_page_early() {
    let r = rig_scripted(|_| {}, script);
    import(&r.core, &episodes());
    let cli = session(&r.core, None);
    let got = call(
        &r.core,
        &cli,
        json!({"when": "March 2026", "budget_tokens": 100}),
    )
    .await
    .unwrap();
    assert!(
        got.contains("Shown 1–1 of 4. More: call again with cursor 1."),
        "{got}"
    );
}

/// A stand-in index over every imported node: each with its own time,
/// whatever the span (the core keeps to it).
fn index_over(core: &Arc<Core>, sessions: Vec<String>) -> Ask {
    let store = core.store.clone();
    Arc::new(move |_p| -> AskFuture {
        let nodes: Vec<(u64, Node)> = sessions
            .iter()
            .flat_map(|s| store.session_nodes(s).unwrap())
            .collect();
        let hits = nodes
            .into_iter()
            .enumerate()
            .map(|(i, (position, n))| IndexHit {
                text: crate::recall::text_of(&n),
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
                sources: [(
                    "bm25".to_string(),
                    IndexSourceRank {
                        rank: i + 1,
                        score: 1.0,
                    },
                )]
                .into(),
                fused: 1.0 / (61 + i) as f64,
            })
            .collect();
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

/// Words go through recall's pipeline: each item dated by its message
/// (inside its episode's span), kept to a month when one is named, and
/// veiled as the catalog's are.
#[tokio::test]
async fn words_are_recalled_dated_and_veiled() {
    let r = rig_scripted(|_| {}, script);
    let ids = import(&r.core, &episodes());
    r.core
        .runner
        .memory
        .set_ask(index_over(&r.core, ids.clone()));
    let cli = session(&r.core, None);
    let got = call(
        &r.core,
        &cli,
        json!({"words": "parser", "when": "March 2026", "limit": 25}),
    )
    .await
    .unwrap();
    assert!(
        got.contains("recall's items for \"parser\" in March 2026"),
        "{got}"
    );
    assert!(!got.contains(&ids[4]), "September: {got}");
    assert!(!got.contains(HIDDEN) && got.contains(SHOWN), "{got}");
    let day =
        crate::judge::spend::local_day(crate::wake::parse_at("2026-03-10T18:00:00Z").unwrap());
    let line = got.lines().find(|l| l.contains(&ids[1])).unwrap();
    assert!(line.contains(&format!("{day} (episode {day})")), "{line}");
    assert!(
        line.contains("company-confidential") && line.contains("book casebook"),
        "{line}"
    );
    // Without a span, September comes back too.
    let got = call(&r.core, &cli, json!({"words": "renderer", "limit": 25}))
        .await
        .unwrap();
    assert!(got.contains(&ids[4]), "{got}");
}

/// A search of words whose vector search fails (or is late) answers from
/// the word sources alone, `words_only` (recall-fallback, theseus-w9qv's
/// join): the lookup takes their hits as an answer, not as an index that
/// did not answer.
#[tokio::test]
async fn words_only_hits_are_an_answer() {
    let r = rig_scripted(|_| {}, script);
    let ids = import(&r.core, &episodes());
    let words = index_over(&r.core, ids.clone());
    r.core.runner.memory.set_ask(Arc::new(
        move |p: theseus_protocol::index::IndexQueryParams| -> AskFuture {
            if p.sources.iter().any(|s| s == "vector") {
                return Box::pin(async { Err("the query would not embed".to_string()) });
            }
            words(p)
        },
    ));
    let cli = session(&r.core, None);
    let got = call(
        &r.core,
        &cli,
        json!({"words": "parser", "when": "March 2026"}),
    )
    .await
    .unwrap();
    assert!(got.contains("recall's items for \"parser\""), "{got}");
    assert!(got.contains(SHOWN) && !got.contains(&ids[4]), "{got}");
}
