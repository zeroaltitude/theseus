//! The view record's readers through the whole core (issue 44): `view.show` answers the owner in person
//! alone, over real protocol connections; a seal proposed after a view was kept keeps its piece out of the
//! owner's inspection and of the model's own look; and the look reads this turn's presence alone. A fold and a
//! task's card are listed when today's records prove them, and the listing agrees with the stored counts.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::json;
use theseus_protocol::view::{ViewPiece, ViewShowResult, ViewShown};
use theseus_protocol::{error_code, method};

use crate::approval::{Client, Surface};
use crate::exchange::{self, Exchange, Opener};
use crate::peer_proc::{Behind, JOB};
use crate::tests_m3::{rpc_as, surface};
use crate::tests_places::{rig_scripted, session, Rig, LAB};
use crate::view::budget::{Shown, View};
use crate::view::record;
use crate::view::select::Band;
use crate::view::{
    Anchor, Audience, Author, Form, Handle, Marks, Piece, PointOfView, Pos, Reason, Sizes,
    SourceRange, Space,
};
use crate::Core;

fn rig() -> Rig {
    rig_scripted(|_| {}, |_| crate::provider::Scripted::text("ok"))
}

/// An exchange of `session` at `place`, as the ledger writes it.
fn exchange_of(core: &Core, turn: &str, session: &str, place: &str) {
    core.store
        .append(&[exchange::exchange_record(&an_exchange(turn, session, place)).unwrap()])
        .unwrap();
}

/// [`exchange_of`] for each of `turns`, a hundred to a frame.
fn exchanges_of(core: &Core, turns: &[String], session: &str, place: &str) {
    let records: Vec<_> = (turns.iter())
        .map(|t| exchange::exchange_record(&an_exchange(t, session, place)).unwrap())
        .collect();
    for frame in records.chunks(100) {
        core.store.append(frame).unwrap();
    }
}

fn an_exchange(turn: &str, session: &str, place: &str) -> Exchange {
    Exchange {
        turn_id: turn.into(),
        session_id: session.into(),
        place: place.into(),
        private: true,
        opened_by: Opener::Message,
        person: None,
        start_ms: 1_000,
        end_ms: Some(2_000),
        ask: None,
        reply: None,
        messages: Vec::new(),
        stop: None,
        rebuilt: false,
    }
}

fn piece(place: &str, turn: &str) -> Piece {
    let pos = Pos {
        at_ms: 1_000,
        id: turn.into(),
    };
    Piece {
        range: SourceRange {
            space: Space::Exchanges {
                place: place.into(),
            },
            first: pos.clone(),
            last: pos,
        },
        band: Band::Recall,
        slot: Band::Recall.row().slot,
        reasons: BTreeSet::from([Reason::Recalled]),
        form: Form::Line,
        marks: Marks {
            place: Some(place.into()),
            audience: Audience::Private,
            author: Author::His {
                may_authorise: true,
            },
            outside: false,
            sealed: false,
            veiled: false,
        },
        sizes: Sizes {
            whole: 30,
            spoken: 20,
            line: 10,
        },
        handle: Handle(format!("x:{turn}")),
        unit: None,
    }
}

/// A view at `anchor` holding `pieces`, kept for `turn` under `compilation`.
fn keep(core: &Core, turn: &str, compilation: &str, anchor: &str, pieces: Vec<Piece>) {
    keep_at(core, turn, compilation, anchor, pieces, 5_000);
}

/// [`keep`], built at `now_ms`.
fn keep_at(
    core: &Core,
    turn: &str,
    compilation: &str,
    anchor: &str,
    pieces: Vec<Piece>,
    now_ms: u64,
) {
    let shown: Vec<Shown> = pieces
        .into_iter()
        .map(|p| Shown {
            form: p.form,
            tokens: p.tokens(),
            piece: p,
        })
        .collect();
    let view = View {
        pov: PointOfView {
            anchor: Anchor::Place(anchor.into()),
            person: None,
            present: crate::view::Present::OwnerAlone,
            at_hand: None,
            now_ms,
        },
        tokens: shown.iter().map(|s| s.tokens).sum(),
        shown,
        dropped: Vec::new(),
        residue: Vec::new(),
    };
    record::keep(&core.store, turn, compilation, &view).unwrap();
}

async fn show(
    core: &Arc<Core>,
    client: Client,
    params: serde_json::Value,
) -> Result<String, String> {
    match rpc_as(core, client, method::VIEW_SHOW, params).await {
        Ok(v) => Ok(v.to_string()),
        Err(e) => {
            assert_eq!(e.code, error_code::REFUSED, "{e:?}");
            Err(e.message)
        }
    }
}

