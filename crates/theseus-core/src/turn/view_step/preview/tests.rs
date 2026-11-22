//! `view.preview` through the whole core (issue 44): the preview lays what a real turn then records; a
//! place with un-ledgered history says it falls back; an agent's shell gets an agent's view of its own place
//! and no text; a sealed thread is in no piece and no count; and nothing is written.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_memory::gate::{PointOfView, Present};
use theseus_memory::recall::Place;
use theseus_protocol::view::{ViewPreviewResult, ViewShown};
use theseus_protocol::{error_code, method};

use crate::approval::Surface;
use crate::config::assembly::{Assembly, AssemblySetting};
use crate::peer_proc::Behind;
use crate::session::SessionRecord;
use crate::tests_m3::{rpc_as, surface};
use crate::tests_places::{rig_setup, rig_with, turn, Rig, LAB};
use crate::tests_private_view::session_in;
use crate::view::record::{self, show::Reader, source::GateSource};
use crate::Core;

fn view_rig() -> Rig {
    rig_with(|c| c.context.assembly = Some(AssemblySetting::All(Assembly::View)))
}

/// `view.preview` from the owner's terminal, or from `client`.
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

async fn preview(core: &Arc<Core>, params: Value) -> Result<ViewPreviewResult, String> {
    preview_as(core, surface("sock#1", Surface::Cli), params).await
}

fn place(core: &Core, sid: &str) -> String {
    crate::view_gate::place_name(&core.runner, sid).expect("the session's place")
}

type Laid = (String, String, String, String, String);

/// The pieces a view lays, by slot, kind, range and form: the frame and the message apart (their sizes are
/// the spec's and the message's, not the place's), and recall's (a preview runs none), and `but` (the
/// turn's own exchange).
fn laid(v: &ViewShown, but: &str) -> Vec<Laid> {
    let mut out: Vec<Laid> = (v.slots.iter())
        .filter(|s| s.slot != "frame" && s.slot != "now")
        .flat_map(|s| s.pieces.iter().map(move |x| (s, x)))
        .filter(|(_, x)| !x.reasons.iter().any(|r| r == "recalled") && x.first != but)
        .map(|(s, x)| {
            let at = (x.first.clone(), x.last.clone());
            (s.slot.clone(), x.kind.clone(), at.0, at.1, x.form.clone())
        })
        .collect();
    out.sort();
    out
}

