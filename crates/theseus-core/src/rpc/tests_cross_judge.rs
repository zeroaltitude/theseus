//! The cross judge's world (theseus-98bz.10): its tests went with the thread
//! distance's recall step (p17: anything goes); these helpers stay while the
//! thread link's tests read them, and go with them.

use std::sync::Arc;

use theseus_judge::fake::FakeJev;
use theseus_protocol::threads::ThreadSealParams;

use super::tests_thread_recall::{cli, new_thread, say, scored, thread_session};
use super::Core;
use crate::config::{Config, MemoryMode, PackMode};
use crate::judge::cross::CROSS_PACK;
use crate::tests_rerank::{pack_mode, rig, sent, Rig};

pub(super) const CUTOFF: &str = "the flagging cutoff for the datatidy report is 0.55 from now on";
const PORT: &str = "the homesvc staging copy listens on port 8453";
const ZIRCON: &str = "the zircon embargo lifts after the ledgerweb disclosure";

/// Recall live, rerank off and this pack live; the lens alone would drop
/// every other stream's note here (`own_top_admissible` off, so the
/// thread's own note in context outranks it).
pub(super) fn rigged(jev: &FakeJev, tweak: impl FnOnce(&mut Config)) -> Rig {
    rig(Some(jev), |c| {
        c.memory.mode = MemoryMode::Live;
        c.memory.recall_max_items = 5;
        c.threads.own_top_admissible = false;
        pack_mode(c, crate::judge::rerank::RERANK_PACK, PackMode::Off);
        pack_mode(c, CROSS_PACK, PackMode::Live);
        tweak(c);
    })
}

/// The pipeline thread asks; `stats` holds the cutoff, `ops` the port, and
/// `embargo`, sealed, its own fact. Returns the asking session.
pub(super) fn world(c: &Arc<Core>, extra: usize) -> String {
    for (slug, title) in [
        ("pipeline", "datatidy pipeline"),
        ("stats", "datatidy stats"),
        ("ops", "homesvc maintenance"),
        ("embargo", "ledgerweb fix"),
    ] {
        new_thread(c, slug, title);
    }
    let pipe = thread_session(c, "pipeline");
    say(c, &pipe, "the datatidy report groups rows by region");
    let stats = thread_session(c, "stats");
    say(c, &stats, CUTOFF);
    for k in 0..extra {
        say(
            c,
            &stats,
            &format!("the datatidy report's cutoff draft number {k}"),
        );
    }
    let ops = thread_session(c, "ops");
    say(c, &ops, PORT);
    let embargo = thread_session(c, "embargo");
    say(c, &embargo, ZIRCON);
    c.thread_seal(
        &ThreadSealParams {
            slug: "embargo".into(),
            reason: Some("before disclosure".into()),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    let mut picks = vec![
        (pipe.clone(), 0, 0.104),
        (stats.clone(), 0, 0.097),
        (ops, 0, 0.096),
        (embargo, 0, 0.099),
    ];
    for k in 0..extra {
        picks.push((stats.clone(), k + 1, 0.095 - k as f64 * 0.001));
    }
    c.runner.memory.set_ask(scored(c, picks));
    pipe
}

pub(super) fn cross_requests(jev: &FakeJev) -> Vec<String> {
    jev.seen()
        .iter()
        .map(|s| serde_json::to_string(&s.body).unwrap())
        .filter(|b| b.contains("recall_cross.v1/"))
        .collect()
}

pub(super) fn admitted_texts(r: &Rig) -> String {
    sent(&r.model).last().cloned().unwrap_or_default()
}

/// A node's text, read back.
pub(super) fn sent_text(c: &Core, node: &str) -> String {
    c.store
        .get_node(node)
        .ok()
        .flatten()
        .map(|(_, n)| crate::recall::text_of(&n))
        .unwrap_or_default()
}
