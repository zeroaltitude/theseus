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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail4-onto`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

Your branch starts from `main` at `8260dbab` (2026-10-22: release stage 2, which landed the threads lane rounds 1-3b and
the China-origin chain rounds 1-4 on top of install #69's `29f5dcad`); that is your base. Line numbers quoted below are
at `8260dbab` but may still be off by a few: search by name.

**Store format on your base: 49** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 50 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows may bump at once; the maintainer renumbers at the join, so never write the number as a
literal anywhere but the constant and its tests. Prefer serde-default fields and META keys: most rows need no bump.

**What landed since `29f5dcad`** (`git log --first-parent 29f5dcad..HEAD` lists them):
- `bb265d6c` + `8260dbab`: the threads lane (a place's home splits into threads, thread routing with the directory rule
  and the which-thread ask, the place digest, thread moves and growth, the protocol nodes split).
- `38743708`: the China-origin chain (the data rule's open view, the open request, the tool screen, the draft review),
  with join fixes `c0d35275` (a session's data state is store format 49) through `30d6472e`.

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 (one line of room: add nothing)
- `crates/theseus/src/render.rs` 3,126 / 3,128
- `crates/theseus-discord/src/render.rs` 3,003 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-protocol/src/lib.rs` 2,556 / 2,733 (room here, but the release branch counts other lanes' lines too:
  add at most two)

An unlisted file stays at 2,500 lines or fewer. Put new code in new files (a module of its own, one `mod` line in the
parent). Never raise a ceiling yourself (the keel guard flags it). If a line you truly need will not fit, leave the file
one line over and say so in your report's first section with the file and the reason.

