//! A checkpoint's effort (theseus-53ujg.3): a routed turn whose loops keep
//! failing gets a verdict that keeps its profile and asks more effort, and
//! its next request carries it, past the sticky rule, with no move; the 0.70
//! gate for `xhigh` and `max` still holds, and a checkpoint never lowers a
//! turn's effort. And the floor: a profile a routing class chose never runs
//! below its own effort. The rigs are `tests_turn_checkpoint`'s and
//! `tests_route`'s, on the template's `sonnethi` profile (Sonnet 5.5 at
//! high) that takes no per-message effort, as `tests_effort_sticky`'s does.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted as Jev};

use crate::catalog::CatalogRow;
use crate::config::Effort;
use crate::provider::{FakeProvider, ProviderRequest};
use crate::tests_route::{decided, mode, rig, rig_parts, turn, Rig};
use crate::tests_turn_checkpoint::{checkpoints, loops, ASK};
use crate::Config;

/// The template's `sonnethi` (it carries one since theseus-tqomi) with the live
/// profile `live`, under the rig's own sections; Sonnet 5.5 takes no per-message effort, and every
/// request is long enough for the sticky rule.
fn sonnethi(c: &mut Config, live: &str) {
    let text = Config::EXAMPLE_TOML
        .replace("live = \"sonnet\"\n", &format!("live = \"{live}\"\n"));
    let (mut with, _) = Config::parse(&text).unwrap();
    with.server = c.server.clone();
    with.tools = c.tools.clone();
    with.judge = c.judge.clone();
    with.routing = c.routing.clone();
    with.memory = c.memory.clone();
    with.ontology = c.ontology.clone();
    with.cache.keep_warm_min_tokens = 1;
    let row = CatalogRow {
        per_message_effort: Some(false),
        ..Default::default()
    };
    with.catalog
        .extend([("claude-sonnet-5-5".to_string(), row)]);
    *c = with;
}

/// Jev's answer to `question`, at `confidence`.
fn choice(option: &str, confidence: f64) -> Jev {
    Jev::Choice {
        option: option.into(),
        confidence,
    }
}

/// A conversation whose route reads the message as routine coding, run on
/// `sonnethi` at high (Jev's first effort), and whose checkpoint, on three
/// failed loops, says routine coding again (`checkpoint_mode`) and asks
/// `level` at `confidence`; on `live` as the session's base.
fn routed(
    live: &str,
    checkpoint_mode: &str,
    (level, confidence): (&str, f64),
) -> (Rig, Arc<FakeProvider>, FakeJev) {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "routine_coding", 0.95);
    jev.script("reply_effort", choice("high", 0.95));
    let at = "failing: 3 failed";
    jev.script_when(at, None, "mode", choice(checkpoint_mode, 0.95));
    jev.script_when(at, None, "reply_effort", choice(level, confidence));
    let claude = Arc::new(FakeProvider {
        delay_ms: 60,
        ..FakeProvider::scripted(loops(8))
    });
    let provider = claude.clone();
    let rig = rig_parts(
        Some(&jev),
        4,
        |c| {
            sonnethi(c, live);
            // The gate's line the tests draw between 0.69 and 0.71.
            c.routing.high_effort_confidence = 0.70;
            c.routing.classes.routine_coding.profiles = vec!["sonnethi".into()];
        },
        move |p| {
            p.providers.insert("anthropic".into(), provider);
        },
    );
    (rig, claude, jev)
}

/// The top-level effort each request carried, and that every one went to
/// Sonnet 5.5.
fn levels(claude: &FakeProvider) -> Vec<String> {
    let reqs: Vec<ProviderRequest> = claude.requests();
    assert!(reqs.iter().all(|r| r.model == "claude-sonnet-5-5"));
    reqs.iter()
        .map(|r| {
            r.output_config
                .as_ref()
                .and_then(|o| o["effort"].as_str())
                .unwrap_or("none")
                .to_string()
        })
        .collect()
}

/// Where the requests moved from `before` to `after` (at a loop after the
/// third), and that they stayed.
fn raised_at(levels: &[String], before: &str, after: &str) -> usize {
    let at = levels
        .iter()
        .position(|l| l == after)
        .unwrap_or_else(|| panic!("never {after}: {levels:?}"));
    assert!((3..=6).contains(&at), "{levels:?}");
    assert!(levels[..at].iter().all(|l| l == before), "{levels:?}");
    assert!(levels[at..].iter().all(|l| l == after), "{levels:?}");
    at
}