/// A turn's view is a read (anything goes, 2026-11-22): an agent's process at the terminal, a Theseus job (its
/// process, or a shell carrying its marker), an agent behind the web UI and the Discord binding are each
/// answered, the record and its text alike, for another place's turn.
#[tokio::test]
async fn view_show_answers_every_reader() {
    let r = rig();
    let lab_place = format!("discord:channel:{LAB}");
    let lab = session(&r.core, Some(&format!("channel:{LAB}")));
    exchange_of(&r.core, "t-lab", &lab, &lab_place);
    keep(
        &r.core,
        "t-lab",
        "cmp_lab",
        &lab_place,
        vec![piece(&lab_place, "t-lab")],
    );
    let named = json!({"turn_id": "t-lab"});
    let agent = surface("sock#2", Surface::Cli).behind(Behind::Agent("claude".into()));
    let job = surface("sock#3", Surface::Cli).behind(Behind::Agent(JOB.into()));
    let web_agent = surface("web#2", Surface::Web).behind(Behind::Agent("claude".into()));
    let discord = Client::new("discord", Surface::Discord);
    for (who, client, params) in [
        ("agent", agent.clone(), named.clone()),
        (
            "agent, text",
            agent,
            json!({"turn_id": "t-lab", "text": true}),
        ),
        ("job", job, named.clone()),
        (
            "job's marker",
            surface("sock#4", Surface::Cli),
            json!({"turn_id": "t-lab", "from_job": "s1"}),
        ),
        ("web agent", web_agent, named.clone()),
        ("discord", discord, named.clone()),
        ("owner", surface("sock#1", Surface::Cli), named.clone()),
    ] {
        let shown = show(&r.core, client, params)
            .await
            .unwrap_or_else(|e| panic!("{who}: {e}"));
        assert!(shown.contains("x:t-lab"), "{who}: {shown}");
    }
    let web = show(
        &r.core,
        surface("web#1", Surface::Web),
        json!({"turn_id": "t-lab", "tally": true}),
    )
    .await
    .unwrap();
    assert!(
        web.contains("x:t-lab") && web.contains("\"tallies\":[{"),
        "{web}"
    );
}

/// Of a turn's many compilations, the look reads the newest.
#[tokio::test]
async fn the_look_reads_this_turns_newest_view() {
    let r = rig();
    let desk = session(&r.core, None);
    exchange_of(&r.core, "t-here", &desk, "local");
    for i in 0..100u64 {
        let c = format!("cmp_{i:03}");
        keep_at(
            &r.core,
            "t-desk",
            &c,
            "local",
            vec![piece("local", "t-here")],
            5_000 + i,
        );
    }
    let said = crate::look::view::words(&r.core, &desk, "t-desk");
    assert!(
        said.contains("view cmp_099") && !said.contains("cmp_098"),
        "{said}"
    );
    assert_eq!(said.matches("x:t-here").count(), 1, "{said}");
}

/// The owner alone is shown every piece of this turn's view, and never a piece's text.
#[tokio::test]
async fn the_owner_alone_is_shown_every_piece_and_no_text() {
    let r = rig();
    let desk = session(&r.core, None);
    exchange_of(&r.core, "t-here", &desk, "local");
    exchange_of(&r.core, "t-two", &desk, "local");
    keep(
        &r.core,
        "t-desk",
        "cmp_desk",
        "local",
        vec![piece("local", "t-here"), piece("local", "t-two")],
    );
    let said = crate::look::view::words(&r.core, &desk, "t-desk");
    assert!(
        said.contains("x:t-here") && said.contains("x:t-two"),
        "{said}"
    );
    assert!(said.contains("view cmp_desk"), "{said}");
    assert!(!said.contains(" | "), "no text: {said}");
    let restricted = crate::view::record::show::RESTRICTED;
    assert!(!said.contains(restricted), "{said}");
}

/// A turn with no view says so.
#[tokio::test]
async fn a_turn_with_no_view_says_so() {
    let r = rig();
    let desk = session(&r.core, None);
    let said = crate::look::view::words(&r.core, &desk, "t-desk");
    assert!(said.contains("no view record"), "{said}");
}

/// Fold node `id` at `place` and `level`, standing for `turns` (in the store or not), first at `ms`.
fn fold_node(core: &Core, id: &str, place: &str, level: u8, turns: &[&str], ms: u64) {
    let node = crate::ladder::fold::FoldNode {
        id: id.into(),
        place: place.into(),
        level,
        exchanges: turns.iter().map(|t| t.to_string()).collect(),
        first_ms: ms,
        last_ms: ms,
        his: vec![],
        counted: 0,
        children: vec![],
        title: "FOLD TITLE".into(),
        account: "FOLD ACCOUNT".into(),
        marks: Marks {
            place: Some(place.into()),
            audience: Audience::Private,
            author: Author::Theseus,
            outside: false,
            sealed: false,
            veiled: false,
        },
        model: None,
        writer: 1,
        at_ms: ms,
    };
    core.store
        .append(&crate::ladder::fold::records(&node).unwrap())
        .unwrap();
}

/// A fold piece of `place` at `level` over `first..last`, by `handle`.
fn fold_piece(place: &str, level: u8, (first, last): (&str, &str), handle: &str) -> Piece {
    let mut p = piece(place, first);
    p.range.space = Space::Folds {
        place: place.into(),
        level,
    };
    p.range.last.id = last.into();
    p.band = Band::Ladder;
    p.slot = Band::Ladder.row().slot;
    p.reasons = BTreeSet::from([Reason::Ladder]);
    p.form = Form::Whole;
    p.marks.author = Author::Theseus;
    p.handle = Handle(handle.into());
    p
}

