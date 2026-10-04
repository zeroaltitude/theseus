//! Enforcer 1 for a direct call: what the gate does with an `aws.call` (the design's §3.4, §3.6, §3.9).
//!
//! The order, first match winning:
//! 1. a `direct` guardrail that hits is the floor, even for an operation that is otherwise IaC-only
//!    (lifting the budget's stop is a `DetachRolePolicy`);
//! 2. a stack write is invalid input, pointing to the stack tools, which show its change set;
//! 3. an IaC-only operation is invalid input, pointing to `aws.stack.plan`; its guardrails are checked
//!    when the stack applies;
//! 4. any other guardrail that hits is the floor;
//! 5. a destructive operation waits for approval;
//! 6. otherwise the posture decides.

use serde_json::Value;

use crate::eval::{Context, Truth};
use crate::list::{GuardList, Guardrail};
use crate::node::Node;

#[derive(Debug)]
pub enum Verdict<'l> {
    /// Nothing on the lists: `[policy.aws]` decides.
    Clear,
    /// The floor: the call asks at every posture. When a hit is `guarded`, AWS refuses the call in a
    /// work session, so an approved one runs in a floor session.
    Floor(Vec<Hit<'l>>),
    /// A stack write outside the stack tools: `aws.call` returns it as invalid input, and no request is
    /// sent.
    StackOnly,
    /// Durable infrastructure: `aws.call` returns it as invalid input, and no request is sent.
    IacOnly,
    /// Deleting a stateful resource, or a stack: the approve list.
    Destructive,
}

#[derive(Debug)]
pub struct Hit<'l> {
    pub guardrail: &'l Guardrail,
    /// False when the input left the answer open (a template's unknown value): it asks all the same.
    pub certain: bool,
    /// The value that hit, as `path = value`.
    pub detail: Option<String>,
}

impl Hit<'_> {
    /// The floor's confirm line, naming the guardrail: "public ingress: ec2:AuthorizeSecurityGroupIngress,
    /// a security group rule open to … (IpPermissions[0].IpRanges[0].CidrIp = 0.0.0.0/0)".
    pub fn confirm(&self, op: &str) -> String {
        let g = self.guardrail;
        let mut s = format!("{}: {op}, {}", g.limit.label(), g.summary);
        if let Some(d) = &self.detail {
            s.push_str(&format!(" ({d})"));
        }
        if !self.certain {
            s.push_str(" [may hit: a value is unresolved]");
        }
        s
    }

    pub fn guarded(&self) -> bool {
        self.guardrail.guarded()
    }
}

impl Guardrail {
    /// Does this entry hit the input? An entry without a `when` hits every call.
    pub fn hit(&self, input: &Node, ctx: &Context) -> Option<Hit<'_>> {
        match &self.when {
            None => Some(Hit {
                guardrail: self,
                certain: true,
                detail: None,
            }),
            Some(w) => {
                let f = w.eval(input, ctx);
                f.truth.hits().then(|| Hit {
                    guardrail: self,
                    certain: f.truth == Truth::Yes,
                    detail: f.detail,
                })
            }
        }
    }
}

