//! Which rule held each secret (theseus-3i6d1), through `aws.call`.
//!
//! `meta["secrets"]` said *that* a value was held and never *why*. The
//! owner's open question about over-masking turns on exactly that
//! distinction: `grafana DescribeWorkspace`'s `grafanaToken` is held by a
//! `HELD` row, a credential by decision, while its workspace `name` is
//! held only because AWS's own model marks the member `sensitive` and a
//! `WHEN_PRESENT` row runs the walk over the whole answer. From an answer
//! the two were indistinguishable, so no reading of the masking could be
//! made from the record.
//!
//! One operation here carries both, and the test asserts the pair, so a
//! walk that reported one constant rule — or none — cannot pass.
//!
//! `meta["secrets"]` itself is a surface: eight sites read its elements as
//! plain strings. It is left byte-identical and the rules ride a second
//! key.

use serde_json::{json, Value};

use super::tests::{board, layer, plain_ctx, sts, tool, Fake, Reply, Seen, ACCOUNT};

const WORKSPACE_TOKEN: &str = "example-workspace-token-0031";
const WORKSPACE_NAME: &str = "example-workspace-name-0032";

fn answers(s: &Seen, n: usize) -> Reply {
    if s.action() == Some("GetCallerIdentity") {
        return sts(ACCOUNT, n);
    }
    let path = s.target.split('?').next().unwrap_or("");
    if path.starts_with("/workspaces") {
        let body = json!({"workspace": {
            "id": "g-example",
            "name": WORKSPACE_NAME,
            "status": "ACTIVE",
            "grafanaToken": WORKSPACE_TOKEN,
        }});
        return (
            200,
            vec![
                ("x-amzn-requestid", format!("req-{n}")),
                ("content-type", "application/json".into()),
            ],
            body.to_string(),
        );
    }
    (400, vec![], "{}".into())
}

/// One answer, two secrets, two different rules, each named beside its
/// handle; `secrets` is unchanged.
#[tokio::test]
async fn an_answer_says_which_rule_held_each_of_its_secrets() {
    let fake = Fake::start(answers);
    let b = board();
    let aws = layer(&fake, b.clone());
    let t = tool(&aws, "aws.call");
    let input = json!({
        "service": "grafana",
        "operation": "DescribeWorkspace",
        "input": {"workspaceId": "g-example"},
    });
    let binding = aws.bind("exe_test", "act_test_1", "toolu_test_1");
    let ctx = theseus_tools::ToolCtx {
        aws: Some(binding),
        ..plain_ctx()
    };
    t.plan(&input, &ctx).expect("the plan");
    let (out, _) = t.run_async(&input, &ctx).await.expect("the call");
    let said = format!("{} {}", out.text, out.meta);
    assert!(!said.contains(WORKSPACE_TOKEN), "the token in {said}");
    assert!(!said.contains(WORKSPACE_NAME), "the name in {said}");

    let rules = out.meta["secret_rules"]
        .as_object()
        .unwrap_or_else(|| panic!("secret_rules is an object: {}", out.meta));
    let mut pairs: Vec<(String, String)> = rules
        .iter()
        .map(|(h, r)| {
            (
                h.rsplit('#').next().unwrap_or(h).to_string(),
                r.as_str().unwrap_or("?").to_string(),
            )
        })
        .collect();
    pairs.sort();
    // The pair, as an equality, with the count asserted: an empty map, or a
    // walk that reported one constant rule, fails here.
    assert_eq!(pairs.len(), 2, "{rules:?}");
    assert_eq!(
        pairs,
        [
            ("workspace.grafanaToken".to_string(), "held".to_string()),
            ("workspace.name".to_string(), "marked".to_string()),
        ],
        "{rules:?}"
    );

    // `secrets` keeps its shape: an array of plain handle strings, one per
    // rule row, and every rule row names a handle that is in it.
    let handles: Vec<&str> = out.meta["secrets"]
        .as_array()
        .expect("secrets is an array")
        .iter()
        .map(|h| h.as_str().expect("a plain string handle"))
        .collect();
    assert_eq!(handles.len(), 2, "{}", out.meta);
    for h in rules.keys() {
        assert!(handles.contains(&h.as_str()), "{h} is in {handles:?}");
    }
}

/// A call that holds nothing says nothing: no `secret_rules` key at all,
/// exactly as `secrets` is absent.
#[tokio::test]
async fn a_call_that_holds_nothing_carries_no_rules_key() {
    fn empty(s: &Seen, n: usize) -> Reply {
        if s.action() == Some("GetCallerIdentity") {
            return sts(ACCOUNT, n);
        }
        (
            200,
            vec![
                ("x-amzn-requestid", format!("req-{n}")),
                ("content-type", "application/json".into()),
            ],
            json!({"workspaces": []}).to_string(),
        )
    }
    let fake = Fake::start(empty);
    let b = board();
    let aws = layer(&fake, b.clone());
    let t = tool(&aws, "aws.call");
    let input: Value = json!({
        "service": "grafana",
        "operation": "ListWorkspaces",
        "input": {},
    });
    let binding = aws.bind("exe_test", "act_test_1", "toolu_test_1");
    let ctx = theseus_tools::ToolCtx {
        aws: Some(binding),
        ..plain_ctx()
    };
    t.plan(&input, &ctx).expect("the plan");
    let (out, _) = t.run_async(&input, &ctx).await.expect("the call");
    assert!(out.meta.get("secrets").is_none(), "{}", out.meta);
    assert!(out.meta.get("secret_rules").is_none(), "{}", out.meta);
}