/// Task `task`'s card piece, as the card's reader makes it.
fn task_piece(task: &str) -> Piece {
    let mut p = piece("local", task);
    p.range.space = Space::Task { task: task.into() };
    p.band = Band::TaskCard;
    p.slot = Band::TaskCard.row().slot;
    p.reasons = BTreeSet::from([Reason::Task]);
    p.form = Form::Whole;
    p.marks.author = Author::Theseus;
    p.handle = Handle(format!("task:{task}@v1"));
    p
}

/// `view.show` from the owner's own terminal, read back.
async fn shown_of(core: &Arc<Core>, params: serde_json::Value) -> ViewShowResult {
    let v = rpc_as(
        core,
        surface("sock#1", Surface::Cli),
        method::VIEW_SHOW,
        params,
    )
    .await
    .unwrap();
    serde_json::from_value(v).unwrap()
}

/// Every piece listed, with its slot.
fn listed(v: &ViewShown) -> Vec<(&str, &ViewPiece)> {
    (v.slots.iter())
        .flat_map(|s| s.pieces.iter().map(move |p| (s.slot.as_str(), p)))
        .collect()
}

/// The turns of `sid`, in order.
fn turns_of(core: &Core, sid: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (_, n) in core.store.session_nodes(sid).unwrap() {
        if let Some(t) = n.turn_id.clone().filter(|t| !out.contains(t)) {
            out.push(t);
        }
    }
    out
}

/// Three real turns, a level-1 fold over the first two, and a fourth turn, which lays the fold: the rig, the
/// session, the fourth turn and the two folded.
async fn a_turn_with_a_fold() -> (crate::tests_places::Rig, String, String, Vec<String>) {
    let r = crate::tests_places::rig_with(|_| {});
    let c = &r.core;
    let sid = crate::tests_private_view::session_in(c, None, &r.root);
    for m in ["first: the tide", "second: the moon", "third: the stars"] {
        crate::tests_places::turn(c, &sid, m).await;
    }
    let turns = turns_of(c, &sid);
    let first = exchange::exchange(&c.store, &turns[0]).unwrap().unwrap();
    let folded = [turns[0].as_str(), turns[1].as_str()];
    fold_node(c, "fold-tide-1", &first.place, 1, &folded, first.start_ms);
    crate::tests_places::turn(c, &sid, "fourth: and now").await;
    let last = (c.store.get_session::<crate::session::SessionRecord>(&sid))
        .unwrap()
        .unwrap()
        .last_turn_id
        .unwrap();
    let folded = folded.iter().map(|t| t.to_string()).collect();
    (r, sid, last, folded)
}

/// A real task, made by a turn's call: the rig and the task.
async fn a_task() -> (
    crate::tests_task_graph::Rig,
    theseus_protocol::tasks::TaskRecord,
) {
    let r = crate::tests_task_graph::rig();
    let s = crate::tests_task_graph::session(&r.core);
    r.calls(&[(
        "task_create",
        json!({"title": "Chart the outer harbour", "acceptance": ["a chart exists"]}),
    )]);
    crate::tests_task_graph::turn(&r.core, &s, "plan it").await;
    let t = r.by_title("Chart the outer harbour");
    (r, t)
}

/// The names a message holds: the one task.
struct Named(String);

impl crate::view::select::sources::Names for Named {
    fn named(&self, _: &str) -> anyhow::Result<Vec<String>> {
        Ok(vec![crate::task_graph::joined::thing_of(&self.0)])
    }
}

/// Task `task`'s card as a turn's view lays it: the card reader's reference, as a piece of the called slot.
fn joined_card(core: &Core, task: &str) -> Piece {
    use crate::view::select::sources::TaskCards;
    let cards = crate::task_graph::joined::StoreCards {
        store: &core.store,
        kernel: &core.kernel,
        names: &Named(task.into()),
        homes: &core.runner,
        judged: None,
    };
    let pov = PointOfView {
        anchor: Anchor::Place("local".into()),
        person: Some("owner".into()),
        present: crate::view::Present::OwnerAlone,
        at_hand: None,
        now_ms: theseus_protocol::now_unix_ms(),
    };
    let got = cards.joined(&pov, "how is the chart going").unwrap();
    assert_eq!(got.len(), 1, "the card is called in");
    let r = got.into_iter().next().unwrap();
    Piece {
        range: r.range,
        band: Band::TaskCard,
        slot: Band::TaskCard.row().slot,
        reasons: BTreeSet::from([Reason::Task]),
        form: r.form,
        marks: r.marks,
        sizes: r.sizes,
        handle: r.handle,
        unit: r.unit,
    }
}