**Work in flight beside you** (other sessions and lanes, not yet on `main`; stay out of their files where you can, and
where you can't, keep your edit small and additive so the joins merge):
- `integ/tail3` (`e29be1ed`) and `integ/threads` round 4 (`a52cac7b`) are being joined into the next release stage.
  Your task's "Rules for this row" names the files of theirs that sit near yours, and how to read their diff
  (`git diff origin/main origin/integ/tail3 -- <file>`). Read that diff before you edit those files.
- The rows beside you in this wave, and the files each owns, are named in your task's own "Rules for this row" and
  "Not in this row" sections. Those win over anything older.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

## Your task: tail4-onto, the sweep renames a stale category, clusters `other` answers across names, and Jev names a minted kind (theseus-63tfk.1)

Branch: `cloud/20261021-tail4-onto`. Every commit's subject carries `theseus-63tfk.1`. Deadline for the report: 4 hours
after you start.

**Why.** The nightly ontology sweep can merge and split categories (`same_thing.v1`, `two_things.v1`, theseus-nzzz4)
and mint a kind from three sessions that said `other` about one shared mined name (theseus-81zx7). Three things the
design asks for are still missing:
1. it never **renames** a category whose name has gone stale;
2. it never clusters `other` answers whose mined names **differ but mean the same kind**;
3. it names a minted kind by its **most shared** mined name instead of asking Jev.

**The design** (`docs/design/jev-ontology-growth.md` §3.1 and §3.2; that file is not on your base, so its words are
here):
- §3.1: "**Split, merge, rename** are not decided on the hot path. They need more than one message of evidence, so they
  belong to the sweep: near-duplicate pairs get a `same_thing` Noul and merge ...; a large category gets a `two_things`
  Noul ...; **a rename is a Choice among mined names with the old name kept in the description. Each is act, tell,
  undo.**"
- §3.2: "An `other` at the act band is recorded with the message's mined names. The nightly sweep clusters the day's
  `other` answers (**the same mined name, or a `same_kind` Noul over pairs**). A cluster of three or more sessions mints
  a kind ..., **named from the cluster's names (a Choice again)**, with its first categories the cluster's. Act, tell
  with a card, undo (retire the kind and its categories ...)."
- Jev cannot write a name: every name is a Choice among names code mined (titles, project and repository names,
  paths, hosts, capitalised terms), plus `none`. Jev's answer acts only at the act band.

**Risk: medium-low.** Night-time writes by the sweep, each undoable; no turn's path changes.

**Prerequisite: base `origin/main` at `8260dbab`.** Your branch joins the new lane `integ/tail4`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail4-onto` | theseus-63tfk.1 | rename by Choice; `same_kind` Noul over `other` pairs; the minted kind's name by Choice | three pack files or questions in `crates/theseus-judge/packs/` (see 1-3), their builders in `crates/theseus-judge/src/builders/sweep.rs` (or a new `builders/sweep_names.rs`), the embedded-pack list and its count test; `crates/theseus-core/src/judge/onto/judged.rs` (the new asks beside `pairs`/`splits`; or a new `judge/onto/renamed.rs` if judged.rs would pass ~800 lines), `judge/onto/mint.rs` (clusters across names, the name Choice), `judge/onto/digest.rs` (the morning note's lines); `crates/theseus-ontology/src/` (a rename `Step` with undo, if none exists: `sweep.rs`, `ontology.rs`, their tests); `crates/theseus-core/src/tests_onto_sweep.rs`; `crates/theseus-core/AGENTS.md`'s ontology passage | L |

## What exists (at `8260dbab`; search by name)
- **The sweep's Jev part.** `crates/theseus-core/src/judge/onto/judged.rs` (557 lines): the module doc (packs
  `SAME_PACK` = `same_thing.v1`, `SPLIT_PACK` = `two_things.v1`; money: `[ontology] sweep_usd_per_day`, the META mark
  `MARK` = `onto.sweep.judged`, `paid`, `left`, `going`; not asked again for `AGAIN_MS` 30 days), `onto_sweep_judged`,
  `pairs`, `splits`, `acts(j)` (act band), `noul(j, question, yes)`, `named(j, question, names)` (a Choice among
  names), `sweep_category`. The deterministic plan: `crates/theseus-ontology/src/sweep.rs` (`sweep_plan`,
  `plan_merges`, `plan_pair_merge`, `fold`). The orchestration: `judge/onto/sweep.rs` (`onto_sweep`,
  `onto_sweep_after_serving`).
- **The kind mint.** `crates/theseus-core/src/judge/onto/mint.rs`: `WINDOW_MS` (7 days), `CLUSTER` (3 sessions),
  `PER_RUN` (3), `Said { session, names, confidence }`, `said_of(rows)` (the `other` steps of `onto.filed` rows),
  `kind_name`, `plan(o, said, now_ms, write)` (clusters by one shared folded name; "the most shared name leads"),
  `mint`, `onto_mint`. Undo: `Ontology::undo` retires the kind with its category; a name a kind already lists in
  `examples` is not clustered again.
- **Packs.** `crates/theseus-judge/packs/same_thing.v1.toml` and `two_things.v1.toml` (copy their shape: questions,
  bands, Noul/Choice), builders `crates/theseus-judge/src/builders/sweep.rs` (`SameThingInput`, `TwoThingsInput`,
  `SweepCategory`), registered in `builders.rs` (near :52 and :1273). The embedded packs are counted by a loader test
  (`crates/theseus-judge/src/pack.rs` near :954 and the theseusd judge test: the nzzz4 joins `4ee0da32` and `c38be060`
  had to bump both: search `embedded packs` and `25`); a new pack means those counts and lists change (that is a
  listing, not a weakened test; say so).
- **Categories.** `crates/theseus-ontology/src/category.rs` `Category { id, name, description?, parent, .. }`;
  `crates/theseus-ontology/src/ontology.rs`; the undo machinery and `Origin` (`Origin::Sweep` or what the sweep's writes
  use). The morning note lists the sweep's writes (`judge/onto/digest.rs`) with the id `theseus ontology undo` takes.

## What to build
1. **Rename.** For a category a machine wrote whose name no longer fits (pick the evidence rule and say it; for
   example: none of its member sessions' mined names folds to its name, and one mined name is shared by at least half
   of them), the sweep asks a Choice among the members' mined names plus `keep` (a new pack `rename.v1`, or a question
   added to an existing sweep pack if packs version that way: follow how `two_things` names children). Jev's pick at
   the act band renames it; the old name goes into the description ("was: <old>"); act, tell (the morning note), undo
   (restores the old name and description). Same money, mark and `AGAIN_MS` rules as the other two asks.
2. **`same_kind`.** Before `mint::plan` clusters, pairs of `other` answers (by session) whose folded names differ are
   asked a `same_kind` Noul (a new pack, cheap: two sessions' mined names and titles), at most N pairs a run (a
   constant, say 20, like `PAIRS_MAX`); a yes at the act band joins the two into one cluster. Clusters still need
   `CLUSTER` sessions. Never asked twice for the same pair within `AGAIN_MS`.
3. **The kind's name.** A cluster that will mint is asked a Choice among its names (the names its sessions mined) plus
   `none`; Jev's pick at the act band names the kind; `none` or below the band falls back to today's most-shared rule,
   and the report row says which happened.
4. Docs: two or three sentences in `crates/theseus-core/AGENTS.md`'s ontology/sweep passage; the module docs of the
   files you change.

## Tests (each with a planted revert; the first is the key test)
- **`a_stale_category_is_renamed_by_jevs_pick_and_undone`** (`tests_onto_sweep.rs`, a fake judge answering the
  Choice): renamed, old name in the description, undo restores both. Plant: skip the write: it fails.
- `a_rename_below_the_act_band_writes_nothing`. Plant: drop the band check.
- **`two_other_names_jev_calls_one_kind_cluster_and_mint`**: three sessions, two folded names, a `same_kind` yes ->
  one kind minted. Plant: skip the Noul's join.
- `a_minted_kind_is_named_by_jevs_choice`, and `with_no_pick_the_most_shared_name_names_it`. Plant: always the
  most-shared name.
- `the_sweeps_new_asks_stop_at_the_days_money`: the cap holds across the three new asks (a fake price). Plant: skip
  `going()` in one of them.
- The existing sweep, mint and judged tests pass unchanged. `cargo nextest run -p theseus-ontology -p theseus-judge`
  and `cargo nextest run -p theseus-core onto` must pass, then `cargo nextest run -p theseusd judge`.

## Not in this row
- The in-turn `onto.v1` pack, its questions and its filing (the hot path): unchanged.
- Any judge budget or `sweep_usd_per_day` default: unchanged (a raise is a keel finding).
- The cockpit's Ontology view: unchanged (if it should show renames, a follow-up).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- **Near you, not yet on main:** `integ/tail3` changed `crates/theseus-core/src/judge/onto/judged.rs` (a doc line,
  theseus-94782). Read `git diff origin/main origin/integ/tail3 -- crates/theseus-core/src/judge/onto/` first.
- **Rows beside you on `main` 8260dbab, joining `integ/tail4` with you:** tail4-voice, tail4-discord, tail4-tui,
  tail4-route, tail4-tests (core `rpc/tests.rs` test helpers). Stay out of their files.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Dependencies.** None new. **Store format** 49. A category's rename is a change of existing fields; a pack's mark is
  META; this row should add no stored field. If an undo record needs a new serde-default field, that is no bump; a
  required one is a bump to 50 with the old layout's sample in `crates/theseus-core/src/tests_layouts.rs`.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture names are invented.
- Read `crates/theseus-core/AGENTS.md` (ontology, judge), `crates/theseus-judge/AGENTS.md` and
  `crates/theseus-ontology/AGENTS.md` (if present) first. Run `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: what changed; the rename's evidence rule; each new pack (name, questions, bands) and why it is its
own pack or a question in another; the money and not-again rules across all five asks; each test and its plant with
the failing line; the pack-count changes; the keel guard's output; the store format; a `docs/status.md` sentence for
the integrator; follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
