<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate you touch, scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

**Setup** (about 15 minutes, once):
- First, in the foreground and alone, run `cargo --version` and wait for it: it installs the toolchain that
  rust-toolchain.toml names (about 30 seconds). Start no other cargo or rustup command until it is done: two at once
  collide in rustup's download directory. If it fails, run it again.
- `cargo install cargo-nextest --locked` (about 3.5 minutes) and `cargo install cargo-deny --locked`.
- `npm ci` in cockpit/.
- `cargo build --workspace --all-targets` (about 9 minutes cold).
- `cargo deny fetch`, so the gate's deny phase can run offline. If the fetch fails, skip that phase and say so in the report.
- Run long commands in the background and wait for their completion notice. Don't end your turn while work remains, unless a background command will wake you.
- Never delete anything outside the repository and /tmp (nothing under /root/.rustup or /root/.cargo). This
  environment refuses some commands, and three refusals in a row stop the session until a person looks: when one is
  refused, take another route instead of retrying it.

**Known on this VM, and not yours to fix** unless your task names them (other changes fix them):
- The sandbox's contract tests that need a non-root user can fail or skip here: the VM runs everything as root, and Linux exempts root from `RLIMIT_NPROC` (theseus-pv6i).
- `theseus-core tests_push::the_snapshot_and_the_events_agree_under_the_position_rule` failed once in 20 suites here (theseus-amr2).
- A reaping or job test whose `turn.submit` is answered `internal`, `No such file or directory (os error 2)`, under load (theseus-46ya: a spool read race).

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine. Any other failure is yours to explain.

**Other changes in flight.** About twenty-five other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

