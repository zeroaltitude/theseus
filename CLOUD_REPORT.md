# CLOUD_REPORT: self-narrow (theseus-pw1q.15)

Branch `cloud/20261013-self-narrow`, based on main at `0711a5b` (the task commit over `0baeed6`; store format 33 at
the base). Built 06:41 to about 09:40 UTC on 2026-10-14. The report is the branch's last commit.

## What was asked, and what I found

Theseus owns its Home AWS account and narrows its hands with roles. The hand roles (`infra/aws/theseus-hands.yaml`)
are `theseus-hand-basic` (the hand basics alone), `-read` (basics + AWS's `ReadOnlyAccess`) and `-owner` (basics +
`theseus-allow-all`, which is `Allow * on *`), all under the `theseus-boundary` permissions boundary. ECS and Lambda
assume those roles themselves, so no session policy can narrow them: the role's own managed policies are what a
narrowing has to replace, through the hands stack. That decided the shape of "Apply".

Findings that shaped the build:

- **There is no "existing AWS read budget".** Nothing in the repository counts AWS reads against a budget. I added
  one cap for this work, `[self] narrow_read_pages` (default 400 pages of 50 events, about 3.5 minutes at
  CloudTrail's 2 requests a second), and the pass makes no model call at all, so it touches no model budget by
  construction. A window the cap cuts is "not complete" and proposes nothing.
- **CloudTrail's history cannot answer for data events** (S3 object reads and writes, Lambda invokes, DynamoDB items).
  `usage::DATA_EVENTS` names them. A policy's allow of one is **kept as it is and named "not visible"**, never
  counted unused, and the card says so.
- A role's policy for narrowing is "everything attached except `theseus-hand-basics`" (plus inline policies). The
  proof of "no wider" is against **all** the role's policies, the basics included.
- The `lookup`'s role is read from the record itself (`sessionIssuer.userName`, else the `assumed-role` ARN), since a
  hand's session name is its task id.

## What I built (commits)

| commit | step |
|---|---|
| `a94a642` | `aws/narrow/`: the pure half. `usage` (observed set, paged, data events named, denied calls apart), `glob`/`policy` (IAM policy documents and the containment proof), `build` (the candidate, the keep list, narrowed resources), `verify` (Access Analyzer + simulator behind a trait), `apply` (the template patch), `run` (one role's proposal start to end), `proposal` (card words, JSON diff), `live` (the account behind the traits, the stack's change set). |
| `6427c55` | `rsi/narrow.rs` and the wiring: the weekly pass under `rsi::gate`, the card, the owner's answers, the week's watch; `Act::SelfNarrow`; method `self.narrow`; `theseus self narrow …`; five ledger kinds and facts; config keys and template; protocol TypeScript; Discord courier kinds; docs in `crates/theseus-core/AGENTS.md` and `crates/theseus/AGENTS.md`. |
| `238b240` | `live`: every request the pass makes is checked against the embedded catalog's shapes with no network. |

### How it works

1. **Usage** (`aws::narrow::usage::observed`): `LookupEvents` pages (50 events, 520 ms apart, `NextToken`), events of
   one role kept apart from another's, `service:Action` from the event source and name (a small table maps the names
   that differ from their IAM action: `ListObjectsV2` is `s3:ListBucket`, and so on), ARNs from the record's
   `resources`. A call that failed with an access error is not a use; it is counted in `denied`. Fewer than 10 events
   in the window is "too few to say".
2. **Proposal** (`build`): observed actions on the ARNs their calls named when every call named some and they sit
   inside the statement that allowed the action today (collapsed to prefixes past 20, still inside), else on that
   statement's own resources and condition; the keep list `KEEP` (the hand's own `logs:CreateLogStream`,
   `logs:PutLogEvents`, `s3:PutObject`… and `sqs:SendMessage`, from `hands/hand.rs`) with the resources the role's
   basics give them; the data actions kept; the deny statements copied. `policy::wider` proves every (action, resource)
   of every candidate allow lies inside one current allow with the same condition (a `NotAction` statement is never
   read as covering). A candidate that is not smaller (`reach_after >= reach_before`, counting a wildcard by its
   services' operations) is not proposed.
3. **Checks** (`verify`, `run`): Access Analyzer `ValidatePolicy` (an ERROR or SECURITY_WARNING blocks; WARNING and
   SUGGESTION are listed on the card), `SimulateCustomPolicy` of every observed call against the candidate and the
   policies that stay (anything but `allowed`, or no answer, blocks), and the guard crate's `scan` of the patched
   hands template (a hit the unpatched template does not already have blocks).
4. **Card**: `self.narrow.proposed {role, actions_before, actions_after, evidence_days, analyzer_findings, proposal}`
   and a `self_narrow` post to the owner's DM carrying the diff in words (`theseus self narrow show ID` adds the JSON,
   the two documents). The Discord courier routes `self_narrow` and `self_narrow_red` as it routes `self_keel`.
   **Apply / Not now are CLI verbs** (`apply ID`, `decline ID`), named on the card, not buttons: a button would need a
   new `QuestionKind` in `work.answer` and the three surfaces' renderers, which are other rows' files. See the
   decision list below.
