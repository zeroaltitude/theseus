//! `[context] assembly` in the compiler (issue 44; `compiler::view`, `turn::view_step`): the transcript's
//! requests are the golden's whatever the setting says of other places, and a place set to `view` sends
//! its window as real turns, read across sessions, frozen within a turn, with a front that holds from one
//! turn to the next.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::config::assembly::{Assembly, AssemblyByPlace, AssemblySetting};
use crate::provider::binding::unmarked;
use crate::provider::ProviderRequest;
use crate::tests_places::{last_user, rig_with, session_in, turn, Rig};
use crate::tests_private_view::{body, owners_rig, said_earlier, with_person, ASKED};

const GOLDEN: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/golden/private_requests.txt"
);

/// The golden's two requests (`tests_private_view`), made under `setting`.
async fn golden_under(setting: Option<AssemblySetting>) -> String {
    let (r, ids) = owners_rig(|c| c.context.assembly = setting);
    let c = &r.core;
    let tmp = r.root.parent().unwrap().to_path_buf();
    let mut got = String::new();
    for (name, place) in [
        ("cli", None),
        ("dm", Some(format!("dm:{}", crate::tests_places::OWNER))),
    ] {
        let sid = session_in(c, place.as_deref(), &r.root);
        with_person(c, &sid);
        said_earlier(c, &sid);
        turn(c, &sid, ASKED).await;
        let req = r.requests().pop().unwrap();
        got.push_str(&format!(
            "== {name}\n{}",
            body(&req, &tmp, &[&[sid], &ids[..]].concat())
        ));
    }
    got
}

/// With `transcript`, said outright or as the default of a table that sets another place to `view`, every
/// golden request is byte for byte what it is with no setting at all.
#[tokio::test]
async fn the_transcript_assembly_sends_the_golden_requests_byte_for_byte() {
    let want = std::fs::read_to_string(GOLDEN).unwrap();
    let elsewhere = AssemblyByPlace {
        default: Assembly::Transcript,
        places: BTreeMap::from([("discord:elsewhere".to_string(), Assembly::View)]),
    };
    for setting in [
        None,
        Some(AssemblySetting::All(Assembly::Transcript)),
        Some(AssemblySetting::ByPlace(elsewhere)),
    ] {
        let got = golden_under(setting.clone()).await;
        assert!(got == want, "{setting:?} moved the golden requests:\n{got}");
    }
}

fn view_rig() -> Rig {
    rig_with(|c| c.context.assembly = Some(AssemblySetting::All(Assembly::View)))
}

/// The requests whose newest user message says `what`, in the order they were sent.
fn asked(r: &Rig, what: &str) -> Vec<ProviderRequest> {
    (r.requests().into_iter())
        .filter(|q| last_user(q).contains(what))
        .collect()
}

/// A request's messages without their cache marks: what the provider's prefix is compared by.
fn plain(q: &ProviderRequest) -> Vec<Value> {
    q.messages.iter().map(unmarked).collect()
}

fn texts(q: &ProviderRequest) -> String {
    serde_json::to_string(&q.messages).unwrap()
}

/// Every call has its result in the message after it, and the roles alternate.
fn paired(q: &ProviderRequest) {
    for (i, m) in q.messages.iter().enumerate() {
        if i > 0 {
            assert_ne!(
                m["role"],
                q.messages[i - 1]["role"],
                "roles alternate at {i}"
            );
        }
        let uses: Vec<&str> = (m["content"].as_array().into_iter().flatten())
            .filter(|b| b["type"] == "tool_use")
            .filter_map(|b| b["id"].as_str())
            .collect();
        for id in uses {
            let next = &q.messages[i + 1]["content"];
            let answered = (next.as_array().into_iter().flatten())
                .any(|b| b["type"] == "tool_result" && b["tool_use_id"] == id);
            assert!(answered, "call {id} has its result after it");
        }
    }
}

/// How many cache breakpoints a request carries, the top-level one included.
fn breakpoints(q: &ProviderRequest) -> usize {
    let marked = |v: &Value| v.get("cache_control").is_some();
    let blocks = (q.messages.iter())
        .flat_map(|m| m["content"].as_array().cloned().unwrap_or_default())
        .filter(marked)
        .count();
    q.system.iter().filter(|b| marked(b)).count() + blocks + usize::from(q.cache_control.is_some())
}

/// A place set to `view`: the window is the place's earlier exchanges as real turns, calls paired with
/// their results; each call of a turn begins with the bytes of the call before it; and the next turn's
/// request begins with this turn's up to the end of its window.
#[tokio::test]
async fn a_view_place_sends_its_window_as_real_turns_frozen_within_a_turn() {
    let r = view_rig();
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    turn(c, &sid, "first: the tide").await;
    turn(c, &sid, "look around").await;
    turn(c, &sid, "third: and now").await;

    let second = asked(&r, "look around");
    assert_eq!(second.len(), 2, "the tool turn made two calls");
    let third = asked(&r, "third: and now");
    let third = third.last().unwrap();
    let all = texts(third);
    for said in [
        "first: the tide",
        "Hello.",
        "look around",
        "Looking.",
        "Done.",
    ] {
        assert!(all.contains(said), "the window carries {said:?}: {all}");
    }
    paired(third);
    // Frozen within a turn: the second call's request begins with the first's messages, byte for byte.
    let (a, b) = (plain(&second[0]), plain(&second[1]));
    assert_eq!(
        b[..a.len()],
        a[..],
        "the turn's second call repeats its first"
    );
    assert_eq!(second[0].system, second[1].system);
    // The fixed front: turn 2's window (turn 1's exchange) heads turn 3's request unchanged.
    let window = a.len() - 1;
    assert_eq!(
        plain(third)[..window],
        a[..window],
        "the front holds across turns"
    );
    assert_eq!(third.system, second[0].system);
    // The window's end carries the mark, within the provider's four.
    if third.cache_control.is_some() {
        let end = &third.messages[third.messages.len() - 2]["content"];
        let last = end.as_array().and_then(|b| b.last()).unwrap();
        assert!(
            last.get("cache_control").is_some(),
            "the window's end is marked"
        );
    }
    assert!(
        breakpoints(third) <= 4,
        "{} breakpoints",
        breakpoints(third)
    );
}

/// The window is the place's, not the session's: a new session in the same place sees the exchange the
/// earlier one ran; the transcript's does not.
#[tokio::test]
async fn a_view_window_reads_the_place_across_its_sessions() {
    for (view, sees) in [(true, true), (false, false)] {
        let r = match view {
            true => view_rig(),
            false => rig_with(|_| {}),
        };
        let c = &r.core;
        let before = session_in(c, None, &r.root);
        turn(c, &before, "alpha: the earlier words").await;
        let after = session_in(c, None, &r.root);
        turn(c, &after, "beta: what came before").await;
        let q = asked(&r, "beta: what came before").pop().unwrap();
        assert_eq!(
            texts(&q).contains("alpha: the earlier words"),
            sees,
            "view {view}: {}",
            texts(&q)
        );
    }
}
