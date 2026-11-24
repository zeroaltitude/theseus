//! `loop.v2` (issue 46): `loop.v1` without the four questions no code read and the three rollback rules no
//! code fed. The two questions that decide the stop nudge are v1's, field for field; v1 is retired, its file
//! kept as it was.

use crate::learn::RollbackRule;
use crate::pack::{by_name, Pack};

/// The questions loop.v2 drops: their answers were recorded and nothing read them.
const DROPPED: [&str; 4] = [
    "stopping_point_defined",
    "same_action_repeating",
    "cost_out_of_proportion",
    "wants_human_input",
];

fn ids(p: &Pack) -> Vec<String> {
    p.questions.iter().map(|q| q.id.clone()).collect()
}

/// v2 is v1 parsed, minus [`DROPPED`] and every rule but `nudge_loop`; everything else is equal.
#[test]
fn loop_v2_is_loop_v1_without_the_unread_questions_and_the_unfed_rules() {
    let v1 = by_name("loop.v1")
        .or_else(|| crate::pack::retired("loop.v1"))
        .unwrap();
    let v2 = by_name("loop.v2").expect("loop.v2 is embedded");
    assert_eq!((v1.version, v2.version), (1, 2));
    for q in DROPPED {
        assert!(ids(&v1).contains(&q.to_string()), "{q} in v1");
    }
    assert_eq!(v1.rollback.len(), 4);
    let mut planted = (*v1).clone();
    planted.version = 2;
    planted.sha256 = v2.sha256.clone();
    planted
        .questions
        .retain(|q| !DROPPED.contains(&q.id.as_str()));
    planted
        .rollback
        .retain(|r| matches!(r, RollbackRule::NudgeLoop));
    assert_eq!(planted, *v2);
    assert_eq!(ids(&v2), ["announced_unfinished", "work_state"]);
}
