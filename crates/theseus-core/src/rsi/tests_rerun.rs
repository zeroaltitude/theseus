//! Reading a red suite's log (issue 20): which tests failed, when the log can be trusted to say,
//! and what a second run must show before a failure is let go.

use super::rerun::{all_passed, filter, suite_failures, Failed};

fn failed(binary: &str, test: &str) -> Failed {
    Failed {
        binary: binary.into(),
        test: test.into(),
    }
}

const RED: &str = "\
    Starting 3 tests across 3 binaries (1 test skipped)
        PASS [   0.011s] a tests::ok
        FAIL [   0.021s] b job::tests::flaky
        PASS [   0.012s] c tests::fine
------------
     Summary [   0.050s] 3 tests run: 2 passed (1 slow), 1 failed, 1 skipped
        FAIL [   0.021s] b job::tests::flaky
error: test run failed
";

#[test]
fn the_failures_of_a_plain_red_suite_are_read_once_each() {
    assert_eq!(
        suite_failures(RED),
        Some(vec![failed("b", "job::tests::flaky")])
    );
}

/// Newer nextests put a counter before the names; the names are the last two words.
#[test]
fn a_counter_before_the_names_is_no_part_of_them() {
    let log = RED.replace("FAIL [   0.021s]", "FAIL [   0.021s] ( 2/3)");
    assert_eq!(
        suite_failures(&log),
        Some(vec![failed("b", "job::tests::flaky")])
    );
}

/// A phase before the suite that ran nextest (the reader rule) is not read as the suite.
#[test]
fn only_the_last_run_in_the_log_is_read() {
    let log = format!(
        "    Starting 1 test across 1 binary\n        FAIL [ 0.1s] z old::failure\n     Summary [ 0.1s] 1 test run: 0 passed, 1 failed, 0 skipped\n{RED}"
    );
    assert_eq!(
        suite_failures(&log),
        Some(vec![failed("b", "job::tests::flaky")])
    );
}

/// A log that cannot be read whole is not read: a timeout, a signal, a count that does not match,
/// no summary, a summary that says something else, no failure at all, a second summary.
#[test]
fn a_log_that_cannot_be_read_whole_is_not_read() {
    let cases = [
        (
            "a timeout",
            RED.replace(
                "PASS [   0.011s] a tests::ok",
                "TIMEOUT [ 120.0s] a tests::ok",
            ),
        ),
        (
            "a signal",
            RED.replace(
                "PASS [   0.011s] a tests::ok",
                "SIGABRT [ 1.0s] a tests::ok",
            ),
        ),
        (
            "a leak",
            RED.replace("PASS [   0.011s] a tests::ok", "LEAK [ 1.0s] a tests::ok"),
        ),
        (
            "a surplus name",
            RED.replace(
                "error: test run failed",
                "        FAIL [   0.021s] d forged::line\nerror: test run failed",
            ),
        ),
        (
            "a missing name",
            RED.replace("        FAIL [   0.021s] b job::tests::flaky\n", ""),
        ),
        (
            "another kind of count",
            RED.replace("1 skipped", "1 timed out"),
        ),
        ("no summary", RED.replace("     Summary", "     Sum")),
        (
            "nothing failed",
            RED.replace("1 failed", "0 failed")
                .replace("        FAIL [   0.021s] b job::tests::flaky\n", ""),
        ),
        (
            "a second summary",
            format!("{RED}     Summary [   0.050s] 3 tests run: 2 passed, 1 failed, 1 skipped\n"),
        ),
        (
            "a name outside the alphabet",
            RED.replace("b job::tests::flaky", "b job::tests::fl'aky"),
        ),
    ];
    for (what, log) in cases {
        assert_eq!(suite_failures(&log), None, "{what}:\n{log}");
    }
}

#[test]
fn the_filter_names_exactly_the_failed_tests() {
    let tests = [
        failed("b", "job::tests::flaky"),
        failed("theseusd::stops", "x_y"),
    ];
    assert_eq!(
        filter(&tests),
        "(binary_id(=b) & test(=job::tests::flaky)) | (binary_id(=theseusd::stops) & test(=x_y))"
    );
}

#[test]
fn a_binary_id_names_its_package_first() {
    assert_eq!(failed("theseusd::stops", "t").package(), "theseusd");
    assert_eq!(failed("theseus-kernel", "t").package(), "theseus-kernel");
}

#[test]
fn a_second_run_clears_only_when_every_test_ran_and_passed_and_nothing_else() {
    let tests = [failed("b", "job::tests::flaky")];
    let ok = "    Starting 1 test across 1 binary\n        PASS [ 0.1s] b job::tests::flaky\n     Summary [ 0.1s] 1 test run: 1 passed, 0 skipped\n";
    assert!(all_passed(ok, &tests));
    // One that failed again, none that ran, and more than were asked for.
    for (what, log) in [
        ("failed again", ok.replace("1 passed", "0 passed, 1 failed")),
        ("none ran", "    Starting 0 tests across 0 binaries\n     Summary [ 0.0s] 0 tests run: 0 passed, 0 skipped\n".to_string()),
        ("a timeout", ok.replace("PASS [ 0.1s]", "TIMEOUT [ 9.0s]")),
        ("too many", ok.replace("1 test run: 1 passed", "2 tests run: 2 passed")),
        ("no summary", "    Starting 1 test across 1 binary\n".to_string()),
    ] {
        assert!(!all_passed(&log, &tests), "{what}:\n{log}");
    }
}

/// The second look builds under the gate's own cargo settings: with incremental compilation on,
/// cargo hashes every workspace crate afresh and rebuilds all of them beside the gate's build.
#[test]
fn the_second_look_builds_as_the_gate_does() {
    let env = super::tree::rerun_env();
    assert!(
        env.contains(&("CARGO_INCREMENTAL".to_string(), "0".to_string())),
        "{env:?}"
    );
    let gate = include_str!("../../../../scripts/gate.sh");
    assert!(
        gate.contains("export CARGO_INCREMENTAL=\"${CARGO_INCREMENTAL:-0}\""),
        "the gate's own setting is what this one follows"
    );
}
