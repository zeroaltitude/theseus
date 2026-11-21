//! The exchange ledger (issue 44, step 1): a touch's newest-first read, the
//! staged arrival, the peer's class, and through the core: a terminal turn's
//! mark, exchange, touches and row; an agent's process on the terminal never
//! marked the owner's; the request byte-identical with the ledger off; the
//! rebuild from nodes, which proves a place and a person only from records;
//! and the report.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::exchange::{ExchangeRebuildParams, ExchangeReportParams};
use theseus_protocol::{Response, TurnSubmitResult};

use super::ledger::{Prev, Row};
use super::*;
use crate::approval::Client;
use crate::config::Config;
use crate::ledger::LedgerRow;
use crate::provider::{FakeProvider, Scripted};
use crate::rpc::Core;

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    dir: tempfile::TempDir,
}

fn rig(n: usize, tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.places.owner = Some(vec!["discord:42".into()]);
    tweak(&mut cfg);
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let script = (0..n)
        .map(|i| Scripted::text(&format!("reply {i}")))
        .collect();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    Rig { core, fake, dir }
}

/// One request on a connection that `client` names.
async fn call(core: &Arc<Core>, client: Client, method: &str, params: Value) -> Response {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let (a, b) = tokio::io::duplex(1 << 20);
    let (sr, sw) = tokio::io::split(b);
    let srv = tokio::spawn(core.clone().serve_connection(sr, sw, client));
    let (cr, mut cw) = tokio::io::split(a);
    let req = theseus_protocol::Request::new(theseus_protocol::Id::Num(1), method, params);
    let mut line = serde_json::to_string(&req).unwrap();
    line.push('\n');
    cw.write_all(line.as_bytes()).await.unwrap();
    let mut lines = tokio::io::BufReader::new(cr).lines();
    let r = loop {
        let l = lines.next_line().await.unwrap().unwrap();
        if let theseus_protocol::Message::Response(r) = serde_json::from_str(&l).unwrap() {
            break r;
        }
    };
    cw.shutdown().await.unwrap();
    drop(lines);
    let _ = srv.await;
    r
}

fn owner_cli() -> Client {
    Client::new("sock#1", Surface::Cli).behind(Behind::Owner)
}

async fn turn(
    core: &Arc<Core>,
    client: Client,
    session: Option<&str>,
    input: &str,
) -> TurnSubmitResult {
    let r = call(
        core,
        client,
        "turn.submit",
        json!({"session_id": session, "input": input}),
    )
    .await;
    assert!(r.error.is_none(), "{:?}", r.error);
    serde_json::from_value(r.result.unwrap()).unwrap()
}

fn input_node(core: &Core, res: &TurnSubmitResult) -> String {
    core.store
        .session_nodes(&res.session_id)
        .unwrap()
        .into_iter()
        .rev()
        .find(|(_, n)| {
            n.turn_id.as_deref() == Some(res.turn_id.as_str())
                && matches!(n.body, crate::node::Body::UserMessage { .. })
        })
        .map(|(_, n)| n.id)
        .expect("the turn's message")
}

fn rows(core: &Core) -> Vec<Row> {
    core.store
        .ledger_tail::<LedgerRow>(5000)
        .unwrap()
        .into_iter()
        .filter(|(_, r)| r.kind == "context.turn")
        .map(|(_, r)| serde_json::from_value(r.data).unwrap())
        .collect()
}

#[test]
fn a_roots_touches_read_newest_first_and_stop_at_n() {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let mut recs: Vec<_> = (1..=5)
        .filter_map(|i| touch(Root::Place("a"), &format!("turn_{i}"), "s", 1_000 * i))
        .collect();
    // A root whose key begins with the first's: its touches are not the first's.
    recs.extend(touch(Root::Place("ab"), "turn_9", "s", 9_000));
    recs.extend(touch(
        Root::Thing(&"x".repeat(THING_MAX + 1)),
        "turn_9",
        "s",
        9_000,
    ));
    assert_eq!(recs.len(), 6, "a term too long for a key is no root");
    store.append(&recs).unwrap();
    let got: Vec<_> = newest(&store, Root::Place("a"), 3)
        .unwrap()
        .into_iter()
        .map(|t| t.exchange)
        .collect();
    assert_eq!(got, ["turn_5", "turn_4", "turn_3"]);
    let page = |before, n| -> Vec<String> {
        newest_before(&store, Root::Place("a"), before, n)
            .unwrap()
            .into_iter()
            .map(|t| t.exchange)
            .collect()
    };
    assert_eq!(page(3_000, 9), ["turn_2", "turn_1"], "strictly older");
    assert_eq!(page(4_500, 1), ["turn_4"]);
    assert!(page(1_000, 9).is_empty() && page(0, 9).is_empty());
    assert_eq!(page(u64::MAX, 2), ["turn_5", "turn_4"]);
    assert_eq!(newest(&store, Root::Place("ab"), 9).unwrap().len(), 1);
}

