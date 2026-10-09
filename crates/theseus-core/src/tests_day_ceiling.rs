//! The daemon's day ceiling through the core (theseus-kp20): the key and its
//! check; a turn whose call the ceiling refuses makes no call and ends with
//! the words, once noticed to the owner's DM and never to a shared place, and
//! never posted with no DM bound; a restart reads the day back and the
//! ceiling still holds, with no second notice; the judge skips its call;
//! `budget.list` shows the day; and the defaults leave a session's limit as it
//! was.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use theseus_protocol::{SessionKind, Usage};

use crate::bus::EventSink;
use crate::places::BoundPlace;
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::{TurnError, TurnRequest};
use crate::{Config, Core};

const PLACE: &str = "dm:5150";
const OWNER_DM: &str = "discord:dm:5150";

fn billed(text: &str) -> Scripted {
    Scripted::Billed {
        usage: Usage {
            input_tokens: 2_000,
            output_tokens: 1_000,
            ..Usage::default()
        },
        then: Box::new(Scripted::text(text)),
    }
}

struct Rig {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    dir: tempfile::TempDir,
}

fn rig(script: Vec<Scripted>, cfg: impl FnOnce(&mut Config)) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let (core, fake) = build(dir.path(), script, cfg);
    Rig { core, fake, dir }
}

fn build(
    dir: &std::path::Path,
    script: Vec<Scripted>,
    cfg: impl FnOnce(&mut Config),
) -> (Arc<Core>, Arc<FakeProvider>) {
    let mut c = Config::example();
    c.server.state_dir = dir.to_string_lossy().into_owned();
    c.tools.roots = vec![];
    cfg(&mut c);
    let store = Store::open(&dir.join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts::for_tests(c, fake.clone(), store)).unwrap();
    (core, fake)
}

/// A DM with the owner bound, as the binding's start tells it.
fn bind_dm(core: &Core) {
    core.runner.place_rule.bind(vec![BoundPlace {
        target: OWNER_DM.into(),
        name: "DM @owner".into(),
        private: true,
        guild: None,
        ceiling: None,
    }]);
}

async fn turn(
    core: &Arc<Core>,
    input: &str,
) -> (String, anyhow::Result<theseus_protocol::TurnSubmitResult>) {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    core.outbox.bind_place(PLACE, &rec.session_id).unwrap();
    let sid = rec.session_id.clone();
    let res = turn_in(core, &sid, input).await;
    (sid, res)
}

