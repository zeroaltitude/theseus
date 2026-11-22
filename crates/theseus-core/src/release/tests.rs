//! A piece released by the owner's own hand (issue 44, step 3b): the red-team cases of the release. A released
//! piece is admitted in the place the record names and nowhere else, and nothing else of its place is; a
//! withdrawal ends it; only his own words of a private place are released, never a sealed or moved one, also when
//! sealed after the release; a record is made once, and an unknown or replayed id does nothing; an agent's
//! process cannot make one.

use super::*;
use crate::exchange::{exchange_record, Opener};
use crate::history_tool::page::{none_released, released_here, Lens};
use crate::node::Node;
use crate::view::Present;

const DM: &str = "discord:dm:42";
const DEN: &str = "discord:channel:7";
const PIER: &str = "discord:channel:8";

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    (dir, store)
}

fn put(store: &Store, turn: &str, session: &str, place: &str, person: &str) {
    let ask = Node::user(session, Some(turn), "owner", "the plan is the lighthouse");
    let x = Exchange {
        turn_id: turn.into(),
        session_id: session.into(),
        place: place.into(),
        private: place != PIER,
        opened_by: Opener::Message,
        person: Some(person.into()),
        start_ms: 1_000,
        end_ms: Some(1_500),
        ask: Some(ask.id.clone()),
        reply: None,
        messages: vec![ask.id.clone()],
        stop: Some("end_turn".into()),
        rebuilt: false,
    };
    store
        .append(&[ask.record().unwrap(), exchange_record(&x).unwrap()])
        .unwrap();
}

fn ex(store: &Store, turn: &str) -> Exchange {
    exchange::exchange(store, turn).unwrap().unwrap()
}

fn clear(_: &Exchange) -> bool {
    false
}

fn no(_: &str) -> bool {
    false
}

fn yes(_: &str) -> bool {
    true
}

/// Whether a reader in `here` (others present) admits `turn`, by the history tool's lens, with `hidden` the
/// seal's answer for sessions and `moved` for exchanges.
fn admits(
    s: &Store,
    here: &str,
    turn: &str,
    hidden: &(dyn Fn(&str) -> bool + Sync),
    moved: &(dyn Fn(&Exchange) -> bool + Sync),
) -> bool {
    let released = released_here(s, here);
    let lens = Lens {
        store: s,
        here: here.into(),
        present: Present::Shared,
        hidden,
        moved,
        private: &yes,
        released: &released,
        in_view: None,
        words: None,
    };
    lens.admits(&ex(s, turn))
}

/// Released to the den, the piece is admitted in the den alone, and nothing else of the DM is; withdrawn, it is
/// admitted nowhere, and the withdrawal cannot be repeated or replayed.
#[test]
fn a_released_piece_is_admitted_in_the_place_it_names_alone_and_withdrawing_ends_it() {
    let (_d, s) = store();
    put(&s, "t_piece", "ses_dm", DM, OWNER);
    put(&s, "t_other", "ses_dm", DM, OWNER);
    assert!(
        !admits(&s, DEN, "t_piece", &no, &clear),
        "before any release"
    );
    let r = grant(&s, "t_piece", DEN, "cli", &clear).unwrap();
    assert_eq!((r.from.as_str(), r.to.as_str()), (DM, DEN));
    assert!(admits(&s, DEN, "t_piece", &no, &clear));
    assert!(
        !admits(&s, DEN, "t_other", &no, &clear),
        "nothing else of the DM"
    );
    assert!(
        !admits(&s, PIER, "t_piece", &no, &clear),
        "no place the record does not name"
    );
    assert_eq!(released_to(&s, DEN).len(), 1);
    assert!(released_to(&s, PIER).is_empty());
    // A lens that reads no release admits nothing of it, as every reader but the history tool's.
    let lens = Lens {
        store: &s,
        here: DEN.into(),
        present: Present::Shared,
        hidden: &no,
        moved: &clear,
        private: &yes,
        released: &none_released,
        in_view: None,
        words: None,
    };
    assert!(!lens.admits(&ex(&s, "t_piece")));
    withdraw(&s, &r.id, "cli").unwrap();
    assert!(!admits(&s, DEN, "t_piece", &no, &clear), "withdrawn");
    assert!(released_to(&s, DEN).is_empty());
    assert!(
        withdraw(&s, &r.id, "cli").is_err(),
        "a withdrawal is made once"
    );
    assert!(
        withdraw(&s, "rel_forged", "cli").is_err(),
        "an unknown id does nothing"
    );
    assert!(
        grant(&s, "t_piece", DEN, "cli", &clear).is_err(),
        "a release is made once, withdrawn or not"
    );
    let all = all(&s).unwrap();
    assert_eq!(all.len(), 1);
    assert!(
        all[0].1.is_some(),
        "the end is a second record; the release is kept"
    );
}

/// Only the owner's own words of a private place are released, to another place, and never a sealed or moved
/// piece; a piece sealed or moved AFTER its release is admitted nowhere.
#[test]
fn a_sealed_or_moved_piece_is_never_released_also_when_sealed_after() {
    let (_d, s) = store();
    put(&s, "t_guest", "ses_dm", DM, "discord:9");
    put(&s, "t_open", "ses_pier", PIER, OWNER);
    put(&s, "t_piece", "ses_dm", DM, OWNER);
    assert!(
        grant(&s, "t_guest", DEN, "cli", &clear).is_err(),
        "not his words"
    );
    assert!(
        grant(&s, "t_open", DEN, "cli", &clear).is_err(),
        "not a private place's"
    );
    assert!(
        grant(&s, "t_piece", DM, "cli", &clear).is_err(),
        "not to its own place"
    );
    assert!(
        grant(&s, "t_piece", DEN, "cli", &|_: &Exchange| true).is_err(),
        "sealed now"
    );
    assert!(
        grant(&s, "t_none", DEN, "cli", &clear).is_err(),
        "no such exchange"
    );
    grant(&s, "t_piece", DEN, "cli", &clear).unwrap();
    assert!(admits(&s, DEN, "t_piece", &no, &clear));
    let sealed = |sid: &str| sid == "ses_dm";
    assert!(
        !admits(&s, DEN, "t_piece", &sealed, &clear),
        "sealed after the release"
    );
    let moved = |e: &Exchange| e.turn_id == "t_piece";
    assert!(
        !admits(&s, DEN, "t_piece", &no, &moved),
        "moved after the release"
    );
}

/// A release is written only by the owner in person: an agent's process at the terminal, and a channel, are
/// refused by the proof every release method asks before it reads anything (`rpc::exchanges`).
#[tokio::test]
async fn an_agents_process_or_a_channel_cannot_release() {
    use crate::approval::{Answerer, Surface};
    let rig = crate::tests_places::rig();
    let rule = &rig.core.runner.place_rule;
    let cfg = &rig.core.cfg;
    let who = |surface, agent: Option<&str>, discord| Answerer {
        label: "x".into(),
        surface,
        discord,
        agent: agent.map(str::to_string),
    };
    let ok = |a: &Answerer| crate::places::owner_in_person_of(a, rule, cfg, "", "").is_ok();
    assert!(ok(&who(Surface::Cli, None, None)), "his own terminal");
    assert!(
        !ok(&who(Surface::Cli, Some("claude"), None)),
        "an agent's process"
    );
    assert!(!ok(&who(Surface::Mcp, None, None)), "the MCP server");
    assert!(
        !ok(&who(Surface::Unnamed, None, None)),
        "an unnamed connection"
    );
}
