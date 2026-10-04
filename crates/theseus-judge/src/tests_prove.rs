//! The prove report's tests (L3): synthetic cohorts with known outcomes give
//! exact metrics per task and per dollar; small cohorts say "insufficient";
//! and the two planted reverts the design names (swapped arms, spend not
//! normalized) fail a test each.

use crate::prove::{
    markdown, parse_records, prove, ArmName, ProveMinimum, Report, TaskRecord, VerdictKind,
};

const WINS: &str = include_str!("../fixtures/prove/canary_wins.jsonl");
const SMALL: &str = include_str!("../fixtures/prove/small.jsonl");

fn rec(task: String, arm: ArmName, success: Option<bool>, spend_micros: u64) -> TaskRecord {
    TaskRecord {
        task,
        arm,
        success,
        spend_micros,
        judge_micros: 0,
        turns: 6,
        nudges: 0,
        unnecessary_nudges: 0,
        false_completion: None,
        stops: Vec::new(),
    }
}

/// `n` tasks in an arm, the first `wins` of them successes, each spending `micros`.
fn arm_of(arm: ArmName, n: usize, wins: usize, micros: u64) -> Vec<TaskRecord> {
    (0..n)
        .map(|i| rec(format!("{}-{i}", arm.as_str()), arm, Some(i < wins), micros))
        .collect()
}

