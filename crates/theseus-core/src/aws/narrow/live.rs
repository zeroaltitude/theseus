//! The account behind the traits: CloudTrail's history, IAM, Access Analyzer
//! and CloudFormation, through the account's own client, signed as the work
//! session is, and recorded on a call's binding like any AWS read. The
//! hands stack's change sets are the owner's act here, through the same
//! `Cfn` the stack tools use.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_tools::AwsBinding;

use super::apply::STACK;
use super::policy::Doc;
use super::run::{RoleDocs, Roles};
use super::usage::Source;
use super::verify::{Answer, Call, Checker, Finding};
use crate::aws::session::Kind;
use crate::aws::stack::{resource_changes, role_arn, Cfn, ChangeSetSpec, Param, Ready, DEPLOYER};
use crate::aws::{stack_tags, Account, Request, Signer};

/// The managed policy every hand role keeps.
pub const BASICS: &str = "theseus-hand-basics";
const SETTLE: Duration = Duration::from_secs(10 * 60);

/// `LookupEvents`' input for a page.
fn lookup_input(start_s: u64, end_s: u64, token: Option<&str>) -> Value {
    let mut input =
        json!({"StartTime": start_s, "EndTime": end_s, "MaxResults": super::usage::PAGE});
    if let Some(t) = token {
        input["NextToken"] = json!(t);
    }
    input
}

/// Access Analyzer's `ValidatePolicy` input for an identity policy.
fn validate_input(doc: &Value) -> Value {
    json!({
        "policyDocument": doc.to_string(),
        "policyType": "IDENTITY_POLICY",
        "maxResults": 100,
    })
}

/// IAM's `SimulateCustomPolicy` input.
fn simulate_input(docs: &[String], actions: &[&str], arns: &[String]) -> Value {
    let mut input = json!({"PolicyInputList": docs, "ActionNames": actions});
    if !arns.is_empty() {
        input["ResourceArns"] = json!(arns);
    }
    input
}

pub struct Live {
    pub account: Arc<Account>,
    pub region: String,
    pub binding: Option<Arc<AwsBinding>>,
}

impl Live {
    async fn call(
        &self,
        service: &str,
        operation: &str,
        input: &Value,
        pages: u32,
    ) -> Result<theseus_aws::Output, String> {
        let r = Request {
            service,
            operation,
            input,
            region: &self.region,
            pages,
            class: "read",
            signer: Signer::As(Kind::Work),
        };
        self.account
            .request(self.binding.as_deref(), &r)
            .await
            .map_err(|f| format!("{service}:{operation}: {f}"))
    }

    fn cfn(&self) -> Cfn<'_> {
        Cfn {
            account: &self.account,
            region: &self.region,
            binding: self.binding.as_deref(),
            signer: Signer::As(Kind::Work),
        }
    }
}

/// The catalog's services with their operation counts, for a policy's size.
pub fn universe() -> Vec<(String, u32)> {
    theseus_aws::catalog::Catalog::embedded()
        .map(|c| {
            c.services()
                .iter()
                .map(|e| (e.signing_name.clone(), e.operations))
                .collect()
        })
        .unwrap_or_default()
}

impl Source for Live {
    async fn page(
        &self,
        start_s: u64,
        end_s: u64,
        token: Option<&str>,
    ) -> Result<(Vec<Value>, Option<String>), String> {
        let input = lookup_input(start_s, end_s, token);
        let out = self.call("cloudtrail", "LookupEvents", &input, 1).await?;
        let events = out.body["Events"].as_array().cloned().unwrap_or_default();
        let next = out
            .next
            .as_ref()
            .and_then(|m| m.get("NextToken"))
            .and_then(Value::as_str)
            .map(String::from);
        Ok((events, next))
    }
}

impl Checker for Live {
    async fn validate(&self, doc: &Value) -> Result<Vec<Finding>, String> {
        let input = validate_input(doc);
        let out = self
            .call("accessanalyzer", "ValidatePolicy", &input, 5)
            .await?;
        Ok(out.body["findings"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| Finding {
                kind: f["findingType"].as_str().unwrap_or("?").into(),
                code: f["issueCode"].as_str().unwrap_or("?").into(),
                detail: f["findingDetails"].as_str().unwrap_or("").into(),
            })
            .collect())
    }

