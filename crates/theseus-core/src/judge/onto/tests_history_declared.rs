//! His declaration over the history's filing (issue 51; `history.rs`):
//! the old exchanges no mark proves, whose opening message arrived in one
//! of his own places, counted as his only when he says so, through a whole
//! core and the fake Jev, over invented exchanges in a temporary store.

use serde_json::json;
use theseus_judge::fake::FakeJev;
use theseus_protocol::exchange::ExchangeTopicsParams;
use theseus_protocol::{error_code, method, LedgerKind};

use super::tests_history::{
    a_judgments_row, ledger, ledger_topic, of_kind, params, rows_written, run, said, timeline,
    untouch, SAID, WORDS,
};
use crate::approval::{Client, Surface};
use crate::exchange::{self, Exchange, Opener};
use crate::node::Node;
use crate::peer_proc::Behind;
use crate::tests_m3::{rpc_as, surface};
use crate::tests_onto::{bound, rig};
use crate::Core;

/// A run that carries his declaration.
fn declared(dry_run: bool) -> ExchangeTopicsParams {
    ExchangeTopicsParams {
        include_unmarked: true,
        ..params(dry_run, None)
    }
}

/// The owner's own DM as an exchange records it: the place code its
/// message stored, and the exchange's recorded place, the DM `bound` binds
/// (whose person the default rule makes the owner).
const HIS_DM: (Option<&str>, &str) = (Some("discord:dm"), "discord:dm:42");

/// An old exchange as the rebuild wrote it from history before marks: no
/// person, no place, and an opening message with no mark, which arrived at
/// the place code `at` (none: before places were recorded).
fn unmarked(
    core: &Core,
    (sid, turn): (&str, &str),
    text: &str,
    at: Option<&str>,
    start_ms: u64,
) -> Node {
    unmarked_in(core, (sid, turn), text, (at, "unknown"), start_ms)
}

/// An old unmarked exchange, no person, whose message stored the place code
/// `at` and whose exchange recorded the place `place`.
fn unmarked_in(
    core: &Core,
    (sid, turn): (&str, &str),
    text: &str,
    (at, place): (Option<&str>, &str),
    start_ms: u64,
) -> Node {
    let mut n = Node::user(sid, Some(turn), "sock#1", text);
    n.surface = at.map(str::to_string);
    core.store.append(&[n.record().unwrap()]).unwrap();
    let e = Exchange {
        turn_id: turn.into(),
        session_id: sid.into(),
        place: place.into(),
        private: false,
        opened_by: Opener::Message,
        person: None,
        start_ms,
        end_ms: Some(start_ms + 1_000),
        ask: Some(n.id.clone()),
        reply: None,
        messages: vec![n.id.clone()],
        stop: Some("end_turn".into()),
        rebuilt: true,
    };
    core.store
        .append(&[exchange::exchange_record(&e).unwrap()])
        .unwrap();
    n
}

/// Every request Jev saw, as one string.
fn sent(jev: &FakeJev) -> String {
    jev.seen().iter().map(|s| s.body.to_string()).collect()
}

/// KEY: an old exchange in his DM that no mark proves is not asked on a
/// plain run, and is counted as no record proves it and as in his own
/// places; with his declaration it is asked once, touched, and its row says
/// it was declared.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unmarked_dm_exchange_is_asked_only_when_he_declares() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    unmarked_in(&r.core, (&sid, "turn_dm"), SAID, HIS_DM, 5_000);
    let plain = run(&r.core, params(false, None)).await;
    assert_eq!(
        (plain.unproven, plain.declared, plain.to_ask, plain.asked),
        (1, 1, 0, 0),
        "{plain:?}"
    );
    assert!(!plain.include_unmarked);
    assert!(r.jev.seen().is_empty(), "no call without his declaration");
    assert!(timeline(&r.core, "topic:ledger").is_empty());
    let out = run(&r.core, declared(false)).await;
    assert!(out.include_unmarked, "{out:?}");
    assert_eq!(
        (out.declared, out.to_ask, out.asked, out.filed, out.touches),
        (1, 1, 1, 1, 1),
        "{out:?}"
    );
    assert_eq!(r.jev.seen().len(), 1, "one call");
    let [t] = timeline(&r.core, "topic:ledger")
        .try_into()
        .unwrap_or_else(|v| panic!("one touch: {v:?}"));
    assert_eq!((t.exchange.as_str(), t.at_ms), ("turn_dm", 5_000));
    let calls = of_kind(&r.core, LedgerKind::JudgeCall);
    let [call] = calls.as_slice() else {
        panic!("one judgment: {calls:?}");
    };
    assert_eq!(call.data["context"]["purpose"], "history");
    assert_eq!(call.data["context"]["declared"], "unmarked");
}

