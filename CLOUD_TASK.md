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
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261017-trim-names`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: trim-names, one right name for each thing: an everyday CLI screen with `help all` for the rest, the duplicate commands folded into noun-then-verb groups, "mode" used for the conversation mode only, config sections that say what they hold, and one spelling for each surface (theseus-rwpbg.10)

Branch: `cloud/20261017-trim-names`. Every commit's subject carries `theseus-rwpbg.10`. Deadline for the report: 4 hours after you start.

**The owner's decision (Discord DM, 2026-10-17 17:23:18 MST), verbatim:** "Your question 6: good recommendation. Use is
not yet habitual so also feel free to be aggressive about trimming aliases and changing patterns for consistency and
simplicity-- more important to be right than to be historically conservative this early". Question 6 was the trim
review's part 4 renames and folds (F1, F3, F5, F6, F10). So:
- **No compatibility aliases.** Each thing gets one right name. Where an old spelling is in a script, a test, a doc, a
  bench config or the herdr plugin, **fix the caller**; do not keep the old spelling as a clap alias or a serde alias.
- **An old config key fails the load with a clear message**, never a silent alias: `` `[model] max_loops_mode` was
  renamed to `[model] on_max_loops` ``. One table of (old, new) and one function that turns an unknown-field error for
  an old key into that message; one test per row of the table. This is a single-user system: the maintainer migrates
  the owner's live file at install, from your report's list.
- Where something stays two spellings for a real reason, say why in the report.

**This row is the exception to the shared section's "Out of scope" list** for exactly these items: CLI hiding, renames
and folds; the vocabulary renames named below; the config section renames named below. Everything else on that list
stays out of scope. **Jev pack prompt text is not yours** (`crates/theseus-judge/packs/*.toml` prompt wording and
anything a pack's prompt is built from): a pack keeps saying "mode" if it does today, and routing class values
(`trivial`, `sophisticated`, `deep_coding`, `routine_coding`) keep their spellings everywhere.

**Source:** the trim review's part 4, F1, F3, F5, F6, F10, and F8's template note. File:line references below are at
`0a44e1f1`; search by name.

**Prerequisite:** `main`. Builds now; joins **9th**, after trim-config-inherit (.7), trim-docs (.8) and
trim-keel-deletions (.9), and before trim-tests-dedup (.1, last). See "Overlaps" below.

## Your row
| # | Slug | Beads | Scope | Files | Tests | Size |
|---|---|---|---|---|---|---|
| 10 | `trim-names` | theseus-rwpbg.10 | A: CLI tiers + `help all`; B: the folds, noun then verb; C: vocabulary (task class, rung, `on_max_loops`, surface spellings); D: config sections (`[route_bench]`, `[persona_import]`, one `[cache]`, `[watch_for]`) with the renamed-key error; E: `self` naming in Rust | `crates/theseus/src/main.rs`, `crates/theseus/src/cmd*/`, the CLI's `answer.rs`, `remind.rs`, `work.rs`, `own_bench.rs`, `prefix_cmd.rs`, `voice_cmd.rs`, `notices_cmd.rs`, `watch_cmd.rs`; `crates/theseus/tests/` (help goldens); core `config.rs`, `config/{cache,route,routing,persona_settings,threads,watches,rsi}.rs`, `keep_warm*.rs`, the template; `approval.rs` / `standing.rs` surface names; `theseus packs` display; cockpit labels; `scripts/`, `bench/`, `contrib/herdr/`, `docs/` callers | every fold's old tests converted to the new spelling; a renamed-key test per table row; `help` and `help all` goldens; a test that every hidden command is listed by `help all` | L |

Do the parts in this order, one commit (or a few) each, each green before the next: **E, A, C, D, B** (smallest and
safest first, so a session that runs out of time still lands whole parts).

## E. `self` in Rust (F3; no user-visible change)
`[self]` is `Config::self_improve`, read from `config/rsi.rs`, struct `SelfConfig`: three names for one section. Make
the Rust side one name: module `config/self_improve.rs`, struct `SelfImproveConfig`, field `self_improve` (keep
`#[serde(rename = "self")]`; the TOML key stays `[self]`). The CLI command stays `theseus self`. `theseus-protocol`'s
`rsi.rs` and the `rsi/` store paths are wire and stored names: leave them. A pure rename: no plant needed.

