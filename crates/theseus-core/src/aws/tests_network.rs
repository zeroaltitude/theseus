//! The hands on an existing network (step 40's network, theseus-mgw.9):
//! the network template in both of its modes, read as the guard's scan
//! reads it, and the guard's template rule for a resource in plumbing the
//! stack does not own.

use std::collections::BTreeMap;

use theseus_aws_guard::{Context, Truth};

use super::tests::ACCOUNT;

/// The hands network, as `aws.stack.plan` reads it from the repository.
pub(super) const NETWORK_TEMPLATE: &str =
    include_str!("../../../../infra/aws/theseus-hands-network.yaml");

/// An existing network's invented ids.
pub(super) const VPC: &str = "vpc-0a1b2c3d4e5f60718";
pub(super) const SUBNETS: &str = "subnet-0a1b2c3d4e5f60711,subnet-0a1b2c3d4e5f60722";

fn ctx() -> Context {
    Context {
        account: ACCOUNT.into(),
        region: "us-west-2".into(),
    }
}

fn given(params: &[(&str, &str)]) -> BTreeMap<String, String> {
    params
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// What the network template makes under `params`: logical id and type.
fn made(params: &[(&str, &str)]) -> Vec<(String, String)> {
    let t = theseus_aws_guard::parse_template(NETWORK_TEMPLATE).unwrap();
    theseus_aws_guard::planned_resources(&t, &ctx(), &given(params))
        .unwrap()
        .into_iter()
        .map(|r| {
            assert_eq!(r.exists, Truth::Yes, "{} under {params:?}", r.logical_id);
            (r.logical_id, r.resource_type)
        })
        .collect()
}

/// The guard's floor lines for the network template under `params`.
fn hits(template: &str, params: &[(&str, &str)]) -> Vec<String> {
    let t = theseus_aws_guard::parse_template(template).unwrap();
    let scan = theseus_aws_guard::embedded()
        .scan(&t, &ctx(), &given(params))
        .unwrap();
    scan.hits.iter().map(|h| h.confirm()).collect()
}

#[test]
fn an_existing_vpc_makes_the_hands_group_alone_and_never_a_nat() {
    // Its own VPC, the NAT off and on: the VPC and its parts, and the NAT
    // only when enabled.
    let own = made(&[]);
    let types = |m: &[(String, String)]| m.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>();
    for ty in [
        "AWS::EC2::VPC",
        "AWS::EC2::Subnet",
        "AWS::EC2::RouteTable",
        "AWS::EC2::VPCEndpoint",
        "AWS::EC2::FlowLog",
        "AWS::EC2::SecurityGroup",
    ] {
        assert!(types(&own).contains(&ty.to_string()), "{ty}");
    }
    assert!(!types(&own).contains(&"AWS::EC2::NatGateway".to_string()));
    let nat = made(&[("NatGateway", "enabled")]);
    assert!(types(&nat).contains(&"AWS::EC2::NatGateway".to_string()));

    // An existing VPC: the hands' security group alone, even if the
    // parameter asks for the NAT (the template's Rules refuse that; this is
    // the second line).
    for params in [
        &[("ExistingVpcId", VPC), ("ExistingSubnetIds", SUBNETS)][..],
        &[
            ("ExistingVpcId", VPC),
            ("ExistingSubnetIds", SUBNETS),
            ("NatGateway", "enabled"),
        ][..],
    ] {
        assert_eq!(
            made(params),
            vec![(
                "HandsSecurityGroup".to_string(),
                "AWS::EC2::SecurityGroup".to_string()
            )],
            "{params:?}"
        );
    }
    // And nothing at all when the config names a group there.
    assert_eq!(
        made(&[
            ("ExistingVpcId", VPC),
            ("ExistingSubnetIds", SUBNETS),
            ("ExistingSecurityGroupId", "sg-0a1b2c3d4e5f60733"),
        ]),
        vec![]
    );
}

#[test]
fn the_network_template_scans_clean_in_both_modes() {
    assert_eq!(hits(NETWORK_TEMPLATE, &[]), Vec::<String>::new());
    assert_eq!(
        hits(
            NETWORK_TEMPLATE,
            &[("ExistingVpcId", VPC), ("ExistingSubnetIds", SUBNETS)]
        ),
        Vec::<String>::new(),
        "the group in an existing VPC is not a change to it"
    );
    // Its own NAT is the floor, as at bootstrap.
    assert!(!hits(NETWORK_TEMPLATE, &[("NatGateway", "enabled")]).is_empty());
}