/// The checkpoint's row: its profile, whether it switched, and its effort.
fn row(d: &Value) -> (Value, Value, Value, Value, Value) {
    (
        d["profile"].clone(),
        d["switch"].clone(),
        d["effort_reason"].clone(),
        d["effort_applied"].clone(),
        d["effort_ran"].clone(),
    )
}

/// The key test: a routed turn on `sonnethi` at high whose loops keep
/// failing gets a verdict that keeps `sonnethi` and asks `xhigh` above 0.70.
/// Its next request carries `xhigh` though the prompt is past the sticky
/// floor, no move is made, and the row and the result say it applied.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_checkpoint_raises_effort_without_a_move() {
    let (rig, claude, _jev) = routed("sonnet", "routine_coding", ("xhigh", 0.9));
    let res = turn(&rig.core, None, ASK, None).await;
    assert_eq!(res.profile, "sonnethi", "{}", res.output);
    let levels = levels(&claude);
    assert_eq!(levels.len(), 9, "{}", res.output);
    raised_at(&levels, "high", "xhigh");
    let rows = checkpoints(&rig);
    assert_eq!(
        row(&rows[0]),
        (
            json!("sonnethi"),
            json!(false),
            json!("applied"),
            json!(true),
            json!("xhigh")
        ),
        "{rows:?}"
    );
    assert_eq!(rows[0]["from"], "sonnethi");
    let route = res.route.expect("the checkpoint's route");
    assert_eq!(route.effort_applied.as_deref(), Some("xhigh"));
    assert_eq!(route.from, "sonnethi");
}

/// `xhigh` and `max` still need confidence above `high_effort_confidence`
/// at a checkpoint: `max` at 0.69 is held at `high`, which the turn already
/// runs at, so nothing changes; at 0.71 the turn runs at `max`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn xhigh_and_max_still_need_high_confidence_at_a_checkpoint() {
    let (rig, claude, _jev) = routed("sonnet", "routine_coding", ("max", 0.69));
    turn(&rig.core, None, ASK, None).await;
    assert_eq!(levels(&claude), ["high"; 9]);
    assert_eq!(
        row(&checkpoints(&rig)[0]),
        (
            json!("sonnethi"),
            json!(false),
            json!("held_high"),
            json!(false),
            json!("high")
        )
    );
    let (rig, claude, _jev) = routed("sonnet", "routine_coding", ("max", 0.71));
    turn(&rig.core, None, ASK, None).await;
    raised_at(&levels(&claude), "high", "max");
    assert_eq!(checkpoints(&rig)[0]["effort_ran"], "max");
}

/// A verdict asking less of a turn at `high` (on the session's base, where
/// no floor holds it) leaves it at `high`: a checkpoint never lowers. `low`,
/// two steps down, which the sticky rule would not hold (`medium`, one step,
/// it holds: the row says `sticky`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_checkpoint_never_lowers_effort() {
    let (rig, claude, _jev) = routed("sonnethi", "routine_coding", ("low", 0.95));
    let res = turn(&rig.core, None, ASK, None).await;
    assert_eq!(res.profile, "sonnethi");
    assert_eq!(levels(&claude), ["high"; 9]);
    let rows = checkpoints(&rig);
    assert_eq!(
        row(&rows[0]),
        (
            json!("sonnethi"),
            json!(false),
            json!("applied"),
            json!(false),
            json!("low")
        ),
        "{rows:?}"
    );
}