/// A turn laid a fold: the owner's listing names it in slot `here`, by the exchanges it stands for and its
/// tokens as the record holds them, and never its words, even when the text is asked for.
#[tokio::test]
async fn view_show_lists_a_laid_fold_with_its_range_and_tokens() {
    let (r, _, last, folded) = a_turn_with_a_fold().await;
    let rec = record::newest(&r.core.store, &last)
        .unwrap()
        .expect("the turn kept its view");
    let laid: Vec<&Shown> = (rec.shown.iter())
        .filter(|s| matches!(s.piece.range.space, Space::Folds { .. }))
        .collect();
    assert_eq!(laid.len(), 1, "the turn laid the fold");
    let shown = shown_of(&r.core, json!({"turn_id": last, "text": true})).await;
    let v = (shown.views.iter())
        .find(|v| v.compilation == rec.compilation)
        .expect("the newest view");
    let folds: Vec<_> = (listed(v).into_iter())
        .filter(|(_, p)| p.kind == "folds")
        .collect();
    assert_eq!(folds.len(), 1, "{}", theseus_protocol::view::lines(v));
    let (slot, f) = folds[0];
    assert_eq!(slot, "here");
    assert_eq!((&f.first, &f.last), (&folded[0], &folded[1]));
    assert_eq!(f.handle, laid[0].piece.handle.0);
    assert!(f.tokens == laid[0].tokens && f.tokens > 0, "{f:?}");
    assert_eq!(f.text, None, "never a fold's words");
    // Its rung, and no other piece's.
    assert_eq!(f.level, Some(1));
    assert!(
        (listed(v).iter()).all(|(_, p)| (p.kind == "folds") == p.level.is_some()),
        "{v:?}"
    );
    let line = format!("fold-tide-1 folds@1 {}..{} [ladder]", folded[0], folded[1]);
    let lines = theseus_protocol::view::lines(v);
    assert!(lines.contains(&line), "{lines}");
    let said = serde_json::to_string(&shown).unwrap();
    assert!(
        !said.contains("FOLD TITLE") && !said.contains("FOLD ACCOUNT"),
        "{said}"
    );
}

/// A joined task's card a turn's view laid: listed in slot `called` as the task, by its id and tokens, with
/// no words.
#[tokio::test]
async fn view_show_lists_a_joined_task_card() {
    let (r, t) = a_task().await;
    let card = joined_card(&r.core, &t.id);
    let tokens = card.tokens();
    keep(&r.core, "t-ask", "cmp_ask", "local", vec![card]);
    let shown = shown_of(&r.core, json!({"turn_id": "t-ask", "text": true})).await;
    let v = &shown.views[0];
    let tasks: Vec<_> = (listed(v).into_iter())
        .filter(|(_, p)| p.kind == "task")
        .collect();
    assert_eq!(tasks.len(), 1, "{}", theseus_protocol::view::lines(v));
    let (slot, c) = tasks[0];
    assert_eq!(slot, "called");
    assert_eq!((&c.first, &c.last), (&t.id, &t.id));
    assert!(c.tokens == tokens && c.tokens > 0, "{c:?}");
    assert_eq!(c.text, None, "never a card's words");
    assert_eq!((v.totals.shown, v.totals.tokens), (1, tokens));
}

/// The owner's listing of each view and the view's stored counts agree, slot by slot and in total: what the
/// stored tally counts, the listing lists, once.
async fn agree(core: &Arc<Core>, turn: &str) {
    let shown = shown_of(core, json!({"turn_id": turn, "tally": true})).await;
    assert!(!shown.views.is_empty(), "{turn} has views");
    for v in &shown.views {
        let said = theseus_protocol::view::lines(v);
        let t = (shown.tallies.iter())
            .find(|t| t.compilation == v.compilation)
            .expect("its tally");
        let slots: std::collections::BTreeMap<&str, (u64, u64)> = (v.slots.iter())
            .map(|s| (s.slot.as_str(), (s.pieces.len() as u64, s.tokens)))
            .collect();
        let tallied: std::collections::BTreeMap<&str, (u64, u64)> = (t.slots.iter())
            .map(|(k, c)| (k.as_str(), (c.pieces, c.tokens)))
            .collect();
        assert_eq!(slots, tallied, "{said}");
        let dropped = t
            .dropped
            .values()
            .fold((0, 0), |(n, k), c| (n + c.pieces, k + c.tokens));
        assert_eq!(
            (v.totals.dropped, v.totals.dropped_tokens),
            dropped,
            "{said}"
        );
        let rec = record::read(&core.store, &v.compilation).unwrap().unwrap();
        let shown_tokens: u64 = t.slots.values().map(|c| c.tokens).sum();
        assert_eq!(v.totals.tokens, shown_tokens, "{said}");
        if rec.residue.is_empty() {
            assert_eq!(v.totals.tokens, t.tokens, "{said}");
        }
    }
}

/// A turn that laid a fold, and a view that holds a task's card beside an exchange: the owner's listing and
/// the stored tally agree on every view.
#[tokio::test]
async fn the_owners_listing_and_the_stored_tally_agree() {
    let (r, _, last, _) = a_turn_with_a_fold().await;
    agree(&r.core, &last).await;

    let (r, t) = a_task().await;
    let desk = crate::tests_task_graph::session(&r.core);
    exchange_of(&r.core, "t-here", &desk, "local");
    let pieces = vec![joined_card(&r.core, &t.id), piece("local", "t-here")];
    keep(&r.core, "t-ask", "cmp_ask", "local", pieces);
    agree(&r.core, "t-ask").await;
}

/// The model's own look at a turn that laid a fold lists the fold, by its handle and kind.
#[tokio::test]
async fn the_models_look_lists_its_fold() {
    let (r, sid, last, folded) = a_turn_with_a_fold().await;
    let said = crate::look::view::words(&r.core, &sid, &last);
    let line = format!("{}..{}", folded[0], folded[1]);
    assert!(
        said.lines()
            .any(|l| l.contains("fold-tide-1 folds") && l.contains(&line)),
        "{said}"
    );
    assert!(!said.contains("FOLD TITLE"), "{said}");
}

