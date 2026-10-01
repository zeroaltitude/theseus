//! The list's types, read from `guardrails.toml`, and the rules a list keeps to be loaded at all.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::Deserialize;

use crate::eval::{glob, OneOrMany, When};

/// The operator limit an entry serves (the design's §2: the budget, and the SOC2 stance).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Limit {
    PublicIngress,
    AuditTrail,
    Encryption,
    Credentials,
    Budget,
    /// The guards' own integrity: what would undo the others.
    Guard,
}

impl Limit {
    pub fn label(self) -> &'static str {
        match self {
            Limit::PublicIngress => "public ingress",
            Limit::AuditTrail => "the audit trail",
            Limit::Encryption => "encryption at rest",
            Limit::Credentials => "long-lived credentials",
            Limit::Budget => "the budget",
            Limit::Guard => "the guards",
        }
    }

    /// Which of the operator's two hard limits it holds.
    pub fn serves(self) -> &'static str {
        match self {
            Limit::PublicIngress | Limit::AuditTrail | Limit::Encryption | Limit::Credentials => {
                "SOC2"
            }
            Limit::Budget => "budget",
            Limit::Guard => "SOC2 and budget",
        }
    }
}

/// An entry's AWS-side form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scp {
    /// Denied to everyone: the session guard, and an SCP the deployer is under too.
    Deny,
    /// Denied by the session guard, and by an SCP to all but the deployer role, so a stack may do it
    /// after the gate's scan asked.
    DenyExceptDeployer,
    /// The gate alone; the entry's note says what holds AWS-side instead.
    None,
}

/// An IAM condition block: operator, then key, then values.
pub type IamCondition = BTreeMap<String, BTreeMap<String, OneOrMany>>;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Guardrail {
    pub name: String,
    pub limit: Limit,
    pub summary: String,
    pub operations: Vec<String>,
    #[serde(default)]
    pub iam: Option<Vec<String>>,
    #[serde(default)]
    pub resources: Option<Vec<String>>,
    #[serde(default)]
    pub direct: bool,
    pub scp: Scp,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub when: Option<When>,
    #[serde(default)]
    pub iam_condition: Option<IamCondition>,
    #[serde(default)]
    pub template: Vec<TemplateRule>,
    #[serde(default)]
    pub change: Vec<ChangeRule>,
}

impl Guardrail {
    /// The IAM actions the AWS side denies.
    pub fn iam_actions(&self) -> &[String] {
        self.iam.as_deref().unwrap_or(&self.operations)
    }

