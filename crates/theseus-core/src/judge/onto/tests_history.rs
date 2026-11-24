//! The history filed under topics once (issue 46; `history.rs`), through a
//! whole core and the fake Jev, over invented exchanges in a temporary store:
//! the touches, what it never writes, the resume, the dry run, the cap, the
//! day's budget, who may run it, and that a message's words reach no row and
//! no answer.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, FakeMode, Scripted};
use theseus_ontology::{Category, CategoryId, MemberList, Membership, Origin, Record};
use theseus_protocol::exchange::{ExchangeTopicsParams, ExchangeTopicsResult};
use theseus_protocol::{error_code, method, LedgerKind, SessionKind};
use theseus_store::Store as _;

use crate::approval::Surface;
use crate::exchange::{self, Exchange, Opener};
use crate::ledger::LedgerRow;
use crate::node::Node;
use crate::peer_proc::Behind;
use crate::session::SessionRecord;
use crate::tests_m3::{rpc_as, surface};
use crate::tests_onto::{bound, rig, rig_with};
use crate::Core;

/// Words no row and no answer may hold.
const WORDS: [&str; 2] = ["Quillfeather", "Marrowby"];
const SAID: &str = "plan the Quillfeather crossing with Marrowby before the tide";

fn id(s: &str) -> CategoryId {
    CategoryId::parse(s).unwrap()
}

fn operator_writes(core: &Core, records: Vec<Record>) {
    core.runner
        .ontology
        .write(&core.store, records, Origin::Operator, |_| Ok(vec![]))
        .unwrap();
}

/// The one topic the ontology holds.
fn ledger_topic(core: &Core) {
    let c = Category::new(id("topic:ledger"), "Ledger", "ada");
    operator_writes(core, vec![Record::Category(c)]);
}

/// A plain conversation's session, in no place.
fn session(core: &Core) -> String {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    rec.session_id
}

/// An old exchange of `sid`, opened by `node` (written first) at `start_ms`.
fn old_exchange(core: &Core, sid: &str, turn: &str, node: &Node, start_ms: u64) {
    core.store.append(&[node.record().unwrap()]).unwrap();
    let e = Exchange {
        turn_id: turn.into(),
        session_id: sid.into(),
        place: "discord:dm:42".into(),
        private: true,
        opened_by: Opener::Message,
        person: Some(exchange::OWNER.into()),
        start_ms,
        end_ms: Some(start_ms + 1_000),
        ask: Some(node.id.clone()),
        reply: None,
        messages: vec![node.id.clone()],
        stop: Some("end_turn".into()),
        rebuilt: false,
    };
    core.store
        .append(&[exchange::exchange_record(&e).unwrap()])
        .unwrap();
}

/// An old exchange opened by the person's `text`.
fn said(core: &Core, sid: &str, turn: &str, text: &str, start_ms: u64) {
    let n = Node::user(sid, Some(turn), "test", text);
    old_exchange(core, sid, turn, &n, start_ms);
}

fn params(dry_run: bool, cap_usd: Option<f64>) -> ExchangeTopicsParams {
    ExchangeTopicsParams { dry_run, cap_usd }
}

/// The job, as the command runs it once the owner's act is judged.
async fn run(core: &Arc<Core>, p: ExchangeTopicsParams) -> ExchangeTopicsResult {
    let r = core.topics_history(&p).await.unwrap();
    flush(core).await;
    r
}

/// The sink's rows written now.
async fn flush(core: &Arc<Core>) {
    let judge = core.runner.judge.clone();
    tokio::task::spawn_blocking(move || judge.flush_sink())
        .await
        .unwrap();
}