/// The guard's sources: two exchanges here and one elsewhere, a session whose place cannot be read and its
/// exchange, a proven node and one damaged node of each kind, and a copy of task `t` homed in that session.
fn damaged_sources(c: &Arc<Core>, t: &theseus_protocol::tasks::TaskRecord) {
    let desk = crate::tests_task_graph::session(c);
    for turn in ["t-a", "t-b"] {
        exchange_of(c, turn, &desk, "local");
    }
    exchange_of(c, "t-far", &desk, "discord:dm:7");
    // A session whose place cannot be read today: its record does not read as a session's.
    let unread = "sess_unread".to_string();
    c.store
        .put_session(&unread, &json!({"not": "a session"}))
        .unwrap();
    exchange_of(c, "t-unread", &unread, "local");
    // The one proven: a whole node of the range's place.
    fold_node(c, "fold-whole", "local", 1, &["t-a", "t-b"], 10);
    // A node one of whose exchanges is no longer in the store.
    fold_node(c, "fold-hole", "local", 1, &["t-a", "t-gone"], 20);
    // A node of another place, named by a range of this one.
    fold_node(c, "fold-there", "discord:dm:7", 1, &["t-far"], 30);
    // A node of this place standing for another place's exchange.
    fold_node(c, "fold-mixed", "local", 1, &["t-a", "t-far"], 40);
    // A node of this place standing for an exchange of the session whose place does not read.
    fold_node(c, "fold-unread", "local", 1, &["t-a", "t-unread"], 50);
    // A node of another place standing for this place's exchanges: only the node's own place is wrong.
    fold_node(c, "fold-away", "discord:dm:7", 1, &["t-a", "t-b"], 60);
    // A task homed in the session whose place does not read: a copy of a real task under another id.
    let mut homeless = t.clone();
    homeless.id = "tsk_homeless".into();
    homeless.origin.session = unread;
    c.store
        .append(&[crate::task_graph::record(&homeless).unwrap()])
        .unwrap();
}

/// A fold is listed only when today's node, found by the record's handle, is of the range's place, level and
/// ends and reads whole, every exchange it stands for of that place and from a session whose place reads (the
/// proven node's own handle, named at another level or over other ends, is not); a task's card only
/// when the task reads today and its home session's place reads (a place reads unless the session's own
/// record cannot be read: anything goes, 2026-11-22). Each other is left out of the listing and of
/// every total, and a dropped one held is taken out of its band's residue as before; a dropped one proven
/// stays in it.
#[tokio::test]
async fn a_fold_or_task_whose_source_is_gone_is_not_listed_or_counted() {
    let (r, t) = a_task().await;
    let c = &r.core;
    damaged_sources(c, &t);
    let pieces = vec![
        fold_piece("local", 1, ("t-a", "t-b"), "fold-whole"),
        fold_piece("local", 1, ("t-a", "t-b"), "fold-none"),
        fold_piece("local", 1, ("t-a", "t-gone"), "fold-hole"),
        fold_piece("local", 1, ("t-far", "t-far"), "fold-there"),
        fold_piece("local", 1, ("t-a", "t-far"), "fold-mixed"),
        fold_piece("local", 1, ("t-a", "t-unread"), "fold-unread"),
        fold_piece("local", 1, ("t-a", "t-b"), "fold-away"),
        // The proven node's handle, at a level it is not, and over ends it does not have.
        fold_piece("local", 2, ("t-a", "t-b"), "fold-whole"),
        fold_piece("local", 1, ("t-a", "t-a"), "fold-whole"),
        task_piece(&t.id),
        task_piece("tsk_none"),
        task_piece("tsk_homeless"),
    ];
    let proven = (pieces[0].tokens(), pieces[9].tokens());
    let shown: Vec<Shown> = (pieces.into_iter())
        .map(|p| Shown {
            form: p.form,
            tokens: p.tokens(),
            piece: p,
        })
        .collect();
    let drop = |handle: &str, first: &str, last: &str| {
        let p = fold_piece("local", 1, (first, last), handle);
        crate::view::budget::Dropped {
            range: p.range,
            slot: p.slot,
            band: Band::Ladder,
            reasons: p.reasons,
            handle: p.handle,
            tokens: 70,
            why: crate::view::budget::DropWhy::OverBudget,
        }
    };
    let view = View {
        pov: PointOfView {
            anchor: Anchor::Place("local".into()),
            person: None,
            present: crate::view::Present::OwnerAlone,
            at_hand: None,
            now_ms: 5_000,
        },
        tokens: shown.iter().map(|s| s.tokens).sum(),
        shown,
        dropped: vec![
            drop("fold-whole", "t-a", "t-b"),
            drop("fold-none", "t-a", "t-b"),
        ],
        residue: vec![crate::view::budget::Residue {
            band: Band::Ladder,
            pieces: 2,
            tokens: 140,
        }],
    };
    record::keep(&c.store, "t-gone", "cmp_gone", &view).unwrap();
    let shown = shown_of(c, json!({"turn_id": "t-gone"})).await;
    let v = &shown.views[0];
    let said = theseus_protocol::view::lines(v);
    let handles: Vec<&str> = (listed(v).into_iter())
        .map(|(_, p)| p.handle.as_str())
        .collect();
    let card = format!("task:{}@v1", t.id);
    assert_eq!(handles, ["fold-whole", card.as_str()], "{said}");
    assert_eq!(
        (v.totals.shown, v.totals.tokens),
        (2, proven.0 + proven.1),
        "{said}"
    );
    let dropped: Vec<&str> = v.dropped.iter().map(|d| d.handle.as_str()).collect();
    assert_eq!(dropped, ["fold-whole"], "{said}");
    assert_eq!(
        (v.totals.dropped, v.totals.dropped_tokens),
        (1, 70),
        "{said}"
    );
    let residue: Vec<_> = (v.residue.iter())
        .map(|r| (r.band.as_str(), r.pieces, r.tokens))
        .collect();
    assert_eq!(residue, [("ladder", 1, 70)], "{said}");
}