/// KEY (the shared place stays out): with his declaration, none of these is
/// sent, a dry run's count or a run's call: a shared channel's unmarked
/// message, an agent's message at his terminal (its mark proves it was not
/// him), a guest's in a private channel, a message from before places were
/// recorded, a voice message, a Discord one whose place no record names.
/// The one from before places is counted on its own.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_declaration_reaches_no_shared_place_no_marked_message_and_no_unrecorded_place() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    let c = &r.core;
    let words = [
        "Gullwhistle",
        "Brackenmoor",
        "Tiddlesworth",
        "Ombrelune",
        "Fennick",
        "Quorrow",
    ];
    unmarked(
        c,
        (&sid, "turn_shared"),
        &format!("ask {} now", words[0]),
        Some("discord:channel:shared:harbor"),
        1_000,
    );
    let agent = unmarked(
        c,
        (&sid, "turn_agent"),
        &format!("ask {} now", words[1]),
        Some("terminal"),
        2_000,
    );
    let mark = exchange::OwnerMark {
        node: agent.id.clone(),
        in_person: false,
        owner_handle: None,
        peer: exchange::Peer::Agent,
        agent: Some("an agent".into()),
        surface: "cli".into(),
        place: "local".into(),
        arrived_session: sid.clone(),
        ran_session: sid.clone(),
        author: None,
        at_ms: 2_000,
    };
    c.store
        .append(&[exchange::mark_record(&mark).unwrap()])
        .unwrap();
    unmarked(
        c,
        (&sid, "turn_guest"),
        &format!("ask {} now", words[2]),
        Some("discord:channel:private:den"),
        3_000,
    );
    let mut e = exchange::exchange(&c.store, "turn_guest").unwrap().unwrap();
    e.person = Some("discord:77".into());
    c.store
        .append(&[exchange::exchange_record(&e).unwrap()])
        .unwrap();
    unmarked(
        c,
        (&sid, "turn_before"),
        &format!("ask {} now", words[3]),
        None,
        4_000,
    );
    unmarked(
        c,
        (&sid, "turn_voice"),
        &format!("ask {} now", words[4]),
        Some("voice"),
        5_000,
    );
    unmarked(
        c,
        (&sid, "turn_discord"),
        &format!("ask {} now", words[5]),
        Some("discord"),
        6_000,
    );
    let dry = run(c, declared(true)).await;
    assert_eq!(
        (
            dry.not_owner,
            dry.unproven,
            dry.declared,
            dry.unplaced,
            dry.to_ask
        ),
        (6, 4, 0, 1, 0),
        "{dry:?}"
    );
    let out = run(c, declared(false)).await;
    assert_eq!((out.to_ask, out.asked, out.touches), (0, 0, 0), "{out:?}");
    let seen = sent(&r.jev);
    for w in words {
        assert!(!seen.contains(w), "{w} was sent");
    }
    assert!(r.jev.seen().is_empty(), "no call");
    assert!(of_kind(c, LedgerKind::JudgeCall).is_empty());
}

