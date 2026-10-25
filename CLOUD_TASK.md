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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261024-p3b-flux-follow`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of your base for this wave (it wins over anything below about main)

Your branch starts from `main` at `ac298666` (release 1.0.2, signed tag `release-1.0.2`; the release the owner runs
now). Fetch it and branch from `ac298666`. Line numbers below are at `ac298666` and may be off by a few: search by
name. Where the release preamble above disagrees with this note about the base, ceilings or the work in flight, this
note wins.

**Store format at your base: 51** (`const MANIFEST_FORMAT: u32 = 51;` in `crates/theseus-store/src/store.rs`, :688).
This row needs no store change. Do not bump it.

**Line ceilings.** Never raise one (`scripts/long-files.txt`; every file not listed there caps at 2,500 lines). At the
base: `crates/theseus-discord/src/runtime/voice.rs` 2,032; `crates/theseus-voice/src/engine.rs` 2,027;
`crates/theseus-voice/src/engine/flux_path.rs` 666; `crates/theseus-discord/src/runtime/voice/tests_flux.rs` 193.

**Your lane: `integ/p3-infra`.** Its other row, fired at about the same time on the same base: `p3b-voice-length`
(theseus-dgbcx: the built-in persona's voice line, the `FRAMING` const in `crates/theseus-discord/src/runtime/voice.rs`
:87-91, goldens that quote the persona, and an audit of voice caps). **Shared with you:** only
`crates/theseus-discord/src/runtime/voice.rs`, where it changes only `FRAMING` and its doc comment. You do not need to
edit `voice.rs` at all (your Discord-side test goes in `voice/tests_flux.rs`); if you must, keep well clear of
:80-95. Other lanes of this wave work in the cockpit, the ontology code and the trust marks; the wave-1 lane works in
recall, the catalog and the index. Stay out of all of them. Do not edit `docs/status.md` (the integrator writes it).

## Your task: p3b-flux-follow, three Low follow-ups from the Flux fix review (theseus-82rbm)

Branch: `cloud/20261024-p3b-flux-follow`.

Every commit subject carries `theseus-82rbm`. Deadline for the report: about 2 hours after you start.

These come from the review of the Flux fix row (theseus-v774k, joined for install #78). All three are Low: nothing
leaks today. Each is small; do them as three commits.

### 1. A Discord-side test that `Event::Streamed` books a trailing `speech.transcribed` row

- The pump's `Event::Streamed` arm is `crates/theseus-discord/src/runtime/voice.rs` :1090-1100. It builds a detail
  `{"speaker": …, "trailing": true}` and calls `book(.., SpeechKind::Transcribed, usage, Duration::ZERO, detail)`
  (`book` at ~:1332). It is documented at `crates/theseus-discord/AGENTS.md` ~:519-520. It has no Discord-side test.
- Mirror `a_flux_utterance_is_booked_at_fluxs_price` in `crates/theseus-discord/src/runtime/voice/tests_flux.rs`
  :132-155: it sends `Event::Utterance` through `pump(...)`, polls `rows(&core, &sid, "speech.transcribed")` (helper
  ~:124) and asserts the model and `cost_usd`. Its imports (:10) already include `Event`.
- Your test sends an `Event::Streamed` (read the variant's fields in `crates/theseus-voice`; `Event::Streamed` fires at
  a `Down` and at the call's end, per the voice crate's AGENTS.md) and asserts: exactly one `speech.transcribed` row,
  its detail has `"trailing": true` and the speaker, its duration is zero, and its usage/cost are what `book` gives
  for that usage (compute the expected figure from the price the existing test uses; do not hard-code a number you
  did not derive).
- Red first: plant P1 by making the arm skip `book` (or book a different kind): your test red. Restore.

### 2. `start_turn`'s dead supersede branch (review item L-a)

- `crates/theseus-voice/src/engine.rs` `start_turn` (:1798) returns early at :1799-1801 when `self.turn.is_some()`.
  So the later `self.turn.replace(Turn { .. })` (:1848) and its `if let Some(old) = ...` that removes
  `self.first_words.remove(&old.id)` (:1854) can never see an old turn: that branch is dead. The comment there (~:1847)
  says "A turn still waiting for its reply is superseded by this one", which is not what happens.
- Fix (the review's suggestion): since the early return guarantees there is no current turn, restore the plain
  assignment (`self.turn = Some(Turn { .. })`) and delete the dead `first_words.remove` branch and its comment. Do NOT
  change the early return or any behaviour: this is a dead-code removal. If, reading the code, you conclude the early
  return is itself wrong (a superseding turn SHOULD replace a waiting one), do not change it: say so in the report as
  a finding with the evidence.
- The test `an_eager_turn_superseded_by_the_whole_sentence_leaves_no_entry` is in
  `crates/theseus-voice/src/engine/tests_leaks.rs` :61-77. It has no doc comment of its own; the module doc (:1-3)
  speaks for it. What it actually exercises: Start@10, Eager "Book a table"@40, Resumed@50, End "Book a table for
  two."@70, then `leftover(listen, true, "")` == 0, which is the `speakable` empty-reply removal at `engine.rs`
  ~:1681-1683, NOT the supersede branch. Give the test a doc comment that says what it really proves (an empty reply
  that never gets audio removes the turn's `first_words` entry). Do not rename the test or change its assertions.
- The voice crate's own rules (`crates/theseus-voice/AGENTS.md`): with `join_guard`, `judge_unclear` or
  `echo_cancel` off, events and times must be byte for byte the old ones; `tests/aec.rs` is a byte-for-byte golden.
  This change must move no event: `tests/turns.rs`, `tests/judged.rs`, `tests/flux.rs`, `tests/flux_socket.rs` and
  `tests/aec.rs` stay green untouched.
- No red-first test is possible for dead-code removal; instead show it is dead: in the report, the early return's
  lines and why `self.turn` is always `None` at the assignment.

### 3. `silence_ticks`' literal 20 (review item L-c)

- `crates/theseus-voice/src/engine/flux_path.rs` :137: `listen.silence().as_millis() as u64 / 20`. The tick is
  `FRAME` in `crates/theseus-voice/src/audio.rs` :15 (`Duration::from_millis(20)`, "One frame: Discord's packet
  length, and the engine's tick"); `engine.rs` :174 `frames()` already divides by `FRAME`. `flux_path.rs` :51 imports
  `duration_of, rms, Audio, FRAME_SAMPLES` from audio, not `FRAME`.
- Fix: derive the tick count from `FRAME` (reuse `frames()` if it fits, else divide by `FRAME.as_millis()`), and do
  the same for the other literal 20s that mean the tick: `flux_path.rs` :63 (`TURN_MAX_TICKS`) and :660, and the test
  helper `crates/theseus-voice/tests/flux.rs` :24 if it means the tick. A literal 20 that means something else stays:
  say which.
- A unit test that `silence_ticks` equals `silence / FRAME` for a few silences (0, 19 ms, 20 ms, 1 s). It passes at
  the base too (the value is the same); plant P2 by changing `FRAME` in a local scratch copy only if that is cheap,
  otherwise say the test guards the derivation, not a behaviour change. Never commit a changed `FRAME`.

## Not in this row

No behaviour change in the engine (events and times identical). No change to the Discord DM text flow or any text
message. No change to `FRAMING` (the lane's other row owns it).

## Rules for this row

- **Locked Discord DM task flow:** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical. Run
  `git diff --exit-code ac298666 -- crates/theseus-discord/src/golden/dm_flow.txt` and paste the result. A fix that
  would change any message of that flow STOPS that part: write `OWNER CALL: <what and why>` in the report.
- **Keel:** remove or weaken no test; raise no budget, cap or ceiling; do not edit `scripts/keel-guard.py` or its
  tests; rename no test; edit no golden. Before the report run
  `python3 scripts/keel-guard.py --base ac298666 --head HEAD --quiet` and paste its output.
- **Public repo:** no person's name, no real organisation, no address, no email, no key. Fixtures use invented content
  under `example.test`. Write "the owner".
- **Red first** where a behaviour is tested; right over compatible.
- **The gate** (read the root `AGENTS.md`, `crates/theseus-discord/AGENTS.md` and `crates/theseus-voice/AGENTS.md`
  first): `cargo fmt --check`, `cargo clippy` on `theseus-voice` and `theseus-discord` with `-D warnings`,
  `cargo nextest run -p theseus-voice -p theseus-discord` (the Discord crate's tests run under nextest, as the gate
  does); then `scripts/shape.sh`.

## Your report

Commit `CLOUD_REPORT.md`. Write it with your editor tools, never a shell heredoc, and check after the commit that it
is not empty. Push the branch. End with a short final message that starts `CLOUD REPORT COMPLETE`.

Report sections: (1) **For the maintainer:** store format (51, untouched), any ceiling, OWNER CALL or STOP.
(2) **Item 1:** the test, what it asserts, and plant P1 red then green. (3) **Item 2:** why the branch is dead (lines),
the change, the new doc comment, and any finding about the early return. (4) **Item 3:** each literal 20 and whether
it was the tick; the new test. (5) **Gate** commands and results, and that the voice crate's byte-for-byte tests are
green untouched. (6) **Keel** output. (7) **DM golden** diff result. (8) **Join notes:** every file touched, and
confirmation that `voice.rs`'s `FRAMING` lines are untouched. (9) **Follow-ups.**
