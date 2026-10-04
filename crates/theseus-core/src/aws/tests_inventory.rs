//! The inventory and the reaper's report (AWS design §3.7; C3 = 14c),
//! against a fake account that also holds another project's resources:
//! untagged, or tagged as that project's. They are never listed, never
//! named, and never touched; the reaper only reports.

use serde_json::{json, Value};

use super::tests::{board, layer, plain_ctx, sts, Fake, Reply, Seen, ACCOUNT};
use super::tests_c3::{call_in, json_reply, target};

/// Another project's names: none may appear in what the tool says.
const FOREIGN: &[&str] = &[
    "nimbus-analytics-data",
    "nimbus-etl",
    "i-0nimbus0000000001",
    "nimbus-prod-stack",
];

fn tag(k: &str, v: &str) -> Value {
    json!({"Key": k, "Value": v})
}

fn answers(s: &Seen, n: usize) -> Reply {
    if s.action() == Some("GetCallerIdentity") {
        return sts(ACCOUNT, n);
    }
    if s.action() == Some("DescribeStacks") {
        let stack = |name: &str, made: &str| {
            format!(
                "<member><StackName>{name}</StackName>\
                 <StackId>arn:aws:cloudformation:us-west-2:{ACCOUNT}:stack/{name}/1</StackId>\
                 <CreationTime>{made}</CreationTime><StackStatus>CREATE_COMPLETE</StackStatus></member>"
            )
        };
        return (
            200,
            vec![
                ("x-amzn-requestid", format!("req-{n}")),
                ("content-type", "text/xml".into()),
            ],
            format!(
                "<DescribeStacksResponse><DescribeStacksResult><Stacks>{}{}</Stacks>\
                 </DescribeStacksResult><ResponseMetadata><RequestId>req-{n}</RequestId>\
                 </ResponseMetadata></DescribeStacksResponse>",
                stack("theseus-exp-42", "2026-09-30T12:00:00Z"),
                stack("nimbus-prod-stack", "2025-01-01T00:00:00Z"),
            ),
        );
    }
    match target(s) {
        // The fake answers as if the tag filter were not there: what comes
        // back is checked again on Theseus's side.
        Some("GetResources") if s.region() == Some("us-west-2") => json_reply(
            n,
            json!({"ResourceTagMappingList": [
                {"ResourceARN": format!("arn:aws:cloudformation:us-west-2:{ACCOUNT}:stack/theseus-exp-42/1"),
                 "Tags": [tag("theseus:owner", "theseus"), tag("theseus:ttl", "2020-01-01T00:00:00Z"), tag("theseus:stack", "theseus-exp-42")]},
                {"ResourceARN": "arn:aws:s3:::theseus-exp-42-data",
                 "Tags": [tag("theseus:owner", "theseus"), tag("aws:cloudformation:stack-name", "theseus-exp-42"), tag("theseus:ttl", "2020-01-01T00:00:00Z")]},
                {"ResourceARN": "arn:aws:s3:::theseus-scratch-7",
                 "Tags": [tag("theseus:owner", "theseus"), tag("theseus:ttl", "2020-01-01T00:00:00Z")]},
                {"ResourceARN": format!("arn:aws:ecs:us-west-2:{ACCOUNT}:task/theseus-hands/0a1b"),
                 "Tags": [tag("theseus:owner", "theseus"), tag("theseus:ttl", "2020-01-01T00:00:00Z")]},
                {"ResourceARN": format!("arn:aws:sns:us-west-2:{ACCOUNT}:theseus-alerts"),
                 "Tags": [tag("theseus:owner", "theseus")]},
                {"ResourceARN": "arn:aws:s3:::nimbus-analytics-data",
                 "Tags": [tag("project", "nimbus"), tag("theseus:ttl", "2020-01-01T00:00:00Z")]},
                {"ResourceARN": format!("arn:aws:lambda:us-west-2:{ACCOUNT}:function:nimbus-etl"),
                 "Tags": [tag("theseus:owner", "nimbus")]},
                {"ResourceARN": format!("arn:aws:ec2:us-west-2:{ACCOUNT}:instance/i-0nimbus0000000001"),
                 "Tags": []},
                {"ResourceARN": format!("arn:aws:cloudformation:us-west-2:{ACCOUNT}:stack/nimbus-prod-stack/1"),
                 "Tags": [tag("theseus:ttl", "2020-01-01T00:00:00Z")]}
            ]}),
        ),
        Some("GetResources") => json_reply(n, json!({"ResourceTagMappingList": []})),
        Some("GetCostAndUsage") => json_reply(
            n,
            json!({"ResultsByTime": [{"Groups": [
                {"Keys": ["theseus:stack$theseus-exp-42"], "Metrics": {"UnblendedCost": {"Amount": "0.4012", "Unit": "USD"}}},
                {"Keys": ["theseus:stack$"], "Metrics": {"UnblendedCost": {"Amount": "120.00", "Unit": "USD"}}}
            ]}]}),
        ),
        _ => (400, vec![], "{}".into()),
    }
}

