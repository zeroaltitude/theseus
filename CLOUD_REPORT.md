# CLOUD_REPORT: earned-autonomy, the weekly loosening card from the ledger (theseus-uttu)

Branch `cloud/20261011-earned-autonomy`, from main at fdd14925 (store format 27, **unchanged**: a grant is one new META
key, `policy.earned`, as `mcp.tools.<server>` was, so it owes no bump and no `tests_layouts` sample).

## What I found

- Every approval and decline is already in the ledger, but not in one row: the *question* is `tool.confirm_requested`
  (a `ConfirmRequest`: tool, input, reason, floor, external_text), the *yes* is `action.confirmed` (correlation id, by),
  the *no* is `action.declined`, and "should have asked" is `policy.tightened` with a `correlation_id`. The finder joins
  them by correlation id. `action.declined` has no argv, so a decline is placed through its question's row.
- A notified call keeps no proposal in its action (`plan_confirm` alone keeps one), so a "should have asked" press on a
  notice cannot read the call's argv from the action. The notice's own ledger row (`tool.notified`) names the grant in
  its `setting` (`earned grant ern_…`), so the press reads that.
- The gate's `decide_with` could not say whether a posture alone had asked (vs. the floor, the approve lists, the
  roots, AWS rules), and a grant must loosen only the posture. I split its pre-posture answers into `preempted` (a pure
  extraction, same order, every existing test unchanged) and added `ToolPolicy::posture_only`.
- `[policy] enforcement` is `notify` in the shipped template, so a grant matters to a config that sets `proc.run` (or the
  file tools) to `approve`, as the owner's does; the daemon test and the core tests run under `approve`.

## What I changed (commits, oldest first)

| commit | what |
|---|---|
| `781553e` | config: the retired-approval test names the section, not the word. **A test changes its meaning**, so it is a commit of its own (see "Keel findings expected"). |
| `1b9e290` | The feature: protocol types, the finder, the grant and its gate step, the methods and the card's tender, the number, the CLI, `[policy.earned]`, health's `earned`, the daemon's test, the AGENTS.md notes. |
| `dc68f30` | The card in the owner's DM with its buttons (theseus-discord). |
| `04467eb` | A cadence test the planted revert showed was missing, and MCP connections' refusals for the two new methods. |

### The design, in the order of the task

1. **The finder** (`crates/theseus-core/src/policy/earned/find.rs`, pure over ledger rows). It joins `tool.confirm_requested`
   (the question: tool, input, reason, floor, external text), `action.confirmed`, `action.declined` and `policy.tightened`
   (with a call) by correlation id. `proc.run` approvals go into a **trie of argv words** (the program by its base name); a
   node is a candidate when the approvals that reach it and no narrower candidate hold are at least `min_approvals`. So
   `cargo nextest run -p a` ×30 and `-p b` ×27 are two candidates; 57 `git commit -m <each a different message>` are one,
   `git commit -m`, shown as "(and what follows it)". Never shorter than the program plus one argument. A group is
   dropped when **any** call under its prefix in the window was declined, pressed "should have asked" on, or reached the
   floor. Only an approval asked by *the tool's posture alone* counts (`by_posture`: the reason ends `approve (enforcement =`
   or a `[policy.*]` line): one the approve list, the roots, the floor, a tightening or the external-text hold asked for says
   nothing about the posture a grant changes. Calls with an `env` or `steps` are no evidence. File tools (`fs.*`) group by
   tool and workspace root. Each candidate carries count, first/last time, sessions, three newest samples, and a digest of
   its calls' correlation ids.
2. **The card**: `Core::earned_propose`, from a tender after serving (`earned_after_serving`: once a day at `[judge]
   learning_hour`, a missed day once, never within 10 min of a start, its work on a nice-19 thread; no ledger read until
   `every_days` have passed). One frame: the open card and `last_card_ms` in META `policy.earned`, the
   `policy.earned_proposed` row, and the DM post (`stage_to_operator`, kind `earned_card`, which only the owner's DM
   courier takes). At most 5 candidates. "Not this one" is remembered 90 days (`policy.earned_passed`). Discord: Yes, with a
   notice · Not this one · Show the record per candidate (`runtime/earned.rs`); the CLI has the same three
   (`theseus policy earned accept|pass|show ID`).