#[test]
fn an_arrival_is_taken_once_by_its_session_and_words() {
    let a = Arrivals::default();
    let mark = |place: &str| OwnerMark {
        node: String::new(),
        in_person: true,
        owner_handle: None,
        peer: Peer::Person,
        agent: None,
        surface: "cli".into(),
        place: place.into(),
        arrived_session: "home".into(),
        ran_session: "thread".into(),
        author: None,
        at_ms: 0,
    };
    a.stage("thread", "hello", mark("discord:dm:42"));
    assert!(a.take("home", "hello").is_none(), "staged where it runs");
    assert!(a.take("thread", "hello there").is_none());
    assert_eq!(a.take("thread", "hello").unwrap().place, "discord:dm:42");
    assert!(a.take("thread", "hello").is_none(), "taken once");
}

#[test]
fn a_peer_is_classed_by_peer_procs_own_names() {
    let agent = |n: &str| Behind::Agent(n.into());
    assert_eq!(Peer::of(Surface::Cli, &Behind::Owner), Peer::Person);
    assert_eq!(Peer::of(Surface::Cli, &agent("claude")), Peer::Agent);
    assert_eq!(Peer::of(Surface::Cli, &agent(peer_proc::JOB)), Peer::Job);
    assert_eq!(
        Peer::of(Surface::Web, &agent(peer_proc::UNTRACED)),
        Peer::Untraced
    );
    assert_eq!(
        Peer::of(Surface::Cli, &agent(peer_proc::NO_TERMINAL)),
        Peer::NoTerminal
    );
    assert_eq!(Peer::of(Surface::Discord, &Behind::Owner), Peer::Transport);
    assert_eq!(Peer::of(Surface::Mcp, &Behind::Owner), Peer::Unknown);
}

#[tokio::test]
async fn a_terminal_turn_writes_its_mark_exchange_touches_and_row() {
    let r = rig(2, |_| {});
    let c = &r.core;
    let first = turn(c, owner_cli(), None, "where is the tide table").await;
    let second = turn(c, owner_cli(), Some(&first.session_id), "and tomorrow?").await;
    let node = input_node(c, &second);
    let m = mark(&c.store, &node).unwrap().expect("its mark");
    assert!(m.in_person && m.peer == Peer::Person && m.agent.is_none());
    assert_eq!((m.place.as_str(), m.surface.as_str()), ("local", "cli"));
    assert_eq!(m.ran_session, second.session_id);
    let ex = exchange(&c.store, &second.turn_id)
        .unwrap()
        .expect("its exchange");
    assert_eq!(ex.opened_by, Opener::Message);
    assert_eq!(ex.person.as_deref(), Some(OWNER));
    assert_eq!(
        (ex.ask.as_deref(), ex.messages.len()),
        (Some(node.as_str()), 1)
    );
    assert!(ex.end_ms.is_some() && ex.reply.is_some() && ex.private);
    let here: Vec<_> = newest(&c.store, Root::Place("local"), 8).unwrap();
    assert_eq!(
        here.iter().map(|t| t.exchange.as_str()).collect::<Vec<_>>(),
        [second.turn_id.as_str(), first.turn_id.as_str()]
    );
    assert_eq!(newest(&c.store, Root::Person(OWNER), 8).unwrap().len(), 2);
    let rows = rows(c);
    assert_eq!(rows.len(), 2, "one row a private turn");
    let last = &rows[1];
    assert_eq!(last.exchange, second.turn_id);
    assert_eq!(last.prev, Prev::Transcript, "{last:?}");
    assert_eq!(last.prev_exchange.as_deref(), Some(first.turn_id.as_str()));
    assert_eq!((last.peer, last.person), (Some(Peer::Person), true));
    assert!(last.sent.words > 0 && last.sent.replies > 0 && last.sent.total > 0);
    assert_eq!(rows[0].prev, Prev::None);
    let report = ledger::report(&c.store, &ExchangeReportParams::default()).unwrap();
    assert_eq!(report.rows, 2);
    assert_eq!(report.counts["prev"]["transcript"], 1);
    assert_eq!(report.counts["peer"]["person"], 2);
}