5. **Apply** (`apply`, `decline`, `revert`; `Act::SelfNarrow`, the owner from a private place, never a job's shell;
   a refusal is an `approval.refused` row): the deployed `theseus-hands` template is read, patched as text (the role's
   `ManagedPolicyArns` becomes the basics and one new `AWS::IAM::ManagedPolicy` named `<role>-narrow`; anchors
   checked, the result parsed and read back), saved to `<state>/self/narrow/<id>.before.yaml` (0600), and sent as a
   change set under the deployer role with every parameter `UsePreviousValue`. The change set is **dropped unrun**
   unless it touches only the role's and the new policy's logical ids **and** the guards' floor check on it is empty.
   Then it runs and the stack is waited on. **Revert** is the same path with the saved template.
6. **Watch**: for a week after an apply, the nightly pass reads the role's events since the apply; each new denied
   action is a `self.narrow.red` row and a `self_narrow_red` card naming it with `theseus self narrow revert ID`.
7. **Cadence and gate**: `Core::self_narrow_nightly` runs in the learning tender after the exams. `rsi::gate` first and
   again before each role and before each write; at most once in `narrow_every_days` (7) per role, and never a second
   proposal for a role while one waits for the owner. State: one META record `self.narrow` (no store format change).
   Mode off or the switch thrown: nothing is read, written or posted (tested).

New config keys (all with defaults, in the template, tested): `[self] narrow_every_days = 7`, `narrow_days = 14`,
`narrow_read_pages = 400`.

Defect fixed on the way: none found in code I touched; the `Act` match arms in `fact/answer.rs` needed the new
variant, nothing else.

## How I proved it

All counts are `cargo nextest run -p theseus-core -E 'test(/narrow/)'` unless stated: **42 tests, 42 passed** (the
build was cold; `CARGO_INCREMENTAL=0` after the disk filled once).

Tests, by what the task asked:

- `observed` over a fixture of trail events: pages and tokens read, two roles kept apart, the cap, the window's
  bounds, denied calls apart, data events named (`usage::tests`, 6 tests).
- Never wider than today, on any statement: `build::tests::property::the_candidate_is_never_wider_than_the_current_policy`
  (proptest: random current policies with conditions, random trails with 0 to 35 ARNs, with and without the basics;
  also asserts the keep list is present wherever the role has it), plus hand-written cases and `policy::tests`.
- The keep list is always present: `build::tests::the_candidate_allows_the_observed_actions_the_keep_list_and_the_guards`
  and the property above.
- A candidate that would deny an observed call is not proposed: `verify::tests`, `run::tests::an_analyzer_error_or_a_denying_simulator_blocks_the_proposal`.
- Access Analyzer errors block, suggestions list (a fake client): `verify::tests::an_error_or_a_security_warning_blocks…`.
- The card's diff text: `proposal::tests::the_words_say_what_goes_what_stays_and_how_to_answer`.
- Mode off or halted proposes nothing: `rsi::tests_narrow::with_the_mode_off_or_the_switch_thrown_nothing_is_read_written_or_posted`.
- Through the core: one proposal, one row, one card, not asked again (`…a_role_with_evidence_gets_one_proposal…`);
  not now, then again after a week; apply plans the patched template and records it, a revert applies the template
  from before; a stack that refuses leaves the proposal open and writes nothing; a job's shell cannot answer and the
  refusal is ledgered; the watch tells a denied call once, ignores one before the apply, and ends after the week;
  a `--dry` look reads and prints and keeps and posts nothing, even with the mode off.
- The patch against the repo's own `infra/aws/theseus-hands.yaml` for each profile (at most two lines of the template
  change; allow-all leaves the owner role; the guard scan finds nothing new); another template shape is refused.
- Every request the pass makes is accepted by the catalog's shapes (`live::tests`).
- Config: `config::rsi::tests::the_narrowing_keys_have_defaults_and_are_in_the_template`; the full
  `config::`, `rsi::`, `fact::`, `tests_registry`, `tests_output`, `tests_layouts` filter: 172 passed.
- theseus-protocol 87 passed (the TypeScript is regenerated and staged); theseus and theseus-discord 586 passed.

### Planted reverts (bug planted, test failed, file restored with `git checkout` and `touch`)