## A. Tiers: the everyday screen and `help all` (F1 step 1)
`theseus help` shows exactly these, in this order: **ask chat tui history watch status tasks answer stop agenda digest
remind routine wake project thread places profile health**. Every other command gets `#[command(hide = true)]` and
still parses, runs and has its `--help`. Add `theseus help all`, which lists every command, hidden ones included, one
line each, grouped as the everyday screen then the rest. (Clap has no built-in for this: the smallest way is a `help`
handler that renders the full command list from `Cli::command()` with `hide` ignored. Keep `theseus <cmd> --help`
working for every command.) The everyday screen ends with one line: `theseus help all` lists every command.
- While you edit each `about` line, take internal issue/milestone ids out of it (`theseus-[a-z0-9]{4,5}(\.[0-9]+)?`,
  `(M6)`, `(M7 43a, 43b)`): trim-docs (.8) does the same on its branch, so doing it here makes the joiner's
  main.rs conflicts resolve to your side.
- Tests: `the_everyday_help_lists_exactly_the_everyday_commands` (plant: unhide one); `help_all_lists_every_command`
  built from the clap tree, so a new command can't be missed (plant: drop one from the renderer); the help goldens.

## B. The folds: noun then verb, one way to do each job (F1 step 2, F10)
The pattern after this row: **everyday actions are top-level verbs** (`ask`, `chat`, `answer`, `stop`, `cancel`,
`remind`); **everything else is a noun group with its verbs under it** (`wake pause`, `model replay`, `persona voice`,
`memory index`). No top-level verb acts on one noun's things alone (no `pause`, `resume`, `done`, `snooze` at the top).
For each fold: move the behaviour, convert the old command's tests to the new spelling (same assertions; a renamed test
is fine, a deleted one is not), delete the old variant and its dispatch, and fix every caller (`rg -n` over `scripts
bench contrib crates docs cockpit/src`, including strings inside tests and help text that tell the user what to type,
e.g. "`theseus confirm` answers", "`theseus snooze`").

| Old | New | Notes |
|---|---|---|
| `confirm` (list), `confirm ID` | `answer` (no args lists what waits), `answer ID approve` | keep confirm's behaviour of **following the turn it resumes** (pin it in a test first) |
| `done ID` | `answer ID done` | |
| `snooze ID [WHEN]` | `answer ID snooze [WHEN]` (a reminder that fired) and `wake snooze ID [WHEN]` (a pending wake) | one handler; `answer` gains the WHEN argument; today's `answer ID snooze_1h` form goes (one spelling) |
| `work` | `tasks --watch` | |
| `wakes [--session S]` | `wake list [--session S]` (and `wake` alone lists) | |
| `pause ID [--until]`, `resume ID` | `wake pause ID [--until]`, `wake resume ID` | |
| `cancel ID` | `cancel ID`: a task (and its jobs) or a wake, by the id's kind; also `wake cancel ID` (the same handler) | top-level `cancel` is hidden; `stop SESSION` stays the way to stop a conversation |
| `executions` | `sessions --state` (the state, turns, outstanding actions and budget columns) | `executions cancel` is removed: `cancel ID` covers tasks, `stop SESSION` conversations |
| `context [SESSION]` | `why --short [SESSION]` | |
| `voice …`, `notices rate …` | `persona voice …`, `persona rate up\|down [ID]` | |
| `turn replay`, `route report [--week]`, `prefix report`, `catalog` | `model replay`, `model bench [--week]`, `model prefix`, `model catalog` | **this row's choice of the model group** (the owner left it to us): the four are about models and their cost; `bench` matches `[route_bench]` and `own_bench.rs`; `profile` stays its own visible command (it is the everyday "which model am I on") |
| `index status`, `index search Q` | `memory index status`, `memory index search Q` | |
| `watches`, `watches add`, `watches rm` | `watch-for`, `watch-for add`, `watch-for rm` | `watch SESSION` stays the session follower; drop the "(`watch SESSION` is the session follower.)" note |

