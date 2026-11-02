//! `theseus judge replay --cases` (theseus-h66v4, row 1; the four packs of theseus-h66v4.6),
//! through a whole core and the fake Jev: each pack's labelled cases are asked of it, one call each
//! with the case's own state slice, and graded against their labels. A Jev scripted to answer
//! every label meets every bar; a Jev that answers one `must` case wrong shows it, with its top
//! option and confidence, and still meets a bar that allows it (80%, rounded up); the run's rows
//! are scoped `judge.replay:<pack id>`; and every refusal (a recorded set beside the cases, a file
//! written for another pack, the run's limit) sends nothing.

use theseus_judge::fake::{FakeJev, Scripted};
use theseus_judge::route_cases::{self, CaseSet, SPLIT, V10, WORK};
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

/// The model run: `route.v10` over its own file, the default candidate.
fn params() -> JudgeReplayParams {
    JudgeReplayParams {
        cases: Some(V10.to_string()),
        ..Default::default()
    }
}

/// A run of `pack` over `cases`.
fn params_for(pack: &str, cases: &str) -> JudgeReplayParams {
    JudgeReplayParams {
        cases: Some(cases.to_string()),
        candidate: Some(pack.to_string()),
        ..Default::default()
    }
}

/// The set and the pack it is asked of.
fn set_of(pack_name: &str, text: &str) -> (CaseSet, std::sync::Arc<theseus_judge::Pack>) {
    let pack = theseus_judge::pack::by_name(pack_name).expect("the pack is embedded");
    (CaseSet::parse(text, &pack).unwrap(), pack)
}

