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

/// A turn after the place's `/new`: the view leaves the exchange before it out on purpose, and the row says so,
/// so the report counts it apart from a miss.
#[tokio::test]
async fn a_turn_after_new_says_its_previous_was_held_by_new() {
    let r = rig(2, |_| {});
    let c = &r.core;
    let first = turn(c, owner_cli(), None, "where is the tide table").await;
    let ex = exchange(&c.store, &first.turn_id).unwrap().unwrap();
    let at = crate::view::Pos {
        at_ms: ex.start_ms,
        id: ex.turn_id.clone(),
    };
    let mark = crate::ladder::new_mark_record(&ex.place, &at).unwrap();
    c.store.append(&[mark]).unwrap();
    let second = turn(c, owner_cli(), Some(&first.session_id), "and tomorrow?").await;
    let rows = rows(c);
    let last = rows.last().unwrap();
    assert_eq!(last.exchange, second.turn_id);
    assert_eq!(
        (last.prev, last.prev_why.as_deref()),
        (Prev::Missed, Some("new")),
        "{last:?}"
    );
    let report = ledger::report(&c.store, &ExchangeReportParams::default()).unwrap();
    let a = report.answers.expect("the answers");
    assert_eq!(a.previous.get("after_new"), Some(&1), "{a:?}");
    assert_eq!(a.previous.get("missed"), None, "{a:?}");
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

/// History whose place was never recorded is touched under `unknown` by the
/// rebuild, at its exchange's start, so a look-back by date reaches it (a date
/// page shows it, marked "place not recorded"); a second run writes nothing.
#[tokio::test]
async fn the_rebuild_touches_history_whose_place_was_never_recorded() {
    let r = rig(1, |_| {});
    let c = &r.core;
    let old = turn(c, owner_cli(), None, "an old terminal message").await;
    crate::test_support::before_the_ledger(&c.store, &old.session_id);
    rebuilt(c, None, false).await;
    let e = exchange(&c.store, &old.turn_id).unwrap().unwrap();
    assert_eq!(e.place, rebuild::UNKNOWN);
    let t = newest(&c.store, Root::Place(rebuild::UNKNOWN), 8).unwrap();
    let turns: Vec<&str> = t.iter().map(|t| t.exchange.as_str()).collect();
    assert_eq!(turns, [old.turn_id.as_str()]);
    assert_eq!(t[0].at_ms, e.start_ms);
    assert_eq!(rebuilt(c, None, false).await.touches, 0);
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

/// The reading sheet is the owner's to read in person (issue 44, step 4): his own terminal or the web UI
/// answers it; an agent's process on either, a job's and a process with no terminal are refused, and so is
/// a chat transport.
#[tokio::test]
async fn gists_is_refused_to_an_agents_process() {
    let r = rig(0, |_| {});
    let c = &r.core;
    let asked = json!({"lines": 5, "nodes": 2, "seed": 7});
    for owner in [
        owner_cli(),
        Client::new("ws#1", Surface::Web).behind(Behind::Owner),
    ] {
        let ok = call(c, owner, "exchange.gists", asked.clone()).await;
        assert!(ok.error.is_none(), "{:?}", ok.error);
        let sheet: theseus_protocol::exchange::ExchangeGistsResult =
            serde_json::from_value(ok.result.unwrap()).unwrap();
        assert_eq!(
            (sheet.seed, sheet.lines_total, sheet.nodes_total),
            (7, 0, 0)
        );
    }
    let agents = [
        ("claude", Surface::Cli),
        (peer_proc::JOB, Surface::Cli),
        (peer_proc::NO_TERMINAL, Surface::Cli),
        ("claude", Surface::Web),
    ];
    for (name, surface) in agents {
        let agent = Client::new("sock#9", surface).behind(Behind::Agent(name.into()));
        let no = call(c, agent, "exchange.gists", asked.clone()).await;
        let e = no.error.expect("an agent's process is refused");
        assert_eq!(
            e.code,
            theseus_protocol::error_code::REFUSED,
            "{name}: {e:?}"
        );
        assert!(e.message.contains("agent's process"), "{name}: {e:?}");
    }
    let dc = Client::new("discord#1", Surface::Discord);
    let no = call(c, dc, "exchange.gists", asked).await;
    let e = no.error.expect("a chat transport is refused");
    assert_eq!(e.code, theseus_protocol::error_code::REFUSED, "{e:?}");
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

/// A picture label belongs at its turn's time, not its exchange's start: the
/// rebuild leaves one there and moves one elsewhere there, so a later write
/// of the label finds the one key the turn holds.
#[tokio::test]
async fn a_rebuild_keeps_a_picture_label_at_its_turns_time() {
    let r = rig(1, |_| {});
    let c = &r.core;
    let res = turn(c, owner_cli(), None, "a message with a picture").await;
    let mut ex = exchange(&c.store, &res.turn_id).unwrap().unwrap();
    let minted = crate::id_ms(&res.turn_id).expect("a turn's id holds its time");
    ex.start_ms = minted + 5_000;
    let (kept, moved) = (
        Root::Thing("picture:document"),
        Root::Thing("picture:photo"),
    );
    let (turn_id, sid) = (ex.turn_id.as_str(), ex.session_id.as_str());
    c.store
        .append(&[
            exchange_record(&ex).unwrap(),
            touch(kept, turn_id, sid, minted).unwrap(),
            touch(moved, turn_id, sid, ex.start_ms).unwrap(),
        ])
        .unwrap();
    rebuilt(c, None, false).await;
    for root in [kept, moved] {
        let got: Vec<(String, u64)> = (newest(&c.store, root, 8).unwrap().into_iter())
            .map(|t| (t.exchange, t.at_ms))
            .collect();
        assert_eq!(got, [(res.turn_id.clone(), minted)], "{}", root.key());
    }
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

/// The rebuild writes each missing label touch once (issue 44: labels raised before their touches were
/// written): each flag on an item's record touches `label:<flag>` at its turn's time (the time its id was
/// minted, `crate::id_ms`), a removed one too (it is what Jev said: the record holds `personal` raised and
/// `secret` raised then removed by the owner), and a second run writes nothing. No model is called.
#[tokio::test]
async fn the_rebuild_writes_each_missing_label_touch_once() {
    use crate::content::{Flag, ItemKind, Label, Removed};
    let r = rig(1, |_| {});
    let c = &r.core;
    let t = turn(c, owner_cli(), None, "my diagnosis came back").await;
    let node = input_node(c, &t);
    let mut item = crate::item_labels::ItemLabels::new(&node, ItemKind::OperatorMessage, "cli", 1);
    for flag in [Flag::Personal, Flag::Secret] {
        item.raised.push(Label {
            flag,
            node: Some(node.clone()),
            kind: ItemKind::OperatorMessage,
            source: "cli".into(),
            confidence: 0.97,
            band: theseus_judge::band::Band::Confirm,
            at_ms: 1,
            judgment: String::new(),
        });
    }
    let removal = Removed {
        flag: Flag::Secret,
        by: "the owner".into(),
        via: "cli".into(),
        at_ms: 2,
    };
    crate::item_labels::removed(&mut item, &t.session_id, &removal);
    c.store.append(&[item.record().unwrap()]).unwrap();
    let roots = [Flag::Personal, Flag::Secret].map(crate::item_labels::touch_root);
    for root in &roots {
        assert!(newest(&c.store, Root::Thing(root), 8).unwrap().is_empty());
    }
    let first = rebuilt(c, None, false).await;
    assert_eq!(first.touches, 2, "{first:?}");
    let at = crate::id_ms(&t.turn_id).expect("a live turn's id is a UUIDv7");
    for root in &roots {
        let got = newest(&c.store, Root::Thing(root), 8).unwrap();
        assert_eq!(
            got.iter()
                .map(|t| (t.exchange.as_str(), t.at_ms))
                .collect::<Vec<_>>(),
            [(t.turn_id.as_str(), at)],
            "{root}"
        );
    }
    assert_eq!(rebuilt(c, None, false).await.touches, 0);
}

/// The rebuild's repair keeps a label where its turn began and moves one found elsewhere there (issue 44: one
/// key for one turn and one label); every other root is moved to its exchange's start, as before. The exchange's
/// start is set well after the turn's time so the two differ. A second run changes nothing.
#[tokio::test]
async fn the_rebuild_keeps_a_label_at_its_turns_time_and_moves_one_found_elsewhere() {
    let r = rig(1, |_| {});
    let c = &r.core;
    let t = turn(c, owner_cli(), None, "the kelp survey").await;
    let at = crate::id_ms(&t.turn_id).expect("a live turn's id is a UUIDv7");
    let mut ex = exchange(&c.store, &t.turn_id).unwrap().unwrap();
    ex.start_ms = at + 5_000;
    let (turn_id, sid) = (t.turn_id.as_str(), t.session_id.as_str());
    let mut recs = vec![exchange_record(&ex).unwrap()];
    // At the turn's time: kept. At the exchange's start: moved to the turn's time.
    recs.extend(touch(Root::Thing("label:personal"), turn_id, sid, at));
    recs.extend(touch(
        Root::Thing("label:secret"),
        turn_id,
        sid,
        ex.start_ms,
    ));
    // Any other root at the turn's time: moved to the exchange's start, as before.
    recs.extend(touch(Root::Thing("topic:kelp"), turn_id, sid, at));
    c.store.append(&recs).unwrap();
    let first = rebuilt(c, None, false).await;
    assert!(first.repaired > 0, "{first:?}");
    let times = |root: &str| -> Vec<u64> {
        (newest(&c.store, Root::Thing(root), 8).unwrap().iter())
            .filter(|t| t.exchange == turn_id)
            .map(|t| t.at_ms)
            .collect()
    };
    assert_eq!(times("label:personal"), [at], "kept");
    assert_eq!(times("label:secret"), [at], "moved to the turn's time");
    assert_eq!(times("topic:kelp"), [ex.start_ms], "moved as before");
    let again = rebuilt(c, None, false).await;
    assert_eq!((again.touches, again.repaired), (0, 0), "{again:?}");
    assert_eq!(times("label:personal"), [at]);
    assert_eq!(times("label:secret"), [at]);
    assert_eq!(times("topic:kelp"), [ex.start_ms]);
}

// ------------------------------------------------------------ records under `labels.` that do not read (issue 44)

/// Words in the payloads of the records below: a log that quoted a payload would show them.
const PAYLOAD: &str = "PAYLOAD-WORDS-q7";

/// A label raised on `node`, as a stored item's `raised` holds it.
fn label_json(node: &str, flag: &str) -> Value {
    json!({"flag": flag, "node": node, "kind": "operator_message", "source": "cli",
           "confidence": 0.97, "band": "confirm", "at_ms": 1})
}

/// An item's record as the builds from 7b89d3944 to ab786c5da wrote it: a removal kept on the item named no
/// session, which the item's reader of today does not read.
fn older_item(node: &str, flag: &str) -> Value {
    json!({"node": node, "kind": "operator_message", "source": "cli", "entered_ms": 1,
           "raised": [label_json(node, flag)],
           "removed": [{"flag": "secret", "by": "the owner", "via": "cli", "at_ms": 2, "note": PAYLOAD}]})
}

/// An item's record of today's shape but for one raised label, `injection`, that does not read (it has no
/// `source`): a damaged item's record, which is read for nothing.
fn damaged_item(node: &str) -> Value {
    let mut bad = label_json(node, "injection");
    bad.as_object_mut().unwrap().remove("source");
    bad["note"] = json!(PAYLOAD);
    json!({"node": node, "kind": "operator_message", "source": "cli", "entered_ms": 1,
           "raised": [label_json(node, "personal"), bad]})
}

/// An object of 268 bytes with no node: the shape of the record a rebuild failed on.
fn object_268() -> Value {
    let mut big = json!({"kind": "operator_message", "source": "cli", "entered_ms": 1, "note": PAYLOAD,
                         "pad": ""});
    let len = serde_json::to_vec(&big).unwrap().len();
    big["pad"] = json!("x".repeat(268 - len));
    assert_eq!(serde_json::to_vec(&big).unwrap().len(), 268);
    big
}

/// What a store may hold under `labels.` that is no item's (a rebuild failed on one: "missing field `node`
/// at line 1 column 268"): an object of 268 bytes with no node, an empty object, a string, and a key that
/// was ended. Their keys, but the ended one's.
fn odd_records(c: &Core) -> [&'static str; 3] {
    let keys = ["labels.odd-268", "labels.odd-empty", "labels.odd-text"];
    for (key, v) in keys.iter().zip([object_268(), json!({}), json!(PAYLOAD)]) {
        c.store.put_meta(key, &v).unwrap();
    }
    c.store
        .put_meta("labels.odd-ended", &json!({"note": PAYLOAD}))
        .unwrap();
    c.store.delete_meta("labels.odd-ended").unwrap();
    keys
}

/// Two turns in one session: the first's message carries `personal` on a record of today's shape, the
/// second's `confidential` on an older one; beside them, [`odd_records`]. The two turns, the second's reply, and
/// the older record's key.
async fn labelled_with_odd_records(
    c: &Arc<Core>,
) -> (TurnSubmitResult, TurnSubmitResult, String, String) {
    use crate::content::{Flag, ItemKind, Label};
    let first = turn(c, owner_cli(), None, "my diagnosis came back").await;
    let sid = first.session_id.clone();
    let second = turn(c, owner_cli(), Some(&sid), "and the kelp survey").await;
    let (a, b) = (input_node(c, &first), input_node(c, &second));
    let mut item = crate::item_labels::ItemLabels::new(&a, ItemKind::OperatorMessage, "cli", 1);
    item.raised.push(Label {
        flag: Flag::Personal,
        node: Some(a),
        kind: ItemKind::OperatorMessage,
        source: "cli".into(),
        confidence: 0.97,
        band: theseus_judge::band::Band::Confirm,
        at_ms: 1,
        judgment: String::new(),
    });
    c.store.append(&[item.record().unwrap()]).unwrap();
    let older = crate::item_labels::key(&b);
    c.store
        .put_meta(&older, &older_item(&b, "confidential"))
        .unwrap();
    odd_records(c);
    let reply = (c.store.session_nodes(&sid).unwrap().into_iter().rev())
        .find(|(_, n)| matches!(n.body, crate::node::Body::AssistantMessage { .. }))
        .map(|(_, n)| n.id)
        .expect("the second turn's reply");
    (first, second, reply, older)
}

/// A rebuild over a store that holds records under `labels.` that are no item's reads past them (issue 44):
/// through the real method, its dry run and its run each say how many it passed over, the dry run appends
/// nothing, the run writes the label touches of today's record and of the older one (read in its older
/// shape), and the older record and those passed over are left as they are, at their positions, byte for
/// byte.
#[tokio::test]
async fn a_rebuild_passes_over_records_under_labels_that_are_no_items() {
    use crate::content::Flag;
    let r = rig(2, |_| {});
    let c = &r.core;
    let (first, second, _, older) = labelled_with_odd_records(c).await;
    let roots = [Flag::Personal, Flag::Confidential].map(crate::item_labels::touch_root);
    let mut left = vec![older];
    left.extend(["labels.odd-268", "labels.odd-empty", "labels.odd-text"].map(String::from));
    let held = || -> Vec<(u64, Vec<u8>)> {
        (left.iter())
            .map(|key| {
                let r = (c.store.inner())
                    .latest_by_key(theseus_store::kinds::META, key)
                    .unwrap();
                let r = r.unwrap_or_else(|| panic!("{key} is held"));
                (r.position, r.payload)
            })
            .collect()
    };
    let before = held();
    let end = c.store.last_position();
    let rebuild = |dry_run: bool| {
        call(
            c,
            owner_cli(),
            "exchange.rebuild",
            json!({"dry_run": dry_run}),
        )
    };
    let dry = rebuild(true).await;
    assert!(dry.error.is_none(), "{:?}", dry.error);
    let dry = dry.result.unwrap();
    assert_eq!(
        (dry["labels_passed_over"].as_u64(), dry["touches"].as_u64()),
        (Some(3), Some(2)),
        "{dry}"
    );
    for root in &roots {
        assert!(
            newest(&c.store, Root::Thing(root), 8).unwrap().is_empty(),
            "the dry run wrote {root}"
        );
    }
    assert_eq!(c.store.last_position(), end, "the dry run appended");
    assert_eq!(held(), before, "the dry run left each record as it is");
    let run = rebuild(false).await;
    assert!(run.error.is_none(), "{:?}", run.error);
    let run = run.result.unwrap();
    assert_eq!(
        (run["labels_passed_over"].as_u64(), run["touches"].as_u64()),
        (Some(3), Some(2)),
        "{run}"
    );
    for (root, t) in roots.iter().zip([&first, &second]) {
        let got = newest(&c.store, Root::Thing(root), 8).unwrap();
        let turns: Vec<&str> = got.iter().map(|t| t.exchange.as_str()).collect();
        assert_eq!(turns, [t.turn_id.as_str()], "{root}");
    }
    assert_eq!(held(), before, "the run left each record as it is");
    let ended = c
        .store
        .inner()
        .latest_by_key(theseus_store::kinds::META, "labels.odd-ended");
    assert!(ended.unwrap().is_none());
}

/// The rebuild that failed with "missing field `node` at line 1 column 268" completes (issue 44): one item's
/// labels and, under `labels.`, one object of 268 bytes with no node. Its label is touched, and the object is
/// passed over and counted.
#[tokio::test]
async fn a_rebuild_over_an_object_of_268_bytes_with_no_node_completes() {
    use crate::content::{Flag, ItemKind, Label};
    let r = rig(1, |_| {});
    let c = &r.core;
    let t = turn(c, owner_cli(), None, "my diagnosis came back").await;
    let node = input_node(c, &t);
    let mut item = crate::item_labels::ItemLabels::new(&node, ItemKind::OperatorMessage, "cli", 1);
    item.raised.push(Label {
        flag: Flag::Personal,
        node: Some(node.clone()),
        kind: ItemKind::OperatorMessage,
        source: "cli".into(),
        confidence: 0.97,
        band: theseus_judge::band::Band::Confirm,
        at_ms: 1,
        judgment: String::new(),
    });
    c.store.append(&[item.record().unwrap()]).unwrap();
    c.store.put_meta("labels.odd-268", &object_268()).unwrap();
    let run = call(c, owner_cli(), "exchange.rebuild", json!({})).await;
    assert!(run.error.is_none(), "{:?}", run.error);
    let run = run.result.unwrap();
    assert_eq!(
        (run["labels_passed_over"].as_u64(), run["touches"].as_u64()),
        (Some(1), Some(1)),
        "{run}"
    );
    let root = crate::item_labels::touch_root(Flag::Personal);
    let got = newest(&c.store, Root::Thing(&root), 8).unwrap();
    let turns: Vec<&str> = got.iter().map(|t| t.exchange.as_str()).collect();
    assert_eq!(turns, [t.turn_id.as_str()]);
}

/// A damaged item's record under `labels.` (one raised label does not read) is passed over and counted by the
/// rebuild, and none of its labels is touched, not even the one that reads by itself (issue 44).
#[tokio::test]
async fn a_rebuild_passes_over_a_damaged_items_record_and_touches_none_of_its_labels() {
    use crate::content::Flag;
    let r = rig(1, |_| {});
    let c = &r.core;
    let t = turn(c, owner_cli(), None, "my diagnosis came back").await;
    let node = input_node(c, &t);
    let key = crate::item_labels::key(&node);
    c.store.put_meta(&key, &damaged_item(&node)).unwrap();
    let run = call(c, owner_cli(), "exchange.rebuild", json!({})).await;
    assert!(run.error.is_none(), "{:?}", run.error);
    let run = run.result.unwrap();
    assert_eq!(
        (run["labels_passed_over"].as_u64(), run["touches"].as_u64()),
        (Some(1), Some(0)),
        "{run}"
    );
    for flag in [Flag::Personal, Flag::Injection] {
        let root = crate::item_labels::touch_root(flag);
        let got = newest(&c.store, Root::Thing(&root), 8).unwrap();
        assert!(got.is_empty(), "{root} was touched");
    }
    let left = (c.store.inner()).latest_by_key(theseus_store::kinds::META, &key);
    assert!(left.unwrap().is_some(), "{key} is left as it is");
}

/// An item's record of today's shape whose removals are `removed`, with `personal` raised on `node`.
fn item_removing(node: &str, removed: Value) -> Value {
    json!({"node": node, "kind": "operator_message", "source": "cli", "entered_ms": 1,
           "raised": [label_json(node, "personal")], "removed": removed})
}

/// Removals that are not the older shape, where every removal has no `session` key at all: one whose
/// `session` is null, and a list of one with a session and one without.
fn removals_not_older(sid: &str) -> [Value; 2] {
    let removal =
        json!({"flag": "secret", "by": "the owner", "via": "cli", "at_ms": 2, "note": PAYLOAD});
    let mut null_session = removal.clone();
    null_session["session"] = Value::Null;
    let mut named = removal.clone();
    named["session"] = json!(sid);
    [json!([null_session]), json!([named, removal])]
}

/// An item's record whose removals are neither today's nor the older shape (a removal's `session` is null, or
/// only some removals have one) is passed over and counted by the rebuild, and none of its labels is touched
/// (issue 44).
#[tokio::test]
async fn a_rebuild_passes_over_an_items_record_whose_removals_are_neither_shape() {
    use crate::content::Flag;
    for i in 0..2 {
        let r = rig(1, |_| {});
        let c = &r.core;
        let t = turn(c, owner_cli(), None, "my diagnosis came back").await;
        let node = input_node(c, &t);
        let removed = removals_not_older(&t.session_id)[i].clone();
        let key = crate::item_labels::key(&node);
        c.store
            .put_meta(&key, &item_removing(&node, removed))
            .unwrap();
        let run = call(c, owner_cli(), "exchange.rebuild", json!({})).await;
        assert!(run.error.is_none(), "{:?}", run.error);
        let run = run.result.unwrap();
        assert_eq!(
            (run["labels_passed_over"].as_u64(), run["touches"].as_u64()),
            (Some(1), Some(0)),
            "{i}: {run}"
        );
        let root = crate::item_labels::touch_root(Flag::Personal);
        let got = newest(&c.store, Root::Thing(&root), 8).unwrap();
        assert!(got.is_empty(), "{i}: {root} was touched");
    }
}

/// A fork of a session over such a store completes (issue 44), and carries today's record's label and the
/// older record's (its removal, which names no session, is not read: a label stands). A record that does not
/// read, whose key and `node` name no item of the fork, is read for nothing: a label raised in it is not
/// carried, though it names a node inside the fork.
#[tokio::test]
async fn a_fork_reads_past_records_under_labels_that_do_not_read() {
    use crate::content::Flag;
    let r = rig(2, |_| {});
    let c = &r.core;
    let (first, second, reply, _) = labelled_with_odd_records(c).await;
    let a = input_node(c, &first);
    let placed = json!({"raised": [label_json(&a, "secret")], "note": PAYLOAD});
    c.store.put_meta("labels.odd-placed", &placed).unwrap();
    let fork = c
        .session_fork(
            theseus_protocol::SessionForkParams {
                session: second.session_id,
                at: reply,
                ..Default::default()
            },
            Surface::Cli,
        )
        .expect("the fork completes");
    let rec: crate::session::SessionRecord =
        c.store.get_session(&fork.session_id).unwrap().unwrap();
    let flags = rec.data.map(|b| *b).unwrap_or_default();
    for f in [Flag::Personal, Flag::Confidential] {
        assert!(flags.has(f), "{f:?} is carried: {flags:?}");
    }
    assert!(
        !flags.has(Flag::Secret),
        "nothing is read from it: {flags:?}"
    );
}

/// A tracing writer that keeps what is logged.
#[derive(Clone, Default)]
struct Log(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Log {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Log {
    type Writer = Log;
    fn make_writer(&'a self) -> Log {
        self.clone()
    }
}

/// Each record under `labels.` that does not read is named once, by its key, in a warning that quotes
/// nothing of its payload (issue 44); a record in the older shape reads, and is not named; an ended key is
/// not read at all.
#[tokio::test]
async fn a_record_under_labels_that_does_not_read_is_named_by_its_key_alone() {
    let r = rig(2, |_| {});
    let c = &r.core;
    let (first, _, _, older) = labelled_with_odd_records(c).await;
    let log = Log::default();
    let sub = tracing_subscriber::fmt()
        .with_writer(log.clone())
        .with_ansi(false)
        .finish();
    let mut flags = crate::content::ContentFlags::default();
    let read = tracing::subscriber::with_default(sub, || {
        crate::fork::item_data_through(&c.store, &first.session_id, u64::MAX, &mut flags)
    });
    assert!(read.is_ok(), "{read:?}");
    let text = String::from_utf8(log.0.lock().unwrap().clone()).unwrap();
    for key in ["labels.odd-268", "labels.odd-empty", "labels.odd-text"] {
        assert_eq!(text.matches(key).count(), 1, "{key} once: {text}");
    }
    assert!(!text.contains(older.as_str()), "{text}");
    assert!(!text.contains(PAYLOAD), "a payload was quoted: {text}");
    assert!(!text.contains("labels.odd-ended"), "{text}");
}

/// Words a planted record holds where a decoder's error would quote them.
const TOUCH_WORDS: &str = "TOUCH-WORDS-m3";

/// A record at the key of a touch the rebuild would move, which does not read as a touch: its time is words.
async fn unread_touch(c: &Arc<Core>) -> (Exchange, String) {
    let res = turn(c, owner_cli(), None, "a message that names nothing").await;
    let ex = exchange(&c.store, &res.turn_id).unwrap().unwrap();
    let at = ex.end_ms.unwrap() + 5;
    let key = touch(Root::Thing("file:x.rs"), &ex.turn_id, &ex.session_id, at)
        .unwrap()
        .key
        .unwrap();
    let odd = json!({"exchange": ex.turn_id, "session_id": ex.session_id, "at_ms": TOUCH_WORDS});
    c.store.put_meta(&key, &odd).unwrap();
    (ex, key)
}

/// A touch the rebuild would move that does not read as a touch fails neither the run nor the dry run
/// (issue 46): it is passed over and counted, never moved, rewritten or ended, and the rest is moved.
#[tokio::test]
async fn a_rebuild_passes_over_a_touch_that_does_not_read() {
    let r = rig(1, |_| {});
    let c = &r.core;
    let (ex, key) = unread_touch(c).await;
    let readable = Root::Thing("file:y.rs");
    let moved = touch(readable, &ex.turn_id, &ex.session_id, ex.end_ms.unwrap() + 5).unwrap();
    c.store.append(&[moved]).unwrap();
    let held = || {
        let r = (c.store.inner()).latest_by_key(theseus_store::kinds::META, &key);
        r.unwrap().map(|r| (r.position, r.payload))
    };
    let before = held();
    assert!(before.is_some());
    let end = c.store.last_position();
    let dry = call(c, owner_cli(), "exchange.rebuild", json!({"dry_run": true})).await;
    assert!(dry.error.is_none(), "{:?}", dry.error);
    let dry = dry.result.unwrap();
    assert_eq!(
        (dry["touches_passed_over"].as_u64(), dry["repaired"].as_u64()),
        (Some(1), Some(1)),
        "{dry}"
    );
    assert_eq!(c.store.last_position(), end, "the dry run appended");
    let run = call(c, owner_cli(), "exchange.rebuild", json!({})).await;
    assert!(run.error.is_none(), "{:?}", run.error);
    let run = run.result.unwrap();
    assert_eq!(
        (run["touches_passed_over"].as_u64(), run["repaired"].as_u64()),
        (Some(1), Some(1)),
        "{run}"
    );
    assert_eq!(held(), before, "the run left it as it is");
    let got = newest(&c.store, readable, 8).unwrap();
    assert_eq!(
        got.iter().map(|t| t.at_ms).collect::<Vec<_>>(),
        [ex.start_ms],
        "the touch that reads is moved"
    );
}

/// A touch that does not read is named in the log by its key alone, never by what the record holds
/// (issue 46).
#[tokio::test]
async fn a_touch_that_does_not_read_is_named_by_its_key_alone() {
    let r = rig(1, |_| {});
    let c = &r.core;
    let (ex, key) = unread_touch(c).await;
    let starts = std::collections::HashMap::from([(ex.turn_id.as_str(), ex.start_ms)]);
    let log = Log::default();
    let sub = tracing_subscriber::fmt()
        .with_writer(log.clone())
        .with_ansi(false)
        .finish();
    let read = tracing::subscriber::with_default(sub, || rebuild::misplaced(&c.store, &starts));
    let (moves, passed) = read.unwrap();
    assert_eq!((moves.len(), passed), (0, 1));
    let text = String::from_utf8(log.0.lock().unwrap().clone()).unwrap();
    assert_eq!(text.matches(key.as_str()).count(), 1, "{text}");
    assert!(!text.contains(TOUCH_WORDS), "the record was quoted: {text}");
}

/// Words planted records hold where a decoder's error would quote them.
const LINE_WORDS: &str = "LINE-WORDS-r2";
const NODE_WORDS: &str = "NODE-WORDS-n8";

/// A record under the summary lines' prefix and one under the fold nodes', neither of which reads: their
/// keys.
fn unread_line_and_node(c: &Core) -> [String; 2] {
    let line = format!("{}odd", crate::ladder::did::DID_PREFIX);
    let node = format!("{}odd", crate::view::fold::FOLD_PREFIX);
    let odd_line = json!({"turn_id": "trn_odd", "session_id": "ses_odd", "line": "", "model": "",
                          "writer": LINE_WORDS});
    let odd_node = json!({"id": "fld_odd", "place": "local:odd", "level": NODE_WORDS});
    c.store.put_meta(&line, &odd_line).unwrap();
    c.store.put_meta(&node, &odd_node).unwrap();
    [line, node]
}

/// The reading sheet passes over a summary line and a fold node it cannot read (issue 46): it completes,
/// counts each, draws from the rest and leaves both records as they are.
#[tokio::test]
async fn the_reading_sheet_passes_over_records_that_do_not_read() {
    let r = rig(1, |_| {});
    let c = &r.core;
    let keys = unread_line_and_node(c);
    let held = || {
        (keys.iter())
            .map(|k| (c.store.inner()).latest_by_key(theseus_store::kinds::META, k))
            .map(|r| r.unwrap().map(|r| (r.position, r.payload)))
            .collect::<Vec<_>>()
    };
    let before = held();
    let got = call(
        c,
        owner_cli(),
        "exchange.gists",
        json!({"lines": 5, "nodes": 5, "seed": 1}),
    )
    .await;
    assert!(got.error.is_none(), "{:?}", got.error);
    let got = got.result.unwrap();
    assert_eq!(
        (
            got["lines_unread"].as_u64(),
            got["nodes_unread"].as_u64(),
            got["lines_total"].as_u64(),
            got["nodes_total"].as_u64()
        ),
        (Some(1), Some(1), Some(0), Some(0)),
        "{got}"
    );
    assert_eq!(held(), before, "left as they are");
}

/// The reading sheet names a record it cannot read by its key alone in the log, never by what it holds
/// (issue 46).
#[tokio::test]
async fn the_reading_sheet_names_a_record_it_cannot_read_by_its_key_alone() {
    let r = rig(1, |_| {});
    let c = &r.core;
    let keys = unread_line_and_node(c);
    let p = theseus_protocol::exchange::ExchangeGistsParams {
        lines: 5,
        nodes: 5,
        seed: Some(1),
        ..Default::default()
    };
    let log = crate::test_support::Log::default();
    let read = log.capture(|| crate::ladder::gists::sheet(&c.store, &c.tools.scrubber, &p));
    let read = read.unwrap();
    assert_eq!((read.lines_unread, read.nodes_unread), (1, 1));
    let text = log.text();
    for key in &keys {
        assert_eq!(text.matches(key.as_str()).count(), 1, "{key} once: {text}");
    }
    for quoted in [LINE_WORDS, NODE_WORDS] {
        assert!(!text.contains(quoted), "a record was quoted: {text}");
    }
}
