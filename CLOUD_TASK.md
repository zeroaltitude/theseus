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

**Other changes in flight.** Several other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

Merged into `main` tonight (your clone holds them if your base is main): turn-end facts, the shared tool catalog
(speed-send-path), the store's one blocking wait, the fleet's full-screen view, a self task's acts in its own tree,
the task's home thread, the owner's needs-you cards, and voice at the join (theseus-yf9ct) with its two join fixes.

Other cloud sessions like you, each on its own branch: the ontology in core (R2, reviewed, joining next); the job
truths; the release build's size (release-size); the timing flakes; the thread bench's threads arms. And this batch,
fired together: home-split (a place's home split into threads), onto-wire (the ontology's in-loop wire, stacked on
R2), voice-aec (voice layer 3, echo cancellation) and voice-jev (voice layer 4, a Jev judgment on unclear transcripts).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (main is at 45 as you clone it; this row should need no bump), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,696) and crates/theseus-core/src/compiler.rs (2,547); near it, crates/theseus-discord/src/runtime.rs, crates/theseus-discord/src/render.rs, crates/theseus/src/render.rs, crates/theseus-kernel/src/kernel.rs, and crates/theseus-core/src/turn.rs. Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

The gate's shape phase fails a file over its line ceiling in scripts/long-files.txt. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, on top of its first commit (CLOUD_TASK.md), whose base your task names. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261019-voice-jev`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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


## Your task: voice-jev, voice layer 4: one fast Jev judgment on a short or unclear transcript: speech to answer, Theseus's own echo, or noise (theseus-s3dwj)

Branch: `cloud/20261019-voice-jev`, based on `main` at `74a797ed`, which already holds the voice-at-the-join row
(theseus-yf9ct) and its two join fixes. Every commit's subject carries `theseus-s3dwj`. Deadline for the report: 5
hours after you start.

A sibling row, voice-aec (layer 3, echo cancellation on the audio), is built beside you from the same base. You work
on transcripts and the runtime; it works on audio. Keep out of each other's way: your code in new modules, and in
`engine.rs`, `songbird_io.rs` and `heard.rs` only the call, the field or the `mod` line.

**Why.** Layers 1-3 (the client's canceller, the transcript rules in `heard.rs`, the join guard; echo cancellation
soon) decide most utterances. What they can't decide well is a short or garbled transcript in Theseus's own speech,
just after it, or at the join: "yeah", "the", "is miss here" might be the owner, an echo, or someone else in the
room. Today the rules guess, and a wrong guess is a false turn (Theseus answers its own echo) or a missed one. Jev, a
fast classifier already used for judgments (`crates/theseus-judge`, about 180 ms a call), can decide from the
transcript, what Theseus just said and the timing, with no main-model call. This is a showcase of Jev in the loop.

A collaborator's voice agent judges self-echo by the share of heard words in its recent speech (60 % within a 1.2 s
tail); the voice-join review declined that rule because it calls "Yes, deploy it now." an echo of "Shall I deploy it
now?". Ambiguous cases like that one are exactly what this judgment is for.