/// KEY: his declaration never makes old words his. After a declared run,
/// the exchange's stored person is still none and it is still the
/// rebuild's; the view shows its author as unknown.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_declared_exchange_keeps_no_person() {
    use crate::view::select::sources::Exchanges as _;
    use crate::view::sources_store::{Sessions, StoreSources};
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    unmarked_in(&r.core, (&sid, "turn_dm"), SAID, HIS_DM, 5_000);
    let out = run(&r.core, declared(false)).await;
    assert_eq!(out.filed, 1, "{out:?}");
    let ex = exchange::exchange(&r.core.store, "turn_dm")
        .unwrap()
        .unwrap();
    assert_eq!((ex.person, ex.rebuilt), (None, true));
    let sessions = Sessions::default();
    let src = StoreSources {
        store: &r.core.store,
        mark: None,
        recalled: Vec::new(),
        sessions: &sessions,
        own: None,
    };
    let facts = src.facts("turn_dm").unwrap().unwrap();
    assert_eq!(facts.marks.author, crate::view::Author::Unknown);
    assert_eq!(facts.person, None);
}

/// KEY: a declared exchange is asked as in a private place, as the live
/// point asks his own: the state's place says `(private)` and its author is
/// the operator, at his terminal and in his DM alike.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_declared_exchange_is_asked_as_private() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    unmarked_in(&r.core, (&sid, "turn_dm"), SAID, HIS_DM, 5_000);
    unmarked(&r.core, (&sid, "turn_cli"), SAID, Some("terminal"), 6_000);
    let out = run(&r.core, declared(false)).await;
    assert_eq!(out.asked, 2, "{out:?}");
    let calls = of_kind(&r.core, LedgerKind::JudgeCall);
    assert_eq!(calls.len(), 2, "{calls:?}");
    for call in &calls {
        let kind = call.data["context"]["place_kind"]
            .as_str()
            .unwrap_or_default();
        assert!(kind.ends_with("(private)"), "{kind}");
    }
    let seen = sent(&r.jev);
    assert!(!seen.contains("a person in a shared place"), "{seen}");
}

/// KEY (the undo holds): a declared answer's missing touches are written
/// again from its row only by a run that carries the declaration; a plain
/// run writes none and asks nothing. Neither calls.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn declared_rows_are_replayed_only_under_the_declaration() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    unmarked_in(&r.core, (&sid, "turn_dm"), SAID, HIS_DM, 5_000);
    let first = run(&r.core, declared(false)).await;
    assert_eq!(first.filed, 1, "{first:?}");
    untouch(&r.core, "turn_dm");
    assert!(timeline(&r.core, "topic:ledger").is_empty());
    let plain = run(&r.core, params(false, None)).await;
    assert_eq!(
        (plain.asked_before, plain.asked, plain.filed, plain.touches),
        (0, 0, 0, 0),
        "{plain:?}"
    );
    assert!(
        timeline(&r.core, "topic:ledger").is_empty(),
        "a plain run replays none"
    );
    let again = run(&r.core, declared(false)).await;
    assert_eq!(
        (again.asked_before, again.asked, again.filed),
        (1, 0, 1),
        "{again:?}"
    );
    assert_eq!(timeline(&r.core, "topic:ledger").len(), 1);
    assert_eq!(r.jev.seen().len(), 1, "the first run's call alone");
}

/// The requests of `exchange.topics` that carry his declaration, a dry run
/// and a run.
fn declarations() -> Vec<serde_json::Value> {
    vec![
        json!({"dry_run": true, "include_unmarked": true}),
        json!({"dry_run": false, "include_unmarked": true}),
    ]
}

