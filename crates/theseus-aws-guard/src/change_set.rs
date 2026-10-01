//! A change set checked before `aws.stack.apply` executes it (the design's §3.4): a change to what a
//! guardrail protects (the trail, the budget, the guards) is the floor, and a stateful resource
//! replaced or removed waits for approval. The template's own scan has already run, at plan.

use crate::eval::glob;
use crate::list::{GuardList, Guardrail};

/// CloudFormation's `Action` for a resource change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeAction {
    Add,
    Modify,
    Remove,
    Import,
    /// Decided only when the change set runs (a nested stack's changes): treated as a modify.
    Dynamic,
}

/// One resource change, as `DescribeChangeSet` reports it.
#[derive(Clone, Debug)]
pub struct ResourceChange {
    pub logical_id: String,
    /// The resource's physical id; there is one for a modify or a removal.
    pub physical_id: Option<String>,
    pub resource_type: String,
    pub action: ChangeAction,
    /// `Replacement` is `True` or `Conditional`.
    pub replacement: bool,
}

#[derive(Debug)]
pub struct ChangeHit<'l> {
    /// The guardrail, for the floor; none for a destructive change.
    pub guardrail: Option<&'l Guardrail>,
    pub logical_id: String,
    pub resource_type: String,
    pub why: String,
}

#[derive(Debug, Default)]
pub struct ChangeSetVerdict<'l> {
    pub floor: Vec<ChangeHit<'l>>,
    pub destructive: Vec<ChangeHit<'l>>,
}

impl GuardList {
    pub fn check_change_set(&self, changes: &[ResourceChange]) -> ChangeSetVerdict<'_> {
        let mut v = ChangeSetVerdict::default();
        for c in changes {
            let touches = matches!(
                c.action,
                ChangeAction::Modify | ChangeAction::Remove | ChangeAction::Dynamic
            );
            if touches {
                for g in &self.guardrails {
                    let hit = g.change.iter().any(|rule| {
                        rule.types.contains(&c.resource_type)
                            && match (&rule.name, &c.physical_id) {
                                (None, _) => true,
                                (Some(n), Some(p)) => glob(n, p),
                                // A protected name, and no physical id to rule it out: ask.
                                (Some(_), None) => true,
                            }
                    });
                    if hit {
                        v.floor.push(ChangeHit {
                            guardrail: Some(g),
                            logical_id: c.logical_id.clone(),
                            resource_type: c.resource_type.clone(),
                            why: format!(
                                "{}: {} {} ({}), {}",
                                g.limit.label(),
                                verb(c),
                                c.logical_id,
                                c.resource_type,
                                g.summary
                            ),
                        });
                    }
                }
            }
            let loses_state = c.action == ChangeAction::Remove
                || (matches!(c.action, ChangeAction::Modify | ChangeAction::Dynamic)
                    && c.replacement);
            if loses_state && self.stateful(&c.resource_type) {
                v.destructive.push(ChangeHit {
                    guardrail: None,
                    logical_id: c.logical_id.clone(),
                    resource_type: c.resource_type.clone(),
                    why: format!(
                        "{} {} ({}), a stateful resource",
                        verb(c),
                        c.logical_id,
                        c.resource_type
                    ),
                });
            }
        }
        v
    }
}

fn verb(c: &ResourceChange) -> &'static str {
    match (c.action, c.replacement) {
        (ChangeAction::Remove, _) => "removes",
        (ChangeAction::Modify | ChangeAction::Dynamic, true) => "replaces",
        (ChangeAction::Modify | ChangeAction::Dynamic, false) => "modifies",
        (ChangeAction::Add, _) => "adds",
        (ChangeAction::Import, _) => "imports",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(
        id: &str,
        ty: &str,
        physical: Option<&str>,
        action: ChangeAction,
        replacement: bool,
    ) -> ResourceChange {
        ResourceChange {
            logical_id: id.into(),
            physical_id: physical.map(String::from),
            resource_type: ty.into(),
            action,
            replacement,
        }
    }

    #[test]
    fn protected_changes_ask_and_stateful_losses_wait() {
        let l = crate::embedded();
        let v = l.check_change_set(&[
            change(
                "Trail",
                "AWS::CloudTrail::Trail",
                Some("theseus-trail"),
                ChangeAction::Modify,
                false,
            ),
            change(
                "Budget",
                "AWS::Budgets::Budget",
                Some("theseus-monthly"),
                ChangeAction::Remove,
                false,
            ),
            change(
                "Deployer",
                "AWS::IAM::Role",
                Some("theseus-cfn-deployer"),
                ChangeAction::Modify,
                false,
            ),
            change(
                "HandRole",
                "AWS::IAM::Role",
                Some("theseus-hand-basic"),
                ChangeAction::Modify,
                false,
            ),
            change(
                "Data",
                "AWS::S3::Bucket",
                Some("theseus-data"),
                ChangeAction::Modify,
                true,
            ),
            change(
                "Logs",
                "AWS::Logs::LogGroup",
                Some("/theseus/hands"),
                ChangeAction::Modify,
                false,
            ),
            change(
                "NewTrail",
                "AWS::CloudTrail::Trail",
                None,
                ChangeAction::Add,
                false,
            ),
        ]);
        let floor: Vec<&str> = v.floor.iter().map(|h| h.logical_id.as_str()).collect();
        assert_eq!(floor, ["Trail", "Budget", "Deployer"]);
        let destructive: Vec<&str> = v
            .destructive
            .iter()
            .map(|h| h.logical_id.as_str())
            .collect();
        assert_eq!(destructive, ["Data"]);
        assert_eq!(
            v.floor[0].why,
            "the audit trail: modifies Trail (AWS::CloudTrail::Trail), deleting the trail, or changing what it records"
        );
    }
}