/// The inventory lists what Theseus made, in each region, asks AWS for its
/// tag alone, and never lists, names, or reaps another project's resource,
/// even when AWS returns one; the reaper reports a stack whole, a loose run
/// stopped, and a loose bucket asked about, and deletes nothing.
#[tokio::test]
async fn the_inventory_never_lists_an_untagged_resource_and_the_reaper_only_reports() {
    let fake = Fake::start(answers);
    let aws = layer(&fake, board());
    let (r, rows, plan) = call_in(&aws, "aws.inventory", json!({"costs": true}), plain_ctx()).await;
    let (out, marked) = r.expect("the inventory");
    let t = out.text;
    for name in FOREIGN {
        assert!(!t.contains(name), "{name} is another project's: {t}");
        assert!(!out.meta.to_string().contains(name));
    }
    assert!(t.contains("us-west-2, eu-west-1"), "{t}");
    assert!(t.contains(": 5 resources\n"), "{t}");
    assert!(
        t.contains("(4 more that AWS returned do not carry theseus:owner = theseus: left out.)"),
        "{t}"
    );
    assert!(
        t.contains("cloudformation:stack theseus-exp-42 · us-west-2 · made "),
        "{t}"
    );
    assert!(t.contains("$0.40 this month"), "{t}");
    assert!(
        t.contains("would delete stack theseus-exp-42 (us-west-2): its theseus:ttl passed"),
        "{t}"
    );
    assert!(t.contains("would stop ecs:task theseus-hands"), "{t}");
    assert!(
        t.contains("would ask the operator before deleting s3:bucket theseus-scratch-7"),
        "{t}"
    );
    assert!(
        !t.contains("deleting s3:bucket theseus-exp-42-data"),
        "a stack's resource is reaped with its stack: {t}"
    );
    assert_eq!(out.meta["would_reap"], 3);
    assert!(marked.is_none(), "an inventory is not outside text");
    assert_eq!(plan.class, Some(theseus_tools::ToolClass::Read));
    // Every request is a read, and each region's asks for Theseus's tag.
    let ops: Vec<&str> = rows
        .iter()
        .map(|r| r.row["operation"].as_str().unwrap())
        .collect();
    assert_eq!(
        ops,
        [
            "GetResources",
            "DescribeStacks",
            "GetResources",
            "GetCostAndUsage"
        ]
    );
    assert!(rows.iter().all(|r| r.row["class"] == "read"));
    for s in fake
        .seen()
        .iter()
        .filter(|s| target(s) == Some("GetResources"))
    {
        let body: Value = serde_json::from_str(&s.body).unwrap();
        assert_eq!(
            body["TagFilters"],
            json!([{"Key": "theseus:owner", "Values": ["theseus"]}])
        );
    }
    // It deleted, stopped, and tagged nothing: only these were sent.
    for s in fake.seen() {
        let what = target(&s).or(s.action()).unwrap_or_default().to_string();
        assert!(
            [
                "GetCallerIdentity",
                "GetResources",
                "DescribeStacks",
                "GetCostAndUsage"
            ]
            .contains(&what.as_str()),
            "the inventory sent {what}"
        );
    }
}