## What to build (read crates/theseus-voice/AGENTS.md, heard.rs, engine.rs, tests/turns.rs, the runtime under crates/theseus-discord/src/runtime/voice*, and how theseus-core calls a judgment pack, e.g. classify.v3, first)
1. **When to ask.** Only when the rules would make a turn (or commit a barge-in's cut) from a transcript that is
   short (at most 3 words) or low-confidence (Deepgram's confidence, where the transcript carries it, below a
   threshold you choose and justify), and that began during Theseus's speech, within the echo tail after it
   (`ECHO_TAIL`), or in the join window. Everything else is decided by the rules exactly as today.
2. **The pack `heard.v1`** (`crates/theseus-judge/packs/heard.v1.toml`, with a builder and fake answers like the
   other packs): state = the transcript, Theseus's last spoken sentences (at most 3, the same strings `heard.rs`
   matches), the timing (start relative to Theseus's speech start and end, duration, overlap kind), whether the
   speaker is echo-prone, the place kind; one Choice: `addressed` (speech to answer), `echo` (Theseus's own voice),
   `other` (noise or another speaker). Scrubbed as the inbound state is.
3. **Acting on it.** `echo` or `other` at the act band (>= 0.80) -> no turn and no cut; a journal line says so with
   the judgment id. `addressed` at the act band -> a turn. Anything else, a Jev failure, or no answer within the
   deadline (`[voice] judge_ms`, default 250 ms, under the turn's start) -> the rules' verdict, exactly as today. The
   wait must not hold the floor or the audio: the utterance's handling is deferred by at most the deadline, nothing
   else. Each call is a `judge.call` row with its time and answer, so accuracy and latency can be read back later.
4. **Switch**: `[voice] judge_unclear = true` (the rollback; off -> byte for byte today's events and times). Both keys
   in the example-config template with their tests.
5. **A labelled set for later**: a small fixture (at least 20 invented cases: echoes, fragments, real short answers,
   room noise) and an eval test that runs the builder over it with the fake judge, so the maintainer can replay it
   against live Jev (`theseus-judge`'s eval path) and report accuracy. No live call here.

**No new dependency.** If the judge can't be reached from where the voice runtime decides without one, say so in the
report: a new dependency needs the owner's yes.

## Tests (each with a planted revert; the first is the key test)
- KEY, in virtual time through the seam (`tests/turns.rs` or a `tests_heard_judge.rs`): Theseus is speaking; a 1-word
  transcript that the rules make a turn arrives in the echo tail; the fake judge answers `echo` at 0.9 -> no turn, the
  journal line names the judgment. The same with `addressed` at 0.9 -> a turn. Plant: ignore the judge's answer
  (always take the rules' verdict) -> the `echo` case fails on a turn that should not exist.
- The deadline: the fake judge answers after 400 ms -> the rules' verdict, at the rules' time + at most 250 ms. Plant:
  wait without a deadline.
- A confirm-band answer (0.6) -> the rules' verdict.
- Not asked: a 6-word clear transcript, or a short one far from Theseus's speech: no `heard.v1` call at all. Plant:
  ask on every utterance -> the call count test fails.
- With `judge_unclear = false`, the key scenario's events and times equal today's exactly.
- The builder round-trips; the labelled fixture's eval test runs green with the fake judge.

## The checks for this row (these replace the preamble's gate line)
- `cargo fmt --all --check`; `bash scripts/shape.sh`; `cargo clippy -p theseus-voice -p theseus-discord -p theseus-judge -p theseus-core --all-targets -- -D warnings`.
- `TZ=America/Phoenix cargo nextest run -p theseus-voice -p theseus-discord -p theseus-judge`, and theseus-core's judge
  tests if you touch it; `tests_first_request` on the defaults; the example-config template tests.
- The full gate once before the report if time allows; if not, say so. theseus-voice builds libopus with cmake: the
  first cold build takes a few minutes.

## After the join (the maintainer's, not yours)
Replay the labelled set against live Jev and report accuracy; on a live speakers call, read the journal's `heard.v1`
lines (answers, latency) and count the turns saved. Give the exact commands in the report.

## Rules for every row of this batch

- **Model roles:** you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller, and make an old config key fail the load with
  "X was renamed to Y". Stored data stays readable.
- **On, with a named rollback.** The behaviour ships on by default. Where the row names a switch, it is the rollback: a
  test shows that with it off the old behaviour is byte for byte back.
- **Planted reverts, with notes for the reviewer.** Every behaviour change gets a test and a planted revert: break it,
  see the named test fail, restore the file and `touch` it, then `git status`. The row's **key test** (named first under
  Tests) must be shown failing on its plant. In the report, give each plant as a note the reviewer can replay: the file,
  the exact edit (the line before and after), and the test that failed with its failure line. A pure move needs no plant:
  say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. **Never edit a
  keel-guarded file** (`scripts/keel-guard.py`, `scripts/keel-signers`, `scripts/test_keel_guard.py`,
  `crates/theseus-core/src/toolrun/guarded.rs`, `crates/theseus-core/src/config/self_improve.rs`,
  `crates/theseus-core/src/rsi/sealed*`, `crates/theseus-exam/exam/`, and the first request's budgets in
  `crates/theseus-core/src/first_request.rs`): any change there needs a signed commit with a `Keel:` trailer quoting
  the owner's yes, which this session can't make. If your design needs one, stop at that point and put the exact change
  in the report.
- **Signing:** you can't sign here. Commit without `-S`; sign nothing. The maintainer re-signs at the join.
- **First-request budgets stay** (cli/task 8,000, dm 15,000 tokens; `tests_first_request.rs` must pass on the
  defaults). A plain turn stays 4 frames. Nothing new on the path before a request leaves.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package.
- **Secrets and names.** No person's name anywhere in the repo, in commit messages or in the report: write "the owner"
  and "collaborator". Fixture names, guild and channel ids, and paths are invented. Run anything that leaves through
  `core.tools.scrubber` as the code already does.
- **Docs in code.** Where your change makes a crate's or the cockpit's AGENTS.md wrong, fix those lines in the same
  commit, in plain words, in the file's existing style. Read the AGENTS.md/CLAUDE.md of every crate you touch first.
- **CLOUD_REPORT.md last.** It is the branch's last commit, as the preamble says, and your final message starts
  `CLOUD REPORT COMPLETE`. Keep it honest: what you ran, what passed, what you could not run here and the exact commands
  for the maintainer.

