//! The generated AWS side (the AWS design's §3.5 and §3.6): every policy fits its limit, a work session
//! can carry the guards, every entry is denied in each of its forms, and the copies in `policies/` and
//! the foundation template's guard documents are exactly what the list generates. After the list
//! changes, rewrite them with `THESEUS_GUARD_WRITE=1 cargo test -p theseus-aws-guard --test policies`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde_json::Value;
use theseus_aws_guard::{
    embedded, glob, IamCondition, Policy, Scp, Statement, SCPS_PER_TARGET, SESSION_POLICY_ARNS,
    SESSION_POLICY_PLAINTEXT,
};

fn policies_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("policies")
}

fn foundation_template() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../infra/aws/theseus-foundation.yaml")
}

/// The foundation stack makes the guards a session carries (C2, theseus-nyzn): each guard and the
/// boundary the list generates is a managed policy there by its name, its document the generated
/// one, minified on one line. So AWS enforces this list, the one the gate reads.
#[test]
fn the_foundation_template_carries_the_generated_guards() {
    let write = std::env::var_os("THESEUS_GUARD_WRITE").is_some();
    let path = foundation_template();
    let text = std::fs::read_to_string(&path).expect("infra/aws/theseus-foundation.yaml");
    let l = embedded();
    let generated: Vec<Policy> = l
        .guard_limits()
        .into_iter()
        .chain(l.guard_iac())
        .chain(l.guard_stacks())
        .chain(l.guard_deployer())
        .chain([l.boundary()])
        .collect();
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let mut stale = Vec::new();
    for p in &generated {
        let named = format!("ManagedPolicyName: {}", p.name);
        let Some(at) = lines.iter().position(|x| x.trim() == named) else {
            stale.push(format!(
                "{}: no managed policy of that name; add one",
                p.name
            ));
            continue;
        };
        let Some(doc) = lines[at..]
            .iter()
            .position(|x| x.trim_start().starts_with("PolicyDocument:"))
            .map(|i| at + i)
        else {
            stale.push(format!("{}: no PolicyDocument after its name", p.name));
            continue;
        };
        let want = format!("      PolicyDocument: {}", p.minified());
        if lines[doc] != want {
            if write {
                lines[doc] = want;
            } else {
                stale.push(p.name.clone());
            }
        }
    }
    for x in &lines {
        if let Some(rest) = x.trim().strip_prefix("ManagedPolicyName: theseus-guard-") {
            let name = format!("theseus-guard-{rest}");
            if !generated.iter().any(|p| p.name == name) {
                stale.push(format!(
                    "{name}: the list generates no such guard; remove it"
                ));
            }
        }
    }
    if write {
        std::fs::write(&path, lines.join("\n") + "\n").expect("the template is writable");
    }
    assert!(
        stale.is_empty(),
        "infra/aws/theseus-foundation.yaml is out of step with guardrails.toml ({}): rewrite it with THESEUS_GUARD_WRITE=1 cargo test -p theseus-aws-guard --test policies",
        stale.join("; ")
    );
}

#[test]
fn every_policy_fits_its_limit() {
    for p in embedded().policies() {
        assert!(
            p.fits(),
            "{} is {} characters, over {}",
            p.name,
            p.size(),
            p.kind.limit()
        );
    }
}

#[test]
fn the_scps_fit_beside_full_aws_access() {
    let n = embedded().scps().len();
    assert!(
        n < SCPS_PER_TARGET,
        "{n} SCPs, and FullAWSAccess, are more than an account or OU holds ({SCPS_PER_TARGET})"
    );
}

#[test]
fn a_session_carries_its_guards_and_allow_all() {
    // A session's managed session policies (§3.5): a work session's guards and theseus-allow-all, and
    // a job session's, which add the stack path's.
    let l = embedded();
    let work: Vec<String> = l
        .guard_limits()
        .iter()
        .chain(&l.guard_iac())
        .map(|p| p.name.clone())
        .chain(["theseus-allow-all".to_string()])
        .collect();
    let mut job = work.clone();
    job.extend(l.guard_stacks().iter().map(|p| p.name.clone()));
    for (session, names) in [("work", work), ("job", job)] {
        let arns: Vec<String> = names
            .iter()
            .map(|name| format!("arn:aws:iam::111122223333:policy/{name}"))
            .collect();
        assert!(
            arns.len() <= SESSION_POLICY_ARNS,
            "a {session} session takes {} policies",
            arns.len()
        );
        // STS counts the ARNs' text and a job's inline narrowing together; most of it stays the
        // narrowing's.
        let text: usize = arns.iter().map(String::len).sum();
        assert!(
            text <= SESSION_POLICY_PLAINTEXT / 4,
            "a {session} session's ARNs take {text} characters"
        );
    }
}

