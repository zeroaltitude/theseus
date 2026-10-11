//! A routed turn's recall drops reach one compile (theseus-y9p4). Recall's
//! pack has room for one of two heron notes, so it drops the other for its
//! budget; the first compile carries that drop while routing decides
//! (`recall_compiled` clones it, theseus-3urn), and `keep_first` lets it go
//! once that compile is the one the call uses. The turn has a second loop (a
//! `fs.read` after the first answer), and that loop's compile is a new
//! compilation, so its budget is stored and can be read: it names what the
//! second loop's own compaction dropped, never the recall's drop again. The
//! window is a catalog override on the models the turn runs on, small enough
//! that the first answer's billed input passes it, so the second loop's compile
//! is a compaction on the routed model's own window.
//!
//! The rig is `tests_route`'s, a verdict that keeps the first compile
//! (`chat`, on sonnet) and one that switches (`sophisticated`, to opus).
//! Today `keep_first`'s clear alone does not change what these read: the
//! provider call's dispatch ends the pending recall node first, and
//! `recall_compiled` reads the drops only for a pending node, so removing the
//! clear changes none of these assertions (an equivalent plant). Removing it
//! together with the dispatch's `t.recall.pending = None` fails the `chat`
//! half: the second loop's budget names the recall's drop again. The test
//! holds that pair.

use serde_json::json;
use theseus_judge::fake::FakeJev;

use crate::compiler::Compilation;
use crate::ledger::LedgerRow;
use crate::provider::Scripted;
use crate::tests_route::{mode, rig, turn};

/// The window the turn's models get, and their output cap: a request budget of
/// 35,904 tokens (40,000 and 34,500 before the job tools joined the request's
/// tools, theseus-n8gk, whose first compile then rang).
const WINDOW: u64 = 41_000;
const OUTPUT: u32 = 1_000;

/// The input the first answer is billed at, past the request budget.
const BILLED: u64 = 35_500;

/// The tiers of what a compilation's budget report dropped.
fn tiers(c: &Compilation) -> Vec<String> {
    let b = c.budget.as_ref().expect("a budget report");
    b.dropped.iter().map(|d| d.tier.clone()).collect()
}

async fn two_loops(verdict: &str, model: &str) -> (Vec<Compilation>, Vec<String>) {
    let jev = FakeJev::start().unwrap();
    let work = tempfile::tempdir().unwrap();
    let work = work.path().canonicalize().unwrap();
    let r = rig(Some(&jev), 0, |c| {
        c.tools.projects_dir = Some(work.to_string_lossy().into_owned());
        c.memory.mode = crate::config::MemoryMode::Live;
        c.memory.recall_budget_tokens = 30;
        for m in ["claude-sonnet-5-5", "claude-opus-5-5"] {
            c.catalog.insert(
                m.into(),
                crate::catalog::CatalogRow {
                    context_window: Some(WINDOW),
                    max_output_tokens: Some(OUTPUT),
                    ..Default::default()
                },
            );
        }
    });
    std::fs::write(
        work.join("a.txt"),
        "the heron stood on one leg. ".repeat(580),
    )
    .unwrap();
    {
        let mut s = r.claude.script.lock().unwrap();
        s.push_back(Scripted::Billed {
            usage: theseus_protocol::Usage {
                input_tokens: BILLED,
                output_tokens: 50,
                ..Default::default()
            },
            then: Box::new(Scripted::tools(
                "",
                &[("r1", "fs_read", json!({"path": "a.txt"}))],
            )),
        });
        // The compaction's summary, asked of the summary profile, then the
        // second loop's answer.
        s.push_back(Scripted::text("A summary of the boats and the reeds."));
        s.push_back(Scripted::text("The heron nests by the weir."));
    }
    let notes = crate::tests_recall::session(
        &r.core,
        None,
        &[
            "the heron nests by the weir in spring, and the reeds there are cut back each autumn",
            "the heron fishes at dawn by the mill, where the race runs shallow over the stones",
        ],
    );
    let ask = crate::tests_recall::index_of(&r.core, vec![notes]);
    r.core.runner.memory.set_ask(ask);
    // Earlier messages the compaction can leave out.
    let main = crate::tests_recall::session(
        &r.core,
        None,
        &[
            &"the first thing said was about boats. ".repeat(430),
            &"the second thing said was about reeds. ".repeat(430),
        ],
    );
    mode(&jev, verdict, 0.95);
    let res = turn(
        &r.core,
        Some(&main),
        "Where does the heron nest and fish?",
        None,
    )
    .await;
    assert_eq!(res.loops, 2, "{verdict}");
    assert_eq!(res.recalled, 1, "{verdict}");
    let asked: Vec<String> = r
        .claude
        .requests()
        .iter()
        .map(|q| q.model.clone())
        .collect();
    assert_eq!(
        asked, [model; 3],
        "{verdict}: both loops and the summary on {model}"
    );
    let all: Vec<(u64, LedgerRow)> = r.core.store.ledger_tail(500).unwrap();
    let compiled: Vec<Compilation> = all
        .into_iter()
        .filter(|(_, row)| row.kind == "context.compiled")
        .map(|(_, row)| {
            let id = row.data["compilation_id"].as_str().unwrap().to_string();
            r.core.store.get_compilation(&id).unwrap().unwrap()
        })
        .collect();
    let strategies = compiled.iter().map(|c| c.strategy.clone()).collect();
    (compiled, strategies)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_kept_first_compile_holds_the_recall_drops_in_its_loop_alone() {
    for (verdict, model) in [
        ("chat", "claude-sonnet-5-5"),
        ("sophisticated", "claude-opus-5-5"),
    ] {
        let (compiled, strategies) = two_loops(verdict, model).await;
        assert_eq!(compiled.len(), 2, "{verdict}: {strategies:?}");
        assert_eq!(strategies[1], "compaction", "{verdict}: {strategies:?}");
        assert_eq!(tiers(&compiled[0]), ["recall"], "{verdict}: loop 0");
        assert_eq!(tiers(&compiled[1]), ["compaction"], "{verdict}: loop 1");
    }
}