/// KEY (his decision: his own terminal only): his declaration is refused
/// from his web UI, an agent's process at his terminal, Discord, MCP and an
/// unnamed connection, a dry run and a run, in words; nothing is asked and
/// nothing written; each is ledgered but MCP's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_his_own_terminal_declares() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    unmarked_in(&r.core, (&sid, "turn_dm"), SAID, HIS_DM, 5_000);
    let callers = [
        ("the owner's web UI", Client::new("ws#1", Surface::Web)),
        (
            "an agent at the terminal",
            surface("sock#2", Surface::Cli).behind(Behind::Agent("claude".into())),
        ),
        ("discord", Client::new("discord", Surface::Discord)),
        ("mcp", Client::new("mcp", Surface::Mcp)),
        (
            "an unnamed connection",
            Client::new("conn#9", Surface::Unnamed),
        ),
    ];
    let asks = declarations();
    for (who, client) in callers {
        for p in &asks {
            let e = rpc_as(&r.core, client.clone(), method::EXCHANGE_TOPICS, p.clone())
                .await
                .expect_err(who);
            assert_eq!(e.code, error_code::REFUSED, "{who}: {e:?}");
            let said = match who {
                "mcp" => "may not call exchange.topics",
                _ => "Nothing was asked.",
            };
            assert!(e.message.ends_with(said), "{who}: {e:?}");
        }
    }
    let refused = of_kind(&r.core, LedgerKind::ApprovalRefused);
    assert_eq!(
        refused.len(),
        4 * asks.len(),
        "each refusal ledgered, MCP's apart: {refused:?}"
    );
    assert!(r.jev.seen().is_empty(), "no call");
    assert!(of_kind(&r.core, LedgerKind::JudgeCall).is_empty());
    assert!(timeline(&r.core, "topic:ledger").is_empty());
}

/// KEY: from his own terminal, in person, his declaration runs; its answer
/// and every row it wrote hold counts and no word of the message.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn his_declaration_at_his_terminal_runs_and_no_word_reaches_a_row() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    unmarked_in(&r.core, (&sid, "turn_dm"), SAID, HIS_DM, 5_000);
    let mut answers = String::new();
    for p in declarations() {
        let v = rpc_as(
            &r.core,
            surface("sock#1", Surface::Cli),
            method::EXCHANGE_TOPICS,
            p,
        )
        .await
        .unwrap();
        answers.push_str(&v.to_string());
    }
    assert!(
        sent(&r.jev).contains(WORDS[0]),
        "Jev was asked about the message"
    );
    assert_eq!(timeline(&r.core, "topic:ledger").len(), 1);
    let rows = format!("{:?}", ledger(&r.core));
    for w in WORDS {
        assert!(!answers.contains(w), "{w} in the answer: {answers}");
        assert!(!rows.contains(w), "{w} in a row");
    }
}

/// The undo's request, a dry run or a run.
fn undo(dry_run: bool) -> ExchangeTopicsParams {
    ExchangeTopicsParams {
        undo_declared: true,
        ..params(dry_run, None)
    }
}

/// KEY (the undo): it ends the touches of the answers his declaration filed,
/// and nothing else: his own exchange, proven by its mark, keeps its touch,
/// and every row stays. It says how many, and calls nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn undo_ends_only_the_touches_of_declared_rows() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    said(&r.core, &sid, "turn_his", SAID, 4_000);
    unmarked_in(&r.core, (&sid, "turn_dm"), SAID, HIS_DM, 5_000);
    let out = run(&r.core, declared(false)).await;
    assert_eq!((out.asked, out.filed), (2, 2), "{out:?}");
    assert_eq!(timeline(&r.core, "topic:ledger").len(), 2);
    let rows = ledger(&r.core).len();
    let u = run(&r.core, undo(false)).await;
    assert!(u.undo_declared, "{u:?}");
    assert_eq!(
        (u.declared_rows, u.undone, u.ended, u.asked, u.filed),
        (1, 1, 1, 0, 0),
        "{u:?}"
    );
    let [t] = timeline(&r.core, "topic:ledger")
        .try_into()
        .unwrap_or_else(|v| panic!("his own touch alone: {v:?}"));
    assert_eq!(t.exchange, "turn_his");
    assert_eq!(ledger(&r.core).len(), rows, "the undo writes no row");
    assert_eq!(
        of_kind(&r.core, LedgerKind::JudgeCall).len(),
        2,
        "the rows stay"
    );
    assert_eq!(r.jev.seen().len(), 2, "no call");
}

