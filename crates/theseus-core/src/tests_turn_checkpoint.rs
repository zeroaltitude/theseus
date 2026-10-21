//! A routed turn's checkpoints (theseus-a1wvq): a conversation's turn that
//! the route put on a cheap profile, and whose loops keep failing, asks the
//! route pack about its recent state at a loop boundary as a task does
//! (`tests_task_checkpoint.rs`), and moves up. A "set up X" message reads
//! routine and turns out deep; without this, every loop ran on the first
//! guess. A turn the owner pinned asks nothing.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted as Jev};

use crate::provider::{FakeProvider, Scripted};
use crate::tests_route::{decided, mode, rig_parts, turn, Rig};

/// A loop whose one tool call fails: a call with no input is invalid.
fn failing(n: usize) -> Scripted {
    Scripted::tools(
        &format!("Trying approach {n}."),
        &[(&format!("f{n}"), "text_diff", json!({}))],
    )
}

/// `n` failing loops, then the answer.
pub(crate) fn loops(n: usize) -> Vec<Scripted> {
    let mut s: Vec<Scripted> = (0..n).map(failing).collect();
    s.push(Scripted::text("Done."));
    s
}

pub(crate) const ASK: &str = "Set up the build for the parser crate.";

/// Each session's reads of its turn's checkpoint state
/// (`checkpoints_for`), counted.
pub(crate) static READS: Mutex<BTreeMap<String, usize>> = Mutex::new(BTreeMap::new());

/// A conversation's rig whose route reads every message as routine coding
/// (Haiku) and whose checkpoint, on three failed loops, says deep coding
/// (Sonnet), on a provider that takes 60 ms a loop.
fn routed(tweak: impl FnOnce(&mut crate::Config)) -> (Rig, Arc<FakeProvider>, FakeJev) {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "routine_coding", 0.95);
    jev.script_when(
        "failing: 3 failed",
        None,
        "mode",
        Jev::Choice {
            option: "deep_coding".into(),
            confidence: 0.95,
        },
    );
    let claude = Arc::new(FakeProvider {
        delay_ms: 60,
        ..FakeProvider::scripted(loops(8))
    });
    let provider = claude.clone();
    let rig = rig_parts(
        Some(&jev),
        4,
        |c| {
            c.routing.classes.deep_coding.profiles = vec!["sonnet".into()];
            tweak(c);
        },
        move |p| {
            p.providers.insert("anthropic".into(), provider);
        },
    );
    (rig, claude, jev)
}

fn models(claude: &FakeProvider) -> Vec<&'static str> {
    claude
        .requests()
        .iter()
        .map(|r| match r.model.as_str() {
            "claude-haiku-5-5" => "haiku",
            "claude-sonnet-5-5" => "sonnet",
            _ => "other",
        })
        .collect()
}

pub(crate) fn checkpoints(rig: &Rig) -> Vec<Value> {
    decided(&rig.core.store)
        .into_iter()
        .filter(|d| {
            d["source"]
                .as_str()
                .is_some_and(|s| s.starts_with("turn checkpoint"))
        })
        .collect()
}

/// A routine verdict whose loops fail three times in a row moves the turn to
/// Sonnet at a later loop boundary, recorded as the turn's checkpoint.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_routed_turn_whose_loops_fail_moves_up() {
    let (rig, claude, _jev) = routed(|_| {});
    let res = turn(&rig.core, None, ASK, None).await;
    let models = models(&claude);
    assert_eq!(models.len(), 9, "{}", res.output);
    assert!(models[..3].iter().all(|m| *m == "haiku"), "{models:?}");
    let at = models
        .iter()
        .position(|m| *m == "sonnet")
        .expect("it moved");
    assert!((3..=6).contains(&at), "{models:?}");
    assert!(models[at..].iter().all(|m| *m == "sonnet"), "{models:?}");
    let rows = checkpoints(&rig);
    assert_eq!(
        (
            rows[0]["trigger"].as_str(),
            rows[0]["from"].as_str(),
            rows[0]["profile"].as_str()
        ),
        (Some("failing"), Some("haikuhi"), Some("sonnet")),
        "{rows:?}"
    );
}

/// With checkpoints off, the same turn runs every loop on its first guess,
/// as every routed turn did before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_checkpoints_off_a_routed_turn_stays_on_its_first_guess() {
    let (rig, claude, _jev) = routed(|c| c.routing.task_checkpoints.enabled = false);
    turn(&rig.core, None, ASK, None).await;
    assert_eq!(models(&claude), ["haiku"; 9]);
    assert!(checkpoints(&rig).is_empty());
}

/// A turn on the owner's pinned profile runs every loop there and asks no
/// checkpoint, whatever its loops do.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pinned_turn_asks_no_checkpoint() {
    let (rig, claude, _jev) = routed(|_| {});
    turn(&rig.core, None, ASK, Some("haikuhi")).await;
    assert_eq!(models(&claude), ["haiku"; 9]);
    assert!(checkpoints(&rig).is_empty());
}

/// A turn no checkpoint is for reads its state once: its later loops pay one
/// `match`, never the read again (theseus-53ujg.3).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_ineligible_turn_pays_one_match() {
    let (rig, claude, _jev) = routed(|_| {});
    let res = turn(&rig.core, None, ASK, Some("haikuhi")).await;
    assert_eq!(models(&claude).len(), 9);
    let reads = READS.lock().unwrap()[&res.session_id];
    assert_eq!(reads, 1);
}