/// Does some deny in `policies` refuse `action` on at least everything `resources` names (all, when
/// none), under at most `condition` and the deployer's exemption?
fn denies(
    policies: &[Policy],
    action: &str,
    resources: Option<&[String]>,
    condition: Option<&IamCondition>,
    deployer_exempt: Option<&str>,
) -> bool {
    let resources = resources.map_or_else(|| vec!["*".to_string()], <[String]>::to_vec);
    let mut want: BTreeMap<String, BTreeMap<String, Value>> = BTreeMap::new();
    if let Some(c) = condition {
        for (op, keys) in c {
            for (k, v) in keys {
                let v = match v.values() {
                    [one] => Value::String(one.clone()),
                    many => Value::Array(many.iter().cloned().map(Value::String).collect()),
                };
                want.entry(op.clone()).or_default().insert(k.clone(), v);
            }
        }
    }
    if let Some(role) = deployer_exempt {
        want.entry("ArnNotLike".into()).or_default().insert(
            "aws:PrincipalArn".into(),
            Value::String(format!("arn:aws:iam::*:role/{role}")),
        );
    }
    let covers = |s: &Statement| {
        s.effect == "Deny"
            && s.action.iter().any(|a| glob(a, action))
            && (s.resource == ["*"] || resources.iter().all(|r| s.resource.contains(r)))
            && s.condition
                .clone()
                .unwrap_or_default()
                .iter()
                .all(|(op, keys)| {
                    keys.iter()
                        .all(|(k, v)| want.get(op).and_then(|w| w.get(k)) == Some(v))
                })
    };
    policies
        .iter()
        .flat_map(|p| &p.document.statement)
        .any(covers)
}