/// Wait until Jev has seen `n` requests.
async fn seen(jev: &FakeJev, n: usize) {
    let t0 = std::time::Instant::now();
    while jev.seen().len() < n {
        assert!(t0.elapsed().as_secs() < 30, "{n} request(s) seen");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

/// The command through the RPC, from the owner's terminal.
async fn command(core: &Arc<Core>, dry_run: bool) -> Result<Value, theseus_protocol::RpcError> {
    rpc_as(
        core,
        surface("sock#1", Surface::Cli),
        method::EXCHANGE_TOPICS,
        json!({"dry_run": dry_run}),
    )
    .await
}

/// Roll the ontology's pack back to shadow, as the ladder's rollback does:
/// its judge no longer acts.
fn rolled_back(core: &Core) {
    let row = theseus_protocol::packs::PackModeRow {
        pack: crate::judge::onto::ONTO_PACK.into(),
        mode: crate::judge::ladder::Rung::Shadow.as_str().into(),
        from: crate::judge::ladder::Rung::Live.as_str().into(),
        who: "owner".into(),
        by: "owner".into(),
        via: "rollback".into(),
        why: "a test's rollback".into(),
        ..Default::default()
    };
    core.runner.judge.ladder().write(row).unwrap();
}

/// Take back the touch of exchange `turn`, as a stop between an answer's
/// row and its touches would leave it.
fn untouch(core: &Core, turn: &str) {
    let key = (core.store.inner())
        .latest_with_prefix(theseus_store::kinds::META, exchange::TOUCH_PREFIX)
        .unwrap()
        .into_iter()
        .filter_map(|r| r.key)
        .find(|k| k.ends_with(turn))
        .unwrap();
    let gone = theseus_store::NewRecord::delete_meta(&key);
    core.store.append(&[gone]).unwrap();
}

fn timeline(core: &Core, category: &str) -> Vec<exchange::Touch> {
    crate::names::topics::timeline(&core.store, &id(category), 10).unwrap()
}

fn ledger(core: &Core) -> Vec<LedgerRow> {
    core.store
        .ledger_tail::<LedgerRow>(100_000)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .collect()
}

fn of_kind(core: &Core, kind: LedgerKind) -> Vec<LedgerRow> {
    let kind = kind.as_str();
    ledger(core)
        .into_iter()
        .filter(|r| r.kind == kind)
        .collect()
}

/// What the ontology files: its categories, and every session's lists of
/// every kind `onto` asks.
fn filed(core: &Core, sessions: &[&str]) -> String {
    let o = core.runner.ontology.snapshot(&core.store).unwrap();
    let mut cats: Vec<String> = o.categories().map(|c| c.id.to_string()).collect();
    cats.sort();
    let lists: Vec<Value> = sessions
        .iter()
        .flat_map(|s| {
            theseus_judge::builders::ONTO_KINDS.iter().map(|k| {
                let list = o.member_list(s, k).map(|l| l.into_owned());
                list.map_or(Value::Null, |l| Record::Members(l).to_value())
            })
        })
        .collect();
    let projects: Vec<Option<String>> = sessions
        .iter()
        .map(|s| {
            let r: SessionRecord = core.store.get_session(s).unwrap().unwrap();
            r.project
        })
        .collect();
    serde_json::to_string(&json!([cats, lists, projects])).unwrap()
}

/// KEY (the plan's test 1): an old exchange that touches no topic gains a
/// touch of the topic Jev chose at the act band, at the exchange's own time,
/// and its judgment says it was this job's.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn history_filing_touches_an_old_exchange_by_the_topics_jev_chose() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    said(&r.core, &sid, "turn_old", SAID, 5_000);
    let out = run(&r.core, params(false, None)).await;
    assert_eq!(
        (
            out.exchanges,
            out.to_ask,
            out.asked,
            out.answered,
            out.filed,
            out.touches
        ),
        (1, 1, 1, 1, 1, 1),
        "{out:?}"
    );
    let [t] = timeline(&r.core, "topic:ledger")
        .try_into()
        .unwrap_or_else(|v| panic!("one touch: {v:?}"));
    assert_eq!(
        (t.exchange.as_str(), t.session_id.as_str(), t.at_ms),
        ("turn_old", sid.as_str(), 5_000)
    );
    let calls = of_kind(&r.core, LedgerKind::JudgeCall);
    let [call] = calls.as_slice() else {
        panic!("one judgment: {calls:?}");
    };
    assert_eq!(call.data["context"]["purpose"], "history");
    assert_eq!(call.data["context"]["turn"], "turn_old");
    assert_eq!(call.data["mode"], "shadow", "the sink acts on none of it");
    assert_eq!(r.jev.seen().len(), 1, "one call");
}

/// KEY (the plan's test 2): no category is minted, no membership changes and
/// no project is attached, even when Jev says a new topic is wanted, names
/// it, and says a held topic no longer fits; no filing row, no card.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn history_filing_mints_nothing_and_files_no_session() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "new", 0.97);
    jev.script_onto("topic_name", "n1", 0.97);
    jev.script("onto.v1/still_topic", Scripted::Noul(0.01));
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    let other = session(&r.core);
    let held = MemberList {
        session: sid.clone(),
        kind: "topic".into(),
        members: vec![Membership::operator(id("topic:ledger"), 1_000)],
    };
    operator_writes(&r.core, vec![Record::Members(held)]);
    said(&r.core, &sid, "turn_a", SAID, 5_000);
    said(
        &r.core,
        &other,
        "turn_b",
        "Lantern Harbour needs a new topic",
        6_000,
    );
    let before = filed(&r.core, &[&sid, &other]);
    let out = run(&r.core, params(false, None)).await;
    assert_eq!((out.asked, out.answered), (2, 2), "{out:?}");
    assert_eq!(filed(&r.core, &[&sid, &other]), before, "nothing filed");
    assert_eq!((out.filed, out.touches), (0, 0), "a `new` names nothing");
    assert!(timeline(&r.core, "topic:ledger").is_empty());
    assert!(of_kind(&r.core, LedgerKind::OntoFiled).is_empty());
    assert_eq!(of_kind(&r.core, LedgerKind::JudgeCall).len(), 2);
}