- Wire methods do not change (F10 is a freeze, not a fold): the CLI's new spellings call today's methods
  (`work.answer`, `wake.*`, `task.cancel`, …). Add one sentence to `crates/theseus-protocol/AGENTS.md` (or the crate's
  guide if it is CLAUDE.md): new clients use `work.answer`, `work.watch`, `session.*` and one cancel; the other answer,
  watch and cancel methods are kept for the cockpit and the TUI and get no new siblings.
- The herdr plugin (`contrib/herdr/`) calls `theseus watch --interactive` and `theseus herdr …`: check, and fix any call
  to a folded command. Leave `crates/theseus/src/herdr*` alone unless a fold breaks its build (contract 7).
- Each fold that changes behaviour (answer's list and WHEN, cancel by id kind, `sessions --state`, `why --short`) gets
  a test with a planted revert. A pure move (`persona voice`, `model *`, `memory index`, `watch-for`, `wake *`) needs
  only its converted tests: say so.

## C. One meaning per word (F5, F6)
In help text, CLI and cockpit output, docs, the template's comments and config keys (never in wire field names or
stored ledger values; no store format change):
- **routing mode → task class.** `[routing.modes.<name>]` becomes `[routing.classes.<name>]` (the class names are
  unchanged). Rust types may follow (`RoutingMode` → `TaskClass`) where it is a contained rename; say what you renamed.
  `why` and `profile` print "task class", never "mode", for it. "Interaction mode" goes too.
- **pack mode → rung** (shadow, canary, live: the ladder). `theseus packs` prints "rung"; any user-facing config key or
  CLI flag named `mode` for a pack becomes `rung`. The stored pack-state field and the wire field keep their names.
- **`max_loops_mode` → `on_max_loops`** (values `notice` / `end` unchanged), in `[model]`, `[profiles.*]`, the
  template, bench and sim configs (`bench/theseus-bench*.toml`, `bench/harbor/test_bench.py`, `bench/recall/`,
  `crates/theseus-sim`, `crates/theseusd/tests/`), and the crates' AGENTS.md notes. `rg -n max_loops_mode` must find
  only the renamed-key table and its test afterwards.
- **"mode" is then the conversation mode only** (ask, edits, plan, auto). `[memory] mode` and the spend-limit modes
  are not renamed in this row; list every remaining user-facing "mode" in the report with a one-line suggestion.
- **Surfaces:** the user-facing spelling is `terminal`, `cockpit`, `discord`, `voice`, `mcp` everywhere a user reads or
  types one (`standing add --surface`, approval rows as displayed, `status`/`why`/cockpit labels). `approval::Surface`
  stores `cli`/`web`: stored values stay, and one function maps a stored value to its display name (collapse
  `Surface::as_str`-for-display and `surface_name` into it). Input may also accept `cli`/`web` **only** if that is the
  same one function with no second path; otherwise accept the user spelling alone. Also qualify **place class**
  (private/shared) and **place kind** (cli/dm/channel/task/headless) wherever help text says just "place" for one of
  them.
- Tests: a renamed-key test for `max_loops_mode` and `[routing.modes]`; a display test per surface (stored `cli` shows
  `terminal`; plant: return the stored value); `theseus packs` output shows "rung".

