//! The hands' network (step 40's network, theseus-mgw.9): the hands
//! network stack's own VPC, or an existing VPC the config names, whose
//! private subnets route out through its own NAT. Theseus uses an existing
//! network and never changes it (the guard list's `network-not-ours`).
//!
//! - **The plan takes the config's network.** `aws.stack.plan` of
//!   `theseus-hands-network` fills `ExistingVpcId`, `ExistingSubnetIds`,
//!   and `ExistingSecurityGroupId` from `[aws.accounts.<id>.hands_network]`
//!   (each empty when it names none), and refuses a plan that gives any of
//!   them another value, or `NatGateway=enabled` beside an existing VPC,
//!   saying why. The config is the one place the network is chosen: a plan
//!   cannot move the hands into a VPC the operator did not name, and no
//!   reconcile changes the stack on its own.
//! - **Discovery reads the routes.** When the stack's `NatGateway` output
//!   reads `existing`, [`egress`] reads the VPC's route tables
//!   (`DescribeRouteTables`, a read) and a Fargate launch is refused, naming
//!   the subnet, unless each subnet's table (its own, else the VPC's main
//!   one) sends `0.0.0.0/0` to a NAT gateway. Its words never suggest a NAT
//!   of Theseus's own: the network is its owner's.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{json, Value};
use theseus_tools::AwsBinding;

use crate::aws::session::Kind;
use crate::aws::{Account, Request, Signer};
use crate::config::HandsNetwork;

/// The hands network's stack.
pub const STACK: &str = "theseus-hands-network";

/// A plan's parameters for `stack`: for the hands network, `given` with the
/// config's network filled in, or why the plan is refused; any other stack's
/// as given.
pub fn plan_parameters(
    stack: &str,
    account: &str,
    net: Option<&HandsNetwork>,
    mut given: BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, String> {
    if stack != STACK {
        return Ok(given);
    }
    let table = format!("[aws.accounts.{account}.hands_network]");
    for (key, want) in HandsNetwork::parameters(net) {
        match given.get(key) {
            Some(v) if *v != want => {
                let named = if want.is_empty() {
                    "none".to_string()
                } else {
                    want
                };
                return Err(format!(
                    "{STACK}'s {key} comes from the config's {table} ({named}), not the plan \
                     ({v:?}): the operator chooses the hands' network there"
                ));
            }
            Some(_) => {}
            None => {
                given.insert(key.to_string(), want);
            }
        }
    }
    if net.is_some() && given.get("NatGateway").is_some_and(|v| v == "enabled") {
        return Err(format!(
            "{STACK} runs the hands in the existing VPC {table} names, whose subnets route out \
             through its own NAT: the stack never makes a NAT beside it (NatGateway=enabled is \
             refused)"
        ));
    }
    Ok(given)
}

/// Whether a Fargate hand in `subnets` of the existing VPC `vpc` has a way
/// out: each subnet's route table sends `0.0.0.0/0` to a NAT gateway. The
/// reason, naming the subnet, when one does not, or when the tables could
/// not be read.
pub async fn egress(
    account: &Arc<Account>,
    binding: Option<&AwsBinding>,
    region: &str,
    vpc: &str,
    subnets: &[String],
) -> Result<(), String> {
    let input = json!({"Filters": [{"Name": "vpc-id", "Values": [vpc]}]});
    let out = account
        .request(
            binding,
            &Request {
                service: "ec2",
                operation: "DescribeRouteTables",
                input: &input,
                region,
                pages: 5,
                class: "read",
                signer: Signer::As(Kind::Work),
            },
        )
        .await
        .map_err(|e| {
            format!(
                "Fargate hands run in the existing VPC {vpc}, whose route tables could not be \
                 read ({e}); the call can run on Lambda"
            )
        })?;
    let tables: Vec<&Value> = out.body["RouteTables"]
        .as_array()
        .into_iter()
        .flatten()
        .collect();
    for subnet in subnets {
        unrouted(&tables, vpc, subnet).map_or(Ok(()), Err)?;
    }
    Ok(())
}

/// Why `subnet` has no way out through a NAT, if it has none.
fn unrouted(tables: &[&Value], vpc: &str, subnet: &str) -> Option<String> {
    let assoc = |t: &&&Value, f: &dyn Fn(&Value) -> bool| {
        t["Associations"].as_array().into_iter().flatten().any(f)
    };
    let table = tables
        .iter()
        .find(|t| assoc(t, &|a| a["SubnetId"].as_str() == Some(subnet)))
        .or_else(|| {
            tables
                .iter()
                .find(|t| assoc(t, &|a| a["Main"].as_bool() == Some(true)))
        });
    let refuse = |why: String| {
        Some(format!(
            "Fargate hands need a way out of the existing VPC {vpc}, and {why}, so a hand could \
             not pull its image or reach the queue. Theseus uses that network and never changes \
             it: name subnets whose route tables send 0.0.0.0/0 to the VPC's NAT gateway in the \
             config's hands_network, or the call runs on Lambda"
        ))
    };
    let Some(t) = table else {
        return refuse(format!("subnet {subnet} has no route table there"));
    };
    let id = t["RouteTableId"].as_str().unwrap_or("?");
    let out = t["Routes"].as_array().into_iter().flatten().any(|r| {
        r["DestinationCidrBlock"].as_str() == Some("0.0.0.0/0")
            && r["NatGatewayId"].as_str().is_some_and(|n| !n.is_empty())
            && r["State"].as_str() != Some("blackhole")
    });
    if out {
        None
    } else {
        refuse(format!(
            "subnet {subnet}'s route table ({id}) sends 0.0.0.0/0 to no NAT gateway"
        ))
    }
}