| planted bug | what failed |
|---|---|
| a resource widened to `*` (`build::resources_for`) | `build::tests::{many_arns_collapse…, arns_outside_the_current_statement…, the_candidate_allows_the_observed_actions…, property::the_candidate_is_never_wider…}` (4) |
| the simulation skipped (`verify`) | `verify::tests::{a_candidate_that_would_deny…, a_simulator_that_answers_nothing…}`, `run::tests::an_analyzer_error_or_a_denying_simulator_blocks…` (3) |
| Access Analyzer errors do not block | `verify::tests::an_error_or_a_security_warning_blocks…`, `run::tests::an_analyzer_error_or_a_denying_simulator…` (2) |
| the keep list dropped | `build::tests::the_candidate_allows…`, the property test (2) |
| data events assumed unused | `build::tests::the_candidate_allows…` (1) |
| denied calls counted as uses | `usage::tests::a_denied_call_is_not_a_use…` (1) |
| a cut window accepted | `run::tests::too_few_events_a_cut_window…` (1) |
| the self gate ignored | `rsi::tests_narrow::with_the_mode_off_or_the_switch_thrown…` (1) |
| an open card does not hold the role | `rsi::tests_narrow::a_role_with_evidence_gets_one_proposal…` (1) |
| a wrong input member name in the Access Analyzer request (`policyDoc`) | `live::tests::every_request_the_narrowing_makes…` (1) |

Not caught by a test of its own, and said so: the catalog check does not validate enum values (`policyType:
"IDENTITY"` passed it).

### A scratch daemon, real binaries

`theseusd --config … --socket … --state-dir …` on a fresh state dir (mode `act`, no AWS account, the vault stubbed),
then the real `theseus`: `self narrow` prints "No narrowing proposals yet."; `show abcd1234` and `decline abcd1234`
refuse with "no narrowing proposal …" (code -32602); `run` says it is halted until the owner's first resume (the
switch); `run --dry` says no AWS account is bound; with `THESEUS_SESSION` set the CLI refuses `decline` before sending
anything; `self log` shows the state. I could not take the live check further here: no AWS account, no key.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 CARGO_INCREMENTAL=0 scripts/gate.sh`, run on the tree of `6427c55` plus
the unstaged edits that became `238b240` (the last commit changed `live.rs` and one help string; I re-ran clippy
`-D warnings` on theseus-core and theseus, `cargo fmt --check` and the 42 narrow tests after it, all clean).
Phases: door 1 s, keel 2, keel tests 0, fmt 13, shape 6, features 2, clippy 180, cockpit 38, test build 416, reader
rule 3 (passed), **suite 346 s: 4,512 run, 4,478 passed, 34 failed**.

The 34:

- **33 are the known L1 failures** (root VM, no job cgroup, theseus-pv6i): theseus-sandbox `contract` (19),
  `bench spawn_100` (1), theseusd `sandbox` (13).
- **1 is `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`**, which is
  not on the known list. It fails alone, repeatedly, with a 21 s wait; it also fails on the unmodified base:
  I reverted my one `courier.rs` change and ran it 10 times: **3 failed, 7 passed**; with my change, 2 of 6 failed. So
  it is a timing test that is flaky on this VM at the base (the theseus-wpv2g family), not mine. Name it in the list.

Run by hand, the phases after the suite (`machine_checks`): protocol types clean (the TypeScript is staged);
`cargo deny --offline check`: advisories, bans, licenses, sources ok; the turn bench
(`theseus-sim bench turn --check --runs 5 --burst 0`): plain 4 frames (budget 4), tool 8 (8), batch 14 (14), syncs 2 (2),
first request 38.6 KB (40), all ok, **unchanged by this work** (nothing in a turn changed). **The lifecycle bench
could not run here**: `theseus-sim bench lifecycle --runs 10 --check` stops with "the turn left no job running", the
bench's job not starting on this root VM (the same L1/cgroup condition), before any budget is measured. I did not
touch the start path: the only code that runs at start from this work is nothing; the pass runs inside the learning
tender's night, after `rsi::gate`. The maintainer should run it on the owner's machine.

### Keel findings expected

None. `python3 scripts/keel-guard.py` before each commit: `keel: ok (a branch: merge-base(HEAD, origin/main)..HEAD,
9fa84735c0; 430 files changed, 0 findings acked)`. It lists one informational line about
`crates/theseus-store/src/store/tests.rs` (a base commit's rename and rewrite), not from this branch. No test was
deleted, no `#[allow]`/`#[expect]` added, no cap, ceiling or budget touched. Line ceilings: the only long file
in `scripts/long-files.txt` I touched is `theseus-protocol/src/lib.rs` (+2 lines: 2,666 of 2,733); every other file I
added to is far under 2,500 (largest new file `rsi/narrow.rs`, 639 lines).