    /// Does AWS refuse this entry's calls too? Then an approved call runs in a floor session.
    pub fn guarded(&self) -> bool {
        self.scp != Scp::None
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateRule {
    pub types: Vec<String>,
    #[serde(default)]
    pub when: Option<When>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeRule {
    pub types: Vec<String>,
    /// A glob over the resource's physical id; absent, every resource of the types.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IacGroup {
    pub group: String,
    pub operations: Vec<String>,
    #[serde(default)]
    pub iam: Option<Vec<String>>,
    #[serde(default)]
    pub resources: Option<Vec<String>>,
}

impl IacGroup {
    pub fn iam_actions(&self) -> &[String] {
        self.iam.as_deref().unwrap_or(&self.operations)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Destructive {
    pub operations: Vec<String>,
    pub stateful_types: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Deployer {
    role: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Boundary {
    #[serde(default)]
    compact: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stacks {
    #[serde(default)]
    operations: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    deployer: Deployer,
    guardrail: Vec<Guardrail>,
    iac: Vec<IacGroup>,
    #[serde(default)]
    stacks: Stacks,
    destructive: Destructive,
    #[serde(default)]
    boundary: Boundary,
}

/// The loaded list, indexed by operation.
#[derive(Debug)]
pub struct GuardList {
    /// The one role SCPs let through, for stacks: CloudFormation's service role.
    pub deployer_role: String,
    pub guardrails: Vec<Guardrail>,
    pub iac: Vec<IacGroup>,
    /// The stack path: what only the stack tools do, since a stack writes with the deployer's power.
    /// CloudFormation's operations are their own IAM actions.
    pub stacks: Vec<String>,
    pub destructive: Destructive,
    /// The patterns that compact the boundary's unconditional denies, so every guard fits one policy.
    pub compact: Vec<String>,
    by_op: HashMap<String, Vec<usize>>,
    iac_ops: HashSet<String>,
    stack_ops: HashSet<String>,
    destructive_ops: HashSet<String>,
}

/// The verbs a compaction pattern may start with: writes to infrastructure, IAM, and posture. Reads
/// (Describe, Get, List), runs (Run, Start, Invoke), data (Send, Publish), passing a role, and tags
/// are not among them, so no pattern can deny one.
const WRITE_VERBS: [&str; 31] = [
    "Accept",
    "Add",
    "Allocate",
    "Associate",
    "Attach",
    "Authorize",
    "Change",
    "Copy",
    "Create",
    "Delete",
    "Deregister",
    "Detach",
    "Disable",
    "Disassociate",
    "Enable",
    "Execute",
    "Import",
    "Leave",
    "Modify",
    "Put",
    "Register",
    "Remove",
    "Replace",
    "Request",
    "Reset",
    "Restore",
    "Revoke",
    "Set",
    "Stop",
    "Update",
    "Upload",
];

#[derive(Debug, thiserror::Error)]
pub enum ListError {
    #[error("the guardrail list does not parse: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("the guardrail list breaks its rules: {}", .0.join("; "))]
    Invalid(Vec<String>),
}

impl GuardList {
    pub fn parse(text: &str) -> Result<GuardList, ListError> {
        let raw: Raw = toml::from_str(text)?;
        let mut by_op: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, g) in raw.guardrail.iter().enumerate() {
            for op in &g.operations {
                by_op.entry(op.clone()).or_default().push(i);
            }
        }
        let iac_ops = raw
            .iac
            .iter()
            .flat_map(|g| g.operations.iter().cloned())
            .collect();
        let destructive_ops = raw.destructive.operations.iter().cloned().collect();
        let list = GuardList {
            deployer_role: raw.deployer.role,
            guardrails: raw.guardrail,
            iac: raw.iac,
            destructive: raw.destructive,
            stack_ops: raw.stacks.operations.iter().cloned().collect(),
            stacks: raw.stacks.operations,
            compact: raw.boundary.compact,
            by_op,
            iac_ops,
            destructive_ops,
        };
        let problems = list.problems();
        if problems.is_empty() {
            Ok(list)
        } else {
            Err(ListError::Invalid(problems))
        }
    }

    pub fn guardrail(&self, name: &str) -> Option<&Guardrail> {
        self.guardrails.iter().find(|g| g.name == name)
    }

    /// The entries that name an operation, in the list's order.
    pub fn for_operation<'l>(&'l self, op: &str) -> impl Iterator<Item = &'l Guardrail> + 'l {
        self.by_op
            .get(op)
            .map(|v| v.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|&i| &self.guardrails[i])
    }

    /// Does the operation make or change durable infrastructure, which only a stack may?
    pub fn iac_only(&self, op: &str) -> bool {
        self.iac_ops.contains(op)
    }

    /// Is the operation a stack write, which only the stack tools make?
    pub fn stack_only(&self, op: &str) -> bool {
        self.stack_ops.contains(op)
    }

    /// Is the operation on the approve list: deleting a stateful resource, or a stack?
    pub fn destructive(&self, op: &str) -> bool {
        self.destructive_ops.contains(op)
    }

    /// Does replacing or removing a resource of this type lose state?
    pub fn stateful(&self, resource_type: &str) -> bool {
        self.destructive
            .stateful_types
            .iter()
            .any(|t| t == resource_type)
    }

    /// The IAM actions a hand's guards (the limits, IaC, and the stack path) deny on every resource with
    /// no condition, once each, in the list's order: what the boundary's compaction works on.
    pub(crate) fn unconditional_denies(&self) -> Vec<String> {
        let entries = self
            .guardrails
            .iter()
            .filter(|g| g.guarded() && g.iam_condition.is_none() && g.resources.is_none())
            .map(Guardrail::iam_actions);
        let groups = self
            .iac
            .iter()
            .filter(|g| g.resources.is_none())
            .map(IacGroup::iam_actions);
        let mut out: Vec<String> = Vec::new();
        for a in entries
            .chain(groups)
            .chain([self.stacks.as_slice()])
            .flatten()
        {
            if !out.contains(a) {
                out.push(a.clone());
            }
        }
        out
    }

    fn problems(&self) -> Vec<String> {
        let mut p = Vec::new();
        if !self
            .deployer_role
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+=,.@_-".contains(c))
            || self.deployer_role.is_empty()
        {
            p.push(format!(
                "the deployer role {:?} is not a role name",
                self.deployer_role
            ));
        }
        let mut names = HashSet::new();
        for g in &self.guardrails {
            let n = &g.name;
            if !names.insert(n.as_str()) {
                p.push(format!("{n}: the name is used twice"));
            }
            if !kebab(n) {
                p.push(format!("{n}: a name is lowercase words joined by hyphens"));
            }
            if g.summary.trim().is_empty() {
                p.push(format!("{n}: no summary"));
            }
            if g.operations.is_empty() {
                p.push(format!("{n}: no operations"));
            }
            let mut seen = HashSet::new();
            for op in &g.operations {
                if !operation_name(op) {
                    p.push(format!("{n}: {op:?} is not service:Operation"));
                }
                if !seen.insert(op) {
                    p.push(format!("{n}: {op} is named twice"));
                }
            }
            match g.scp {
                Scp::None => {
                    if g.iam.is_some() || g.resources.is_some() || g.iam_condition.is_some() {
                        p.push(format!(
                            "{n}: scp = \"none\" has no AWS side, so iam, resources, and iam_condition would go unused"
                        ));
                    }
                }
                Scp::Deny | Scp::DenyExceptDeployer => {
                    for a in g.iam_actions() {
                        if !iam_action(a) {
                            p.push(format!("{n}: {a:?} is not an IAM action"));
                        }
                    }
                }
            }
            if let Some(res) = &g.resources {
                p.extend(resource_problems(n, res));
            }
            if let Some(c) = &g.iam_condition {
                p.extend(condition_problems(n, c));
            }
            if g.when.as_ref().is_some_and(When::is_empty) {
                p.push(format!("{n}: an empty `when` never holds"));
            }
            // The two enforcers must agree on which calls the entry covers. A `when` narrows the gate; when
            // AWS would refuse every call of a direct operation, the gate must narrow AWS too.
            if g.when.is_some() && g.guarded() && g.iam_condition.is_none() && g.resources.is_none()
            {
                for op in g.operations.iter().filter(|op| !self.iac_only(op)) {
                    p.push(format!(
                        "{n}: its `when` narrows the gate, but AWS would refuse every {op}: give it an iam_condition or resources, or scp = \"none\""
                    ));
                }
            }
            if g.direct && g.when.is_none() {
                for op in g.operations.iter().filter(|op| self.iac_only(op)) {
                    p.push(format!(
                        "{n}: direct, but {op} is IaC-only and no `when` says which calls are direct"
                    ));
                }
            }
            for (i, t) in g.template.iter().enumerate() {
                if t.types.is_empty() {
                    p.push(format!("{n}: template rule {i} names no types"));
                }
                for ty in &t.types {
                    if !resource_type(ty) {
                        p.push(format!("{n}: {ty:?} is not a resource type"));
                    }
                }
                if t.when.as_ref().is_some_and(When::is_empty) {
                    p.push(format!("{n}: template rule {i} has an empty `when`"));
                }
            }
            for c in &g.change {
                for ty in &c.types {
                    if !resource_type(ty) {
                        p.push(format!("{n}: {ty:?} is not a resource type"));
                    }
                }
                if c.name.as_deref().is_some_and(|s| s.trim().is_empty()) {
                    p.push(format!("{n}: a change rule's name is empty"));
                }
            }
        }
        let mut groups = HashSet::new();
        for g in &self.iac {
            if !groups.insert(g.group.as_str()) {
                p.push(format!("iac {}: the group is named twice", g.group));
            }
            if g.operations.is_empty() && g.iam.is_none() {
                p.push(format!("iac {}: no operations", g.group));
            }
            for op in &g.operations {
                if !operation_name(op) {
                    p.push(format!("iac {}: {op:?} is not service:Operation", g.group));
                }
            }
            for a in g.iam_actions() {
                if !iam_action(a) {
                    p.push(format!("iac {}: {a:?} is not an IAM action", g.group));
                }
            }
            if let Some(res) = &g.resources {
                p.extend(resource_problems(&format!("iac {}", g.group), res));
            }
        }
        for op in &self.destructive.operations {
            if !operation_name(op) {
                p.push(format!("destructive: {op:?} is not service:Operation"));
            }
        }
        for ty in &self.destructive.stateful_types {
            if !resource_type(ty) {
                p.push(format!("destructive: {ty:?} is not a resource type"));
            }
        }
        let denies = self.unconditional_denies();
        for pat in &self.compact {
            if !compaction_pattern(pat) {
                p.push(format!(
                    "boundary: {pat:?} is not service:Verb… with a wildcard, starting with a write verb"
                ));
            } else if !denies.iter().any(|a| glob(pat, a)) {
                p.push(format!("boundary: {pat} compacts nothing the guards deny"));
            }
        }
        for op in &self.stacks {
            if !operation_name(op) || !op.starts_with("cloudformation:") {
                p.push(format!("stacks: {op:?} is not cloudformation:Operation"));
            }
            if self.iac_only(op) {
                p.push(format!(
                    "stacks: {op} is IaC-only too, and the work session's stack tools need it"
                ));
            }
        }
        p
    }
}

/// `service:VerbStem*`: an IAM action pattern with a wildcard, whose action starts with a write verb.
fn compaction_pattern(s: &str) -> bool {
    let Some((_, act)) = s.split_once(':') else {
        return false;
    };
    iam_action(s)
        && act.contains('*')
        && WRITE_VERBS.iter().any(|v| {
            act.strip_prefix(v)
                .is_some_and(|rest| rest.starts_with(|c: char| c == '*' || c.is_ascii_uppercase()))
        })
}

fn kebab(s: &str) -> bool {
    !s.is_empty()
        && s.split('-').all(|w| {
            !w.is_empty()
                && w.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}

/// `service:Operation`, by the service's botocore id.
fn operation_name(s: &str) -> bool {
    let Some((svc, op)) = s.split_once(':') else {
        return false;
    };
    !svc.is_empty()
        && svc
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && op.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && op.chars().all(|c| c.is_ascii_alphanumeric())
}

fn iam_action(s: &str) -> bool {
    let Some((svc, act)) = s.split_once(':') else {
        return false;
    };
    !svc.is_empty()
        && svc
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !act.is_empty()
        && act.chars().all(|c| c.is_ascii_alphanumeric() || c == '*')
}

fn resource_type(s: &str) -> bool {
    let parts: Vec<&str> = s.split("::").collect();
    parts.len() == 3
        && parts[0] == "AWS"
        && parts[1..]
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric()))
}

fn resource_problems(n: &str, res: &[String]) -> Vec<String> {
    let mut p = Vec::new();
    if res.is_empty() {
        p.push(format!("{n}: resources is empty"));
    }
    for r in res {
        if r != "*" && !r.starts_with("arn:aws:") {
            p.push(format!("{n}: {r:?} is not an ARN"));
        }
    }
    p
}

const OPERATORS: [&str; 24] = [
    "StringEquals",
    "StringNotEquals",
    "StringEqualsIgnoreCase",
    "StringNotEqualsIgnoreCase",
    "StringLike",
    "StringNotLike",
    "NumericEquals",
    "NumericNotEquals",
    "NumericLessThan",
    "NumericLessThanEquals",
    "NumericGreaterThan",
    "NumericGreaterThanEquals",
    "DateEquals",
    "DateNotEquals",
    "DateLessThan",
    "DateGreaterThan",
    "Bool",
    "BinaryEquals",
    "IpAddress",
    "NotIpAddress",
    "ArnEquals",
    "ArnLike",
    "ArnNotEquals",
    "ArnNotLike",
];

fn condition_problems(n: &str, c: &IamCondition) -> Vec<String> {
    let mut p = Vec::new();
    if c.is_empty() {
        p.push(format!("{n}: iam_condition is empty"));
    }
    for (op, keys) in c {
        let base = op
            .strip_prefix("ForAnyValue:")
            .or_else(|| op.strip_prefix("ForAllValues:"))
            .unwrap_or(op);
        let base = base.strip_suffix("IfExists").unwrap_or(base);
        if base != "Null" && !OPERATORS.contains(&base) {
            p.push(format!("{n}: {op:?} is not an IAM condition operator"));
        }
        for key in keys.keys() {
            let ok = key
                .split_once(':')
                .is_some_and(|(svc, k)| !svc.is_empty() && !k.is_empty());
            if !ok {
                p.push(format!("{n}: {key:?} is not a condition key"));
            }
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
[deployer]
role = "theseus-cfn-deployer"

[[guardrail]]
name = "trail-stop"
limit = "audit-trail"
summary = "stopping the trail"
operations = ["cloudtrail:StopLogging"]
direct = true
scp = "deny"

[[iac]]
group = "network"
operations = ["ec2:CreateVpc"]

[destructive]
operations = ["s3:DeleteBucket"]
stateful_types = ["AWS::S3::Bucket"]
"#;

    #[test]
    fn a_minimal_list_loads_and_indexes() {
        let l = GuardList::parse(MINIMAL).unwrap();
        assert_eq!(l.for_operation("cloudtrail:StopLogging").count(), 1);
        assert!(l.iac_only("ec2:CreateVpc"));
        assert!(l.destructive("s3:DeleteBucket"));
        assert!(l.stateful("AWS::S3::Bucket"));
    }

    #[test]
    fn the_rules_reject_what_would_split_the_enforcers() {
        // A `when` on a direct operation with an unconditional AWS side: the gate would pass what AWS refuses.
        let bad = MINIMAL.replace(
            "direct = true\nscp = \"deny\"",
            "scp = \"deny\"\n[guardrail.when]\nequals = { Name = \"x\" }",
        );
        let e = GuardList::parse(&bad).unwrap_err().to_string();
        assert!(e.contains("narrows the gate"), "{e}");
        let unknown_key = MINIMAL.replace("direct = true", "direct = true\ncolour = \"red\"");
        assert!(matches!(
            GuardList::parse(&unknown_key),
            Err(ListError::Parse(_))
        ));
        let bad_op = MINIMAL.replace("cloudtrail:StopLogging", "cloudtrail stop");
        assert!(GuardList::parse(&bad_op)
            .unwrap_err()
            .to_string()
            .contains("not service:Operation"));
    }
}
