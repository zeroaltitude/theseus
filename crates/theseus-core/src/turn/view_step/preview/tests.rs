//! `view.preview` through the whole core (issue 44): the preview lays what a real turn then records (folds
//! and his other places included); a place with un-ledgered history says it falls back; only the owner in
//! person is answered; a sealed thread, a fork's inherited prefix of one, a move into one and another
//! place's exchanges are in no piece and no count; and nothing a later turn reads is written.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_memory::gate::{PointOfView, Present};
use theseus_memory::recall::Place;
use theseus_protocol::view::{ViewPreviewResult, ViewShown};
use theseus_protocol::{error_code, method};

use crate::approval::Surface;
use crate::config::assembly::{Assembly, AssemblySetting};
use crate::exchange::{self, Exchange, Opener};
use crate::peer_proc::{Behind, JOB};
use crate::session::SessionRecord;
use crate::tests_m3::{rpc_as, surface};
use crate::tests_places::{rig_setup, rig_with, turn, Rig, OWNER};
use crate::tests_private_view::session_in;
use crate::view::record::{self, show::Reader, source::GateSource};
use crate::view::Space;
use crate::Core;

fn view_rig() -> Rig {
    rig_with(|c| c.context.assembly = Some(AssemblySetting::All(Assembly::View)))
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
    let source = GateSource {
        runner: &c.runner,
        pov: PointOfView {
            anchor: at.clone(),
            place: Place::Private,
            present: Present::OwnerAlone,
        },
        asker: None,
    };
    let mut kept = Reader {
        source: &source,
        text: None,
        restricted: false,
    }
    .show(&rec);
    // The real turn's folds, as its assembly laid them (the listing's own reader proves none).
    let folds: Vec<_> = (rec.shown.iter())
        .filter(|s| matches!(s.piece.range.space, Space::Folds { .. }))
        .collect();
    assert_eq!(folds.len(), 1, "the real turn laid the fold");
    for s in &folds {
        let slot = theseus_protocol::view::ViewSlot {
            slot: record::show::name(&s.piece.slot),
            ..Default::default()
        };
        let piece = theseus_protocol::view::ViewPiece {
            kind: "folds".into(),
            first: s.piece.range.first.id.clone(),
            last: s.piece.range.last.id.clone(),
            form: record::show::name(&s.form),
            tokens: s.tokens,
            ..Default::default()
        };
        match kept.slots.iter_mut().find(|k| k.slot == slot.slot) {
            Some(k) => k.pieces.push(piece),
            None => kept.slots.push(theseus_protocol::view::ViewSlot {
                pieces: vec![piece],
                ..slot
            }),
        }
    }
    assert!(!laid(&seen, &last).is_empty(), "{seen:?}");
    assert_eq!(laid(&seen, &last), laid(&kept, &last));
    assert_eq!(p.folds.get("1"), Some(&1), "{p:?}");
    assert!(
        (seen.slots.iter().flat_map(|s| &s.pieces))
            .any(|x| x.kind == "folds" && x.tokens == folds[0].tokens && x.tokens > 0),
        "{seen:?}"
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

/// A place set to `transcript` says so, and what a switch to `view` would do: never "takes the view".
#[tokio::test]
async fn a_transcript_place_is_not_said_to_take_the_view() {
    let r = rig_with(|_| {});
    let c = &r.core;
    let sid = session_in(c, None, &r.root);
    turn(c, &sid, "first: the tide").await;
    let p = preview(&r.core, json!({"place": place(c, &sid)}))
        .await
        .unwrap();
    assert_eq!(p.assembly, "transcript");
    let said = theseus_protocol::view::preview_lines(&p);
    assert!(said.contains("set to transcript"), "{said}");
    assert!(!said.contains("it would take the view"), "{said}");
}

/// Only the owner in person is answered. The daemon cannot tell which session an agent's process runs, so
/// a session the caller names is no scope: an agent's process naming another place's session, a Theseus
/// job's process, a shell with a job's marker, Discord, and text from the web UI are each refused in words,
/// and none is told any id.
#[tokio::test]
async fn only_the_owner_in_person_is_answered() {
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
        let said = preview_as(&r.core, client, params).await.unwrap_err();
        assert!(said.starts_with("not shown:"), "{who}: {said}");
        assert!(
            !said.contains(&sid) && !said.contains(&other),
            "{who}: {said}"
        );
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

fn thread_rig(seal: bool) -> (Rig, String) {
    let thread = std::cell::RefCell::new(String::new());
    let r = rig_setup(
        |c| {
            c.context.assembly = Some(AssemblySetting::All(Assembly::View));
            c.threads.enabled = true;
        },
        |s| *thread.borrow_mut() = kelp(s, seal),
    );
    (r, thread.into_inner())
}

/// A sealed thread's session in the same place contributes nothing to the owner's preview: no piece, no
/// id, and no count (its session and turns are not in the earlier turns' figures). Unsealed, it is counted.
#[tokio::test]
async fn a_sealed_thread_contributes_nothing_not_even_a_count() {
    let mut sessions = Vec::new();
    for seal in [false, true] {
        let (r, thread) = thread_rig(seal);
        let c = &r.core;
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

/// An unsealed fork of a sealed thread's session inherits its prefix: the preview of the fork's place counts
/// the fork's own turn alone, and names nothing of the prefix.
#[tokio::test]
async fn a_forks_inherited_prefix_of_a_sealed_session_is_in_no_count() {
    let (r, thread) = thread_rig(true);
    let c = &r.core;
    turn(c, &thread, "kelp: the embargoed figure is 7741").await;
    let nodes = c.store.session_nodes(&thread).unwrap();
    let mut fork = SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    fork.forked_from = Some(Box::new(theseus_protocol::ForkOrigin {
        session: thread.clone(),
        node: nodes.last().unwrap().1.id.clone(),
        position: nodes.len() as u64,
        compilation: None,
        at_ms: 1,
    }));
    c.store.put_session(&fork.session_id, &fork).unwrap();
    let inherited = c.store.transcript(&fork.session_id).unwrap().len();
    assert!(inherited > 0, "the fork inherits the thread's prefix");
    turn(c, &fork.session_id, "fork: my own words").await;
    let p = preview(&r.core, json!({"place": place(c, &fork.session_id)}))
        .await
        .unwrap();
    assert_eq!(p.session, fork.session_id);
    assert_eq!((p.earlier.sessions, p.earlier.turns), (1, 1), "{p:?}");
    let said = serde_json::to_string(&p).unwrap();
    let kelp_turn = turns_of(c, &thread).remove(0);
    assert!(
        !said.contains(&kelp_turn) && !said.contains(&thread),
        "{said}"
    );
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

/// Move `node` into the thread `kelp`, as a split records it.
fn move_into_kelp(c: &Core, node: &str) {
    let to = crate::moved::MovedTo {
        thread: "kelp".into(),
        split: "split_test".into(),
    };
    c.store
        .put_meta(&crate::moved::moved_of_key(node), &Some(to))
        .unwrap();
}

/// The desk's message moved into a sealed thread: counted nowhere (the preview's own counts ask the
/// node's seal).
#[tokio::test]
async fn a_message_moved_into_a_sealed_thread_is_in_no_count() {
    let (r, _thread) = thread_rig(true);
    let c = &r.core;
    let desk = crate::tests_places::session(c, None);
    turn(c, &desk, "desk: the embargoed figure is 4471").await;
    let asked = (c.store.session_nodes(&desk).unwrap().into_iter())
        .find(|(_, n)| n.kind == crate::stub::Kind::UserMessage)
        .unwrap()
        .1;
    move_into_kelp(c, &asked.id);
    let p = preview(&r.core, json!({"place": place(c, &desk)}))
        .await
        .unwrap();
    assert_eq!((p.earlier.sessions, p.earlier.turns), (1, 0), "{p:?}");
}

/// The same move keeps the moved exchange out of the listing too. The listing is the shared readers'
/// (`GateSource`, the gated ladder, the window), which ask the node's seal once `p14/reader-seals` is
/// merged; until then they read the session's seal alone, and this fails.
#[tokio::test]
#[ignore = "passes once p14/reader-seals (the shared readers ask node_seal) is merged"]
async fn a_message_moved_into_a_sealed_thread_is_not_listed() {
    let (r, _thread) = thread_rig(true);
    let c = &r.core;
    let desk = crate::tests_places::session(c, None);
    turn(c, &desk, "desk: the embargoed figure is 4471").await;
    let moved_turn = turns_of(c, &desk).remove(0);
    let asked = (c.store.session_nodes(&desk).unwrap().into_iter())
        .find(|(_, n)| n.kind == crate::stub::Kind::UserMessage)
        .unwrap()
        .1;
    move_into_kelp(c, &asked.id);
    turn(c, &desk, "desk: and now?").await;
    let p = preview(&r.core, json!({"place": place(c, &desk), "text": true}))
        .await
        .unwrap();
    let said = serde_json::to_string(&p).unwrap();
    assert!(
        !said.contains(&moved_turn) && !said.contains("4471"),
        "{said}"
    );
}

/// A preview resolves no thread card: a legacy compilation with no card kept gets none installed in the
/// shared cache (which would pin what a later turn sends), where the census's own read installs one.
#[tokio::test]
async fn a_preview_installs_no_card_a_later_turn_reads() {
    let (r, thread) = thread_rig(false);
    let c = &r.core;
    turn(c, &thread, "kelp: the plan").await;
    let legacy = "cmp_legacy_preview";
    c.store
        .update_session(&thread, |s| {
            s.compilation_id = Some(legacy.into());
            Ok(vec![])
        })
        .unwrap();
    let at = exchange::exchange(&c.store, &turns_of(c, &thread).remove(0))
        .unwrap()
        .unwrap()
        .place;
    let p = preview(&r.core, json!({"place": at})).await.unwrap();
    assert_eq!(p.session, thread);
    assert_eq!(
        c.runner.cards.held(legacy),
        None,
        "the preview installed a card"
    );
    // The census's read does install one: the test would see the old path.
    let rec = c
        .store
        .get_session::<SessionRecord>(&thread)
        .unwrap()
        .unwrap();
    let (live, _) = c.live_profile();
    c.runner.census_request(Some(&rec), "cli", &live).unwrap();
    assert!(c.runner.cards.held(legacy).is_some());
}