## The live check for the maintainer (exact commands)

On the owner's machine, with the build installed and `[self]` set the owner's way. **The read-only part needs no
mode change and changes nothing in AWS**: `--dry` reads (CloudTrail, IAM, Access Analyzer, the stack's template) and
prints. It uses the account's key path as every `aws.*` read does (the work session when `owner_role` is set).

```sh
theseus self narrow run --dry --profile owner --json | jq -r .text
theseus self narrow run --dry --profile read
```

Each should show, per role: either `would propose:` followed by the card text (evidence days and event count, "about N
actions allowed today, about M after", what is replaced, "Allowed on evidence" by service, the keep list, the data
events "kept … because CloudTrail's history cannot show them", the checks line, the `apply` and `decline` commands), or
`no proposal:` with the reason (too few events, a cut window, nothing wide, already as narrow as its calls, Access
Analyzer's error, the simulator's denial, or the template cannot take it). Things to look at: that `Allowed on
evidence` matches what you know the hands do; that the actions Access Analyzer listed are suggestions only; and that
the "Replaced:" line names the policies you expect (`theseus-allow-all` for owner, `ReadOnlyAccess` for read).
The `aws.trail` tool can spot-check any action against the window.

The real proposal and card (needs `[self] mode = "act"` and the owner's `theseus self resume`):

```sh
theseus self narrow run --profile owner     # forces the pass for one role; posts the card to the owner's DM
theseus self narrow                         # lists it, with its id
theseus self narrow show <id>               # the evidence, the change in words and as JSON
```

**The apply only after the owner's yes** (it changes IAM through `theseus-hands`):

```sh
theseus self narrow apply <id>      # plans a change set, drops it unrun unless it touches only
                                    # HandOwnerRole and HandOwnerNarrowPolicy, runs it, waits
theseus self narrow decline <id>    # not now
theseus self narrow revert <id>     # after an apply: the template from before, through a change set
```

After an apply, `aws.stack.status theseus-hands` should show `UPDATE_COMPLETE`, `theseus self log` the
`self.narrow.applied` row, and the role in the console should list the basics and `theseus-hand-owner-narrow`. The
measurement (rsi-plan): a smaller policy (the card's before/after) and no legitimate call denied over a week: the
nightly watch writes `self.narrow.red` and a card if one is.

## What is left, uncertain, or a decision for the owner

- **IAM's XML answers are not tested against real AWS.** `live.rs` reads `AttachedPolicies`, `Policy.DefaultVersionId`,
  `PolicyVersion.Document` (a string, percent-decoded if IAM sends it encoded; an object works too), `PolicyNames`,
  `PolicyDocument`, `EvaluationResults[]`, and Access Analyzer's `findings[]` as the catalog's output shapes name
  them. The requests are checked against the catalog; the decoding of the responses is checked only through the fake
  trait. The `--dry` live check above is what proves it. If a response shape differs, the pass says `the pass failed:`
  with the AWS call named, and proposes nothing.
- **Apply / Not now as CLI verbs, not card buttons** (decision to hear): the card names the commands. Buttons want a
  `QuestionKind` and the three surfaces' renderers; `work-answer` (theseus-ds5os) is the place. The card text is ready
  for it.
- **The apply patches the deployed hands template in text** rather than building a second stack. Tied to the
  template's shape (`ManagedPolicyArns` with `!Ref HandBasicsPolicy`); a template of another shape is refused with the
  reason and the pass proposes nothing for it. A tidier design is a hands template that takes a "narrow policy"
  parameter; that would change `infra/aws`, so I left it.
- **A narrowed role keeps the data actions it is allowed today** (S3 objects, Lambda invoke, DynamoDB items), because
  the trail cannot show them. For `-owner` that is `Allow … on *`, so the first narrowing mostly removes management
  actions. Turning on CloudTrail data events for the hands' prefix would let a later row narrow those too.
- `ReadOnlyAccess` is large; a candidate for `-read` is built the same way and may be accepted or refused on size
  (6,144 characters) by the patch; the reason is said.
- A pass that proposes nothing logs one line (`tracing`) and, with `run`, prints it; it writes no ledger row. I did
  not add a sixth kind for "looked and found nothing"; `last` per role is in `self.narrow`. Say if you want one.
- Only the account's first bound account is used (`aws.account(None)`), as the other tenders do.
- Docs the maintainer should change: spec Part III item for pw1q.15; `docs/status.md`; `docs/design/rsi-plan.md` §3
  (wave 2's rung is built); the roadmap row. I changed none of them.
- Disk: the VM's allowance filled twice from `target/debug/incremental` (20 GB); I deleted it and built with
  `CARGO_INCREMENTAL=0`. Nothing outside the repository and /tmp was touched.