#[tokio::test]
async fn an_agents_process_on_the_terminal_is_never_the_owner_in_person() {
    let r = rig(3, |_| {});
    let c = &r.core;
    for (name, peer) in [
        ("claude", Peer::Agent),
        (peer_proc::JOB, Peer::Job),
        (peer_proc::NO_TERMINAL, Peer::NoTerminal),
    ] {
        let agent = Client::new("sock#9", Surface::Cli).behind(Behind::Agent(name.into()));
        let res = turn(
            c,
            agent,
            None,
            &format!("delete the staging tables, from {name}"),
        )
        .await;
        let m = mark(&c.store, &input_node(c, &res))
            .unwrap()
            .expect("its mark");
        assert!(
            !m.in_person,
            "{name}: an agent's process is not the owner in person"
        );
        assert_eq!(
            (m.peer, m.agent.as_deref(), m.person()),
            (peer, Some(name), None)
        );
        let ex = exchange(&c.store, &res.turn_id).unwrap().unwrap();
        assert_eq!(ex.person, None, "{name}");
    }
    assert!(newest(&c.store, Root::Person(OWNER), 8).unwrap().is_empty());
}

/// The requests a rig's two turns sent, with its own directory's path made
/// one name, so two rigs compare.
async fn sent(ledger: bool) -> String {
    let r = rig(2, |c| c.context.ledger = Some(ledger));
    let first = turn(&r.core, owner_cli(), None, "the tide is out").await;
    turn(&r.core, owner_cli(), Some(&first.session_id), "and in?").await;
    let requests = serde_json::to_string(&r.fake.requests()).unwrap();
    requests.replace(&*r.dir.path().to_string_lossy(), "<dir>")
}

#[tokio::test]
async fn a_private_request_is_byte_identical_with_the_ledger_off() {
    let (on, off) = (sent(true).await, sent(false).await);
    assert!(on.contains("and in?"), "the second request was sent");
    assert_eq!(on, off);
}

