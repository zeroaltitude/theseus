//! Share this exact message to this place (issue 44): through the history tool's real reader and the real
//! connection. Only the message's words are shown, in the named place alone; one strict check at the release and
//! at every read (a seal set later, the reader inside that very thread, a move to a clear thread); one record
//! however many grants race; the page's own bounds; and no connection but the owner's own reaches a record.

use std::sync::Arc;

use serde_json::{json, Value};

use super::*;
use crate::exchange::{
    exchange_record, mark_record, touch, Exchange, Opener, OwnerMark, Peer, Root,
};
use crate::history_tool::{answer, Board};
use crate::node::Node;
use crate::view::history::{HistoryArgs, LOOKS_PER_TURN};
use crate::view::{Anchor, PointOfView, Present};

const DM: &str = "discord:dm:42";
const DEN: &str = "discord:channel:7";
const PIER: &str = "discord:channel:8";
const HARMLESS: &str = "the offsite is on Friday";

/// One exchange of the owner's at `place` in `session`: his message, and `rest` (reply, steer, later nodes) in it.
fn said(core: &Core, turn: &str, session: &str, words: &str, rest: &[Node]) -> Node {
    let ask = Node::user(session, Some(turn), "owner", words);
    let mark = OwnerMark {
        node: ask.id.clone(),
        in_person: true,
        owner_handle: None,
        peer: Peer::Person,
        agent: None,
        surface: "discord".into(),
        place: DM.into(),
        arrived_session: session.into(),
        ran_session: session.into(),
        author: None,
        at_ms: ask.created_at_ms,
    };
    let mut messages = vec![ask.id.clone()];
    messages.extend(rest.iter().map(|n| n.id.clone()));
    let x = Exchange {
        turn_id: turn.into(),
        session_id: session.into(),
        place: DM.into(),
        private: true,
        opened_by: Opener::Message,
        person: Some(OWNER.into()),
        start_ms: ask.created_at_ms,
        end_ms: None,
        ask: Some(ask.id.clone()),
        reply: rest.first().map(|n| n.id.clone()),
        messages,
        stop: None,
        rebuilt: false,
    };
    let mut recs = vec![
        ask.record().unwrap(),
        mark_record(&mark).unwrap(),
        exchange_record(&x).unwrap(),
    ];
    recs.extend(rest.iter().map(|n| n.record().unwrap()));
    recs.extend(touch(Root::Place(DM), turn, session, x.start_ms));
    core.store.append(&recs).unwrap();
    ask
}

fn reply(session: &str, turn: &str, text: &str) -> Node {
    let mut n = Node::user(session, Some(turn), "assistant", text);
    n.origin = crate::node::Origin::Agent;
    n
}

/// The history tool's answer for a turn of `session` anchored at `place`, others present.
async fn read(core: &Arc<Core>, board: &Board, session: &str, place: &str, args: Value) -> String {
    let pov = PointOfView {
        anchor: Anchor::Place(place.into()),
        person: Some(OWNER.into()),
        present: Present::Private,
        at_hand: None,
        now_ms: 0,
    };
    let a: HistoryArgs = serde_json::from_value(args).unwrap();
    match answer(core, board, (session, "t_reader"), &pov, &a).await {
        Ok(o) => o.text,
        Err(e) => format!("ERR {e}"),
    }
}

