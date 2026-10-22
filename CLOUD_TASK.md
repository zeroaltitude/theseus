<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate or directory you touch (cockpit/ has its own), scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

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
- About 33 L1 tests fail here: theseus-sandbox's contract tests, its bench's `spawn_100`, and theseusd's sandbox tests. The VM runs as root, and L1 refuses a root daemon's job that has no job cgroup (theseus-pv6i).
- theseus-core's `tests_output::the_cores_output_matches_its_golden` fails under this VM's UTC clock, because two wake lines carry the offset's sign (theseus-ig6n). The gate line below sets `TZ=America/Phoenix` for it; set the same when you run the suite yourself, and commit only the golden lines your change moves.
- Under load, `theseus-store tests_pages::a_filtered_page_equals_the_scans_answer` can pass nextest's 120 s kill (theseus-hohs), and `theseus-core term::tests::python3s_repl_computes_on_the_screen` can fail (theseus-1n2y). Neither is on the flaky list: rerun it alone, and name it in the report.

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine. Any other failure is yours to explain.

**Other changes in flight.** About fifteen other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

These are reviewed, and merging into `main` one at a time tonight. Your clone may hold some of them already:
- route.v1 on the turn path (`[routing]`, detours and switches; store format 15);
- the live rerank (rerank's own breaker, a bounded live wait, per-item grading);
- security.v3's live notices and their brake;
- replay, audit and backfill of judgments;
- the ladder (`pack.mode`, `mode_for`, `ask_mode`, `pack_arm`, `theseus packs`, the cockpit's Ladder panel);
- the tools smalls (categorize on an empty ontology, the LSP's `start_on_edit`, AWS hands' runaway mode);
- the tasks smalls (layer 1 for the owner's tasks only, `task.change_expired`, place warnings, a check's restricted view; store format 16).

These are other cloud sessions like you, each on its own branch:
- consolidation, `Synthesis` nodes and the `+synthesis` arm;
- FSRS-6 retention and the `+retention` arm;
- activation's adjacency and the `+activation` arm;
- tiering: stubs and the bounded heat cache;
- situations, the precedence line and testimony;
- claim leases, the task board, `/tasks` and the cockpit's task graph;
- the cockpit's Budgets, Ledger and Policy tabs;
- the learning loop, if it launches beside them.

On the owner's machine:
- gliding with the place rule (`channel.post` and `channel.read`; places.rs, rpc/publish.rs, approval.rs, outbox.rs);
- the cockpit's Ship view (a gentle roll);
- the Linux lanes: spawn without fork and a light job cgroup (the job launch path), one sync per job completion, background work that yields, LSP watching, then socket activation;
- a benchmark run (bench/).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (main is at 15 since route, and 16 after the tasks smalls), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,696) and crates/theseus-core/src/compiler.rs (2,547); near it, crates/theseus-discord/src/runtime.rs, crates/theseus-discord/src/render.rs, crates/theseus/src/render.rs, crates/theseus-kernel/src/kernel.rs, and crates/theseus-core/src/turn.rs. Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

The gate's shape phase fails a file over its line ceiling in scripts/long-files.txt. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail5-effort`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of `main` for this wave (it wins over anything below about `main`)

Your branch starts from `main` at `f1fc9b03` (2026-10-22: release stage 3, which landed `integ/tail3` and the threads
lane's round 4 on top of stage 2's `8260dbab`); that is your base. Line numbers quoted below are at `f1fc9b03` but may
still be off by a few: search by name.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 50 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows may bump at once; the maintainer renumbers at the join, so never write the number as a
literal anywhere but the constant and its tests. Prefer serde-default fields and META keys: most rows need no bump.

**What landed since `8260dbab`** (`git log --first-parent 8260dbab..HEAD` lists them):
- `814302a0` + `7cebad5b`: `integ/tail3` (Google fixes, the forum archive and the task home's DM board, test flakes).
- `066b1401` + `f1fc9b03`: the threads lane's round 4 (a held compile failure, quick-mode steers, thread moves and
  growth, the placed chat's filters).

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 (one line of room: add nothing)
- `crates/theseus/src/render.rs` 3,126 / 3,128
- `crates/theseus-discord/src/render.rs` 3,003 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-protocol/src/lib.rs` 2,563 / 2,733 (room here, but the release branch counts other lanes' lines too:
  add at most two)

An unlisted file stays at 2,500 lines or fewer. Put new code in new files (a module of its own, one `mod` line in the
parent). Never raise a ceiling yourself (the keel guard flags it). If a line you truly need will not fit, leave the file
one line over and say so in your report's first section with the file and the reason.

**Work in flight beside you** (other sessions and lanes, not yet on `main`; stay out of their files where you can, and
where you can't, keep your edit small and additive so the joins merge):
- `integ/tail4` (`origin/integ/tail4`, `795afd3b` or later) and `integ/cn` round 5 (not pushed yet; its rows are the
  branches `origin/cloud/20261021-tail3-cn`, `-cn-config-guidance`, `-tail4-cn-draft` and `-tail4-cn-reply`) land as
  the next release stage. Your task's "Rules for this row" names the files of theirs that sit near yours, and how to read
  their diff (`git diff origin/main origin/integ/tail4 -- <file>`, or `git diff origin/main...origin/cloud/20261021-<row> -- <file>`).
  Read that diff before you edit those files.
- The rows beside you in this wave, and the files each owns, are named in your task's own "Rules for this row" and
  "Not in this row" sections. Those win over anything older.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

## Your task: tail5-effort, Jev's effort answer applies on its own bar, not the model switch's (theseus-7pm8t)

Branch: `cloud/20261021-tail5-effort`. Every commit's subject carries `theseus-7pm8t`. Deadline for the report: 3 hours
after you start.

**Why.** On a Terminal-Bench rerun of the routed arm (138 route decisions), Jev asked for `max` effort 8 times, each at
confidence 0.35-0.42, and **no turn ran at max**. The owner's call (2026-10-22): "We should reduce required confidence on
this one." The live config already lowered `[routing] high_effort_confidence` from 0.70 to 0.40, but that alone does
nothing for these answers: each was under the effort question's own confirm bar (its band was `escalate`), so
`routing::effort::decide` returned `EffortReason::Unsure` ("the profile's own effort stands") before it ever reached the
`xhigh`/`max` gate. The effort answer is held to the same kind of bar as the model switch, and a model switch is a far
costlier move (a cold cache) than an effort level. Decouple them.

**Risk: medium.** Turns whose effort answer was unsure now run at Jev's level more often. `xhigh` and `max` stay gated.

**Prerequisite: base `origin/main` at `f1fc9b03`.** Your branch joins the new lane `integ/tail5`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail5-effort` | theseus-7pm8t | an effort answer applies at `[routing] effort_confidence` or more even when its band is unsure; `xhigh`/`max` still need `high_effort_confidence` | `crates/theseus-core/src/routing/effort.rs` (`decide`, `EffortAnswer`, `EffortReason`, the module doc, its tests), `crates/theseus-core/src/turn/route_step.rs` (the one `effort::decide` call, near :513), `crates/theseus-core/src/config/routing.rs` (one field, its default, its validation and test), `crates/theseus-core/config/theseus.example.toml` (the `[routing]` effort comment and one key line), `bench/theseus-bench-routed.toml` (`[routing]`: the live bars), `crates/theseus-core/AGENTS.md` (the effort paragraph, near :1648) | M |

## What exists (at `f1fc9b03`; search by name)
- **The rule.** `crates/theseus-core/src/routing/effort.rs` (520 lines). `EffortAnswer { level, confidence, sure }`;
  `sure` is set in `crates/theseus-core/src/judge/inbound.rs` `verdict` (near :900) as `band != Band::Escalate` (the
  effort question's own band, from the pack's thresholds). `decide(bounds, high_confidence, answer, runs, acts, carried)`
  checks, in order: `!acts` -> `Recorded`; `carried` -> `Carried`; `unclear` -> `Unclear`; **`!answer.sure` ->
  `Unsure`**; no effort on the model -> `NoEffort`; `fixed` -> `Fixed`; then the `xhigh`/`max` gate
  (`confidence <= high_confidence` -> `High`, reason `HeldHigh`), the floor, the bounds. `EffortReason::as_str` is the
  `route.decided` row's `effort_reason`. Tests at the bottom (`a`, `at`, `runs` helpers; `BAR` 0.70).
- **The call.** `crates/theseus-core/src/turn/route_step.rs` near :497-521 (`fn effort` or the method holding
  `let decided = effort::decide(`): passes `r.high_effort_confidence` from `self.cfg.routing`.
- **The config.** `crates/theseus-core/src/config/routing.rs`: `switch_confidence` (0.6) and `high_effort_confidence`
  (0.70, `fn high_effort_confidence()`, the validation near :381 "must be above 0 and at most 1", tests near :451, :540,
  :655). The template `crates/theseus-core/config/theseus.example.toml` near :1615-1640: the comment paragraph that ends
  "Below the answer's confirm bar ... the profile's own effort stands. An answer of xhigh or max applies only when its
  confidence is above high_effort_confidence", then the `[routing]` keys.
- **The bench's routed profile.** `bench/theseus-bench-routed.toml` `[routing]` (near :162) sets `enabled`, `rung`,
  `max_wait_ms` only, so it runs on the defaults; its header comment (lines 6-10) says "at Jev's effort where Jev is sure".
- **The live config** the owner runs has `high_effort_confidence = 0.40` since 2026-10-22.

## What to build
1. **A new key `[routing] effort_confidence`**, default **0.35**, validated like `high_effort_confidence` (above 0, at
   most 1). An effort answer whose band is unsure (`!answer.sure`) but whose confidence is **at or above**
   `effort_confidence` now applies as a sure one would (then the gate, the floor, the bounds, the sticky rule, unchanged).
   Under it the profile's own effort stands with reason `Unsure`, as today. A sure answer is unaffected.
2. **`xhigh` and `max` still need `high_effort_confidence`** (the gate, unchanged in logic). The default of
   `high_effort_confidence` goes from **0.70 to 0.40** to match the owner's live config (update its default fn, its
   test, the template line and comment, the AGENTS.md paragraph and the module doc). Validation: `effort_confidence`
   must not exceed `high_effort_confidence` (say so in the error), or decide and document why not.
3. **The row says which bar applied.** A turn that applied on the lower bar records it: prefer a new `EffortReason`
   variant (for example `AppliedUnsure`, `as_str` `applied_unsure`, counted in `applied()`) over a new ledger field, so
   no wire or stored shape changes. If `HeldHigh` or `Clamped` or `Floored` also applies to such a turn, pick one rule
   and write it in the module doc (for example: the gate's, floor's and bounds' reasons win, as today; `applied_unsure`
   only where the answer's own level ran). Check the cockpit and the CLI for a list of reason strings
   (`git grep -n held_high`); add the new one wherever such a list exists.
4. **The bench's routed profile** sets `high_effort_confidence = 0.40` and `effort_confidence = 0.35` explicitly under
   `[routing]`, with one comment line ("the owner's live bars, 2026-10-22"), and its header's "where Jev is sure" says
   the new rule in a few words.
5. **Docs.** The template's `[routing]` comment paragraph and key line; `crates/theseus-core/AGENTS.md`'s effort
   paragraph (route.v3, theseus-qe3v) one sentence; the module doc's bullet list (the "confidence rules are route.v2's"
   bullet and the `xhigh`/`max` bullet). A `docs/status.md` sentence goes in your report, not the file.

## Tests (each with a planted revert; the first is the key test)
- **`a_max_answer_at_0_42_with_an_unsure_band_runs_at_max`** (unit, `effort.rs`): `at("max", 0.42, false)`, bars 0.35
  and 0.40, a model that takes effort: `ran == Some(Max)`, reason `applied_unsure` (or your variant). Plant: restore the
  `!answer.sure -> Unsure` early return: it runs at the profile's own.
- `an_unsure_answer_under_effort_confidence_keeps_the_profiles_effort`: `at("max", 0.30, false)` -> `Unsure`, `ran` the
  profile's own. Plant: compare with `>` 0.0 instead of the bar.
- `an_unsure_xhigh_between_the_bars_runs_high`: with `effort_confidence` 0.35 and `high_effort_confidence` 0.50,
  `at("xhigh", 0.42, false)` -> `High`, `HeldHigh`. Plant: skip the gate for unsure answers.
- A turn-level test through `route_step` (find the existing route.v3 effort turn tests: `git grep -n effort_reason
  crates/theseus-core/src`), Jev answering `max` at 0.42 with an escalate band: the request carries `max` and the
  `route.decided` row says your reason. Plant: pass `0.0`/`1.0` as the bar at the call site.
- Config: the default 0.35 and 0.40, a bad value refused, `effort_confidence` above `high_effort_confidence` refused (if
  you chose that rule). The existing effort tests pass, with only the 0.70 literals moved to the new default where the
  test means the default (say which in the report; no test deleted or weakened).
- `cargo nextest run -p theseus-core effort` and `cargo nextest run -p theseus-core routing`, then the whole crate.

## Not in this row
- The route pack (`route.v7`'s questions and thresholds) and the model switch's bars (`switch_confidence`, class
  bars): unchanged.
- The bench rerun and the compcert retest: the maintainer's, after install.
- theseus-1g7uj (`apply_effort` recomputes the request digest): a separate issue; leave it.

## Rules for this row
- Model roles: you design the small rule and write the code; keep it minimal. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- **Near you, not yet on main:** `integ/cn` round 5 edits `crates/theseus-core/src/config/routing.rs` (a new
  `first_pass` class key and `routing_cn.rs`, rows `cloud/20261021-tail4-cn-draft` and `cloud/20261021-cn-config-guidance`),
  `crates/theseus-core/config/theseus.example.toml` (the `[routing.*]`/`[profiles.*]` China-origin guidance blocks) and
  `crates/theseus-core/AGENTS.md`. Your edits to those three files cannot be avoided; keep them to the `[routing]`
  section's own lines (one field, its default fn, validation and test; the effort comment and one key line; one
  sentence). Read `git diff origin/main...origin/cloud/20261021-cn-config-guidance -- crates/theseus-core/config/theseus.example.toml crates/theseus-core/src/config/routing.rs`
  and the same for `-tail4-cn-draft` first, and put your new field directly under `high_effort_confidence`, not beside
  their lines. `integ/tail4` edits the template's `[voice]` comment only (no conflict).
- **Rows beside you on `main` f1fc9b03, joining `integ/tail5` with you:** tail5-threads (core `rpc/thread_move.rs`,
  `rpc/thread_grow.rs`, `thread_grow.rs`, `thread_flow.rs`, `tests_hold_compile.rs`, `turn/route_step.rs`'s `held`
  only, TUI `chat/events.rs`) and tail5-discord (Discord `runtime/task_home.rs`, `runtime/task_channels.rs`). In
  `route_step.rs` you edit only the effort call; tail5-threads edits only `held`.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Lowering
  `high_effort_confidence`'s default is the owner's own decision (2026-10-22), not a budget: say so in the commit body.
  A cloud row cannot carry a `Keel:` trailer. Before your last commit run
  `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It must be
  empty; if it flags the default change, stop and report the finding verbatim instead of working around it.
- **Dependencies.** None new. **Store format** 49; this row adds no stored field.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture ids are invented.
- Read `crates/theseus-core/AGENTS.md` (routing, the effort paragraph) first. Run
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: the rule as built (the order of checks in `decide`, the two bars, which reason wins); the defaults and
the validation; each test and its plant with the failing line; the reason strings you added and where they are listed;
the keel guard's output; the store format (unchanged); a `docs/status.md` sentence and an owner-visible line for the
integrator; follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