/// KEY (the plan's test 3): a rerun asks nothing of an exchange this job
/// asked before, though its answer touched nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rerun_asks_nothing() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "none", 0.9);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    said(&r.core, &sid, "turn_old", SAID, 5_000);
    let first = run(&r.core, params(false, None)).await;
    assert_eq!((first.asked, first.filed), (1, 0), "{first:?}");
    let calls = r.jev.seen().len();
    let again = run(&r.core, params(false, None)).await;
    assert_eq!(r.jev.seen().len(), calls, "no call");
    assert_eq!(
        (again.asked_before, again.to_ask, again.asked),
        (1, 0, 0),
        "{again:?}"
    );
    assert_eq!(of_kind(&r.core, LedgerKind::JudgeCall).len(), 1);
}

/// An answer this job recorded whose touches were not written (a stop
/// between the two) is touched on the rerun from its row, with no call.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rerun_writes_a_recorded_answers_missing_touches_without_a_call() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    said(&r.core, &sid, "turn_old", SAID, 5_000);
    run(&r.core, params(false, None)).await;
    let [t] = timeline(&r.core, "topic:ledger").try_into().unwrap();
    let key = (r.core.store.inner())
        .latest_with_prefix(theseus_store::kinds::META, exchange::TOUCH_PREFIX)
        .unwrap()
        .into_iter()
        .filter_map(|r| r.key)
        .find(|k| k.ends_with(&t.exchange))
        .unwrap();
    let gone = theseus_store::NewRecord::delete_meta(&key);
    r.core.store.append(&[gone]).unwrap();
    assert!(timeline(&r.core, "topic:ledger").is_empty());
    let calls = r.jev.seen().len();
    let again = run(&r.core, params(false, None)).await;
    assert_eq!(r.jev.seen().len(), calls, "no call");
    assert_eq!((again.asked_before, again.filed), (1, 1), "{again:?}");
    assert_eq!(timeline(&r.core, "topic:ledger").len(), 1);
}

/// KEY (the plan's test 4): the dry run counts and prices, calls nothing and
/// writes nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_dry_run_prices_and_calls_nothing() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    said(&r.core, &sid, "turn_a", SAID, 5_000);
    said(&r.core, &sid, "turn_b", "and the Lantern Ledger too", 6_000);
    let v = rpc_as(
        &r.core,
        surface("sock#1", Surface::Cli),
        method::EXCHANGE_TOPICS,
        json!({"dry_run": true}),
    )
    .await
    .unwrap();
    flush(&r.core).await;
    let out: ExchangeTopicsResult = serde_json::from_value(v).unwrap();
    assert!(out.dry_run);
    assert_eq!(
        (out.to_ask, out.asked, out.cap_usd),
        (2, 0, 10.0),
        "{out:?}"
    );
    assert!(out.worst_usd > 0.0, "{out:?}");
    assert!(r.jev.seen().is_empty(), "no call");
    assert!(of_kind(&r.core, LedgerKind::JudgeCall).is_empty());
    assert!(timeline(&r.core, "topic:ledger").is_empty());
}

