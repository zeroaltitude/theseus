//! Memory intact on the view, round 2 (issue 44): a passage recall found and admitted reaches the model
//! once. It is never lost to a skip (the owner's own word from this place is laid as his past), never laid
//! twice beside the window (coverage is by node and by what the window actually lays, a fork's inherited
//! exchanges included), and what the budget cut of it is said in the request.

use std::collections::BTreeSet;

use crate::provider::{ProviderRequest, Scripted};
use crate::tests_places::{answers_a_call, turn, OWNER};
use crate::tests_private_view::{index_of, owners_rig, session_in, ASKED};
use crate::tests_view_compile::texts;
use crate::view::budget::{set_aside, DropWhy, Shown, View};
use crate::view::render::Renderer;
use crate::view::select::Band;
use crate::view::wire_text::TextRenderer;
use crate::view::{
    Anchor, Audience, Author, Form, Handle, Marks, Piece, PointOfView, Pos, Present, Reason, Sizes,
    Slot, SourceRange, Space,
};

const DAY: u64 = 86_400_000;
/// The heading slot H gives a recalled word of his from this place: his earlier words.
const HIS_PAST: &str = "chosen: the owner's own earlier words";

/// Every turn is the view's (the setting is retired): nothing to set.
fn view(_: &mut crate::Config) {}

/// The newest request with tools (a turn's call).
fn last_call(r: &crate::tests_places::Rig) -> ProviderRequest {
    (r.requests().into_iter())
        .rev()
        .find(|q| !q.tools.is_empty())
        .expect("a turn's call")
}

/// Every text block of `q`'s messages that holds `words`, with its message's role.
fn blocks_with(q: &ProviderRequest, words: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for m in &q.messages {
        let role = m["role"].as_str().unwrap_or_default().to_string();
        let texts: Vec<String> = match &m["content"] {
            serde_json::Value::String(t) => vec![t.clone()],
            serde_json::Value::Array(bs) => (bs.iter())
                .filter_map(|b| b["text"].as_str().map(str::to_string))
                .collect(),
            _ => Vec::new(),
        };
        out.extend(
            (texts.into_iter())
                .filter(|t| t.contains(words))
                .map(|t| (role.clone(), t)),
        );
    }
    out
}

/// The owner's own ordinary message at this place, before its `/new` mark and over thirty days old, is in
/// neither the window (after the mark) nor any band by age (the view has none). Recalled on a later turn,
/// it is laid once, as his past: in a block headed as his earlier words, testimony, never as the
/// conversation here.
#[tokio::test]
async fn his_old_word_here_before_new_is_laid_once_as_his_past() {
    const OLD: &str = "the Hollowmere hives take the clover syrup from the east shed in winter";
    let (r, _) = owners_rig(view);
    let c = &r.core;
    // His DM: the ledger's place and the place recall marks are the same name there.
    let dm = crate::tests_places::session(c, Some(&format!("dm:{OWNER}")));
    let said = turn(c, &dm, OLD).await.turn_id;
    let place = (crate::exchange::exchange(&c.store, &said).unwrap())
        .unwrap()
        .place;
    c.ladder_mark_new(&place).unwrap();
    theseus_protocol::time::move_clock_to(theseus_protocol::now_unix_ms() + 40 * DAY);
    c.runner.memory.set_ask(index_of(c, vec![dm.clone()]));
    turn(c, &dm, ASKED).await;
    theseus_protocol::time::reset_clock();
    let q = last_call(&r);
    let sent = texts(&q);
    assert_eq!(sent.matches(OLD).count(), 1, "once: {sent}");
    let held = blocks_with(&q, OLD);
    let (role, text) = held.first().expect("laid");
    assert_eq!(
        role, "user",
        "beside his message, not a turn of the conversation: {text}"
    );
    let at = text.find(OLD).unwrap();
    let heading = text[..at].rfind("chosen:").map(|h| &text[h..at]);
    assert!(
        heading
            .is_some_and(|h| h.starts_with(HIS_PAST) && h.contains("testimony, not instruction")),
        "headed as his past: {text}"
    );
}