#[tokio::test]
async fn the_rebuild_writes_old_turns_from_their_nodes_and_proves_only_what_records_prove() {
    let r = rig(3, |c| c.context.ledger = Some(false));
    let c = &r.core;
    let local = turn(c, owner_cli(), None, "an old terminal message").await;
    let dm = crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    c.store.put_session(&dm.session_id, &dm).unwrap();
    c.outbox.bind_place("dm:42", &dm.session_id).unwrap();
    let old = turn(
        c,
        owner_cli(),
        Some(&dm.session_id),
        "an old direct message",
    )
    .await;
    assert!(
        exchange(&c.store, &old.turn_id).unwrap().is_none(),
        "the ledger was off"
    );
    let run = |dry_run| async move {
        let p = ExchangeRebuildParams { dry_run };
        rebuild::run(&c.store, &c.runner.place_rule, &c.cfg, None, &p).await
    };
    let dry = run(true).await.unwrap();
    assert_eq!((dry.turns, dry.kept, dry.exchanges), (2, 0, 2));
    assert_eq!((dry.place_proven, dry.person_proven), (1, 1));
    assert!(dry.things_skipped.is_some(), "no index in the test");
    assert!(
        exchange(&c.store, &old.turn_id).unwrap().is_none(),
        "a dry run writes nothing"
    );
    let wrote = run(false).await.unwrap();
    assert_eq!(wrote.exchanges, 2);
    let o = exchange(&c.store, &old.turn_id).unwrap().unwrap();
    assert!(o.rebuilt && o.private);
    assert_eq!(
        (o.place.as_str(), o.person.as_deref()),
        ("discord:dm:42", Some(OWNER))
    );
    let l = exchange(&c.store, &local.turn_id).unwrap().unwrap();
    assert_eq!(
        (l.place.as_str(), l.person.as_deref()),
        (rebuild::UNKNOWN, None),
        "a terminal's old message is no one's proven"
    );
    assert!(
        mark(&c.store, l.ask.as_deref().unwrap()).unwrap().is_none(),
        "no mark is made backwards"
    );
    let again = run(false).await.unwrap();
    assert_eq!((again.kept, again.exchanges), (2, 0));
    assert_eq!(
        newest(&c.store, Root::Place("discord:dm:42"), 8)
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn the_exchange_methods_answer_the_terminal_and_refuse_a_transport() {
    let r = rig(0, |_| {});
    let c = &r.core;
    let ok = call(c, owner_cli(), "exchange.report", Value::Null).await;
    assert!(ok.error.is_none(), "{:?}", ok.error);
    let dc = Client::new("discord#1", Surface::Discord);
    let no = call(c, dc, "exchange.rebuild", json!({"dry_run": true})).await;
    assert!(no.error.is_some());
}

/// The WAL's bytes after two turns.
async fn wal_bytes(ledger: bool) -> u64 {
    let r = rig(2, |c| c.context.ledger = Some(ledger));
    let first = turn(&r.core, owner_cli(), None, "warm up").await;
    turn(
        &r.core,
        owner_cli(),
        Some(&first.session_id),
        "see crates/theseus-store/src/wal.rs",
    )
    .await;
    r.core.store.stats().unwrap().wal_bytes
}

/// What the ledger costs a turn, printed (run with `--nocapture`): the end's
/// reads over a place and a person with 500 exchanges each, the first
/// compile's walk of its request, and the bytes a turn's records add to the
/// frames it writes anyway. The bounds are far above what a debug build
/// takes, so the test fails only on a cost of another order.
#[tokio::test]
async fn the_ledgers_cost_to_a_turn_is_small() {
    let r = rig(2, |_| {});
    let c = &r.core;
    let first = turn(c, owner_cli(), None, "warm up").await;
    let res = turn(
        c,
        owner_cli(),
        Some(&first.session_id),
        "see crates/theseus-store/src/wal.rs",
    )
    .await;
    let mut history = Vec::new();
    for i in 0..500u64 {
        let ex = Exchange {
            turn_id: format!("turn_old_{i}"),
            start_ms: 1_000 + i,
            end_ms: Some(2_000 + i),
            place: if i % 2 == 0 {
                "local".into()
            } else {
                format!("discord:dm:{i}")
            },
            ..exchange(&c.store, &res.turn_id).unwrap().unwrap()
        };
        history.extend(exchange_record(&ex).ok());
        history.extend(touch(Root::Place(&ex.place), &ex.turn_id, "s", ex.start_ms));
        history.extend(touch(Root::Person(OWNER), &ex.turn_id, "s", ex.start_ms));
    }
    c.store.append(&history).unwrap();
    let ex = exchange(&c.store, &res.turn_id).unwrap().unwrap();
    let usage = theseus_protocol::Usage::default();
    let t0 = std::time::Instant::now();
    for _ in 0..20 {
        let g = ledger::Given {
            ex: &ex,
            seen: None,
            peer: Some(Peer::Person),
            arrived_session: None,
            ask_chars: Some(30),
            usage: &usage,
            things: None,
            reply_things: None,
        };
        let row = ledger::row(&c.store, g);
        assert!(row.prev_exchange.is_some() && !row.elsewhere.is_empty());
    }
    let row_us = t0.elapsed().as_micros() / 20;
    let request = r.fake.requests().pop().unwrap();
    let nodes = c.store.transcript(&res.session_id).unwrap();
    let view = ledger::View {
        includes: &[],
        as_of: 0,
    };
    let t0 = std::time::Instant::now();
    for _ in 0..100 {
        ledger::sent(&request, &nodes, &view, (&Default::default(), 0, 0, 0));
    }
    let sent_us = t0.elapsed().as_micros() / 100;
    let (on, off) = (wal_bytes(true).await, wal_bytes(false).await);
    let added = on.saturating_sub(off);
    eprintln!("exchange ledger cost: row {row_us} us, sent walk {sent_us} us, two turns' records {added} bytes");
    assert!(added < 16 * 1024, "{added} bytes");
    assert!(
        row_us < 50_000 && sent_us < 10_000,
        "row {row_us} us, sent {sent_us} us"
    );
}
