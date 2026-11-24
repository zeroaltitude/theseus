//! A label, through the history tool (issue 44): one of Jev's four content labels (`personal`, `secret`,
//! `confidential`, `injection`) reads the timeline of the exchanges it was raised in; anything else is read as
//! a topic or a thing, as `thing` was. A label with a span and no place reads every place; beside a place it
//! narrows to that place. A call that still says `thing` is refused in words. A line of a content label's page
//! says what the label was on, and that the owner removed it.

use serde_json::{json, Value};

use super::{answer, args, ex, noon_nov, pov, put, Board, Ex, DM, HERE, LOBBY};
use crate::content::{Flag, ItemKind, Label, Removed};
use crate::item_labels::{self, ItemLabels};
use crate::store::Store;
use crate::view::{PointOfView, Present};

const PERSONAL: &[&str] = &["label:personal"];

/// `answer` for session `me`'s turn, from `HERE`, as of 22 November 2026.
async fn call(core: &std::sync::Arc<crate::Core>, me: &str, v: Value) -> Result<String, String> {
    let at = PointOfView {
        now_ms: noon_nov(22),
        ..pov(HERE, Present::Open)
    };
    answer(core, &Board::default(), (me, "t"), &at, &args(v))
        .await
        .map(|o| o.text)
}

/// `node`'s record, raised `flag` (and removed by the owner when `removed`).
fn labelled(store: &Store, node: &str, kind: ItemKind, flag: Flag, removed: bool) {
    let mut item = ItemLabels::new(node, kind, "test", 1);
    item.raised.push(Label {
        flag,
        node: Some(node.into()),
        kind,
        source: "test".into(),
        confidence: 0.97,
        band: theseus_judge::band::Band::Confirm,
        at_ms: 1,
        judgment: String::new(),
    });
    if removed {
        let r = Removed {
            flag,
            by: "the owner".into(),
            via: "cli".into(),
            at_ms: 2,
        };
        item_labels::removed(&mut item, "ses_any", &r);
    }
    store.append(&[item.record().unwrap()]).unwrap();
}

/// Exchanges on 11 November in three places, two labelled `personal`, one not; one labelled on the 12th.
fn days(core: &crate::Core, me: &str, other: &str) {
    let day = noon_nov(11);
    let puts = [
        (1, HERE, me, day, PERSONAL),
        (2, DM, other, day + 1_000, PERSONAL),
        (3, LOBBY, other, day + 2_000, &[][..]),
        (4, DM, other, noon_nov(12), PERSONAL),
    ];
    for (i, place, session, at, things) in puts {
        put(
            &core.store,
            &Ex {
                at,
                things,
                ..ex(i, place, session)
            },
        );
    }
}

/// A label and a span with no place read every place: the span's exchanges the label was raised in, newest
/// first, each line naming its place; none outside the span, none unlabelled.
#[tokio::test]
async fn a_label_and_a_span_read_every_place() {
    use crate::tests_places::{rig, session};
    let r = rig();
    let (me, other) = (session(&r.core, None), session(&r.core, None));
    days(&r.core, &me, &other);
    let page = call(
        &r.core,
        &me,
        json!({"label": "personal", "when": "2026-11-11"}),
    )
    .await
    .unwrap();
    let (one, two) = (page.find("turn_0001"), page.find("turn_0002"));
    assert!(one.is_some() && two.is_some(), "{page}");
    assert!(two < one, "newest first: {page}");
    assert!(page.contains(&format!(" · {DM} · ")), "{page}");
    assert!(!page.contains("turn_0003"), "not labelled: {page}");
    assert!(!page.contains("turn_0004"), "outside the span: {page}");
}

/// A label beside a place narrows it to that place's exchanges.
#[tokio::test]
async fn a_label_beside_a_place_narrows_it() {
    use crate::tests_places::{rig, session};
    let r = rig();
    let (me, other) = (session(&r.core, None), session(&r.core, None));
    days(&r.core, &me, &other);
    let page = call(&r.core, &me, json!({"label": "personal", "place": DM}))
        .await
        .unwrap();
    assert!(
        page.contains("turn_0002") && page.contains("turn_0004"),
        "{page}"
    );
    assert!(!page.contains("turn_0001"), "another place: {page}");
}

/// A topic or a thing is a label too: its own timeline, wherever the reader is.
#[tokio::test]
async fn a_topic_label_reads_the_topics_timeline() {
    use crate::tests_places::{rig, session};
    let r = rig();
    let (me, other) = (session(&r.core, None), session(&r.core, None));
    put(
        &r.core.store,
        &Ex {
            things: &["topic:kelp"],
            ..ex(1, DM, &other)
        },
    );
    put(&r.core.store, &ex(2, HERE, &me));
    let page = call(&r.core, &me, json!({"label": "topic:kelp"}))
        .await
        .unwrap();
    assert!(page.contains("turn_0001"), "{page}");
    assert!(!page.contains("turn_0002"), "{page}");
}

/// A call that still says `thing` is refused in words that name `label`, by the tool's own reading of its input
/// (the live call and its plan alike).
#[test]
fn thing_is_refused_in_words() {
    let e = crate::history_tool::parse(&json!({"thing": "topic:kelp"})).unwrap_err();
    assert!(e.contains("thing is now label"), "{e}");
    assert!(crate::history_tool::parse(&json!({"label": "topic:kelp"})).is_ok());
}

