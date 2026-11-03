//! The fan-out cue (theseus-z9mze.4, promoted by theseus-h66v4.6; `turn/route_step/fan_out_cue.rs`):
//! a `fan_out.cue` row for every verdict that carries Jev's `split` answer, fired or not, and on a
//! fire one tail line (the owner's hint). The rig is route.v10's (`tests_route_open::open`): the
//! family is asked beside the route pack, and the turn waits for it.

use serde_json::Value;
use theseus_judge::fake::{FakeJev, Scripted as Jev};

use crate::provider::ProviderRequest;
use crate::tests_judge::kinds;
use crate::tests_route::turn;
use crate::tests_route_open::{open, says};

/// The cue's line is the one tail block that begins with the owner's hint.
const LINE: &str =
    "This request looks parallelizable: consider starting tasks for the independent parts.";

fn is_line(b: &Value) -> bool {
    b["text"].as_str().is_some_and(|t| t.starts_with(LINE))
}

fn carries(req: &ProviderRequest) -> bool {
    req.messages.iter().any(|m| {
        m["content"]
            .as_array()
            .is_some_and(|b| b.iter().any(is_line))
    })
}

const ASK: &str = "Go through every mooring on the outer wall and say which were checked.";
const PLAIN: &str = "Which moorings on the outer wall were checked?";

/// Jev's `split` answer, and its `work` answer beside it.
fn split(jev: &FakeJev, option: &str, confidence: f64) {
    jev.script(
        "split",
        Jev::Choice {
            option: option.into(),
            confidence,
        },
    );
}

fn rows(r: &crate::tests_route::Rig) -> Vec<Value> {
    kinds(&r.core.store, "fan_out.cue")
        .into_iter()
        .map(|r| r.data)
        .collect()
}

/// The key test: Jev's `splits_by_area` fires the cue and `one_area` does not, whatever words the ask
/// uses; each is a row, and the line is the request's last block with the system and tools byte for
/// byte as an ordinary turn's (it is no part of the cached prefix).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn jevs_label_fires_the_cue_not_the_words_and_moves_no_prefix_byte() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "sonnet", "high", 0.95);
    let o = open(&jev, 2, |_| {});
    let r = &o.r;
    split(&jev, "one_area", 0.95);
    // The ask is full of the words the deleted list fired on; the label says one area.
    let one = turn(&r.core, None, ASK, None).await;
    split(&jev, "splits_by_area", 0.95);
    // And the second ask uses none of them.
    let _ = turn(&r.core, Some(&one.session_id), PLAIN, None).await;
    let q = r.claude.requests();
    let (a, b) = (&q[0], &q[1]);
    assert!(!carries(a), "one_area is no cue, whatever the words");
    assert!(carries(b), "splits_by_area is the cue, whatever the words");
    assert_eq!(
        serde_json::to_string(&a.system).unwrap(),
        serde_json::to_string(&b.system).unwrap()
    );
    assert_eq!(a.tools, b.tools, "the tools of the first request");
    let last = b.messages.last().unwrap()["content"]
        .as_array()
        .unwrap()
        .last()
        .cloned();
    assert!(is_line(&last.unwrap()), "in the tail");
    let got = rows(r);
    assert_eq!(
        got.len(),
        2,
        "one row a verdict that carries a split answer"
    );
    assert_eq!(
        (got[0]["split"].as_str(), got[0]["fired"].as_bool()),
        (Some("one_area"), Some(false))
    );
    assert_eq!(
        (
            got[1]["split"].as_str(),
            got[1]["fired"].as_bool(),
            got[1]["sure"].as_bool()
        ),
        (Some("splits_by_area"), Some(true), Some(true))
    );
    for gone in ["class", "words", "words_rev"] {
        assert!(
            got[1].get(gone).is_none(),
            "{gone} is a deleted word-list field"
        );
    }
}

/// An answer under the confirm bar is not a label to act on: the row says so and nothing fires.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_split_answer_under_the_bar_does_not_fire_the_cue() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "sonnet", "high", 0.95);
    split(&jev, "splits_by_area", 0.40);
    let o = open(&jev, 1, |_| {});
    let r = &o.r;
    let _ = turn(&r.core, None, ASK, None).await;
    assert!(!carries(&r.claude.requests()[0]));
    let got = rows(r);
    assert_eq!(got.len(), 1);
    assert_eq!(
        (got[0]["fired"].clone(), got[0]["sure"].clone()),
        (false.into(), false.into())
    );
}

/// The capacity it was given: with no task-class slot to spare, the label alone does not fire it,
/// and the row says so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_room_for_two_helpers_holds_the_cue_back() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "sonnet", "high", 0.95);
    split(&jev, "splits_by_area", 0.95);
    let o = open(&jev, 1, |c| c.kernel.admission_ceiling = 2);
    let r = &o.r;
    let _ = turn(&r.core, None, ASK, None).await;
    assert!(!carries(&r.claude.requests()[0]));
    let got = rows(r);
    assert_eq!(got[0]["fired"], false);
    assert_eq!(got[0]["split"], "splits_by_area");
}

/// A routed switch rebuilds the request spec and its deferral (`route_to`), so the cue's admission
/// is made again there: a split ask that switches to Opus, with a cue that fired, still carries the
/// helpers family on the request that goes to Opus (theseus-z9mze.4 join fix).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_routed_switch_after_a_fired_cue_still_carries_the_helpers_family() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "opus", "high", 0.95);
    split(&jev, "splits_by_area", 0.95);
    // Tool search on (`parts.cfg`, not the config tweak `open` takes), route.v10's shape acting.
    let deepseek = std::sync::Arc::new(crate::provider::FakeProvider::scripted(
        crate::tests_judge::texts(2),
    ));
    let r = crate::tests_route::rig_parts(
        Some(&jev),
        2,
        |c| c.serve_deepseek_flash(),
        |parts| {
            parts.cfg.tools.defer.enabled = true;
            parts.providers.insert("deepseek".into(), deepseek);
        },
    );
    let r = &r;
    let res = turn(&r.core, None, ASK, None).await;
    assert_eq!(res.route.as_ref().unwrap().reason, "verdict");
    split(&jev, "one_area", 0.95);
    let _ = turn(&r.core, None, PLAIN, None).await;
    let q = r.claude.requests();
    assert_eq!(q[0].model, "claude-opus-5-5", "the turn switched");
    assert_eq!(rows(r)[0]["fired"], true, "the cue fired");
    // With tool search the helpers' tools are in the request, hidden or not: `defer_loading` says.
    let hidden = |req: &ProviderRequest, tool: &str| {
        let t = req.tools.iter().find(|t| t["name"] == tool);
        t.unwrap_or_else(|| panic!("{tool} is not in the request"))["defer_loading"] == true
    };
    for tool in ["task_fan_out", "task_wait"] {
        assert!(
            hidden(&q[1], tool),
            "{tool}: an ordinary turn leaves the family hidden"
        );
        assert!(
            !hidden(&q[0], tool),
            "{tool}: the cue's admission is lost on the switch"
        );
    }
}
