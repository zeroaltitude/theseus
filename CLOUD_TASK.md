<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/ and the benchmark adapters under bench/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, the owner's machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate or directory you touch (cockpit/ has its own; bench/ has its README), scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

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
- Under load, these timing tests can fail. The flaky list in .config/nextest.toml is empty (since d279767f), so none retries: rerun a failing one alone, and name it in the report. Those marked ‡ and § had fixes joined on 2026-10-06 and 10-07; they are listed because a slow VM can still trip them.
  - theseus-core: `tests_push::a_client_that_stops_reading_hears_what_it_lost_and_catches_up` can pass nextest's 120 s
    kill (theseus-0u6g‡); `learning::tender::tests::a_pool_thread_started_from_the_idle_thread_keeps_its_policy`
    (theseus-1g8j‡); `tests_m3::parallel::a_calls_time_is_its_own_run_not_its_wait_for_the_turn`'s 20 ms bound
    (theseus-b38m‡); `tests_activation_pace::a_clean_stop_ends_the_warm_builds_waits` (theseus-9o2o‡); and
    `term::tests::a_close_leaves_no_child_behind` (theseus-d006‡, a negative assertion: keep its output);
  - theseus-core's aws/hands `runaway_mode_refuses_at_ten_times_the_hours_line` and
    `ten_times_refuses_and_a_cancel_still_runs` fail when a run crosses an hour boundary (they read the wall clock's
    hour, theseus-5a50): rerun them mid-hour;
  - theseus-kernel's `children::a_sweep_reaps_wrappers_and_orphans_and_never_an_owned_child` (theseus-r4hn‡: a count of
    how processes were classed, so its failure is a finding) and theseus-aws-catalog's
    `one_service_decodes_in_under_5_ms` (theseus-rnl3‡: a wall-clock decode bound in a debug build);
  - theseus-discord: tests_outbox's `a_cards_settle_waits_for_its_create_and_edits_it_by_id` (theseus-0bq1‡, sj0t‡),
    `a_refused_pin_leaves_the_board_unpinned_and_edited` (theseus-o2tm‡), `live_edits_of_one_message_coalesce_into_the_last`
    and `a_shared_channels_card_goes_to_the_dm_and_nothing_mentions_anyone` (theseus-3ae1, pb3l), and tests_gateway's
    `a_jev_notice_goes_to_the_owners_dm_and_a_press_there_labels_it` (theseus-3ae1);
  - theseusd: `bench_profile::a_first_byte_timeout_is_retried_inside_the_headless_turn`, which saw 2 model requests
    with transient retries off (theseus-jtrc§, a count: keep its output), and
    `stops::a_stop_of_three_jobs_that_ignore_sigterm_takes_one_grace_and_holds_no_worker`, which read an empty SIGTERM
    file (theseus-y0lm§).
- A negative assertion ("nothing of X reached Y", "no process is left", "no retry") that fails even once is a finding, not a flake: keep its output, name it in the report, and don't retry it away.

Timing tests also fail here more often than on the owner's 16-core machine. Any failure outside the lists above is yours to explain.