/// Three views of one turn that each hold one fold over three exchanges and one task's card: the owner's call
/// reads the fold's node and each exchange it stands for once, and the tasks once, however many views hold
/// them.
#[tokio::test]
async fn a_turns_views_prove_a_fold_and_a_card_once_per_call() {
    let (r, t) = a_task().await;
    let c = &r.core;
    let desk = crate::tests_task_graph::session(c);
    let turns = ["t-1", "t-2", "t-3"];
    for turn in turns {
        exchange_of(c, turn, &desk, "local");
    }
    fold_node(c, "fold-three", "local", 1, &turns, 10);
    for i in 0..3u64 {
        let pieces = vec![
            fold_piece("local", 1, ("t-1", "t-3"), "fold-three"),
            task_piece(&t.id),
        ];
        keep_at(c, "t-many", &format!("cmp_{i}"), "local", pieces, 5_000 + i);
    }
    let views = record::of_turn(&c.store, "t-many").unwrap();
    assert_eq!(views.len(), 3);
    let (shown, reads) = c.owners_all(&views, None);
    for v in &shown {
        assert_eq!(v.totals.shown, 2, "{}", theseus_protocol::view::lines(v));
    }
    // The node and its three exchanges, and the task list.
    assert_eq!(reads, 1 + 3 + 1);
}

/// The owner's source at `local`, as `view.show` makes it.
fn owners_source(core: &Core) -> crate::view::record::source::GateSource<'_> {
    let pov = theseus_memory::gate::PointOfView {
        anchor: "local".into(),
        place: theseus_memory::recall::Place::Private,
        present: theseus_memory::gate::Present::OwnerAlone,
    };
    crate::view::record::source::GateSource::new(&core.runner, pov, None)
}

/// Whether `source` lists `p` today.
fn lists(source: &dyn crate::view::record::show::Source, p: &Piece) -> bool {
    source.may_show_piece(&p.range, &p.handle, Some(&p.marks))
}

/// One source, as one `view.show` call keeps it: a fold it proved, read again after one of its exchanges was
/// ended (`Store::delete_meta`) and that write returned, is refused. The stamp notices a write that finished
/// between two views of one call: a best-effort freshener, not a guarantee (`view::record::source`).
#[tokio::test]
async fn a_fold_proved_once_is_refused_once_an_exchange_of_it_goes() {
    let r = rig();
    let c = &r.core;
    let desk = session(c, None);
    for turn in ["t-a", "t-b"] {
        exchange_of(c, turn, &desk, "local");
    }
    fold_node(c, "fold-whole", "local", 1, &["t-a", "t-b"], 10);
    let p = fold_piece("local", 1, ("t-a", "t-b"), "fold-whole");
    let source = owners_source(c);
    assert!(lists(&source, &p), "proven");
    c.store
        .delete_meta(&format!("{}t-b", exchange::EXCHANGE_PREFIX))
        .unwrap();
    assert!(!lists(&source, &p), "an exchange of it is gone");
    assert!(!lists(&owners_source(c), &p), "a fresh source agrees");
}

/// The same for a task's card: proven, then its home session stops reading (a session with no binding, whose
/// own record then does not decode: a bound session's place is its binding's); and, on another store, proven,
/// then its record stops reading (a later record of it that does not decode: no writer ends a task's record).
/// Each is refused through the source that proved it.
#[tokio::test]
async fn a_task_card_proved_once_is_refused_once_its_home_or_its_record_goes() {
    let (r, t) = a_task().await;
    let c = &r.core;
    let home =
        crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    c.store.put_session(&home.session_id, &home).unwrap();
    let mut homed = t.clone();
    homed.id = "tsk_homed".into();
    homed.origin.session = home.session_id.clone();
    c.store
        .append(&[crate::task_graph::record(&homed).unwrap()])
        .unwrap();
    let p = task_piece(&homed.id);
    let source = owners_source(c);
    assert!(lists(&source, &p), "proven");
    c.store
        .put_session(&home.session_id, &json!({"not": "a session"}))
        .unwrap();
    assert!(!lists(&source, &p), "its home does not read");

    let (r, t) = a_task().await;
    let c = &r.core;
    let p = task_piece(&t.id);
    let source = owners_source(c);
    assert!(lists(&source, &p), "proven");
    let gone = theseus_store::NewRecord::json(
        theseus_store::kinds::TASK,
        Some(&t.id),
        &json!({"not": "a task"}),
    )
    .unwrap();
    c.store.append(&[gone]).unwrap();
    assert!(!lists(&source, &p), "its record does not read");
}