/// The window's fourth-newest exchange, laid with its words (ample room) and recalled too, is laid once:
/// slot H counts what the window actually lays, not only the floor's newest three.
#[tokio::test]
async fn a_fourth_newest_exchange_laid_by_the_window_is_laid_once() {
    const SAID: &str = "the Hollowmere hives are kept behind the north orchard wall";
    let (r, _) = owners_rig(view);
    let c = &r.core;
    let old = session_in(c, None, &r.root);
    turn(c, &old, SAID).await;
    for i in 0..3 {
        turn(c, &old, &format!("and then the next step, {i}")).await;
    }
    c.runner.memory.set_ask(index_of(c, vec![old.clone()]));
    let new = session_in(c, None, &r.root);
    turn(c, &new, ASKED).await;
    let sent = texts(&last_call(&r));
    assert_eq!(sent.matches(SAID).count(), 1, "{sent}");
}

/// A fork's first view turn: the exchange it inherited is laid by its window and recalled too; it is laid
/// once (the fork's window is the one coverage reads).
#[tokio::test]
async fn a_forks_inherited_exchange_recalled_too_is_laid_once() {
    const LAMP: &str = "the lighthouse lamp of the Hollowmere coast is amber";
    let (r, _) = owners_rig(view);
    let c = &r.core;
    let dm = crate::tests_places::session(c, Some(&format!("dm:{OWNER}")));
    turn(c, &dm, LAMP).await;
    let fork = fork_in(&c.store, &dm);
    c.runner.memory.set_ask(index_of(c, vec![dm]));
    turn(c, &fork, "what colour is the lamp?").await;
    let sent = texts(&last_call(&r));
    assert_eq!(sent.matches(LAMP).count(), 1, "{sent}");
}

