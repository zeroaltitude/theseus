//! A fresh install recalls (theseus-is8t): a config that says nothing in
//! `[memory]` runs `live` on `baseline`, so a private place's turn gets a
//! recall in front of its model, and a shared place's draws on its own
//! sessions alone (the place rule, unchanged). `tests_recall` runs the rest
//! of the place rule over generated stores.

use std::sync::{Arc, Mutex};

use theseus_protocol::index::IndexQueryParams;

use crate::config::memory::MemoryArm;
use crate::config::{MemoryConfig, MemoryMode};
use crate::recall::AskFuture;
use crate::tests_recall::{admitted, index_of, recalls, rig_with, session, turn, PIER};

const KITE: &str = "Remember: the red kite roosts in the beeches above Tarnwick.";

/// The config's whole `[memory]` as a fresh install has it, whatever the
/// rig set first.
fn fresh(c: &mut crate::Config) {
    c.memory = MemoryConfig::default();
}

/// With nothing in `[memory]`: a turn in a private place (the CLI's) asks the
/// index for `baseline`'s sources, and its recall reaches the model.
#[tokio::test]
async fn a_fresh_config_recalls_live_on_baseline_in_a_private_place() {
    let r = rig_with(MemoryMode::Off, fresh);
    let c = &r.core;
    assert_eq!(
        (c.cfg.memory.mode, c.cfg.memory.arm),
        (MemoryMode::Live, MemoryArm::Baseline)
    );
    let past = session(c, None, &[KITE]);
    let now = session(c, None, &[]);
    let asked = Arc::new(Mutex::new(Vec::new()));
    let inner = index_of(c, vec![past.clone()]);
    let seen = asked.clone();
    c.runner
        .memory
        .set_ask(Arc::new(move |p: IndexQueryParams| -> AskFuture {
            seen.lock().unwrap().push(p.clone());
            inner(p)
        }));
    let res = turn(c, &now, "Where does the red kite roost?").await;
    assert_eq!(res.recalled, 1, "the recall reached the model");
    let rows = recalls(c, &now);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].mode.as_str(), rows[0].arm.as_deref()),
        ("live", Some("baseline"))
    );
    assert_eq!(admitted(&rows[0]), [past.as_str()]);
    assert_eq!(
        asked.lock().unwrap()[0].sources,
        ["bm25", "entity", "vector"]
    );
}

/// And a shared place's turn, on the same fresh config, is offered only its
/// own sessions: the CLI's session is dropped by the place rule, and no
/// recall reaches the model.
#[tokio::test]
async fn a_fresh_config_keeps_a_shared_place_to_its_own_sessions() {
    let r = rig_with(MemoryMode::Off, fresh);
    let c = &r.core;
    let cli = session(c, None, &[KITE]);
    let pier = session(c, Some(&format!("channel:{PIER}")), &[]);
    c.runner.memory.set_ask(index_of(c, vec![cli.clone()]));
    let res = turn(c, &pier, "Where does the red kite roost?").await;
    assert_eq!(
        res.recalled, 0,
        "nothing of the CLI's reached a shared place"
    );
    let rows = recalls(c, &pier);
    assert_eq!(rows[0].mode, "live");
    assert!(admitted(&rows[0]).is_empty(), "{:?}", admitted(&rows[0]));
    assert_eq!(
        crate::tests_recall::dropped_for(&rows[0], "place"),
        [cli.as_str()]
    );
}

/// An existing config that sets `mode` keeps its value: `off` recalls
/// nothing and asks the index nothing.
#[tokio::test]
async fn a_config_that_sets_mode_off_keeps_it() {
    let r = rig_with(MemoryMode::Off, |c| c.memory.mode = MemoryMode::Off);
    let c = &r.core;
    let past = session(c, None, &[KITE]);
    let now = session(c, None, &[]);
    let asked = Arc::new(Mutex::new(0u32));
    let inner = index_of(c, vec![past]);
    let seen = asked.clone();
    c.runner
        .memory
        .set_ask(Arc::new(move |p: IndexQueryParams| -> AskFuture {
            *seen.lock().unwrap() += 1;
            inner(p)
        }));
    let res = turn(c, &now, "Where does the red kite roost?").await;
    assert_eq!(res.recalled, 0);
    assert_eq!(*asked.lock().unwrap(), 0, "off asks the index nothing");
    assert!(recalls(c, &now).is_empty());
}
