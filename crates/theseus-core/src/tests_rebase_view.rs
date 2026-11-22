//! The readers that leaned on a transcript compilation, re-based on the view (issue 44): a bare
//! `context.history` call begins where the view's window does; the measuring row's "seen" is what the view
//! showed (front, window and beside); `context.census` of a place set to the view is built by the view's
//! assembler, not the transcript's compile; own-benchmark refuses a turn the view built, in words, by its
//! compilation's strategy; and a fork's window holds the run it forked from, under the gate.

use std::collections::HashSet;

use crate::config::assembly::{Assembly, AssemblySetting};
use crate::exchange::ledger::{Prev, Row};
use crate::exchange::{self, Exchange};
use crate::history_tool::page::{self, Lens};
use crate::session::SessionRecord;
use crate::tests_places::{last_user, rig_setup, rig_with, session, turn, Rig, OWNER};
use crate::tests_private_view::session_in;
use crate::view::history::HistoryArgs;
use crate::view::record::{self, ViewRecord};
use crate::view::{Form, Present, Slot, Space};

fn view_rig() -> Rig {
    rig_with(|c| c.context.assembly = Some(AssemblySetting::All(Assembly::View)))
}

fn none(_: &str) -> bool {
    false
}

fn none_moved(_: &Exchange) -> bool {
    false
}

fn all_private(_: &str) -> bool {
    true
}

/// The exchanges a view laid in its window (slot G), with the form each was laid in.
fn window(rec: &ViewRecord) -> Vec<(String, Form)> {
    (rec.shown.iter())
        .filter(|s| s.piece.slot == Slot::Here)
        .filter(|s| matches!(s.piece.range.space, Space::Exchanges { .. }))
        .map(|s| (s.piece.range.first.id.clone(), s.form))
        .collect()
}

/// Whether a view showed exchange `turn` in any slot.
fn shows(rec: &ViewRecord, turn: &str) -> bool {
    (rec.shown.iter())
        .filter(|s| matches!(s.piece.range.space, Space::Exchanges { .. }))
        .any(|s| s.piece.range.first.id == turn || s.piece.range.last.id == turn)
}

/// A view whose window starts mid-history (the place's `/new` mark: nothing before it is in the window): a
/// bare history call begins with the newest exchange the window does not lay whole, so it pages nothing the
/// model already sees in full, and reaches everything older. (The transcript's rule read the compilation's
/// cut and cleared nodes; a view compilation has neither, so it began before the session's first node and
/// paged the whole window again.)
#[tokio::test]
async fn a_bare_history_call_begins_where_the_views_window_does() {
    let r = view_rig();
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    let mut turns: Vec<String> = Vec::new();
    for i in 0..10 {
        if i == 6 {
            let ex = exchange::exchange(&c.store, &turns[5]).unwrap().unwrap();
            let at = crate::view::Pos {
                at_ms: ex.start_ms,
                id: ex.turn_id.clone(),
            };
            let mark = crate::ladder::new_mark_record(&ex.place, &at).unwrap();
            c.store.append(&[mark]).unwrap();
        }
        turns.push(turn(c, &sid, &format!("note {i}: the tide")).await.turn_id);
    }
    let last = turns.last().unwrap().clone();
    let rec = record::newest(&c.store, &last)
        .unwrap()
        .expect("the turn kept its view");
    let laid = window(&rec);
    let whole: HashSet<&str> = (laid.iter())
        .filter(|(_, f)| *f == Form::Whole)
        .map(|(t, _)| t.as_str())
        .collect();
    assert!(!whole.is_empty(), "{laid:?}");
    assert!(
        !laid.iter().any(|(t, _)| *t == turns[0] || *t == turns[4]),
        "the window starts mid-history: {laid:?}"
    );
    let begins = page::in_view(&c.store, &sid)
        .unwrap()
        .expect("where a bare call begins");
    let here = exchange::exchange(&c.store, &turns[0])
        .unwrap()
        .unwrap()
        .place;
    let lens = Lens {
        store: &c.store,
        here,
        present: Present::OwnerAlone,
        hidden: &none,
        moved: &none_moved,
        private: &all_private,
        in_view: Some(begins),
        words: None,
    };
    let paged: Vec<String> = (lens
        .walk(&HistoryArgs::default(), None, 0)
        .unwrap()
        .page
        .lines)
        .into_iter()
        .map(|l| l.handle.0)
        .collect();
    assert!(
        paged.iter().all(|t| !whole.contains(t.as_str())),
        "nothing laid whole is paged again: {paged:?}"
    );
    let earlier = &turns[..turns.len() - 1];
    let want = earlier.iter().rev().find(|t| !whole.contains(t.as_str()));
    assert_eq!(paged.first(), want, "{paged:?} {laid:?}");
    assert!(paged.contains(&turns[0]), "{paged:?}");
}

/// The measuring row of a view turn counts as seen what its view showed: the place's previous exchange in
/// its window, and his DM's exchange in the block beside his message (another session's, which a
/// compilation's `includes` never holds).
#[tokio::test]
async fn the_measuring_rows_seen_is_what_the_view_showed() {
    let r = view_rig();
    let c = &r.core;
    let dm = session(c, Some(&format!("dm:{OWNER}")));
    let in_dm = turn(c, &dm, "dm: the plum orchard is due a visit")
        .await
        .turn_id;
    let sid = session_in(c, None, &r.root);
    let first = turn(c, &sid, "first: the tide").await.turn_id;
    let second = turn(c, &sid, "second: the moon").await.turn_id;
    let rec = record::newest(&c.store, &second)
        .unwrap()
        .expect("the turn kept its view");
    assert!(shows(&rec, &first) && shows(&rec, &in_dm), "{rec:?}");
    let rows: Vec<Row> = (crate::test_support::rows(c, "context.turn").into_iter())
        .map(|v| serde_json::from_value(v).unwrap())
        .collect();
    let row = (rows.iter())
        .find(|r| r.exchange == second)
        .expect("the turn's row");
    assert_eq!(
        (row.prev, row.prev_exchange.as_deref()),
        (Prev::Transcript, Some(first.as_str()))
    );
    let dm_seen = (row.elsewhere.iter())
        .find(|e| e.exchange == in_dm)
        .expect("his DM's exchange is counted");
    assert!(dm_seen.in_view, "{row:?}");
}

