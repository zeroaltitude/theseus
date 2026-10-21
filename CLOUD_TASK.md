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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-job-truths-r2`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

This task was written against an older `main`. Your branch starts from `main` at `1d092ded` (2026-10-20). Where the
text below names `bbc65530`, a store format, line ceilings or "work in flight tonight", this section is right and the
text below is stale. Line numbers quoted below may have moved: search by name.

**Store format on your base: 47** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). If your row adds a
field to a stored record, bump it to 48 and add the old layout's literal sample to `crates/theseus-core/src/tests_layouts.rs`
as AGENTS.md says. Several rows in this wave may bump to 48 at once; the maintainer renumbers at the join, so never
write the number as a literal anywhere but the constant and its tests.

**What landed since `bbc65530`** (67 merges; `git log --first-parent bbc65530..HEAD` lists them). The ones most likely
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
- Tests made hermetic on fast hosts (`76237a96`), load-flaky tests ordered by events (`a2f8d276`).

**Line ceilings on your base** (`scripts/long-files.txt`; length / ceiling). Tight ones first:
- `crates/theseus-discord/src/runtime.rs` 3,499 / 3,500 (one line of room)
- `crates/theseus-protocol/src/lib.rs` 2,730 / 2,733 (three lines of room)
- `crates/theseus/src/render.rs` 3,115 / 3,128
- `crates/theseus-discord/src/render.rs` 2,976 / 3,009
- `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030
- `crates/theseus-core/src/tests_m3.rs` 7,979 / 8,050
- `crates/theseus-core/src/turn.rs` 3,404 / 3,525
- `crates/theseus-index/src/vectors.rs` 2,561 / 2,650

Put new code in new files (a module of its own, one `mod` line in the parent). New wire types go in a protocol module
file of their own, re-exported with one line. If you truly must pass a ceiling, raise it in `scripts/long-files.txt`
with the reason in the same commit, as its header says; never pass it silently (the gate's shape step fails).

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
- On the maintainer's machine: task questions and reports (`owner_ask`, `turn/join_step.rs`, `task.close`), the TUI
  chat (`crates/theseus-tui`), herdr (`contrib/herdr`).

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

---
## Your task: job-truths, a job that `pkill -f` cannot hit by accident, and tool results that say what happened (theseus-53ujg.2)

**A relaunch.** A first session on this row (branch `cloud/20261019-job-truths`) ended without pushing any code: only
its task commit is there. Ignore that branch and start fresh. Push each proved sub-step at once, so the work survives.

Branch: `cloud/20261021-job-truths-r2`. Every commit's subject carries `theseus-53ujg.2`. Deadline for the report: 5
hours after you start.

**Why.** Reading the v1 benchmark's failed trials found tool results that misled the model, each a thing a person's
own work hits:
- **A job kills itself.** A job's command text sits on two command lines Theseus owns: the wrapper's
  (`theseusd job-wrapper --spool … --correlation-id … -- bash -c '<the whole command>'`, from
  `theseus-kernel/src/job.rs` `spawn_detached`: `cmd.arg("--").args(&args.argv)`) and the shell's (`bash -c '<the
  whole command>'`). So a command like `pkill -f qemu-system; qemu-system-i386 …` matches its own wrapper and shell:
  the wrapper takes the SIGTERM, stops its tree (the new process never starts), and the result reads
  `[outcome unknown: the harness could not establish whether this finished]`, while the daemon records a lost wrapper
  as a security event (`rpc/driver.rs` `wrapper_signalled`). In the v1 run, 4 of the 6 "a job's wrapper was killed
  before it reported" warnings were exactly this (`pkill -f qemu-system` twice, `pkill -f test.py`,
  `pkill -f convert_masks`). `pkill -f <my dev server>` is an everyday command.
- **"Outcome unknown" says nothing about why,** though the daemon knows the signal (it logs it).
- **`fs_read` on an unrecognised binary says "fs_read returns text only".** A QEMU screendump (PPM data under a
  `.png` name) got that answer, and the model gave up looking at the screen; `fs_read` does show PNG, JPEG, GIF and
  WebP. A binary's kind (a PIE executable, a PPM image of 720×400, a SQLite database) is the useful fact.
- **`file_read` on an absolute path** answered "this session has no file named /tmp/t.png: no file has been given to
  it yet", with no word that `fs_read` reads files on disk.