/// A turn in the session `sid`.
async fn turn_in(
    core: &Arc<Core>,
    sid: &str,
    input: &str,
) -> anyhow::Result<theseus_protocol::TurnSubmitResult> {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    core.runner
        .run(TurnRequest {
            prompt: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink: EventSink::new(core.bus.clone(), sid, None),
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
}

fn rows(core: &Core, kind: &str) -> Vec<crate::ledger::LedgerRow> {
    let rows: Vec<(u64, crate::ledger::LedgerRow)> = core.store.ledger_tail(5000).unwrap();
    rows.into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == kind)
        .collect()
}

/// The posts of `kind` waiting in the outbox for the owner.
fn posts(core: &Core, kind: &str) -> Vec<serde_json::Value> {
    core.outbox
        .open_for(crate::outbox::OPERATOR_TARGET)
        .iter()
        .filter(|a| crate::outbox::kind_of(a) == kind)
        .map(|a| crate::outbox::body_of(a).clone())
        .collect()
}

/// `[kernel] daily_spend_ceiling_usd`: $200 with no line, shown by
/// `theseusd config`, carried by the template, and refused at 0, below it,
/// and as NaN with `spend_limit_usd`'s words.
#[test]
fn the_key_defaults_to_200_and_refuses_what_is_not_a_dollar_amount() {
    let (c, w) = Config::parse("[secrets]\nanthropic_api_key = \"op://v/i/f\"\n").unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(c.kernel.daily_spend_ceiling_usd, 200.0);
    assert_eq!(
        c.kernel.to_kernel_config().daily_ceiling_micros,
        200_000_000
    );
    let shown = toml::to_string(&c.kernel).unwrap();
    assert!(shown.contains("daily_spend_ceiling_usd = 200.0"), "{shown}");
    assert!(
        Config::EXAMPLE_TOML.contains("\ndaily_spend_ceiling_usd = 200.0 "),
        "the template carries it"
    );
    assert_eq!(Config::example().kernel.daily_spend_ceiling_usd, 200.0);
    for bad in ["0.0", "-5.0", "nan"] {
        let doc = format!(
            "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n[kernel]\ndaily_spend_ceiling_usd = {bad}\n"
        );
        let err = format!("{:#}", Config::parse(&doc).unwrap_err());
        assert!(
            err.contains(&format!(
                "kernel.daily_spend_ceiling_usd = {} must be a dollar amount above zero",
                bad.parse::<f64>().unwrap()
            )),
            "{bad}: {err}"
        );
    }
    let (c, _) = Config::parse(
        "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n\n[kernel]\ndaily_spend_ceiling_usd = 350.0\n",
    )
    .unwrap();
    assert_eq!(
        c.kernel.to_kernel_config().daily_ceiling_micros,
        350_000_000
    );
}

/// A ceiling below the call's reservation: the turn's call is never made
/// (the fake provider is never asked), the turn fails `daily_ceiling` with
/// the words, the day's `spend.ceiling` row is written once, and with a DM
/// bound one post goes to the owner, never to the session's place; a second
/// refusal that day posts nothing more.
#[tokio::test]
async fn a_turn_at_the_ceiling_makes_no_call_and_ends_with_the_words() {
    let r = rig(vec![billed("never")], |c| {
        c.kernel.daily_spend_ceiling_usd = 0.01
    });
    bind_dm(&r.core);
    let (_, res) = turn(&r.core, "hello").await;
    let err = res.unwrap_err();
    let te = err.downcast_ref::<TurnError>().expect("a turn's failure");
    assert_eq!(te.class, "daily_ceiling");
    let words = format!("{:#}", te.source);
    assert!(
        words.starts_with("today's spend reached the $0.01 daily ceiling")
            && words.contains(
                "the owner can raise `[kernel] daily_spend_ceiling_usd` or wait until midnight"
            ),
        "{words}"
    );
    assert!(r.fake.requests().is_empty(), "no call was made");
    let failed = rows(&r.core, "turn.failed");
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].data["reason"], "daily_ceiling");
    let ceiling = rows(&r.core, "spend.ceiling");
    assert_eq!(ceiling.len(), 1, "{ceiling:?}");
    assert_eq!(ceiling[0].data["what"], "turn");
    assert_eq!(ceiling[0].data["limit_usd"], 0.01);
    assert_eq!(ceiling[0].data["posted"], true);
    let p = posts(&r.core, "spend_ceiling");
    assert_eq!(p.len(), 1, "{p:?}");
    let text = p[0]["text"].as_str().unwrap();
    assert!(
        text.contains("$0.01 daily ceiling") && text.contains("daily_spend_ceiling_usd"),
        "{text}"
    );
    // The session's place hears the turn failed, with the words.
    let failed: Vec<serde_json::Value> = r
        .core
        .outbox
        .open_for(OWNER_DM)
        .iter()
        .filter(|a| crate::outbox::kind_of(a) == "failed")
        .map(|a| crate::outbox::body_of(a).clone())
        .collect();
    assert_eq!(failed.len(), 1, "{failed:?}");
    assert_eq!(failed[0]["class"], "daily_ceiling");
    assert!(
        failed[0]["error"]
            .as_str()
            .unwrap()
            .contains("daily ceiling"),
        "{failed:?}"
    );
    // The owner's post is no post to the session's place.
    assert!(r
        .core
        .outbox
        .open_for(OWNER_DM)
        .iter()
        .all(|a| crate::outbox::kind_of(a) != "spend_ceiling"));
    // The session keeps a note of it, which the model's next context reads.
    let sid = &failed[0]["session_id"];
    let nodes = r.core.store.transcript(sid.as_str().unwrap()).unwrap();
    let (_, last) = nodes.last().unwrap();
    assert_eq!(last.author.as_deref(), Some("harness:day_ceiling"));
    let note = match &last.body {
        crate::node::Body::UserMessage { text, .. } => text.clone(),
        other => panic!("{other:?}"),
    };
    assert!(note.contains("daily_spend_ceiling_usd"), "{note}");
    let c = r.core.kernel.day_ceiling();
    c.set_limit(200_000_000);
    turn_in(&r.core, sid.as_str().unwrap(), "the next day")
        .await
        .unwrap();
    let sent = serde_json::to_string(&r.fake.requests()[0].messages).unwrap();
    assert!(
        sent.contains("this turn's model call was not made"),
        "{sent}"
    );
    c.set_limit(10_000);
    // A second refusal the same day writes no second row or post.
    let (_, res) = turn(&r.core, "again").await;
    assert!(res.is_err());
    assert_eq!(r.fake.requests().len(), 1, "only the next day's call");
    assert_eq!(rows(&r.core, "spend.ceiling").len(), 1);
    assert_eq!(posts(&r.core, "spend_ceiling").len(), 1);
    // budget.list says it, and `theseus budgets` prints it.
    let day = r
        .core
        .budget_list()
        .unwrap()
        .day_ceiling
        .expect("the day's block");
    assert!(day.reached && day.reached_at_ms.is_some());
    assert_eq!(day.ceiling_usd, 0.01);
}

