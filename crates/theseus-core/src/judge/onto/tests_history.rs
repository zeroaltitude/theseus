//! The history filed under topics once (issue 46; `history.rs`), through a
//! whole core and the fake Jev, over invented exchanges in a temporary store:
//! the touches, what it never writes, the resume, the cap and the day's
//! budget.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted};
use theseus_ontology::{Category, CategoryId, MemberList, Membership, Origin, Record};
use theseus_protocol::exchange::{ExchangeTopicsParams, ExchangeTopicsResult};
use theseus_protocol::{LedgerKind, SessionKind};
use theseus_store::Store as _;

use crate::exchange::{self, Exchange, Opener};
use crate::ledger::LedgerRow;
use crate::node::Node;
use crate::session::SessionRecord;
use crate::tests_onto::{bound, rig, rig_with};
use crate::Core;

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