/// KEY (the plan's test 5): the run stops before the exchange whose worst
/// case would pass the cap, says so, and a rerun with room goes on.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_cap_stops_before_an_exchange_that_would_pass_it() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    said(&r.core, &sid, "turn_a", SAID, 5_000);
    let first = run(&r.core, params(true, None)).await.worst_usd;
    let long = format!(
        "the Lantern Ledger again: {}",
        "a longer word list ".repeat(60)
    );
    said(&r.core, &sid, "turn_b", &long, 6_000);
    let both = run(&r.core, params(true, None)).await.worst_usd;
    let second = both - first;
    assert!(second > first, "{first} {second}");
    let cap = first + (second - first) / 2.0;
    let out = run(&r.core, params(false, Some(cap))).await;
    assert_eq!((out.asked, out.left), (1, 1), "{out:?}");
    assert!(out.spent_usd <= cap, "{out:?}");
    let why = out.stopped.as_deref().unwrap_or_default();
    assert!(why.starts_with("stopped at the cap"), "{why}");
    assert_eq!(r.jev.seen().len(), 1, "the second was not asked");
    let touched: Vec<String> = timeline(&r.core, "topic:ledger")
        .into_iter()
        .map(|t| t.exchange)
        .collect();
    assert_eq!(touched, vec!["turn_a".to_string()]);
    let rest = run(&r.core, params(false, None)).await;
    assert_eq!((rest.touched_before, rest.asked), (1, 1), "{rest:?}");
}

/// The judge's day budget is not gone around: a day with no room left asks
/// nothing and says why, whatever the cap.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_days_budget_stops_the_run_before_a_call() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig_with(jev, Vec::new(), |c| {
        c.judge.shadow_limit_usd_per_day = 0.000_001
    });
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    said(&r.core, &sid, "turn_old", SAID, 5_000);
    let out = run(&r.core, params(false, Some(100.0))).await;
    assert_eq!((out.asked, out.left), (0, 1), "{out:?}");
    let why = out.stopped.as_deref().unwrap_or_default();
    assert!(why.contains("budget for today"), "{why}");
    assert!(r.jev.seen().is_empty());
    assert!(timeline(&r.core, "topic:ledger").is_empty());
}

/// Only an opening the live point would have asked of is asked: a wake's,
/// Theseus's own message, a slash command, a task's session and an empty
/// message are counted and left.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_a_persons_own_opening_message_is_asked() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    let c = &r.core;
    let own = Node::user(&sid, Some("turn_own"), "self:wake", SAID);
    old_exchange(c, &sid, "turn_own", &own, 1_000);
    said(c, &sid, "turn_slash", "/tasks 2", 2_000);
    said(c, &sid, "turn_empty", "   ", 3_000);
    let mut task = SessionRecord::new(SessionKind::Conversation, None);
    task.task = Some(crate::session::TaskOf {
        parent_session: sid.clone(),
        parent_execution: "exe_parent".into(),
        by: "call_parent".into(),
        target: None,
        arrangement: None,
        check: None,
        notify_done: false,
    });
    c.store.put_session(&task.session_id, &task).unwrap();
    said(c, &task.session_id, "turn_task", SAID, 4_000);
    let wake = Node::user(&sid, Some("turn_wake"), "test", SAID);
    old_exchange(c, &sid, "turn_wake", &wake, 5_000);
    let mut e = exchange::exchange(&c.store, "turn_wake").unwrap().unwrap();
    e.opened_by = Opener::Wake;
    c.store
        .append(&[exchange::exchange_record(&e).unwrap()])
        .unwrap();
    said(c, &sid, "turn_person", SAID, 6_000);
    let out = run(c, params(false, None)).await;
    assert_eq!(
        (out.exchanges, out.no_message, out.to_ask, out.asked),
        (6, 5, 1, 1),
        "{out:?}"
    );
    let touched: Vec<String> = timeline(c, "topic:ledger")
        .into_iter()
        .map(|t| t.exchange)
        .collect();
    assert_eq!(touched, vec!["turn_person".to_string()]);
}