    async fn simulate(&self, policies: &[Value], calls: &[Call]) -> Result<Vec<Answer>, String> {
        let docs: Vec<String> = policies.iter().map(Value::to_string).collect();
        let mut out = Vec::new();
        let mut bare: Vec<&str> = Vec::new();
        let run = |actions: Vec<&str>, arns: &[String]| simulate_input(&docs, &actions, arns);
        let mut inputs = Vec::new();
        for c in calls {
            if c.resources.is_empty() {
                bare.push(&c.action);
            } else {
                for arns in c.resources.chunks(25) {
                    inputs.push(run(vec![c.action.as_str()], arns));
                }
            }
        }
        for chunk in bare.chunks(50) {
            inputs.push(run(chunk.to_vec(), &[]));
        }
        for input in &inputs {
            let r = self.call("iam", "SimulateCustomPolicy", input, 3).await?;
            for e in r.body["EvaluationResults"].as_array().into_iter().flatten() {
                out.push(Answer {
                    action: e["EvalActionName"].as_str().unwrap_or("").into(),
                    resource: e["EvalResourceName"].as_str().map(String::from),
                    decision: e["EvalDecision"].as_str().unwrap_or("").into(),
                    missing: e["MissingContextValues"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|m| m.as_str().map(String::from))
                        .collect(),
                });
            }
        }
        Ok(out)
    }
}

fn document(v: &Value) -> Result<Doc, String> {
    match v {
        Value::String(s) => Doc::parse(s),
        o @ Value::Object(_) => Doc::from_value(o),
        _ => Err("a policy has no document".into()),
    }
}

impl Roles for Live {
    async fn role_docs(&self, role: &str) -> Result<RoleDocs, String> {
        let mut wide = Doc::default();
        let mut rest = Doc::default();
        let mut names = Vec::new();
        let attached = self
            .call(
                "iam",
                "ListAttachedRolePolicies",
                &json!({"RoleName": role}),
                3,
            )
            .await?;
        for p in attached.body["AttachedPolicies"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let (Some(name), Some(arn)) = (p["PolicyName"].as_str(), p["PolicyArn"].as_str())
            else {
                continue;
            };
            let got = self
                .call("iam", "GetPolicy", &json!({"PolicyArn": arn}), 1)
                .await?;
            let version = got.body["Policy"]["DefaultVersionId"]
                .as_str()
                .unwrap_or("v1");
            let v = self
                .call(
                    "iam",
                    "GetPolicyVersion",
                    &json!({"PolicyArn": arn, "VersionId": version}),
                    1,
                )
                .await?;
            let doc = document(&v.body["PolicyVersion"]["Document"])
                .map_err(|e| format!("{name}: {e}"))?;
            if name == BASICS {
                rest.stmts.extend(doc.stmts);
            } else {
                names.push(name.to_string());
                wide.stmts.extend(doc.stmts);
            }
        }
        let inline = self
            .call("iam", "ListRolePolicies", &json!({"RoleName": role}), 3)
            .await?;
        for n in inline.body["PolicyNames"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let got = self
                .call(
                    "iam",
                    "GetRolePolicy",
                    &json!({"RoleName": role, "PolicyName": n}),
                    1,
                )
                .await?;
            let doc = document(&got.body["PolicyDocument"]).map_err(|e| format!("{n}: {e}"))?;
            names.push(format!("{n} (inline)"));
            wide.stmts.extend(doc.stmts);
        }
        Ok(RoleDocs {
            wide,
            wide_names: names,
            rest,
        })
    }

    async fn template(&self) -> Result<String, String> {
        self.cfn()
            .template(STACK)
            .await
            .map_err(|f| format!("the {STACK} template: {f}"))
    }
}

/// A stack change that ran.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    pub change_set: String,
    pub status: String,
    pub changed: Vec<String>,
}

