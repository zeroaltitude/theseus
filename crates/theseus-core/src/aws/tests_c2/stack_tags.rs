//! Stable stack tags and the tag-only change set (theseus-mpk2s): a plan from
//! a later session lists only the real change; the first change set after the
//! tags became stable drops the old execution tag, and only that passes the
//! floor without its guard and budget lines; the budget's reconcile applies
//! with that migration and with nothing else.

use serde_json::{json, Value};

use super::super::stack_tags::{
    migration, stable_tags, tag_only, tag_only_gaps, tags_of, VOLATILE,
};
use super::*;

fn tags(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// A stack as `DescribeStacks` gives it, holding `pairs` as tags.
fn stack_with(pairs: &[(&str, &str)]) -> Value {
    json!({"Tags": pairs.iter().map(|(k, v)| json!({"Key": k, "Value": v})).collect::<Vec<_>>()})
}

const OLD: [(&str, &str); 4] = [
    ("theseus:owner", "theseus"),
    ("theseus:stack", "theseus-foundation"),
    ("theseus:deployment", "default"),
    ("theseus:execution", "exe_first"),
];
const STABLE: [(&str, &str); 3] = [
    ("theseus:owner", "theseus"),
    ("theseus:stack", "theseus-foundation"),
    ("theseus:deployment", "default"),
];

/// A change as CloudFormation describes a tag change on a resource.
fn tag_change(id: &str, ty: &str) -> Value {
    json!({"Type": "Resource", "ResourceChange": {
        "Action": "Modify", "LogicalResourceId": id, "ResourceType": ty, "Replacement": "False",
        "Scope": ["Tags"],
        "Details": [{"Target": {"Attribute": "Tags", "RequiresRecreation": "Never"},
                     "Evaluation": "Static", "ChangeSource": "DirectModification"}]}})
}

/// A resource whose property mentions an attribute of another: listed as a
/// possible change, which CloudFormation settles at execution.
fn knock_on(id: &str, ty: &str, cause: &str, replacement: &str) -> Value {
    json!({"Type": "Resource", "ResourceChange": {
        "Action": "Modify", "LogicalResourceId": id, "ResourceType": ty, "Replacement": replacement,
        "Scope": ["Properties"],
        "Details": [{"Target": {"Attribute": "Properties", "Name": "SourceArn", "RequiresRecreation": "Always"},
                     "Evaluation": "Dynamic", "ChangeSource": "ResourceAttribute",
                     "CausingEntity": cause}]}})
}

/// A guard resource whose tags change and whose policy is edited too.
fn edited_with_tags(id: &str, ty: &str) -> Value {
    json!({"Type": "Resource", "ResourceChange": {
        "Action": "Modify", "LogicalResourceId": id, "ResourceType": ty, "Replacement": "False",
        "Scope": ["Properties", "Tags"],
        "Details": [
            {"Target": {"Attribute": "Properties", "Name": "PolicyDocument", "RequiresRecreation": "Never"},
             "Evaluation": "Static", "ChangeSource": "DirectModification"},
            {"Target": {"Attribute": "Tags", "RequiresRecreation": "Never"},
             "Evaluation": "Static", "ChangeSource": "DirectModification"}]}})
}

#[test]
fn a_new_stack_gets_its_three_tags_and_an_old_one_keeps_its_own_less_the_execution() {
    let new = stable_tags(None, "theseus-hands", "default");
    assert_eq!(
        new,
        tags(&[
            ("theseus:owner", "theseus"),
            ("theseus:stack", "theseus-hands"),
            ("theseus:deployment", "default"),
        ])
    );
    // Whoever plans next sets what the stack holds, and no execution.
    let old = stack_with(&OLD);
    assert_eq!(tags_of(Some(&old)), tags(&OLD));
    let kept = stable_tags(Some(&old), "theseus-foundation", "default");
    assert_eq!(kept, tags(&STABLE));
    let again = stack_with(&STABLE);
    assert_eq!(
        stable_tags(Some(&again), "theseus-foundation", "default"),
        kept
    );
    // A stack made by a plan, which never set a deployment, keeps that too.
    let bare = stack_with(&[("theseus:owner", "theseus"), ("theseus:execution", "exe_a")]);
    assert_eq!(
        stable_tags(Some(&bare), "theseus-hands", "default"),
        tags(&[("theseus:owner", "theseus")])
    );
}

#[test]
fn the_migration_is_the_drop_of_the_execution_tag_and_nothing_else() {
    let old = stack_with(&OLD);
    assert!(migration(Some(&old), &tags(&STABLE)));
    // Another tag added, changed, or dropped: not that migration.
    let mut more = tags(&STABLE);
    more.push(("theseus:ttl".into(), "2026-12-01".into()));
    assert!(!migration(Some(&old), &more));
    let mut changed = tags(&STABLE);
    changed[2].1 = "elsewhere".into();
    assert!(!migration(Some(&old), &changed));
    assert!(!migration(Some(&old), &tags(&STABLE)[..2]));
    // The stack already holds none: there is nothing to migrate.
    assert!(!migration(Some(&stack_with(&STABLE)), &tags(&STABLE)));
    assert!(!migration(None, &tags(&STABLE)));
}

#[test]
fn a_pure_tag_change_is_tag_only_with_its_knock_on_and_nothing_else_is() {
    let bucket = tag_change("TheseusBucket", "AWS::S3::Bucket");
    let role = tag_change("OwnerRole", "AWS::IAM::Role");
    let knock = knock_on(
        "ReaperInvokePermission",
        "AWS::Lambda::Permission",
        "Reaper.Arn",
        "Conditional",
    );
    let reaper = tag_change("Reaper", "AWS::Lambda::Function");
    assert!(tag_only(&[bucket.clone(), role.clone()]));
    assert!(tag_only(&[reaper.clone(), knock.clone()]));
    assert!(tag_only_gaps(&[reaper.clone(), knock.clone()]).is_empty());

    // Nothing to say is not a migration.
    assert!(!tag_only(&[]));

    // A tag change beside a property edit of a guard resource, and of the
    // budget's stop action: the guard's own lines stay.
    for (id, ty) in [
        ("HandBasicsPolicy", "AWS::IAM::ManagedPolicy"),
        ("HandsBudgetStopAction", "AWS::Budgets::BudgetsAction"),
    ] {
        let set = [bucket.clone(), edited_with_tags(id, ty)];
        assert!(!tag_only(&set), "{id}");
        assert_eq!(tag_only_gaps(&set), vec![format!("Modify {id}")]);
    }

    // A knock-on is no pass when its cause is not a tag change in the set,
    // when it is a direct edit, when its cause is a custom resource, or when
    // it replaces for certain.
    let stranger = knock_on(
        "ReaperInvokePermission",
        "AWS::Lambda::Permission",
        "Other.Arn",
        "Conditional",
    );
    assert!(!tag_only(&[reaper.clone(), stranger]));
    let certain = knock_on(
        "ReaperInvokePermission",
        "AWS::Lambda::Permission",
        "Reaper.Arn",
        "True",
    );
    assert!(!tag_only(&[reaper.clone(), certain]));
    let mut direct = knock.clone();
    direct["ResourceChange"]["Details"][0]["Evaluation"] = json!("Static");
    assert!(!tag_only(&[reaper, direct]));
    let custom = tag_change("Reaper", "Custom::Thing");
    assert!(!tag_only(&[custom, knock]));

    // An add, a removal, a replacement, or a tag change that recreates.
    let add = json!({"ResourceChange": {"Action": "Add", "LogicalResourceId": "New",
                                        "ResourceType": "AWS::SQS::Queue"}});
    let remove = json!({"ResourceChange": {"Action": "Remove", "LogicalResourceId": "Old",
                                           "ResourceType": "AWS::SQS::Queue"}});
    let mut replaced = bucket.clone();
    replaced["ResourceChange"]["Replacement"] = json!("True");
    let mut recreates = bucket;
    recreates["ResourceChange"]["Details"][0]["Target"]["RequiresRecreation"] = json!("Always");
    for set in [add, remove, replaced, recreates] {
        assert!(!tag_only(&[role.clone(), set]));
    }
}

// ------------------------------------------------------------------ the reconcile

/// The foundation as an older apply left it: the execution in its tags.
fn old_foundation(budget: &str) -> Stack {
    Stack {
        tags: tags(&OLD),
        ..foundation(budget)
    }
}

fn change_set_forms(fake: &Fake) -> Vec<BTreeMap<String, String>> {
    fake.seen()
        .iter()
        .map(|s| form(&s.body))
        .filter(|f| f.get("Action").map(String::as_str) == Some("CreateChangeSet"))
        .collect()
}

fn tags_sent(f: &BTreeMap<String, String>) -> Vec<(String, String)> {
    (1..)
        .map_while(|n| {
            Some((
                f.get(&format!("Tags.member.{n}.Key"))?.clone(),
                f.get(&format!("Tags.member.{n}.Value"))?.clone(),
            ))
        })
        .collect()
}

/// The owner's raise of the month from 50 to 100 on a stack that still holds
/// the old execution tag: the change set is the budget and the tag migration
/// (every resource's tags, and a dynamic knock-on), applied with the tags
/// every session sets; run again, it is equal and asks for nothing.
#[tokio::test]
async fn the_reconcile_applies_the_month_with_the_tag_migration_alone() {
    let (fake, c) = cloud(Cloud {
        stacks: BTreeMap::from([("theseus-foundation".to_string(), old_foundation("50"))]),
        owner_role: true,
        knock_on: Some(("TheseusBucketPolicy".into(), "TheseusBucket".into())),
        ..Default::default()
    });
    let aws = layer(&fake, true, Some(100));
    let account = aws.account(None).unwrap().clone();
    assert_eq!(
        tend::reconcile(&account).await,
        tend::Reconciled::Migrated { from: 50, to: 100 }
    );
    let sets = change_set_forms(&fake);
    assert_eq!(sets.len(), 1);
    assert_eq!(tags_sent(&sets[0]), tags(&STABLE));
    {
        let held = &c.lock().unwrap().stacks["theseus-foundation"];
        assert_eq!(held.params["MonthlyBudgetUsd"], "100");
        assert_eq!(held.tags, tags(&STABLE));
    }
    assert_eq!(
        tend::reconcile(&account).await.line(),
        "the stack holds the config's $100 a month"
    );
    assert_eq!(
        actions(&fake)
            .iter()
            .filter(|a| *a == "ExecuteChangeSet")
            .count(),
        1
    );
}

/// Anything else beside the migration stops the reconcile as before, and the
/// change set is deleted, never applied: a guard resource's policy edited as
/// its tags change, another resource added, a knock-on of a resource no tag
/// change causes.
#[tokio::test]
async fn the_reconcile_refuses_the_migration_with_any_other_change() {
    for (what, state, said) in [
        (
            "a guard policy edited with its tags",
            Cloud {
                edited_with_tags: Some("OwnerRole".into()),
                ..Default::default()
            },
            "Modify OwnerRole",
        ),
        (
            "a change beyond the budget",
            Cloud {
                beyond_budget: true,
                ..Default::default()
            },
            "Modify OwnerRole",
        ),
        (
            "a knock-on that no tag change causes",
            Cloud {
                knock_on: Some(("TheseusBucketPolicy".into(), "NotATagChange".into())),
                ..Default::default()
            },
            "Modify TheseusBucketPolicy",
        ),
    ] {
        let (fake, c) = cloud(Cloud {
            stacks: BTreeMap::from([("theseus-foundation".to_string(), old_foundation("50"))]),
            owner_role: true,
            ..state
        });
        let aws = layer(&fake, true, Some(100));
        match tend::reconcile(aws.account(None).unwrap()).await {
            tend::Reconciled::Stopped(why) => assert!(
                why.contains("touches more than the budget") && why.contains(said),
                "{what}: {why}"
            ),
            other => panic!("{what}: {other:?}"),
        }
        let a = actions(&fake);
        assert!(
            a.contains(&"DeleteChangeSet".to_string())
                && !a.contains(&"ExecuteChangeSet".to_string()),
            "{what}: {a:?}"
        );
        let held = &c.lock().unwrap().stacks["theseus-foundation"];
        assert_eq!(
            (held.params["MonthlyBudgetUsd"].as_str(), &held.tags),
            ("50", &tags(&OLD))
        );
    }

    // A stack that holds no execution tag has no migration to ride: a change
    // set that changes the tags anyway is beyond the budget.
    let (fake, _) = cloud(Cloud {
        stacks: BTreeMap::from([(
            "theseus-foundation".to_string(),
            Stack {
                tags: tags(&[("theseus:owner", "someone-else")]),
                ..foundation("50")
            },
        )]),
        owner_role: true,
        ..Default::default()
    });
    let aws = layer(&fake, true, Some(100));
    // The tags it sends are the stack's own, so nothing but the budget moves.
    assert_eq!(
        tend::reconcile(aws.account(None).unwrap()).await,
        tend::Reconciled::Changed { from: 50, to: 100 }
    );
    assert_eq!(
        tags_sent(&change_set_forms(&fake)[0]),
        tags(&[("theseus:owner", "someone-else")])
    );
}

// ------------------------------------------------------------------ the plan

fn text_of(r: theseus_tools::AsyncResult) -> (String, Value) {
    let (out, _) = r.expect("the plan");
    (out.text, out.meta)
}

/// A call of an AWS tool from a session of its own: its execution id.
async fn call_as(
    aws: &Arc<Aws>,
    execution: &str,
    name: &str,
    input: Value,
) -> (theseus_tools::AsyncResult, Vec<theseus_tools::AwsRequest>) {
    let t = tool(aws, name);
    let b = aws.bind(execution, &format!("act_{execution}"), "toolu_test_1");
    let ctx = theseus_tools::ToolCtx {
        aws: Some(b.clone()),
        ..plain_ctx()
    };
    t.plan(&input, &ctx).expect("the call plans");
    let r = t.run_async(&input, &ctx).await;
    (r, b.requests())
}

/// Plans of the foundation from the sessions of an old stack, a migrated
/// one, and a second session with a real change.
#[tokio::test]
async fn a_plan_from_a_later_session_lists_only_its_real_change() {
    let dir = tempfile::tempdir().unwrap();
    let template = dir.path().join("theseus-foundation.yaml");
    std::fs::write(&template, bootstrap::FOUNDATION_TEMPLATE).unwrap();
    let path = template.display().to_string();
    let params = |budget: &str| {
        json!({"stack": "theseus-foundation", "template": path,
               "parameters": {"OwnerUserName": "example", "MonthlyBudgetUsd": budget, "AlertEmail": ""}})
    };
    let (fake, c) = cloud(Cloud {
        stacks: BTreeMap::from([("theseus-foundation".to_string(), old_foundation("50"))]),
        owner_role: true,
        knock_on: Some(("TheseusBucketPolicy".into(), "TheseusBucket".into())),
        ..Default::default()
    });
    let aws = layer(&fake, true, None);

    // First: the migration. Every resource's tags change, nothing at the
    // floor for it; the description names the execution that made the set.
    let (r, _) = call_as(&aws, "exe_one", "aws.stack.plan", params("50")).await;
    let (text, meta) = text_of(r);
    assert!(
        text.contains("A tag-only migration") && !text.contains("At the floor"),
        "{text}"
    );
    assert_eq!(meta["tag_migration"], true);
    assert_eq!(meta["floor"], 0);
    let sets = change_set_forms(&fake);
    assert_eq!(tags_sent(&sets[0]), tags(&STABLE));
    assert!(
        sets[0]["Description"].contains("exe_one"),
        "{:?}",
        sets[0]["Description"]
    );
    assert!(!sets[0].values().any(|v| v == "theseus:execution"));

    // Apply it: its execute row says what it was, and the stack holds the
    // stable tags.
    let digest = meta["digest"].as_str().unwrap().to_string();
    let (r, requests) = call_as(
        &aws,
        "exe_one",
        "aws.stack.apply",
        json!({"stack": "theseus-foundation", "digest": digest}),
    )
    .await;
    r.expect("the apply");
    assert_eq!(
        c.lock().unwrap().stacks["theseus-foundation"].tags,
        tags(&STABLE)
    );
    let executed: Vec<&Value> = requests
        .iter()
        .map(|r| &r.row)
        .filter(|r| r["operation"] == "ExecuteChangeSet")
        .collect();
    assert_eq!(executed.len(), 1);
    assert_eq!(executed[0]["tag_migration"], VOLATILE);

    // A second session, the same template: nothing.
    let (r, _) = call_as(&aws, "exe_two", "aws.stack.plan", params("50")).await;
    let (text, _) = text_of(r);
    assert!(text.contains("change nothing"), "{text}");

    // A second session with a real change lists it alone: the budget, no
    // resource's tags.
    let (r, _) = call_as(&aws, "exe_two", "aws.stack.plan", params("100")).await;
    let (text, meta) = text_of(r);
    let lines: Vec<&str> = text.lines().filter(|l| l.starts_with("  ")).collect();
    assert_eq!(meta["changes"], 1, "{text}");
    assert!(
        lines.iter().any(|l| l.contains("MonthlyBudget")) && !text.contains("Tags"),
        "{text}"
    );
    assert!(!text.contains("tag-only"), "{text}");
    let sets = change_set_forms(&fake);
    let last = sets.last().unwrap();
    assert_eq!(tags_sent(last), tags(&STABLE));
    assert!(last["Description"].contains("exe_two"));
}

/// The floor for everything but that migration is what it was: the same drop
/// of the old tag with a guard resource's policy edited besides is planned
/// with the guard's lines.
#[tokio::test]
async fn a_plan_that_edits_a_guard_beside_the_migration_keeps_the_floor() {
    let dir = tempfile::tempdir().unwrap();
    let template = dir.path().join("theseus-foundation.yaml");
    std::fs::write(&template, bootstrap::FOUNDATION_TEMPLATE).unwrap();
    let path = template.display().to_string();
    let (fake, _c) = cloud(Cloud {
        stacks: BTreeMap::from([("theseus-foundation".to_string(), old_foundation("50"))]),
        owner_role: true,
        edited_with_tags: Some("OwnerRole".into()),
        ..Default::default()
    });
    let aws = layer(&fake, true, None);
    let input = json!({"stack": "theseus-foundation", "template": path,
                       "parameters": {"OwnerUserName": "example", "MonthlyBudgetUsd": "50", "AlertEmail": ""}});
    let (r, _) = call_as(&aws, "exe_one", "aws.stack.plan", input.clone()).await;
    let (text, meta) = text_of(r);
    assert!(
        text.contains("At the floor") && !text.contains("A tag-only migration"),
        "{text}"
    );
    assert_eq!(meta["tag_migration"], false);
    assert!(meta["floor"].as_u64().unwrap() > 0);

    // And the stack that holds other tags than it should: no migration either.
    let (fake, _c) = cloud(Cloud {
        stacks: BTreeMap::from([(
            "theseus-foundation".to_string(),
            Stack {
                tags: tags(&[
                    ("theseus:owner", "theseus"),
                    ("theseus:execution", "exe_first"),
                    ("theseus:extra", "x"),
                ]),
                ..foundation("50")
            },
        )]),
        owner_role: true,
        ..Default::default()
    });
    let aws = layer(&fake, true, None);
    let (r, _) = call_as(&aws, "exe_one", "aws.stack.plan", input).await;
    let (_, meta) = text_of(r);
    // It sets the stack's tags as they are, less the execution: that is the
    // migration, and its extra tag is the stack's own.
    assert_eq!(meta["tag_migration"], true);
}