#[test]
fn every_entry_is_denied_in_each_of_its_forms() {
    // The one list kept in step with its AWS side (§3.6): every guarded entry's actions are denied by
    // the session guard, the boundary, and the SCPs, scoped as the entry scopes them; every IaC-only
    // action by the IaC guard and the boundary; and the stack path by its guard and the boundary.
    let l = embedded();
    let guard = l.guard_limits();
    let iac = l.guard_iac();
    let stacks = l.guard_stacks();
    let boundary = [l.boundary()];
    let scps = l.scps();
    let mut missing = Vec::new();
    for g in l.guardrails.iter().filter(|g| g.guarded()) {
        let exempt = (g.scp == Scp::DenyExceptDeployer).then_some(l.deployer_role.as_str());
        for a in g.iam_actions() {
            for (form, policies, exempt) in [
                ("theseus-guard-limits", guard.as_slice(), None),
                ("theseus-boundary", boundary.as_slice(), None),
                ("the SCPs", scps.as_slice(), exempt),
            ] {
                if !denies(
                    policies,
                    a,
                    g.resources.as_deref(),
                    g.iam_condition.as_ref(),
                    exempt,
                ) {
                    missing.push(format!("{}: {a} is not denied by {form}", g.name));
                }
            }
        }
    }
    for grp in &l.iac {
        for a in grp.iam_actions() {
            for (form, policies) in [
                ("theseus-guard-iac", iac.as_slice()),
                ("theseus-boundary", &boundary),
            ] {
                if !denies(policies, a, grp.resources.as_deref(), None, None) {
                    missing.push(format!("iac {}: {a} is not denied by {form}", grp.group));
                }
            }
        }
    }
    let deployer = [format!("arn:aws:iam::*:role/{}", l.deployer_role)];
    let stack_path = l
        .stacks
        .iter()
        .map(|a| (a.as_str(), None))
        .chain([("iam:PassRole", Some(deployer.as_slice()))]);
    for (a, resources) in stack_path {
        for (form, policies) in [
            ("theseus-guard-stacks", stacks.as_slice()),
            ("theseus-boundary", &boundary),
        ] {
            if !denies(policies, a, resources, None, None) {
                missing.push(format!("stacks: {a} is not denied by {form}"));
            }
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}

/// What stays direct (the design's §3.4: data, runs, operations on what exists, tags, change sets) and
/// what AWS services do with a role's own permissions: a VPC Lambda makes its network interfaces, and
/// EBS makes its KMS grants. No guard, and no compaction of the boundary, may deny one everywhere.
const DIRECT: [&str; 58] = [
    // data
    "s3:PutObject",
    "s3:PutObjectTagging",
    "s3:DeleteObject",
    "s3:AbortMultipartUpload",
    "dynamodb:PutItem",
    "dynamodb:UpdateItem",
    "dynamodb:DeleteItem",
    "dynamodb:BatchWriteItem",
    "sqs:SendMessage",
    "sqs:DeleteMessage",
    "sqs:ChangeMessageVisibility",
    "sns:Publish",
    "events:PutEvents",
    "logs:CreateLogStream",
    "logs:PutLogEvents",
    "cloudwatch:PutMetricData",
    "ssm:PutParameter",
    "secretsmanager:PutSecretValue",
    "kinesis:PutRecord",
    "kinesis:PutRecords",
    "firehose:PutRecord",
    // runs
    "ecs:RunTask",
    "ecs:StopTask",
    "ecs:RegisterTaskDefinition",
    "lambda:InvokeFunction",
    "states:StartExecution",
    "batch:SubmitJob",
    "batch:CancelJob",
    "codebuild:StartBuild",
    "athena:StartQueryExecution",
    "glue:StartJobRun",
    "sagemaker:CreateTrainingJob",
    "sagemaker:CreateProcessingJob",
    "sagemaker:InvokeEndpoint",
    "ssm:SendCommand",
    // operations on what exists
    "ec2:StartInstances",
    "ec2:StopInstances",
    "ec2:TerminateInstances",
    "ecs:UpdateService",
    "rds:StartDBInstance",
    "rds:StopDBInstance",
    "cloudfront:CreateInvalidation",
    "secretsmanager:RotateSecret",
    "lambda:PublishVersion",
    // tags and change sets
    "ec2:CreateTags",
    "tag:TagResources",
    "cloudformation:CreateChangeSet",
    "cloudformation:ExecuteChangeSet",
    // a role's permissions, used by AWS services on its behalf
    "ec2:CreateNetworkInterface",
    "ec2:DeleteNetworkInterface",
    "ec2:AssignPrivateIpAddresses",
    "kms:Decrypt",
    "kms:GenerateDataKey",
    "ecr:PutImage",
    "ecr:InitiateLayerUpload",
    "ecr:CompleteLayerUpload",
    "iam:PassRole",
    "sts:AssumeRole",
];

#[test]
fn no_guard_denies_a_direct_operation_everywhere() {
    // The work session's guards leave every direct operation. A job session's and a hand's leave all
    // but the stack path's: only the work session's stack tools write stacks.
    let l = embedded();
    let mut work = l.guard_limits();
    work.extend(l.guard_iac());
    let mut job_and_hand = l.guard_stacks();
    job_and_hand.push(l.boundary());
    let mut denied = Vec::new();
    for (policies, stack_tools) in [(work, true), (job_and_hand, false)] {
        for p in &policies {
            for s in p
                .document
                .statement
                .iter()
                .filter(|s| s.effect == "Deny" && s.condition.is_none() && s.resource == ["*"])
            {
                for op in DIRECT
                    .iter()
                    .filter(|op| stack_tools || !op.starts_with("cloudformation:"))
                {
                    if let Some(a) = s.action.iter().find(|a| glob(a, op)) {
                        denied.push(format!("{}: {a} denies {op}", p.name));
                    }
                }
            }
        }
    }
    assert!(denied.is_empty(), "{}", denied.join("\n"));
}

#[test]
fn the_boundary_is_one_policy_that_holds_every_guard() {
    let l = embedded();
    let b = l.boundary();
    assert!(b.fits(), "theseus-boundary is {} characters", b.size());
    let patterns = b.document.statement[1]
        .action
        .iter()
        .filter(|a| a.contains('*'))
        .count();
    assert_eq!(
        patterns,
        l.compact.len(),
        "every pattern compacts something, so every one is used"
    );
}

#[test]
fn the_policies_on_disk_are_what_the_list_generates() {
    let write = std::env::var_os("THESEUS_GUARD_WRITE").is_some();
    let dir = policies_dir();
    let policies = embedded().policies();
    let names: BTreeSet<String> = policies
        .iter()
        .map(|p| format!("{}.json", p.name))
        .collect();
    let mut stale = Vec::new();
    for p in &policies {
        let path = dir.join(format!("{}.json", p.name));
        if write {
            std::fs::write(&path, p.pretty()).expect("policies/ is writable");
        } else if std::fs::read_to_string(&path).ok().as_deref() != Some(p.pretty().as_str()) {
            stale.push(p.name.clone());
        }
    }
    for entry in std::fs::read_dir(&dir).expect("policies/ exists") {
        let name = entry
            .expect("a directory entry")
            .file_name()
            .to_string_lossy()
            .into_owned();
        if name.ends_with(".json") && !names.contains(&name) {
            if write {
                std::fs::remove_file(dir.join(&name)).expect("a stale policy is removable");
            } else {
                stale.push(name);
            }
        }
    }
    assert!(
        stale.is_empty(),
        "policies/ is out of step with guardrails.toml ({}): rewrite it with THESEUS_GUARD_WRITE=1 cargo test -p theseus-aws-guard --test policies",
        stale.join(", ")
    );
}

/// The condition every network deny carries: no `theseus:owner` tag, and not a tag put on at a create.
fn untagged() -> BTreeMap<String, BTreeMap<String, Value>> {
    let t = |k: &str| (k.to_string(), Value::String("true".into()));
    BTreeMap::from([(
        "Null".to_string(),
        BTreeMap::from([t("aws:ResourceTag/theseus:owner"), t("ec2:CreateAction")]),
    )])
}

/// Does `s` deny `action` on `arn`, whatever its condition?
fn on(s: &Statement, action: &str, arn: &str) -> bool {
    s.effect == "Deny"
        && s.action.iter().any(|a| glob(a, action))
        && s.resource.iter().any(|r| glob(r, arn))
}

/// Changes to network plumbing, each with a resource it names.
const NETWORK_CHANGES: [(&str, &str); 5] = [
    (
        "ec2:DeleteRoute",
        "arn:aws:ec2:us-west-2:111122223333:route-table/rtb-0a1b2c3d4e5f6074a",
    ),
    (
        "ec2:ReplaceRoute",
        "arn:aws:ec2:us-west-2:111122223333:route-table/rtb-0a1b2c3d4e5f6074a",
    ),
    (
        "ec2:DeleteNatGateway",
        "arn:aws:ec2:us-west-2:111122223333:natgateway/nat-0a1b2c3d4e5f6075a",
    ),
    (
        "ec2:ModifySubnetAttribute",
        "arn:aws:ec2:us-west-2:111122223333:subnet/subnet-0a1b2c3d4e5f60711",
    ),
    (
        "ec2:CreateTags",
        "arn:aws:ec2:us-west-2:111122223333:route-table/rtb-0a1b2c3d4e5f6074a",
    ),
];

/// Use, never change (theseus-mgw.9): the session guard, the boundary, and the deployer's guard each
/// refuse a change to network plumbing on a resource not tagged `theseus:owner`. Running a task in
/// another VPC's subnets stays allowed: ECS's own role makes its network interface.
#[test]
fn network_plumbing_not_ours_is_refused_and_running_in_it_is_not() {
    let l = embedded();
    let mut wrong = Vec::new();
    for (form, policies) in [
        ("theseus-guard-limits", l.guard_limits()),
        ("theseus-boundary", vec![l.boundary()]),
        ("theseus-guard-deployer", l.guard_deployer()),
    ] {
        let statements: Vec<&Statement> = policies
            .iter()
            .flat_map(|p| &p.document.statement)
            .collect();
        for (action, arn) in NETWORK_CHANGES {
            let refused = statements.iter().any(|s| {
                on(s, action, arn) && (s.condition.is_none() || s.condition == Some(untagged()))
            });
            if !refused {
                wrong.push(format!(
                    "{form}: {action} on an untagged {arn} is not refused"
                ));
            }
        }
        for (action, arn) in [
            (
                "ecs:RunTask",
                "arn:aws:ecs:us-west-2:111122223333:task-definition/theseus-hand:1",
            ),
            (
                "ec2:CreateNetworkInterface",
                "arn:aws:ec2:us-west-2:111122223333:subnet/subnet-0a1b2c3d4e5f60711",
            ),
        ] {
            if let Some(s) = statements
                .iter()
                .find(|s| on(s, action, arn) || on(s, action, "*"))
            {
                wrong.push(format!("{form}: {} denies {action}", s.sid));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The deployer's guard is all that binds the stacks: plumbing Theseus tagged stays theirs to change,
/// and the hands' own group may be made in another VPC. IAM authorizes `ec2:CreateSecurityGroup`
/// against the VPC too, so the group comes first, and the AWS side leaves that create out.
#[test]
fn the_deployer_changes_only_what_it_tagged_and_may_make_the_hands_group() {
    let deployer = embedded().guard_deployer();
    let statements: Vec<&Statement> = deployer
        .iter()
        .flat_map(|p| &p.document.statement)
        .collect();
    let mut wrong = Vec::new();
    for (action, arn) in NETWORK_CHANGES {
        if let Some(s) = statements
            .iter()
            .find(|s| on(s, action, arn) && s.condition != Some(untagged()))
        {
            wrong.push(format!("{}: denies {action} whatever the tag", s.sid));
        }
    }
    for arn in [
        "arn:aws:ec2:us-west-2:111122223333:vpc/vpc-0a1b2c3d4e5f60718",
        "arn:aws:ec2:us-west-2:111122223333:security-group/sg-0a1b2c3d4e5f60733",
    ] {
        if let Some(s) = statements
            .iter()
            .find(|s| on(s, "ec2:CreateSecurityGroup", arn))
        {
            wrong.push(format!(
                "{}: denies ec2:CreateSecurityGroup on {arn}",
                s.sid
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