/// No DM with the owner bound (a headless daemon, the bench): the row says
/// it, and nothing is posted anywhere.
#[tokio::test]
async fn with_no_dm_bound_the_row_says_it_and_nothing_posts() {
    let r = rig(vec![billed("never")], |c| {
        c.kernel.daily_spend_ceiling_usd = 0.01
    });
    let (_, res) = turn(&r.core, "hello").await;
    assert!(res.is_err());
    let ceiling = rows(&r.core, "spend.ceiling");
    assert_eq!(ceiling.len(), 1);
    assert_eq!(ceiling[0].data["posted"], false);
    assert!(posts(&r.core, "spend_ceiling").is_empty());
}

/// A restart reads today's spend back from the ledger, so the ceiling still
/// holds; and a restart on a day already stopped posts no second notice.
#[tokio::test]
async fn a_restart_reads_the_day_back_and_posts_no_second_notice() {
    let dir = tempfile::tempdir().unwrap();
    let spent = {
        let (core, fake) = build(dir.path(), vec![billed("one")], |_| {});
        let (_, res) = turn(&core, "hello").await;
        res.unwrap();
        assert_eq!(fake.requests().len(), 1);
        let c = core.kernel.day_ceiling();
        let t = c.today(c.now());
        assert!(t.spent > 0);
        t.spent
    };
    // The same store, a ceiling just past the day's spend: the next call's
    // reservation passes it.
    let ceiling = theseus_kernel::micros_to_usd(spent + 1);
    let (core, fake) = build(dir.path(), vec![billed("two")], |c| {
        c.kernel.daily_spend_ceiling_usd = ceiling;
    });
    bind_dm(&core);
    let c = core.kernel.day_ceiling();
    assert_eq!(c.today(c.now()).spent, spent, "read back at the start");
    let (_, res) = turn(&core, "again").await;
    assert_eq!(
        res.unwrap_err().downcast_ref::<TurnError>().unwrap().class,
        "daily_ceiling"
    );
    assert!(fake.requests().is_empty());
    assert_eq!(posts(&core, "spend_ceiling").len(), 1);
    drop(core);
    // A third start, the same day: stopped still, and no second notice.
    let (core, fake) = build(dir.path(), vec![billed("three")], |c| {
        c.kernel.daily_spend_ceiling_usd = ceiling;
    });
    bind_dm(&core);
    let c = core.kernel.day_ceiling();
    assert!(c.today(c.now()).reached());
    let (_, res) = turn(&core, "once more").await;
    assert!(res.is_err());
    assert!(fake.requests().is_empty());
    assert_eq!(rows(&core, "spend.ceiling").len(), 1, "one row a day");
    assert!(posts(&core, "spend_ceiling").len() <= 1, "no second post");
    drop(dir);
}