/// Where the ontology's judge does not act (`[ontology] judge` off), the
/// live point writes no touch, and neither does this: nothing is asked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nothing_is_asked_where_the_ontologys_judge_does_not_act() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig_with(jev, Vec::new(), |c| c.ontology.judge = false);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    said(&r.core, &sid, "turn_old", SAID, 5_000);
    let out = run(&r.core, params(false, None)).await;
    assert_eq!(
        (out.not_acting, out.to_ask, out.asked),
        (1, 0, 0),
        "{out:?}"
    );
    assert!(r.jev.seen().is_empty());
}

/// KEY: the command is the owner's in person. An agent's process at his
/// terminal is refused, ledgered, and asks nothing; so is a chat surface.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_agents_process_is_refused_and_asks_nothing() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    said(&r.core, &sid, "turn_old", SAID, 5_000);
    let agent = surface("sock#2", Surface::Cli).behind(Behind::Agent("claude".into()));
    let discord = crate::approval::Client::new("discord", Surface::Discord);
    for (who, client) in [("agent", agent), ("discord", discord)] {
        for dry_run in [true, false] {
            let e = rpc_as(
                &r.core,
                client.clone(),
                method::EXCHANGE_TOPICS,
                json!({"dry_run": dry_run}),
            )
            .await
            .expect_err(who);
            assert_eq!(e.code, error_code::REFUSED, "{who}: {e:?}");
        }
    }
    let refused = of_kind(&r.core, LedgerKind::ApprovalRefused);
    assert_eq!(refused.len(), 4, "the agent's two and Discord's: {refused:?}");
    assert!(r.jev.seen().is_empty(), "no call");
    assert!(timeline(&r.core, "topic:ledger").is_empty());
    assert!(of_kind(&r.core, LedgerKind::JudgeCall).is_empty());
}

/// KEY: a message's words reach Jev and nothing else here: not the
/// command's answer, a dry run's or a run's, and not one ledger row the run
/// wrote.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_word_of_a_message_is_in_the_answer_or_the_rows() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    jev.script_onto("topic_name", "n1", 0.9);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    said(&r.core, &sid, "turn_old", SAID, 5_000);
    let mut answers = String::new();
    for dry_run in [true, false] {
        let v = rpc_as(
            &r.core,
            surface("sock#1", Surface::Cli),
            method::EXCHANGE_TOPICS,
            json!({"dry_run": dry_run}),
        )
        .await
        .unwrap();
        answers.push_str(&v.to_string());
    }
    flush(&r.core).await;
    let sent: String = r.jev.seen().iter().map(|s| s.body.to_string()).collect();
    assert!(sent.contains(WORDS[0]), "Jev was asked about the message");
    assert_eq!(timeline(&r.core, "topic:ledger").len(), 1);
    let rows = format!("{:?}", ledger(&r.core));
    for w in WORDS {
        assert!(!answers.contains(w), "{w} in the answer: {answers}");
        assert!(!rows.contains(w), "{w} in a row");
    }
}