/// The scripted answers: a case's `(option, confidence)` for one question, or its label at 0.9.
type Tweak<'a> = &'a dyn Fn(&str) -> Option<(&'static str, f64)>;

/// Scripts Jev to answer `question` of every case of `text` as `pack_name` asks it: the label at
/// 0.9, except where `tweak` says otherwise. The state the rule matches is the case's whole state
/// slice for that pack, so W11 and W14 (one message, two states) are told apart.
fn script(jev: &FakeJev, pack_name: &str, text: &str, question: &str, tweak: Tweak<'_>) {
    let (set, pack) = set_of(pack_name, text);
    for c in &set.cases {
        let state = route_cases::prepared(&pack, c).state.value().to_string();
        let (option, confidence) = match tweak(&c.id) {
            Some((o, conf)) => (o.to_string(), conf),
            None => (c.want.of(question).to_string(), 0.9),
        };
        jev.script_when(
            &state,
            None,
            &format!("{pack_name}/{question}"),
            Scripted::Choice { option, confidence },
        );
    }
}

fn nothing(_: &str) -> Option<(&'static str, f64)> {
    None
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_jev_that_answers_every_label_meets_the_bar() {
    let jev = FakeJev::start().unwrap();
    let r = cases_rig(&jev);
    let c = &r.core;
    script(&jev, "route.v10", V10, "model", &nothing);
    let out: JudgeReplayResult = c.judge_replay(params(), "cli").await.unwrap();
    // No candidate was named: the file's own pack was asked.
    assert_eq!(out.candidate, "route.v10");
    assert_eq!(
        (out.set.as_str(), out.judgments, out.called),
        ("cases", 61, 61)
    );
    let cases = out.cases.expect("a cases replay's numbers");
    assert_eq!(cases.cases.len(), 61);
    assert_eq!(cases.asked, ["model"], "the model pack grades the model");
    assert_eq!((cases.answered, cases.model_ok), (61, 61));
    assert_eq!(
        (cases.must_total, cases.must_ok, cases.must_under_confirm),
        (13, 13, 0)
    );
    // Two bars, each a ratio of its own set: 49 of 61, and 11 of the 13 replayed musts.
    let bars: Vec<(&str, u32, u32, u32, bool)> = cases
        .bars
        .iter()
        .map(|b| (b.question.as_str(), b.ok, b.of, b.need, b.met))
        .collect();
    assert_eq!(
        bars,
        [("model", 61, 61, 49, true), ("must", 13, 13, 11, true)]
    );
    assert_eq!(cases.bar, Some(true));
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

/// The other three packs of the family are each replayed over their own labels, by name: the
/// model file's `effort` by `effort.v1`, and `work.v1` and `split.v1` over their own files,
/// each against a bar that is 80% of that file's own count, rounded up.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn effort_work_and_split_are_each_replayed_over_their_own_labels() {
    let jev = FakeJev::start().unwrap();
    let r = cases_rig(&jev);
    script(&jev, "effort.v1", V10, "effort", &nothing);
    script(&jev, "work.v1", WORK, "work", &nothing);
    script(&jev, "split.v1", SPLIT, "split", &nothing);
    for (pack, text, q, n, need) in [
        ("effort.v1", V10, "effort", 61, 49),
        ("work.v1", WORK, "work", 65, 52),
        ("split.v1", SPLIT, "split", 62, 50),
    ] {
        let out = r
            .core
            .judge_replay(params_for(pack, text), "cli")
            .await
            .unwrap();
        assert_eq!(out.candidate, pack);
        let cases = out.cases.unwrap();
        assert_eq!(cases.asked, [q], "{pack}");
        assert_eq!(cases.cases.len(), n, "{pack}");
        let bar = &cases.bars[0];
        assert_eq!(
            (bar.question.as_str(), bar.ok, bar.of, bar.need, bar.met),
            (q, n as u32, n as u32, need, true),
            "{pack}"
        );
        assert_eq!((cases.bars.len(), cases.bar), (1, Some(true)), "{pack}");
        assert_eq!(cases.under_effort + cases.under_work + cases.under_split, 0);
    }
    // split.v1 is wired (in shadow), and a cases run still replays it: it names no incumbent.
    assert!(crate::judge::WIRED.iter().any(|(n, _)| *n == "split.v1"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_wrong_must_case_is_shown_with_jevs_option_and_confidence_and_the_must_bar_allows_two() {
    let jev = FakeJev::start().unwrap();
    let r = cases_rig(&jev);
    // W26 (CVE triage, `must`) goes to Fable at 0.71; W27 (biology) is under the confirm bar.
    script(&jev, "route.v10", V10, "model", &|id| match id {
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
    // 12 of 13 musts is over the 11 the bar needs: one wrong must case does not fail the run.
    assert_eq!(cases.bar, Some(true));
    // W27 is right, but not sure enough to act; W27 is no `must` case.
    let w27 = cases.cases.iter().find(|c| c.id == "W27").unwrap();
    assert!(w27.model.agrees && !w27.must);
    assert_eq!(cases.must_under_confirm, 0);
}

/// theseus-mir9c: the replay grades what routing decides. A pick under a
/// question's `confirm` bar is that question's `no_match` (route.v10's `model`:
/// `sonnet`; effort.v1's: `high`), the way the daemon's `sure` reads it; the raw
/// picks are counted beside.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pick_under_the_confirm_bar_is_graded_as_its_questions_no_match() {
    let jev = FakeJev::start().unwrap();
    let r = cases_rig(&jev);
    script(&jev, "route.v10", V10, "model", &|id| match id {
        // W18 wants opus: a right opus under the bar runs sonnet.
        "W18" => Some(("opus", 0.55)),
        // L2 wants haiku: haiku at the bar (0.60) stands.
        "L2" => Some(("haiku", 0.60)),
        _ => None,
    });
    script(&jev, "effort.v1", V10, "effort", &|id| match id {
        // L3 wants medium: a right `medium` under the bar is `high`.
        "L3" => Some(("medium", 0.55)),
        // L11 wants high: a `medium` under the bar is `high`, which it wants.
        "L11" => Some(("medium", 0.55)),
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
    // At the bar, nothing changes.
    let l2 = get("L2");
    assert!(!l2.model.sub_confirm && l2.model.agrees);
    // The model run: one lost (W18), one under the bar; the raw picks lose none.
    assert_eq!(
        (cases.model_ok, cases.raw_model_ok, cases.under_model),
        (60, 61, 1)
    );
    // W18 is a `must` case: its model fell, so a must case is wrong and under the bar.
    assert_eq!(
        (cases.must_ok, cases.must_total, cases.must_under_confirm),
        (12, 13, 1)
    );
    // The effort run, from the same file by the pack that asks it.
    let out = r
        .core
        .judge_replay(params_for("effort.v1", V10), "cli")
        .await
        .unwrap();
    let cases = out.cases.unwrap();
    let get = |id: &str| cases.cases.iter().find(|c| c.id == id).unwrap();
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
    // effort loses one exact (L3, one rung off); the raw picks lose L3's and L11's exact and keep
    // every rung close; two answers fell under the bar.
    assert_eq!(
        (cases.effort_exact, cases.effort_close, cases.under_effort),
        (60, 61, 2)
    );
    // L3's raw `medium` is right and L11's is one rung off: the raw exact count loses L11 alone.
    assert_eq!((cases.raw_effort_exact, cases.raw_effort_close), (60, 61));
    assert_eq!(cases.asked, ["effort"]);
    assert_eq!(cases.bar, Some(true));
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
