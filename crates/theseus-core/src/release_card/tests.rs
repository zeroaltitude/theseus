//! The release card's second look (issue 44, step 3b): what may be a candidate (his own words, in his private
//! conversation alone, never a sealed session's, never the company place's), and that the record holds ids and
//! counts only, under no session and no turn, whatever was found.

use std::collections::HashSet;

use serde_json::json;

use super::*;
use crate::exchange::{exchange_record, Exchange, Opener};
use crate::node::{Body, Node};
use crate::store::Store;

const DM: &str = "discord:dm:42";
const LOBBY: &str = "discord:channel:7";
const OTHER_DM: &str = "discord:dm:99";

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    (dir, store)
}

/// One exchange of `place` in `session`, said by `person`, at `at`.
fn put(store: &Store, turn: &str, session: &str, place: &str, person: Option<&str>, at: u64) {
    let ask = Node::user(session, Some(turn), "owner", "the plan for the offsite");
    let x = Exchange {
        turn_id: turn.into(),
        session_id: session.into(),
        place: place.into(),
        private: place != LOBBY,
        opened_by: Opener::Message,
        person: person.map(str::to_string),
        start_ms: at,
        end_ms: Some(at + 500),
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

fn all_private(_: &str) -> bool {
    true
}

/// Of everything the words index could name, a candidate is an exchange of his own words in his private
/// conversation, not held out by a seal; the company place's exchanges, another person's words there, another
/// conversation's and a sealed session's are never candidates. Newest first.
#[test]
fn a_candidate_is_his_own_words_in_his_private_conversation_and_never_a_sealed_one() {
    let (_d, s) = store();
    put(&s, "t_old", "ses_dm", DM, Some(OWNER), 1_000);
    put(&s, "t_new", "ses_dm", DM, Some(OWNER), 2_000);
    put(&s, "t_guest", "ses_dm", DM, Some("discord:7"), 3_000);
    put(&s, "t_lobby", "ses_lobby", LOBBY, Some(OWNER), 4_000);
    put(&s, "t_other", "ses_other", OTHER_DM, Some(OWNER), 5_000);
    put(&s, "t_sealed", "ses_sealed", DM, Some(OWNER), 6_000);
    put(&s, "t_moved", "ses_dm", DM, Some(OWNER), 7_000);
    let hidden = |sid: &str| sid == "ses_sealed";
    let lens = page::Lens {
        store: &s,
        here: DM.into(),
        present: Present::OwnerAlone,
        hidden: &hidden,
        moved: &|e: &Exchange| e.turn_id == "t_moved",
        private: &all_private,
        released: &page::none_released,
        in_view: None,
        words: None,
    };
    let found: HashSet<String> = [
        "t_old", "t_new", "t_guest", "t_lobby", "t_other", "t_sealed", "t_moved", "t_none",
    ]
    .iter()
    .map(|t| t.to_string())
    .collect();
    assert_eq!(candidates(&lens, &found), vec!["t_new", "t_old"]);
}

/// One candidate is the one case a card would offer; several are no card; none is none.
#[test]
fn one_candidate_is_the_only_offer() {
    assert_eq!(Outcome::of(0), Outcome::Nothing);
    assert_eq!(Outcome::of(1), Outcome::One);
    assert_eq!(Outcome::of(2), Outcome::Several);
    assert_eq!(Outcome::of(40), Outcome::Several);
}

/// The record is under no session and no turn, so nothing that reads a session's or a turn's rows (the company
/// turn's included) finds it; it holds ids and counts and never a piece's words, and its fields are the same
/// whatever the look found.
#[test]
fn the_record_holds_ids_and_counts_only_whatever_was_found() {
    let found = Look {
        outcome: Outcome::One,
        found: 1,
        ids: vec!["t_new".into()],
    };
    let none = Look::bare(Outcome::NotAsked);
    let a = record("turn_c", "node_c", Some(LEANS_ELSEWHERE), &found);
    let b = record("turn_c", "node_c", Some("stands_alone"), &none);
    for row in [&a, &b] {
        assert_eq!(row.kind, "context.release_look");
        assert_eq!(row.session_id, None);
        assert_eq!(row.turn_id, None);
    }
    let keys = |v: &serde_json::Value| -> Vec<String> {
        let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        k.sort();
        k
    };
    assert_eq!(
        keys(&a.data),
        keys(&b.data),
        "the same fields, found or not"
    );
    assert_eq!(
        keys(&a.data),
        vec![
            "answer",
            "company_turn",
            "found",
            "ids",
            "mode",
            "node",
            "outcome"
        ]
    );
    assert_eq!(a.data["ids"], json!(["t_new"]));
    assert_eq!(a.data["mode"], "shadow");
    assert!(
        !a.data.to_string().contains("offsite"),
        "no words: {}",
        a.data
    );
}

/// The look follows only the trigger's `not_said_here`; the message's own words are read from its node.
#[test]
fn the_look_follows_only_not_said_here() {
    assert_eq!(LEANS_ELSEWHERE, "not_said_here");
    let p = theseus_judge::pack::by_name(crate::judge::leans_on::PACK).unwrap();
    let ids: Vec<&str> = p.questions[0]
        .options
        .iter()
        .map(|o| o.id.as_str())
        .collect();
    assert!(ids.contains(&LEANS_ELSEWHERE), "{ids:?}");
    let (_d, s) = store();
    let n = Node::user("ses_c", Some("turn_c"), "owner", "use what I told you");
    s.append(&[n.record().unwrap()]).unwrap();
    let (_, got) = s.get_node(&n.id).unwrap().unwrap();
    assert!(
        matches!(got.body, Body::UserMessage { ref text, .. } if text == "use what I told you")
    );
}

/// One exchange of `place` in `session`, said by `person`, with its touch, as a turn writes it.
fn said(
    store: &Store,
    turn: &str,
    session: &str,
    place: &str,
    person: &str,
    words: &str,
    at: u64,
) -> Node {
    let ask = Node::user(session, Some(turn), "owner", words);
    let x = Exchange {
        turn_id: turn.into(),
        session_id: session.into(),
        place: place.into(),
        private: true,
        opened_by: Opener::Message,
        person: Some(person.into()),
        start_ms: at,
        end_ms: Some(at + 500),
        ask: Some(ask.id.clone()),
        reply: None,
        messages: vec![ask.id.clone()],
        stop: Some("end_turn".into()),
        rebuilt: false,
    };
    let mut recs = vec![ask.record().unwrap(), exchange_record(&x).unwrap()];
    recs.extend(crate::exchange::touch(
        crate::exchange::Root::Place(place),
        turn,
        session,
        at,
    ));
    store.append(&recs).unwrap();
    ask
}

/// A words index that finds every one of `nodes` in the sessions it is scoped to.
fn index(nodes: Vec<Node>) -> crate::recall::Ask {
    use crate::recall::AskFuture;
    use theseus_protocol::index::{IndexHit, IndexQueryParams, IndexQueryResult, IndexSourceRank};
    std::sync::Arc::new(move |p: IndexQueryParams| -> AskFuture {
        let hits: Vec<IndexHit> = nodes
            .iter()
            .filter(|n| p.filters.sessions.is_empty() || p.filters.sessions.contains(&n.session_id))
            .take(p.k)
            .enumerate()
            .map(|(i, n)| IndexHit {
                text: crate::recall::text_of(n),
                time_ms: n.created_at_ms,
                node_id: n.id.clone(),
                chunk: 0,
                session_id: n.session_id.clone(),
                position: 0,
                kind: "user_message".into(),
                origin: "operator".into(),
                author: None,
                place: None,
                tool: None,
                external: false,
                entities_matched: vec![],
                entities: Vec::new(),
                sources: [(
                    "bm25".to_string(),
                    IndexSourceRank {
                        rank: i + 1,
                        score: 1.0,
                    },
                )]
                .into(),
                fused: 1.0 / (61 + i) as f64,
            })
            .collect();
        Box::pin(async move {
            Ok(IndexQueryResult {
                hits,
                indexed_through: 0,
                lag: Default::default(),
                timings: Default::default(),
                skipped: Default::default(),
                weights: Default::default(),
            })
        })
    })
}

/// Through the whole core: with his direct conversation bound, the look anchored there finds his one exchange
/// of it, and never the company channel's exchange of the same words, nor a guest's words; a second of his makes
/// it several. With no direct conversation bound there is no look.
#[tokio::test]
async fn the_look_reads_his_private_conversation_alone_and_names_one_or_several() {
    use crate::places::BoundPlace;
    use crate::tests_places::{rig, session, OWNER as OWNER_ID};
    let r = rig();
    let no_dm = look(&r.core, "the offsite plan").await;
    assert_eq!(no_dm.outcome, Outcome::NoPrivateConversation);
    let dm_target = format!("discord:dm:{OWNER_ID}");
    r.core.bind_places(vec![
        BoundPlace {
            persona: None,
            target: dm_target.clone(),
            name: "DM".into(),
            private: true,
            guild: None,
            ceiling: None,
        },
        BoundPlace {
            persona: None,
            target: "discord:channel:7".into(),
            name: "#lobby".into(),
            private: false,
            guild: Some("100000000000000001".into()),
            ceiling: None,
        },
    ]);
    assert_eq!(
        private_conversation(&r.core).as_deref(),
        Some(dm_target.as_str())
    );
    let dm = session(&r.core, Some(&format!("dm:{OWNER_ID}")));
    let lobby = session(&r.core, Some("channel:7"));
    let his = said(
        &r.core.store,
        "t_his",
        &dm,
        &dm_target,
        OWNER,
        "the offsite plan is the lighthouse",
        1_000,
    );
    let guest = said(
        &r.core.store,
        "t_guest",
        &dm,
        &dm_target,
        "discord:7",
        "the offsite plan, guest",
        2_000,
    );
    let company = said(
        &r.core.store,
        "t_lobby",
        &lobby,
        "discord:channel:7",
        OWNER,
        "the offsite plan",
        3_000,
    );
    r.core
        .runner
        .memory
        .set_ask(index(vec![his.clone(), guest.clone(), company.clone()]));
    let one = look(&r.core, "the offsite plan").await;
    assert_eq!(
        one,
        Look {
            outcome: Outcome::One,
            found: 1,
            ids: vec!["t_his".into()]
        }
    );
    let again = said(
        &r.core.store,
        "t_his_2",
        &dm,
        &dm_target,
        OWNER,
        "the offsite plan, again",
        4_000,
    );
    r.core
        .runner
        .memory
        .set_ask(index(vec![his, guest, company, again]));
    let several = look(&r.core, "the offsite plan").await;
    assert_eq!(several.outcome, Outcome::Several);
    assert_eq!(several.ids, vec!["t_his_2", "t_his"]);
}

/// Red team, through the whole core: the owner, in a channel, leans on what he told Theseus in his direct
/// conversation, and the trigger says so. The look finds his one exchange there and records its id, but nothing
/// the company turn sends holds a word of it, and the trigger's own state holds only the channel's lines.
#[tokio::test(flavor = "multi_thread")]
async fn red_team_the_company_turn_sends_nothing_of_the_look() {
    use crate::places::BoundPlace;
    use theseus_judge::fake::{FakeJev, Scripted};
    const SECRET: &str = "lighthouse";
    const HIM: u64 = 161_803_398_874_989_484;
    const DEN: u64 = 271_000_000_000_000_001;
    let jev = FakeJev::start().unwrap();
    jev.script(
        "leans_on",
        Scripted::Choice {
            option: LEANS_ELSEWHERE.into(),
            confidence: 0.99,
        },
    );
    let r = crate::tests_rerank::rig(Some(&jev), |c| {
        c.judge.packs.remove(crate::judge::leans_on::PACK);
    });
    let dm_target = format!("discord:dm:{HIM}");
    r.core.bind_places(vec![
        BoundPlace {
            target: format!("discord:channel:{DEN}"),
            name: "#den".into(),
            private: true,
            ..Default::default()
        },
        BoundPlace {
            target: dm_target.clone(),
            name: "DM".into(),
            private: true,
            ..Default::default()
        },
    ]);
    let dm = crate::tests_rerank::session(&r.core, Some(&format!("dm:{HIM}")), &[]);
    let his = said(
        &r.core.store,
        "xq_private_turn_9",
        &dm,
        &dm_target,
        OWNER,
        &format!("the offsite plan is the {SECRET} on Friday"),
        1_000,
    );
    r.core.runner.memory.set_ask(index(vec![his]));
    let den = crate::tests_rerank::session(&r.core, Some(&format!("channel:{DEN}")), &[]);
    crate::tests_rerank::turn(&r.core, &den, "go with the offsite plan I told you about").await;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let row = loop {
        let rows: Vec<(u64, LedgerRow)> = r.core.store.ledger_tail(500).unwrap();
        if let Some((_, row)) = rows
            .into_iter()
            .find(|(_, x)| x.kind == "context.release_look")
        {
            break row;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no release look was recorded"
        );
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    };
    assert_eq!(row.data["answer"], LEANS_ELSEWHERE);
    assert_eq!(row.data["outcome"], "one", "{}", row.data);
    assert_eq!(row.data["ids"], json!(["xq_private_turn_9"]));
    assert!(!row.data.to_string().contains(SECRET));
    let sent = r.model.requests.lock().unwrap().clone();
    assert!(!sent.is_empty());
    for req in &sent {
        let bytes = serde_json::to_string(req).unwrap();
        assert!(
            !bytes.contains(SECRET),
            "the company turn was sent the private piece"
        );
        assert!(
            !bytes.contains("xq_private_turn_9"),
            "the company turn was sent the look's id"
        );
    }
    let asked: Vec<_> = jev
        .seen()
        .into_iter()
        .filter(|s| s.body["questions"].to_string().contains("leans_on.v1/"))
        .collect();
    assert_eq!(asked.len(), 1);
    assert!(!asked[0].body["state"].to_string().contains(SECRET));
}