/// The refusal's remedy works (a join fix of the review): a stopped day,
/// then a higher ceiling and a restart, and the day is no longer stopped:
/// `budget.list` says so, a turn's call is made, and a refusal at the new
/// ceiling is said again (a second row, a second post).
#[tokio::test]
async fn a_raised_ceiling_and_a_restart_lift_the_days_stop() {
    let dir = tempfile::tempdir().unwrap();
    let low = {
        let (core, _fake) = build(dir.path(), vec![], |c| {
            c.kernel.daily_spend_ceiling_usd = 0.000_001;
        });
        bind_dm(&core);
        let (_, res) = turn(&core, "hello").await;
        assert_eq!(
            res.unwrap_err().downcast_ref::<TurnError>().unwrap().class,
            "daily_ceiling"
        );
        assert_eq!(rows(&core, "spend.ceiling").len(), 1);
        core.kernel.day_ceiling().limit()
    };
    // The owner raises the ceiling and restarts: the stop is lifted.
    let (core, fake) = build(dir.path(), vec![billed("one")], |_| {});
    bind_dm(&core);
    let c = core.kernel.day_ceiling();
    assert!(c.limit() > low);
    let t = c.today(c.now());
    assert!(
        !t.reached(),
        "a stop under a lower ceiling is not today's: {t:?}"
    );
    let (_, res) = turn(&core, "again").await;
    res.unwrap();
    assert_eq!(fake.requests().len(), 1, "the call was made");
    // At the new ceiling a refusal is said again.
    c.set_limit(c.today(c.now()).spent + 1);
    let (_, res) = turn(&core, "once more").await;
    assert!(res.is_err());
    assert_eq!(
        rows(&core, "spend.ceiling").len(),
        2,
        "said again at the new ceiling"
    );
    assert_eq!(posts(&core, "spend_ceiling").len(), 2);
    drop(core);
    drop(dir);
}

/// The judge holds every judgment on the day too: at the ceiling its
/// reservation is refused, the judgment skipped (not queued), and the core
/// told; what a judgment settles is spent on the day.
#[test]
fn the_judge_skips_its_call_at_the_ceiling() {
    use crate::judge::spend::{Reserve, ShadowBudget};
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let clock = theseus_kernel::VirtualClock::new(1_000_000);
    let day = Arc::new(theseus_kernel::DayCeiling::new(
        1_000,
        theseus_kernel::TimeZone::UTC,
        clock,
    ));
    let b = ShadowBudget::new(1.0);
    let told = Arc::new(AtomicU32::new(0));
    let t = told.clone();
    b.set_ceiling(
        day.clone(),
        Box::new(move |_| {
            t.fetch_add(1, Ordering::SeqCst);
        }),
    );
    let today = "1970-01-01";
    assert!(matches!(
        b.reserve(&store, today, 600),
        Reserve::Granted(..)
    ));
    assert_eq!(day.today(day.now()).held, 600);
    assert!(matches!(b.reserve(&store, today, 600), Reserve::Paused(..)));
    assert_eq!(told.load(Ordering::SeqCst), 1);
    b.settle(today, 600, 250, true, false);
    let d = day.today(day.now());
    assert_eq!((d.spent, d.held), (250, 0));
}

/// The defaults leave a session's limit as it was: a turn under $200 runs,
/// and the day's block shows its spend, not reached.
#[tokio::test]
async fn the_defaults_leave_the_turn_as_it_was() {
    let r = rig(vec![billed("fine")], |_| {});
    let (_, res) = turn(&r.core, "hello").await;
    res.unwrap();
    assert_eq!(r.fake.requests().len(), 1);
    let day = r.core.budget_list().unwrap().day_ceiling.unwrap();
    assert_eq!(day.ceiling_usd, 200.0);
    assert!(!day.reached && day.spent_usd > 0.0 && day.held_usd == 0.0);
    assert!(rows(&r.core, "spend.ceiling").is_empty());
    drop(r.dir);
}

