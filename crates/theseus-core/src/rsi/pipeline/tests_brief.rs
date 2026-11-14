//! A started item's first brief opens with the rules (issue 6, condition 1), then the
//! item's own task, then its tree.

use super::tests::rig_with;
use crate::rsi::backlog::brief::rules;

/// KEY: the objective a build is started with begins with the rules, whatever the item is.
#[tokio::test]
async fn a_started_build_is_told_the_rules_first() {
    let r = rig_with(1, 30.0);
    r.tick().await;
    let objectives = r.fake.objectives.lock().unwrap();
    assert_eq!(objectives.len(), 1);
    let o = &objectives[0];
    assert!(o.starts_with(&rules(3)), "{o}");
    assert!(o.contains("Your tree is "), "{o}");
    let (rules_at, task_at) = (
        o.find("Before you build").unwrap(),
        o.find("Fix the cause").unwrap_or(usize::MAX),
    );
    assert!(rules_at < task_at, "the rules come before the task: {o}");
}
