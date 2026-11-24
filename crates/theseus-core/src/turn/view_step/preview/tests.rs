//! `view.preview` through the whole core (issue 44): the preview lays what a real turn then records (folds
//! and his other places included); a place with un-ledgered history says it falls back; only the owner in
//! person is answered; another place's exchanges are in no piece and no count; and nothing a later turn
//! reads is written.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_memory::gate::{PointOfView, Present};
use theseus_memory::recall::Place;
use theseus_protocol::view::{ViewPreviewResult, ViewShown};
use theseus_protocol::{error_code, method};

use crate::approval::Surface;
use crate::exchange::{self, Exchange, Opener};
use crate::peer_proc::{Behind, JOB};
use crate::session::SessionRecord;
use crate::tests_m3::{rpc_as, surface};
use crate::tests_places::{rig_with, turn, Rig, OWNER};
use crate::tests_private_view::session_in;
use crate::view::record::{self, show::Reader, source::GateSource};
use crate::view::Space;
use crate::Core;

fn view_rig() -> Rig {
    rig_with(|_| {})
}

/// `view.preview` from `client`.
async fn preview_as(
    core: &Arc<Core>,
    client: crate::approval::Client,
    params: Value,
) -> Result<ViewPreviewResult, String> {
    match rpc_as(core, client, method::VIEW_PREVIEW, params).await {
        Ok(v) => Ok(serde_json::from_value(v).unwrap()),
        Err(e) => {
            assert_eq!(e.code, error_code::REFUSED, "{e:?}");
            Err(e.message)
        }
    }
}

/// `view.preview` from the owner's own terminal.
async fn preview(core: &Arc<Core>, params: Value) -> Result<ViewPreviewResult, String> {
    preview_as(core, surface("sock#1", Surface::Cli), params).await
}

fn place(core: &Core, sid: &str) -> String {
    crate::view_gate::place_name(&core.runner, sid).expect("the session's place")
}

type Laid = (String, String, String, String, String, u64);

/// The pieces a view lays, by slot, kind, range, form and tokens: the frame and the message apart (their
/// sizes are the spec's and the message's, not the place's), recall's (a preview runs none), and `but`
/// (the turn's own exchange).
fn laid(v: &ViewShown, but: &str) -> Vec<Laid> {
    let mut out: Vec<Laid> = (v.slots.iter())
        .filter(|s| s.slot != "frame" && s.slot != "now")
        .flat_map(|s| s.pieces.iter().map(move |x| (s, x)))
        .filter(|(_, x)| !x.reasons.iter().any(|r| r == "recalled") && x.first != but)
        .map(|(s, x)| {
            let at = (x.first.clone(), x.last.clone());
            (
                s.slot.clone(),
                x.kind.clone(),
                at.0,
                at.1,
                x.form.clone(),
                x.tokens,
            )
        })
        .collect();
    out.sort();
    out
}

/// A level-1 fold over the exchange of `turn_id`.
fn fold_over(c: &Core, turn_id: &str) {
    let ex = exchange::exchange(&c.store, turn_id).unwrap().unwrap();
    let node = crate::ladder::fold::FoldNode {
        id: "fold-tide-1".into(),
        place: ex.place.clone(),
        level: 1,
        exchanges: vec![turn_id.into()],
        first_ms: ex.start_ms,
        last_ms: ex.start_ms,
        his: vec![],
        counted: 0,
        children: vec![],
        title: "the tide".into(),
        account: "ACCOUNT: the tide was asked about".into(),
        marks: crate::view::Marks {
            place: Some(ex.place.clone()),
            audience: crate::view::Audience::Private,
            author: crate::view::Author::Theseus,
            outside: false,
            sealed: false,
            veiled: false,
        },
        model: None,
        writer: 1,
        at_ms: ex.start_ms,
    };
    c.store
        .append(&crate::ladder::fold::records(&node).unwrap())
        .unwrap();
}

fn turns_of(c: &Core, sid: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (_, n) in c.store.session_nodes(sid).unwrap() {
        if let Some(t) = n.turn_id.clone().filter(|t| !out.contains(t)) {
            out.push(t);
        }
    }
    out
}

/// A place with a window and a fold, after activity in his DM: the preview lays the pieces, in the same
/// slots, forms and tokens, that its next real turn of the owner in person then records (the elsewhere
/// slot included); the fold is listed with its level and the real assembly's tokens.
#[tokio::test]
async fn a_preview_lays_the_pieces_the_next_turn_records() {
    let r = view_rig();
    let c = &r.core;
    let dm = crate::tests_places::session(c, Some(&format!("dm:{OWNER}")));
    turn(c, &dm, "dm: the plum orchard is due a visit").await;
    let sid = session_in(c, None, &r.root);
    turn(c, &sid, "first: the tide").await;
    turn(c, &sid, "second: the moon").await;
    let first = turns_of(c, &sid).remove(0);
    fold_over(c, &first);
    // The place as the ledger names it, which the window reads.
    let at = exchange::exchange(&c.store, &first).unwrap().unwrap().place;
    let p = preview(&r.core, json!({"place": at, "message": "third: and now"}))
        .await
        .unwrap();
    assert_eq!(
        (p.reader.as_str(), p.session.as_str()),
        ("owner", sid.as_str())
    );
    assert_eq!((p.fallback.as_deref(), p.assembly.as_str()), (None, "view"));
    assert!(p.window >= 1, "{p:?}");
    let seen = p.view.clone().expect("a view");
    // The tally in plain lines, as `theseus view preview` prints it (`--no-capture` shows it).
    eprintln!("{}", theseus_protocol::view::preview_lines(&p));

    turn(c, &sid, "third: and now").await;
    let last = (c.store.get_session::<SessionRecord>(&sid).unwrap().unwrap())
        .last_turn_id
        .unwrap();
    let rec = record::newest(&c.store, &last)
        .unwrap()
        .expect("the turn kept its view");
    let pov = PointOfView {
        anchor: at.clone(),
        place: Place::Private,
        present: Present::OwnerAlone,
    };
    let source = GateSource::new(&c.runner, pov, None);
    let kept = Reader {
        source: &source,
        text: None,
        restricted: false,
    }
    .show(&rec);
    // The real turn's folds, as its assembly laid them: the listing's own reader proves them.
    let folds: Vec<_> = (rec.shown.iter())
        .filter(|s| matches!(s.piece.range.space, Space::Folds { .. }))
        .collect();
    assert_eq!(folds.len(), 1, "the real turn laid the fold");
    assert!(!laid(&seen, &last).is_empty(), "{seen:?}");
    assert_eq!(laid(&seen, &last), laid(&kept, &last));
    assert_eq!(p.folds.get("1"), Some(&1), "{p:?}");
    assert!(
        (seen.slots.iter().flat_map(|s| &s.pieces))
            .any(|x| x.kind == "folds" && x.tokens == folds[0].tokens && x.tokens > 0),
        "{seen:?}"
    );
}