/// The real reader shows the released message's words alone, labelled, in the named place alone: on the page and
/// when opened. The reply's private sentinel, another person's steer and a node added after the grant never appear.
#[tokio::test]
async fn only_the_released_message_is_shown_and_only_where_it_was_released() {
    let r = crate::tests_places::rig();
    let dm = crate::tests_places::session(&r.core, Some("dm:42"));
    let den = crate::tests_places::session(&r.core, Some("channel:7"));
    let steer = Node::user(&dm, Some("t_src"), "guest", "GUEST-STEER-SENTINEL");
    let ask = said(
        &r.core,
        "t_src",
        &dm,
        HARMLESS,
        &[reply(&dm, "t_src", "PRIVATE-REPLY-SENTINEL"), steer],
    );
    let shown = piece(&r.core, &ask.id).unwrap();
    assert_eq!(shown.text, HARMLESS);
    let rel = grant(&r.core, &ask.id, &shown.digest, DEN, "cli").unwrap();
    // Added to the exchange after the grant.
    r.core
        .store
        .append(&[reply(&dm, "t_src", "LATER-SENTINEL").record().unwrap()])
        .unwrap();
    let board = Board::default();
    let page = read(&r.core, &board, &den, DEN, json!({})).await;
    let opened = read(&r.core, &board, &den, DEN, json!({"open": rel.id})).await;
    for text in [&page, &opened] {
        assert!(text.contains(HARMLESS) && text.contains(LABEL), "{text}");
        for s in [
            "PRIVATE-REPLY-SENTINEL",
            "GUEST-STEER-SENTINEL",
            "LATER-SENTINEL",
        ] {
            assert!(!text.contains(s), "{s} escaped: {text}");
        }
    }
    let elsewhere = read(&r.core, &board, &den, PIER, json!({})).await;
    assert!(!elsewhere.contains(HARMLESS), "{elsewhere}");
    let opened_elsewhere = read(&r.core, &board, &den, PIER, json!({"open": rel.id})).await;
    assert!(!opened_elsewhere.contains(HARMLESS), "{opened_elsewhere}");
    // Words that are not the words he was shown are not released; a release is made once.
    assert!(grant(&r.core, &ask.id, &digest("other words"), PIER, "cli").is_err());
    assert!(grant(&r.core, &ask.id, &shown.digest, DEN, "cli").is_err());
    withdraw(&r.core.store, &rel.id, "cli").unwrap();
    let after = read(&r.core, &board, &den, DEN, json!({"open": rel.id})).await;
    assert!(!after.contains(HARMLESS), "{after}");
    assert!(withdraw(&r.core.store, &rel.id, "cli").is_err());
    assert!(withdraw(&r.core.store, "rel_forged", "cli").is_err());
}

/// One strict check at every read, with no thread's own exception: released, then its thread is sealed, and a
/// company arrival routed into that very thread reads nothing; released, then moved into a CLEAR thread, nothing.
#[tokio::test]
async fn a_seal_or_a_move_after_the_release_ends_it_even_inside_that_thread() {
    use theseus_protocol::threads::{ThreadNewParams, ThreadSealParams};
    let r = crate::tests_places::rig();
    let cli = || crate::approval::Answerer {
        label: "the CLI".into(),
        surface: crate::approval::Surface::Cli,
        discord: None,
        agent: None,
    };
    let new = ThreadNewParams {
        slug: "kelp".into(),
        title: Some("Kelp".into()),
        ..Default::default()
    };
    r.core.thread_new(&new, cli()).unwrap();
    let kelp = r.core.thread_session("kelp", cli()).unwrap().session_id;
    let dm = crate::tests_places::session(&r.core, Some("dm:42"));
    let in_thread = said(&r.core, "t_kelp", &kelp, "the kelp plan", &[]);
    let in_dm = said(&r.core, "t_dm", &dm, "the dm plan", &[]);
    let a = grant(&r.core, &in_thread.id, &digest("the kelp plan"), DEN, "cli").unwrap();
    let b = grant(&r.core, &in_dm.id, &digest("the dm plan"), DEN, "cli").unwrap();
    let board = Board::default();
    let page = read(&r.core, &board, &kelp, DEN, json!({})).await;
    assert!(
        page.contains("the kelp plan") && page.contains("the dm plan"),
        "{page}"
    );
    let seal = ThreadSealParams {
        slug: "kelp".into(),
        reason: Some("embargoed".into()),
        ..Default::default()
    };
    r.core.thread_seal(&seal, cli()).unwrap();
    r.core
        .store
        .put_meta(
            &crate::moved::moved_of_key(&in_dm.id),
            &crate::moved::MovedTo {
                thread: "open".into(),
                split: "s".into(),
            },
        )
        .unwrap();
    let board = Board::default();
    let page = read(&r.core, &board, &kelp, DEN, json!({})).await;
    for (id, words) in [(&a.id, "the kelp plan"), (&b.id, "the dm plan")] {
        assert!(!page.contains(words), "{page}");
        let opened = read(&r.core, &board, &kelp, DEN, json!({"open": id})).await;
        assert!(!opened.contains(words), "{opened}");
    }
    assert!(
        grant(&r.core, &in_dm.id, &digest("the dm plan"), PIER, "cli").is_err(),
        "moved: never released"
    );
}

