//! A task's limits that notify (theseus-usei; `turn/budget_step.rs`): at the
//! spend limit the turn goes on and one notice posts to the session's place,
//! one more at twice it, never one per call; a call whose reservation passes
//! the limit goes out; at `max_loops` the turn goes on with a notice; with
//! `spend_limit_mode = "ask"` or `max_loops_mode = "end"`, today's behaviour.

use std::sync::Arc;

use serde_json::json;
use theseus_protocol::{SessionKind, TurnSubmitResult, Usage};

use crate::bus::EventSink;
use crate::config::{MaxLoopsMode, SpendLimitMode};
use crate::provider::{FakeProvider, Scripted};
use crate::session::SessionRecord;
use crate::store::Store;
use crate::turn::TurnRequest;
use crate::{Config, Core};

const PLACE: &str = "dm:4417";
/// Where its posts go.
const TARGET: &str = "discord:dm:4417";

/// A call that diffs, billed 2,000 tokens in and 30,000 out: about $0.304 on
/// the template's live profile, whose reservation (its 128,000-token output
/// cap) is past $1.28.
fn diffing(n: usize) -> Scripted {
    Scripted::Billed {
        usage: Usage {
            input_tokens: 2_000,
            output_tokens: 30_000,
            ..Usage::default()
        },
        then: Box::new(Scripted::tools(
            "Diffing.",
            &[(
                &format!("t{n}"),
                "text_diff",
                json!({"a": "x\n", "b": "y\n"}),
            )],
        )),
    }
}

struct Ran {
    core: Arc<Core>,
    fake: Arc<FakeProvider>,
    sid: String,
    res: TurnSubmitResult,
    _dir: tempfile::TempDir,
}

/// The template, with every profile's `max_loops = 40` made `to`.
fn loops(to: &str) -> Config {
    let text = Config::EXAMPLE_TOML
        .replace("max_loops_mode = \"notify\"", "")
        .replace("max_loops = 40", to);
    Config::parse(&text).unwrap().0
}

async fn one_turn(script: Vec<Scripted>, cfg: impl FnOnce(&mut Config)) -> Ran {
    turn_on(Config::example(), script, cfg).await
}

async fn turn_on(mut c: Config, script: Vec<Scripted>, cfg: impl FnOnce(&mut Config)) -> Ran {
    let dir = tempfile::tempdir().unwrap();
    c.server.state_dir = dir.path().to_string_lossy().into_owned();
    c.tools.roots = vec![];
    cfg(&mut c);
    let store = Store::open(&dir.path().join("store")).unwrap();
    let fake = Arc::new(FakeProvider::scripted(script));
    let core = Core::build(crate::rpc::Parts::for_tests(c, fake.clone(), store)).unwrap();
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    core.outbox.bind_place(PLACE, &rec.session_id).unwrap();
    let sid = rec.session_id.clone();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let res = core
        .runner
        .run(TurnRequest {
            prompt: None,
            session: rec,
            input: Some("diff these, again and again".into()),
            target,
            sink: EventSink::new(core.bus.clone(), &sid, None),
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
        })
        .await
        .unwrap();
    Ran {
        core,
        fake,
        sid,
        res,
        _dir: dir,
    }
}

impl Ran {
    fn rows(&self, kind: &str) -> Vec<serde_json::Value> {
        let rows: Vec<(u64, crate::ledger::LedgerRow)> = self.core.store.ledger_tail(5000).unwrap();
        rows.into_iter()
            .filter(|(_, r)| r.kind == kind && r.session_id.as_deref() == Some(self.sid.as_str()))
            .map(|(_, r)| r.data)
            .collect()
    }

    /// The notices posted to the place, in order.
    fn notices(&self) -> Vec<String> {
        self.core
            .outbox
            .open_for(TARGET)
            .iter()
            .filter(|a| crate::outbox::kind_of(a) == "notice")
            .map(|a| {
                crate::outbox::body_of(a)["text"]
                    .as_str()
                    .unwrap_or("")
                    .to_string()
            })
            .collect()
    }
}

/// Four calls of about $0.304 under a $0.50 limit that notifies: the second
/// takes the spend past the limit, the fourth past twice it. Every call goes
/// out (each one's reservation alone is past the limit), the turn ends as the
/// model ends it, and exactly two notices post, one at each multiple, each a
/// `budget.reached` row.
#[tokio::test]
async fn at_the_spend_limit_the_turn_goes_on_with_one_notice_and_one_more_at_twice_it() {
    let script = vec![
        diffing(1),
        diffing(2),
        diffing(3),
        diffing(4),
        Scripted::text("Done."),
    ];
    let ran = one_turn(script, |c| c.kernel.spend_limit_usd = 0.50).await;
    assert_eq!(
        (ran.res.stop_reason.as_str(), ran.res.loops),
        ("no_tool_calls", 5),
        "{:?}",
        ran.res
    );
    assert_eq!(ran.fake.requests().len(), 5, "every call went out");
    let reached = ran.rows("budget.reached");
    let multiples: Vec<u64> = reached
        .iter()
        .map(|r| r["multiple"].as_u64().unwrap())
        .collect();
    assert_eq!(multiples, [1, 2], "{reached:?}");
    assert_eq!(reached[0]["limit_usd"], 0.5);
    assert_eq!(reached[0]["mode"], "notify");
    assert_eq!(reached[0]["posted"], true);
    let notices = ran.notices();
    assert_eq!(notices.len(), 2, "{notices:?}");
    assert!(
        notices[0].contains("past its $0.50 limit"),
        "{}",
        notices[0]
    );
    assert!(
        notices[0].contains(&format!("theseus stop {}", ran.sid)),
        "{}",
        notices[0]
    );
    assert!(
        notices[1].contains("past 2 times its $0.50 limit"),
        "{}",
        notices[1]
    );
    // Each call reserved its worst case, past the limit, and it was recorded.
    let planned = ran.rows("action.planned");
    assert!(
        planned
            .iter()
            .filter(|r| r["tool"] == "provider.messages")
            .all(|r| r["reserved_usd"].as_f64().unwrap() > 0.5),
        "{planned:?}"
    );
    assert!(ran.rows("budget.asked").is_empty(), "nothing asked");
}

