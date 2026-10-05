<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/ and the benchmark adapters under bench/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

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
- theseus-core's `tests_output::the_cores_output_matches_its_golden` fails under this VM's UTC clock, because two wake lines carry the offset's sign (theseus-ig6n). The gate line below sets `TZ=America/Phoenix` for it; set the same when you run the suite yourself, and commit only the golden lines your change moves.
- Under load, these timing tests can fail, and none is on the flaky list: rerun it alone, and name it in the report.
  - theseus-store's `tests_pages::a_filtered_page_equals_the_scans_answer` can pass nextest's 120 s kill (theseus-hohs);
  - theseus-core's `term::tests::python3s_repl_computes_on_the_screen` (theseus-1n2y),
    `tests_m3::parallel::a_cancel_during_a_batch_leaves_no_call_dispatched` (theseus-t2yb),
    `tests_lsp_edits::the_block_adds_no_frame` (theseus-xx6w), and `tests_route`'s tests that slow Jev's verdict past
    route's wait;
  - theseus-discord's `tests_outbox::a_cards_settle_waits_for_its_create_and_edits_it_by_id` (theseus-0bq1);
  - theseusd's `a_stop_of_three_jobs_that_ignore_sigterm_takes_one_grace_and_holds_no_worker` (theseus-1n5f).
- One negative assertion failed once under load and is a finding, not a flake: theseus-kernel's `tree`
  `the_deadline_stops_the_whole_tree_too` (theseus-g11i; a batch-7 session fixes it). If it fails for you, keep its
  output, name it in the report, and don't retry it away.

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine (today: the kernel sim's put-back check, theseus-81ig; theseusd's stop on a SIGTERM or a SIGINT; a clean stop that closes the index). Any other failure is yours to explain.

**What main holds.** You clone main at e6f90af3 or later, with store format 16. These joined main last night, so your clone has them:
- route.v1 on the turn path (`[routing]`, detours and switches; store format 15), and its fix: a routed session keeps its move only while route.v1 acts for it, and `profile.use` moves it;
- the live rerank (rerank's own breaker, a bounded live wait, per-item grading);
- security.v3's live notices and their brake;
- replay, audit and backfill of judgments;
- the ladder (`pack.mode`, `mode_for`, `ask_mode`, `pack_arm`, `theseus packs`, the cockpit's Ladder panel);
- the tools smalls (categorize on an empty ontology, the language servers' `start_on_edit`, AWS hands' runaway mode);
- the tasks smalls (layer 1 for the owner's tasks only, `task.change_expired`, place warnings, a check's restricted view; store format 16);
- gliding with the place rule (`channel.post` and `channel.read`), and the cockpit Ship view's gentle roll;
- the Linux lanes: a job's wrapper and its L0 command spawn without a fork; each L0 job in a cgroup of its own where the daemon's is delegated, with a process cap and an exact stop; one sync per job completion; background passes that wait while the machine is busy; language servers that watch their own files;
- the bench fixes: the bench asks for its model's whole output cap, `[policy] private_addresses`, and `[model.retries]` (a transient failure's bounded retry inside its turn);
- rust-analyzer's compiler errors after an edit, a saved document waiting for the check after its save;
- the refusal fallback: a refused request goes once to its model's fallback (Sonnet 5.5's is Sonnet 5), and every surface says so.

**Other changes in flight.** About twenty other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

Under review now, merging into `main` one at a time over the next hours (your clone may hold some of them):
- consolidation, `Synthesis` nodes and the `+synthesis` arm;
- FSRS-6 retention and the `+retention` arm;
- activation's adjacency and the `+activation` arm;
- tiering: stubs and the bounded heat cache;
- situations, the precedence line and testimony;
- claim leases, the task board, `/tasks` and the cockpit's task graph;
- the cockpit's Budgets, Ledger and Policy tabs;
- the learning loop: labeled examples into packs, thresholds re-fit from calibration.

On the owner's machine:
- files: every surface accepts any file, and PDFs reach the model (the Discord attachments, `AttachmentContent`, `fs.read`, `web/fetch.rs`, the compiler's document blocks);
- speed: a confidence bar per routing mode, Jev's connection kept warm, and a reply that reaches Discord before its settle's sync (`config/routing.rs`, `turn/route_step.rs`, the judge client, the Discord outbox and streaming);
- context-honesty: the persona says how a request is assembled, and recall's notes say the harness chose them (`turn.rs`'s `PERSONA`, `recall/render.rs`).