/// A line of a content label's page says what the label was on (the message, the reply, or something the
/// exchange read), and that the owner removed it; a topic's page says nothing of it.
#[tokio::test]
async fn a_label_line_says_what_it_was_on_and_a_removal() {
    use crate::tests_places::{rig, session};
    let r = rig();
    let me = session(&r.core, None);
    let day = noon_nov(11);
    let (ask, _) = put(
        &r.core.store,
        &Ex {
            at: day,
            things: PERSONAL,
            ..ex(1, HERE, &me)
        },
    );
    labelled(
        &r.core.store,
        &ask.id,
        ItemKind::OperatorMessage,
        Flag::Personal,
        true,
    );
    let (_, reply) = put(
        &r.core.store,
        &Ex {
            at: day + 1_000,
            things: PERSONAL,
            ..ex(2, HERE, &me)
        },
    );
    labelled(
        &r.core.store,
        &reply.id,
        ItemKind::Reply,
        Flag::Personal,
        false,
    );
    put(
        &r.core.store,
        &Ex {
            at: day + 2_000,
            things: PERSONAL,
            ..ex(3, HERE, &me)
        },
    );
    // Raised on the message and on the reply; the owner removed the message's only.
    let (ask, reply) = put(
        &r.core.store,
        &Ex {
            at: day + 3_000,
            things: PERSONAL,
            ..ex(4, HERE, &me)
        },
    );
    labelled(
        &r.core.store,
        &ask.id,
        ItemKind::OperatorMessage,
        Flag::Personal,
        true,
    );
    labelled(
        &r.core.store,
        &reply.id,
        ItemKind::Reply,
        Flag::Personal,
        false,
    );
    let page = call(&r.core, &me, json!({"label": "personal"}))
        .await
        .unwrap();
    let line = |t: &str| {
        page.lines()
            .find(|l| l.contains(t))
            .unwrap_or_else(|| panic!("{t}: {page}"))
            .to_string()
    };
    assert!(
        line("turn_0001")
            .ends_with("· personal: on the message, since removed by the owner · turn_0001"),
        "{page}"
    );
    assert!(
        line("turn_0002").ends_with("· personal: on the reply · turn_0002"),
        "{page}"
    );
    // Neither the message nor the reply holds it: another item of the exchange does (a tool's result, a reply
    // before the last), whose removal this read cannot see, so none is claimed.
    assert!(
        line("turn_0003").ends_with("· personal: on another item of the exchange · turn_0003"),
        "{page}"
    );
    assert!(
        line("turn_0004").ends_with(
            "· personal: on the message and the reply, since removed by the owner from the message · turn_0004"
        ),
        "{page}"
    );
}

/// A page is headed by what was asked (issue 44): a place named, or every place for a label with none, and the
/// label by name; never the place the turn is in when the page is another's.
#[tokio::test]
async fn a_page_is_headed_by_what_was_asked() {
    use crate::tests_places::{rig, session};
    let r = rig();
    let (me, other) = (session(&r.core, None), session(&r.core, None));
    days(&r.core, &me, &other);
    put(
        &r.core.store,
        &Ex {
            things: &["topic:kelp"],
            ..ex(5, DM, &other)
        },
    );
    for (v, heading) in [
        (
            json!({"label": "personal"}),
            "of every place's history labelled personal, newest first",
        ),
        (
            json!({"label": "personal", "place": DM}),
            "of discord:dm:42's history labelled personal, newest first",
        ),
        (
            json!({"place": DM}),
            "of discord:dm:42's history, newest first",
        ),
        (
            json!({"label": "topic:kelp"}),
            "of every place's history labelled topic:kelp, newest first",
        ),
        (json!({}), "of local's history, newest first"),
    ] {
        let page = call(&r.core, &me, v.clone()).await.unwrap();
        let head = page.lines().next().unwrap();
        assert!(head.contains(heading), "{v}: {head}");
    }
}

/// A line's handle is its last field, by rule (a reader of a page takes the handle as the field after a line's
/// last ` · `): through the page renderer, on a content label's page whose lines carry what the label was on, on
/// a topic's page whose lines carry none, and on a date's page of every place.
#[tokio::test]
async fn a_lines_last_field_is_its_handle_with_or_without_a_label() {
    use crate::tests_places::{rig, session};
    let r = rig();
    let me = session(&r.core, None);
    let day = noon_nov(11);
    let (ask, _) = put(
        &r.core.store,
        &Ex {
            at: day,
            things: &["label:personal", "topic:kelp"],
            ..ex(1, HERE, &me)
        },
    );
    labelled(
        &r.core.store,
        &ask.id,
        ItemKind::OperatorMessage,
        Flag::Personal,
        true,
    );
    put(
        &r.core.store,
        &Ex {
            at: day + 1_000,
            things: &["label:personal", "topic:kelp"],
            ..ex(2, DM, &me)
        },
    );
    let mut tails = 0;
    for v in [
        json!({"label": "personal"}),
        json!({"label": "topic:kelp"}),
        json!({"when": "2026-11-11"}),
    ] {
        let page = call(&r.core, &me, v.clone()).await.unwrap();
        let lines: Vec<&str> = page.lines().filter(|l| l.starts_with("- ")).collect();
        assert_eq!(lines.len(), 2, "{v}: {page}");
        for l in lines {
            let (rest, last) = l.rsplit_once(" · ").unwrap_or_else(|| panic!("{l}"));
            assert!(["turn_0001", "turn_0002"].contains(&last), "{v}: {l}");
            tails +=
                usize::from(rest.ends_with("personal: on the message, since removed by the owner"));
        }
    }
    assert_eq!(
        tails, 1,
        "the label's page carries its tail before the handle"
    );
}