3. **Accepting** is `judge_act(Act::Earned)`, the place rule's `owner_in_private`, as an untighten. The grant is one
   record in META `policy.earned`, written with its `policy.earned_granted` row (the record's digest and counts). The gate
   step `earned::gate` (`toolrun/order.rs`, a new `Layer::Earned` after the call's own policy and before the broker's)
   turns an approve into a **notify** (never open) when: the tool's posture alone asked (`ToolPolicy::posture_only`), the
   place is private, the call has no `env`, no word after the prefix names a URL, every path argument is inside the roots,
   the tool is not tightened, and a grant covers it. Every later layer (the broker's secret posture, the place's floor, the
   external-text hold, an MCP client's floor) still raises it. The notice's setting is `earned grant <id>` and its rule
   says `theseus policy revoke <id>` ends it. `theseus policy` lists the grants ("earned: 57 approvals, Oct 1 to Oct 8")
   from health's new `earned`. `theseus policy revoke <id>` (anyone's: it tightens) ends one, and the group is not proposed
   for 90 days. **"Should have asked" on a grant's notice** (`tighten_by_earned`; the notified call keeps no proposal, so
   the notice's own `tool.notified` row names the grant) ends the grant, tightens nothing, and the group is never
   proposed again (a decline "from then on").
4. **The number** (`policy/earned/measure.rs`, `theseus policy earned`): interrupts (questions a person was asked) per
   active hour (a clock hour with a turn or a question) over the last four weeks, a line per week, and how many of those
   questions the current grants would have saved, with the rate had they held ("47 % fewer").

Out of scope, with the hook noted in `crates/theseus-core/AGENTS.md` and `policy/earned.rs`'s doc: M5's Jev decision on
the approval ladder (a judgment before the card), modes, and `scoped-grants` (a grant with a single call's scope); both
would sit beside `earned::gate`.

### Choices the owner should hear about

- **`window_days` is a fourth key** under `[policy.earned]` beside `enabled`, `every_days`, `min_approvals` (default 7).
- **"Since the last card"** is not a second window: a candidate is either a grant, passed on (90 days), or on the open
  card (held for a week, then it may ride the next card), so nothing is proposed twice.
- **A card needs a DM bound to the owner** to post on its own; `--propose-now` posts regardless (it lands in the outbox).
- **A grant never beats a tightening** of the same tool: "should have asked" on `proc.run` itself still makes every
  `proc.run` ask, grants or not.
- **No store format bump**: a new META key is how `mcp.tools.<server>` and `self.switch` were added.
- **`policy.explain` shows no earned row** (the layer returns none, as a language server's does without one): a grant is
  per call prefix, not per tool.
- **Surfaces I did not build**: no cockpit view of the card or the number (the types are generated for it).
- **The Discord card is not edited after an answer**: the presser gets an ephemeral reply, and a second press answers
  "already" (the core's state is the record). Editing the card means the core posting a settle for it; left out.
- `ToolPolicy::decide_with` was split (`view`, `preempted`, `posture_only`): behaviour-preserving, every existing policy
  test unchanged. Two doc comments in `theseus-discord/src/render.rs` were tightened so its new `Buttons::Earned` fits its
  line ceiling (3009).

## Security review

**What an attacker who can make the owner approve 20 times gains.** The attacker is injected text (a page, an issue, an
MCP result) steering the model to propose 20 harmless-looking commands that share a prefix over a week, each approved
with no refusal. What they gain is the *chance* that the prefix runs later **with a notice instead of a question**:

- It is never silent and never theirs to take: the grant needs the owner's yes from a private place (`judge_act`:
  the CLI inside a job is refused by the CLI and again by the daemon, a copy of the card's buttons in a shared channel is
  refused and ledgered, an MCP connection is refused and ledgered), after a card that names the prefix, says when it takes
  "what follows it", and offers "Show the record" (count, dates, sessions, three calls).
- A single decline, a single "should have asked", or one call under the prefix that reached the floor in the window rules
  the group out; so the attacker needs 20 approvals with **no** refusal.
- The grant never loosens past what the owner's own posture was: `notify`, never `open`. It applies only where the tool's
  posture alone asked: not to the floor, the approve lists, a path or working directory outside the roots, a private
  address, an AWS guard, or a layer-1 change (`posture_only`); not in a shared place; not to a call with an `env`; not to
  a call whose words after the prefix name a URL (a grant on `curl -s` or `git clone` is no exit for what is under the
  roots); not when a path argument is outside the roots; not for a tool someone tightened. And every later layer still
  raises it: the external-text hold (a session that read web text still waits, so the injection that earned the grant
  cannot use it in the session it came from without the owner trusting it), an MCP client's floor, a place's ceiling, a
  broker grant's own posture.
- Every use posts a notice naming the grant, with a "should have asked" button that ends it for good and
  `theseus policy revoke <id>`.
- **Residual**: a prefix with a free tail (`git commit -m <anything>`, `cargo run -p x --`) lets the owner's approved
  program run with other arguments, with a notice; `cargo test`, `make` and the like already run repository code at every
  approval, so a grant on them extends nothing new but the absence of the question. A file-tool grant ("fs.write under
  /w") covers every write under that root that is not on the floor or the approve list (a `.git/hooks` file included).
  The ledger's rows are the finder's evidence, and a process that can write the store can forge them, as it can forge
  a tightening: the digest is a light check, the same standing as the config copy's (AGENTS.md).

**Why the prefix rule stops `bash -c`.** Three independent checks, none of which parses what a command does. (1) The
finder never proposes a prefix whose program is a shell or a program that runs what its arguments give it (`sh`, `bash`,
`env`, `xargs`, `sudo`, `ssh`, `timeout`, `find`, `awk`, `sed`, …), or whose words hold `-c`, `-e`, `--eval`, `-exec`;
`bash -c` is a prefix of the program and one argument whose argument is code, so it fails on both, and an interpreter
(`python3`, `perl`, `node`) must name a script file before any option. (2) The gate asks `prefix_ok` again of **every
stored grant** when it covers a call, so a grant written by hand into the store (the test does exactly that, for `bash
-c`) covers nothing. (3) The argv a grant matches is the typed argv `proc.run` runs with no shell, so `bash -c "x"`
cannot hide inside another prefix: a grant on `echo hello` covers `echo hello …` and nothing that starts a shell.

## How I proved it

All commands from the repository root on this VM, `TZ=America/Phoenix`. Counts are from the runs, not estimates.

- **New tests** (all pass): finder, 7 tests on hand-made ledgers (`policy/earned/tests_find.rs`: 20 approvals → candidate,
  19 → none, one decline → none, one "should have asked" → none, a floor call → none, `bash -c` and other shells and
  launchers never, the stable prefix chosen (30 + 27 + 57 distinct messages), file tools by root, a program by its base
  name, only a posture's ask counts, an `env` or `steps` is no evidence, a group already open or passed on is skipped);
  the number, the coverage and the storage, 4 (`tests_measure.rs`: the number by week, a prefix and a stored shell, a
  grant in one frame and read back after a reopen, an open card for a week); whole cores, 10 (`tests_m3/earned.rs`: the
  finder reads the record and one card a week; a second card waits for the week; a yes from a shared place refused and one
  from the CLI grants; a granted prefix runs with a notice, a non-matching argv asks, a reopen keeps it; "not this one"
  remembered and a revoke asks again; "should have asked" ends the grant and tightens nothing; the gate's conditions
  twice; a stored shell, the floor and a shared place; MCP refused); the config section (1); Discord, 3 (the card's
  buttons and presses, and `tests_gateway/earned.rs`: the card in the owner's DM, a forged yes in `#lab` refused, the
  owner's yes in her DM grants, the record shown to the presser alone); and **the real `theseusd` and `theseus`**
  (`crates/theseusd/tests/earned.rs`: three approvals from the CLI, the forced card, a job's own accept refused by the
  CLI, the owner's accept, a run with one notice and no question, another command asks, the number, a revoke, asks again).
- **Suite** (`THESEUS_KEEL_BASE=ef5c51e CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`,
  below): 3,769 tests, 33 failed, all of them the known L1 ones (theseus-sandbox's contract tests and `spawn_100`, theseusd's
  `sandbox` tests: the VM runs as root). No timing test failed.
- **Planted reverts**, each on the committed tree, the guarding tests run, the file restored with `git checkout` and
  touched (`git status` clean after each). Every one failed as the bug would:

| plant | caught by |
|---|---|
| the threshold off by one | `tests_find::twenty_approvals_are_a_candidate_and_nineteen_none` |
| a decline no longer rules a group out | `tests_find::a_decline_a_press_or_a_floor_call_under_the_prefix_rules_it_out` |
| a floor call no longer rules a group out | the same test |
| a listed ask counts as evidence | `tests_find::only_a_posture_ask_without_env_or_steps_counts` |
| a shell may be a prefix (denylist emptied) | `tests_find::a_shell_or_a_launcher_is_never_a_prefix` (`-c` is a second guard: the core test of a stored `bash -c` still passed) |
| a grant acts in a shared place | `tests_m3::earned::the_gate_applies_a_grant_in_a_private_place_to_a_covered_call_alone` |
| a grant passes what the lists or the floor asked | `…::the_gate_never_lets_a_grant_pass_the_lists_the_floor_or_a_tightening` |
| a grant covers a call with an `env` | `…::the_gate_applies_a_grant_in_a_private_place…` |
| a grant beats a tightening | `…::the_gate_never_lets_a_grant_pass…` |
| a URL after the prefix is covered | `tests_measure::a_prefix_covers_what_follows_it…` and the gate test |
| a yes from a shared place counts | `…::a_yes_from_a_shared_place_does_not_count_and_one_from_the_cli_grants` |
| "should have asked" tightens instead of ending the grant | `…::should_have_asked_on_a_grants_notice_ends_it_and_the_tool_keeps_its_posture` |
| a revoke keeps the grant | `…::not_this_one_is_remembered_and_a_revoke_asks_again` and the should-have-asked test |
| a restart loses the grants | `tests_measure::a_grant_is_stored_with_its_row…` and `…::a_granted_prefix_runs_with_a_notice_and_a_restart_keeps_the_grant` |
| a group passed on is proposed again | `…::not_this_one_is_remembered…` and the should-have-asked test |
| more than one card a week | **not caught at first** (the open candidate held the second card back, not the week); `04467eb` adds `a_second_card_waits_for_the_week_even_with_a_new_candidate`, which then failed under the plant |
| an MCP connection may answer or revoke | `…::an_mcp_connection_cannot_answer_a_card_or_revoke_a_grant` (first plant did not compile: the array's length; replanted with it) |

  I did not plant reverts under load: nothing here is a race or a timing assertion (they read and write a store and a fake provider), so the recipe's load adds nothing.

## Live check for the maintainer

The same scenario as `crates/theseusd/tests/earned.rs` (which passes here with a stand-in model), with the owner's keys:

```sh
S=$(mktemp -d); theseusd example-config > $S/theseus.toml
# edit $S/theseus.toml: [policy] enforcement = "approve"   and   [policy.earned] min_approvals = 3
theseusd --config $S/theseus.toml --state-dir $S/state --socket $S/sock &
T="theseus --socket $S/sock"
for w in one two three; do
  $T ask "Run echo hello $w with proc_run"   # exits 6: the call waits for you
  $T confirm                                 # lists it, with its id (act_…)
  $T confirm --approve <ACTION_ID>           # the owner's yes; three times in all
done
$T policy earned --propose-now     # "Posted a card to your DM": echo hello, earned: 3 approvals, Oct N to Oct N; prints ern_xxxxxxxx
$T policy earned show ern_xxxxxxxx # the record: 3 approvals, 1 session(s), three samples
$T policy earned accept ern_xxxxxxxx   # "echo hello now runs with a notice instead of asking"
$T ask "Run echo hello four with proc_run"   # no question; $T ledger -k tool.notified | tail -1 : setting "earned grant ern_xxxxxxxx"
$T ask "Run echo goodbye with proc_run"      # still asks
$T policy                          # an "earned grant: ern_xxxxxxxx  echo hello …  (earned: 3 approvals, …)" line
$T policy earned                   # interrupts per active hour, the grant's saving
$T policy revoke ern_xxxxxxxx      # "asks first again"; the next `echo hello five` asks
$T shutdown
```

With a Discord DM bound, the card also arrives there with the three buttons per candidate; a press from `#lab` (shared)
answers "🔐 Your answer did not count".

## What is left or uncertain

- **The proof's target (down 60 %)** needs weeks of the owner's own record: `theseus policy earned` is the measure, and
  prints the rate had the grants held. Until then the number says what the grants *would have* saved over four weeks.
- **A candidate's argv is read from the question's row**; a question older than the window's two-day look-back whose
  answer falls inside it is read, one older than that is not. A decline row has no argv of its own.
- **The finder reads at most 400,000 rows of a kind** (the number says when it cut).
- **No cockpit surface**, no `policy.explain` row, no card edit after an answer (all above).

## Docs the maintainer should change (I did not touch them)

- The spec: a Part III item for theseus-uttu (what it built, how it was proven, where it diverged: the trie finder,
  the `posture_only` split, the URL rule, grants never beating a tightening, MCP's JUDGED), §3.9's "Notices and records"
  (a notice may carry `earned grant <id>`), and the version line.
- `docs/status.md`: "Updated", the recently landed step, and the roadmap's R18 row.
- `docs/design/`: the killer-features plan §4.8's row R18 (done: the card, the grant, the number).

## Gate

`THESEUS_KEEL_BASE=ef5c51e CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, on the head
before this report (`04467eb`):

| phase | result |
|---|---|
| door, keel, keel tests | ok: 0 findings in `ef5c51e..the working tree` (see below for why the base is set) |
| fmt, shape, features, clippy (`-D warnings`), cockpit (lint, test, build), test build | ok |
| reader rule (`tests_registry`) | ok, 15 tests |
| suite | 3,771 tests: **3,738 passed, 33 failed**, 44 skipped. The 33 are the known L1 failures and nothing else: 20 in theseus-sandbox (`contract` ×19 and `bench::spawn_100`) and 13 in `theseusd::sandbox`; a root daemon's L1 job with no cgroup is refused (theseus-pv6i). No timing test failed, no negative assertion failed, nothing retried. |
| protocol types, deny | run by hand after the suite (as the task allows): ok (`git diff` of `cockpit/src/protocol.gen` empty after `git add`; `cargo deny --offline check`: advisories ok, bans ok, licenses ok, sources ok) |
| lifecycle, jobs, turn benches | skipped by `THESEUS_GATE_NO_BENCH=1`, as for a lane; no startup work was added (the card's tender sleeps until its hour, 10 minutes after a start at the earliest) |

Earlier runs, for the record: the first gate stopped in the keel on the five findings below that are not this branch's;
with the base set to my own start it passed the keel; the second hit "No space left on device" in the test build (this VM's
allowance is about 40 GB and `target/debug/incremental` alone is 7 GB: I deleted it and the stale workspace artifacts and
used `CARGO_INCREMENTAL=0`); the third failed clippy on a signature I had changed in a test (mine, fixed); the fourth is
the run in the table (suite failures only the 33 known ones), and the one above is the same on the final code. Before the
gate, a `cargo nextest` of theseus-core, theseus, theseus-discord and theseus-protocol found the two real conflicts: a
`config::tests` assertion that the config written back holds no "approval" at all, which `min_approvals` breaks (fixed in
`781553e`, a test that changes meaning), and the golden of `theseus policy`, which pins its requests, so an extra
`policy.earned` request would have changed it. I put the grants in health's `earned` instead (no extra request), so no golden
changed.

## Keel findings expected

None from this branch's code. `python3 scripts/keel-guard.py` against my own base (`THESEUS_KEEL_BASE=ef5c51e`):
`keel: ok (… 55 files changed, 0 findings acked)`. Two things to know:

- **A test changes its meaning** (`781553e`, its own commit): `config::tests::an_approval_section_is_retired_and_loads_with_a_warning`
  asserted `!shown.contains("approval")` on the config written back, which `[policy.earned] min_approvals` (the task's
  own key) now breaks. It asserts `!shown.contains("[approval")` and no line starting `approval`: the retired table and key,
  which is what it meant. Same number of assertions, same three cases. The guard did not flag it.
- With the default base (`origin/main`, which in this clone is `a9ad950fec`, behind `fdd14925`) the guard lists five
  findings that are **not this branch's**: `crates/theseus-tui/src/tests.rs` (an assertion removed), `crates/theseus-index/src/vectors.rs`
  and `crates/theseus-protocol/src/tests_work_join.rs` (an `#[expect]`/`#[allow]` added), and `scripts/long-files.txt`
  (the `lib.rs` ceiling 2726 → 2733, and `golden.rs` 2506), from commits between `a9ad950` and `fdd14925` whose signed acks do
  not verify in this clone. I added no `#[allow]` or `#[expect]` (the first drafts of two long tests had `#[expect(clippy::too_many_lines)]`;
  I split them instead), raised no ceiling (`theseus-protocol/src/lib.rs` is 2,725 lines of its 2,733; `theseus-discord/src/render.rs`
  is exactly its ceiling, 3,009, after two doc comments were shortened), and deleted no test.