/// A view of turn `t-call` at `local` holding one fold over two exchanges of a session here: the session.
fn a_kept_fold(c: &Core) -> String {
    let desk = session(c, None);
    for turn in ["t-a", "t-b"] {
        exchange_of(c, turn, &desk, "local");
    }
    fold_node(c, "fold-whole", "local", 1, &["t-a", "t-b"], 10);
    let p = fold_piece("local", 1, ("t-a", "t-b"), "fold-whole");
    keep(c, "t-call", "cmp_call", "local", vec![p]);
    desk
}

/// Nothing one call proved is reused by another (the reader's contract): `view.show` lists the fold, one of
/// its exchanges is ended and that write returns, and the next call refuses it. Each call makes its own
/// sources and drops them when it answers, so a write that finished before a call began is in that call.
#[tokio::test]
async fn each_view_show_call_proves_from_the_store_as_it_stands() {
    let r = rig();
    let c = &r.core;
    a_kept_fold(c);
    let handles = |s: &ViewShowResult| -> Vec<String> {
        (listed(&s.views[0]).into_iter())
            .map(|(_, p)| p.handle.clone())
            .collect()
    };
    let first = shown_of(c, json!({"turn_id": "t-call"})).await;
    assert_eq!(handles(&first), ["fold-whole"], "proven");
    c.store
        .delete_meta(&format!("{}t-b", exchange::EXCHANGE_PREFIX))
        .unwrap();
    let second = shown_of(c, json!({"turn_id": "t-call"})).await;
    let said = theseus_protocol::view::lines(&second.views[0]);
    assert!(handles(&second).is_empty(), "an exchange of it is gone: {said}");
    assert_eq!(second.views[0].totals.shown, 0, "{said}");
}

/// The same through the model's look: each look makes its own source, so the next look refuses the fold.
#[tokio::test]
async fn each_look_proves_from_the_store_as_it_stands() {
    let r = rig();
    let c = &r.core;
    let desk = a_kept_fold(c);
    let first = crate::look::view::words(c, &desk, "t-call");
    assert!(first.contains("fold-whole folds"), "proven: {first}");
    c.store
        .delete_meta(&format!("{}t-b", exchange::EXCHANGE_PREFIX))
        .unwrap();
    let second = crate::look::view::words(c, &desk, "t-call");
    assert!(
        !second.contains("fold-whole"),
        "an exchange of it is gone: {second}"
    );
}

/// On a store that does not move, a second call reuses nothing the first proved: each proves the fold's node,
/// its three exchanges and the task list itself.
#[tokio::test]
async fn a_second_call_proves_again_what_the_first_proved() {
    let (r, t) = a_task().await;
    let c = &r.core;
    let desk = crate::tests_task_graph::session(c);
    let turns = ["t-1", "t-2", "t-3"];
    for turn in turns {
        exchange_of(c, turn, &desk, "local");
    }
    fold_node(c, "fold-three", "local", 1, &turns, 10);
    let pieces = vec![
        fold_piece("local", 1, ("t-1", "t-3"), "fold-three"),
        task_piece(&t.id),
    ];
    keep(c, "t-twice", "cmp_twice", "local", pieces);
    let views = record::of_turn(&c.store, "t-twice").unwrap();
    let at = c.store.last_position();
    for call in ["first", "second"] {
        let (shown, reads) = c.owners_all(&views, None);
        assert_eq!(shown[0].totals.shown, 2, "{call}");
        assert_eq!(reads, 1 + 3 + 1, "{call}");
    }
    assert_eq!(c.store.last_position(), at, "the store did not move");
}

/// The stamp a source keeps with each proof is the store's last position, so for the freshener to notice a
/// finished write, every writer of what a proof reads moves it: a fold's node and its handle, an exchange's
/// record, a session's record (its place) and a task's record, each written and each ended. Necessary, not
/// sufficient: when a write is published to the reader is another matter (`view::record::source`).
#[tokio::test]
async fn every_write_a_proof_reads_moves_the_stores_position() {
    let (r, t) = a_task().await;
    let c = &r.core;
    let desk = crate::tests_task_graph::session(c);
    let moved = |what: &str, write: &dyn Fn()| {
        let before = c.store.last_position();
        write();
        assert!(c.store.last_position() > before, "{what}");
    };
    moved("an exchange written", &|| {
        exchange_of(c, "t-a", &desk, "local")
    });
    moved("a fold's node written", &|| {
        fold_node(c, "fold-a", "local", 1, &["t-a"], 10)
    });
    moved("an exchange ended", &|| {
        c.store
            .delete_meta(&format!("{}t-a", exchange::EXCHANGE_PREFIX))
            .unwrap()
    });
    moved("a fold's handle ended", &|| {
        c.store
            .delete_meta(&format!("{}fold-a", crate::ladder::fold::HANDLE_PREFIX))
            .unwrap()
    });
    moved("a session's record written", &|| {
        c.store
            .put_session(&desk, &json!({"not": "a session"}))
            .unwrap()
    });
    moved("a task's record written", &|| {
        c.store
            .append(&[crate::task_graph::record(&t).unwrap()])
            .map(drop)
            .unwrap()
    });
}

