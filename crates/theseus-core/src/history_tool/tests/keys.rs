//! A timeline read by its own keys (issue 44): a thing's or a label's touch is keyed at its own time (a label at
//! its turn's), which can be before its exchange opened. A page's cursor is the key of the last touch it
//! admitted, so the next page continues exactly after its last line; a walk gives one turn once; a span is
//! matched on the touch's own time, and a line still says when its exchange opened.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::{answer, args, ex, noon_nov, pov, put, Board, Ex, HERE};
use crate::exchange::{touch, Root};
use crate::view::{PointOfView, Present};

/// More than two pages.
const N: u64 = 230;

/// `answer` for session `me`'s turn, from `HERE`, as of 22 November 2026: the page's text and its meta.
async fn call(core: &std::sync::Arc<crate::Core>, me: &str, v: Value) -> (String, Value) {
    let at = PointOfView {
        now_ms: noon_nov(22),
        ..pov(HERE, Present::Open)
    };
    let o = answer(core, &Board::default(), (me, "t"), &at, &args(v))
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    (o.text, o.meta)
}

/// The handles a page's lines end with, in order.
fn handles(page: &str) -> Vec<String> {
    (page.lines().filter(|l| l.starts_with("- ")))
        .map(|l| l.rsplit(" · ").next().unwrap().to_string())
        .collect()
}

/// Every page of `v`, each from the cursor the one before it returned, until a page says there is no more.
async fn every_page(core: &std::sync::Arc<crate::Core>, me: &str, v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let mut before = Value::Null;
    for _ in 0..10 {
        let mut asked = v.clone();
        if !before.is_null() {
            asked["before"] = before.clone();
        }
        let (text, meta) = call(core, me, asked).await;
        out.extend(handles(&text));
        if meta["end"] != "more" {
            return out;
        }
        before = meta["next"].clone();
        assert!(
            !before.is_null(),
            "a page with more names its cursor: {meta}"
        );
    }
    panic!("more than ten pages")
}

/// Exchange `i`'s key under the thing roots, and when its exchange opened: keys a second apart, newest last,
/// each exchange after a multiple of seven sharing that one's key; each exchange opened a little after its key,
/// and one long after.
fn times(i: u64) -> (u64, u64) {
    let tied = i % 7 == 1 && i > 1;
    let key = noon_nov(13) + (i - u64::from(tied)) * 1_000;
    let opened = match i {
        150 => key + 3 * 86_400_000,
        _ => key + 700,
    };
    (key, opened)
}

/// KEY: more than a page of exchanges touched under one thing root (`picture:document`, the shape of a picture
/// label's root) and under `label:personal`, each keyed before its exchange opened (ties among them, one opened
/// days after its key), paged by the cursor each page returns, to the end: every exchange once, in key order,
/// none skipped. One turn holds a second key under `label:personal`, as an older store can: it is given once.
#[tokio::test]
async fn a_thing_timeline_whose_keys_precede_their_exchanges_pages_every_exchange_once() {
    use crate::tests_places::{rig, session};
    let r = rig();
    let me = session(&r.core, None);
    let mut order: BTreeMap<(std::cmp::Reverse<u64>, String), ()> = BTreeMap::new();
    for i in 1..=N {
        let (key, opened) = times(i);
        let e = Ex {
            at: opened,
            ..ex(i, HERE, &me)
        };
        put(&r.core.store, &e);
        let mut recs = Vec::new();
        for root in ["picture:document", "label:personal"] {
            recs.extend(touch(Root::Thing(root), &e.turn, &me, key));
        }
        if i == 40 {
            recs.extend(touch(
                Root::Thing("label:personal"),
                &e.turn,
                &me,
                key + 300,
            ));
        }
        r.core.store.append(&recs).unwrap();
        order.insert((std::cmp::Reverse(key), e.turn.clone()), ());
    }
    let want: Vec<String> = order.into_keys().map(|(_, t)| t).collect();
    assert_eq!(want.len() as u64, N);
    for label in ["picture:document", "personal"] {
        let got = every_page(&r.core, &me, &json!({ "label": label })).await;
        assert_eq!(got.len(), want.len(), "{label}: each exchange once");
        assert_eq!(got, want, "{label}: in key order");
    }
}

/// KEY: a span over a label's timeline is matched on each touch's own time, at both edges: an exchange whose key
/// falls before the span's start is not on the page though it opened inside it; one whose key falls inside the
/// span is on the page though it opened after the span's end. Its line says when its exchange opened.
#[tokio::test]
async fn a_span_over_a_label_is_matched_by_each_touchs_own_time_at_both_edges() {
    use crate::tests_places::{rig, session};
    let r = rig();
    let me = session(&r.core, None);
    let span = crate::recall::when::read_local("2026-11-11", noon_nov(22)).unwrap();
    let (from, to) = (span.from_ms.unwrap(), span.to_ms.unwrap());
    // (exchange, its key, when it opened)
    let cases = [
        (1, from - 60_000, from + 60_000),
        (2, from + 3_600_000, from + 3_600_500),
        (3, to - 60_000, to + 60_000),
    ];
    for (i, key, opened) in cases {
        let e = Ex {
            at: opened,
            ..ex(i, HERE, &me)
        };
        put(&r.core.store, &e);
        let recs: Vec<_> = touch(Root::Thing("label:personal"), &e.turn, &me, key)
            .into_iter()
            .collect();
        r.core.store.append(&recs).unwrap();
    }
    let (text, _) = call(
        &r.core,
        &me,
        json!({"label": "personal", "when": "2026-11-11"}),
    )
    .await;
    assert_eq!(handles(&text), ["turn_0003", "turn_0002"], "{text}");
    // The line says when its exchange opened, after the span's end.
    let line = (text.lines().find(|l| l.ends_with("turn_0003"))).unwrap();
    assert!(
        line.starts_with(&format!("- {}", super::super::page::when(to + 60_000))),
        "{text}"
    );
}
