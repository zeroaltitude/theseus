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
/// tools, theseus-n8gk, whose first compile then rang), and 41,000 and 35,500
/// before `wake.cancel` and `wake.move` and `wake.at`'s sentence about them joined
/// the tools block (theseus-lherl: 700 more of window; 41,300 rang again), and
/// 41,700 and 36,200 before `task_create`'s `start_at` and `start_after` joined it
/// (theseus-rcgph: 200 more), and 41,900 and 36,400 before `wake.at`'s `for` and
/// `say_only` joined it (theseus-8ux6x: 300 more), and 42,200 and 36,700
/// before `routine.create` (theseus-pvm5c: 600 more), `watch` (theseus-f2r2b:
/// 600 more) and `thread.read` (theseus-f2g0: 300 more) joined it (43,400 and
/// 37,900 rang with all three; their joins' review), and 43,700 and 38,200 before
/// standing-surface (theseus-slvl.4) and wait-cost (theseus-7m7w.3) joined (43,700
/// rang with both; 43,800 to 44,200 hold: a sweep at their join).
/// And 43,900 and 38,400 before `extend.propose`'s `hands` joined it (theseus-pw1q.14: 43,900 and
/// 44,100 rang; 44,300 to 44,900 hold: a sweep at its join), and 44,500 before owner-ask's
/// `owner_ask` and `task_create`'s `notify_done` joined it (theseus-j7cje: 44,300, 44,500 and
/// 45,100 rang; 44,700 and 44,900 hold: a sweep at its join), and 44,800 before projects'
/// `task_create` fields and `project.attach` joined it (theseus-n5ui2: no window held with
/// the first answer billed at 38,600, 43,800 to 45,000 rang and 45,100 and up never compacted
/// the second loop; with it billed at 39,400, 45,200 and 45,600 hold, 44,800 and 46,000 do
/// not: a sweep at its join), and 45,400 before modes' `plan_present` and `plan_step` joined the
/// catalog (theseus-lyxy: deferred in the `plan` family, so no request sends them, but this rig's
/// catalog counts them whole; billed at 39,400 no window held from 45,400 to 46,600; billed at
/// 40,200, 45,800 and 46,200 hold, 45,400 and 46,600 do not: a sweep at its join).
const WINDOW: u64 = 46_000;
const OUTPUT: u32 = 1_000;

/// The input the first answer is billed at, past the request budget (38,600
/// before projects joined, theseus-n5ui2, and 39,400 before modes, theseus-lyxy:
/// the band above).
const BILLED: u64 = 40_200;

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
