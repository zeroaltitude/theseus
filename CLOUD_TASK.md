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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail-tooling`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

This task was written against an older `main`. Your branch starts from `main` at `6918e3c1` (2026-10-21, after the patch1 stage-1 landing), unless your task names another base (an integration branch). Where the
text below names `bbc65530`, a store format, line ceilings or "work in flight tonight", this section is right and the
text below is stale. Line numbers quoted below may have moved: search by name.

**Store format on your base: 48** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 49 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows in this wave may bump to 49 at once; the maintainer renumbers at the join, so never
write the number as a literal anywhere but the constant and its tests.

**What landed since `bbc65530`** (77 merges; `git log --first-parent bbc65530..HEAD` lists them). The ones most likely
to touch your area:
- Google part 1 (`f897d6b4`, with join fixes `897c3ea7`, `26b3d91b`): the `theseus-google` crate, `[google]`,
  sign-in, `google.call` (it classifies the filled path and refuses a query or an escape in a raw path).
- The data rule (`63596193`): sensitive names go only to models of US origin, failing closed; a routed turn that keeps
  failing asks and moves up a model; Jev warns on fishing for personal details.
- Turn-end facts (`9b4d2a33`, join fixes `dfd3b55e`, `69c8d74a`): a command's result names the new files it left; a
  turn about to end beside them is told once.
- Bench measurement (`5e0bd195`), the first request's persona layer per place (`fa5832c7`), the tool catalog built
  once (`17d6deec`), `blocking` as one function (`6c1332c1`).
- Needs-you cards (`37d1edb2`), the task home `#theseus-tasks` (`cf243efe`), agent answers labelled (`32512fbd`),
  self-improvement fails open (`051fc186`), a tool `tools_find` finds is callable in the same turn (`05655588`).
