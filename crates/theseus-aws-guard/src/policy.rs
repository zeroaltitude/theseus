//! Enforcer 2: the list's AWS-side forms, generated, never written by hand (the design's §3.5, §3.6).
//!
//! - `theseus-guard-limits`: the guardrails' denies, a session policy on every work and job session.
//! - `theseus-guard-iac` (with `-2` and on, when one policy cannot hold it): the IaC-only list's denies,
//!   on the same sessions.
//! - `theseus-guard-stacks`: the stack path's writes, and passing the deployer, on job sessions only.
//! - `theseus-boundary`: allow-all less every guard, every hand role's permissions boundary (a boundary
//!   has to allow, so it cannot be the deny-only guard itself). A role has one boundary, so it is one
//!   policy, compacted to fit.
//! - `theseus-scp-guardrails` (and on): the guardrails as SCPs, for Eddie's management account. Nothing
//!   here applies them.

use std::collections::BTreeMap;

use serde::{Serialize, Serializer};
use serde_json::Value;

use crate::eval::{glob, OneOrMany};
use crate::list::{GuardList, IamCondition, Scp};

/// IAM's limit on a managed policy, whitespace not counted.
pub const MANAGED_POLICY_LIMIT: usize = 6_144;
/// Organizations' limit on an SCP.
pub const SCP_LIMIT: usize = 5_120;
/// SCPs one account or OU can hold, `FullAWSAccess` among them.
pub const SCPS_PER_TARGET: usize = 5;
/// Managed policies one role session can take (`PolicyArns`).
pub const SESSION_POLICY_ARNS: usize = 10;
/// STS's limit on a session's policy text: the managed policies' ARNs and the inline policy together.
pub const SESSION_POLICY_PLAINTEXT: usize = 2_048;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyKind {
    /// A deny-only managed policy passed as a session policy.
    Guard,
    /// A permissions boundary.
    Boundary,
    /// A service control policy.
    Scp,
}