/// Two grants of one message to one place racing: one record; its withdrawal ends admission.
#[tokio::test(flavor = "multi_thread")]
async fn racing_grants_make_one_record() {
    let r = crate::tests_places::rig();
    let dm = crate::tests_places::session(&r.core, Some("dm:42"));
    let ask = said(&r.core, "t_race", &dm, HARMLESS, &[]);
    let d = digest(HARMLESS);
    let goes: Vec<_> = (0..8)
        .map(|_| {
            let (core, id, d) = (r.core.clone(), ask.id.clone(), d.clone());
            std::thread::spawn(move || grant(&core, &id, &d, DEN, "cli"))
        })
        .collect();
    let won: Vec<Release> = goes
        .into_iter()
        .filter_map(|g| g.join().unwrap().ok())
        .collect();
    assert_eq!(won.len(), 1, "{won:?}");
    assert_eq!(released_to(&r.core.store, DEN).len(), 1);
    withdraw(&r.core.store, &won[0].id, "cli").unwrap();
    assert!(released_to(&r.core.store, DEN).is_empty());
    assert!(live(&r.core.store, DEN, &ask.id).is_none());
}

/// Released messages ride the page's bounds: a page lists ten and says how many more; a look past the turn's
/// allowance lists none.
#[tokio::test]
async fn released_messages_keep_to_the_pages_bounds() {
    let r = crate::tests_places::rig();
    let dm = crate::tests_places::session(&r.core, Some("dm:42"));
    let den = crate::tests_places::session(&r.core, Some("channel:7"));
    for i in 0..(PER_PAGE + 2) {
        let words = format!("plan number {i}");
        let ask = said(&r.core, &format!("t_{i}"), &dm, &words, &[]);
        grant(&r.core, &ask.id, &digest(&words), DEN, "cli").unwrap();
    }
    let board = Board::default();
    let page = read(&r.core, &board, &den, DEN, json!({})).await;
    assert_eq!(page.matches(LABEL).count(), PER_PAGE, "{page}");
    assert!(page.contains("2 more released messages"), "{page}");
    for _ in 1..LOOKS_PER_TURN {
        read(&r.core, &board, &den, DEN, json!({})).await;
    }
    let spent = read(&r.core, &board, &den, DEN, json!({})).await;
    assert_eq!(spent.matches(LABEL).count(), 0, "{spent}");
}

