//! A recalled note's header: when it was said, in the owner's zone, frozen
//! when it is recalled; and the span a words search names, as the wire has it.

use std::sync::Arc;

use super::render::{self, header_in, Sources};
use super::stamp::{self, Zone};
use crate::node::{Node, RecalledRef};

fn phoenix(_: u64) -> Zone {
    Zone {
        offset_secs: -7 * 3600,
        name: "MST".into(),
    }
}

/// The header says when the note was said in the owner's zone, not UTC: an
/// evening message is that evening's day, where UTC read the next.
#[test]
fn an_evening_note_is_dated_in_the_owners_zone() {
    // 2026-03-18 02:40 UTC is Tuesday the 17th, 19:40 in Phoenix.
    let at = 1_773_801_600_000;
    let mut n = Node::user("ses_weir", None, "cli", "The ferry rota is settled.");
    n.created_at_ms = at;
    let header = header_in(&n, 42, "#harbor", "The ferry rota is settled.", &phoenix);
    assert_eq!(
        header,
        "a message from cli in #harbor, said Tue 2026-03-17 19:40 MST (as of @42)"
    );
    assert!(!header.contains("03-18"), "{header}");
}

/// A header is frozen when the item is recalled: the render prints its
/// stored bytes, whatever the zone or the clock is when it is rendered.
#[test]
fn a_frozen_header_renders_its_stored_bytes() {
    let mut n = Node::user("ses_weir", None, "cli", "We moved the rota yesterday.");
    n.id = "msg_rota".into();
    n.created_at_ms = 1_773_801_600_000;
    let stored = header_in(&n, 7, "#harbor", "We moved the rota yesterday.", &phoenix);
    let item = RecalledRef {
        node_id: "msg_rota".into(),
        session_id: "ses_weir".into(),
        position: 7,
        chunk: (0, 28),
        header: stored.clone(),
        tokens: 8,
    };
    let sources: Sources = [("msg_rota".to_string(), Arc::new(n))]
        .into_iter()
        .collect();
    let first = render::render(std::slice::from_ref(&item), &sources);
    assert!(
        first.contains(&format!("\n(1) {stored}\n    \"We moved")),
        "{first}"
    );
    assert_eq!(first, render::render(&[item], &sources));
    assert_eq!(
        stamp::said_in(1_773_801_600_000, &phoenix),
        "Tue 2026-03-17 19:40 MST"
    );
}

#[test]
fn a_span_is_the_wires_own_type_and_serialises_as_it_always_did() {
    use super::when::When;
    let both = When {
        from_ms: Some(1_000),
        to_ms: Some(2_000),
        said: "march".into(),
    };
    assert_eq!(
        serde_json::to_string(&both).unwrap(),
        r#"{"said":"march","from_ms":1000,"to_ms":2000}"#
    );
    let open = When {
        from_ms: None,
        to_ms: Some(2_000),
        said: "before march".into(),
    };
    assert_eq!(
        serde_json::to_string(&open).unwrap(),
        r#"{"said":"before march","to_ms":2000}"#
    );
    assert!(both.holds(1_000) && both.holds(1_999) && !both.holds(2_000) && !both.holds(999));
    assert!(open.holds(0) && !open.holds(2_000));
}
