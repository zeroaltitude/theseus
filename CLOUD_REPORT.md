# CLOUD_REPORT: AWS hands on an existing network, step 40's network (theseus-mgw.9)

Branch `cloud/20261004-hands-network`, from `main` at `a4da5e1` (the task's commit `cb7aedc` on top). Four commits,
one per step, each gated:

| step | commit | subject |
|---|---|---|
| 1 | `161225d` | infra: the hands network in an existing VPC, never a NAT of its own |
| 2 | `89aa52c` | aws: the hands network's VPC comes from the config, and a plan cannot change it |
| 3 | `840ff07` | aws: Fargate hands in an existing VPC read its routes, and say which subnet has none |
| 4 | `ab27e36` | guard: another project's network, used and never changed, one entry and two enforcers |

No new dependency. No stored record changes: `MANIFEST_FORMAT` stays 11 (step 3 explains why). No protocol type
changes. The long files: `config.rs` grew by 5 lines (one `pub use` and two asserts), to 2,763 of 2,910.

## Step 1: the template (`161225d`)

**Found.** `theseus-hands-network.yaml` always made its own VPC, with nothing to skip.

**Changed.**
- New parameters `ExistingVpcId`, `ExistingSubnetIds` (a `CommaDelimitedList`) and `ExistingSecurityGroupId`, each
  defaulting to empty and pattern-checked.
- The condition `OwnVpc` now covers everything that belongs to the VPC itself: the VPC, its three subnets, both route
  tables and their three associations, the S3 endpoint, and the flow log with its log group and role. `NatEnabled`
  becomes `OwnVpc` and `NatGateway=enabled`, so it also covers the internet gateway, its attachment, the EIP, the NAT
  and both default routes.
- `HandsSecurityGroup` is made under `OwnSecurityGroup`, in `!If [ExistingVpc, ExistingVpcId, Vpc]`. It has no
  ingress and is tagged.
- `Rules`:
  - `NoNatBesideAnExistingVpc` asserts `NatGateway=disabled` and non-empty subnets when a VPC is named.
  - `NoExistingPartsWithoutTheVpc` refuses subnets or a group named without a VPC.
- The outputs keep their keys:
  - `VpcId`, `PrivateSubnetIds` and `HandsSecurityGroupId` name the existing network.
  - `NatGateway` reads `existing`.
- `infra/aws/test/rules.py` gains the `network-modes` rule. It evaluates the template's conditions and Rules under
  five parameter sets: its own VPC with the NAT off and on, an existing VPC with and without a named group, and the
  NAT asked for beside an existing VPC. `test_rules.py` has a hit for each miss.
- `infra/aws/README.md` is updated.

**Proved.**
- `infra/aws/check.sh` (cfn-lint 1.57.1, from a venv): 5 templates, 0 violations, and 36 rule tests pass. cfn-lint
  checks the template once, across both modes' conditions. `network-modes` then checks each mode's resources and Rules.
- New Rust tests in `theseus-core` `aws::tests_network`, run through the guard's static reader:
  - `an_existing_vpc_makes_the_hands_group_alone_and_never_a_nat`: an existing VPC plans `HandsSecurityGroup` alone,
    even with `NatGateway=enabled`, and nothing at all with a named group.
  - `the_network_template_scans_clean_in_both_modes`.
- **Planted revert:** `NatEnabled: !Equals [!Ref NatGateway, enabled]` (no `OwnVpc`).
  - The Rust test above failed (`tests_network.rs:87`).
  - `test_rules.test_network_modes_hold_for_the_template` failed.
  - `rules.py` printed six `network-modes` violations: InternetGateway, InternetGatewayAttachment, EgressDefaultRoute,
    NatAddress, Nat and PrivateDefaultRoute were all made beside an existing VPC.
  - Restored (`cmp` identical), touched, and the test passed again.
- The botocore models: pip's botocore ships `service-2.json.gz`, and `rules.py` reads only `service-2.json`. I
  unpacked them to `/tmp/botocore-data` and set `THESEUS_BOTOCORE_DATA`. On the owner's machine, the `aws` CLI's
  models are plain JSON.

## Step 2: the config (`89aa52c`)

**Changed.**
- `[aws.accounts.<id>.hands_network]` takes `vpc`, `subnets` and an optional `security_group`. Each id's form is
  checked at load (`vpc-`, `subnet-`, `sg-` and 8 to 17 hex digits).
- The default is none: the stack makes its own VPC, as before.
- `theseus.example.toml` carries the commented entry. The uncommented-template test now asserts it.
- `aws.stack.plan` of `theseus-hands-network` fills the three `Existing*` parameters from the config (empty when it
  names none). The scan and the change set use the filled parameters.
- The plan is refused before anything is sent, with the reason, when:
  - it gives any of those parameters another value (the message names the config table and its value), or
  - it asks for `NatGateway=enabled` beside an existing VPC.
- The rule lives in `aws::hands::network::plan_parameters`.

**Design choice:** I chose the refusal, not tend.rs's reconcile. A reconcile that applies on its own would remove the
stack's own VPC, or move the hands, with no plan shown. That is the wrong posture next to another project's network.

**Proved.**
- `config::aws::tests::an_aws_account_names_its_owner_role…`, extended:
  - the default is none;
  - the parameters are right with and without a network;
  - three bad forms are refused.
- `aws::tests_c2::the_hands_network_plan_takes_the_configs_existing_network`, against C2's stateful CloudFormation
  fake:
  - a new stack's change set is exactly `+ HandsSecurityGroup (AWS::EC2::SecurityGroup)`, with no VPC, subnet,
    route, endpoint, NAT, flow log or EIP, and nothing at the floor;
  - the `CreateChangeSet` carries the config's three values;
  - another VPC, and `NatGateway=enabled`, are refused with no request sent;
  - with no network in the config, a plan naming a VPC is refused (`(none)`).

## Step 3: discovery (`840ff07`)

**Found, a bug in the way:** `discover`'s `stack()` merged a stack's parameters over its outputs. In existing mode the
`NatGateway` parameter stays `disabled` while the output reads `existing`, so the mode would have been read as a NAT
that is off. Outputs now win.

**Changed.**
- With `NatGateway` reading `existing`, `network::egress` sends `DescribeRouteTables`, filtered on `vpc-id` (a read,
  in the work session). Each subnet's table is its own association, or else the VPC's main table. That table must send
  `0.0.0.0/0` to a `NatGatewayId` that is not a blackhole.
- Otherwise the Fargate launch is refused, naming the subnet and the table. The words never suggest a NAT: they tell
  the operator to name routed subnets in the config's `hands_network`, or run on Lambda.
- A refused discovery is not cached (`Hands::env`), so the next call reads the routes again.
- `HandsEnv.existing` is `#[serde(skip)]`. The group record keeps only `nat`, which is true for a routed existing VPC.
  So the stored layout is unchanged and no format bump is needed. The reviewer may prefer an in-memory map beside
  `Hands::envs`. I chose the skipped field as the smaller change.

**Proved.**
- `aws::hands::tests_network::fargate_in_an_existing_vpc_needs_its_routes_and_runs_in_its_subnets` runs against part
  1's stateful fake. Its EC2 XML route tables have the second subnet falling to the main table.
  - Unrouted: the refusal names `subnet …0722's route table (rtb-…074b)`. It has no `NatGateway=enabled` and no
    `$36`. There is no `RunTask` and no task definition, and one route read, filtered on the VPC.
  - Routed: the same core reads the routes again. One `RunTask` runs in the configured subnets with the stack's
    group and `assignPublicIp: DISABLED`. The record has `nat`, and its envelope settles the hand `Succeeded`.
- **Planted revert:** outputs merged before parameters again. The test failed with "the hands VPC's NAT, which is
  off". Restored, touched, and it passed.
- **Under load** (four busy loops at nice 0, the tests at nice 19): this test, the plan test and part 1's Fargate test,
  as one nextest run of three tests, passed 20 of 20 runs.

## Step 4: the guard (`ab27e36`)

**Changed.** One entry, `network-not-ours`, under a new limit, `others-resources` (label "another project's
resources").

- **The gate.**
  - `operations` is every direct change to plumbing that exists: deletions, modifies, replaces, revokes,
    disassociations and detaches of VPCs, subnets, route tables and routes, NAT, internet and egress-only gateways,
    ACLs, endpoints, flow logs, security groups and peering.
  - It is `direct`, so `ReplaceRoute` and `ModifySubnetAttribute` ask at the floor although they are IaC-only. The
    list's own rule requires a `when` for that, so its `when` is `present` on the ids those calls name: a call that
    names plumbing that exists.
  - Creates of new things (`CreateRoute`, `CreateSubnet`, …) stay IaC-only.
  - `check_call` now names every entry that hits beside a direct one. A `ModifySubnetAttribute` that maps public IPs
    says `subnet-public-ip` and `network-not-ours`.
- **The template rule.** Routes, associations, subnets, CIDR blocks, route tables, endpoints, NATs, flow logs,
  gateway attachments, ACLs and their entries, SG ingress and egress, DHCP associations, peering, transit attachments
  and route propagation all ask at plan when a member names a resource the template does not make.
  - The evaluator's new `not_own` test reads that member. A literal or a parameter's value is a yes, an import is a
    maybe, and the template's own `Ref` or `GetAtt` is a no.
  - A security group in another VPC is not a hit.
- **AWS side.**
  - The rule is `scp = "deny"`, on IAM patterns `ec2:Delete*`, `Disassociate*`, `Detach*`, `Replace*`, `Modify*`,
    `Associate*`, `Attach*`, `Authorize*`, `Revoke*`, and `CreateRoute`, `CreateNetworkAclEntry` and `CreateTags`.
  - It applies to the ARNs of VPCs (`vpc*`), subnets, route tables, NATs, internet gateways, ACLs and security groups.
  - The condition is `Null: {aws:ResourceTag/theseus:owner: true, ec2:CreateAction: true}`: untagged, and not a tag put
    on at a create. A tag can't be forged onto someone else's resource, and creates with tags still work.
  - `security-group-rule` ARNs are left out of the resources, so a rule's own (untagged) resource never trips it.
  - It appears in `theseus-guard-limits` (5,535 of 6,144), the boundary (**6,016 of 6,144**: little room left for
    other branches' additions), the SCPs, and a new `theseus-guard-deployer`.
- **The deployer.** "The deployer included" had no enforcer inside the account: the deployer carries no session
  guard, and SCPs need an Organization.
  - `theseus-guard-deployer` (2,045 characters) holds every `scp = "deny"` entry's denies. It is generated, and
    attached to `DeployerRole`'s `ManagedPolicyArns` in the foundation, with `GuardDeployerPolicy` added to the
    foundation's stack policy.
  - Its other entries (the account's public-access block, StopLogging, EBS default encryption, IAM users and keys,
    root, the budget's stop, Organizations, contacts, passing the deployer) are nothing a stack does today.
  - **This needs the owner's attention.**
- **Which comes first: the group.** IAM authorizes `ec2:CreateSecurityGroup` against the VPC too, so a deny on
  untagged resources would stop the hands' group in another VPC. The AWS side leaves that create out, and the stack
  makes a tagged group with no ingress. A test pins that the deployer's guard does not deny it, on the VPC or the group.
- **Inventory and reaper:** unchanged. They list by `theseus:owner` (inventory's `TagFilters`; the reaper's ECS tasks),
  so the existing network is never theirs.

**Proved.**
- `tests/cases.toml` gains six call cases and five template cases:
  - hits: `DeleteRoute`, `ReplaceRoute`, `DeleteNatGateway`, `ModifySubnetAttribute`; a Route into a literal table;
    an association naming a literal subnet; an endpoint into an imported VPC;
  - near misses: `DescribeRouteTables`, `CreateRoute`, a route in the stack's own table, a security group in a
    literal VPC.
- `check::tests::a_change_to_network_plumbing_asks_and_a_create_stays_iac_only` checks the gate's verdicts:
  - the four above ask at the floor;
  - the public-IP case names both entries;
  - `CreateRoute` stays `IacOnly`;
  - a read is `Clear`.
- `aws::tests_network::a_route_into_a_table_the_stack_does_not_own_asks`: a full template whose `AWS::EC2::Route`
  names a literal table, a parameter, or an import asks ("another project's resources: Out (AWS::EC2::Route)…"). A
  `!Ref` of its own table does not.
- `tests/policies.rs`:
  - `network_plumbing_not_ours_is_refused_and_running_in_it_is_not`: `DeleteRoute`, `ReplaceRoute`,
    `DeleteNatGateway`, `ModifySubnetAttribute` and `CreateTags` on an untagged resource are refused by the session
    guard, the boundary and the deployer's guard. No statement denies `ecs:RunTask` or `ec2:CreateNetworkInterface`.
  - `the_deployer_changes_only_what_it_tagged_and_may_make_the_hands_group`.
  - All existing tests pass, among them each form for each entry, the boundary holding every guard, and the copies on
    disk and in the foundation (regenerated with `THESEUS_GUARD_WRITE=1`).
- check.sh after the guard: 0 violations. The boundary rule covers `theseus-guard-deployer`, and the wildcards rule
  finds no read among the new patterns.
- **Planted revert:** the `[guardrail.iam_condition]` was removed from the entry. Three tests failed:
  - `the_deployer_changes_only_what_it_tagged_and_may_make_the_hands_group`, with "NetworkNotOurs: denies
    ec2:DeleteRoute whatever the tag" and the same for each of the five;
  - the two copies-on-disk tests.

  Restored, touched, and all 10 passed.

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` ran before each commit. fmt, shape, features, clippy, the cockpit, the test
build and the reader rule passed each time. The suite failed each time on the same 34, none of them mine. The last
gate ran 2,125 tests: 2,091 passed and 34 failed.

- **32 sandbox tests:** 19 in `theseus-sandbox::contract` and 13 in `theseusd::sandbox`. The VM runs as root, and
  `spawn` refuses because Linux exempts root from `RLIMIT_NPROC` (theseus-pv6i, known).
- **`theseus-sandbox::bench spawn_100`:** the same cause.
- **`theseus-core tests_output::the_cores_output_matches_its_golden`:** the golden has a `-#:#` UTC offset and this VM
  prints `+#:#` (the wake line's local time). It fails on the base commit too: I checked with my changes stashed. It
  is environmental, and the maintainer's machine should pass it.

The phases after the suite, run by hand each time, all passed:
- the protocol types (`cockpit/src/protocol.gen` unchanged);
- the turn bench, `--runs 5 --burst 0`: plain 5 of 5, tool 9 of 9;
- `cargo deny --offline check`: advisories, bans, licences and sources ok, after `cargo deny fetch` succeeded.

Not run here: the lifecycle and jobs benches (`NO_BENCH`) and `infra/aws/check.sh` in the gate (it isn't part of it).
I ran check.sh myself, as above.

## The live check (the maintainer's, with the owner's go)

Use a scratch daemon with the account bound, built from this branch, with the foundation and hands stacks applied.
First apply the foundation: its plan shows `+ GuardDeployerPolicy` and `~ DeployerRole` at the floor (deployer-role).
Then:

1. Put the network in the account's config table: `[aws.accounts.<the account>.hands_network]` with `vpc`, `subnets`,
   and no `security_group`. Then, in a session:
   `aws.stack.plan {"stack": "theseus-hands-network", "template": "infra/aws/theseus-hands-network.yaml"}`.
   - **Expect** a new stack's change set of exactly `+ HandsSecurityGroup (AWS::EC2::SecurityGroup)`, and nothing at
     the floor.
   - **If the stack already exists with its own VPC**, the plan shows `~ HandsSecurityGroup` as replaced (its VPC
     changes). It also shows `-` for Vpc, the three subnets, both route tables and their three associations, the S3
     endpoint, FlowLog, FlowLogRole and FlowLogGroup, and the NAT parts if they were on. FlowLogGroup is a log group,
     so it waits for approval as stateful. The replaced group then takes the old name `theseus-hands` in a new VPC.
     CloudFormation creates before it deletes, and `GroupName` is fixed, so a replace may fail on the name, or the
     group may still be in use by a running task. Watch for that.
   - A plan with `"parameters": {"ExistingVpcId": "vpc-…"}` naming another VPC is refused with the config's value.
     `{"NatGateway": "enabled"}` is refused too.
   - Apply with the owner's go: `aws.stack.apply {"stack": "theseus-hands-network", "digest": "<the plan's>"}`.
   - **Risk to watch:** the deployer's new guard denies `ec2:CreateTags` on an untagged security group outside a
     create. CloudFormation should tag the group at creation (`ec2:CreateAction` present). If it tags after the
     create, the apply fails on `CreateTags`, and the fix is the config's `security_group` or removing `CreateTags`
     from the entry's `iam`.
2. A one-hand Fargate group: `aws.hands.run {"argv": ["true"], "backend": "fargate"}`. (Start a fresh daemon, or let
   the first call discover after the apply. A cached env from before the apply stays until restart.)
   - **Expect:** an `aws.called` row for `ec2:DescribeRouteTables`, then the launch. The task's
     `networkConfiguration` names the configured subnets and the stack's group. The envelope arrives and the hand
     settles once (`theseus` session view; `hands.list`).
   - If a subnet does not route to the NAT, the result names it, and no task runs.
3. Ask the model: "delete the default route from the route table of `<a configured subnet>`". The `ec2:DeleteRoute`
   asks at the floor with "another project's resources: ec2:DeleteRoute, a change to network plumbing …". Decline.
   (Approved, AWS would still refuse it in a work session: the route table is untagged. In a floor session,
   allow-all alone, it would run.)
4. `aws.inventory`: lists `theseus-hands` (the security group) and nothing of the existing VPC, its subnets, route
   tables or NAT.

## Questions and decisions for the owner

- **Is a Theseus security group in another project's VPC a change to it?** I built it as not a change: the group is
  tagged, has no ingress, and is attached to nothing but Theseus's tasks, so the VPC's traffic is unchanged. The
  alternative, the config's `security_group`, means using another project's group: its rules would then govern
  Theseus's tasks, and any edit to them would touch the other project. Both work; the tagged group is the default.
- **Flow logs:** none, on a VPC Theseus did not make. The other project's own flow logs, if it has any, cover the
  hands' traffic. Switching an existing stack to the existing mode deletes the stack's flow-log group, which waits for
  approval as stateful.
- **The S3 endpoint:** none. The hands' S3 traffic (and ECR's layers, which are on S3) goes through the existing NAT
  at about $0.045 a GB. A gateway endpoint would mean changing the other project's route tables, which the guard
  refuses.
- **The deployer's guard** binds the deployer to every `scp = "deny"` entry, not only this one. That is what "deny"
  promises, but the deployer was not held to it before. If any stack ever needs one of those actions, its apply fails
  until that changes.
- **What the AWS side leaves to the gate:** creates of new things inside another VPC (`CreateSubnet`,
  `CreateRouteTable`, `CreateVpcEndpoint`, `CreateFlowLogs`, `CreateNatGateway`, `CreateSecurityGroup`).
  - Their new resources carry no tag yet, and denying on that would also stop Theseus's own tagged creates.
  - Sessions are already denied them (IaC-only, and the boundary's compaction), and in a stack the template rule asks.
  - Only the deployer could make one, and only through a change set the floor saw.
- **The floor session** (allow-all alone, after an approval) is not under the guards, as for every other entry.
  Under an Organization, the SCP form would also hold it.
- `plan_parameters` keys on the stack's name, `theseus-hands-network`. The same template planned under another name
  is not filled from the config.

## Docs the maintainer should update

- `docs/design/aws-toolset.md`:
  - §3.3's "Network, without public ingress": the existing-VPC mode, no NAT of Theseus's own, and the discovery's
    route check.
  - §3.6 and appendix C: `network-not-ours`, the `others-resources` limit, `not_own`, and `theseus-guard-deployer`.
- The spec's Part III item for theseus-mgw.9, and `docs/status.md`'s roadmap row for step 40's network.
- `crates/theseus-core/AGENTS.md`, if it lists the hands modules: `aws::hands::network`.