/// KEY (the undo holds, and twice is harmless): after it, a run without
/// the declaration asks nothing and writes none of its touches again; a
/// second undo ends nothing; a run with the declaration writes them again
/// from the rows, with no call.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_run_after_undo_without_the_flag_touches_nothing() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    unmarked_in(&r.core, (&sid, "turn_dm"), SAID, HIS_DM, 5_000);
    run(&r.core, declared(false)).await;
    let u = run(&r.core, undo(false)).await;
    assert_eq!(u.ended, 1, "{u:?}");
    let plain = run(&r.core, params(false, None)).await;
    assert_eq!((plain.asked, plain.touches), (0, 0), "{plain:?}");
    assert!(timeline(&r.core, "topic:ledger").is_empty());
    let twice = run(&r.core, undo(false)).await;
    assert_eq!(
        (
            twice.undo_declared,
            twice.declared_rows,
            twice.undone,
            twice.ended
        ),
        (true, 1, 0, 0),
        "{twice:?}"
    );
    let again = run(&r.core, declared(false)).await;
    assert_eq!(
        (again.asked_before, again.asked, again.filed),
        (1, 0, 1),
        "{again:?}"
    );
    assert_eq!(timeline(&r.core, "topic:ledger").len(), 1);
    assert_eq!(r.jev.seen().len(), 1, "the first run's call alone");
}

/// KEY: the undo's dry run counts what a run would end, and ends nothing;
/// an undo that also carries the declaration is refused and does neither.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_undo_dry_run_counts_and_writes_nothing() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    unmarked_in(&r.core, (&sid, "turn_dm"), SAID, HIS_DM, 5_000);
    run(&r.core, declared(false)).await;
    let rows = ledger(&r.core).len();
    let d = run(&r.core, undo(true)).await;
    assert_eq!(
        (
            d.dry_run,
            d.undo_declared,
            d.declared_rows,
            d.undone,
            d.ended
        ),
        (true, true, 1, 1, 1),
        "{d:?}"
    );
    assert_eq!(timeline(&r.core, "topic:ledger").len(), 1, "nothing ended");
    assert_eq!(ledger(&r.core).len(), rows);
    let both = ExchangeTopicsParams {
        include_unmarked: true,
        ..undo(false)
    };
    let e = r.core.topics_history(&both).await.unwrap_err();
    assert!(e.to_string().contains("--undo-declared"), "{e}");
    assert_eq!(timeline(&r.core, "topic:ledger").len(), 1, "nothing ended");
    assert_eq!(r.jev.seen().len(), 1, "no call");
}

/// KEY: a declared touch the rebuild moved (its key at a new time), and one
/// of a topic retired since, are ended all the same: the undo finds a touch
/// by its exchange and the category the answer chose.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_undo_ends_a_moved_touch_of_a_retired_topic() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    unmarked_in(&r.core, (&sid, "turn_dm"), SAID, HIS_DM, 5_000);
    run(&r.core, declared(false)).await;
    untouch(&r.core, "turn_dm");
    let moved = exchange::touch(
        exchange::Root::Thing("topic:ledger"),
        "turn_dm",
        &sid,
        7_000,
    );
    r.core.store.append(&[moved.unwrap()]).unwrap();
    let topic = theseus_ontology::CategoryId::parse("topic:ledger").unwrap();
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    let retired = o.category(&topic).unwrap().retired(9_000);
    (r.core.runner.ontology)
        .write(
            &r.core.store,
            vec![theseus_ontology::Record::Category(retired)],
            theseus_ontology::Origin::Operator,
            |_| Ok(vec![]),
        )
        .unwrap();
    assert_eq!(timeline(&r.core, "topic:ledger").len(), 1);
    let u = run(&r.core, undo(false)).await;
    assert_eq!((u.undone, u.ended), (1, 1), "{u:?}");
    assert!(timeline(&r.core, "topic:ledger").is_empty());
}

