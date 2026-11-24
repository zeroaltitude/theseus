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
    /// Held for the rig's life: the store lives in it.
    _dir: tempfile::TempDir,
}

fn rig(n: usize, tweak: impl FnOnce(&mut Config)) -> Rig {
    let replies: Vec<String> = (0..n).map(|i| format!("reply {i}")).collect();
    rig_saying(
        &replies.iter().map(String::as_str).collect::<Vec<_>>(),
        tweak,
    )
}

/// A rig whose model answers `replies`, in order.
fn rig_saying(replies: &[&str], tweak: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::example();
    cfg.server.state_dir = dir.path().to_string_lossy().into_owned();
    cfg.places.owner = Some(vec!["discord:42".into()]);
    tweak(&mut cfg);
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let script = replies.iter().map(|r| Scripted::text(r)).collect();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts::for_tests(cfg, fake.clone(), store)).unwrap();
    Rig {
        core,
        fake,
        _dir: dir,
    }
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

/// `rebuild::run` on the rig's store, with `index`.
async fn rebuilt(
    c: &Arc<Core>,
    index: Option<Arc<dyn crate::memory_pass::PassIndex>>,
    dry_run: bool,
) -> theseus_protocol::exchange::ExchangeRebuildResult {
    let p = ExchangeRebuildParams { dry_run };
    rebuild::run(&c.store, &c.runner.place_rule, &c.cfg, index, &p)
        .await
        .unwrap()
}

/// History in a DM bound to the owner, which the terminal and an agent's
/// process wrote into, is no one's: a binding is where a session posts now,
/// not proof of who typed an old message or where it arrived.
#[tokio::test]
async fn the_rebuild_never_takes_an_author_or_a_place_from_a_binding() {
    let r = rig(3, |_| {});
    let c = &r.core;
    let local = turn(c, owner_cli(), None, "an old terminal message").await;
    let dm = crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    c.store.put_session(&dm.session_id, &dm).unwrap();
    c.outbox.bind_place("dm:42", &dm.session_id).unwrap();
    let by_owner = turn(
        c,
        owner_cli(),
        Some(&dm.session_id),
        "an old direct message",
    )
    .await;
    let agent = Client::new("sock#9", Surface::Cli).behind(Behind::Agent("claude".into()));
    let by_agent = turn(c, agent, Some(&dm.session_id), "an agent's old message").await;
    for s in [&local.session_id, &dm.session_id] {
        crate::test_support::before_the_ledger(&c.store, s);
    }
    assert!(
        exchange(&c.store, &by_owner.turn_id).unwrap().is_none(),
        "the ledger was off"
    );
    let dry = rebuilt(c, None, true).await;
    assert_eq!((dry.turns, dry.kept, dry.exchanges), (3, 0, 3));
    assert_eq!((dry.place_proven, dry.person_proven), (0, 0));
    assert!(dry.things_skipped.is_some(), "no index in the test");
    assert!(
        exchange(&c.store, &by_owner.turn_id).unwrap().is_none(),
        "a dry run writes nothing"
    );
    assert_eq!(rebuilt(c, None, false).await.exchanges, 3);
    for res in [&local, &by_owner, &by_agent] {
        let e = exchange(&c.store, &res.turn_id).unwrap().unwrap();
        assert!(e.rebuilt);
        assert_eq!(
            (e.place.as_str(), e.person.as_deref()),
            (rebuild::UNKNOWN, None),
            "{e:?}"
        );
        assert!(
            mark(&c.store, e.ask.as_deref().unwrap()).unwrap().is_none(),
            "no mark is made backwards"
        );
    }
    assert!(newest(&c.store, Root::Person(OWNER), 8).unwrap().is_empty());
    let again = rebuilt(c, None, false).await;
    assert_eq!((again.kept, again.exchanges, again.touches), (3, 0, 0));
}

/// The index of the tests: `file:wal.rs` for a text that names it, after
/// `delay`.
struct Naming {
    delay: std::time::Duration,
}

