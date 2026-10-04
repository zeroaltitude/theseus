//! Fargate hands in an existing VPC (step 40's network, theseus-mgw.9),
//! against part 1's stateful fake: discovery reads the VPC's route tables,
//! a subnet with no route to a NAT is refused, naming it, with no
//! `RunTask` and no word of a NAT to turn on; once it routes, the group
//! runs in the configured subnets with the stack's group, and its envelope
//! settles its hand.

use std::sync::atomic::Ordering;

use serde_json::json;
use theseus_kernel::ActionState;

use super::envelope::HandSpec;
use super::tests_hands::{group_of, outputs, record, rig, signed, state_of, turn, until};
use crate::provider::Scripted;

const VPC: &str = "vpc-0a1b2c3d4e5f60718";
const SUBNET_A: &str = "subnet-0a1b2c3d4e5f60711";
const SUBNET_B: &str = "subnet-0a1b2c3d4e5f60722";
const GROUP: &str = "sg-0a1b2c3d4e5f6073c";

/// The network stack's outputs in an existing VPC.
pub(super) fn existing_outputs() -> String {
    outputs(
        &[
            ("VpcId", VPC),
            ("PrivateSubnetIds", &format!("{SUBNET_A},{SUBNET_B}")),
            ("HandsSecurityGroupId", GROUP),
            ("NatGateway", "existing"),
        ],
        &[
            ("ExistingVpcId", VPC),
            ("ExistingSubnetIds", &format!("{SUBNET_A},{SUBNET_B}")),
            ("NatGateway", "disabled"),
        ],
    )
}

/// The existing VPC's route tables, as EC2 answers `DescribeRouteTables`:
/// the first subnet's own table routes out through the VPC's NAT; the
/// second subnet has no table of its own, so the main one is its, and that
/// one sends 0.0.0.0/0 to the NAT too, unless `unrouted`.
pub(super) fn route_tables(unrouted: bool) -> String {
    let local = "<item><destinationCidrBlock>10.20.0.0/16</destinationCidrBlock>\
                 <gatewayId>local</gatewayId><state>active</state></item>";
    let nat = "<item><destinationCidrBlock>0.0.0.0/0</destinationCidrBlock>\
               <natGatewayId>nat-0a1b2c3d4e5f6075a</natGatewayId><state>active</state></item>";
    let table = |id: &str, routes: String, assoc: String| {
        format!(
            "<item><routeTableId>{id}</routeTableId><vpcId>{VPC}</vpcId>\
             <routeSet>{routes}</routeSet><associationSet>{assoc}</associationSet></item>"
        )
    };
    let own = table(
        "rtb-0a1b2c3d4e5f6074a",
        format!("{local}{nat}"),
        format!(
            "<item><routeTableAssociationId>rtbassoc-0a</routeTableAssociationId>\
             <routeTableId>rtb-0a1b2c3d4e5f6074a</routeTableId><subnetId>{SUBNET_A}</subnetId>\
             <main>false</main></item>"
        ),
    );
    let main = table(
        "rtb-0a1b2c3d4e5f6074b",
        if unrouted {
            local.to_string()
        } else {
            format!("{local}{nat}")
        },
        "<item><routeTableAssociationId>rtbassoc-0b</routeTableAssociationId>\
         <routeTableId>rtb-0a1b2c3d4e5f6074b</routeTableId><main>true</main></item>"
            .into(),
    );
    format!(
        "<DescribeRouteTablesResponse xmlns=\"http://ec2.amazonaws.com/doc/2016-11-15/\">\
         <requestId>r</requestId><routeTableSet>{own}{main}</routeTableSet>\
         </DescribeRouteTablesResponse>"
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn fargate_in_an_existing_vpc_needs_its_routes_and_runs_in_its_subnets() {
    let fargate = |id: &str| {
        Scripted::tools(
            "",
            &[(
                id,
                "aws_hands_run",
                json!({"argv": ["true"], "backend": "fargate"}),
            )],
        )
    };
    let r = rig(vec![
        fargate("t1"),
        Scripted::text("No route."),
        fargate("t2"),
        Scripted::text("Running."),
        Scripted::text("Done."),
    ]);
    r.state.existing.store(true, Ordering::SeqCst);
    r.state.unrouted.store(true, Ordering::SeqCst);

    // The second subnet's table (the VPC's main one) has no NAT route.
    let res = turn(&r.core, "run on fargate").await;
    let text = r
        .core
        .store
        .session_nodes(&res.session_id)
        .unwrap()
        .into_iter()
        .find_map(|(_, n)| match &n.body {
            crate::node::Body::ToolResult { tool, content, .. } if tool == "aws.hands.run" => {
                Some(content.clone())
            }
            _ => None,
        })
        .unwrap();
    assert!(
        text.contains(&format!(
            "subnet {SUBNET_B}'s route table (rtb-0a1b2c3d4e5f6074b) sends 0.0.0.0/0 to no NAT gateway"
        )) && text.contains("never changes it"),
        "{text}"
    );
    assert!(
        !text.contains("NatGateway=enabled") && !text.contains("$36"),
        "an existing network's refusal suggests no NAT of Theseus's own: {text}"
    );
    assert!(r.state.ran.lock().unwrap().is_empty());
    assert!(r.state.registered.lock().unwrap().is_empty());
    let reads = r.state.route_reads.lock().unwrap().clone();
    assert_eq!(reads.len(), 1);
    assert!(
        reads[0].contains("vpc-id") && reads[0].contains(VPC),
        "{}",
        reads[0]
    );

    // Its owner routes it: the next call reads the routes again (a refusal
    // is not kept), and the group runs in the existing subnets with the
    // stack's group and no public IP.
    r.state.unrouted.store(false, Ordering::SeqCst);
    turn(&r.core, "run on fargate again").await;
    assert_eq!(r.state.route_reads.lock().unwrap().len(), 2);
    let ran = r.state.ran.lock().unwrap().clone();
    assert_eq!(ran.len(), 1);
    let net = &ran[0]["networkConfiguration"]["awsvpcConfiguration"];
    assert_eq!(net["subnets"], json!([SUBNET_A, SUBNET_B]));
    assert_eq!(net["securityGroups"], json!([GROUP]));
    assert_eq!(net["assignPublicIp"], "DISABLED");
    let g = group_of(&r.core);
    let rec = record(&r.core, &g);
    assert!(
        rec.env.nat && rec.env.existing.is_none(),
        "the record keeps nat alone"
    );

    // Its envelope arrives and settles its hand (a duplicate's single
    // settle is part 1's test).
    let spec: HandSpec = serde_json::from_str(
        ran[0]["overrides"]["containerOverrides"][0]["environment"][0]["value"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    r.core.poll_hands_after_serving();
    r.state.push(signed(&spec, "succeeded", 0));
    until("the hand settled", || {
        state_of(&r.core, &spec.correlation_id) == ActionState::Succeeded
    })
    .await;
}