## D. Config sections that say what they hold (F3, F8)
| Old | New |
|---|---|
| `[route]` (your own benchmark's sampler) | `[route_bench]` |
| `[persona] import_files`, `[persona] own_boost` | `[persona_import] import_files`, `own_boost` (if `own_boost` turns out not to be about imported persona files, say where it belongs and put it there) |
| `[model] cache_ttl` | `[cache] ttl` |
| `[model] keep_warm_hours` | `[cache] keep_warm_hours` |
| `[cache] keep_warm_minutes` | `[cache] keep_warm_every_minutes` |
| `[threads] keep_warm_hours` | `[cache] threads_keep_warm_hours` |
| `[cache] keep_warm_min_tokens`, `stub_min_tokens`, `stub_after_turns` | unchanged, in `[cache]` |
| `[watches]` | `[watch_for]` (to match the command; skip and say why if it is not a user-facing section) |
| `[profiles.*] cache_ttl`, `keep_warm_hours` | unchanged: a profile still overrides `[cache] ttl` / `keep_warm_hours` |

- A profile that leaves `cache_ttl` / `keep_warm_hours` unset takes `[cache]`'s value (today it takes `[model]`'s).
  Keep that rule in **one small function** next to `config/cache.rs`'s `inherit`, and do not otherwise touch `inherit`:
  trim-config-inherit (.7) generalises `inherit` to every profile field, and the joiner lays your two cache fields over
  its version. Give the merged form in the report.
- `[prefix]` (the prefix library) is not prompt-cache settings and stays.
- Every row of every rename table in this task is in the renamed-key table, and `load` of a file with the old key fails
  with "`<old>` was renamed to `<new>`" naming the file. Test each row (plant: drop one row and see its test fail).
- The template (`crates/theseus-core/config/theseus.example.toml`): rename, move the cache keys into one `[cache]`
  block with one comment saying a profile may override `cache_ttl` and `keep_warm_hours`; its tests
  (`example_template_uncommented_still_parses`, the section tests) pass. The owner-shaped fixtures
  (`crates/theseus-core/fixtures/owner_shape.toml` and friends) move to the new keys, and a test proves the old-shaped
  and new-shaped fixture resolve to the same `Config` (serialise both after load: equal).
- `theseusd config --sparse` and the full `theseusd config` print the new names.

## Overlaps (the joiner merges you 9th)
- **trim-config-inherit (.7)** edits `config.rs` (`ModelConfig`, `ProfileConfig`, profile resolution), `config/cache.rs`
  (`inherit`), `config/sparse.rs` and the template. Your `[cache]` move and the renames merge **on top of** its rule:
  keep your config.rs edits to field moves and the renamed-key table, and touch `inherit` and `sparse.rs` only where a
  renamed field name forces it.