/// A level-1 fold over the exchange of `turn_id`.
fn fold_over(c: &Core, turn_id: &str) {
    let ex = crate::exchange::exchange(&c.store, turn_id)
        .unwrap()
        .unwrap();
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

/// A place with a window and a fold previews the pieces, in the same slots and forms, that its next real
/// turn then records in its view record; the preview is the view's, with no fallback.
#[tokio::test]
async fn a_preview_lays_the_pieces_the_next_turn_records() {
    let r = view_rig();
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    turn(c, &sid, "first: the tide").await;
    turn(c, &sid, "second: the moon").await;
    let first = turns_of(c, &sid).remove(0);
    fold_over(c, &first);
    // The place as the ledger names it, which the window reads.
    let at = (crate::exchange::exchange(&c.store, &first).unwrap())
        .unwrap()
        .place;
    let p = preview(&r.core, json!({"place": at, "message": "third: and now"}))
        .await
        .unwrap();
    assert_eq!(
        (p.reader.as_str(), p.session.as_str()),
        ("owner", sid.as_str())
    );
    assert_eq!(p.fallback, None, "{p:?}");
    assert!(p.window >= 1, "{p:?}");
    let seen = p.view.clone().expect("a view");

    turn(c, &sid, "third: and now").await;
    let last = (c.store.get_session::<SessionRecord>(&sid).unwrap().unwrap())
        .last_turn_id
        .unwrap();
    let rec = record::newest(&c.store, &last)
        .unwrap()
        .expect("the turn kept its view");
    let source = GateSource {
        runner: &c.runner,
        pov: PointOfView {
            anchor: at.clone(),
            place: Place::Private,
            present: Present::OwnerAlone,
        },
        asker: None,
    };
    let kept = Reader {
        source: &source,
        text: None,
        restricted: false,
    }
    .show(&rec);
    assert!(!laid(&seen, &last).is_empty(), "{seen:?}");
    assert_eq!(laid(&seen, &last), laid(&kept, &last));
    let folds = |v: &ViewShown| (v.slots.iter().flat_map(|s| &s.pieces)).any(|x| x.kind == "folds");
    assert_eq!(folds(&seen), folds(&kept));
    assert_eq!(
        p.folds.values().sum::<u64>(),
        u64::from(folds(&seen)),
        "{p:?}"
    );
}

/// A session whose earlier turn the ledger holds no exchange for says the turn would be the transcript's,
/// and why; the preview writes no fallback row and no view record.
#[tokio::test]
async fn a_place_with_unledgered_history_says_unledgered_history() {
    let r = view_rig();
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    let earlier = crate::node::Node::user(&sid, Some("turn_old"), "test", "options: red or blue");
    c.store.append(&[earlier.record().unwrap()]).unwrap();
    let p = preview(&r.core, json!({"place": place(c, &sid)}))
        .await
        .unwrap();
    assert_eq!(p.fallback.as_deref(), Some("unledgered_history"), "{p:?}");
    assert_eq!((p.earlier.turns, p.earlier.no_exchange), (1, 1), "{p:?}");
    assert!(p.notes.iter().any(|n| n.contains("empty message")), "{p:?}");
    assert!(crate::test_support::rows(c, "context.view_fallback").is_empty());
    assert!(record::of_turn(&c.store, super::PREVIEW_TURN)
        .unwrap()
        .is_empty());
}

/// From a Theseus job's shell the preview is an agent's turn of the job's own place: not the owner alone,
/// restricted, no text; text, another place, and an agent's process with no job are refused in words.
#[tokio::test]
async fn an_agents_preview_is_its_own_places_with_no_text() {
    let r = view_rig();
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    turn(c, &sid, "first: the tide").await;
    let at = place(c, &sid);
    let p = preview(&r.core, json!({"place": at, "from_job": sid}))
        .await
        .unwrap();
    assert_eq!(p.reader, "agent");
    let v = p.view.expect("a view");
    assert!(v.restricted.is_some(), "{v:?}");
    assert_ne!(v.pov.present, "owner_alone", "{v:?}");
    assert!((v.slots.iter().flat_map(|s| &s.pieces)).all(|x| x.text.is_none()));
    let lab = crate::tests_places::session(c, Some(&format!("channel:{LAB}")));
    let agent = surface("sock#2", Surface::Cli).behind(Behind::Agent("claude".into()));
    for (who, client, params) in [
        (
            "text",
            surface("sock#1", Surface::Cli),
            json!({"place": at, "from_job": sid, "text": true}),
        ),
        (
            "another place",
            surface("sock#1", Surface::Cli),
            json!({"place": at, "from_job": lab}),
        ),
        ("no job", agent, json!({"place": at})),
        (
            "discord",
            crate::approval::Client::new("discord", Surface::Discord),
            json!({"place": at}),
        ),
    ] {
        let said = preview_as(&r.core, client, params).await.unwrap_err();
        assert!(said.starts_with("not shown:"), "{who}: {said}");
    }
}

/// A thread `kelp`, bound to its own session, sealed as `seal` says.
fn kelp(store: &crate::store::Store, seal: bool) -> String {
    let r = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    store.put_session(&r.session_id, &r).unwrap();
    let mut t = json!({"slug": "kelp", "title": "kelp"});
    if seal {
        t["sealed"] = json!({"by": "cli", "at_ms": 1, "reason": "embargo"});
    }
    store.put_meta("thread:kelp", &t).unwrap();
    store
        .put_meta("discord.session.thread:kelp", &r.session_id)
        .unwrap();
    r.session_id
}

/// A sealed thread's session in the same place contributes nothing to the owner's preview: no piece, no
/// id, and no count (its session and turns are not in the earlier turns' figures). Unsealed, it is counted.
#[tokio::test]
async fn a_sealed_thread_contributes_nothing_not_even_a_count() {
    let mut sessions = Vec::new();
    for seal in [false, true] {
        let thread = std::cell::RefCell::new(String::new());
        let r = rig_setup(
            |c| {
                c.context.assembly = Some(AssemblySetting::All(Assembly::View));
                c.threads.enabled = true;
            },
            |s| *thread.borrow_mut() = kelp(s, seal),
        );
        let c = &r.core;
        let thread = thread.into_inner();
        turn(c, &thread, "kelp: the embargoed figure is 7741").await;
        let desk = crate::tests_places::session(c, None);
        turn(c, &desk, "desk: and now?").await;
        let p = preview(&r.core, json!({"place": place(c, &desk)}))
            .await
            .unwrap();
        assert_eq!(p.session, desk);
        let said = serde_json::to_string(&p).unwrap();
        let kelp_turn = turns_of(c, &thread).remove(0);
        assert_eq!(said.contains(&kelp_turn), !seal, "sealed {seal}: {said}");
        assert!(!said.contains(&thread), "{said}");
        sessions.push((p.earlier.sessions, p.earlier.turns));
    }
    assert_eq!(sessions[0], (2, 2), "unsealed, both are counted");
    assert_eq!(sessions[1], (1, 1), "sealed, the thread is in no count");
}