/// On a model that takes per-message effort a turn's level is placed only
/// before its user message: the checkpoint's raise is recorded, not applied,
/// and the requests are unchanged.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_model_with_per_message_effort_is_not_raised_mid_turn() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "routine_coding", 0.95);
    jev.script("reply_effort", choice("high", 0.95));
    let at = "failing: 3 failed";
    jev.script_when(at, None, "mode", choice("routine_coding", 0.95));
    jev.script_when(at, None, "reply_effort", choice("xhigh", 0.9));
    let claude = Arc::new(FakeProvider {
        delay_ms: 60,
        ..FakeProvider::scripted(loops(8))
    });
    let provider = claude.clone();
    let rig = rig_parts(
        Some(&jev),
        4,
        |c| {
            sonnethi(c, "sonnet");
            c.catalog.remove("claude-sonnet-5-5");
            c.routing.classes.routine_coding.profiles = vec!["sonnethi".into()];
        },
        move |p| {
            p.providers.insert("anthropic".into(), provider);
        },
    );
    turn(&rig.core, None, ASK, None).await;
    let first = claude.requests()[0].clone();
    for r in claude.requests() {
        assert_eq!(r.output_config, first.output_config);
        assert_eq!(
            crate::compiler::effort::effective(&r),
            Some(Effort::High),
            "{:?}",
            r.output_config
        );
    }
    let rows = checkpoints(&rig);
    assert_eq!(rows[0]["effort_ran"], "xhigh", "{rows:?}");
    assert_eq!(rows[0]["effort_applied"], false, "{rows:?}");
}

/// A routed profile never runs below its own effort: routine coding on
/// `haikuhi` (high) with a sure `medium` runs at high, `floored`; the
/// session's base profile with a sure `low` still runs at low; and under
/// bounds of `[low, medium]` `haikuhi` runs at medium (the bounds win).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_routed_profile_never_runs_below_its_own_effort() {
    let cases = [
        (
            "routine_coding",
            "medium",
            None,
            "haikuhi",
            "high",
            "floored",
        ),
        ("chat", "low", None, "sonnet", "low", "applied"),
        (
            "routine_coding",
            "medium",
            Some([Effort::Low, Effort::Medium]),
            "haikuhi",
            "medium",
            "applied",
        ),
    ];
    for (class, level, bounds, profile, ran, reason) in cases {
        let jev = FakeJev::start().unwrap();
        mode(&jev, class, 0.95);
        jev.script("reply_effort", choice(level, 0.95));
        let r = rig(Some(&jev), 1, |c| {
            if let Some(b) = bounds {
                c.routing.effort_bounds = b;
            }
        });
        let res = turn(&r.core, None, "Rename these twelve call sites.", None).await;
        assert_eq!(res.profile, profile, "{class} {level}");
        let req = &r.claude.requests()[0];
        let sent = crate::compiler::effort::effective(req).map(crate::routing::effort::name);
        assert_eq!(sent, Some(ran), "{class} {level} {bounds:?}");
        let d = &decided(&r.core.store)[0];
        assert_eq!(
            (&d["effort_reason"], &d["effort_ran"]),
            (&json!(reason), &json!(ran)),
            "{class} {level} {bounds:?}"
        );
    }
}

/// What Jev read at each checkpoint of a turn: the text's effort and
/// checkpoint lines (theseus-ujzuh).
fn evidence_lines(jev: &FakeJev) -> Vec<(Option<String>, String)> {
    jev.seen()
        .iter()
        .filter_map(|s| {
            let body = s.body.to_string();
            let at = body.find("Checkpoints this turn:")?;
            let line = |from: usize| {
                let rest = &body[from..];
                rest[..rest.find("\\n").unwrap_or(rest.len())].to_string()
            };
            let effort = body[..at].rfind("Effort: ").map(line);
            Some((effort, line(at)))
        })
        .collect()
}

/// The second failing checkpoint reads the effort the loops ran at, that a
/// checkpoint raised it, and that two checkpoints in a row found the work
/// failing; the first reads one failing and no raise.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_checkpoint_tells_jev_the_effort_the_work_ran_at() {
    let (rig, claude, jev) = routed("sonnet", "routine_coding", ("xhigh", 0.9));
    turn(&rig.core, None, ASK, None).await;
    let at = raised_at(&levels(&claude), "high", "xhigh");
    let seen = evidence_lines(&jev);
    assert!(seen.len() >= 2, "{seen:?}");
    assert_eq!(
        seen[0],
        (
            Some("Effort: high".into()),
            "Checkpoints this turn: 1, failing 1 in a row".into()
        )
    );
    assert_eq!(
        seen[1],
        (
            Some(format!("Effort: xhigh, raised from high at loop {at}")),
            "Checkpoints this turn: 2, failing 2 in a row".into()
        )
    );
}