These are reviewed, and merging into `main` one at a time tonight. Your clone may hold some of them already:
- the Jev wire-in (`[judge]`, `JudgeService`, the judgment sink, `loop.v1` in shadow, crates/theseus-judge's client);
- the ontology wired in (crates/theseus-ontology's records, its snapshot, the context compiler's guidance walk; `MANIFEST_FORMAT` goes to 7);
- MCP tools in turns (`McpBoard`, `[mcp.servers]`, the `Tool` trait's names as `&str`);
- `theseus judge prove` (a report generator in crates/theseus-judge);
- five gate flakes;
- the user unit's restart limits and install checks (crates/theseusd/src/install/, scripts/user-service.sh);
- the MCP server (`[mcp_server]`, `/mcp` on loopback, `Surface::Mcp`);
- AWS hands on Lambda and Fargate (the `hand` role, `aws.hands.run`, the SQS poller, infra/aws/'s hands files);
- the durability tender (WAL segments and blobs to S3, index rows to DynamoDB);
- a terminal toolset (`term.*`, a pty per session);
- the gate's bench build, features recheck and cockpit build (scripts/gate.sh).

These are other cloud sessions like you, each on its own branch:
- the judgment surfaces (trace marks, `judge` spans, `judge.list`/`get`, the cockpit's Judgment section);
- `security.v1` in shadow at the gate;
- `classify.v1` and `role.v1` at inbound;
- CONTINUE's signals and `continue.v1` in shadow;
- the arrangement on `task.create`;
- the memory `Recall` node, `derived_from` edges and the `BudgetReport`;
- the `+rerank` memory arm;
- the cockpit's Ontology view;
- `categorize.v1` in shadow and `tasks.parked` in health;
- bindings format 2 (many guilds, per-place ceilings);
- MCP prompts (`/prompt`, `theseus prompt`, the cockpit's picker);
- `extend.propose`;
- `theseus restore --from s3://…`;
- AWS hands' cancellation, TTL reaper, reservations and grid;
- the LSP board and tools (crates/theseus-lsp's consumers).

On the owner's machine:
- a benchmark run (bench/, and docs/benchmarks.md at its end).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field to a stored record or a new record kind, bump it by one from main's number as you cloned it, and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. Others bump it too: the maintainer renumbers at the merge.
- **Files near their line ceiling** (scripts/long-files.txt): crates/theseus-protocol/src/lib.rs, crates/theseus-core/src/turn.rs, crates/theseus-core/src/config.rs, crates/theseus-discord/src/runtime.rs, crates/theseus-kernel/src/kernel.rs, and crates/theseus-core/src/tests_m3.rs. Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).

**The gate, before every commit:** `THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

The gate's shape phase fails a file over its line ceiling in scripts/long-files.txt. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-hands-network`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- No new dependencies: Cargo.lock and the package-lock.json files must not gain a package. If the right design needs one, say so in the report instead.
- Use invented names in fixtures, tests, and commits (AGENTS.md, Item 16). Write nothing about the owner, his machine, his accounts, or anyone else.
- Don't edit the spec, docs/status.md, the README, or docs/design/. The maintainer writes those at review. Where a doc should change, say what and where in the report.
- Don't commit half a step. If time runs out mid-step, leave it out and report what you found.

**The report:** when you are done, or at your task's deadline (by the clock, waits included), whichever comes first, write CLOUD_REPORT.md at the repository root. Commit it as the branch's last commit, subject `cloud report (not for main)`, and push. The maintainer reads it from the branch and drops that commit at the merge. For each step it says:
- what you found;
- what you changed (commit hashes);
- how you proved it: the commands and their results, with test counts, the runs under load, and each planted revert and what failed;
- the live check the maintainer should run, as exact commands, and what each should show;
- what is left or uncertain, and any design choice the owner should hear about.

Then the gate's result, naming each failing test and why. Your final message is short (under 1,200 characters): the line `CLOUD REPORT COMPLETE`, then the branch, its head commit, one line per step, and the gate's result.

**Planted reverts:** to prove a test guards a behaviour, plant the bug, show the test fail, then restore the file and `touch` it, so cargo rebuilds it (a restored file with its old mtime keeps the planted build). Run `git status` after every restore.

**Load:** where your task asks for runs under load, use AGENTS.md's recipe. Priority, not count, makes the load: run the test with `nice -n 19`, and beside it four busy loops at nice 0, each `sh -c 'while :; do :; done' &`. Kill the loops by the pids you started, never by a name pattern.

---
## Your task: AWS hands on an existing network, never a NAT of their own, step 40's network (theseus-mgw.9)

Branch: `cloud/20261004-hands-network`. Every commit's subject carries `theseus-mgw.9`. Deadline for the report: 5
hours after you start.

**Background.** Hands are jobs that run in AWS (the AWS design §3.3). Step 40's two parts, merged before you start,
built `aws.hands.run` on Lambda and Fargate, with its cancels, reservations and grid. A Fargate hand runs in the
private subnets of infra/aws/theseus-hands-network.yaml, which always makes a VPC of its own, with a NAT behind
`NatGateway` (about $36 a month while on). With the NAT off, a Fargate hand can't pull its image or reach the queue,
so launch.rs refuses it. The owner's account already holds another project's VPC, whose private subnets route out
through its own NAT gateway. The owner asked that hands use it: the hands stacks deploy into an existing VPC and its
subnets, chosen by config, route out through the NAT already there, and never create one. Theseus may use that
network, and must never change it.

**Read first:**
- docs/design/aws-toolset.md §2's first principle (the other project's resources), §3.3's network, §3.4, §3.6, and
  §3.7's reaper;
- infra/aws: README.md, theseus-hands-network.yaml, theseus-hands.yaml, check.sh, and test/rules.py;
- crates/theseus-core/src/aws: hands/launch.rs (`discover`, `HandsEnv`, the backend check), stack.rs, tend.rs (the
  budget's reconcile keeps a config value in a stack's parameter), inventory.rs (it lists by the `theseus:owner` tag);
  config/aws.rs (`AwsAccountConfig`);
- crates/theseus-aws-guard: guardrails.toml (its entry format, the `network` IaC group, `[boundary]`), lib.rs,
  policy.rs, and tests/.

**What changed since the design was written** (the code and AGENTS.md are the truth where the design differs):
- **The gap you close:** network writes are IaC-only for `aws.call`, but deletions stay direct, and a write is `notify`
  by default (`[policy.aws]`). Today a call could delete a route in a VPC Theseus never made, with only a notice.
- **Part 1 and part 2's names:** the `theseusd hand` role, `aws.hands.run`, the group's META record, `HandsEnv`, and
  the refusal "Fargate hands need the hands VPC's NAT, which is off". The code on main is the truth.
- **The repository is public.** Never name the other project, its VPC, its subnets, or the account, in code, tests,
  docs, or commits. In config and docs it is "an existing VPC". Use invented ids in fixtures.
- **The config is sparse:** every new key has a default and a template entry.

**What to build,** each a green commit:
1. **The template.** Parameters `ExistingVpcId` and `ExistingSubnetIds`, and an optional `ExistingSecurityGroupId`.
   With them, a condition skips every resource that belongs to the VPC itself: the VPC, the subnets, the route tables
   and their associations, the S3 endpoint, the internet gateway, the NAT and its address, and the flow logs and their
   role. The stack then makes only the hands' security group (no ingress, tagged `theseus:owner`), or nothing when a
   group is named. A template rule refuses `NatGateway=enabled` beside an existing VPC. The outputs keep their keys,
   and `NatGateway` reads `existing`. `infra/aws/check.sh` passes in both modes (`pip install cfn-lint` into a venv;
   its rules read botocore's models).
2. **The config.** `[aws.accounts.<id>]` names the existing network: the VPC, its subnets, and optionally a security
   group. By default it names none, and the stack makes its own VPC as today. The network stack's plan takes these
   values from the config, and a plan whose network parameters differ is refused with the reason; or follow tend.rs's
   reconcile. Say which.
3. **Discovery.** With `NatGateway = existing`, launch.rs reads each subnet's route table (`DescribeRouteTables`), and
   refuses a Fargate launch, naming the subnet, unless the table sends `0.0.0.0/0` to a NAT gateway. For an existing
   network, its words never suggest turning on a NAT.
4. **The guard: use, never change.** One guardrail entry, two enforcers, as the list's other entries are:
   - The gate asks at the floor before a direct call that changes network plumbing (VPCs, subnets, route tables and
     routes, NAT and internet gateways, network ACLs, endpoints, flow logs, security groups), and before a change set
     that touches one the stack does not own.
   - AWS denies the same to every Theseus identity, the deployer included, on any resource not tagged
     `theseus:owner`, in the generated session guards and the hands' boundary (mind its 6,144 characters).
   - Running a task in those subnets stays allowed: ECS's own role makes its network interfaces.
   - IAM authorizes `ec2:CreateSecurityGroup` against the VPC too, so a deny on untagged resources also stops a new
     group there. Decide which comes first, test it, and report it.
   - The inventory and the reaper keep listing by the tag, so the existing network is never theirs.

**Proof, offline:**
- check.sh in both modes, and the rule against `NatGateway=enabled` beside an existing VPC.
- Against part 1's stateful fake of AWS: a Fargate group in an existing network runs in the configured subnets with the
  stack's group, and a subnet with no route to a NAT is refused, with no `RunTask`.
- The guard's cases: `DeleteRoute`, `ReplaceRoute`, `DeleteNatGateway` and `ModifySubnetAttribute` ask at the floor; a
  change set that adds a route to a table the stack doesn't own asks; the generated policies deny these on an untagged
  resource and still allow `ecs:RunTask` (tests/policies.rs).
- Planted reverts: drop the condition that skips the NAT in existing mode, and show a template test fail; drop the
  tag condition from the deny, and show the policy test fail.

**The live check is the maintainer's, and it waits for the owner's go.** A Fargate hand costs a few cents, and its
image's pull goes through the existing NAT (about $0.045 a GB). Write it in the report as exact commands on a scratch
daemon with the account bound and the foundation and hands stacks applied. Name the account's config table, never its
id:
1. Put the existing network in the config, then plan `theseus-hands-network`: its change set adds the security group
   alone (or nothing), and no VPC, subnet, route, endpoint or NAT. (If the stack already exists with a VPC of its own,
   the plan removes those resources: say so.) Apply it with the owner's go.
2. A one-hand Fargate group runs in the existing subnets, its envelope arrives, and it settles once.
3. Ask the model to delete a route from that VPC's route table: it asks at the floor; decline.
4. `aws.inventory` lists the security group and none of the existing network.

**Questions for the report:** whether a Theseus security group inside another project's VPC counts as changing it
(build the tagged group with no ingress, or the config's existing one); flow logs (none on a VPC Theseus did not make);
and the S3 endpoint (none, so S3 traffic pays the existing NAT's data charge).

**Leave alone:** the hands' launch beyond discovery and its words; the poller, cancels, reservations and grid (part
2); step 16's restore in crates/theseus-core/src/aws/; the durability tender; 42a's `budget.list` and
`policy.explain`; `security.v1` at the gate; the cockpit.