impl GuardList {
    /// The gate's reading of one call: `op` is `service:Operation` by botocore id, `input` the call's
    /// JSON input.
    pub fn check_call(&self, ctx: &Context, op: &str, input: &Value) -> Verdict<'_> {
        let node = Node::from_json(input);
        let hits = |direct_only: bool| -> Vec<Hit<'_>> {
            self.for_operation(op)
                .filter(|g| g.direct || !direct_only)
                .filter_map(|g| g.hit(&node, ctx))
                .collect()
        };
        // A direct hit is the floor, and the confirm names every entry that hits beside it (a subnet
        // change that is also public ingress says both).
        if !hits(true).is_empty() {
            return Verdict::Floor(hits(false));
        }
        if self.stack_only(op) {
            return Verdict::StackOnly;
        }
        if self.iac_only(op) {
            return Verdict::IacOnly;
        }
        let all = hits(false);
        if !all.is_empty() {
            return Verdict::Floor(all);
        }
        if self.destructive(op) {
            return Verdict::Destructive;
        }
        Verdict::Clear
    }

    /// `aws.call`'s invalid-input message for an IaC-only operation.
    pub fn iac_message(op: &str) -> String {
        format!(
            "{op} makes or changes durable infrastructure; use aws.stack.plan (IaC is an operator limit)"
        )
    }

    /// `aws.call`'s invalid-input message for a stack write.
    pub fn stack_message(op: &str) -> String {
        format!(
            "{op} writes a stack outside the review; use aws.stack.plan, then aws.stack.apply or aws.stack.delete, which show the change set"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> Context {
        Context {
            account: "111122223333".into(),
            region: "us-west-2".into(),
        }
    }

    /// Another project's network (theseus-mgw.9): a direct change to plumbing that exists asks at
    /// the floor, even an IaC-only one, and names every entry that hits; making something new stays
    /// IaC-only, and a read is clear.
    #[test]
    fn a_change_to_network_plumbing_asks_and_a_create_stays_iac_only() {
        let l = crate::embedded();
        let names = |v: Verdict<'_>| match v {
            Verdict::Floor(h) => h
                .iter()
                .map(|h| h.guardrail.name.clone())
                .collect::<Vec<_>>(),
            v => panic!("{v:?}"),
        };
        for (op, input) in [
            (
                "ec2:DeleteRoute",
                json!({"RouteTableId": "rtb-0a1b2c3d4e5f6074a", "DestinationCidrBlock": "0.0.0.0/0"}),
            ),
            (
                "ec2:ReplaceRoute",
                json!({"RouteTableId": "rtb-0a1b2c3d4e5f6074a", "DestinationCidrBlock": "0.0.0.0/0", "GatewayId": "igw-0a1b2c3d4e5f6076a"}),
            ),
            (
                "ec2:DeleteNatGateway",
                json!({"NatGatewayId": "nat-0a1b2c3d4e5f6075a"}),
            ),
            (
                "ec2:ModifySubnetAttribute",
                json!({"SubnetId": "subnet-0a1b2c3d4e5f60711", "EnableDns64": {"Value": true}}),
            ),
        ] {
            assert_eq!(
                names(l.check_call(&ctx(), op, &input)),
                ["network-not-ours"],
                "{op}"
            );
        }
        let public =
            json!({"SubnetId": "subnet-0a1b2c3d4e5f60711", "MapPublicIpOnLaunch": {"Value": true}});
        assert_eq!(
            names(l.check_call(&ctx(), "ec2:ModifySubnetAttribute", &public)),
            ["subnet-public-ip", "network-not-ours"]
        );
        let create = json!({"RouteTableId": "rtb-0a1b2c3d4e5f6074a", "DestinationCidrBlock": "0.0.0.0/0", "NatGatewayId": "nat-0a1b2c3d4e5f6075a"});
        assert!(matches!(
            l.check_call(&ctx(), "ec2:CreateRoute", &create),
            Verdict::IacOnly
        ));
        let read = json!({"Filters": [{"Name": "vpc-id", "Values": ["vpc-0a1b2c3d4e5f60718"]}]});
        assert!(matches!(
            l.check_call(&ctx(), "ec2:DescribeRouteTables", &read),
            Verdict::Clear
        ));
    }

    #[test]
    fn the_order_direct_then_iac_then_floor_then_destructive() {
        let l = crate::embedded();
        // Lifting the budget's stop is a direct call to an otherwise IaC-only operation: the floor.
        let lift = json!({"RoleName": "theseus-owner", "PolicyArn": "arn:aws:iam::111122223333:policy/theseus-deny-spend"});
        match l.check_call(&ctx(), "iam:DetachRolePolicy", &lift) {
            Verdict::Floor(h) => assert_eq!(h[0].guardrail.name, "budget-stop-detach"),
            v => panic!("{v:?}"),
        }
        // Any other detach is a role change: a stack's.
        let other = json!({"RoleName": "theseus-owner", "PolicyArn": "arn:aws:iam::111122223333:policy/scratch"});
        assert!(matches!(
            l.check_call(&ctx(), "iam:DetachRolePolicy", &other),
            Verdict::IacOnly
        ));
        // An open security group rule is IaC-only first; its guardrail is checked when the stack applies.
        let open = json!({"GroupId": "sg-0a1", "IpPermissions": [{"IpRanges": [{"CidrIp": "0.0.0.0/0"}]}]});
        assert!(matches!(
            l.check_call(&ctx(), "ec2:AuthorizeSecurityGroupIngress", &open),
            Verdict::IacOnly
        ));
        // Stopping the trail is the floor, and AWS refuses it outside a floor session.
        match l.check_call(
            &ctx(),
            "cloudtrail:StopLogging",
            &json!({"Name": "theseus-trail"}),
        ) {
            Verdict::Floor(h) => {
                assert!(h[0].guarded());
                assert_eq!(
                    h[0].confirm("cloudtrail:StopLogging"),
                    "the audit trail: cloudtrail:StopLogging, stopping the trail's logging"
                );
            }
            v => panic!("{v:?}"),
        }
        // A task with a public IP asks; the same task without one is clear.
        let public = json!({"cluster": "theseus-hands", "networkConfiguration": {"awsvpcConfiguration": {"assignPublicIp": "ENABLED"}}});
        assert!(matches!(
            l.check_call(&ctx(), "ecs:RunTask", &public),
            Verdict::Floor(_)
        ));
        let private = json!({"cluster": "theseus-hands", "networkConfiguration": {"awsvpcConfiguration": {"assignPublicIp": "DISABLED"}}});
        assert!(matches!(
            l.check_call(&ctx(), "ecs:RunTask", &private),
            Verdict::Clear
        ));
        assert!(matches!(
            l.check_call(
                &ctx(),
                "ec2:TerminateInstances",
                &json!({"InstanceIds": ["i-0abc"]})
            ),
            Verdict::Destructive
        ));
        assert!(matches!(
            l.check_call(&ctx(), "s3:ListBuckets", &json!({})),
            Verdict::Clear
        ));
        // A stack changes only through the stack tools; making and reading change sets stays direct.
        let update = json!({"StackName": "theseus-foundation", "UsePreviousTemplate": true});
        assert!(matches!(
            l.check_call(&ctx(), "cloudformation:UpdateStack", &update),
            Verdict::StackOnly
        ));
        let change_set = json!({"StackName": "theseus-foundation", "ChangeSetName": "c1"});
        assert!(matches!(
            l.check_call(&ctx(), "cloudformation:CreateChangeSet", &change_set),
            Verdict::Clear
        ));
    }
}