- Voice: echo cancel (`6bdfe9b1`), the join guard (`5a7bcbb9`), Jev checks an unclear transcript (`a86a4ade`).
- The ontology (`dc2f5f71` core, `c7ee0bb7` Jev files each inbound message, `38745067` pictures, `3a75e094` it
  reaches routing, context and recall, `7895bb45` the nightly sweep, `580c8c37` the labelled replay and the +onto
  bench arm, `1d092ded` the cockpit's Ontology view).
- Threads: a place's home splits into threads (`fd1f2e51`); a thread's recall no longer pulls another stream's facts
  (`eb43b3d4`, the thread lens); a thread adopts its task's running turn (`76237a96`).
- Since 1d092ded (`87696a7c`, 2026-10-21): task questions and reports never stall a task (`owner_ask`, `turn/join_step.rs`, `task.close`; `afa16ac5`); `theseus chat` answers the router's question and keeps an explicit session (`5f36ef45`); herdr actions take the focused pane (`6f8487f2`); the threads bench scorer (`4abaceb2`); the lifecycle bench's margins (`b87a46c4`); three load flakes waited on as events (`87696a7c`).
- Tests made hermetic on fast hosts (`76237a96`), load-flaky tests ordered by events (`a2f8d276`).

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 (one line of room)
- `crates/theseus-protocol/src/lib.rs` 2,730 / 2,733 (three lines of room)
- `crates/theseus/src/render.rs` 3,115 / 3,128
- `crates/theseus-discord/src/render.rs` 2,976 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-core/src/tests_m3.rs` 7,979 / 8,050
- `crates/theseus-core/src/turn.rs` 3,405 / 3,525
- `crates/theseus-index/src/vectors.rs` 2,561 / 2,650

Put new code in new files (a module of its own, one `mod` line in the parent). New wire types go in a protocol module
file of their own, re-exported with one line. Never raise a ceiling yourself (the keel guard flags it). If a line you
truly need will not fit under one, leave the file one line over, and say so in your report's first section with the
file and the reason: the maintainer raises it at the join, where the other rows' lines are counted too.

**Work in flight beside you** (other sessions on other branches, all from this same base; stay out of their files where
you can, and where you can't, keep your edit small and additive so the joins merge):
- Google part 2 rows: gw-mail, gw-calendar, gw-drive, gw-sheets-slides (`crates/theseus-google/`, each its own
  service module, `fake/<service>.rs`, one registration line each).
- Harness rows: job-truths (job command lines, `theseus-kernel/src/job.rs`, fs/file read results), effort-on-evidence
  (turn checkpoints' effort), turn-end follow-ups and long blind waits, recall abstention and the `service` orphan.
- Threads rows: routing (`crates/theseus-core/src/thread_route.rs`, working sets, claims) and cross-stream reach
  (`theseus-memory/src/threads.rs`, `thread_recall.rs`, a place digest, Jev's near-miss check, thread links).
- Ontology follow-ups (`crates/theseus-ontology/`, the sweep, the dream's guidance, the replay), test-infrastructure
  rows (Discord task-thread tests, leaking shell loops), the China-origin model rows (`data_gate.rs`, session data),
  rough edges in progress words, detours and `theseus why`.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

---
## Your task: tail-tooling, the keel guard sees a bench limit raised by a new match arm or a renamed-in budget constant (theseus-qfohm)

Branch: `cloud/20261021-tail-tooling`. Every commit's subject carries `theseus-qfohm`. Deadline for the report: 3
hours after you start.

**Why.** The keel guard (`scripts/keel-guard.py`) exists so that no branch passes the gate by loosening what would
have failed it. It missed a real bench-limit raise. Commit `94eb8a7d` (on `main` through `87696a7c`) gave the
lifecycle bench's `seed` and `health` phases a noise margin of 2 and 3 ms. Their limits went from 2 to 4 ms and from
5 to 8 ms. The owner approved that raise, so nothing is reverted. But `python3 scripts/keel-guard.py --base 1d092ded
--head 94eb8a7d` printed `keel: ok … 0 findings acked`, although the guard's own docstring and its `BUDGETS` table list
`fn margin_ms` in `crates/theseus-sim/src/lifecycle.rs` as a keel item. The commit's `Keel:` trailer was the only
record of the raise, and the guard never checked it. An unapproved raise of the same shape would pass the gate
silently today.

The cause is in `budget_rule` → `compare_rows` (both in `scripts/keel-guard.py`). `compare_rows` turns every line of a
keel item that holds a number into a "shape": the line with each number replaced by `#`. It pairs each base shape
with the same shape at the head and flags a pair whose numbers rose. It also flags a base shape that is gone at the
head (`removal=True`). A line that is **new** at the head, with no base shape to pair with, is never judged.
`margin_ms` is a `match` with a `_ => 0.0` fall-through:

```rust
pub fn margin_ms(phase: &str) -> f64 {
    match phase {
        "cold" | "vault" => 7.0,
        "shutdown" => 4.0,
        "kill" => 25.0,
        "swap" => 2.0,
        "seed" => 2.0,      // new in 94eb8a7d: was the fall-through's 0.0
        "health" => 3.0,    // new in 94eb8a7d: was the fall-through's 0.0
        _ => 0.0,
    }
}
```

The two new arms are new shapes, so nothing was compared. In effect the raise was 0.0 → 2.0 and 0.0 → 3.0 (the value
the fall-through gave those phases before). There is a second hole of the same kind. `budget_ms`'s `"surfaces"` arm
reads `surfaces::BUDGET_MS`, a `pub const` in `crates/theseus-sim/src/lifecycle/surfaces.rs` (50.0 at the base), and
that file is not in `BUDGETS`. Raising that constant moves a bench budget and the guard does not look at the file.

**Risk: low.** This changes only the guard and its test suite. Every finding the guard reports today must still be
reported, with the same text. The gate runs the **base's** copy of the guard (see the docstring: the gate passes
`--base` to the base's copy), so your branch's change judges the branches that come after it, not your own.

**Prerequisite: `main`.**

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail-tooling` | theseus-qfohm | a new arm in a keel item's `match` judged against the value the base's fall-through gave it; budget-like constants in the sim crate judged by name; refuse an ack whose rule is not a rule | `scripts/keel-guard.py`, `scripts/test_keel_guard.py` (nothing else) | S |

## What exists (at `6918e3c1`; search by name if a line moved)
- `scripts/keel-guard.py` (1,598 lines, standard library only).
  - The docstring's rule list near the top. `budget-raised` lists `lifecycle.rs` (`fn budget_ms`, `fn margin_ms`,
    `const CANCEL_FRAMES`), `perf.rs`, `jobs.rs`, `first_request.rs` and `gate.sh`'s
    `THESEUS_GATE_BENCH_ALLOWANCE` default.
  - `NUM`, `num`, `shape_rows`, `compare_rows` (near :954-986).
  - `rust_item_lines(src, kind, name)` (near :988): the comment-free lines of a `fn`'s body or a `const`'s line.
  - `BUDGETS` (near :1015) and `budget_rule` (near :1027).
  - `cap_code_rule` / `cap_items` / `CAP_CONST` (near :1123-1156). This rule already judges constants **by name**
    across the changed `.rs` files: a moved one keeps its value, a new one is no finding. Copy its shape for item 2.
  - `judge` (near :1446) dispatches the rules per path. `if p in BUDGETS or p == "scripts/gate.sh": budget_rule(...)`.
  - `ACK` (near :1478), `acks` (near :1503), `acked` (near :1539), `main`. An ack line matches
    `<rule> <paths> — <why>`. `<rule>` is any `[\w-]+` and is never checked against `RULES`.
- `scripts/test_keel_guard.py` (1,024 lines). It holds the fixtures `LIB`, `LIFECYCLE` (a `budget_ms` with
  `"cold" | "vault"`, `"shutdown"`, `"cancel"` arms and `_ => None`, plus `CANCEL_FRAMES`; it has **no**
  `margin_ms`), `FIRST_REQUEST`, `BENCH`, `LONG` and `BASE`. It also has `Repo`, `Case.assertCaught(rule, path, files)`
  and `Case.assertClean(files)`. The relevant existing cases are `test_a_lifecycle_budget_raised`,
  `test_a_cancel_budget_dropped`, `test_a_budget_lowered` and `test_a_comment_in_a_budget`. The signed-ack cases
  (class near :920) make throwaway gpg keys and are skipped when there is no gpg. Run the suite with
  `python3 -m unittest scripts/test_keel_guard.py`.

## What to build
1. **A new arm is judged against the fall-through it replaced.** In `budget_rule`, for a `fn` item whose body is a
   `match` with a fall-through arm (`_ => <expr>`):
   - At the base, read the numbers in the fall-through arm's expression. For `_ => 0.0` that is `[0.0]`. For
     `_ => None` there are none.
   - At the head, a numeric arm whose shape had no base partner (the rows `compare_rows` leaves unpaired today) is
     compared with the base fall-through's numbers. A number above the fall-through's value is a `budget-raised`
     finding on that line. Its old side reads e.g. `_ => 0.0 (the fall-through)`.
   - Where the base fall-through has no number (`_ => None`: an unbudgeted phase gains a budget, which is a
     tightening), a new arm is no finding.
   - A new arm for a pattern that the base matched elsewhere is just a rewritten row: `"seed" | "health" => 3.0`
     split into two arms, or the reverse. Judge it against the base arm that matched that pattern. A pattern's value
     at the base is the arm whose pattern list names it, else the fall-through.
   - The simplest exact form is a small "value per string pattern" reading of a `match` over string literals. If the
     item is not such a `match`, keep today's behaviour unchanged.
   - Keep `compare_rows` as it is for every other caller: `gate.sh`, `const` items, and fns that are not a `match`.
   - Update the docstring's `budget-raised` paragraph in one line: "a new arm of a keel fn's match is judged against
     the base's fall-through for it".
2. **The sim crate's budget constants, judged by name.** In `crates/theseus-sim/src/**/*.rs`, excluding test files
   (`tests.rs`, `tests_*.rs`, `tests/`), a `const` whose name contains `BUDGET`, `MARGIN`, `SLACK`, `TOLERANCE` or
   `ALLOWANCE` is judged as a budget item.
   - Today this catches `crates/theseus-sim/src/lifecycle/surfaces.rs` `BUDGET_MS` (50.0).
   - Use `cap_code_rule`'s approach: collect such constants across the range's changed files at base and head, pair
     them by name, and flag a pair whose number rose or a constant that is gone. A constant moved to another file
     keeps its value and is not a finding; a new one is not a finding.
   - Rule name `budget-raised`, and the finding's path is the head's path.
   - Keep the scope to `crates/theseus-sim/src/`. Outside the sim crate, names like `MARGIN_MS` or `REFRESH_MARGIN`
     are not bench limits: there are about 28 such constants in other crates at the base, and raising some of them
     is a tightening. Say this in the docstring line.
   - Also add `perf.rs`'s `const BATCH_OVER_ONE_CALL_MS` (the batch turn's allowance over its sleeps, 300.0) to
     `BUDGETS`. It is a budget term that its name does not reveal.
3. **An ack whose rule is not a rule is refused, saying so.** In `acks`, an ack line whose `rule` is not in `RULES`
   is added to `refused` with the reason `` `budget-raise` is not a rule (the rules: test-removed, …) ``. Today such a
   line is accepted and matches nothing, so the person writing it is never told. This is the exact shape of
   `94eb8a7d`'s trailer, `Keel: budget-raise crates/theseus-sim/src/lifecycle.rs margin_ms — …`: its rule is
   `budget-raise`, not `budget-raised`, and its path part has a second word.
   - Also refuse an ack whose path part has a space inside one comma-separated entry, with the reason
     `` a path holds a space: `crates/…/lifecycle.rs margin_ms` (paths are comma-separated) ``.
   - Both reasons print through the existing `keel: refused the ack …` line in `main`.
   - Today an unsigned ack is refused before its form is read. Keep that order: a malformed ack in an unsigned commit
     is still refused as "unsigned".

## Tests (each with a planted revert; the first is the key test)
Add a `margin_ms` to the `LIFECYCLE` fixture, so every existing case still runs against it:
```rust
pub fn margin_ms(phase: &str) -> f64 {
    match phase {
        "cold" | "vault" => 7.0,
        "shutdown" => 4.0,
        _ => 0.0,
    }
}
```
Then confirm that every existing case passes unchanged.

- **`test_a_margin_arm_added_over_the_fall_through`** (the key test). The head adds `"seed" => 2.0,` before
  `_ => 0.0`. It is caught as `budget-raised` on `crates/theseus-sim/src/lifecycle.rs`, and the printed trailer line
  is `Keel: budget-raised crates/theseus-sim/src/lifecycle.rs`. **Plant:** skip item 1's comparison for unpaired rows
  (today's behaviour). The test must fail.
- `test_a_margin_arm_added_at_zero_is_clean`: a new arm `"seed" => 0.0,` is clean. So is a new arm in `budget_ms`
  under its `_ => None` (an unbudgeted phase gaining a budget).
- `test_a_margin_arm_split_keeps_its_value`: `"cold" | "vault" => 7.0,` rewritten as two arms of 7.0 is clean. As 7.0
  and 8.0, it is caught on the 8.0 arm.
- `test_a_margin_lowered`: `"shutdown" => 3.0` is clean (it pairs with the base arm and its number fell).
- `test_a_fall_through_raised`: `_ => 0.0` to `_ => 1.0` is caught. This already works through `compare_rows`; the
  case pins it.
- `test_a_sim_budget_const_raised` (item 2): add a fixture file `crates/theseus-sim/src/lifecycle/surfaces.rs` with
  `pub const BUDGET_MS: f64 = 50.0;` to `BASE`. The head raises it to 60.0 and it is caught. It is clean at 40.0. Moved
  unchanged to `crates/theseus-sim/src/surfaces.rs`, it is clean. A new `const SLACK_MS` is clean. The same constant
  name raised in a non-sim crate fixture (`crates/demo/src/lib.rs`) is clean. **Plant:** drop the name rule, and the
  raise case must fail.
- `test_a_batch_allowance_raised`: `BATCH_OVER_ONE_CALL_MS` 300.0 to 400.0 in a `perf.rs` fixture is caught.
- `test_an_ack_with_no_such_rule_is_refused` (item 3, in the signed class: it needs gpg). A commit signed by the listed
  key carries `Keel: budget-raise crates/theseus-sim/src/lifecycle.rs — the owner's yes`. The guard prints
  `refused the ack` and `is not a rule`, and the finding stays open (exit 1). A second case covers the space-in-path
  form. **Plant:** accept any rule word again, and the case must fail.
- `test_the_lane_margin_shape_is_caught`: the regression in one case, built from `94eb8a7d`'s exact diff shape. Add
  both arms (`"seed" => 2.0` and `"health" => 3.0`) at once, with a doc comment above the fn. Expect two
  `budget-raised` lines.

If this VM has no gpg, the signed class skips itself. Say so in the report, and give the maintainer the command:
`python3 -m unittest scripts.test_keel_guard -k ack` (run from the repo root; adjust to the module path the suite
uses).

Also run the guard over real history, as a check and not as a test. Report the output of
`python3 scripts/keel-guard.py --base 1d092ded --head 94eb8a7d` (or `--base 94eb8a7d^`). It must now name the two
arms as `budget-raised`, and it must print the commit's trailer as refused (`budget-raise` is not a rule). That
refusal is the expected, correct reading of history. Do not change that commit.

Run the guard over the last 20 first-parent merges of `main`: for each merge M, `--base M^1 --head M`. List every
finding that is new against the base's copy of the guard. Each one is either a real raise (name it) or a false
positive (fix the rule until there are none). This step is how you show the rule does not fire on ordinary sim-crate
edits.

## Not in this row
- Any change to `crates/theseus-sim/` or to any budget value. This row does not touch Rust.
- The agent-side branch scrubber: it is tooling outside this repository, tracked elsewhere.
- Name-pattern constants outside `crates/theseus-sim/src/`. Say in the report whether any of the 28 such constants in
  other crates looks like a bench limit that should be listed in `BUDGETS` by hand. Name them, but do not add them.
- Reformatting or restructuring the guard beyond what these three items need.

## The keel and this row
Any change to `scripts/keel-guard.py` or `scripts/test_keel_guard.py` is a `keel-file` finding, even a tightening:
the guard cannot tell a tightening of itself from a loosening. Your final keel-guard run **will** print
```
crates/…: keel-file: …
  Keel: keel-file scripts/keel-guard.py, scripts/test_keel_guard.py — <why>
```
and exit 1. That is the one keel finding this task names, and it is expected.
- **Do not write a `Keel:` trailer yourself.** An ack counts only on a commit signed by a key in the base's
  `scripts/keel-signers`, which this VM does not have.
- The maintainer adds the signed trailer at the join. It quotes the owner's yes to this change, in this form:
  `Keel: keel-file scripts/keel-guard.py, scripts/test_keel_guard.py — the owner, <where and when>: "<his words>"`.
- Paste the full guard output into the report, and confirm it names nothing **but** `keel-file` on those two paths.

## Rules for this row
- **Code style.** You write the code; keep it minimal and elegant, so that every line you add earns its place.
  - Match the guard's existing style: standard library only, small functions, and docstrings that say what a rule
    reads.
  - Prefer extending `budget_rule` and reusing `rust_item_lines`, `shape_rows` and `cap_code_rule`'s pairing over
    writing a parallel mechanism.
- **Keep everything that already works.** Every existing case in `scripts/test_keel_guard.py` passes unchanged, and
  every existing finding keeps its exact text. No existing case is edited except to add `margin_ms` to the `LIFECYCLE`
  fixture and the new fixture files to `BASE`. If an existing case needs another edit, explain why in the report.
- **Plants.** Every behaviour change gets a test and a planted revert. Break the code, watch the named test fail,
  restore it and `touch` the file, then run `git status`.
  - List each plant in the report with the test that failed.
  - The key test, named first under Tests, must be shown failing on its planted revert.
- **Keel.** Do not delete, skip or weaken a test. Do not raise a budget, cap or ceiling. Before your last commit, run
  `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output into the report. It must
  show only the `keel-file` finding described above.
- **No new dependency.** The guard stays standard-library Python, and nothing is added to Cargo.lock or any
  package-lock.json.
- **Secrets and names.** Use no person's name anywhere: not in the repo, not in commit messages, not in the report.
  Write "the owner". Fixture names and e-mail addresses are invented (`@example.invalid`, as the suite already does).
- Read the repository's root `AGENTS.md` (the keel section) first.
- **CLOUD_REPORT.md is honest.** It says what you ran, what passed, and what you could not run on this VM.

## Your report
Commit `CLOUD_REPORT.md`. It covers:
- what changed, rule by rule;
- each new test and its plant;
- the full output of the suite (`python3 -m unittest scripts/test_keel_guard.py`), including how many cases ran and
  how many were skipped;
- the guard's output on `94eb8a7d`, and the new findings, if any, from the last 20 merges of `main`;
- what could not run on the VM (the gpg cases, if skipped) with the exact commands for the maintainer;
- the keel guard's output on your branch (only the expected `keel-file` finding), and the trailer line the maintainer
  must add at the join;
- follow-ups, such as constants outside the sim crate that look like bench limits.

Write it with your editor tools, never a shell heredoc. Check its size after the commit: it must not be empty. Push.
End with a final message that starts `CLOUD REPORT COMPLETE`.