fn row_at(kind: &str, at: u64, data: serde_json::Value) -> theseus_store::NewRecord {
    let r = crate::ledger::LedgerRow {
        at_unix_ms: at,
        kind: kind.into(),
        session_id: Some("ses_seed".into()),
        turn_id: None,
        data,
    };
    theseus_store::NewRecord::json(theseus_store::kinds::LEDGER, None, &r).unwrap()
}

/// The start's read counts today's model spend and nothing else: a
/// provider's settled calls (and a booked reservation), speech, a
/// synthesis, a learning run's; never an AWS hand's settle, a row of
/// another kind, or a row from before local midnight; and today's
/// `spend.ceiling` row says the day's notice was taken.
#[test]
fn the_start_reads_todays_model_spend_and_nothing_else() {
    use serde_json::json;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let now = theseus_protocol::now_unix_ms();
    let since = now - 60_000;
    store
        .append(&[
            row_at(
                "action.succeeded",
                now,
                json!({"producer": "provider:anthropic", "cost_usd": 0.5}),
            ),
            row_at(
                "action.failed",
                now,
                json!({"producer": "provider:anthropic", "cost_usd": 0.25}),
            ),
            row_at(
                "action.outcome_unknown",
                now,
                json!({"producer": "earlier", "cost_usd": 0.125, "cost_basis": "reservation"}),
            ),
            row_at(
                "action.succeeded",
                now,
                json!({"producer": "hands:group", "cost_usd": 3.0}),
            ),
            row_at("speech.synthesized", now, json!({"cost_usd": 0.0625})),
            row_at("synthesis.proposed", now, json!({"cost_usd": 0.03125})),
            row_at("judge.replay", now, json!({"cost_usd": 0.015625})),
            row_at("judge.proposal", now, json!({"writer_usd": 0.0078125})),
            row_at("provider.call", now, json!({"cost_usd": 9.0})),
            row_at(
                "action.succeeded",
                since - 1,
                json!({"producer": "provider:anthropic", "cost_usd": 7.0}),
            ),
            row_at("spend.ceiling", now, json!({"day": "today"})),
        ])
        .unwrap();
    let s = crate::day_ceiling::read_today(&store, since, "today", 200_000_000)
        .unwrap()
        .expect("the pages are built");
    assert_eq!(s.seed.spent, 992_188, "{s:?}");
    assert_eq!(s.seed.reached_at_ms, Some(now));
    let other = crate::day_ceiling::read_today(&store, since, "another day", 200_000_000)
        .unwrap()
        .unwrap();
    assert_eq!(other.seed.reached_at_ms, None);
}

/// The start's read on a busy day (theseus-kp20's FAST check): 5,000 cost
/// rows today among 5,000 others, timed. A measurement:
/// `--run-ignored only --no-capture`.
#[test]
#[ignore = "a measurement"]
fn the_starts_read_of_a_busy_day() {
    use serde_json::json;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let now = theseus_protocol::now_unix_ms();
    for _ in 0..20 {
        let mut frame = Vec::new();
        for i in 0..250 {
            frame.push(row_at("action.succeeded", now, json!({"producer": "provider:anthropic", "cost_usd": 0.001, "correlation_id": format!("act_{i}")})));
            frame.push(row_at(
                "turn.started",
                now,
                json!({"turn_id": format!("trn_{i}")}),
            ));
        }
        store.append(&frame).unwrap();
    }
    for pass in 0..3 {
        let t = std::time::Instant::now();
        let s = crate::day_ceiling::read_today(&store, now - 3_600_000, "today", 200_000_000)
            .unwrap()
            .unwrap();
        let took = t.elapsed();
        assert_eq!(s.rows, 5_000);
        println!(
            "pass {pass}: read 5,000 cost rows (of 10,000 today) in {took:?}, spent {} micros",
            s.seed.spent
        );
    }
}