/// What proving one rung-1 fold costs: the proof's own count (its node and its N exchanges: N + 1 units) and,
/// beside it, the records the store actually read, counted by the store on this thread.
#[tokio::test]
async fn a_rung_one_folds_proof_counts_its_units_and_the_records_read() {
    let r = rig();
    let c = &r.core;
    let desk = session(c, None);
    let n = crate::ladder::N;
    let turns: Vec<String> = (0..n).map(|i| format!("t-{i:02}")).collect();
    exchanges_of(c, &turns, &desk, "local");
    let ids: Vec<&str> = turns.iter().map(String::as_str).collect();
    fold_node(c, "fold-one", "local", 1, &ids, 10);
    let p = fold_piece("local", 1, (ids[0], ids[n - 1]), "fold-one");
    let source = owners_source(c);
    let (read, rows) = (
        theseus_store::records_read_here(),
        theseus_store::index_rows_here(),
    );
    assert!(lists(&source, &p));
    let (read, rows) = (
        theseus_store::records_read_here() - read,
        theseus_store::index_rows_here() - rows,
    );
    eprintln!(
        "a rung-1 fold: {} proof units, {read} records read, {rows} index rows",
        source.reads()
    );
    assert_eq!(source.reads(), 1 + n as u64);
    assert!(read > source.reads(), "{read}");
}

/// The most a settled ladder lays of rungs 1 and 2 (2N - 1 nodes of each, of N and N² exchanges) and a task's
/// card: the model's look proves every one, lists what its bound of pieces lists and counts the rest as
/// pieces it may read, with nothing left unchecked.
#[tokio::test]
async fn the_look_proves_a_settled_ladder_whole() {
    let (r, t) = a_task().await;
    let c = &r.core;
    let desk = crate::tests_task_graph::session(c);
    let n = crate::ladder::N;
    let turns: Vec<String> = (0..n * n).map(|i| format!("t-{i:04}")).collect();
    exchanges_of(c, &turns, &desk, "local");
    let ids: Vec<&str> = turns.iter().map(String::as_str).collect();
    let mut pieces = Vec::new();
    for i in 0..2 * n - 1 {
        let one = format!("fold-1-{i:02}");
        fold_node(c, &one, "local", 1, &ids[..n], 10);
        pieces.push(fold_piece("local", 1, (ids[0], ids[n - 1]), &one));
        let two = format!("fold-2-{i:02}");
        fold_node(c, &two, "local", 2, &ids, 10);
        pieces.push(fold_piece("local", 2, (ids[0], ids[n * n - 1]), &two));
    }
    pieces.push(task_piece(&t.id));
    let all = pieces.len();
    keep(c, "t-look", "cmp_look", "local", pieces);
    let said = crate::look::view::words(c, &desk, "t-look");
    assert!(said.contains(&format!("total: {all} shown")), "{said}");
    assert!(
        said.contains(&format!("({} more pieces you may read", all - 60)),
        "{said}"
    );
    assert!(!said.contains("not checked"), "{said}");
}

/// More fold work than one look may spend: the look proves in the order it prints, lists what its budget
/// proved, leaves out each fold it could not check (fail closed), and says how many in one line.
#[tokio::test]
async fn the_looks_proof_stops_at_its_budget_and_says_so() {
    let r = rig();
    let c = &r.core;
    let desk = session(c, None);
    let n = crate::ladder::N;
    let turns: Vec<String> = (0..n * n * n).map(|i| format!("t-{i:05}")).collect();
    exchanges_of(c, &turns, &desk, "local");
    let ids: Vec<&str> = turns.iter().map(String::as_str).collect();
    let (first, last) = (ids[0], ids[ids.len() - 1]);
    let mut pieces = Vec::new();
    for (handle, level) in [
        ("fold-3-a", 3),
        ("fold-3-b", 3),
        ("fold-3-c", 3),
        ("fold-1-a", 1),
        ("fold-3-d", 3),
    ] {
        let stands = if level == 1 { &ids[..n] } else { &ids[..] };
        fold_node(c, handle, "local", level, stands, 10);
        let end = if level == 1 { ids[n - 1] } else { last };
        pieces.push(fold_piece("local", level, (first, end), handle));
    }
    keep(c, "t-many", "cmp_many", "local", pieces);
    let said = crate::look::view::words(c, &desk, "t-many");
    let listed: Vec<&str> = (said.lines())
        .filter_map(|l| l.trim_start().split(' ').next())
        .filter(|h| h.starts_with("fold-"))
        .collect();
    assert_eq!(listed, ["fold-3-a", "fold-3-b", "fold-1-a"], "{said}");
    assert!(said.contains("total: 3 shown"), "{said}");
    assert!(
        said.contains("(2 folds or task cards were not checked and are not listed:"),
        "{said}"
    );
}
