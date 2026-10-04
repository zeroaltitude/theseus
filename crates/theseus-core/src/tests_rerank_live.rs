//! Jev's rerank of recall made live, bounded, and taught by the owner's
//! labels (M6 step 32d; design §2.7, §2.9, §2.12, §2.14), through whole cores
//! against the fake Jev and a stand-in index, as `tests_rerank` does for
//! 32c's shadow arm.

use theseus_judge::fake::{FakeJev, Scripted as Jev};
use theseus_protocol::memory::MemoryLabelParams;

use crate::config::MemoryMode;
use crate::tests_rerank::{heron_rig, keys_of, turn, until_reranked};

/// A note the owner labeled `wrong` or `stale` never reaches Jev's rerank
/// request, nor comes back through its repack (theseus-mm4a): its key and
/// its text are in neither the state Jev was sent nor the row's admitted
/// lists, though Jev would rank it first. `recall_end` hands the labels over
/// (`Recalled.labeled`); step 32d's live path will too.
async fn a_labeled_note_never_reaches_jev(live: bool) {
    for word in ["wrong", "stale"] {
        let jev = FakeJev::start().unwrap();
        // Were the heron's note asked about, Jev would put it first.
        for k in 1..=3 {
            jev.script(&format!("helps.{k}"), Jev::Noul(0.97));
        }
        let (r, here, order) = heron_rig(&jev, |c| {
            if live {
                c.memory.mode = MemoryMode::Live;
            }
        });
        let c = &r.core;
        let heron = c.store.session_nodes(&order[2]).unwrap()[0].1.id.clone();
        let key = format!("{heron}#0");
        c.memory_label(
            &MemoryLabelParams {
                node_id: heron.clone(),
                label: word.into(),
                recall_id: None,
                note: None,
            },
            "cli",
        )
        .unwrap();
        turn(c, &here, "Where does the grey heron nest?").await;
        let rows = until_reranked(&c.store, 1).await;
        let seen = jev.seen();
        assert_eq!(seen.len(), 1, "{word}: one rerank");
        let state = serde_json::to_string(&seen[0].body).unwrap();
        assert!(!state.contains(&heron), "{word}: the node's id reached Jev");
        assert!(
            !state.contains("grey heron nests"),
            "{word}: the note's text reached Jev"
        );
        let rr = &rows[0].data["context"]["rerank"];
        assert_eq!(rr["eligible"], 2, "{word}: {rr}");
        for list in ["fused_admitted", "reranked_admitted", "top"] {
            assert!(
                !keys_of(&rr[list]).contains(&key),
                "{word}: {list} holds the labeled note: {rr}"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_labeled_note_never_reaches_a_shadow_rerank() {
    a_labeled_note_never_reaches_jev(false).await;
}