/// `spend_limit_mode = "ask"`: today's behaviour. The second call's
/// reservation does not fit, the turn parks on the budget question, and no
/// notice posts.
#[tokio::test]
async fn ask_mode_parks_on_the_budget_question_as_before() {
    let script = vec![diffing(1), diffing(2), Scripted::text("Done.")];
    let ran = one_turn(script, |c| {
        c.kernel.spend_limit_usd = 1.40;
        c.kernel.spend_limit_mode = SpendLimitMode::Ask;
    })
    .await;
    assert_eq!((ran.res.stop_reason.as_str(), ran.res.loops), ("budget", 2));
    assert_eq!(
        ran.fake.requests().len(),
        1,
        "the second call never went out"
    );
    assert_eq!(ran.rows("budget.asked").len(), 1);
    assert!(ran.rows("budget.reached").is_empty());
    assert!(ran.notices().is_empty());
}

/// `max_loops = 2` that notifies: six loops, five of them with a call, run
/// to the model's end; a notice after the 2nd and the 4th loop, none after
/// the 6th, which ends the turn.
#[tokio::test]
async fn at_max_loops_the_turn_goes_on_with_a_notice_at_each_multiple() {
    let script = (1..=5)
        .map(|n| {
            Scripted::tools(
                "Diffing.",
                &[(
                    &format!("t{n}"),
                    "text_diff",
                    json!({"a": "x\n", "b": "y\n"}),
                )],
            )
        })
        .chain([Scripted::text("Done.")])
        .collect();
    let ran = turn_on(loops("max_loops = 2"), script, |_| {}).await;
    assert_eq!(
        (ran.res.stop_reason.as_str(), ran.res.loops),
        ("no_tool_calls", 6)
    );
    let reached = ran.rows("loop.cap_reached");
    let at: Vec<(u64, u64)> = reached
        .iter()
        .map(|r| {
            (
                r["loops"].as_u64().unwrap(),
                r["multiple"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(at, [(2, 1), (4, 2)], "{reached:?}");
    let notices = ran.notices();
    assert_eq!(notices.len(), 2, "{notices:?}");
    assert!(notices[0].contains("2 loops"), "{}", notices[0]);
    assert!(notices[0].contains("max_loops_mode"), "{}", notices[0]);
}

/// `max_loops_mode = "end"`: today's behaviour, the turn ends at the cap.
#[tokio::test]
async fn end_mode_ends_the_turn_at_max_loops_as_before() {
    let script = (1..=5)
        .map(|n| {
            Scripted::tools(
                "Diffing.",
                &[(
                    &format!("t{n}"),
                    "text_diff",
                    json!({"a": "x\n", "b": "y\n"}),
                )],
            )
        })
        .collect();
    let ran = turn_on(
        loops("max_loops = 2\nmax_loops_mode = \"end\""),
        script,
        |_| {},
    )
    .await;
    let (live, _) = ran.core.live_profile();
    assert_eq!(
        ran.core.cfg.profile(&live).unwrap().max_loops_mode,
        MaxLoopsMode::End
    );
    assert_eq!(
        (ran.res.stop_reason.as_str(), ran.res.loops),
        ("max_loops", 2)
    );
    assert!(ran.rows("loop.cap_reached").is_empty());
    assert!(ran.notices().is_empty());
}

/// Speech past a notifying limit still fits (45b's check): the limit stops
/// no voice turn's transcript or reply; under `ask` it does, as before.
#[tokio::test]
async fn speech_past_a_notifying_limit_fits_and_past_an_asking_one_does_not() {
    for (mode, fits) in [(SpendLimitMode::Notify, true), (SpendLimitMode::Ask, false)] {
        let ran = turn_on(Config::example(), vec![Scripted::text("Hello.")], |c| {
            c.kernel.spend_limit_usd = 0.001;
            c.kernel.spend_limit_mode = mode;
        })
        .await;
        assert_eq!(
            ran.core.speech_fits(&ran.sid, 5_000_000).is_ok(),
            fits,
            "{mode:?}"
        );
    }
}