/// KEY: a failed, malformed or drifted answer's row keeps its class and its
/// accounting, and no word the answer brought back. Jev's server says the
/// message's words in its error (500 and 400), a malformed answer chooses
/// them and names them as its model, a drifted one names them as its model,
/// and a rate header carries them each time. Every row is read.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_word_an_answer_brings_back_is_in_a_row() {
    let cases = [
        ("server", FakeMode::Says(500, SAID.into()), "server"),
        ("refused", FakeMode::Says(400, SAID.into()), "invalid_request"),
        ("malformed", FakeMode::Says(200, SAID.into()), "malformed"),
        ("drifted", FakeMode::Up, ""),
    ];
    for (case, mode, class) in cases {
        let jev = FakeJev::start().unwrap();
        jev.script_onto("topic", "c1", 0.9);
        jev.set_mode(mode);
        if class.is_empty() {
            jev.answer_as(Some(SAID));
        }
        let r = rig(jev);
        ledger_topic(&r.core);
        let sid = bound(&r.core);
        said(&r.core, &sid, "turn_old", SAID, 5_000);
        let out = run(&r.core, params(false, None)).await;
        assert_eq!(r.jev.seen().len(), 1, "{case}: one call");
        let rows = format!("{:?}", ledger(&r.core));
        for w in WORDS {
            assert!(!rows.contains(w), "{case}: {w} in a row: {rows}");
        }
        assert_eq!(
            (out.asked, out.answered, out.touches),
            (1, 0, 0),
            "{case}: {out:?}"
        );
        let calls = of_kind(&r.core, LedgerKind::JudgeCall);
        let [call] = calls.as_slice() else {
            panic!("{case}: one judgment: {calls:?}");
        };
        let d = &call.data;
        match class {
            "" => assert_eq!(
                (&d["outcome"]["outcome"], &d["model_drift"], &d["answered_by"]),
                (&json!("answered"), &json!(true), &Value::Null),
                "{case}: {d}"
            ),
            _ => assert_eq!(
                (&d["outcome"]["class"], &d["outcome"]["error"]["class"]),
                (&json!(class), &json!(class)),
                "{case}: {d}"
            ),
        }
        assert!(d["reserve_micros"].as_u64().is_some_and(|m| m > 0), "{case}: {d}");
        assert_eq!(d["rate_limit"], json!({}), "{case}: {d}");
    }
}

/// KEY (once): a second run while one goes is refused in words, a dry run's
/// too; it asks nothing and spends nothing, and the first goes on.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_run_while_one_goes_is_refused_and_asks_nothing() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    jev.set_mode(FakeMode::Held);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    said(&r.core, &sid, "turn_old", SAID, 5_000);
    let core = r.core.clone();
    let first = tokio::spawn(async move { command(&core, false).await });
    seen(&r.jev, 1).await;
    for dry_run in [false, true] {
        let second = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            command(&r.core, dry_run),
        )
        .await
        .expect("the second is answered at once");
        let e = second.expect_err("the second is refused");
        assert_eq!(e.code, error_code::LIMIT, "{e:?}");
        assert!(e.message.contains("already being filed"), "{e:?}");
    }
    assert_eq!(r.jev.seen().len(), 1, "the second asked nothing");
    r.jev.set_mode(FakeMode::Up);
    r.jev.release();
    let v = first.await.unwrap().unwrap();
    let out: ExchangeTopicsResult = serde_json::from_value(v).unwrap();
    assert_eq!((out.asked, out.touches), (1, 1), "{out:?}");
    assert_eq!(r.jev.seen().len(), 1);
}

/// KEY (once): a run returns only once its answers' rows are written, so a
/// rerun at once, with no flush between them, asks nothing of an exchange
/// whose answer touched nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rerun_at_once_asks_nothing_of_an_answer_that_touched_nothing() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "none", 0.9);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    said(&r.core, &sid, "turn_old", SAID, 5_000);
    let first = r.core.topics_history(&params(false, None)).await.unwrap();
    assert_eq!((first.asked, first.filed), (1, 0), "{first:?}");
    assert_eq!(
        of_kind(&r.core, LedgerKind::JudgeCall).len(),
        1,
        "the answer's row is written before the run returns"
    );
    let again = r.core.topics_history(&params(false, None)).await.unwrap();
    assert_eq!(r.jev.seen().len(), 1, "no second call");
    assert_eq!(
        (again.asked_before, again.to_ask, again.asked),
        (1, 0, 0),
        "{again:?}"
    );
}