**What main holds.** You clone main at `5342a01c` or later, with **store format 52** (`MANIFEST_FORMAT` in crates/theseus-store/src/store.rs:692 — verified at your base; check it yourself before you bump it). Your clone has v1's milestones, everything batches 8 to 12 joined, and since 2026-10-07:
- the reliability rows (spawn-enosys: proc.run starts where clone3 is refused, and a failed start says why; phantom-jobs; stop-notice; daemon-robust; daemon-memory), budgets-notify (a session's spend limit and loop cap notify instead of restricting) and daily-ceiling (a daemon-wide hard ceiling on model spend per local day, $200 by default);
- the judgment rows: jev-effort (route.v3: Jev picks the reply's effort), route-feedback, bench-routed (a Theseus-as-shipped bench arm), recall-fallback, memory-lookup (the `memory.lookup` tool), recall-in-deadline (recall's vector query inside its 250 ms deadline);
- the tools: terminals (background jobs a terminal started outlive the session; `term.read` until idle), fs-read-big (`fs.read` reads a window of a file over its size cap), web-search-unkeyed;
- the clients: session-states (sessions live, quiet or retired), cli-status (`theseus status`, `wait --any`), seen-shared, work-types (the work view and the shared notification policy in theseus-protocol), push-fixes, client-names, spawn-ask-follow, paste-order (one connection's requests that change a session apply in their arrival order), tui-paste (a paste is one event), discord-silent (today's pings by default, silence per place, a loop's thinking folded into its process message), footer-cost;
- people and people-jev (person categories, handles and merges; `people.v1` proposes people from text: a model extracts, Jev judges), and the cockpit's context explorer and Benchmarks tab;
- **the keel guard** (scripts/keel-guard.py, a gate phase; see "The keel" below);
- and, landed in the last day: content flags (`9470df70`), the recall refill pin (`47b94066`), **"Not this" — the feedback leg of notify-work-feedback (`79a5f2d7`, theseus-o1h3f.2)**, one definition of a content item (`9cc63f93`), and the RSI door's bounded git fetch (`5342a01c`, theseus-6vlr4).

**Other changes in flight.** These are being built elsewhere, each on its own branch, and join `main` one at a time while you work. Stay out of their areas unless your task needs it; if it does, keep the edit small and say so in the report. Read `git log origin/main` at your start: some will have landed.
- people-ui: proposals grouped by person in `theseus ontology proposals` and the cockpit's proposals panel; the nightly people sweep and `[people] sweep_usd_per_day` (core judge/people modules, the CLI's ontology command, cockpit ontology views, config/);
- keel-fixes: scripts/gate.sh, scripts/keel-guard.py and its suite;
- discord-calls: theseus-discord's lanes and pings (`Lane::process_ping`), theseus-protocol's notices doc, crates/theseus-discord/AGENTS.md;
- bench-dig: benchmark reports under docs/benchmarks/ and small bench/ harness fixes;
- an `integ/p3w-rsi` integration branch of three RSI merges, being prepared on the owner's machine as you are fired; it may land on main before or after you start.

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (**52** at your base), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. If yours needs a bump, say why. Others may bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling:** read scripts/long-files.txt and `wc -l` the files you touch before you add to them; a Rust file the list doesn't name fails past 2,500 lines. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tool-run logic in crates/theseus-core/src/toolrun/, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line. **Note for this wave:** `crates/theseus-protocol/src/lib.rs` was at 2,629 of its 2,733 ceiling on 2026-10-27 and several live branches touch it (two branches can each stay under a ceiling and break it once merged). If your change adds to that file, keep it to the fewest possible lines and say in the report how many you added.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. crates/theseus-core/src/config.rs has room for a key's field line; a new section's type goes in a module under config/. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).
- **Python under bench/** imports only the standard library, except where the Harbor adapter already imports Harbor, and the gate doesn't run its tests: run them yourself before each commit (bench/README.md says how), and say so in the report. Every benchmark run gets a report in docs/benchmarks/: the maintainer writes those, and no task here runs a benchmark.

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone. A commit that changes only Python, Markdown or task files under bench/ changes nothing the gate builds (its one read there is `bench/theseus-bench.toml`, in theseusd's bench_profile test: leave that file as it is unless your task names it).

The gate's shape phase fails a Rust file over its line ceiling in scripts/long-files.txt, and one it doesn't list past 2,500 lines. If you would grow a file past its ceiling, split it: never raise a ceiling (the keel, below).

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, claude-sonnet-5-5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/BRANCH`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- **First, before any of that: push a start commit.** As soon as your setup and first build finish, make an empty commit (`git commit --allow-empty -m "session started"`) and push your branch. Do this even though you have nothing proved yet, and do it before you begin the work. **Why it is mandatory and not merely advised:** on 2026-10-24, three of sixteen rows ended with nothing on their branch but the task commit, and because every other row had pushed only at the very end, a session that died at minute 5 and one that died at minute 110 left *byte-identical* branches — so there was no way to tell a row that never started from one that worked for two hours and lost it. A start commit makes those two cases distinguishable, and it costs one second.
- No new dependencies: Cargo.lock and the package-lock.json files must not gain a package, and bench/'s Python gains no import beyond the standard library and the Harbor its adapter already uses. If the right design needs one, say so in the report instead.
- Use invented names in fixtures, tests, and commits (AGENTS.md, Item 16). Write nothing about the owner, the owner's machine, accounts, or anyone else.
- **Write no person's name anywhere**: not in code, comments, docs, fixtures, goldens, commit messages or your report. Where a person is meant, write "the owner" (the handle `zeroaltitude` where an identity string is needed), "collaborator" for anyone else; the AI assistant may appear only as "Tabitha/Claude", and your commit trailer names only Claude. **This is not a formality and it has cost this bundle real time:** in the wave before yours, a sibling row shipped real people's names in its test fixtures, and clearing them was a blocking join fix that held the merge. The names lane cleared the tree at ea34457e: never bring a name back from memory, an old branch or a fixture you copy. Grep your own diff for names before every commit.
- Don't edit the spec (docs/spec/), docs/status.md, the README, docs/benchmarks.md, docs/benchmarks/, or docs/design/. The maintainer writes those at review. Where a doc under those paths should change, say exactly what and where in the report, with the text you would have written.
- Don't commit half a step. If time runs out mid-step, leave it out and report what you found.

**The report:** when you are done, or at your task's deadline (by the clock, waits included), whichever comes first, write CLOUD_REPORT.md at the repository root. Commit it as the branch's last commit, subject `cloud report (not for main)`, and push. The maintainer reads it from the branch and drops that commit at the merge. For each step it says:
- what you found;
- what you changed (commit hashes);
- how you proved it: the commands and their results, with test counts, the runs under load, and each planted revert and what failed;
- the live check the maintainer should run, as exact commands, and what each should show;
- what is left or uncertain, and any design choice the owner should hear about;
- **integrator notes: exactly which of your commits are for main, and which are not.**

Then the gate's result, naming each failing test and why. Your final message is short (under 1,200 characters): the line `CLOUD REPORT COMPLETE`, then the branch, its head commit, one line per step, and the gate's result.

**The keel.** The gate's keel phase (scripts/keel-guard.py; its docstring lists the rules) fails a range that deletes a test, adds `#[ignore]` or a skip, lowers a test's assertion count, adds an `#[allow]`, raises a bench budget, a spend or loop cap, or a ceiling in scripts/long-files.txt, scripts/shape.sh or .config/nextest.toml, or edits the guard's own files, unless a commit signed by the owner's key acks it. **You can't sign, so you can't ack.** Rewrite an assertion rather than delete it (a rewritten assertion is not a finding), split a file rather than raise its ceiling, and keep caps. Where your task itself changes a cap or a ceiling, or a test must change its meaning, make that change in a commit of its own whose message says why, and list it in the report under **Keel findings expected** (rule, path, why), so the maintainer can bring it to the owner — and where a guard the owner must loosen is involved, the `Keel:` trailer the maintainer will write has to quote the owner verbatim, so give the maintainer the exact sentence you would want quoted. Run `python3 scripts/keel-guard.py` before each commit and paste its findings into the report. When the keel phase fails only on findings you listed there, run the gate's later phases yourself (as for the known failures above) and count the commit green.

**Planted reverts:** to prove a test guards a behaviour, plant the bug, show the test fail, then restore the file and `touch` it, so cargo rebuilds it (a restored file with its old mtime keeps the planted build). Run `git status` after every restore.

**Red-first is the rule, not a preference.** A test that would have passed before your change is not a test of your change. For every behaviour you change, show the test **failing at your base** first, then passing on your change, and put both outputs in the report. A step whose report has no red-first evidence will be sent back.

**Load:** where your task asks for runs under load, use AGENTS.md's recipe. Priority, not count, makes the load: run the test with `nice -n 19`, and beside it four busy loops at nice 0, each `sh -c 'while :; do :; done' &`. Kill the loops by the pids you started, never by a name pattern.
---



# Your task: a family a Find loaded must not be dropped as if a wake loaded it (`theseus-hhxqn`)

## Step 0, before anything else: re-verify every citation in this prompt

**Do this first and report the result.** Every `file:line` below was checked against `origin/main` =
`5342a01c` on 2026-10-27 at about 08:4x. Open each in your own clone and say in your report what it actually
was. **Name every citation this prompt got wrong.** If the ground has moved far enough that the defect no
longer describes the tree, stop, write that up, and do not invent a replacement task.

**One path in the row's notes needs care.** The notes write the third citation as `tools_defer.rs:97-101`.
That is **`crates/theseus-core/src/tools_defer.rs`** — the top-level module — **not**
`crates/theseus-core/src/turn/tools_defer.rs`, which does not exist. There is also a `tools_defer/`
directory of submodules (`family.rs`, `find.rs`, `fit.rs`, `select.rs`, and others); do not confuse them.

## The defect

`integ/p3-o`'s blocking fix 1 added **`Deferral.dropped`**, so that a family the wake's loosening put in does
not stick into the next person's turn.

At `crates/theseus-core/src/turn/defer_step.rs:262` (418 lines at your base), verified exact:

```
259:        // The converse of `added`: a person's turn drops a family only the loosening loaded, so the
260:        // append's tools differ from the compilation's and it recompiles without it. Read after the
261:        // message's own admissions, so a family this turn loads is never dropped.
262:        let dropped: BTreeSet<String> = (!noticed && !loaded.contains(tools_defer::family::TASKS))
263:            .then(|| tools_defer::family::TASKS.to_string())
264:            .into_iter()
265:            .collect();
```

**The recorded set cannot distinguish a family the WAKE loaded from one the Find mechanism loaded.** The
condition at `:262` asks only whether the family is in `loaded` *this* turn — not *what put it there on a
previous turn*. So a tasks family that **a person's own earlier turn found**, through `tools_find`, is
dropped at the next person turn exactly as a wake-loaded one is.

That **contradicts `theseus-342qy`'s stated stickiness for Find-mechanism (GLM-class) models** — models with
no provider-side tool search, for which the whole point of `tools_find` is that a found tool stays available.

The other half of the machinery, at `defer_step.rs:284-299`, verified exact:

```
284:    pub(super) fn defer_found(&self, t: &Turn<'_>, spec: &mut RequestSpec) {
285:        let found = self.tools.defer.take_found(t.tc.session_id);
...
292:        if d.mechanism != Mechanism::Find {
293:            return;
294:        }
295:        let known = tools_defer::families_of(&spec.tools);
296:        let found: BTreeSet<String> = found.into_iter().filter(|f| known.contains(f)).collect();
297:        d.loaded.extend(found.iter().cloned());
298:        d.added.extend(found);
299:    }
```

Note what this tells you: **`defer_found` already knows the mechanism** (`:292` returns early unless it is
`Mechanism::Find`) and already records what a Find added, in `d.added` (`:298`). The information exists —
**it just does not survive to the next turn**, because `added` is per-turn.

And the struct, at `crates/theseus-core/src/tools_defer.rs:93-108`, verified:

```
 93: pub struct Deferral {
 94:     pub mechanism: Mechanism,
 95:     pub loaded: BTreeSet<String>,
 96:     pub resolved: bool,
 97:     /// Families a `tools_find` of this turn asked for, already in `loaded`
 ...
103:     pub added: BTreeSet<String>,
104:     /// Families the fresh choice does not load that a compilation recorded because a turn with
105:     /// no person's input loaded them (theseus-o1h3f.3). An append drops them, so the tools
...
108:     pub dropped: BTreeSet<String>,
```

Read those two doc comments in full. They are the clearest statement of the two concepts you are being asked
to tell apart, and your change must keep both of them true.

## What it costs, and why that shapes the row

**The cost of the bug is a recompile and a re-find. It breaks nothing and loses no work.** That is why the
reviewer marked it non-blocking and why install #89 shipped with it.

So: **this is a correctness-and-politeness row, not an emergency, and a wrong fix is worse than the bug.** If
your change makes a wake-loaded family *stick* — the thing `Deferral.dropped` was added to prevent — you have
traded a cheap recompile for the expensive problem the fix was built to solve. Guard that direction with a
test, explicitly.

## Why this is a design change and not a one-line edit

The row says it plainly: *"Telling the two apart needs new per-session state (which mechanism put the family
in), so it is a design change, not a merge-commit edit."*

That per-session state is the real work. Think it through and **state your design in the report before the
diff**:

- Where does it live? `take_found` at `:285` is keyed by `t.tc.session_id`, so there is already a
  per-session store for found families — look there first. Reusing it is much better than adding a parallel
  one.
- What is its lifetime? A Find-loaded family should survive the *next* person turn, but it should not
  survive for ever: a session that found a family twenty turns ago and never used it should not carry it
  indefinitely. **Decide, justify, and say what you chose.** If you choose "for ever until the session ends",
  say why that is right rather than leaving it unstated.
- Does it need to reach the store? If it does, that is a `MANIFEST_FORMAT` bump (52 at your base) and an
  old-layout sample in `tests_layouts.rs` — see the store's version rule. **Prefer a design that does not
  need one**, and if yours does, say why in the report.
- `Mechanism` is already on `Deferral` at `:94`. Check whether a per-family mechanism is really needed or
  whether the per-deferral one plus the found set is enough.

## What closes this row

The family at `:262` is dropped **only** when what loaded it was the wake's loosening — never when a
`tools_find` loaded it.

Read `reviews/p3o-joinfix-report.md` section 1 and `local/p3o-joinfix-report.md` fix 1 if either is in the
repository; the builder flagged this itself, untested, and was right to. If they are not in the repository,
say so and work from this prompt.

## The tests — three, and the third is the one that matters

In a `tests_<subject>.rs` of its own, or in the existing tests file for this area if the crate's convention
puts it there:

1. **A tasks family loaded by a wake IS still dropped at the next person turn.** This is the regression
   guard on `theseus-o1h3f.3`'s fix. It must pass at the base and still pass after your change. **Say
   explicitly in the report that this one is a guard, not a red-first test.**
2. **A tasks family loaded by `tools_find` in a person's turn is NOT dropped at the next person turn.** This
   is the row. It must **fail at your base** — that is your red-first evidence — and pass on your change.
3. **The two together in one session**, in sequence: a Find loads a family, then a wake loosening loads
   another, then a person's turn. The Find's survives; the wake's is dropped. **This is the test that proves
   you built a distinction rather than just turning the drop off**, and a fix that passes 1 and 2 but fails 3
   is not a fix.

### Planted reverts (required)

- Revert your mechanism check so the drop again ignores how the family was loaded → **test 2 must go RED**.
- Make the drop never fire → **test 1 must go RED**.

Plant, show the red, restore the file, `touch` it, run `git status`. Paste all of it. A test that still
passes with its plant applied has not been written.

Run `cargo nextest list` with your filters and paste the output, proving each filter selects the test you
think it does.

### What will get this row rejected

- Turning the drop off, or weakening it, instead of making it mechanism-aware.
- A fix with no test 3.
- New per-session state with no stated lifetime.
- A `MANIFEST_FORMAT` bump that was not necessary, or a necessary one without the old-layout sample.
- Changing what `added` or `loaded` mean. Their doc comments at `tools_defer.rs:97-103` and `:104-108` must
  stay true; if you must change one, quote the old and the new in the report and say why.

## Scope

`crates/theseus-core/src/turn/defer_step.rs`, `crates/theseus-core/src/tools_defer.rs` and its submodules as
the design needs, plus your tests file. Keep out of `crates/theseus-core/src/rsi/` entirely — two other rows
in this wave and an integration branch are working there.


## The disk — read this before your first build, it cost two sessions a gate cycle each

> Build with `CARGO_INCREMENTAL=0` from the start, and if the gate dies on a full disk or an `ld` Bus error,
> `rm -rf target/debug/incremental` and rebuild rather than investigating the compiler.

Both cloud VMs ran out of disk during a full workspace build on 2026-10-26 (`/dev/vda`, 252 G), and each
burned a whole gate cycle rediscovering it independently. The symptom does not look like a disk problem: it
looks like a linker or compiler fault. It is not. Set `CARGO_INCREMENTAL=0` in your environment now, before
the first build, and you will probably never see it.

## The known-failure set — do NOT read these as your own regression

> The gate's suite has a known failing set on a host without a delegated cgroup: about 34 failures, 20 in
> `theseus-sandbox` (contract, `spawn_100`) and 14 in `theseusd::sandbox` (root daemon, no job cgroup,
> `theseus-pv6i`). These are L1/root failures and are NOT yours. Separately, a test that fails once and then
> passes on rerun and on the untouched base is a fast-host ordering race, not your regression — say so and
> move on. Judge your change by whether a failure reproduces on the base.

A session that does not know this set reads it as its own regression and stalls. **Judge every failure by
whether it reproduces on the untouched base.** Run the base if you are not sure: it is cheaper than guessing.

## Other branches in flight — stay out of their hunks

These are live and unmerged at your base. Your task names the ones that actually meet your files; this is the
full list so you can recognise a conflict when you see one:

- **Returned and under review, not running:** `cloud/20261027-p3u-may-build-stop` (rsi/tree.rs's
  `may_build_paced`, rsi/pr.rs), `cloud/20261027-p3u-tree-close-signal` (rsi/join/joining.rs),
  `cloud/20261027-p3u-burn-ancestors-test` (theseus-kernel: day_ceiling.rs, tests_runaway.rs),
  `cloud/20261027-p3u-late-content-test` (theseus-core: toolrun/late.rs, a tests_content_* file),
  `cloud/20261027-p3u-peek-follower-race` (theseus-core: rpc/tests_peek.rs),
  `cloud/20261027-p3u-paused-clock-sweep` (test-health, potentially many files — **if you are not that row
  and you see a paused-clock change land near you, it is theirs**).
- `cloud/20261027-p3t-fanout-cue` and `cloud/20261027-p3t-people-same` — returned, join fixes being applied.
- `cloud/20261026-p3p-onto-names-hang`, `cloud/20261026-p3n-task-depth`, `cloud/20261026-p3q-late-screen` —
  older, returned, unmerged.
- `integ/p3w-rsi`, an integration branch of three RSI merges being prepared on the owner's machine as you are
  fired. It may land on main before or after you start.
- **The five siblings fired beside you in this wave** (`cloud/20261027-p3x-…`), with the files each owns:
  - `p3x-gate-suite-last` — `scripts/gate.sh` only (`machine_checks`'s phase order).
  - `p3x-x86-two-reds` — `crates/theseus-core/src/term/tests_idle.rs`,
    `crates/theseus-core/src/judge/tests_content.rs`, and whatever production file the cause turns out to be.
  - `p3x-revert-known-after-fetch` — `crates/theseus-core/src/rsi/revert.rs` and a tests file beside it.
  - `p3x-not-this-docs` — `crates/theseus/AGENTS.md`, `crates/theseus-core/AGENTS.md`,
    `crates/theseus-discord/AGENTS.md` (documentation only, no code).
  - `p3x-self-halt-resume` — `crates/theseus-discord/src/runtime/self_note.rs` and its tests,
    `crates/theseus-core/src/rsi/` resume path, possibly `crates/theseus-protocol/`.
  - `p3x-find-family-sticks` — `crates/theseus-core/src/turn/defer_step.rs`,
    `crates/theseus-core/src/tools_defer.rs`.

**Keep your diff inside the files your task names.** Where you genuinely must touch a shared file, keep the
edit to the smallest possible line count, and say so prominently in the report so the maintainer can resolve
it in seconds.

## Your base, and the fact that it moves

Your branch was created from `origin/main` at **`5342a01c057f429005157ff68386d72c30ad55c4`** ("Merge
cloud/20261027-p3t-rsi-fetch-bound into main (theseus-6vlr4): the door's git fetch is bounded, killed and
reaped"), and every `file:line` in this prompt was verified against that commit on 2026-10-27 at about 08:4x.

**Main is moving today.** Several merges are queued on the owner's machine, so by the time you are fired your
base may be one or two merges behind `origin/main`. That is normal and expected; it is not a problem and it is
not a reason to stop. Do not rebase — the maintainer merges. Just read `git log origin/main` at your start,
say what landed after your base, and check that your task still describes the tree.

## Your branch

`cloud/20261027-p3x-find-family-sticks`, already created from `main` at `5342a01c` with this file as its first commit. Your commits go
on top of it. Push with `git push origin HEAD:refs/heads/cloud/20261027-p3x-find-family-sticks` at every green point, and push the
mandatory empty start commit as soon as your first build finishes.

## Keel

Run `python3 scripts/keel-guard.py` before each commit and paste its findings into the report. **You cannot
sign, so you cannot ack.** If the guard flags anything, put that change in a commit of its own whose message
says why, and list it in the report under **Keel findings expected** (rule, path, why) for the maintainer to
take to the owner, together with the exact sentence you would want the owner quoted on in the `Keel:` trailer.

## Deadline

**Six hours by the clock, waits included.** If you reach it mid-step, leave that step out, push what is proved,
and report what you found. **Fewer steps, proved and green, is a better row than every step half-done.**

## The report

Write `CLOUD_REPORT.md` at the repository root, commit it as the branch's **last** commit with subject
`cloud report (not for main)`, and push. The run log cuts every message at about 2,000 characters, so the
report on the branch is the only full record that survives.

Say explicitly in the report:

- **what you checked in `git log origin/main` before you started**, and anything in this prompt that had
  already landed;
- **the `file:line` of everything this prompt cited and what it actually was at your base — name every
  citation this prompt got wrong.** This is step 0 and it is not optional. A correction is worth more to the
  maintainer than a fast commit;
- **the red you saw at the base for every behaviour change**, with the test's output — the red-first evidence;
- `cargo nextest list` output proving your test filters select your tests (a filter that matches nothing runs
  zero tests and exits 0, which looks green and is not; `test(=bare_fn)` with no module path matches nothing);
- the planted reverts: the plant, the failing output, the restore, `git status` after each;
- the keel guard's output for every commit, and a **Keel findings expected** section;
- the gate's result, naming each failing test and why — distinguishing the known-failure set above from
  anything that is actually yours;
- **integrator notes: exactly which of your commits are for main and which are not**;
- which steps you did not do, and why;
- any `OWNER CALL:` you are leaving open, stated precisely.

Your final message is short (under 1,200 characters): the line `CLOUD REPORT COMPLETE`, then the branch, its
head commit, one line per step, and the gate's result.

## Hard rules

- **Check `origin/main` first for what already landed.** Do not redo merged work.
- **Re-verify every `file:line` in this prompt against your own clone before acting on it, and report any
  citation that is wrong.** This is your first step. On 2026-10-26 a row was fired on a description that was
  later proved wrong in two places, and because there is no way to steer a running cloud session, the only
  thing that could have saved it was its own re-verification.
- **Red-first.** A test that would pass before your change is not a test of your change. The report must carry
  the test failing at the base and passing on the change, for every behaviour you change.
- **Write no person's name anywhere** — not in code, comments, docs, fixtures, goldens, commit messages or
  your report. No `/home/<user>/` path and no organisation name either. Where a person is meant, write "the
  owner" (the handle `zeroaltitude` only where an identity string is genuinely needed), "collaborator" for
  anyone else. The AI assistant may appear only as "Tabitha/Claude", and your commit trailer names only
  Claude. A sibling row in the last wave shipped real people's names in test fixtures and it was a blocking
  join fix: grep your own diff for names before every commit so it does not happen twice.
- **The owner's Discord DM task flow is LOCKED.**
  `crates/theseus-discord/src/golden/dm_flow.txt` must be byte-identical to its state at the base commit. If
  your change moves even one character of it — including a cost figure — **stop, change nothing further, and
  say so in your report**. Do not regenerate the golden. A row has been blocked for over a day on exactly one
  cost-footer character; do not create a second one. If your task would require changing a DM task message,
  that is a STOP-and-report, not a change you make.
- **No AWS, at all.** No instance, no spot, no `aws` call. **No cloud spend beyond this session: run no
  benchmark, make no model call, and do no install.**
- Commit subject style: `turn: what changed (theseus-hhxqn)`. End every message with
  `Co-Authored-By: Claude (cloud session, claude-sonnet-5-5) <noreply@anthropic.com>`. Commit without `-S`.
- Never push to `main`, never force-push, never delete a branch, open no pull request.
- No new dependencies: `Cargo.lock` must not gain a package.
- Don't edit the spec (`docs/spec/`), `docs/status.md`, the README, `docs/benchmarks.md`, `docs/benchmarks/`
  or `docs/design/`.
- **`CLOUD_TASK.md` and `CLOUD_REPORT.md` must never reach main.** Leave this file in place; the maintainer
  drops both at the merge.
- Don't commit half a step. If time runs out mid-step, leave it out and report what you found.
- Call no connector and no MCP tool. Use only Bash, Read, Write, Edit, Glob, Grep. **Never search for or
  install a skill, and never run a web search**: either taints the session and every write is then refused.
- End your report with the line `CLOUD REPORT COMPLETE`.