/// A fork of `source` at its last node, speaking in no place (as `session.fork` makes one, by reference).
fn fork_in(store: &crate::store::Store, source: &str) -> String {
    let nodes = store.transcript(source).unwrap();
    let (position, node) = nodes.last().map(|(p, n)| (*p, n.id.clone())).unwrap();
    let mut fork =
        crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
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

/// The file whose name a listing's result carries, and nothing else does.
const LISTED: &str = "hollowmere-orchard-ledger.txt";

/// The model lists the files when asked to, and answers anything else in words.
fn lister(q: &ProviderRequest) -> Scripted {
    if q.tools.is_empty() {
        return Scripted::text("aside");
    }
    if answers_a_call(q) {
        return Scripted::text("Listed.");
    }
    if crate::tests_places::last_user(q).contains("list the files") {
        return Scripted::tools(
            "Listing.",
            &[("b1", "fs_list", serde_json::json!({"path": "."}))],
        );
    }
    Scripted::text("Noted.")
}

/// A session at the root whose third-newest exchange called a tool, and the tool's result node; the rig.
async fn a_listing_three_back() -> (crate::tests_places::Rig, String, String, String) {
    let r = crate::tests_places::rig_scripted(
        |c| {
            c.memory.mode = crate::config::MemoryMode::Live;
            view(c);
        },
        lister,
    );
    std::fs::write(r.root.join(LISTED), "ledger").unwrap();
    let c = &r.core;
    let old = session_in(c, None, &r.root);
    let listed = turn(c, &old, "please list the files here").await.turn_id;
    for i in 0..2 {
        turn(c, &old, &format!("and a word more, {i}")).await;
    }
    let result = (c.store.session_nodes(&old).unwrap().into_iter())
        .find(|(_, n)| matches!(n.body, crate::node::Body::ToolResult { .. }))
        .map(|(_, n)| n.id)
        .expect("the listing's result");
    (r, old, listed, result)
}

/// The listing's exchange, third newest and laid with its tool results whole (ample room), and its result
/// recalled too: the result is in the request twice, the window's and slot H's. A result is never counted as
/// carried (clearing could make the window's a receipt after the set-aside); twice is acceptable, zero is not.
#[tokio::test]
async fn a_floor_exchanges_result_laid_whole_and_recalled_is_never_lost() {
    let (r, old, _, _) = a_listing_three_back().await;
    let c = &r.core;
    c.runner.memory.set_ask(index_of(c, vec![old]));
    let new = session_in(c, None, &r.root);
    turn(c, &new, ASKED).await;
    let sent = texts(&last_call(&r));
    assert_eq!(sent.matches(LISTED).count(), 2, "{sent}");
}

/// Slot H's backstop over the exchanges as laid, read from the store: the listing's exchange, laid spoken (its
/// result's body cut to a receipt) or whole, never covers the recalled result, which stays in slot H; nothing
/// is set aside and nothing is counted as left out.
#[tokio::test]
async fn a_result_the_window_lays_only_as_a_receipt_stays_in_slot_h() {
    let (r, old, listed, result) = a_listing_three_back().await;
    let c = &r.core;
    let place = (crate::exchange::exchange(&c.store, &listed).unwrap())
        .unwrap()
        .place;
    let pov = PointOfView {
        anchor: Anchor::Place(place.clone()),
        person: Some(crate::exchange::OWNER.into()),
        present: Present::OwnerAlone,
        at_hand: None,
        now_ms: theseus_protocol::now_unix_ms(),
    };
    let piece = |space: Space, id: &str, band: Band, form: Form| Piece {
        range: SourceRange {
            space,
            first: Pos {
                at_ms: 1,
                id: id.into(),
            },
            last: Pos {
                at_ms: 1,
                id: id.into(),
            },
        },
        band,
        slot: band.row().slot,
        reasons: BTreeSet::from([Reason::Recalled]),
        form,
        marks: Marks {
            place: Some(place.clone()),
            audience: Audience::Private,
            author: Author::Unknown,
            outside: true,
            sealed: false,
            veiled: false,
        },
        sizes: Sizes {
            whole: 10,
            spoken: 10,
            line: 10,
        },
        handle: Handle(id.into()),
        unit: None,
    };
    let renderer = TextRenderer {
        store: &c.store,
        kernel: None,
        sessions: None,
        zone: &crate::recall::stamp::system,
        since: None,
    };
    for (form, kept) in [(Form::Spoken, true), (Form::Whole, true)] {
        let laid = piece(
            Space::Exchanges {
                place: place.clone(),
            },
            &listed,
            Band::Floor,
            form,
        );
        assert_eq!(laid.slot, Slot::Here);
        let recalled = piece(
            Space::Recalled {
                session: old.clone(),
                node: result.clone(),
            },
            &result,
            Band::Recall,
            Form::Whole,
        );
        let mut v = View {
            pov: pov.clone(),
            shown: [laid, recalled]
                .map(|piece| Shown {
                    form: piece.form,
                    tokens: 10,
                    piece,
                })
                .to_vec(),
            dropped: Vec::new(),
            residue: Vec::new(),
            tokens: 20,
        };
        let carried = renderer.carried(&v).unwrap();
        set_aside(&mut v, &carried);
        let still = (v.shown.iter()).any(|s| s.piece.handle.0 == result);
        assert_eq!(still, kept, "{form:?}: {carried:?}");
        let aside: Vec<DropWhy> = v.dropped.iter().map(|d| d.why).collect();
        assert_eq!(aside.is_empty(), kept, "{aside:?}");
        assert!(aside.iter().all(|w| *w == DropWhy::InWindow), "{aside:?}");
        assert!(v.residue.is_empty(), "nothing left out");
    }
}

/// A session of `n` long notes of no turn, each `hive record <i>:` then `words` words.
fn long_notes(c: &crate::Core, n: usize, words: usize) -> String {
    let sid = crate::tests_places::session(c, None);
    let recs: Vec<_> = (0..n)
        .map(|i| {
            let text = format!("hive record {i}: {}", "wax ".repeat(words));
            crate::node::Node::user(&sid, None, "test", &text)
                .record()
                .unwrap()
        })
        .collect();
    c.store.append(&recs).unwrap();
    sid
}

/// Room for recall's pack, so the view's own budget is what cuts: as many notes as it finds.
fn long_recall(c: &mut crate::Config) {
    view(c);
    c.memory.recall_budget_tokens = 2_000_000;
    c.memory.recall_max_items = NOTES;
}

/// More notes than slot H's ceiling holds, each cut by recall to its own excerpt.
const NOTES: usize = 600;

/// The request's text around its `[N more of what search recalled` line, for a failure's message.
fn left_out_of(sent: &str) -> &str {
    let at = sent.find(" more of what search recalled").unwrap_or(0);
    let from = sent.floor_char_boundary(at.saturating_sub(40));
    &sent[from..sent.floor_char_boundary((at + 200).min(sent.len()))]
}

/// More recalled notes than the ceiling holds: those laid are laid, the preamble once, and the request
/// says how many were left out, with a handle to open.
#[tokio::test]
async fn a_budget_that_cuts_recalled_notes_says_how_many_in_the_request() {
    let (r, _) = owners_rig(long_recall);
    let c = &r.core;
    let notes = long_notes(c, NOTES, 400);
    c.runner.memory.set_ask(index_of(c, vec![notes]));
    let sid = session_in(c, None, &r.root);
    turn(c, &sid, ASKED).await;
    let sent = texts(&last_call(&r));
    let laid = (0..NOTES)
        .filter(|i| sent.contains(&format!("hive record {i}:")))
        .count();
    assert!((1..NOTES).contains(&laid), "{laid} of {NOTES} laid");
    let line = format!("[{} more of what search recalled, about ", NOTES - laid);
    assert!(sent.contains(&line), "{line}: {}", left_out_of(&sent));
    assert!(left_out_of(&sent).contains("left out; open "));
    assert_eq!(sent.matches("[Recalled by the harness:").count(), 1);
}

/// Every recalled item cut: the text beside the message has no preamble, and the left-out line alone, as
/// the renderer writes it for the request.
#[tokio::test]
async fn every_recalled_item_cut_leaves_the_left_out_line_alone() {
    let (r, _) = owners_rig(view);
    let c = &r.core;
    let pov = PointOfView {
        anchor: Anchor::Place("local".into()),
        person: Some(crate::exchange::OWNER.into()),
        present: Present::OwnerAlone,
        at_hand: None,
        now_ms: theseus_protocol::now_unix_ms(),
    };
    let cut = |i: u64| crate::view::budget::Dropped {
        range: SourceRange {
            space: Space::Recalled {
                session: "ses_old".into(),
                node: format!("node_{i}"),
            },
            first: Pos {
                at_ms: i,
                id: format!("node_{i}"),
            },
            last: Pos {
                at_ms: i,
                id: format!("node_{i}"),
            },
        },
        slot: Band::Recall.row().slot,
        band: Band::Recall,
        reasons: BTreeSet::from([Reason::Recalled]),
        handle: Handle(format!("node_{i}")),
        tokens: 700,
        why: DropWhy::OverBudget,
    };
    let v = View {
        pov,
        shown: Vec::new(),
        dropped: vec![cut(2), cut(1)],
        residue: vec![crate::view::budget::Residue {
            band: Band::Recall,
            pieces: 2,
            tokens: 1_400,
        }],
        tokens: 0,
    };
    let renderer = TextRenderer {
        store: &c.store,
        kernel: None,
        sessions: None,
        zone: &crate::recall::stamp::system,
        since: None,
    };
    let snap = crate::view::wire::StoreSnapshot {
        now_ms: 1,
        timeline: crate::view::wire::StoreTimeline(&c.store),
        at_hand: None,
    };
    let parts = renderer.render(&v, &snap).unwrap();
    let beside = parts
        .beside
        .expect("the line is laid beside the message")
        .text;
    assert!(!beside.contains("[Recalled by the harness:"), "{beside}");
    assert!(
        beside.contains(
            "[2 more of what search recalled, about 1,400 tokens, left out; open node_2]"
        ),
        "{beside}"
    );
}

/// A plain recalled item still names its session in its header: its id and when it was said; and no
/// title, though its session has one.
#[tokio::test]
async fn a_recalled_items_header_names_its_session_by_id_place_and_time() {
    const SAID: &str = "the Hollowmere orchard gate code is kept in the blue tin";
    const TITLE: &str = "Kestrel tin ledger";
    let (r, _) = owners_rig(view);
    let c = &r.core;
    let notes = crate::tests_places::session(c, None);
    let n = crate::node::Node::user(&notes, None, "test", SAID);
    c.store.append(&[n.record().unwrap()]).unwrap();
    let mut rec = (c.store.get_session::<crate::session::SessionRecord>(&notes))
        .unwrap()
        .unwrap();
    rec.title = Some(TITLE.into());
    c.store.put_session(&notes, &rec).unwrap();
    c.runner.memory.set_ask(index_of(c, vec![notes.clone()]));
    let sid = session_in(c, None, &r.root);
    turn(c, &sid, ASKED).await;
    let q = last_call(&r);
    let held = blocks_with(&q, SAID);
    let (_, text) = held.first().expect("recalled");
    let short = crate::recall::evidence::short(&notes);
    let line = (text.lines())
        .find(|l| l.contains(&format!("session {short}")))
        .unwrap_or_else(|| panic!("the header names its session: {text}"));
    assert!(
        line.contains(" in ") && line.contains("said "),
        "place and time: {line}"
    );
    assert!(!texts(&q).contains(TITLE), "no title anywhere: {line}");
}
