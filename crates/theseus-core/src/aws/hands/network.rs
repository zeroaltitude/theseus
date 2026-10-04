//! The hands' network (step 40's network, theseus-mgw.9): the hands
//! network stack's own VPC, or an existing VPC the config names, whose
//! private subnets route out through its own NAT. Theseus uses an existing
//! network and never changes it.
//!
//! - **The plan takes the config's network.** `aws.stack.plan` of
//!   `theseus-hands-network` fills `ExistingVpcId`, `ExistingSubnetIds`,
//!   and `ExistingSecurityGroupId` from `[aws.accounts.<id>.hands_network]`
//!   (each empty when it names none), and refuses a plan that gives any of
//!   them another value, or `NatGateway=enabled` beside an existing VPC,
//!   saying why. The config is the one place the network is chosen: a plan
//!   cannot move the hands into a VPC the operator did not name, and no
//!   reconcile changes the stack on its own.

use std::collections::BTreeMap;

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