/// KEY (his decision: his own terminal only): the undo is refused from his
/// web UI, an agent's process at his terminal, Discord, MCP and an unnamed
/// connection, a dry run and a run, in words, and ends nothing; each is
/// ledgered but MCP's. At his own terminal, in person, it ends his
/// declaration's touch.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_agents_process_cannot_undo() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    unmarked_in(&r.core, (&sid, "turn_dm"), SAID, HIS_DM, 5_000);
    run(&r.core, declared(false)).await;
    let callers = [
        ("the owner's web UI", Client::new("ws#1", Surface::Web)),
        (
            "an agent at the terminal",
            surface("sock#2", Surface::Cli).behind(Behind::Agent("claude".into())),
        ),
        ("discord", Client::new("discord", Surface::Discord)),
        ("mcp", Client::new("mcp", Surface::Mcp)),
        (
            "an unnamed connection",
            Client::new("conn#9", Surface::Unnamed),
        ),
    ];
    let asks = [
        json!({"dry_run": true, "undo_declared": true}),
        json!({"dry_run": false, "undo_declared": true}),
    ];
    for (who, client) in callers {
        for p in &asks {
            let e = rpc_as(&r.core, client.clone(), method::EXCHANGE_TOPICS, p.clone())
                .await
                .expect_err(who);
            assert_eq!(e.code, error_code::REFUSED, "{who}: {e:?}");
            let said = match who {
                "mcp" => "may not call exchange.topics",
                _ => "Nothing was asked.",
            };
            assert!(e.message.ends_with(said), "{who}: {e:?}");
        }
    }
    let refused = of_kind(&r.core, LedgerKind::ApprovalRefused);
    assert_eq!(
        refused.len(),
        8,
        "each refusal ledgered, MCP's apart: {refused:?}"
    );
    assert_eq!(timeline(&r.core, "topic:ledger").len(), 1, "nothing ended");
    let v = rpc_as(
        &r.core,
        surface("sock#1", Surface::Cli),
        method::EXCHANGE_TOPICS,
        asks[1].clone(),
    )
    .await
    .unwrap();
    assert_eq!(
        (v["undo_declared"].clone(), v["ended"].clone()),
        (json!(true), json!(1)),
        "{v}"
    );
    assert!(timeline(&r.core, "topic:ledger").is_empty());
}

/// A conversation bound to the DM of Discord user `user`.
fn dm_of(core: &Core, user: &str) -> String {
    let rec = crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    core.outbox
        .bind_place(&format!("dm:{user}"), &rec.session_id)
        .unwrap();
    core.runner.place_rule.bind_one(crate::places::BoundPlace {
        target: format!("discord:dm:{user}"),
        name: "DM".into(),
        private: false,
        ..Default::default()
    });
    rec.session_id
}

/// KEY (fail closed, the review's A2-1): the code `discord:dm` is written
/// for every bound DM, so it is no proof the DM was his. With an owner set
/// (`[places] owner`) that holds only his own DM's person, a bound guest's DM in the shape
/// history before marks has (the rebuild's place `unknown`), and one whose
/// recorded place is the guest's DM, are counted apart, never asked and
/// never written; his own DM, by its recorded place, is.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dm_no_stored_record_shows_to_be_his_is_not_declared() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let owner = crate::tests_places::OWNER.to_string();
    let r = crate::tests_onto::rig_with(jev, Vec::new(), |c| {
        c.places.owner = Some(vec![format!("discord:{}", crate::tests_places::OWNER)]);
    });
    ledger_topic(&r.core);
    let sid = dm_of(&r.core, &owner);
    let guest = dm_of(&r.core, "77");
    let words = ["Wrenquist", "Haddleby"];
    unmarked(
        &r.core,
        (&guest, "turn_rebuilt"),
        &format!("ask {} now", words[0]),
        Some("discord:dm"),
        1_000,
    );
    unmarked_in(
        &r.core,
        (&guest, "turn_guest_dm"),
        &format!("ask {} now", words[1]),
        (Some("discord:dm"), "discord:dm:77"),
        2_000,
    );
    let his = format!("discord:dm:{owner}");
    unmarked_in(
        &r.core,
        (&sid, "turn_his_dm"),
        SAID,
        (Some("discord:dm"), &his),
        3_000,
    );
    let dry = run(&r.core, declared(true)).await;
    assert_eq!(
        (dry.unproven, dry.declared, dry.unshown_dm, dry.to_ask),
        (3, 1, 2, 1),
        "{dry:?}"
    );
    let out = run(&r.core, declared(false)).await;
    assert_eq!((out.asked, out.filed, out.touches), (1, 1, 1), "{out:?}");
    let seen = sent(&r.jev);
    for w in words {
        assert!(!seen.contains(w), "{w} was sent");
    }
    let touched: Vec<String> = timeline(&r.core, "topic:ledger")
        .into_iter()
        .map(|t| t.exchange)
        .collect();
    assert_eq!(touched, vec!["turn_his_dm".to_string()]);
    let calls = of_kind(&r.core, LedgerKind::JudgeCall);
    let [call] = calls.as_slice() else {
        panic!("one judgment: {calls:?}");
    };
    assert_eq!(call.data["context"]["turn"], "turn_his_dm");
}

