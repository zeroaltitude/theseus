//! The readers that leaned on a transcript compilation, re-based on the view (issue 44): a bare
//! `context.history` call begins where the view's window does; the measuring row's "seen" is what the view
//! showed (front, window and beside); `context.census` of a place set to the view is built by the view's
//! assembler, not the transcript's compile; own-benchmark refuses a turn the view built, in words, by its
//! compilation's strategy; and a fork's window holds the run it forked from, under the gate.

use std::collections::HashSet;

use crate::exchange::ledger::{Prev, Row};
use crate::exchange::{self, Exchange};
use crate::history_tool::page::{self, Lens};
use crate::node::{Body, Node};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::tests_places::{last_user, rig_with, session, turn, Rig, OWNER};
use crate::tests_private_view::session_in;
use crate::view::history::HistoryArgs;
use crate::view::record::{self, ViewRecord};
use crate::view::{Form, Slot, Space};

fn view_rig() -> Rig {
    rig_with(|_| {})
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
    // The floor lays the newest exchanges' words; a tool-less exchange is laid whole by no band (issue 44's
    // closing cut: no previous exchange is a mandatory whole), so a bare call pages from the newest earlier one.
    assert!(whole.is_empty(), "{laid:?}");
    assert!(
        !laid.iter().any(|(t, _)| *t == turns[0] || *t == turns[4]),
        "the window starts mid-history: {laid:?}"
    );
    let here = exchange::exchange(&c.store, &turns[0])
        .unwrap()
        .unwrap()
        .place;
    let begins = page::in_view(&c.store, &sid, &here)
        .unwrap()
        .expect("where a bare call begins");
    let lens = Lens {
        store: &c.store,
        here,
        in_view: Some(begins),
        words: None,
        span: None,
        cut: Default::default(),
        scope: None,
        cuts: crate::scrub::Cuts::none(),
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

/// `context.census` of a conversation counts the request the view builds: the place's window, read across
/// sessions (another session's exchange there included), a new chain as every view turn is, and no recall.
/// Nothing of the view is kept.
#[tokio::test]
async fn a_census_of_a_conversation_counts_the_views_request() {
    let r = view_rig();
    let c = &r.core;
    let other = session_in(c, None, &r.root);
    turn(c, &other, "alpha: the kestrel nests on the quay").await;
    let sid = session_in(c, None, &r.root);
    turn(c, &sid, "beta: the heron fishes the weir").await;
    let p = theseus_protocol::census::ContextCensusParams {
        session_id: Some(sid.clone()),
        ..Default::default()
    };
    let pointer = |c: &crate::Core| {
        let rec: SessionRecord = c.store.get_session(&sid).unwrap().unwrap();
        rec.compilation_id
    };
    let (before, compiled) = (
        pointer(c),
        crate::test_support::rows(c, "context.compiled").len(),
    );
    let (out, request, _) = c.context_census(&p).unwrap();
    let sent = serde_json::to_string(&request.messages).unwrap();
    assert!(sent.contains("the heron fishes the weir"), "{sent}");
    assert!(sent.contains("the kestrel nests on the quay"), "{sent}");
    assert_eq!((out.compile.as_str(), out.recall_tokens), ("recompile", 0));
    let noted = out.notes.iter().any(|n| n.contains("as the view builds"));
    assert!(noted, "{:?}", out.notes);
    assert!(record::of_turn(&c.store, "census").unwrap().is_empty());
    assert_eq!(pointer(c), before, "the session's chain is not moved");
    let after = crate::test_support::rows(c, "context.compiled").len();
    assert_eq!(after, compiled, "no compile is recorded");
}

/// A fork of `source` at its last node, speaking in no place (as `session.fork` makes one, by reference).
fn fork_in(store: &Store, source: &str) -> String {
    let nodes = store.transcript(source).unwrap();
    let (position, node) = nodes.last().map(|(p, n)| (*p, n.id.clone())).unwrap();
    let mut fork = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    fork.forked_from = Some(Box::new(theseus_protocol::ForkOrigin {
        session: source.into(),
        node,
        position,
        compilation: None,
        at_ms: 1,
    }));
    store.put_session(&fork.session_id, &fork).unwrap();
    fork.session_id
}

fn fork_of(c: &crate::Core, source: &str) -> String {
    fork_in(&c.store, source)
}

/// What the fork's turn asking `what` was sent, as one string.
fn sent_for(r: &Rig, what: &str) -> String {
    let q = (r.requests().into_iter())
        .rev()
        .find(|q| !q.tools.is_empty() && last_user(q).contains(what))
        .expect("the turn's request");
    serde_json::to_string(&q.messages).unwrap()
}

/// A fork speaks in no place of its source's: its first view turn lays the run it forked from in its window
/// (his DM's exchange as a real turn in slot G, not only beside his message).
#[tokio::test]
async fn a_forks_first_view_turn_shows_the_run_it_forked_from() {
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
}

// ---- the review's fixes (issue 44): the fork's window is uncut, its mark compared by position, its run the
// fork's own where the owner is alone; the history start is per place and knows what the compiler cleared.

/// One ended exchange `turn` of `place` in `session` at `at_ms`, said by the owner, written straight to the
/// store: its message, its reply, its exchange and its place's touch.
fn put_exchange(store: &Store, session: &str, place: &str, turn: &str, at_ms: u64, words: &str) {
    let ask = Node::user(session, Some(turn), "owner", words);
    let said = Node::assistant(
        session,
        turn,
        0,
        Body::AssistantMessage {
            blocks: vec![serde_json::json!({"type": "text", "text": "Noted."})],
            model: "m".into(),
            provider: "p".into(),
            stop_reason: None,
            usage: Default::default(),
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
            effort: None,
        },
    );
    let x = Exchange {
        turn_id: turn.into(),
        session_id: session.into(),
        place: place.into(),
        private: true,
        opened_by: exchange::Opener::Message,
        person: Some(exchange::OWNER.into()),
        start_ms: at_ms,
        end_ms: Some(at_ms + 500),
        ask: Some(ask.id.clone()),
        reply: Some(said.id.clone()),
        messages: vec![ask.id.clone()],
        stop: Some("end_turn".into()),
        rebuilt: false,
    };
    let mut recs = vec![ask.record().unwrap(), said.record().unwrap()];
    recs.push(exchange::exchange_record(&x).unwrap());
    recs.extend(exchange::touch(
        exchange::Root::Place(place),
        turn,
        session,
        at_ms,
    ));
    store.append(&recs).unwrap();
}

/// A session of no place, its record written.
fn bare_session(store: &Store) -> String {
    let r = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    store.put_session(&r.session_id, &r).unwrap();
    r.session_id
}

fn place_of(c: &crate::Core, turn: &str) -> String {
    exchange::exchange(&c.store, turn).unwrap().unwrap().place
}

fn start_of(c: &crate::Core, turn: &str) -> u64 {
    exchange::exchange(&c.store, turn)
        .unwrap()
        .unwrap()
        .start_ms
}

/// The turns of a bare `context.history` page of place `here`, read from `session`'s last compilation.
fn bare_page(c: &crate::Core, session: &str, here: &str) -> Vec<String> {
    let begins = page::in_view(&c.store, session, here).unwrap();
    let lens = Lens {
        store: &c.store,
        here: here.into(),
        in_view: begins,
        words: None,
        span: None,
        cut: Default::default(),
        scope: None,
        cuts: crate::scrub::Cuts::none(),
    };
    let walked = lens.walk(&HistoryArgs::default(), None, 0).unwrap();
    walked.page.lines.into_iter().map(|l| l.handle.0).collect()
}

/// `rec` kept again with its window replaced by `laid`: each (place, turn, ms) one exchange piece laid whole
/// in slot G, made from a window piece of the record's own.
fn relaid(store: &Store, mut rec: ViewRecord, laid: &[(&str, &str, u64)]) {
    let is_window = |s: &crate::view::budget::Shown| {
        s.piece.slot == Slot::Here && matches!(s.piece.range.space, Space::Exchanges { .. })
    };
    let template = (rec.shown.iter().find(|s| is_window(s)))
        .cloned()
        .expect("a window piece");
    rec.shown.retain(|s| !is_window(s));
    for (place, turn, at_ms) in laid {
        let mut s = template.clone();
        let at = crate::view::Pos {
            at_ms: *at_ms,
            id: (*turn).to_string(),
        };
        s.piece.range = crate::view::SourceRange {
            space: Space::Exchanges {
                place: (*place).to_string(),
            },
            first: at.clone(),
            last: at,
        };
        s.form = Form::Whole;
        rec.shown.push(s);
    }
    store.put_meta(&rec.key(), &rec).unwrap();
}

/// A long fork, with no folds and ample room: every inherited exchange is a candidate of its window, so the
/// oldest one's words reach the request (the window had been cut to its newest 39 before admission).
#[tokio::test]
async fn a_long_forks_oldest_inherited_words_reach_its_request() {
    let r = view_rig();
    let c = &r.core;
    let source = bare_session(&c.store);
    let dm = format!("discord:dm:{OWNER}");
    for i in 0..45u64 {
        let words = format!("inherited note {i}: the gannet count");
        let turn = format!("turn_old_{i:02}");
        put_exchange(&c.store, &source, &dm, &turn, 1_000_000 + i * 1_000, &words);
    }
    let fork = fork_of(c, &source);
    let first = turn(c, &fork, "fork: what was the first note?")
        .await
        .turn_id;
    let rec = record::newest(&c.store, &first).unwrap().expect("a view");
    let laid = window(&rec);
    for t in ["turn_old_00", "turn_old_44"] {
        assert!(laid.iter().any(|(x, _)| x == t), "{t}: {laid:?}");
    }
    let sent = sent_for(&r, "what was the first note");
    assert!(sent.contains("inherited note 0: the gannet"), "{sent}");
    assert!(sent.contains("inherited note 44: the gannet"), "{sent}");
}

/// The fork's window compares an inherited exchange with the place's `/new` mark by its whole position, as
/// the store's ladder does: of three at the mark's millisecond, only the one whose id sorts after the mark's.
#[test]
fn an_inherited_exchange_at_the_marks_millisecond_is_kept_only_after_the_mark() {
    use crate::view::fold::{Ladder, WINDOW_N};
    use theseus_memory::gate::{PointOfView as RulePov, Present as RulePresent};
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let source = bare_session(&store);
    for t in ["turn_a", "turn_b", "turn_c"] {
        put_exchange(&store, &source, "discord:dm:42", t, 5_000, t);
    }
    let fork = fork_in(&store, &source);
    let verified = crate::view_gate::Verified::for_tests(RulePov {
        anchor: "local".into(),
        place: theseus_memory::recall::Place::Private,
        present: RulePresent::OwnerAlone,
    });
    let (f, _) = crate::fork::window::Fork::permit(&store, &fork, "turn_f", verified).unwrap();
    let ids = |since: Option<&crate::view::Pos>| -> Vec<String> {
        let read = f.ladder(&store).read("local", WINDOW_N, since).unwrap();
        read.window.into_iter().map(|e| e.turn_id).collect()
    };
    let mark = crate::view::Pos {
        at_ms: 5_000,
        id: "turn_b".into(),
    };
    assert_eq!(ids(Some(&mark)), vec!["turn_c".to_string()]);
    assert_eq!(ids(None), vec!["turn_a", "turn_b", "turn_c"]);
}

/// An exchange laid whole whose tool result the compiler then cleared is not whole in the request: the bare
/// page begins with it. (Every window exchange laid whole, the page began just past the oldest.)
#[tokio::test]
async fn a_bare_history_call_begins_with_an_exchange_whose_tool_result_was_cleared() {
    let r = view_rig();
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    let t0 = turn(c, &sid, "zero: the tide").await.turn_id;
    let t1 = turn(c, &sid, "one: look around").await.turn_id;
    let t2 = turn(c, &sid, "two: the moon").await.turn_id;
    let t3 = turn(c, &sid, "three: and now").await.turn_id;
    let here = place_of(c, &t0);
    let result = (c.store.session_nodes(&sid).unwrap().into_iter())
        .map(|(_, n)| n)
        .find(|n| n.turn_id.as_deref() == Some(&t1) && matches!(n.body, Body::ToolResult { .. }))
        .expect("the tool result of turn one");
    let rec = record::newest(&c.store, &t3).unwrap().expect("a view");
    let laid = [
        (here.as_str(), t0.as_str(), start_of(c, &t0)),
        (here.as_str(), t1.as_str(), start_of(c, &t1)),
        (here.as_str(), t2.as_str(), start_of(c, &t2)),
    ];
    relaid(&c.store, rec, &laid);
    let before = bare_page(c, &sid, &here);
    assert!(!before.contains(&t1) && !before.contains(&t2), "{before:?}");
    // The compiler cleared turn one's tool result (a pass at the chain's start, or one carried).
    let rec: SessionRecord = c.store.get_session(&sid).unwrap().unwrap();
    let mut comp = (c
        .store
        .get_compilation(rec.compilation_id.as_deref().unwrap()))
    .unwrap()
    .unwrap();
    let budget = comp.budget.get_or_insert_with(|| {
        serde_json::from_value(serde_json::json!({"limit_tokens": 0, "used_tokens": 0})).unwrap()
    });
    budget.dropped.push(theseus_protocol::memory::BudgetDrop {
        node_id: Some(result.id),
        reason: "cleared".into(),
        tokens: 100,
        tier: "cleared".into(),
        ..Default::default()
    });
    let kept =
        theseus_store::NewRecord::json(theseus_store::kinds::COMPILATION, Some(&comp.id), &comp)
            .unwrap()
            .scoped(&comp.session_id);
    c.store.append(&[kept]).unwrap();
    let after = bare_page(c, &sid, &here);
    assert_eq!(after.first(), Some(&t1), "{after:?}");
    assert!(after.contains(&t0) && !after.contains(&t2), "{after:?}");
}

/// A fork's inherited exchanges of another place, laid in its window, do not move its own place's history
/// start: the place's history older than its own window is paged, not skipped.
#[tokio::test]
async fn a_forks_inherited_window_does_not_move_its_places_history_start() {
    let r = view_rig();
    let c = &r.core;
    let dm = session(c, Some(&format!("dm:{OWNER}")));
    let d0 = turn(c, &dm, "dm: the anchor is rusted").await.turn_id;
    let local = session(c, None);
    let mut a = Vec::new();
    for i in 0..4 {
        a.push(
            turn(c, &local, &format!("local {i}: the quay"))
                .await
                .turn_id,
        );
    }
    let fork = fork_of(c, &dm);
    let f0 = turn(c, &fork, "fork: and now?").await.turn_id;
    let here = place_of(c, &a[0]);
    assert_eq!(place_of(c, &f0), here, "the fork speaks in the local place");
    let rec = record::newest(&c.store, &f0).unwrap().expect("a view");
    let dm_place = place_of(c, &d0);
    let laid = [
        (dm_place.as_str(), d0.as_str(), start_of(c, &d0)),
        (here.as_str(), a[2].as_str(), start_of(c, &a[2])),
        (here.as_str(), a[3].as_str(), start_of(c, &a[3])),
    ];
    relaid(&c.store, rec, &laid);
    let paged = bare_page(c, &fork, &here);
    assert_eq!(paged, vec![a[1].clone(), a[0].clone()], "{paged:?}");
}

/// A fork carries the run it was made from: a fork of a shared channel's conversation shows its run to the
/// owner alone at his terminal (the place rule would have held another place's shared exchange out).
#[tokio::test]
async fn a_fork_of_a_shared_channels_conversation_shows_its_run_to_the_owner_alone() {
    let r = view_rig();
    let c = &r.core;
    let channel = session(c, Some("channel:7"));
    let said = turn(c, &channel, "channel: the ferry leaves at noon")
        .await
        .turn_id;
    assert!(
        !exchange::exchange(&c.store, &said)
            .unwrap()
            .unwrap()
            .private
    );
    let fork = fork_of(c, &channel);
    let first = turn(c, &fork, "fork: when does the ferry leave?")
        .await
        .turn_id;
    let rec = record::newest(&c.store, &first).unwrap().expect("a view");
    assert!(window(&rec).iter().any(|(t, _)| *t == said), "{rec:?}");
    let sent = sent_for(&r, "when does the ferry leave");
    assert!(sent.contains("the ferry leaves at noon"), "{sent}");
}

/// `context.census` asked from a surface that reads no private text (Discord) carries none of the view's
/// text: its own session's words and his DM's are in no byte of the serialized answer.
#[tokio::test]
async fn a_census_for_a_surface_that_reads_no_private_text_carries_none() {
    let r = view_rig();
    let c = &r.core;
    let dm = session(c, Some(&format!("dm:{OWNER}")));
    turn(c, &dm, "dm: the vault word is plover").await;
    let sid = session_in(c, None, &r.root);
    turn(c, &sid, "desk: the heron fishes the weir").await;
    let p = theseus_protocol::census::ContextCensusParams {
        session_id: Some(sid.clone()),
        ..Default::default()
    };
    let (_, request, _) = c.context_census(&p).unwrap();
    let counted = serde_json::to_string(&request.messages).unwrap();
    assert!(counted.contains("the heron fishes the weir"), "{counted}");
    let client = crate::tests_m3::surface("discord", crate::approval::Surface::Discord);
    let params = serde_json::json!({"session_id": sid});
    let said = match crate::tests_m3::rpc_as(c, client, "context.census", params).await {
        Ok(v) => v.to_string(),
        Err(e) => e.message,
    };
    for private in ["plover", "heron", "fishes the weir"] {
        assert!(!said.contains(private), "{private}: {said}");
    }
}
