//! `theseus judge replay --cases` (theseus-h66v4, row 1), through a whole core
//! and the fake Jev: route.v10's 58 labelled cases are asked of the pack, one
//! call each with the case's own state, and graded against their labels. A
//! Jev scripted to answer every label scores every case and meets the bar; a
//! Jev that answers one `must` case wrong shows it, with its top option and
//! confidence, and fails the bar; the run's rows are scoped `judge.replay:route`;
//! and every refusal (a recorded set beside the cases, a file written for
//! another pack, the run's limit) sends nothing.

use theseus_judge::fake::{FakeJev, Scripted};
use theseus_judge::route_cases::{self, CaseSet, V10};
use theseus_protocol::judge_runs::{JudgeReplayParams, JudgeReplayResult};

use crate::ledger::LedgerRow;
use crate::tests_judge::{texts, Rig};
use crate::tests_replay::rig;

fn limited(c: &mut crate::Config) {
    c.judge.replay_limit_usd = 5.0;
}

fn cases_rig(jev: &FakeJev) -> Rig {
    rig(texts(0), jev, limited)
}

fn params() -> JudgeReplayParams {
    JudgeReplayParams {
        cases: Some(V10.to_string()),
        ..Default::default()
    }
}

/// The set and the pack it is asked of.
fn set() -> (CaseSet, std::sync::Arc<theseus_judge::Pack>) {
    let pack = theseus_judge::pack::by_name("route.v10").expect("route.v10 is embedded");
    (CaseSet::parse(V10, &pack).unwrap(), pack)
}

/// Scripts Jev to answer one case with these top options, each at 0.9; the
/// state the rule matches is the case's whole state, so W11 and W14 (one
/// message, two states) are told apart.
fn script_case(
    jev: &FakeJev,
    state_json: &str,
    model: &str,
    effort: &str,
    conf: f64,
) {
    for (q, option) in [("model", model), ("effort", effort)] {
        jev.script_when(
            state_json,
            None,
            &format!("route.v10/{q}"),
            Scripted::Choice {
                option: option.into(),
                confidence: if q == "model" { conf } else { 0.9 },
            },
        );
    }
}