- **trim-docs (.8)** writes `docs/cli.md` and `docs/glossary.md` (they are **not** on your base) describing today's
  names, and strips ids from main.rs's `about` lines. At the end of your work, `git fetch origin
  cloud/20261017-trim-docs`: if its branch has `docs/cli.md` / `docs/glossary.md`, write `CLOUD_DOCS.patch` (committed
  beside CLOUD_REPORT.md, dropped at the join like it) that brings those two files to your names (every fold, every
  rename, `help all`), as a patch the joiner applies after merging .8; check it applies with `git apply --check` on that
  branch. If the branch has no docs yet, the report gives the joiner the full old → new tables to apply by hand, and
  says to re-run trim-docs' cli.md test on the joined tree.
- **trim-keel-deletions (.9)** deletes retired config keys in `config.rs` (around `:241-370`, `:732-740`, `:830`,
  `:1336-1370`): your renamed-key table must not list a key .9 deletes (those are retired, not renamed).
- **trim-tests-dedup (.1)** joins after you and moves test helpers: where you convert a folded command's tests, keep
  them in the test file they are in today.
- **trim-tokens (.3) / trim-effort (.4)** touch `keep_warm.rs`: rename only the config field reads there, no
  restructuring.
- **The China-origin rows** (theseus-a1wvq.3-.13) are not written yet and will build on `[routing]`: name the renamed
  routing keys in the report so their writer uses the new ones.

## Keel (the joiner's part)
Renaming a test is fine (the guard records a renamed test). Do not delete a test or an assertion: convert a folded
command's tests to the new spelling. Renaming `max_loops_mode` touches a cap the keel guard watches in the template
and `fn default_*`: change no default value. If the guard reports a finding for a renamed cap key or a converted test,
paste it and give the exact `Keel:` trailer the guard asks for, quoting the owner's DM of 2026-10-17 17:23:18 MST
above. If the guard's own watched-key list names `max_loops_mode`, updating `scripts/keel-guard.py` (or its test) is a
keel-file change: make it, and give the trailer for it too. The joiner signs those commits with the owner's key.

## Report extras
- **The maintainer's migration list:** every old → new config key (a table, both TOML paths), and which of them the
  owner-shaped fixture used, so the maintainer can migrate `/etc/theseus/theseus.toml` at install. The owner's live
  file is known to use `[model] cache_ttl`, `[routing.modes.*]` tables and profile `cache_ttl` lines: say what each
  becomes.
- **The CLI change list:** every old command → new spelling, and every caller you fixed (file:line).
- What you hid, and the before/after line counts of `theseus help` and `theseus help all`.

## Proof nothing is lost (report section)
Every old command's behaviour is reachable under its new spelling (its converted tests, named); every old config key
fails with its rename message (the table's tests); the old- and new-shaped owner fixture resolve to the same `Config`;
the help goldens' diff; `rg` for each old spelling over `scripts bench contrib crates docs cockpit/src` shows only the
renamed-key table, the tests that pin the rename errors, and CHANGELOG-like history notes (list any you left and why).

## Where main is

The preamble above may describe an older `main` (its store format and its list of changes in flight are from an earlier
batch). Where the preamble and this task disagree about `main`, this task is right: `main` is at `0a44e1f1` (2026-10-17
16:46 MST) or later, with **store format 45**. (Eight sibling rows started from `432bbbeb`, 15:53, and trim-keel-deletions from `0f93aa81`, 16:34.) The trim review that this row comes from read `c6f62abf`, so every
`file:line` below is at `c6f62abf`; on your base a line may have moved a little, so search by name.

Line ceilings at `432bbbeb` (`scripts/long-files.txt`, length / ceiling): `crates/theseus-protocol/src/lib.rs` **2,732 /
2,733**, `crates/theseus-kernel/src/kernel.rs` **3,026 / 3,030**, `crates/theseus-discord/src/runtime.rs` 3,495 / 3,500,
`crates/theseus/src/render.rs` 3,116 / 3,128, `crates/theseus-core/src/turn.rs` 3,351 / 3,525,
`crates/theseus-core/src/tests_m3.rs` 7,996 / 8,050, `crates/theseus-discord/src/render.rs` 2,926 / 3,009,
`crates/theseus-index/src/vectors.rs` 2,564 / 2,650. A trim should shrink these files, never grow them; never raise a
ceiling in this row.

## The trim rows (ten siblings, built in parallel today from main)

The owner asked for a trim pass before the v1 feature review: "an efficiency and simplicity and elegance check ... what
we can trim without losing anything, both on total code, total binary size, etc." The review found the items; the owner
settled the ones that needed him. Ten cloud rows build them at once, each from `main`, and one joiner merges them in
this order:

| Join | Slug | Beads | Area |
|---|---|---|---|
| 1 | `trim-binary` | theseus-rwpbg.5 | scripts/build.sh, theseusd's web.rs, the cockpit's build, theseus-aws-catalog's Cargo.toml |
| 2 | `trim-docs` | theseus-rwpbg.8 | docs/glossary.md, docs/cli.md (new), the CLI's clap `about` lines, the CLI help goldens |
| 3 | `trim-security-v1` | theseus-rwpbg.6 | core judge/gate.rs, judge/notice.rs, judge/mod.rs, the security packs |
| 4 | `trim-config-inherit` | theseus-rwpbg.7 | core config.rs, config/cache.rs (`inherit`), config/sparse.rs, the template |
| 5 | `trim-effort` | theseus-rwpbg.4 | core turn/route_step.rs, compiler/effort.rs, keep_warm.rs (effort), catalog.rs (one field) |
| 6 | `trim-tokens` | theseus-rwpbg.3 | core tools_defer/, toolrun.rs's tools note, keep_warm (the window cap), turn/compile_step.rs, theseus-index tender |
| 7 | `trim-utils` | theseus-rwpbg.2 | many small sites: clip, dates, now_ms, dead pub items, protocol jsonrpc, core<->protocol mirrors |
| 8 | `trim-keel-deletions` | theseus-rwpbg.9 | the owner-approved deletions: kernel startup migration, retired config keys, seven pack TOMLs, gate.sh's probe clippy, dead code with its tests |
| 9 | `trim-names` | theseus-rwpbg.10 | the CLI's tiers and folds, "mode" and surface vocabulary, config section renames and one `[cache]`, `self` naming in Rust (this row) |
| 10 | `trim-tests-dedup` | theseus-rwpbg.1 | theseusd/tests/ and theseus-core test helpers |

**Contracts between the rows** (so each join is mechanical):
1. **Stay in your area.** If the change you want sits in another row's area, leave it and name it in the report. Do
   not refactor code you do not need to touch, and do not reformat files you only read.
2. **`config/cache.rs`'s `inherit` belongs to trim-config-inherit.** trim-tokens' keep-warm window cap and trim-effort's
   keep-warm effort live in `keep_warm.rs` / the catalog, never in `inherit` or `ProfileConfig`.
3. **`keep_warm.rs`:** trim-effort adds the last request's effort to what keep-warm replays; trim-tokens adds the
   break-even cap on the window. Each adds its own small function and one call; neither restructures the file.
4. **`catalog.rs`:** trim-effort adds one field (`per_message_effort`) to `CatalogEntry` and `CatalogRow`; trim-tokens
   reads the existing cache prices for its cap and adds no field. trim-utils deletes no catalog item.
5. **`compiler/effort.rs`:** trim-utils deletes the dead `without_messages` (`effort.rs:80`); trim-effort must not
   start calling it.
6. **Test helpers:** rows 1-7 add tests in their own `tests_<subject>.rs` files (or a crate's existing test module for
   their subject) and may use today's helpers; trim-tests-dedup moves helpers only in files that exist on `main`.
7. **Leave `crates/theseus-discord/` and `crates/theseus/src/herdr*` alone** (the Discord rows theseus-bh3t.1-.6 and
   theseus-ss36w are joining to main right now, and a herdr lane is open). Discord's copies of `clip` and the date
   math wait for a later pass; say in the report which copies you left.
8. **The China-origin rows** (theseus-a1wvq.3-.13) build later on `catalog.rs`, `config.rs`, routing and the turn path:
   keep your edits there small and additive.

## Out of scope for every trim row (owner questions still open)

(For trim-keel-deletions and trim-names, the task's own list of items overrides this section: see the top of the task.)

CLI command hiding, renames or folds; vocabulary renames (thread, mode, place, surface, section names); the sealed
holdout set; any deletion of a test (the review's part 1 §2b test-only APIs, §3 the old Jev packs, §4 jev-probe, §7 the
serving-daemon chain, §9 the startup migration and retired keys, §19 RPC removals); `PLAN_NOTE` and the plan tools; the
LTO profile. If your work shows one of these is needed, write it in the report and stop there.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. This is a trim: the diff should remove more than it
  adds, and every line you add should earn its place.
- **Nothing lost.** Behaviour, wire JSON and stored records stay the same; user-visible names change only as this task
  says. Where it says otherwise, pin the old output in a test first, then change it, and show the diff.
- No new model tool and no always-loaded text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens; the turn
  bench and the owner-config fixture test must pass). FAST: nothing new on a plain turn (a plain turn stays 5 frames).
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore); list them in
  the report. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test (convert a folded command's tests to its new spelling; renaming a test is fine), and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It
  must be empty unless this task names a keel finding (this row's possible ones are in its Keel section), and then the report gives
  the exact `Keel:` trailer line the guard asks for, for the joiner's signed commit.
- No store format change. No new dependency: Cargo.lock and the package-lock.json files gain no package (a version
  that drops a duplicate is fine; say so). Never loosen a test, budget or ceiling.
- No person's name anywhere in the repo, in commit messages, or in the report: write "the owner". If you edit a file
  that already carries a person's name, replace it with "the owner" in the lines you touch.
- Read the crates' AGENTS.md/CLAUDE.md you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed, what
  you could not run in the cloud. Add a section **"Proof nothing was lost"** listing, per change, the test or the
  before/after evidence.