**Risk: medium.** Item 1 changes how every job's command is executed. The policy gate, the ledger, the job's record and
everything a person sees keep the command exactly as today; only what `/proc/<pid>/cmdline` shows changes.

**Prerequisite: `main`.**

## Your row
| # | Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|---|
| F2 | `job-truths` | theseus-53ujg.2 | the command off every Theseus-owned command line (L0 and L1); the unknown outcome names its signal; `fs_read` names a binary's kind; `file_read`'s miss on a path names `fs_read`; the test helpers that found a job by its text | `theseus-kernel/src/{job,job_l1}.rs`, `theseus-core/src/toolrun/job.rs` (the `ResultStatus::Unknown` arm only), `theseus-core/src/rpc/driver.rs` (`wrapper_signalled`'s reason), `theseus-tools/src/{fs,magic}.rs` (`magic.rs` new), `theseus-core/src/file_read.rs`, `crates/theseusd/tests/{gone_jobs,job_handles}.rs`, `tests_term.rs` | M |

## What exists (on your base; lines at `bbc65530`)
- `theseus-tools/src/proc.rs` `shell_argv` (near :117) is the only builder of `["bash", "-c", line]`; `fit.rs:55-59`
  maps the `bash` alias to it; `steps` builds none. The policy gate judges that argv. **Change neither `shell_argv`
  nor `fit.rs`.**
- `theseus-kernel/src/job.rs`: `spawn_detached` (near :134) puts `-- <argv>` on the wrapper's command line; the
  wrapper's `parse` (near :1488) reads it; `spawn_l0` (near :691) execs it in L0. `job_l1.rs` (near :178) builds L1's
  `Spec::new(args.argv.clone(), cwd)`, sent to the sandbox's init over `SPEC_FD` (`theseus-sandbox/src/init.rs:76`).
- **Relearn** finds a wrapper by argv[1] == `job-wrapper` and its `--correlation-id` (`job.rs:871-883`,
  `children.rs:323`; `--spool` is not read). It runs only on an exec onto a changed note (`children.rs:249`); a cold
  restart uses the spool's pid (`spool.rs:392`). Tests: `kernel/tests/children.rs:210`, `spool.rs:626`.
- `rpc/driver.rs` `wrapper_signalled` (near :500-530) marks the action unknown with reason `wrapper_lost`
  (`mark_unknown`, near :516); the kernel's producer becomes `reconciler:{reason}` (`kernel.rs:2517`); the result's
  `Unknown` arm (`toolrun/job.rs` near :866) prints one sentence for every unknown today.
- `theseus-tools/src/fs.rs`: `is_binary` (near :24-26) is "a NUL in the first 8 KiB"; the binary branch is near :369-381.
- **Test helpers that find a job by its text in `/proc/*/cmdline`:** `theseusd/tests/gone_jobs.rs:235` `job_of` (it
  parses the argv after `--`; used at :266, :271 with `starts_with("sh -c")`, and :341), `job_handles.rs:166`
  `holding(…)`, `tests_term.rs:477` `lives_marked`.

## What to build
1. **The command off the command line.**
   - **`WrapperArgs.argv` stays the job's argv end to end, out of band:** `spawn_detached` writes it to a file in the
     job's spool directory (0600) before the spawn instead of after `--`, and the wrapper's `parse` reads it from there
     (accept an empty `--`). The wrapper's own command line keeps `job-wrapper` and `--correlation-id`.
   - **The wrapper rewrites a shell line only at exec:** in `spawn_l0` and in L1's `Spec::new`, an argv of the form
     `[bash|sh, -c, line, args…]` runs as a fixed runner, e.g. `bash -c '<runner>'` with the line in an environment
     variable that the runner reads and unsets before `eval`, so neither `ps` nor `pgrep -f` sees the text and the
     line's children do not inherit the variable. (A `bash /dev/fd/N` form also works if the descriptor is not
     close-on-exec at the exec.) Every other argv runs as today.
   - **Accepted differences, listed in the report:** an error reads `bash: eval: line N:`, the shell stays as the
     parent of a lone command, `BASH_EXECUTION_STRING` differs, and the one-argument limit becomes 128 KiB minus the
     variable's name. Nothing else may change: test it (below).
   - **`--redact` stays on the wrapper's command line:** it carries secret *names* only (`WrapperArgs.redact`,
     `job.rs:51-55`; the values reach the wrapper as its environment). `parse_wrapper_args` still accepts a non-empty
     `-- <argv>` (the kernel's own tests use it); an empty `--` means the argv is in the spool file.
   - **The test helpers** that found a job by its text (`gone_jobs.rs` `job_of`, `job_handles.rs` `holding`,
     `tests_term.rs` `lives_marked`) find it through the spool's argv file instead. A helper that would now match
     nothing is a weakened test: each must still find its job (show it).
2. **An unknown outcome says why.** `wrapper_signalled` passes the reason `wrapper_lost:SIGTERM` (the signal's name)
   to `mark_unknown`; the `Unknown` arm reads `completion.producer` and, for a lost wrapper, says
   `[outcome unknown: the job's wrapper was ended by SIGTERM before it reported]`. Every other unknown keeps today's
   sentence. No new stored field.
3. **`fs_read` names a binary's kind.** A new `theseus-tools/src/magic.rs` identifies common binaries by their first
   bytes: ELF (class, machine; `ET_EXEC` is an executable, `ET_DYN` **with a `PT_INTERP` program header is a (PIE)
   executable**, `ET_DYN` without one a shared object, `ET_REL` an object), Mach-O, PE, the binary netpbm images P4-P7
   (with width and height; P1-P3 are text and stay text), BMP and TIFF (size where the header gives it), ICO, SQLite
   databases, gzip, xz, zstd, bzip2, 7z, WebAssembly, Java class files and Python bytecode. The binary branch of
   `fs_read` says what it is, as a fact: `/tmp/s.png is a PPM image, 720×400, 864,015 bytes (fs_read shows PNG, JPEG,
   GIF and WebP images)`; `/app/polyglot/main is an ELF 64-bit PIE executable, x86-64, 412,608 bytes`. Unknown data
   keeps today's sentence. The result's metadata gains `kind`.
4. **`file_read`'s miss on a path.** When the name has a `/` and no session file matches, the miss adds that
   `fs_read` reads files on disk by path (`theseus-core/src/file_read.rs`, the two miss messages near :202-209).

## Tests (each with a planted revert; the first is the key test)
- **`a_jobs_own_pkill_f_does_not_kill_it`** (L0, in `crates/theseusd/tests/`, against a scratch daemon that spawns the
  real `theseusd job-wrapper`: core tests' `InlineLauncher` runs the wrapper in-process, so Plant A cannot fail there;
  the same holds for `no_theseus_process_shows_a_jobs_text` and `an_unknown_outcome_names_the_signal`): a job whose command is
  `sleep 30.<nonce> & pkill -f 'sleep 30.<nonce>'; wait; echo survived` (`<nonce>` six random digits a run, so no
  other process on the machine can match) ends with exit code 0 and `survived` in its output. Today the pattern
  matches the job's own shell and wrapper, so the job dies. Plant A: put the argv back after `--` on the wrapper's
  command line (and read it from there). Plant B: exec a shell line as `bash -c <line>` again. Each plant alone must
  fail this test.
- `no_theseus_process_shows_a_jobs_text`: while a job runs `sleep 5; true # token-<random>` (two commands, so a shell
  stays), no `/proc/*/cmdline` of the wrapper or of the shell contains the token. Plant B must fail it.
- `a_line_keeps_its_semantics` (one table test): an exit code (`exit 3`), `set -e` stopping a line, a heredoc, a
  multi-line line, quotes and `$`-expansion, `$0` and `$1` with extra args, `&` plus `wait`, stdin being empty as
  today, and the runner's variable absent from the line's children's environment.
- L1: the key test against plant B only (the wrapper is outside the job's pid namespace), plus the semantics table.
  Give the maintainer the exact commands if the cloud VM cannot run L1.
- `a_restarted_daemon_still_finds_a_running_wrapper`: relearn on an exec and a cold restart's spool pid, both after
  the change (reuse `kernel/tests/children.rs:210` and `spool.rs:626`'s tests; say which).
- `an_unknown_outcome_names_the_signal`: a wrapper sent SIGTERM from outside before it reports gives a result naming
  SIGTERM; a deadline's unknown keeps today's sentence. Plant: drop the signal from the header.
- `fs_read_names_a_binary_kind`: a PPM (P6) fixture with a zero byte in its first 8 KiB and its size, a PIE ELF
  executable (`ET_DYN` with `PT_INTERP`), a shared object, a SQLite file, and unknown bytes keeping today's sentence.
  Plant: return today's sentence for every binary.
- `file_read_miss_on_a_path_names_fs_read`, and a plain name's miss unchanged. Plant: drop the clause.

## Where main is

The preamble above may describe an older `main` (its store format and its list of changes in flight are from an earlier
batch). Where the preamble and this task disagree about `main`, this task is right: `main` is at `bbc65530` (2026-10-18
21:32 MST, the v1 cut's last join fix, tagged as v1) or later, with **store format 45**. The `file:line` references
below are at `bbc65530`; on your base a line may have moved a little: search by name.

Line ceilings at `bbc65530` (`scripts/long-files.txt`, length / ceiling): `crates/theseus-core/src/turn.rs` 3,384 /
3,525, `crates/theseus-protocol/src/lib.rs` 2,706 / 2,733, `crates/theseus/src/render.rs` 3,108 / 3,128,
`crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030, `crates/theseus-discord/src/runtime.rs` 3,497 / 3,500 (full: never
add a line there). **`crates/theseus-core/src/toolrun.rs` is 2,489 lines and unlisted**: a file past 2,500 needs a
ceiling entry, which this batch may not add, and another row of this batch adds one registration line there. So **add
no line to `toolrun.rs`** in these rows. Never raise a ceiling. Put new code in new modules; in a long file add only the
`mod` line, the field or the call.

## v1 patch1: the harness-effectiveness rows

Theseus's v1 is cut and benchmarked. On Terminal-Bench 2.0 it is level with Claude Code on the same model and effort,
and it is cheaper per solved task. The rows below come from reading every failed trial of that run for causes a real
user would also hit; none is tuned to a benchmark task. The owner's words (Discord DM, 2026-10-19 00:21, verbatim
excerpt): "Want to add a full analysis of last benchmark results, which were very generally encouraging, and do another
pass of analysis to see what we could do to be a more effective harness. Analyze, plan and implement in v1 patch1."
His standing rules that bind these rows: benchmarks are a lens, never a target ("we // do // want to take those
occasions to make our tools faster, more useful, more sophisticated ... but we won't ever tune our stuff just for the
benchmark itself", 2026-10-07); minimal prompting: the API contract, then tool shape, then descriptions, then result
metadata, and model-facing prose only as plain facts (2026-10-09); features ship on and measured, with a named
rollback (2026-10-06).

| Row | Slug | Beads | Prerequisite | Area |
|---|---|---|---|---|
| F1 | `turn-end-facts` | theseus-53ujg.1 | main | `toolrun/job/beside.rs` (new, declared by `mod beside;` in `toolrun/job.rs`), `toolrun/job.rs` (the `mod` line, one call at `launch`, one line where the cap and uncapped lines are joined, one line in `remove_job_output`), `rewind.rs` (a turn's written paths, in memory), `turn/nudge_step.rs` (a shared `may_continue` and `Nudge::sent`), `turn/end_note_step.rs` (new), `turn.rs` (the `mod` line, one call in the stop chain, the field and its result), `fact/turn.rs` + `fact/mod.rs` + `theseus-protocol/src/ledger.rs` (one fact), `theseus-protocol/src/files_left.rs` (new; one `pub mod` line and one field in `lib.rs`), `theseus-protocol/src/nudge.rs` (`harness_lines`), `config` (`[model] end_note`), the template |
| F2 | `job-truths` | theseus-53ujg.2 | main | `theseus-kernel/src/job.rs` (`spawn_detached`, the wrapper's `parse`, `spawn_l0`: argv out of band, the runner at exec), `theseus-kernel/src/job_l1.rs` (the runner at exec for L1), `toolrun/job.rs` (the `ResultStatus::Unknown` arm only), `rpc/driver.rs` (`wrapper_signalled`'s reason), `theseus-tools/src/fs.rs` (the binary branch) + `theseus-tools/src/magic.rs` (new), `theseus-core/src/file_read.rs` (the miss message), and the test helpers that find a job by its text in `/proc` (`theseusd/tests/gone_jobs.rs`, `job_handles.rs`, `tests_term.rs`) |
| F3 | `effort-on-evidence` | theseus-53ujg.3 | **the local routing lane's join** (its conversation-turn checkpoints are this row's base) | `turn/checkpoint_step.rs` (the effort-only raise, the `Off` state), `routing/effort.rs` (`Runs.floor`, `decide`), `turn/route_step.rs` (`route_effort`), `turn/effort_step.rs` (`stick`'s exception for a checkpoint) |
| F4 | `bench-measure` | theseus-53ujg.4 | main | `bench/theseus-bench-routed.toml`, `bench/harbor/{theseus_agent,theseus_bench,plan,run}.py` (a plan's effort bound through `--ak`), `bench/recall/drive.py`, `bench/async/{async_agents,driver}.py`, their tests, `crates/theseusd/tests/bench_profile_routed.rs` and the `resolved_profiles` golden (they read the profile), `bench/README.md` |

F1, F2 and F4 run in parallel now. F3 launches once the local routing lane has joined `main`. Joins are plain
merges, one at a time: F2, then F1, then F4, then F3.

**Other work in flight tonight** (stay out of their areas): the Google Workspace rows (`gw-*`: `crates/theseus-google/`,
`theseus-core/src/google/`, one registration line in `toolrun.rs`, `broker.rs`, `data_gate.rs`, the route's data step);
the speed rows (`speed-*`: the census and first request, the turn's path before the request leaves, idle memory and
binary size); a local routing lane (the provider-origin allowlist for private data, routing hard tasks away from the
cheapest model, Jev's warnings: `routing/data.rs`, `data_gate.rs`, `catalog.rs`, `routing.rs`, `turn/route_step.rs`,
`turn/checkpoint_step.rs`, `turn/data_step.rs`, `config/privacy.rs`); a Discord test hang; the README; a voice fix.

**Contracts between these rows** (so each join is mechanical):
1. **Stay in your area.** If the change you want sits in another row's area, leave it and name it in the report. Do
   not refactor code you do not need, and do not reformat files you only read.
2. **`toolrun/job.rs` is shared by F1 and F2 at different places.** F2 edits only the `ResultStatus::Unknown` arm of
   the header in `job_result_of`; F1 adds its `mod beside;` line, one call where a job is launched (`launch`) and its line only where the
   cap and uncapped lines are joined (the `format!("{header}{}{}", …)` near the end of the header's build), and one
   line in `remove_job_output` that drops its entry. Neither touches the other's.
3. **One harness line a turn.** F1's end note and the existing stop nudge (`turn/nudge_step.rs`, `NUDGE_TEXT`) never
   both fire in one turn: if the nudge sent its line, F1's note stands down for that turn (its facts still reach the turn result and the footer).
4. **The store format:** none of these rows changes a store layout. A new ledger fact kind (as `turn.nudged` is one)
   is fine. F1's per-turn facts ride on the turn result (protocol) and its own fact row; if a row finds it must store something, it says why in the
   report and bumps by one from its base (the joiner renumbers).
5. **No new model-facing instruction.** Any text the model reads (a result line, an end note) states facts: what
   exists, what changed, what happened. It never tells the model what to do, never says "must", and never gives
   examples. Tool descriptions change only to state a tool's contract.

## Out of scope for every row

The owner's config, his daemon and his machine. Any benchmark run (the next run measures these rows; a row never
does). `docs/status.md`, the README, `docs/design/` and the spec (the maintainer writes those; say what should change).
Routing choices of which model a turn starts on (the routing lane's), and Jev's packs' prompts. If your work shows one
of these is needed, write it in the report and stop there.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **On, measured, with a named rollback.** The behaviour ships on by default. Where the row names a switch, it is the
  rollback: a test shows that with it off the old behaviour is byte-for-byte back.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). A new model tool is deferred by
  default (these rows add none). FAST: nothing new on a plain turn (a plain turn stays 4 frames,
  `tests_m3::a_plain_turn_stays_within_its_frame_budget`, and the gate's turn bench must not move), nothing on the path before a request leaves, and nothing on the start path grows with history.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. The row's **key test** (named first under
  Tests) must be shown failing on its planted revert. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It
  must be empty unless this task names keel findings.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package. A store format change only
  as the shared contracts say.
- **Secrets and names.** Tool inputs and outputs are not scrubbed at rest: hold raw values in memory only, and run
  anything that leaves (a fact, a report, a push) through `core.tools.scrubber`. No person's name anywhere in the repo,
  in commit messages or in the report: write "the owner". Fixture paths and names are invented.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed,
  what you could not run in the cloud (L1 needs a non-root daemon with a job cgroup: about 33 L1 tests fail on this VM,
  so say which of your tests you could run and give the maintainer the exact commands for the rest).