/// One request over a real connection of `client`, and its answer's line.
async fn rpc_as(
    core: &Arc<Core>,
    client: crate::approval::Client,
    method: &str,
    params: Value,
) -> String {
    use theseus_protocol::{Id, Request};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (ours, theirs) = tokio::io::duplex(64 * 1024);
    let (sr, sw) = tokio::io::split(theirs);
    let _srv = tokio::spawn(core.clone().serve_connection(sr, sw, client));
    let (cr, mut cw) = tokio::io::split(ours);
    let line = serde_json::to_string(&Request::new(Id::Num(1), method, params)).unwrap();
    cw.write_all(format!("{line}\n").as_bytes()).await.unwrap();
    let mut lines = BufReader::new(cr).lines();
    loop {
        let line = tokio::time::timeout(std::time::Duration::from_secs(10), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if line.contains("\"id\":1") {
            return line;
        }
    }
}

/// Through the real connection, each method with valid parameters (a release to a second place, an offer, the
/// list, and a withdrawal of a live release): an agent's process at the terminal and an unnamed connection are
/// refused before anything is read or written, and nothing changes; the owner's own terminal is answered.
#[tokio::test]
async fn an_agents_process_or_an_unnamed_connection_cannot_release() {
    use crate::approval::{Client, Surface};
    use crate::peer_proc::Behind;
    use theseus_protocol::method;
    let r = crate::tests_places::rig();
    let dm = crate::tests_places::session(&r.core, Some("dm:42"));
    let ask = said(&r.core, "t_rpc", &dm, HARMLESS, &[]);
    let rel = grant(&r.core, &ask.id, &digest(HARMLESS), DEN, "cli").unwrap();
    let calls = [
        (
            method::EXCHANGE_RELEASE,
            json!({"message": ask.id, "digest": digest(HARMLESS), "to": PIER}),
        ),
        (method::EXCHANGE_OFFER, json!({"message": ask.id})),
        (method::EXCHANGE_RELEASES, json!({})),
        (method::EXCHANGE_WITHDRAW, json!({"id": rel.id})),
    ];
    // The authorization refusal, before anything is read: never a parse error or a missing record.
    let refused = format!(
        "\"error\":{{\"code\":{}",
        theseus_protocol::error_code::REFUSED
    );
    let agent = || Client::new("sock#1", Surface::Cli).behind(Behind::Agent("claude".into()));
    let unnamed = || Client::new("test", Surface::Unnamed);
    for client in [agent(), unnamed()] {
        for (m, params) in &calls {
            let line = rpc_as(&r.core, client.clone(), m, params.clone()).await;
            assert!(line.contains(&refused), "{m}: {line}");
            assert!(
                !line.contains(HARMLESS) && !line.contains(&rel.id),
                "{m}: {line}"
            );
        }
    }
    assert!(
        released_to(&r.core.store, PIER).is_empty(),
        "no release made"
    );
    let still = live(&r.core.store, DEN, &ask.id).expect("the release is not withdrawn");
    assert_eq!(still.id, rel.id);
    assert_eq!(all(&r.core.store).unwrap().len(), 1);
    let owner = Client::new("sock#2", Surface::Cli).behind(Behind::Owner);
    let offered = rpc_as(
        &r.core,
        owner,
        method::EXCHANGE_OFFER,
        json!({"message": ask.id}),
    )
    .await;
    assert!(
        offered.contains(HARMLESS) && offered.contains(&digest(HARMLESS)),
        "{offered}"
    );
}

/// The released message's words overwritten after the grant: the digest no longer matches, and neither the page
/// nor the handle shows the old words or the new.
#[tokio::test]
async fn a_message_overwritten_after_the_grant_shows_nothing() {
    let r = crate::tests_places::rig();
    let dm = crate::tests_places::session(&r.core, Some("dm:42"));
    let den = crate::tests_places::session(&r.core, Some("channel:7"));
    let ask = said(&r.core, "t_over", &dm, HARMLESS, &[]);
    let rel = grant(&r.core, &ask.id, &digest(HARMLESS), DEN, "cli").unwrap();
    let mut over = Node::user(&dm, Some("t_over"), "owner", "OVERWRITTEN-SENTINEL");
    over.id = ask.id.clone();
    r.core.store.append(&[over.record().unwrap()]).unwrap();
    let (_, now) = r.core.store.get_node(&ask.id).unwrap().unwrap();
    assert!(
        matches!(&now.body, crate::node::Body::UserMessage { text, .. } if text == "OVERWRITTEN-SENTINEL"),
        "the store reads the new words"
    );
    let board = Board::default();
    let page = read(&r.core, &board, &den, DEN, json!({})).await;
    let opened = read(&r.core, &board, &den, DEN, json!({"open": rel.id})).await;
    for text in [&page, &opened] {
        assert!(!text.contains(LABEL), "{text}");
        for s in [HARMLESS, "OVERWRITTEN-SENTINEL"] {
            assert!(!text.contains(s), "{s} shown: {text}");
        }
    }
}

/// The lines a page lists its released messages on, and its "more" line: everything from the first label on.
fn released_part(page: &str) -> String {
    page.find(&format!("\n[{LABEL}]"))
        .map_or_else(String::new, |i| page[i..].to_string())
}

/// What is listed hangs on no position in history: the same releases, then the same place with hundreds of held
/// touches added (a look stopped by the tool's reading limit), list byte for byte the same on a bare first call;
/// a call continued with the opaque token, or with a cursor, lists none, and each still opens by its handle.
#[tokio::test]
async fn released_messages_are_listed_on_a_bare_first_call_alone() {
    use crate::history_tool::page::READ_MOST;
    let r = crate::tests_places::rig();
    let dm = crate::tests_places::session(&r.core, Some("dm:42"));
    let den = crate::tests_places::session(&r.core, Some("channel:7"));
    let mut ids = Vec::new();
    for i in 0..(PER_PAGE + 2) {
        let words = format!("plan number {i}");
        let ask = said(&r.core, &format!("t_{i}"), &dm, &words, &[]);
        ids.push(
            grant(&r.core, &ask.id, &digest(&words), DEN, "cli")
                .unwrap()
                .id,
        );
    }
    let quiet = read(&r.core, &Board::default(), &den, DEN, json!({})).await;
    let listed = released_part(&quiet);
    assert_eq!(listed.matches(LABEL).count(), PER_PAGE, "{quiet}");
    assert!(listed.contains("2 more released messages"), "{quiet}");
    // Held touches: this place's timeline names exchanges no reader is admitted to, all older than the reader's
    // session (a bare page walks from what its view already holds back).
    let base = 1_000;
    let held: Vec<_> = (0..(READ_MOST as u64 + 50))
        .filter_map(|i| {
            touch(
                Root::Place(DEN),
                &format!("t_held_{i}"),
                "ses_held",
                base + i,
            )
        })
        .collect();
    r.core.store.append(&held).unwrap();
    let board = Board::default();
    let limited = read(&r.core, &board, &den, DEN, json!({})).await;
    assert!(limited.contains("reading limit"), "{limited}");
    assert_eq!(released_part(&limited), listed, "byte for byte");
    let token = json!({"before": {"at_ms": 0, "turn_id": "resume:0"}});
    let continued = read(&r.core, &board, &den, DEN, token).await;
    assert!(!continued.starts_with("ERR"), "{continued}");
    let cursor = json!({"before": {"at_ms": base, "turn_id": "t_held_0"}});
    let later = read(&r.core, &board, &den, DEN, cursor).await;
    for text in [&continued, &later] {
        assert_eq!(text.matches(LABEL).count(), 0, "{text}");
        assert!(!text.contains("more released messages"), "{text}");
    }
    for id in [&ids[0], &ids[PER_PAGE + 1]] {
        let opened = read(&r.core, &Board::default(), &den, DEN, json!({"open": id})).await;
        assert!(opened.contains(LABEL), "{opened}");
    }
}

/// A message released, then split into a thread, then put back by the unsplit: the unsplit leaves a move record,
/// and a message that was ever moved is shared again only by a new message. Not shown, and not granted again.
#[tokio::test]
async fn a_message_split_out_and_put_back_stays_unreleased() {
    use theseus_protocol::threads::{ThreadSplitParams, ThreadUnsplitParams};
    let r = crate::tests_places::rig();
    let cli = || crate::approval::Answerer {
        label: "the CLI".into(),
        surface: crate::approval::Surface::Cli,
        discord: None,
        agent: None,
    };
    let dm = crate::tests_places::session(&r.core, Some("dm:42"));
    let den = crate::tests_places::session(&r.core, Some("channel:7"));
    crate::tests_places::turn(&r.core, &dm, "good morning").await;
    let answer = reply(&dm, "t_split", "the reply to it");
    let ask = said(
        &r.core,
        "t_split",
        &dm,
        HARMLESS,
        std::slice::from_ref(&answer),
    );
    let rel = grant(&r.core, &ask.id, &digest(HARMLESS), DEN, "cli").unwrap();
    let shown = read(&r.core, &Board::default(), &den, DEN, json!({})).await;
    assert!(shown.contains(HARMLESS), "{shown}");
    let at = |id: &str| {
        let nodes = r.core.store.transcript(&dm).unwrap();
        nodes
            .iter()
            .find(|(_, n)| n.id == id)
            .map(|(p, _)| *p)
            .unwrap()
    };
    let split = ThreadSplitParams {
        from: dm.clone(),
        slug: "offsite".into(),
        range: Some([at(&ask.id), at(&answer.id)]),
        ..Default::default()
    };
    let into = r
        .core
        .thread_split(&split, cli())
        .map_err(|e| e.message)
        .unwrap();
    let unsplit = ThreadUnsplitParams {
        split: into.split.clone(),
        ..Default::default()
    };
    r.core
        .thread_unsplit(&unsplit, cli())
        .map_err(|e| e.message)
        .unwrap();
    let back: Option<Option<crate::moved::MovedTo>> = r
        .core
        .store
        .get_meta(&crate::moved::moved_of_key(&ask.id))
        .unwrap();
    assert_eq!(back, Some(None), "the unsplit leaves its record");
    let board = Board::default();
    let page = read(&r.core, &board, &den, DEN, json!({})).await;
    let opened = read(&r.core, &board, &den, DEN, json!({"open": rel.id})).await;
    for text in [&page, &opened] {
        assert!(!text.contains(HARMLESS), "{text}");
    }
    assert!(piece(&r.core, &ask.id).is_err());
    assert!(
        grant(&r.core, &ask.id, &digest(HARMLESS), PIER, "cli").is_err(),
        "moved and put back: never granted"
    );
}