/// KEY: a dry run that cannot price an exchange says so and fails, through
/// the RPC: an unpriced model, and a client that cannot be built. Never a
/// worst case of $0; nothing asked, nothing written.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dry_run_that_cannot_price_says_so_and_fails() {
    for case in ["unpriced", "unbuilt"] {
        let jev = FakeJev::start().unwrap();
        jev.script_onto("topic", "c1", 0.9);
        let r = rig(jev);
        ledger_topic(&r.core);
        let sid = bound(&r.core);
        said(&r.core, &sid, "turn_old", SAID, 5_000);
        let judge = &r.core.runner.judge;
        match case {
            "unpriced" => {
                let onto = crate::judge::onto::ONTO_PACK;
                let mut pack = (*judge.pack(onto).unwrap()).clone();
                pack.jev_model = "jev-0.0.0-unpriced".into();
                judge.stand_in(onto, pack);
            }
            _ => judge
                .unbuilt
                .store(true, std::sync::atomic::Ordering::SeqCst),
        }
        let e = command(&r.core, true)
            .await
            .expect_err("no estimate is no answer");
        let why = match case {
            "unpriced" => "no estimate: the judge's model has no price",
            _ => "no estimate: the judge's client could not be built",
        };
        assert!(e.message.contains(why), "{case}: {e:?}");
        assert!(r.jev.seen().is_empty(), "{case}: no call");
        assert!(of_kind(&r.core, LedgerKind::JudgeCall).is_empty(), "{case}");
    }
}

/// KEY: a category is read as it stands when the touch is written: one
/// retired while the call was out gets no touch.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_category_retired_while_the_call_is_out_gets_no_touch() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    jev.set_mode(FakeMode::Held);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    said(&r.core, &sid, "turn_old", SAID, 5_000);
    let core = r.core.clone();
    let running = tokio::spawn(async move { core.topics_history(&params(false, None)).await });
    seen(&r.jev, 1).await;
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    let retired = o.category(&id("topic:ledger")).unwrap().retired(9_000);
    operator_writes(&r.core, vec![Record::Category(retired)]);
    r.jev.set_mode(FakeMode::Up);
    r.jev.release();
    let out = running.await.unwrap().unwrap();
    assert_eq!(
        (out.asked, out.answered, out.touches),
        (1, 1, 0),
        "{out:?}"
    );
    assert!(timeline(&r.core, "topic:ledger").is_empty());
}

/// KEY: whether the ontology's judge acts is asked again before each call
/// and before an answer's touches: a pack rolled back while a call is out
/// gets that answer no touch, and the next exchange is not asked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pack_rolled_back_mid_run_asks_and_touches_no_more() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    jev.set_mode(FakeMode::Held);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    said(&r.core, &sid, "turn_a", SAID, 5_000);
    said(&r.core, &sid, "turn_b", "and the Lantern Ledger too", 6_000);
    let core = r.core.clone();
    let running = tokio::spawn(async move { core.topics_history(&params(false, None)).await });
    seen(&r.jev, 1).await;
    rolled_back(&r.core);
    r.jev.set_mode(FakeMode::Up);
    r.jev.release();
    let out = running.await.unwrap().unwrap();
    assert_eq!(r.jev.seen().len(), 1, "the second was not asked");
    assert_eq!(
        (out.to_ask, out.asked, out.not_acting, out.touches),
        (2, 1, 1, 0),
        "{out:?}"
    );
    assert!(timeline(&r.core, "topic:ledger").is_empty());
}

/// KEY: a recorded answer's missing touches are written again only where
/// the judge acts now, and not once the daemon is stopping.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_recorded_answer_is_not_touched_after_a_rollback_or_a_stop() {
    for case in ["rolled back", "stopping"] {
        let jev = FakeJev::start().unwrap();
        jev.script_onto("topic", "c1", 0.9);
        let r = rig(jev);
        ledger_topic(&r.core);
        let sid = bound(&r.core);
        said(&r.core, &sid, "turn_old", SAID, 5_000);
        run(&r.core, params(false, None)).await;
        untouch(&r.core, "turn_old");
        match case {
            "rolled back" => rolled_back(&r.core),
            _ => r.core.outbox.stop_sending(),
        }
        let again = r.core.topics_history(&params(false, None)).await.unwrap();
        assert_eq!(r.jev.seen().len(), 1, "{case}: no call");
        assert_eq!((again.asked_before, again.touches), (1, 0), "{case}: {again:?}");
        assert!(timeline(&r.core, "topic:ledger").is_empty(), "{case}");
        if case == "stopping" {
            let why = again.stopped.as_deref().unwrap_or_default();
            assert!(why.starts_with("the daemon is stopping"), "{why}");
        }
    }
}