These are other cloud sessions like you, each on its own branch:
- durability-fixes: the durability tender's missing-key read, only synced frames shipped, and two restore tests;
- aws-fixes: runaway mode after a raised line, unknown `[policy.aws]` keys in health, a network test, the hand image's build checks;
- push-once: one serialization per notification for every watcher, and every operator act's `by`;
- kernel-fixes: the deadline's stop of a whole tree, a late completion's one row, a restart's in-process calls, nested locks;
- sim2: kernel-sim drives `/stop`, wakes and the outbox under crashes;
- route-gaps: a routed session follows its base, and the route review's debts;
- bench-efficiency, bench-async and bench-recall: the benchmark program's efficiency track and two new benchmarks;
- proc-steps: `proc.run`'s steps, and `fs.patch`'s recount;
- health-words: health's web and 1-hour cache words in the CLI, `binary` in the cockpit, and disk crossings;
- prove-wire-in: `theseus judge prove` from the ledger.

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (16 on main today; the changes under review take it to 17, 18 or 19, and the files change one more), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,709), crates/theseus-core/src/compiler.rs (2,560) and crates/theseus-discord/src/render.rs (2,928 of 2,930); near it, crates/theseus-discord/src/runtime.rs (3,453 of 3,500), crates/theseus/src/render.rs (3,062 of 3,100), crates/theseus-kernel/src/kernel.rs (2,992 of 3,030), crates/theseus-core/src/turn.rs (3,472 of 3,523) and crates/theseus-core/src/config.rs (2,855 of 2,910). Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).
- **Python under bench/** imports only the standard library, except where the Harbor adapter already imports Harbor, and the gate doesn't run its tests: run them yourself before each commit (bench/README.md says how), and say so in the report.

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone. A commit that changes only Python, Markdown or task files under bench/ changes nothing the gate builds (its one read there is `bench/theseus-bench.toml`, in theseusd's bench_profile test: leave that file as it is). For such a commit, bench/'s suites, as your task names them, are the gate; run `scripts/gate.sh` itself before your first commit and before your last.

The gate's shape phase fails a Rust file over its line ceiling in scripts/long-files.txt, and one it doesn't list past 2,500 lines. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261005-bench-recall`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- No new dependencies: Cargo.lock and the package-lock.json files must not gain a package, and bench/'s Python gains no import beyond the standard library and the Harbor its adapter already uses. If the right design needs one, say so in the report instead.
- Use invented names in fixtures, tests, and commits (AGENTS.md, Item 16). Write nothing about the owner, his machine, his accounts, or anyone else.
- Don't edit the spec, docs/status.md, the README, docs/benchmarks.md, or docs/design/. The maintainer writes those at review. Where a doc should change, say what and where in the report.
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
## Your task: the incidental-recall bench: out-of-the-way facts along a scripted progression of work, probed at set distances in every arm, with its curve and half-life (theseus-7gir.17)

Branch: `cloud/20261005-bench-recall`. Every commit's subject carries its issue id. Deadline for the report: 5 hours
after you start.

**Background.** The memory exam (crates/theseus-exam) is Theseus-only, with no salience levels, progression
distances or indirect probes. This bench replays one scripted, realistic progression of work identically to each
arm (three sessions of about 200 turns, topic shifts, two or three compactions, days between sessions), facts seeded
at known points at two saliences:
**incidental**, said once in passing (a port in a tool's output, a preference in an aside, a path in an error
message, a decision in a side remark), and **central**, the topic of the moment. Probes come at set distances (a few
turns on, after a topic shift, a compaction, a session boundary, days, a supersession: the new value is right), of
three kinds: **direct** ("which port did the relay use?"), **indirect** (a task that silently needs the fact) and
**abstention** (never stated: not to be invented). Each arm uses its own memory: Theseus its memory pass, recall
and compaction roots (a scratch daemon's `[memory] arm` makes each memory arm a sub-arm); Claude Code its compaction
and memory files; OpenClaw, a third arm not yet built (leave it room), memory search and its wiki.

**Read first:** crates/theseus-exam's AGENTS.md, `item.rs`, `generate.rs`, `rng.rs` (SplitMix64), `daemon.rs` (a
scratch daemon's config, start, wait and clean stop), `check.rs`; crates/theseus's `main.rs` (`sessions open`,
`ask -s`, `--json`, `history`, `memory recalled`, `ledger`); theseus-sim's `fake_model.rs`; bench/harbor/ (the
profile, the tests' stand-ins).

**What changed, and what the code says** (the code wins; report each difference):
- **Families to seed from:** the exam's `tool_output`, `superseded`, `time`, `distractor`, `needs_nothing`, for
  their kinds and checks only. exam-v2.toml and the crate's docs carry names from a real history: copy none, invent
  every name (speakers too) from the generator's own lists, and test that none appears in exam-v2.toml.
- **Arms are config** (`[memory] mode = "live"`, `arm`), never a turn's field: main has `none`, `bm25`, `baseline`;
  `+synthesis`, `+retention` and `+activation` are being built, so take the arm as a string. A CLI session has no
  target, so it is private: recall draws on every earlier session.
- **No clock override in either CLI:** days are dated text (each session opens with its date); say what that
  measures and what it can't.
- **Compactions:** Theseus compacts when its context fills (a scratch `[catalog."<model>"] context_window = 32000`
  brings that near the script's marks); Claude Code near its window, or on `/compact` through `-p --resume` if print
  mode runs it (check; say). Measure distance by where each arm actually compacted (history, ledger, session log),
  never by the marks alone.

**What to build,** each a green commit:
1. **The format** (`bench/recall/progression.py`): sessions (label, date); turns (the user's text); facts (id,
   value, salience, family, the turn stating it, its source file or command, what supersedes it); probes (fact,
   kind, distance bucket, check). From it, a scratch workspace per arm, identical for each: logs, configs, scripts
   failing with a path in their error, where the incidental facts live and the turns' work reaches them ("run
   ./status.sh: did the build pass?").
2. **The generator** (`bench/recall/generate.py --seed N --size smoke|full --out DIR`): deterministic (SplitMix64 in
   Python, never `random`); topic blocks, two saliences, supersessions, distractors near the abstention probes;
   buckets stratified so each bucket × salience × kind cell has enough probes at full size (say how many); each
   fact probed once (a second probe reminds the arm). It prints turns, facts, probes, and estimated tokens and
   dollars at the catalog's price for the model named: the budget before a live run. Smoke: two sessions of about
   15 turns, a compaction mark, each probe kind; full: three of about 200.
3. **The drivers** (`bench/recall/drive.py --arm theseus|claude-code`), each arm with fresh state in a scratch
   directory, keeping each turn's raw answer, tokens, dollars and latency. **Theseus:** a scratch `theseusd` with
   `theseus-index` beside it, configured as daemon.rs does (the bench profile's `env:ANTHROPIC_API_KEY`; Discord,
   the web UI and the MCP server off; the index on; `[memory]` from `--memory-arm`), a session per progression
   session, each turn `theseus --json ask -s <id> -`, stopped leaving nothing running. **Claude Code:** `claude -p
   --output-format json`, a scratch `CLAUDE_CONFIG_DIR`, the workspace as its working directory, `--resume <session
   id>` for each next turn, a new session at each boundary, allowed tools doing Theseus's same work (check `claude
   --help`). Check each fact reached the arm (its text in the transcript): an undelivered fact's probe is
   excluded and counted, never a miss. Neither arm's tools are confined to the workspace on a bare host: the README
   says to run live in a throwaway container or VM.
4. **The scorer** (`bench/recall/score.py RUN_DIR... --out DIR`, standard library): deterministic checks as
   check.rs's (the value present, the stale one absent; an abstention gives no value of the asked kind and admits
   it); an indirect probe scored by its effect in the workspace (a file the task writes), never the arm's words; the
   curve, accuracy per arm by bucket and salience, with turns and tokens since the fact; its half-life, where
   accuracy falls to half its nearest bucket's, interpolated ("not reached" if never); confident-wrong (a wrong
   value, no hedge); stale answers; cites where and when; cost and latency per probe. `report.md`, and a hand-written
   SVG.
5. **`bench/recall/README.md`:** the format, the commands, each score, each arm's memory.

If time runs out, leave tuning the full size, then citations, and say so.

**Your suites** (the preamble's rule for bench/): `python3 -m unittest discover -s bench/recall`, on the standard
library alone.

**Proof, offline** (no key): determinism (the smoke's digest pinned, the full twice equal, another seed different);
stratification (every cell filled, each fact before its probe and probed once); no generated name in exam-v2.toml;
the scorer over fixture answers against numbers worked by hand (a known curve's half-life, confident-wrong, stale,
citation, abstention); the Claude Code driver against a stand-in `claude` on PATH (session ids carried, a boundary
opening a new one); the Theseus driver end to end on this workspace's binaries and `theseus-sim fake-model --rules`
over the smoke (skipped when missing; nothing left running). Planted reverts, each naming the test it breaks: a fact
probed twice; an invented value scored an abstention; distance from the marks, not an arm's actual compaction;
`random` in place of SplitMix64.

**The live check is the maintainer's** (an Anthropic key, Claude Code, a release build of `theseus`, `theseusd` and
`theseus-index`, a throwaway container):
1. `python3 bench/recall/generate.py --seed 7 --size smoke --out /tmp/rc-smoke`: its counts and cost.
2. `python3 bench/recall/drive.py --arm theseus --memory-arm baseline --bin-dir target/release --model
   anthropic/claude-sonnet-5-5 --progression /tmp/rc-smoke --out /tmp/rc-th`, then `--arm claude-code --out
   /tmp/rc-cc`: every probe answered, each fact's delivery confirmed, no daemon left after.
3. `python3 bench/recall/score.py /tmp/rc-th /tmp/rc-cc --out /tmp/rc-report`: each arm's curve and scores.
4. With both smokes clean, `--size full` the same way.

**Leave alone:** crates/theseus-exam (read only); bench-efficiency beside you (bench/harbor/, `bench/report/`,
bench/README.md: say in your report what bench/README.md should link); bench-async (`bench/async/`);
`bench/theseus-bench.toml`; recall's arms and theseus-memory; all Rust.