/// Every case answered as labelled, except where `tweak` says otherwise.
fn script_all(jev: &FakeJev, tweak: impl Fn(&str) -> Option<(&'static str, f64)>) {
    let (set, pack) = set();
    for c in &set.cases {
        // The fake reads the request's state as a JSON value, whose keys are in order.
        let state = route_cases::state(c, pack.state_cap_tokens, &theseus_judge::NoScrub).value();
        let state = state.to_string();
        let (model, conf) = match tweak(&c.id) {
            Some((m, conf)) => (m.to_string(), conf),
            None => (c.want.model.clone(), 0.9),
        };
        script_case(jev, &state, &model, &c.want.effort, conf);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_jev_that_answers_every_label_meets_the_bar() {
    let jev = FakeJev::start().unwrap();
    let r = cases_rig(&jev);
    let c = &r.core;
    script_all(&jev, |_| None);
    let out: JudgeReplayResult = c.judge_replay(params(), "cli").await.unwrap();
    // No candidate was named: the file's own pack was asked.
    assert_eq!(out.candidate, "route.v10");
    assert_eq!(
        (out.set.as_str(), out.judgments, out.called),
        ("cases", 61, 61)
    );
    let cases = out.cases.expect("a cases replay's numbers");
    assert_eq!(cases.cases.len(), 61);
    assert_eq!(
        (cases.answered, cases.model_ok, cases.effort_exact),
        (61, 61, 61)
    );
    assert_eq!(
        (cases.must_total, cases.must_ok, cases.must_under_confirm),
        (13, 13, 0)
    );
    // The 61-case file is not the 58 the old bar was written for: none is judged.
    assert_eq!(cases.bar, None);
    assert!(out.left_out.is_empty());
    // One call per case, each with its own message in the state.
    let seen = jev.seen();
    assert_eq!(seen.len(), 61);
    // The rows: a judge.call each and the run, all scoped to the pack's id.
    let rows: Vec<LedgerRow> = c
        .store
        .scope_after("judge.replay:route", 0)
        .unwrap()
        .iter()
        .filter_map(|r| r.decode().ok())
        .collect();
    let calls: Vec<&LedgerRow> = rows.iter().filter(|r| r.kind == "judge.call").collect();
    assert_eq!(calls.len(), 61);
    for row in &calls {
        assert_eq!(row.data["context"]["purpose"], "replay");
        assert_eq!(row.data["context"]["run"], out.id);
        assert!(row.data["context"]["case"].is_string());
    }
    assert_eq!(rows.iter().filter(|r| r.kind == "judge.replay").count(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wrong_must_case_is_shown_with_jevs_option_and_confidence_and_fails_the_bar() {
    let jev = FakeJev::start().unwrap();
    let r = cases_rig(&jev);
    // W26 (CVE triage, `must`) goes to Fable at 0.71; W27 (biology) is under the confirm bar.
    script_all(&jev, |id| match id {
        "W26" => Some(("fable", 0.71)),
        "W27" => Some(("sonnet", 0.55)),
        _ => None,
    });
    let out = r.core.judge_replay(params(), "cli").await.unwrap();
    let cases = out.cases.unwrap();
    let w26 = cases.cases.iter().find(|c| c.id == "W26").unwrap();
    assert!(w26.must && !w26.model.agrees);
    assert_eq!(
        (
            w26.model.want.as_str(),
            w26.model.got.as_str(),
            w26.model.confidence
        ),
        ("sonnet", "fable", 0.71)
    );
    assert_eq!(
        (cases.model_ok, cases.must_ok, cases.must_total),
        (60, 12, 13)
    );
    // No bar is judged on the 61-case file; the must count says the run missed.
    assert_eq!(cases.bar, None);
    // W27 is right, but not sure enough to act; W27 is no `must` case.
    let w27 = cases.cases.iter().find(|c| c.id == "W27").unwrap();
    assert!(w27.model.agrees && !w27.must);
    assert_eq!(cases.must_under_confirm, 0);
}

/// Every case answered as labelled at 0.9, except where `tweak` gives a
/// case's two answers (option and confidence, for `model` and `effort`).
fn script_each(jev: &FakeJev, tweak: impl Fn(&str) -> Option<[(&'static str, f64); 2]>) {
    let (set, pack) = set();
    for c in &set.cases {
        let state = route_cases::state(c, pack.state_cap_tokens, &theseus_judge::NoScrub).value();
        let state = state.to_string();
        let picks: [(String, f64); 2] = match tweak(&c.id) {
            Some(p) => p.map(|(option, conf)| (option.to_string(), conf)),
            None => [(c.want.model.clone(), 0.9), (c.want.effort.clone(), 0.9)],
        };
        for (q, (option, confidence)) in ["model", "effort"].into_iter().zip(picks) {
            jev.script_when(
                &state,
                None,
                &format!("route.v10/{q}"),
                Scripted::Choice { option, confidence },
            );
        }
    }
}

/// theseus-mir9c: the replay grades what routing decides. A pick under a
/// question's `confirm` bar is that question's `no_match` (route.v10's:
/// `sonnet`, `high`), the way the daemon's `sure` reads it; the raw picks are
/// counted beside.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pick_under_the_confirm_bar_is_graded_as_its_questions_no_match() {
    let jev = FakeJev::start().unwrap();
    let r = cases_rig(&jev);
    script_each(&jev, |id| match id {
        // W18 wants opus, medium: a right opus under the bar runs sonnet.
        "W18" => Some([("opus", 0.55), ("medium", 0.9)]),
        // L3 wants sonnet, medium: a right `medium` under the bar is `high`.
        "L3" => Some([("sonnet", 0.9), ("medium", 0.55)]),
        // L11 wants opus, high: a `medium` under the bar is `high`, which it wants.
        "L11" => Some([("opus", 0.9), ("medium", 0.55)]),
        // L2 wants haiku, low: haiku at the bar (0.60) stands.
        "L2" => Some([("haiku", 0.60), ("low", 0.9)]),
        _ => None,
    });
    let out = r.core.judge_replay(params(), "cli").await.unwrap();
    let cases = out.cases.unwrap();
    let get = |id: &str| cases.cases.iter().find(|c| c.id == id).unwrap();
    let w18 = get("W18");
    assert_eq!(
        (
            w18.model.raw.as_str(),
            w18.model.got.as_str(),
            w18.model.sub_confirm,
            w18.model.confirm,
            w18.model.confidence
        ),
        ("opus", "sonnet", true, 0.60, 0.55)
    );
    assert!(!w18.model.agrees && w18.model.raw_agrees);
    let l3 = get("L3");
    assert_eq!(
        (
            l3.effort.raw.as_str(),
            l3.effort.got.as_str(),
            l3.effort.sub_confirm
        ),
        ("medium", "high", true)
    );
    assert!(!l3.effort.agrees && l3.effort.close && l3.effort.raw_agrees);
    let l11 = get("L11");
    assert_eq!(
        (
            l11.effort.raw.as_str(),
            l11.effort.got.as_str(),
            l11.effort.sub_confirm
        ),
        ("medium", "high", true)
    );
    assert!(l11.effort.agrees && !l11.effort.raw_agrees && l11.effort.raw_close);
    // At the bar, nothing changes.
    let l2 = get("L2");
    assert!(!l2.model.sub_confirm && l2.model.agrees);
    // The totals: model loses one (W18) and effort one (L3, one rung off); the
    // raw picks lose L3's and L11's exact effort and keep every rung close.
    assert_eq!(
        (cases.model_ok, cases.effort_exact, cases.effort_close),
        (60, 60, 61)
    );
    assert_eq!(
        (
            cases.raw_model_ok,
            cases.raw_effort_exact,
            cases.raw_effort_close
        ),
        (61, 60, 61)
    );
    // W18 is a `must` case: its model fell, so a must case is wrong and under the bar.
    assert_eq!(
        (cases.must_ok, cases.must_total, cases.must_under_confirm),
        (12, 13, 1)
    );
    assert_eq!(cases.bar, None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_refusals_send_nothing() {
    let jev = FakeJev::start().unwrap();
    let r = cases_rig(&jev);
    let c = &r.core;
    // A recorded set, or --onto, beside the cases.
    for p in [
        JudgeReplayParams {
            report: Some("rpt_x".into()),
            ..params()
        },
        JudgeReplayParams {
            errors: true,
            ..params()
        },
        JudgeReplayParams {
            judgments: vec!["jdg_1".into()],
            ..params()
        },
        JudgeReplayParams {
            onto: true,
            ..params()
        },
    ] {
        let e = c.judge_replay(p, "cli").await.unwrap_err().to_string();
        assert!(
            e.contains("--cases") && e.contains("Nothing was sent"),
            "{e}"
        );
    }
    // The file is written for route.v10, not the wired route.v8.
    let e = c
        .judge_replay(
            JudgeReplayParams {
                candidate: Some("route.v8".into()),
                ..params()
            },
            "cli",
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(e.contains("wired") || e.contains("route.v10"), "{e}");
    // A broken file is refused with the loader's words.
    let e = c
        .judge_replay(
            JudgeReplayParams {
                cases: Some(V10.replacen("model = \"haiku\"", "model = \"glm\"", 1)),
                ..Default::default()
            },
            "cli",
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(e.contains("glm"), "{e}");
    assert!(jev.seen().is_empty(), "a refused run sent a call");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_limit_under_the_estimate_refuses_the_run_before_any_call() {
    let jev = FakeJev::start().unwrap();
    let r = rig(texts(0), &jev, |c| c.judge.replay_limit_usd = 0.000_001);
    let e = r
        .core
        .judge_replay(params(), "cli")
        .await
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("replay_limit_usd") && e.contains("nothing was sent"),
        "{e}"
    );
    assert!(jev.seen().is_empty());
}