/// The day's budget as `judge/spend.rs` has it, for this job's shadow
/// calls: shadow's line is 70% of `[judge] shadow_limit_usd_per_day`, a call
/// past it is still made and the owner told (`judge.over_line`), and the
/// stop is ten times the line (`HARD_MULTIPLE`). A limit of one call's worst
/// case puts the call past the line and under the stop: it is made, with
/// the warning. A tenth of it puts the stop under the call: none is made.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_days_line_warns_and_ten_times_it_stops() {
    let worst = {
        let jev = FakeJev::start().unwrap();
        let r = rig(jev);
        ledger_topic(&r.core);
        let sid = bound(&r.core);
        said(&r.core, &sid, "turn_old", SAID, 5_000);
        r.core.topics_history(&params(true, None)).await.unwrap().worst_usd
    };
    assert!(worst > 0.0);
    for (case, limit_usd, asked) in [("warning", worst, 1), ("stop", worst / 10.0, 0)] {
        let jev = FakeJev::start().unwrap();
        jev.script_onto("topic", "c1", 0.9);
        let r = rig_with(jev, Vec::new(), |c| {
            c.judge.shadow_limit_usd_per_day = limit_usd
        });
        ledger_topic(&r.core);
        let sid = bound(&r.core);
        said(&r.core, &sid, "turn_old", SAID, 5_000);
        let out = r.core.topics_history(&params(false, None)).await.unwrap();
        assert_eq!((out.asked, r.jev.seen().len()), (asked, asked as usize), "{case}: {out:?}");
        let over = of_kind(&r.core, LedgerKind::JudgeOverLine);
        match case {
            "warning" => {
                assert_eq!(out.stopped, None, "{case}: {out:?}");
                assert_eq!(over.len(), 1, "{case}: told at the line: {over:?}");
            }
            _ => {
                let why = out.stopped.as_deref().unwrap_or_default();
                assert!(why.contains("budget for today"), "{case}: {why}");
                assert!(over.is_empty(), "{case}: {over:?}");
            }
        }
    }
}

/// KEY (his decision: his own terminal only): the owner's web UI, an agent's
/// process at the terminal, Discord, MCP and an unnamed connection are each
/// refused, a dry run and a run, in words; nothing is asked and nothing
/// written. Each is ledgered but MCP's: the MCP server's own list refuses it
/// before the method's dispatch (`mcp_server`), so it never reaches this.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_his_own_terminal_runs_it() {
    use crate::approval::Client;
    let jev = FakeJev::start().unwrap();
    jev.script_onto("topic", "c1", 0.9);
    let r = rig(jev);
    ledger_topic(&r.core);
    let sid = bound(&r.core);
    said(&r.core, &sid, "turn_old", SAID, 5_000);
    let callers = [
        ("the owner's web UI", Client::new("ws#1", Surface::Web)),
        (
            "an agent at the terminal",
            surface("sock#2", Surface::Cli).behind(Behind::Agent("claude".into())),
        ),
        ("discord", Client::new("discord", Surface::Discord)),
        ("mcp", Client::new("mcp", Surface::Mcp)),
        ("an unnamed connection", Client::new("conn#9", Surface::Unnamed)),
    ];
    for (who, client) in callers {
        for dry_run in [true, false] {
            let e = rpc_as(
                &r.core,
                client.clone(),
                method::EXCHANGE_TOPICS,
                json!({"dry_run": dry_run}),
            )
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
    assert_eq!(refused.len(), 8, "each refusal ledgered, MCP's apart: {refused:?}");
    let web = format!("{:?}", refused[0].data);
    assert!(web.contains("own terminal alone"), "{web}");
    assert!(r.jev.seen().is_empty(), "no call");
    assert!(of_kind(&r.core, LedgerKind::JudgeCall).is_empty());
    assert!(timeline(&r.core, "topic:ledger").is_empty());
}