/// Plan `body` as the hands stack's new template, refuse it unless the change
/// set touches only `expect` (logical ids) and the guards' floor is empty,
/// then run it and wait for the stack to settle. The owner's act: nothing
/// here asks again.
pub async fn apply_template(
    live: &Live,
    body: &str,
    expect: &[String],
    why: &str,
) -> Result<Applied, String> {
    let cfn = live.cfn();
    let said = |f: crate::aws::Failure| format!("{STACK}: {f}");
    let stack = cfn
        .stack(STACK)
        .await
        .map_err(said)?
        .ok_or_else(|| format!("the {STACK} stack does not exist"))?;
    if body.len() > crate::aws::stack::BODY_LIMIT {
        return Err("the template is too large to send by body".into());
    }
    let params: Vec<(String, Param)> = stack["Parameters"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| {
            p["ParameterKey"]
                .as_str()
                .map(|k| (k.to_string(), Param::Previous))
        })
        .collect();
    let tags = stack_tags::stable_tags(Some(&stack), STACK, live.account.cfg.deployment());
    let correlation = live
        .binding
        .as_ref()
        .map_or("narrow", |b| b.correlation_id.as_str());
    let name = format!(
        "narrow-{}-{}",
        correlation.replace(['_', '.'], "-"),
        theseus_protocol::now_unix_ms() / 1000
    );
    let spec = ChangeSetSpec {
        stack: STACK,
        name,
        kind: "UPDATE",
        body: Some(body),
        url: None,
        parameters: params,
        role: Some(role_arn(&live.account.id, DEPLOYER)),
        tags,
        description: format!("theseus self-narrow: {why}"),
    };
    let id = cfn.create_change_set(&spec).await.map_err(said)?;
    let changes = match cfn.ready(STACK, &id).await.map_err(said)? {
        Ready::Changes { changes, .. } => changes,
        Ready::Empty { id } => {
            let _ = cfn.delete_change_set(STACK, &id).await;
            return Err(
                "the change set changes nothing: the stack already holds this template".into(),
            );
        }
        Ready::Failed { id, reason } => {
            let _ = cfn.delete_change_set(STACK, &id).await;
            return Err(format!("the change set failed: {reason}"));
        }
    };
    let rc = resource_changes(&changes);
    let changed: Vec<String> = rc.iter().map(|c| c.logical_id.clone()).collect();
    let stray: Vec<&String> = changed.iter().filter(|c| !expect.contains(c)).collect();
    let floor = theseus_aws_guard::embedded().check_change_set(&rc).floor;
    if !stray.is_empty() || !floor.is_empty() {
        let _ = cfn.delete_change_set(STACK, &id).await;
        return Err(format!(
            "the change set was dropped unrun: it touches {} (the narrowing touches only {}){}",
            if stray.is_empty() {
                "nothing outside it".to_string()
            } else {
                stray
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            },
            expect.join(", "),
            floor
                .first()
                .map(|h| format!(" and the guards stop it: {}", h.why))
                .unwrap_or_default(),
        ));
    }
    cfn.execute(STACK, &id).await.map_err(said)?;
    let settled = cfn.settle(STACK, SETTLE).await.map_err(said)?;
    if settled.going || settled.status.contains("ROLLBACK") || settled.status.contains("FAILED") {
        return Err(format!(
            "the stack ended {}{}{}",
            settled.status,
            settled.reason.map(|r| format!(": {r}")).unwrap_or_default(),
            if settled.failures.is_empty() {
                String::new()
            } else {
                format!(" ({})", settled.failures.join("; "))
            }
        ));
    }
    Ok(Applied {
        change_set: id,
        status: settled.status,
        changed,
    })
}

/// The hands stack's change sets, as the narrowing runs them.
pub trait Stack {
    fn apply(
        &self,
        body: &str,
        expect: &[String],
        why: &str,
    ) -> impl std::future::Future<Output = Result<Applied, String>> + Send;
}

impl Stack for Live {
    async fn apply(&self, body: &str, expect: &[String], why: &str) -> Result<Applied, String> {
        apply_template(self, body, expect, why).await
    }
}

#[cfg(test)]
mod tests {
    use theseus_aws::{Attribution, Call, Client, ClientConfig};

    use super::*;

    /// Every request `Live` makes is one the catalog knows, with the input
    /// members its shape names: the client checks each with no network.
    #[test]
    fn every_request_the_narrowing_makes_is_one_the_catalog_takes() {
        let client = Client::new(ClientConfig::new("us-west-2"));
        let attribution = Attribution {
            execution: None,
            call: None,
        };
        let doc = json!({"Version": "2012-10-17", "Statement": [
            {"Effect": "Allow", "Action": ["ec2:DescribeVpcs"], "Resource": ["*"]}]});
        let docs = vec![doc.to_string()];
        let policy = "arn:aws:iam::111122223333:policy/p";
        let calls: Vec<(&str, &str, Value)> = vec![
            (
                "cloudtrail",
                "LookupEvents",
                lookup_input(1_000, 2_000, Some("tok")),
            ),
            (
                "cloudtrail",
                "LookupEvents",
                lookup_input(1_000, 2_000, None),
            ),
            ("accessanalyzer", "ValidatePolicy", validate_input(&doc)),
            (
                "iam",
                "SimulateCustomPolicy",
                simulate_input(&docs, &["ec2:DescribeVpcs"], &[]),
            ),
            (
                "iam",
                "SimulateCustomPolicy",
                simulate_input(&docs, &["s3:GetObject"], &["arn:aws:s3:::b/k".to_string()]),
            ),
            (
                "iam",
                "ListAttachedRolePolicies",
                json!({"RoleName": "theseus-hand-owner"}),
            ),
            ("iam", "GetPolicy", json!({"PolicyArn": policy})),
            (
                "iam",
                "GetPolicyVersion",
                json!({"PolicyArn": policy, "VersionId": "v1"}),
            ),
            (
                "iam",
                "ListRolePolicies",
                json!({"RoleName": "theseus-hand-owner"}),
            ),
            (
                "iam",
                "GetRolePolicy",
                json!({"RoleName": "theseus-hand-owner", "PolicyName": "p"}),
            ),
            (
                "cloudformation",
                "GetTemplate",
                json!({"StackName": "theseus-hands", "TemplateStage": "Original"}),
            ),
        ];
        for (service, operation, input) in &calls {
            let call = Call {
                service,
                operation,
                input,
                region: Some("us-west-2"),
                pages: 1,
                attribution: &attribution,
            };
            let checked = client
                .check(&call)
                .unwrap_or_else(|e| panic!("{service}:{operation} {input}: {e}"));
            assert_eq!(&checked.operation, operation);
        }
    }
}
