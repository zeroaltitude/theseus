//! The keep-or-retire rules on their own (theseus-pw1q.21.8): what each
//! number does, in the order the rules are asked.

use std::collections::BTreeMap;

use super::retire::{attributed, decide, Evidence, Rules, Verdict, ATTRIBUTE_MS};
use crate::extend::usage::Call;
use crate::rsi::revert::signals::Red;

const RULES: Rules = Rules {
    retire_after_days: 14,
};

fn e(calls: u64, failures: u64, age_days: u32) -> Evidence {
    Evidence {
        calls,
        failures,
        age_days,
        red: None,
        by_hand: None,
    }
}

fn rule(v: Verdict) -> Option<&'static str> {
    match v {
        Verdict::Retire { rule, .. } => Some(rule),
        Verdict::Keep => None,
    }
}

/// Two failures in five calls (40%) retire now; one in five (exactly 20%) does
/// not, nor do failures among too few calls; a young tool is judged on its
/// failures alone.
#[test]
fn retire_on_the_failure_rate_rule() {
    let v = decide(&e(5, 2, 1), &RULES);
    assert_eq!(rule(v.clone()), Some("failures"));
    assert!(matches!(v, Verdict::Retire { why, .. } if why == "2 of 5 calls failed (40%)"));
    assert_eq!(
        rule(decide(&e(5, 1, 1), &RULES)),
        None,
        "20% is not above 20%"
    );
    assert_eq!(
        rule(decide(&e(4, 2, 1), &RULES)),
        None,
        "fewer than 5 calls"
    );
    assert_eq!(rule(decide(&e(100, 21, 3), &RULES)), Some("failures"));
    assert_eq!(rule(decide(&e(100, 20, 3), &RULES)), None);
}

/// A tool with fewer than 3 calls in `retire_after_days` retires once it is
/// that old: at 14 days, not at 13, and not with 3 calls.
#[test]
fn unused_retires_at_14_days() {
    let v = decide(&e(0, 0, 14), &RULES);
    assert!(matches!(&v, Verdict::Retire { rule: "unused", why } if why == "0 calls in 14 days"));
    assert_eq!(rule(decide(&e(2, 0, 14), &RULES)), Some("unused"));
    assert_eq!(
        rule(decide(&e(2, 0, 13), &RULES)),
        None,
        "not yet 14 days old"
    );
    assert_eq!(
        rule(decide(&e(3, 0, 14), &RULES)),
        None,
        "three calls are use"
    );
    let r = Rules {
        retire_after_days: 30,
    };
    assert_eq!(rule(decide(&e(0, 0, 29), &r)), None);
    assert_eq!(rule(decide(&e(0, 0, 30), &r)), Some("unused"));
}

/// A red attributed to its call retires it now, whatever else is true; a
/// motif whose by-hand runs did not fall retires it once the window has passed.
#[test]
fn a_red_and_a_motif_that_did_not_fall_retire_it() {
    let mut x = e(40, 0, 1);
    x.red = Some("the error rate doubled".into());
    assert_eq!(rule(decide(&x, &RULES)), Some("health"));
    let mut x = e(40, 0, 14);
    x.by_hand = Some((11, 11));
    let v = decide(&x, &RULES);
    assert!(
        matches!(&v, Verdict::Retire { rule: "not-falling", why }
        if why == "by-hand runs of its motif did not fall: 11 then, 11 now"),
        "{v:?}"
    );
    x.by_hand = Some((11, 4));
    assert_eq!(rule(decide(&x, &RULES)), None, "falling");
    x.by_hand = Some((11, 11));
    x.age_days = 13;
    assert_eq!(rule(decide(&x, &RULES)), None, "the window has not passed");
}

fn calls(at: &[u64]) -> Vec<Call> {
    at.iter()
        .map(|&at_ms| Call {
            at_ms,
            ms: 5,
            failed: false,
        })
        .collect()
}

/// A red is a tool's only when it is the one tool with a call just before it,
/// and was loaded then.
#[test]
fn a_red_is_attributed_to_the_one_tool_called_just_before_it() {
    let red = Red {
        signal: "health",
        at_ms: 10 * ATTRIBUTE_MS,
        commit: None,
        detail: "errors up".into(),
    };
    let mut by: BTreeMap<String, Vec<Call>> = BTreeMap::new();
    let at: BTreeMap<String, u64> = [("flow:a".to_string(), 0), ("ext:b".to_string(), 0)].into();
    by.insert("flow:a".into(), calls(&[red.at_ms - 5 * 60_000]));
    by.insert("ext:b".into(), calls(&[red.at_ms - 2 * ATTRIBUTE_MS]));
    assert_eq!(attributed(&red, &by, &at), Some("flow:a"));
    // Two suspects: nobody's.
    by.insert("ext:b".into(), calls(&[red.at_ms - 60_000]));
    assert_eq!(attributed(&red, &by, &at), None);
    // A call after the red is not its cause.
    by.remove("ext:b");
    by.insert("flow:a".into(), calls(&[red.at_ms + 1000]));
    assert_eq!(attributed(&red, &by, &at), None);
    // Nor a tool loaded after it.
    by.insert("flow:a".into(), calls(&[red.at_ms - 1000]));
    let late: BTreeMap<String, u64> = [("flow:a".to_string(), red.at_ms + 1)].into();
    assert_eq!(attributed(&red, &by, &late), None);
}