fn cohorts(c: (usize, usize, u64), k: (usize, usize, u64)) -> Vec<TaskRecord> {
    let mut v = arm_of(ArmName::Canary, c.0, c.1, c.2);
    v.extend(arm_of(ArmName::Control, k.0, k.1, k.2));
    v
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn swapped(rs: &[TaskRecord]) -> Vec<TaskRecord> {
    rs.iter()
        .cloned()
        .map(|mut r| {
            r.arm = match r.arm {
                ArmName::Canary => ArmName::Control,
                ArmName::Control => ArmName::Canary,
            };
            r
        })
        .collect()
}

fn wins_report() -> Report {
    prove(&parse_records(WINS).unwrap(), ProveMinimum::default())
}

#[test]
fn known_cohorts_give_exact_metrics_per_task_and_per_dollar() {
    // Canary: 30 of 40 at $0.50 each ($20). Control: 20 of 40 at $0.50 ($20).
    let r = prove(
        &cohorts((40, 30, 500_000), (40, 20, 500_000)),
        ProveMinimum::default(),
    );
    let (c, k) = (&r.canary, &r.control);
    assert_eq!((c.tasks, c.successes, c.spend_micros), (40, 30, 20_000_000));
    assert_eq!((k.tasks, k.successes, k.spend_micros), (40, 20, 20_000_000));
    // Per task: the rates, and Wilson's intervals (computed independently).
    let (cc, kc) = (&c.completion, &k.completion);
    assert_eq!((cc.value, kc.value), (Some(0.75), Some(0.5)));
    assert!(near(cc.lo.unwrap(), 0.598_060, 1e-5) && near(cc.hi.unwrap(), 0.858_129, 1e-5));
    assert!(near(kc.lo.unwrap(), 0.351_995, 1e-5) && near(kc.hi.unwrap(), 0.648_005, 1e-5));
    assert!(near(c.spend_usd_per_task.value.unwrap(), 0.5, 1e-12));
    assert!(near(c.turns_per_task.value.unwrap(), 6.0, 1e-12));
    // Per dollar: completions over dollars.
    assert!(near(c.completions_per_usd.value.unwrap(), 1.5, 1e-12));
    assert!(near(k.completions_per_usd.value.unwrap(), 1.0, 1e-12));
    assert!(near(c.turns_per_usd.value.unwrap(), 12.0, 1e-12));
    // The differences, and Newcombe's interval for the per-task one.
    let d = r.completion_diff.as_ref().unwrap();
    assert!(
        near(d.value, 0.25, 1e-12) && near(d.lo, 0.037_889, 1e-5) && near(d.hi, 0.433_296, 1e-5)
    );
    let d = r.completions_per_usd_diff.as_ref().unwrap();
    assert!(near(d.value, 0.5, 1e-12) && near(d.lo, 0.09, 0.01) && near(d.hi, 0.91, 0.01));
    assert_eq!(r.spend_ratio, Some(1.0));
    assert_eq!(r.spend_balanced, Some(true));
    assert_eq!(r.verdict.kind, VerdictKind::CanaryBetter);
}

#[test]
fn small_cohorts_say_insufficient_with_their_counts_and_no_number() {
    let r = prove(
        &cohorts((29, 29, 500_000), (29, 0, 500_000)),
        ProveMinimum::default(),
    );
    assert_eq!(r.verdict.kind, VerdictKind::Insufficient);
    let said = r.verdict.reasons.join("\n");
    assert!(
        said.contains("29 labeled") && said.contains("labeled tasks: 29 of 30"),
        "{said}"
    );
    for c in [&r.canary, &r.control] {
        assert_eq!(c.completion.value, None);
        assert_eq!(c.completions_per_usd.value, None);
        assert_eq!(
            c.completion.insufficient.as_deref(),
            Some("labeled tasks: 29 of 30")
        );
    }
    assert!(r.completion_diff.is_none() && r.completions_per_usd_diff.is_none());
    // One arm short is enough.
    let r = prove(
        &cohorts((40, 30, 500_000), (12, 6, 500_000)),
        ProveMinimum::default(),
    );
    assert_eq!(r.verdict.kind, VerdictKind::Insufficient);
    assert!(r.canary.completion.value.is_some() && r.control.completion.value.is_none());
    // An empty arm too, and no arm at all.
    assert_eq!(
        prove(&[], ProveMinimum::default()).verdict.kind,
        VerdictKind::Insufficient
    );
    // The minimum is the operator's to set, and is stated.
    let m = ProveMinimum {
        tasks_per_arm: 10,
        labeled_per_metric: 5,
    };
    assert_ne!(
        prove(&cohorts((12, 12, 500_000), (12, 0, 500_000)), m)
            .verdict
            .kind,
        VerdictKind::Insufficient
    );
}

#[test]
fn the_fixture_that_is_too_small_says_so_in_both_outputs() {
    let r = prove(&parse_records(SMALL).unwrap(), ProveMinimum::default());
    assert_eq!(r.verdict.kind, VerdictKind::Insufficient);
    let md = markdown(&r);
    assert!(md.contains("## Verdict: insufficient"), "{md}");
    assert!(
        md.contains("canary: 5 tasks, 4 labeled, 3 successes, $2.00 spent"),
        "{md}"
    );
    assert!(md.contains("insufficient (labeled tasks: 4 of 30)"), "{md}");
    let j: serde_json::Value = serde_json::to_value(&r).unwrap();
    assert_eq!(j["verdict"]["kind"], "insufficient");
    assert!(j["canary"]["completion"]["value"].is_null());
    assert_eq!(j["canary"]["completion"]["n"], 4);
}

/// Planted revert: select each arm's records from the other arm, and this fails.
#[test]
fn swapping_canary_and_control_flips_the_verdict() {
    let rs = parse_records(WINS).unwrap();
    assert_eq!(
        prove(&rs, ProveMinimum::default()).verdict.kind,
        VerdictKind::CanaryBetter
    );
    let flipped = prove(&swapped(&rs), ProveMinimum::default());
    assert_eq!(flipped.verdict.kind, VerdictKind::CanaryWorse);
    assert!(flipped.completion_diff.unwrap().value < 0.0);
    // And the canary column holds the records marked canary.
    let r = wins_report();
    assert_eq!(r.canary.successes, 30);
    assert_eq!(r.control.successes, 20);
}

/// Planted revert: report per-task rates where per-dollar ones belong, and
/// this fails. Equal rates per task, but the canary spends twice as much.
#[test]
fn a_canary_that_costs_twice_as_much_is_worse_per_dollar_though_equal_per_task() {
    let r = prove(
        &cohorts((60, 30, 1_000_000), (60, 30, 500_000)),
        ProveMinimum::default(),
    );
    assert_eq!(r.completion_diff.as_ref().unwrap().value, 0.0);
    assert!(near(
        r.canary.completions_per_usd.value.unwrap(),
        0.5,
        1e-12
    ));
    assert!(near(
        r.control.completions_per_usd.value.unwrap(),
        1.0,
        1e-12
    ));
    assert!(r.completions_per_usd_diff.as_ref().unwrap().hi < 0.0);
    assert_eq!(r.verdict.kind, VerdictKind::CanaryWorse);
    assert_eq!(r.spend_ratio, Some(2.0));
    assert_eq!(r.spend_balanced, Some(false));
    assert!(r
        .verdict
        .reasons
        .join("\n")
        .contains("not at equal total spend"));
    // And the reverse: cheaper per success, equal per task, is better per dollar.
    let r = prove(
        &cohorts((60, 30, 250_000), (60, 30, 500_000)),
        ProveMinimum::default(),
    );
    assert_eq!(r.verdict.kind, VerdictKind::CanaryBetter);
}

#[test]
fn equal_arms_show_no_difference_and_a_worse_task_rate_is_never_called_better() {
    let r = prove(
        &cohorts((60, 30, 500_000), (60, 30, 500_000)),
        ProveMinimum::default(),
    );
    assert_eq!(r.verdict.kind, VerdictKind::NoDifference);
    // Cheaper per dollar, but succeeds far less per task: worse, never better.
    let r = prove(
        &cohorts((100, 20, 100_000), (100, 80, 500_000)),
        ProveMinimum::default(),
    );
    assert_eq!(r.verdict.kind, VerdictKind::CanaryWorse);
}

#[test]
fn tasks_without_an_outcome_are_counted_and_left_out_of_every_rate() {
    let mut rs = cohorts((40, 30, 500_000), (40, 20, 500_000));
    for i in 0..10 {
        rs.push(rec(
            format!("canary-open-{i}"),
            ArmName::Canary,
            None,
            9_000_000,
        ));
    }
    let r = prove(&rs, ProveMinimum::default());
    assert_eq!(
        (
            r.canary.tasks,
            r.canary.labeled_tasks,
            r.canary.unlabeled_tasks
        ),
        (50, 40, 10)
    );
    assert_eq!(r.canary.completion.value, Some(0.75));
    // Spend is shown for all tasks; the per-dollar rate uses the labeled ones.
    assert_eq!(r.canary.spend_micros, 20_000_000 + 90_000_000);
    assert!(near(
        r.canary.completions_per_usd.value.unwrap(),
        1.5,
        1e-12
    ));
}

#[test]
fn stop_precision_and_recall_come_from_labeled_decisions_only() {
    let r = wins_report();
    // Canary: 28 stops right, 4 stops wrong (precision 28/32); 30 should have
    // stopped, 28 did (recall 28/30). The unlabeled ones are not counted.
    let (p, c) = (&r.canary.stop_precision, &r.canary.stop_recall);
    assert_eq!((p.n, p.value), (32, Some(0.875)));
    assert_eq!((c.n, c.value.map(|v| (v * 30.0).round())), (30, Some(28.0)));
    // Control's baseline stops: 20 right, 20 wrong. Only 20 should have
    // stopped, under the minimum of 30: recall is withheld, with the count.
    assert_eq!(r.control.stop_precision.value, Some(0.5));
    assert_eq!(r.control.stop_recall.value, None);
    assert_eq!(
        r.control.stop_recall.insufficient.as_deref(),
        Some("labeled should-stop tasks: 20 of 30")
    );
    // No stop labels at all: no number, and the counts.
    let none = prove(
        &cohorts((40, 30, 500_000), (40, 20, 500_000)),
        ProveMinimum::default(),
    );
    assert_eq!(none.canary.stop_precision.value, None);
    assert_eq!(
        none.canary.stop_precision.insufficient.as_deref(),
        Some("labeled stop decisions: 0 of 30")
    );
}

#[test]
fn false_completion_and_nudge_rates_need_their_own_minimum() {
    let r = wins_report();
    // Canary: 35 labeled, 3 false. Control: 35 labeled, 15 false.
    assert_eq!(
        (
            r.canary.false_completion.n,
            r.canary.false_completion.value.map(|v| (v * 35.0).round())
        ),
        (35, Some(3.0))
    );
    assert_eq!(
        r.control.false_completion.value.map(|v| (v * 35.0).round()),
        Some(15.0)
    );
    // Canary sent 36 nudges, 6 unnecessary; control sent none: no number.
    assert_eq!((r.canary.nudges, r.canary.unnecessary_nudges), (36, 6));
    assert!(near(
        r.canary.unnecessary_nudge_rate.value.unwrap(),
        6.0 / 36.0,
        1e-12
    ));
    assert_eq!(r.control.unnecessary_nudge_rate.value, None);
    assert_eq!(
        r.control.unnecessary_nudge_rate.insufficient.as_deref(),
        Some("nudges sent: 0 of 30")
    );
}

#[test]
fn the_judges_spend_is_inside_the_arms_spend() {
    let r = wins_report();
    assert_eq!(r.canary.judge_micros, 40 * 20_000);
    assert_eq!(r.canary.spend_micros, 20_000_000);
    assert_eq!(r.control.judge_micros, 0);
    assert_eq!(r.spend_ratio, Some(1.0));
}

#[test]
fn bad_input_is_refused_with_its_line() {
    let ok = r#"{"task":"a","arm":"canary","success":true,"spend_micros":10,"turns":1}"#;
    assert_eq!(parse_records(&format!("{ok}\n\n")).unwrap().len(), 1);
    let e = |text: &str| format!("{:#}", parse_records(text).unwrap_err());
    assert!(e(&format!("{ok}\n{ok}")).contains("line 2: task a appears twice"));
    assert!(e(&format!("{ok}\nnot json")).contains("line 2: not a task record"));
    assert!(
        e(r#"{"task":"a","arm":"treatment","success":true,"spend_micros":1,"turns":1}"#)
            .contains("line 1")
    );
    assert!(e(
        r#"{"task":"a","arm":"canary","success":true,"spend_micros":1,"turns":1,"extra":1}"#
    )
    .contains("line 1"));
    assert!(e(r#"{"task":"a","arm":"canary","spend_micros":1,"turns":1}"#).contains("line 1"));
    assert!(e(
        r#"{"task":"a","arm":"canary","success":true,"spend_micros":1,"judge_micros":2,"turns":1}"#
    )
    .contains("exceeds spend_micros"));
    assert!(e(r#"{"task":"a","arm":"canary","success":true,"spend_micros":1,"turns":1,"nudges":1,"unnecessary_nudges":2}"#).contains("unnecessary_nudges"));
}

#[test]
fn the_report_is_deterministic_and_markdown_matches_its_golden() {
    let a = markdown(&wins_report());
    assert_eq!(a, markdown(&wins_report()));
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/prove/canary_wins.golden.md"
    );
    if std::env::var_os("THESEUS_JUDGE_BLESS").is_some() {
        std::fs::write(path, &a).unwrap();
    }
    let golden = std::fs::read_to_string(path).expect("golden; rerun with THESEUS_JUDGE_BLESS=1");
    assert_eq!(a, golden);
    assert!(a.contains("## Verdict: canary_better"));
}
