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
- Under load, these timing tests can fail. The flaky list in .config/nextest.toml is empty (since d279767f), so none retries: rerun a failing one alone, and name it in the report. Batch 10's branches, reviewed or in review and joining main one at a time, fix those marked ‡; batch 11's sibling session daemon-flakes (below) works on those marked §.
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

**What main holds.** You clone main at 8d9a9c9b or later, with **store format 23**. Your clone has v1's milestones, everything batches 8 to 11 joined, and since 2026-10-06 evening:
- the AWS mints (secret-bearing credential mints, the output-shape rule), health-imported (health counts the owner's sessions apart from the 21,779 imported ones), bench-fair and recall-fair (the bench arms comparable), bench-rows;
- cancel-fast (a job's cancel settles on pidfds in 2 frames; the lifecycle bench's cancel budget is now 100 ms with a fewest-frames check), daemon-flakes (a stopping daemon begins no retry or model call: `turn/stopping_step.rs`), daemon-stops (a `--stdio` daemon stops on `shutdown`, a restart in place closes the store, an import or erase stops between frames, the upgrade's directory sync), route-wait (route.v1 asks alone; Jev's connections kept warm);
- imported-walk (the lists skip the import's key range) and sink-fast (the judge's writes kept off the turns);
- the sdgl fix (`Spool::lingering` leaves a marker with no pid; a young marker is never a dead wrapper's) and v18k (a wrapper that will linger marks itself before its report; a batch's next step waits on the pid file);
- the vjn7 fix (redb's cache bounded at 16 MiB in theseus-store's index.rs; every stop phase and the index's close logged at info: `startup::stop_phase`);
- the cockpit's phase 2 (lanes B, A, C, D: the new Ship with the living sea, the watch's six plates over `action.list`'s `unsettled` option, every chart with a table view, the repairs, the daylight mode) and the Ship's flare from the ledger (cockpit/src/ship/flares.ts);
- docs v0.84 (the spec's Items 216 to 243).

**Other changes in flight.** These are being built on the owner's machine now, each on its own branch, and join `main` one at a time while you work. Stay out of their areas unless your task needs it; if it does, keep the edit small and say so in the report.
- fy0i: `theseus_store::blocking` spends the task's cooperative budget after its wait (theseus-store's blocking module);
- mce3 and fts6: an erase's absolute count written with every frame (core import/write.rs); no frame after the stop's last checkpoint (theseusd main.rs, core startup and the store's close; theseusd tests/config_copy.rs);
- sink follow-ups: tests for sink-fast's guarantees, a sweep of `put_many`'s temporaries, the judged turn bench's per-run lines (core judge/sink.rs, blobs.rs; theseus-sim perf);
- the AWS secrets lane: `SecretBearing::WhenPresent`, the STS session mints at posture approve, seven fail-closed rows (theseus-aws-catalog classify.rs and tables; core aws/secret.rs, policy.rs);
- the import's topics into the ontology (core import/, the ontology's records, a CLI command);
- cockpit phase 3 (the Ship's motion and sound; the watch and the pages), the context explorer and the Benchmarks tab (cockpit/src/ views and new additive protocol methods).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (23 on main today, since soul-import), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. No batch-12 task expects one: if yours needs it, say why. Others may bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling:** read scripts/long-files.txt and `wc -l` the files you touch before you add to them; a Rust file the list doesn't name fails past 2,500 lines. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tool-run logic in crates/theseus-core/src/toolrun/, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).
- **Python under bench/** imports only the standard library, except where the Harbor adapter already imports Harbor, and the gate doesn't run its tests: run them yourself before each commit (bench/README.md says how), and say so in the report. Harbor 0.23.0 needs Python 3.12 or later: where python3 is older, run Harbor's tests in a venv (`python3.12 -m venv /tmp/hvenv && /tmp/hvenv/bin/pip install harbor==0.23.0`); bench/async's Harbor tests also need `ASYNC_HARBOR=1` there. Every benchmark run gets a report in docs/benchmarks/: the maintainer writes those, and no task here runs a benchmark.

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone. A commit that changes only Python, Markdown or task files under bench/ changes nothing the gate builds (its one read there is `bench/theseus-bench.toml`, in theseusd's bench_profile test: leave that file as it is unless your task names it). For such a commit, bench/'s suites, as your task names them, are the gate; run `scripts/gate.sh` itself before your first commit and before your last.

The gate's shape phase fails a Rust file over its line ceiling in scripts/long-files.txt, and one it doesn't list past 2,500 lines. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261009-people`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- No new dependencies: Cargo.lock and the package-lock.json files must not gain a package, and bench/'s Python gains no import beyond the standard library and the Harbor its adapter already uses. If the right design needs one, say so in the report instead.
- Use invented names in fixtures, tests, and commits (AGENTS.md, Item 16). Write nothing about the owner, the owner's machine, accounts, or anyone else.
- **Write no person's name anywhere**: not in code, comments, docs, fixtures, goldens, commit messages or your report. Where a person is meant, write "the owner" (the handle `zeroaltitude` where an identity string is needed), "collaborator" for anyone else; the AI assistant may appear only as "Tabitha/Claude", and your commit trailer names only Claude. The names lane cleared the tree at ea34457e: never bring a name back from memory, an old branch or a fixture you copy.
- Don't edit the spec, docs/status.md, the README, docs/benchmarks.md, docs/benchmarks/, or docs/design/. The maintainer writes those at review. Where a doc should change, say what and where in the report.
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
## Your task: people, the person category beyond Discord DMs, from the import and from Jev, shown in the cockpit (theseus-wy7y)

Branch: `cloud/20261009-people`. Every commit's subject carries `theseus-wy7y`. Deadline for the report: 3 hours after
you start.

**Main has moved since the preamble was written.** You clone main at `78b58b8a` or later. The preamble's "What main
holds" and "Other changes in flight" are out of date: everything they list has joined. The store's format on main is
**25**: if your change adds a field or a variant to a stored record, or a new record kind, bump `MANIFEST_FORMAT`
(crates/theseus-store/src/store.rs) to 26 with the old layout's sample in theseus-core's tests_layouts.rs, as AGENTS.md's
store version rule says, and say why in the report (other rows may bump it too; the maintainer renumbers at the join).
Read `scripts/long-files.txt` before you start: several long files are full or nearly (protocol lib.rs, golden.rs,
config.rs, compiler.rs, the Discord render.rs). Add no net line to a full one: new code goes in a module of its own.

**Rows in flight beside this one** (each on its own branch): keel-guard (scripts/gate.sh, scripts/keel-guard*),
self-ledger-kill (`[self]` config, an `rsi` module, `theseus self`), discord-silent-fold (the Discord binding's
render/silence). Stay out of their areas; name any shared-file edit in your report.

**Why this work exists.** The owner asked (2026-10-09): "Why aren't all the people we know of in the people category in
theseus's ontology?" and "the pulldowns for other ontology elements is awesome -- we should have all the people under
there", then "Yes, please build it!". On his daemon the ontology holds 161 categories: 158 topics, 2 channels and **one
person, his own DM**. The reason is by design: the kinds table makes `person` a `given` kind assigned only by the
transport, and `crates/theseus-core/src/ontology.rs` `place_category` makes a person only from a `discord:dm:<user>`
place at its first bind. On 2026-10-06 the owner's OpenClaw history was imported (tag `openclaw-2026-10`: 21,779
imported sessions, 115,346 nodes); the import made topics from the stored labels (`crates/theseus-core/src/import/
topics.rs`, `theseus import topics`) but could make no person. The people his agents worked with are in that history
as message authors and places (Slack DMs and channels, Discord), and, mostly, as names in text the agents read.

**What to build** (read `crates/theseus-ontology/src/{kind.rs,category.rs,ontology.rs}`, `crates/theseus-core/src/
ontology.rs`, `crates/theseus-core/src/import/topics.rs`, `crates/theseus-core/src/judge/categorize.rs`,
`crates/theseus-core/src/rpc/proposals.rs` and the cockpit's `cockpit/src/components/{OntologyParts.tsx,
Memberships.tsx}` first; follow their patterns and their doc-comment style):

1. **Person categories beyond the transport.** A person category may be made by the transport (as now), by the
   import, and by the operator (`theseus ontology person add NAME [--handle KIND:ID ...] [--desc ...]`, from a private
   place only, as `guide` is). Memberships of a session in a person may come from the transport (given, as now), the
   import (origin `import`) and the operator. Keep the place rule first: a shared place's walk still reads only its own
   place's given memberships. Decide whether `person` stays `given` with extra assigners or gains an interpreted side;
   say why in the report. Keep the per-session limit sensible for people (a session can involve several people).
2. **One person, many handles.** A person category carries its handles: `discord:<id>`, `slack:<id>`, `email:<addr>`,
   and display names. A DM's given person and an imported Slack person with the same human are merged by the
   operator (`theseus ontology person merge A B`, which keeps one id and moves memberships and guidance; reversible
   from the ledger's row) and, when a handle matches exactly, automatically. Never merge on a display name alone.
3. **People from the import, deterministic, no model call:** `theseus import people [TAG]` (the owner's act, like
   `import topics`: never at a start, a live session never touched, idempotent, cut in frames with the machine's
   quiet waited for, and taken back by the erase of its tag). It reads the imported sessions' stored records: message
   authors that are people (not the owner, agents, tools or outside text), and the places' peers (a Slack or Discord
   DM's other party, with its id). Each distinct person becomes one category with its handles and names, and each
   session that person spoke in or was the DM of gets a membership (origin `import`). Report counts with `--dry-run`.
4. **People from names in text, by Jev, as proposals:** extend `categorize.v1` (or add a sibling judgment with the same
   shape and its own pack) so an answer can propose a **person**: an existing person the session mentions, or a new
   person (name, and a short role line from the text). Proposals go through `ontology.proposals` like topics.
   Add **bulk answers**: `theseus ontology accept --kind person --min-confidence X` and the cockpit's equivalent
   (select all, accept). Also a backfill driver over an import tag (`theseus import people TAG --propose`) that runs the
   judgment over imported sessions under a spend cap (default $5, a flag to change it), paced by the machine's quiet,
   resumable; it writes proposals only, never memberships.
5. **The cockpit:** people appear in the ontology pulldowns and the Ontology view exactly as topics do: grouped,
   searchable, with each person's handles, session count and membership controls; a person's page lists their
   sessions; proposals for people sit beside topic proposals with select-all accept. Rebuild the bundle as the gate's
   cockpit step does.
6. **What a person record may hold:** a person's description and guidance hold role facts (what they do and own,
   their projects, channels, how work flows between them and the owner), never evaluations of them. Say so in the
   judgment's instructions and the CLI help for `person add` and `guide`.

**Tests:** the ontology crate (new assigners, handles, merge, refusals); the import (`import people` on a fixture
history: people and memberships, idempotence, the erase takes them back, a shared place still reads none); the
judgment (a fixture answer proposing a person; bulk accept); the cockpit (a component or snapshot test of the people
pulldown). **Use invented names only in tests, fixtures and docs** (no real person's name anywhere in the repo).
Update `theseusd example-config`'s template if you add a config key, with its template tests, and docs/ if a page
describes the ontology's kinds.

**Live check on this VM:** a scratch daemon, a small fixture import with three people across a Slack DM, a Discord DM
and a channel; run `theseus import people --dry-run`, then for real; show `theseus ontology categories` before and
after; merge two handles of one person; erase the tag and show the people gone. Paste the outputs in the report.

Run the gate before every commit; commit CLOUD_REPORT.md as the branch's last commit (what you built, the decisions
with reasons, the format bump if any, the live check's outputs, the commands the owner runs after the install:
`theseus import people openclaw-2026-10 --dry-run`, then without it, then `--propose`), and end with a short final
message starting `CLOUD REPORT COMPLETE`.