/// KEY (the review's A2-2): a declared run stopped at its cap says to go on
/// with the declaration; a plain one, as before, without it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_declared_run_stopped_at_its_cap_goes_on_with_the_declaration() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    unmarked_in(&r.core, (&sid, "turn_dm"), SAID, HIS_DM, 5_000);
    said(&r.core, &sid, "turn_his", SAID, 6_000);
    let tiny = Some(0.000_001);
    let declared_run = ExchangeTopicsParams {
        include_unmarked: true,
        ..params(false, tiny)
    };
    let out = run(&r.core, declared_run).await;
    let why = out.stopped.as_deref().unwrap_or_default();
    assert!(why.starts_with("stopped at the cap"), "{out:?}");
    assert!(
        why.ends_with("Go on with: theseus ledger topics --include-unmarked --cap-usd <dollars>"),
        "{why}"
    );
    let plain = run(&r.core, params(false, tiny)).await;
    let why = plain.stopped.as_deref().unwrap_or_default();
    assert!(
        why.ends_with("Go on with: theseus ledger topics --cap-usd <dollars>"),
        "{why}"
    );
    assert!(r.jev.seen().is_empty(), "nothing asked");
}

/// KEY (the review's B-1): a declared answer's row is durable before any
/// touch of it. The sink's frame holding the row fails once (it is retried
/// later, as a full disk's would be) and the run's wait for it runs out:
/// the run stops with no touch written, so nothing is left that the undo
/// cannot find. Once the row is written, the undo finds it and ends
/// nothing; a declared rerun writes the touch from it, with no call, and
/// the undo then ends that touch.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_declared_answer_is_written_before_its_touch() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    let bounds = (
        std::time::Duration::from_millis(300),
        std::time::Duration::from_secs(20),
    );
    r.core.runner.judge.history_bounds.set(bounds).unwrap();
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    unmarked_in(&r.core, (&sid, "turn_dm"), SAID, HIS_DM, 5_000);
    r.core.store.fail_plain_frame(a_judgments_row);
    let first = r.core.topics_history(&declared(false)).await.unwrap();
    let why = first.stopped.as_deref().unwrap_or_default();
    assert!(why.contains("was not written within"), "{first:?}");
    assert_eq!((first.asked, first.touches), (1, 0), "{first:?}");
    assert!(
        timeline(&r.core, "topic:ledger").is_empty(),
        "no touch before its row"
    );
    rows_written(&r.core, 1).await;
    let u = run(&r.core, undo(false)).await;
    assert_eq!((u.declared_rows, u.undone, u.ended), (1, 0, 0), "{u:?}");
    let again = run(&r.core, declared(false)).await;
    assert_eq!(
        (again.asked_before, again.asked, again.touches),
        (1, 0, 1),
        "{again:?}"
    );
    assert_eq!(r.jev.seen().len(), 1, "no second call");
    let u = run(&r.core, undo(false)).await;
    assert_eq!(u.ended, 1, "{u:?}");
    assert!(timeline(&r.core, "topic:ledger").is_empty());
}