/// A session whose earlier turn the ledger holds no exchange for is the view's all the same (that turn is
/// out of its reach, issue 44): the preview counts it, says no fallback, and writes no fallback row and no
/// view record.
#[tokio::test]
async fn a_place_with_unledgered_history_is_the_views_and_counts_it() {
    let r = view_rig();
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    let earlier = crate::node::Node::user(&sid, Some("turn_old"), "test", "options: red or blue");
    c.store.append(&[earlier.record().unwrap()]).unwrap();
    let p = preview(&r.core, json!({"place": place(c, &sid)}))
        .await
        .unwrap();
    assert_eq!(p.fallback, None, "{p:?}");
    assert_eq!((p.earlier.turns, p.earlier.no_exchange), (1, 1), "{p:?}");
    assert!(p.notes.iter().any(|n| n.contains("empty message")), "{p:?}");
    let said = theseus_protocol::view::preview_lines(&p);
    assert!(
        said.contains("hypothetical") && said.contains("not predicted"),
        "{said}"
    );
    assert!(crate::test_support::rows(c, "context.view_fallback").is_empty());
    assert!(record::of_turn(&c.store, super::PREVIEW_TURN)
        .unwrap()
        .is_empty());
}

/// A preview is a read (anything goes, 2026-11-22): an agent's process naming another place's session, a
/// Theseus job's process, a shell with a job's marker, Discord, and text from the web UI are each answered.
#[tokio::test]
async fn every_reader_is_answered() {
    let r = view_rig();
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    turn(c, &sid, "first: the tide").await;
    let other = crate::tests_places::session(c, Some(&format!("dm:{OWNER}")));
    turn(c, &other, "dm: elsewhere").await;
    let at = place(c, &sid);
    let agent = surface("sock#2", Surface::Cli).behind(Behind::Agent("claude".into()));
    let job = surface("sock#3", Surface::Cli).behind(Behind::Agent(JOB.into()));
    for (who, client, params) in [
        (
            "agent forging a session",
            agent.clone(),
            json!({"place": at, "from_job": other}),
        ),
        ("agent", agent, json!({"place": at})),
        ("job", job.clone(), json!({"place": at})),
        ("job, its own", job, json!({"place": at, "from_job": sid})),
        (
            "job's marker",
            surface("sock#4", Surface::Cli),
            json!({"place": at, "from_job": sid}),
        ),
        (
            "discord",
            crate::approval::Client::new("discord", Surface::Discord),
            json!({"place": at}),
        ),
        (
            "web text",
            surface("web#1", Surface::Web),
            json!({"place": at, "text": true}),
        ),
    ] {
        preview_as(&r.core, client, params)
            .await
            .unwrap_or_else(|e| panic!("{who}: {e}"));
    }
    let mine = preview(&r.core, json!({"place": at})).await.unwrap();
    assert_eq!(
        (mine.reader.as_str(), mine.session.as_str()),
        ("owner", sid.as_str())
    );
    let v = mine.view.expect("the owner's view");
    assert_eq!(v.pov.present, "owner_alone");
    assert!(v.restricted.is_none(), "{v:?}");
}

/// An exchange of `session` at `place`, as the ledger writes it.
fn exchange_of(core: &Core, turn: &str, session: &str, place: &str) {
    let e = Exchange {
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
    };
    core.store
        .append(&[exchange::exchange_record(&e).unwrap()])
        .unwrap();
}

/// A session with exchanges at two places: the preview of one counts its turns there alone, and names
/// nothing of the other's.
#[tokio::test]
async fn another_places_exchange_is_in_no_count() {
    let r = view_rig();
    let c = &r.core;
    let sid = crate::tests_places::session(c, None);
    let here = place(c, &sid);
    for (t, at) in [
        ("turn_here", here.as_str()),
        ("turn_there", "discord:channel:9"),
    ] {
        let n = crate::node::Node::user(&sid, Some(t), "test", t);
        c.store.append(&[n.record().unwrap()]).unwrap();
        exchange_of(c, t, &sid, at);
    }
    let p = preview(&r.core, json!({"place": here})).await.unwrap();
    assert_eq!((p.earlier.turns, p.earlier.no_exchange), (1, 0), "{p:?}");
    let said = serde_json::to_string(&p).unwrap();
    assert!(!said.contains("turn_there"), "{said}");
}