impl PolicyKind {
    pub fn limit(self) -> usize {
        match self {
            PolicyKind::Scp => SCP_LIMIT,
            PolicyKind::Guard | PolicyKind::Boundary => MANAGED_POLICY_LIMIT,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Document {
    #[serde(rename = "Version")]
    version: &'static str,
    #[serde(rename = "Statement")]
    pub statement: Vec<Statement>,
}

impl Document {
    fn new(statement: Vec<Statement>) -> Document {
        Document {
            version: "2012-10-17",
            statement,
        }
    }
}

type Condition = BTreeMap<String, BTreeMap<String, Value>>;

#[derive(Clone, Debug, Serialize)]
pub struct Statement {
    #[serde(rename = "Sid")]
    pub sid: String,
    #[serde(rename = "Effect")]
    pub effect: &'static str,
    #[serde(rename = "Action", serialize_with = "one_or_many")]
    pub action: Vec<String>,
    #[serde(rename = "Resource", serialize_with = "one_or_many")]
    pub resource: Vec<String>,
    #[serde(rename = "Condition", skip_serializing_if = "Option::is_none")]
    pub condition: Option<Condition>,
}

fn one_or_many<S: Serializer>(v: &[String], s: S) -> Result<S::Ok, S::Error> {
    match v {
        [one] => s.serialize_str(one),
        many => many.serialize(s),
    }
}

#[derive(Clone, Debug)]
pub struct Policy {
    pub name: String,
    pub kind: PolicyKind,
    pub document: Document,
}

impl Policy {
    pub fn minified(&self) -> String {
        serde_json::to_string(&self.document).expect("a policy document serializes")
    }

    pub fn pretty(&self) -> String {
        let mut s =
            serde_json::to_string_pretty(&self.document).expect("a policy document serializes");
        s.push('\n');
        s
    }

    /// The minified size. IAM counts less (no whitespace at all), so this errs toward the limit.
    pub fn size(&self) -> usize {
        self.minified().chars().count()
    }

    pub fn fits(&self) -> bool {
        self.size() <= self.kind.limit()
    }

    /// Every action the policy denies, as written.
    pub fn denied_actions(&self) -> impl Iterator<Item = &str> {
        self.document
            .statement
            .iter()
            .filter(|s| s.effect == "Deny")
            .flat_map(|s| s.action.iter().map(String::as_str))
    }
}

fn sid(name: &str) -> String {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

fn condition_json(c: &IamCondition) -> Condition {
    c.iter()
        .map(|(op, keys)| {
            let keys = keys
                .iter()
                .map(|(k, v)| {
                    let v = match v {
                        OneOrMany::One(s) => Value::String(s.clone()),
                        OneOrMany::Many(l) => {
                            Value::Array(l.iter().cloned().map(Value::String).collect())
                        }
                    };
                    (k.clone(), v)
                })
                .collect();
            (op.clone(), keys)
        })
        .collect()
}

fn push_unique(into: &mut Vec<String>, actions: &[String]) {
    for a in actions {
        if !into.contains(a) {
            into.push(a.clone());
        }
    }
}

fn deny(
    sid: String,
    action: Vec<String>,
    resource: Vec<String>,
    condition: Option<Condition>,
) -> Statement {
    Statement {
        sid,
        effect: "Deny",
        action,
        resource,
        condition,
    }
}

impl GuardList {
    /// Every generated policy, in a stable order: the guards, the boundary, then the SCPs.
    pub fn policies(&self) -> Vec<Policy> {
        let mut out = self.guard_limits();
        out.extend(self.guard_iac());
        out.extend(self.guard_stacks());
        out.push(self.boundary());
        out.extend(self.scps());
        out
    }

    /// The stack path's denies: its writes, and passing the deployer at all.
    fn stacks_statements(&self) -> Vec<Statement> {
        vec![
            deny(
                "StackPath".into(),
                self.stacks.clone(),
                vec!["*".into()],
                None,
            ),
            deny(
                "PassTheDeployer".into(),
                vec!["iam:PassRole".into()],
                vec![format!("arn:aws:iam::*:role/{}", self.deployer_role)],
                None,
            ),
        ]
    }

    /// `theseus-guard-stacks`: on every job session beside the other guards, and in the boundary; never
    /// on the work session, whose stack tools write stacks.
    pub fn guard_stacks(&self) -> Vec<Policy> {
        pack(
            "theseus-guard-stacks",
            PolicyKind::Guard,
            self.stacks_statements(),
        )
    }

    /// The deployer's exemption, as an SCP condition.
    fn except_deployer(&self, cond: &mut Condition) {
        cond.entry("ArnNotLike".into()).or_default().insert(
            "aws:PrincipalArn".into(),
            Value::String(format!("arn:aws:iam::*:role/{}", self.deployer_role)),
        );
    }

    /// The guardrails' denies. Entries with no condition and no resources merge into one statement per
    /// form; the rest keep their own, named for the entry.
    fn limits_statements(&self, scp: bool) -> Vec<Statement> {
        let mut everyone: Vec<String> = Vec::new();
        let mut but_deployer: Vec<String> = Vec::new();
        let mut own = Vec::new();
        for g in self.guardrails.iter().filter(|g| g.guarded()) {
            let except = scp && g.scp == Scp::DenyExceptDeployer;
            if g.iam_condition.is_none() && g.resources.is_none() {
                push_unique(
                    if except {
                        &mut but_deployer
                    } else {
                        &mut everyone
                    },
                    g.iam_actions(),
                );
                continue;
            }
            let mut cond = g
                .iam_condition
                .as_ref()
                .map(condition_json)
                .unwrap_or_default();
            if except {
                self.except_deployer(&mut cond);
            }
            own.push(deny(
                sid(&g.name),
                g.iam_actions().to_vec(),
                g.resources.clone().unwrap_or_else(|| vec!["*".into()]),
                (!cond.is_empty()).then_some(cond),
            ));
        }
        let mut out = Vec::new();
        if !everyone.is_empty() {
            let name = if scp {
                "GuardrailsEveryone"
            } else {
                "Guardrails"
            };
            out.push(deny(name.into(), everyone, vec!["*".into()], None));
        }
        if !but_deployer.is_empty() {
            let mut cond = Condition::new();
            self.except_deployer(&mut cond);
            out.push(deny(
                "GuardrailsAllButTheDeployer".into(),
                but_deployer,
                vec!["*".into()],
                Some(cond),
            ));
        }
        out.extend(own);
        out
    }

    pub fn guard_limits(&self) -> Vec<Policy> {
        pack(
            "theseus-guard-limits",
            PolicyKind::Guard,
            self.limits_statements(false),
        )
    }

    fn iac_statements(&self) -> Vec<Statement> {
        let mut merged = Vec::new();
        let mut own = Vec::new();
        for g in &self.iac {
            match &g.resources {
                Some(res) => own.push(deny(
                    sid(&g.group),
                    g.iam_actions().to_vec(),
                    res.clone(),
                    None,
                )),
                None => push_unique(&mut merged, g.iam_actions()),
            }
        }
        let mut statements = vec![deny("IacOnly".into(), merged, vec!["*".into()], None)];
        statements.extend(own);
        statements
    }

    pub fn guard_iac(&self) -> Vec<Policy> {
        pack(
            "theseus-guard-iac",
            PolicyKind::Guard,
            self.iac_statements(),
        )
    }

    /// Allow-all less every guard (the limits, IaC, and the stack path), in one policy: every hand
    /// role's permissions boundary (§3.5). The unconditional denies are compacted by the list's
    /// `[boundary]` patterns, and a deny is left out where another already refuses the same.
    pub fn boundary(&self) -> Policy {
        let denies = self.unconditional_denies();
        let patterns: Vec<String> = self
            .compact
            .iter()
            .filter(|p| denies.iter().any(|a| glob(p, a)))
            .cloned()
            .collect();
        let mut everywhere = patterns.clone();
        everywhere.extend(
            denies
                .into_iter()
                .filter(|a| !patterns.iter().any(|p| glob(p, a))),
        );
        let mut statements = vec![
            Statement {
                sid: "AllowAll".into(),
                effect: "Allow",
                action: vec!["*".into()],
                resource: vec!["*".into()],
                condition: None,
            },
            deny("Guards".into(), everywhere.clone(), vec!["*".into()], None),
        ];
        let scoped: Vec<Statement> = self
            .limits_statements(false)
            .into_iter()
            .chain(self.iac_statements())
            .chain(self.stacks_statements())
            .filter(|s| s.condition.is_some() || s.resource != ["*"])
            .collect();
        // A deny is left out where another refuses the same action with no condition: everywhere; or
        // on the same resources, when it has a condition; or on a strictly larger set of them. A deny
        // with no condition is never left out for one with a condition or with fewer resources, so no
        // two denies drop each other.
        let covered = |i: usize, s: &Statement, a: &str| {
            everywhere.iter().any(|p| glob(p, a))
                || scoped.iter().enumerate().any(|(j, t)| {
                    j != i
                        && t.condition.is_none()
                        && (s.condition.is_some() || t.resource.len() > s.resource.len())
                        && s.resource.iter().all(|r| t.resource.contains(r))
                        && t.action.iter().any(|p| glob(p, a))
                })
        };
        for (i, s) in scoped.iter().enumerate() {
            let mut s = s.clone();
            s.action = s
                .action
                .iter()
                .filter(|a| !covered(i, &s, a))
                .cloned()
                .collect();
            if !s.action.is_empty() {
                statements.push(s);
            }
        }
        Policy {
            name: "theseus-boundary".into(),
            kind: PolicyKind::Boundary,
            document: Document::new(statements),
        }
    }

    pub fn scps(&self) -> Vec<Policy> {
        pack(
            "theseus-scp-guardrails",
            PolicyKind::Scp,
            self.limits_statements(true),
        )
    }
}

/// Pack statements into as few documents as fit the kind's limit, splitting a statement whose actions
/// alone would not fit. Names run `base`, `base-2`, and on.
fn pack(base: &str, kind: PolicyKind, statements: Vec<Statement>) -> Vec<Policy> {
    let fits = |s: &[Statement]| {
        serde_json::to_string(&Document::new(s.to_vec()))
            .expect("a policy document serializes")
            .chars()
            .count()
            <= kind.limit()
    };
    let mut pieces = Vec::new();
    for s in statements {
        if fits(std::slice::from_ref(&s)) {
            pieces.push(s);
            continue;
        }
        let mut part = s.clone();
        part.action.clear();
        let mut n = 1;
        for a in &s.action {
            part.action.push(a.clone());
            if !fits(std::slice::from_ref(&part)) && part.action.len() > 1 {
                let last = part.action.pop().expect("the action just pushed");
                pieces.push(part.clone());
                n += 1;
                part.sid = format!("{}{n}", s.sid);
                part.action = vec![last];
            }
        }
        pieces.push(part);
    }
    let mut docs: Vec<Vec<Statement>> = Vec::new();
    for s in pieces {
        match docs.last_mut() {
            Some(d) if fits(&[d.as_slice(), std::slice::from_ref(&s)].concat()) => d.push(s),
            _ => docs.push(vec![s]),
        }
    }
    docs.into_iter()
        .enumerate()
        .map(|(i, statement)| Policy {
            name: if i == 0 {
                base.to_string()
            } else {
                format!("{base}-{}", i + 1)
            },
            kind,
            document: Document::new(statement),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sids_are_alphanumeric() {
        assert_eq!(sid("sg-ingress-public"), "SgIngressPublic");
        assert_eq!(sid("edges: load balancers, APIs"), "EdgesLoadBalancersAPIs");
    }

    #[test]
    fn a_statement_too_big_for_one_policy_is_split() {
        let actions: Vec<String> = (0..400)
            .map(|i| format!("ec2:CreateSomethingNumber{i:04}"))
            .collect();
        let policies = pack(
            "theseus-guard-iac",
            PolicyKind::Guard,
            vec![deny(
                "IacOnly".into(),
                actions.clone(),
                vec!["*".into()],
                None,
            )],
        );
        assert!(policies.len() > 1);
        assert_eq!(policies[1].name, "theseus-guard-iac-2");
        let mut all: Vec<&str> = policies.iter().flat_map(|p| p.denied_actions()).collect();
        all.sort_unstable();
        let mut want: Vec<&str> = actions.iter().map(String::as_str).collect();
        want.sort_unstable();
        assert_eq!(all, want);
        assert!(policies.iter().all(Policy::fits));
    }
}