/// `context.census` of a session in a place set to the view counts the request the view builds: the place's
/// window, read across sessions (another session's exchange there included), a new chain as every view turn
/// is, and no recall. A place left to the transcript still counts the session's own transcript.
#[tokio::test]
async fn a_census_of_a_view_place_counts_the_views_request() {
    for (as_view, r) in [(true, view_rig()), (false, rig_with(|_| {}))] {
        let c = &r.core;
        let other = session_in(c, None, &r.root);
        turn(c, &other, "alpha: the kestrel nests on the quay").await;
        let sid = session_in(c, None, &r.root);
        turn(c, &sid, "beta: the heron fishes the weir").await;
        let p = theseus_protocol::census::ContextCensusParams {
            session_id: Some(sid.clone()),
            ..Default::default()
        };
        let (out, request, _) = c.context_census(&p).unwrap();
        let sent = serde_json::to_string(&request.messages).unwrap();
        assert!(sent.contains("the heron fishes the weir"), "{sent}");
        let noted = out.notes.iter().any(|n| n.contains("as the view builds"));
        if as_view {
            assert!(sent.contains("the kestrel nests on the quay"), "{sent}");
            assert_eq!((out.compile.as_str(), out.recall_tokens), ("recompile", 0));
            assert!(noted, "{:?}", out.notes);
            assert!(record::of_turn(&c.store, "census").unwrap().is_empty());
        } else {
            assert!(!sent.contains("the kestrel"), "{sent}");
            assert!(!noted, "{:?}", out.notes);
        }
    }
}

/// A fork of `source` at its last node, speaking in no place (as `session.fork` makes one, by reference).
fn fork_of(c: &crate::Core, source: &str) -> String {
    let nodes = c.store.transcript(source).unwrap();
    let (position, node) = nodes.last().map(|(p, n)| (*p, n.id.clone())).unwrap();
    let mut fork = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    fork.forked_from = Some(Box::new(theseus_protocol::ForkOrigin {
        session: source.into(),
        node,
        position,
        compilation: None,
        at_ms: 1,
    }));
    c.store.put_session(&fork.session_id, &fork).unwrap();
    fork.session_id
}

/// What the fork's turn asking `what` was sent, as one string.
fn sent_for(r: &Rig, what: &str) -> String {
    let q = (r.requests().into_iter())
        .rev()
        .find(|q| !q.tools.is_empty() && last_user(q).contains(what))
        .expect("the turn's request");
    serde_json::to_string(&q.messages).unwrap()
}

/// A view rig with a sealed thread `kelp` bound to a session of its own; that session's id.
fn sealed_thread_rig() -> (Rig, String) {
    let thread = std::cell::RefCell::new(String::new());
    let r = rig_setup(
        |c| {
            c.context.assembly = Some(AssemblySetting::All(Assembly::View));
            c.threads.enabled = true;
        },
        |s| {
            let rec = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
            s.put_session(&rec.session_id, &rec).unwrap();
            let t = serde_json::json!({"slug": "kelp", "title": "kelp",
                "sealed": {"by": "cli", "at_ms": 1, "reason": "embargo"}});
            s.put_meta("thread:kelp", &t).unwrap();
            s.put_meta("discord.session.thread:kelp", &rec.session_id)
                .unwrap();
            *thread.borrow_mut() = rec.session_id;
        },
    );
    (r, thread.into_inner())
}

/// A fork speaks in no place of its source's: its first view turn lays the run it forked from in its window
/// (his DM's exchange as a real turn in slot G, not only beside his message), and a sealed source gives
/// nothing, in no slot and no byte of the request.
#[tokio::test]
async fn a_forks_first_view_turn_shows_the_run_it_forked_from_and_nothing_of_a_sealed_source() {
    let r = view_rig();
    let c = &r.core;
    let dm = session(c, Some(&format!("dm:{OWNER}")));
    let said = turn(c, &dm, "dm: the lighthouse lamp is amber")
        .await
        .turn_id;
    let fork = fork_of(c, &dm);
    let first = turn(c, &fork, "fork: what colour is the lamp?")
        .await
        .turn_id;
    let rec = record::newest(&c.store, &first)
        .unwrap()
        .expect("the fork's turn kept its view");
    assert!(window(&rec).iter().any(|(t, _)| *t == said), "{rec:?}");
    let sent = sent_for(&r, "what colour is the lamp");
    assert!(sent.contains("the lighthouse lamp is amber"), "{sent}");

    let (r, thread) = sealed_thread_rig();
    let c = &r.core;
    let held = turn(c, &thread, "kelp: the embargoed figure is 7741")
        .await
        .turn_id;
    let fork = fork_of(c, &thread);
    let first = turn(c, &fork, "fork: and the figure?").await.turn_id;
    let rec = record::newest(&c.store, &first)
        .unwrap()
        .expect("the fork's turn kept its view");
    assert!(!shows(&rec, &held), "{rec:?}");
    let sent = sent_for(&r, "and the figure");
    assert!(!sent.contains("7741"), "{sent}");
}