impl crate::memory_pass::PassIndex for Naming {
    fn neighbours(
        &self,
        _: theseus_protocol::index::IndexNeighboursParams,
    ) -> crate::memory_pass::IndexFuture<theseus_protocol::index::IndexNeighboursResult> {
        Box::pin(async { Err(crate::tender::TenderMiss::Down("none".into())) })
    }

    fn entities(&self, texts: Vec<String>) -> crate::memory_pass::IndexFuture<Vec<Vec<String>>> {
        let delay = self.delay;
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            Ok(texts
                .iter()
                .map(|t| match t.contains("wal.rs") {
                    true => vec!["file:wal.rs".to_string()],
                    false => vec![],
                })
                .collect())
        })
    }

    fn mode(&self) -> crate::memory_pass::IndexFuture<String> {
        Box::pin(async { Ok("bm25_only".into()) })
    }
}

fn wal_rs() -> Root<'static> {
    Root::Thing("file:wal.rs")
}

/// The touches of `root`, once `n` are in, within five seconds.
async fn touches_in(c: &Core, root: Root<'_>, n: usize) -> Vec<Touch> {
    for _ in 0..50 {
        let got = newest(&c.store, root, 8).unwrap();
        if got.len() >= n {
            return got;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    newest(&c.store, root, 8).unwrap()
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

/// The bytes of the ledger's own records after two turns: each message's mark, each turn's exchange and
/// its touches, keys included.
async fn ledger_bytes() -> u64 {
    use theseus_store::Store as _;
    let r = rig(2, |_| {});
    let first = turn(&r.core, owner_cli(), None, "warm up").await;
    turn(
        &r.core,
        owner_cli(),
        Some(&first.session_id),
        "see crates/theseus-store/src/wal.rs",
    )
    .await;
    let inner = r.core.store.inner();
    [SAID_PREFIX, EXCHANGE_PREFIX, TOUCH_PREFIX]
        .iter()
        .flat_map(|p| {
            inner
                .latest_with_prefix(theseus_store::kinds::META, p)
                .unwrap()
        })
        .map(|rec| (rec.payload.len() + rec.key.map_or(0, |k| k.len())) as u64)
        .sum()
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
    let added = ledger_bytes().await;
    eprintln!("exchange ledger cost: row {row_us} us, sent walk {sent_us} us, two turns' records {added} bytes");
    assert!(added < 16 * 1024, "{added} bytes");
    assert!(
        row_us < 50_000 && sent_us < 10_000,
        "row {row_us} us, sent {sent_us} us"
    );
}

/// The reviewer's sequence: the owner's submit of X to S is refused before
/// its node is written (an unknown profile); an agent then submits X to S.
/// The agent's node carries the agent's own mark, and no mark of the owner's
/// is left anywhere for a later message (a wake's, another's) to take.
#[tokio::test]
async fn a_refused_owners_submit_leaves_no_mark_for_a_later_identical_message() {
    let r = rig(3, |_| {});
    let c = &r.core;
    let first = turn(c, owner_cli(), None, "warm up").await;
    let s = first.session_id.as_str();
    let refused = call(
        c,
        owner_cli(),
        "turn.submit",
        json!({"session_id": s, "input": "drop the tables", "profile": "nonesuch"}),
    )
    .await;
    assert!(refused.error.is_some(), "the owner's submit is refused");
    let agent = Client::new("sock#9", Surface::Cli).behind(Behind::Agent("claude".into()));
    let res = turn(c, agent, Some(s), "drop the tables").await;
    let m = mark(&c.store, &input_node(c, &res)).unwrap().unwrap();
    assert!(!m.in_person && m.peer == Peer::Agent, "{m:?}");
    let agent2 = Client::new("sock#9", Surface::Cli).behind(Behind::Agent(peer_proc::JOB.into()));
    turn(c, agent2, Some(s), "another message").await;
    let in_person: Vec<OwnerMark> = c
        .store
        .inner()
        .latest_with_prefix(theseus_store::kinds::META, SAID_PREFIX)
        .unwrap()
        .iter()
        .map(|r| r.decode().unwrap())
        .filter(|m: &OwnerMark| m.in_person)
        .collect();
    assert_eq!(
        in_person.len(),
        1,
        "only the warm-up was the owner's: {in_person:?}"
    );
}

/// The owner and an agent submit the same words to one session at once:
/// each node carries the mark of the submission that made it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn identical_words_from_two_peers_at_once_keep_their_own_marks() {
    let r = rig(4, |_| {});
    let c = &r.core;
    let first = turn(c, owner_cli(), None, "warm up").await;
    let s = first.session_id.as_str();
    let agent = Client::new("sock#9", Surface::Cli).behind(Behind::Agent("claude".into()));
    let (a, b) = tokio::join!(
        turn(c, owner_cli(), Some(s), "ship it"),
        turn(c, agent, Some(s), "ship it"),
    );
    assert!(!a.turn_id.is_empty() && !b.turn_id.is_empty());
    let nodes: Vec<_> = c
        .store
        .session_nodes(s)
        .unwrap()
        .into_iter()
        .filter(|(_, n)| matches!(&n.body, crate::node::Body::UserMessage { text, .. } if text == "ship it"))
        .collect();
    assert_eq!(nodes.len(), 2);
    for (_, n) in nodes {
        let m = mark(&c.store, &n.id).unwrap().expect("its mark");
        let by_agent = n.author.as_deref() == Some("sock#9");
        assert_eq!(
            (m.in_person, m.agent.is_some()),
            (!by_agent, by_agent),
            "{n:?} {m:?}"
        );
    }
}

/// A slow index: the turn ends before its things are named, and the last
/// reply has no turn after it; the touches are written anyway. The ask and
/// the reply both name one thing: one touch, at the exchange's start, and
/// `newest_before` pages it once.
#[tokio::test]
async fn a_slow_index_still_touches_and_an_ask_and_reply_make_one_touch() {
    let r = rig_saying(&["wal.rs is fine"], |_| {});
    let c = &r.core;
    let slow = std::time::Duration::from_millis(400);
    c.runner.pass.set_index(Arc::new(Naming { delay: slow }));
    let res = turn(c, owner_cli(), None, "look at crates/x/src/wal.rs").await;
    assert!(
        newest(&c.store, wal_rs(), 8).unwrap().is_empty(),
        "not named by the turn's end"
    );
    let got = touches_in(c, wal_rs(), 1).await;
    tokio::time::sleep(slow * 3).await;
    let got_after = newest(&c.store, wal_rs(), 8).unwrap();
    let ex = exchange(&c.store, &res.turn_id).unwrap().unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got_after, got, "the reply's naming made no second touch");
    assert_eq!(
        (got[0].exchange.as_str(), got[0].at_ms),
        (res.turn_id.as_str(), ex.start_ms)
    );
    let page = newest_before(&c.store, wal_rs(), ex.start_ms + 1, 8).unwrap();
    assert_eq!(page, got);
    assert!(newest_before(&c.store, wal_rs(), ex.start_ms, 8)
        .unwrap()
        .is_empty());
}

/// A restart, or an index that was down, left an exchange without its
/// things' touches; the rebuild writes them, keeps the exchange and the mark
/// as they were, and ends a touch an older build keyed at the reply's end.
#[tokio::test]
async fn a_rebuild_after_a_restart_writes_missing_touches_and_ends_misplaced_ones() {
    let r = rig(1, |_| {});
    let c = &r.core;
    let res = turn(c, owner_cli(), None, "look at crates/x/src/wal.rs").await;
    let ex = exchange(&c.store, &res.turn_id).unwrap().unwrap();
    let m = mark(&c.store, ex.ask.as_deref().unwrap()).unwrap().unwrap();
    assert!(
        newest(&c.store, wal_rs(), 8).unwrap().is_empty(),
        "no index ran"
    );
    let end = ex.end_ms.unwrap() + 5;
    let old = touch(wal_rs(), &ex.turn_id, &ex.session_id, end).unwrap();
    c.store.append(&[old]).unwrap();
    let naming: Arc<dyn crate::memory_pass::PassIndex> = Arc::new(Naming {
        delay: std::time::Duration::ZERO,
    });
    let dry = rebuilt(c, Some(naming.clone()), true).await;
    assert_eq!(
        (dry.kept, dry.exchanges, dry.touches, dry.repaired),
        (1, 0, 1, 1)
    );
    let wrote = rebuilt(c, Some(naming.clone()), false).await;
    assert_eq!((wrote.touches, wrote.repaired), (1, 1));
    let got = newest(&c.store, wal_rs(), 8).unwrap();
    assert_eq!(got.len(), 1, "one touch of the root for the exchange");
    assert_eq!(got[0].at_ms, ex.start_ms);
    assert_eq!(
        exchange(&c.store, &res.turn_id).unwrap().unwrap(),
        ex,
        "the exchange is kept"
    );
    assert_eq!(
        mark(&c.store, &m.node).unwrap().unwrap(),
        m,
        "the mark is kept"
    );
    let again = rebuilt(c, Some(naming), false).await;
    assert_eq!((again.touches, again.repaired), (0, 0));
}

/// An older build's reply-only touch, keyed at the reply's end, with the
/// index down: the rebuild moves it to the exchange's start, from its own
/// record, and loses nothing.
#[tokio::test]
async fn a_rebuild_with_the_index_down_moves_a_reply_only_touch_and_loses_none() {
    let r = rig(1, |_| {});
    let c = &r.core;
    let res = turn(c, owner_cli(), None, "a message that names nothing").await;
    let ex = exchange(&c.store, &res.turn_id).unwrap().unwrap();
    let only = Root::Thing("file:x.rs");
    let old = touch(only, &ex.turn_id, &ex.session_id, ex.end_ms.unwrap() + 5).unwrap();
    c.store.append(&[old]).unwrap();
    let dry = rebuilt(c, None, true).await;
    assert_eq!((dry.repaired, dry.touches), (1, 0));
    assert_eq!(
        newest(&c.store, only, 8).unwrap()[0].at_ms,
        ex.end_ms.unwrap() + 5,
        "a dry run moves nothing"
    );
    let wrote = rebuilt(c, None, false).await;
    assert!(wrote.things_skipped.is_some(), "the index is down");
    let got = newest(&c.store, only, 8).unwrap();
    assert_eq!(got.len(), 1, "moved, not lost");
    assert_eq!(
        (got[0].exchange.as_str(), got[0].at_ms),
        (res.turn_id.as_str(), ex.start_ms)
    );
    assert_eq!(rebuilt(c, None, false).await.repaired, 0);
}

/// A thing named past 16 KiB of a reply is named again by the rebuild: it
/// reads as much of a text as a live turn does.
#[tokio::test]
async fn the_rebuild_names_a_thing_as_far_into_a_text_as_a_turn_does() {
    let long = format!("{} and then crates/x/src/wal.rs", "filler ".repeat(3_000));
    assert!(long.len() > 16 * 1024 && long.len() < crate::exchange::THINGS_TEXT);
    let r = rig_saying(&[&long], |_| {});
    let c = &r.core;
    turn(c, owner_cli(), None, "say something long").await;
    assert!(
        newest(&c.store, wal_rs(), 8).unwrap().is_empty(),
        "no index ran"
    );
    let naming: Arc<dyn crate::memory_pass::PassIndex> = Arc::new(Naming {
        delay: std::time::Duration::ZERO,
    });
    assert_eq!(rebuilt(c, Some(naming), false).await.touches, 1);
    assert_eq!(newest(&c.store, wal_rs(), 8).unwrap().len(), 1);
}

/// Two places, one's name extending the other's past the key's separator: a
/// read of the first never returns the second's touches, and a touch of a
/// root that holds the separator is refused and counted.
#[test]
fn a_place_whose_name_extends_another_past_the_separator_is_not_read_as_it() {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let before = refused_touches();
    assert!(touch(Root::Place("local:/a\u{1}0"), "turn_9", "s", 9_000).is_none());
    assert!(touch(Root::Person("x\u{1}y"), "turn_9", "s", 9_000).is_none());
    assert!(refused_touches() >= before + 2);
    let mut recs: Vec<_> = (1..=3)
        .filter_map(|i| {
            touch(
                Root::Place("local:/a"),
                &format!("turn_{i}"),
                "s",
                1_000 * i,
            )
        })
        .collect();
    // As an older build could have written it: a place named `local:/a␁0`.
    for (i, at) in [(7u64, 500u64), (8, 2_500), (9, 9_000)] {
        let key = format!(
            "{TOUCH_PREFIX}place:local:/a{SEP}0{SEP}{:020}{SEP}turn_x{i}",
            u64::MAX - at
        );
        let t = Touch {
            exchange: format!("turn_x{i}"),
            session_id: "s".into(),
            at_ms: at,
        };
        recs.push(NewRecord::json(theseus_store::kinds::META, Some(&key), &t).unwrap());
    }
    store.append(&recs).unwrap();
    let names = |v: Vec<Touch>| v.into_iter().map(|t| t.exchange).collect::<Vec<_>>();
    let a = Root::Place("local:/a");
    assert_eq!(names(newest(&store, a, 2).unwrap()), ["turn_3", "turn_2"]);
    assert_eq!(
        names(newest(&store, a, 9).unwrap()),
        ["turn_3", "turn_2", "turn_1"]
    );
    assert_eq!(
        names(newest_before(&store, a, 2_000, 9).unwrap()),
        ["turn_1"]
    );
}

/// Namings that finish in the reverse order: the older reply's counts never
/// stand for the newer's, and the next exchange takes only its preceding
/// exchange's.
#[test]
fn a_late_naming_of_an_older_reply_never_replaces_a_newer_ones_counts() {
    let board = Board::default();
    let counts = |n| std::collections::BTreeMap::from([("file".to_string(), n)]);
    let of = |turn: &str, start_ms, n| ReplyThings {
        turn: turn.into(),
        start_ms,
        counts: counts(n),
    };
    put_reply(&board.replies(), "local", of("turn_b", 200, 2));
    put_reply(&board.replies(), "local", of("turn_a", 100, 1));
    assert_eq!(board.take_reply("local", Some("turn_b")), Some(counts(2)));
    put_reply(&board.replies(), "local", of("turn_a", 100, 1));
    assert_eq!(
        board.take_reply("local", Some("turn_b")),
        None,
        "not the preceding exchange's"
    );
    assert_eq!(
        board.take_reply("local", Some("turn_a")),
        None,
        "taken already"
    );
}

#[test]
fn a_timeline_page_keeps_every_exchange_at_its_boundary_time() {
    use crate::view::timeline::{Timeline, TimelineRoot};
    let dir = tempfile::tempdir().unwrap();
    let store = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let records: Vec<_> = (0..101)
        .map(|i| touch(Root::Person("owner"), &format!("turn_{i:03}"), "ses_a", 123).unwrap())
        .collect();
    store.append(&records).unwrap();
    let timeline = crate::view::wire::StoreTimeline(&store);
    let root = TimelineRoot::Person("owner".into());
    let first = timeline.newest(&root, None, 100).unwrap();
    let second = timeline
        .newest(&root, Some(&first.last().unwrap().cursor()), 100)
        .unwrap();
    assert_eq!(first.len(), 100);
    assert_eq!(
        second.len(),
        1,
        "same-time exchanges must survive pagination"
    );
    let ids: std::collections::BTreeSet<_> =
        first.iter().chain(&second).map(|e| &e.turn_id).collect();
    assert_eq!(ids.len(), 101);
    assert!(timeline
        .newest(&root, Some(&second[0].cursor()), 100)
        .unwrap()
        .is_empty());
}
