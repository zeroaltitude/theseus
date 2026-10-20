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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261019-voice-aec`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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


## Your task: voice-aec, voice layer 3: Theseus cancels its own voice from what it hears, using the exact audio it sent (theseus-yaxcp)

Branch: `cloud/20261019-voice-aec`, based on `main` at `74a797ed`, which already holds the voice-at-the-join row
(theseus-yf9ct: the 3 s join guard, a sound at the join waits for its words, the one-word head echo) and its two
join fixes. Every commit's subject carries `theseus-yaxcp`. Deadline for the report: 5 hours after you start.

A sibling row, voice-jev (layer 4, a Jev judgment on short or unclear transcripts), is built beside you from the
same base. It works on transcripts (`heard.rs`'s verdicts and the runtime); you work on audio. Keep out of each
other's way: your code in new modules, and in `engine.rs`, `songbird_io.rs` and `heard.rs` only the call, the field
or the `mod` line.

**Why.** On laptop speakers Theseus hears itself: its voice comes back in the listener's stream as a delayed (about
100-300 ms, varying per call), attenuated, filtered copy. Today only the client's echo canceller and the transcript
rules (`heard.rs`: echo, fragment echo, join noise) stand between that and a false barge-in, and a barge-in is
counted on raw voice activity (`engine.rs`: `barge_in` 300 ms, `barge_frames`). Theseus holds the exact PCM it sent,
so it can subtract its own voice and judge a barge-in on what is left: little left is its own voice; clear speech
left is the owner interrupting, even mid-sentence on speakers. Another voice agent we compared relies on the
client's canceller alone; this goes further.

## What to build (read crates/theseus-voice/AGENTS.md, engine.rs, io.rs, songbird_io.rs, audio.rs, vad.rs first)
1. **`crates/theseus-voice/src/aec.rs`**, pure DSP, no new dependency: per listener stream, a ring of the outgoing
   audio as played (mono, downsampled, e.g. 8 or 16 kHz, about 1 s); a delay and gain estimate by cross-correlating
   energy envelopes (10 ms frames, lags 40-500 ms), re-estimated as the call goes; an NLMS adaptive filter around the
   estimated delay that subtracts the echo; a residual and its echo-return-loss figure per frame. While Theseus is
   silent it passes audio through untouched and costs nothing.
2. **Barge-in on the residual.** While Theseus speaks (and for the echo tail after), the barge-in's voice activity is
   judged on the residual, not the raw frame. Everything else (the hold, the transcript rules, the join guard) is
   unchanged. If the seam that feeds the engine carries no samples today, extend it minimally so the outgoing PCM and
   each listener's samples reach `aec.rs`, and say what you changed.
3. **Switch**: `[voice] echo_cancel = true` (the rollback; with it off the engine is byte for byte today's: a test
   shows the same events at the same virtual times). Add it to the example-config template with its tests.
4. **Cost.** Measure the CPU time per 20 ms frame per listener on this VM (a test that times 60 s of synthetic audio,
   reported, not asserted against a tight number) and put it in the report; aim under 0.2 ms per frame.

## Tests (each with a planted revert; the first is the key test)
- KEY, through the engine's seam in virtual time (`tests/turns.rs` or a `tests_aec.rs` beside it): Theseus is speaking
  a reply; the listener's stream carries Theseus's own outgoing samples delayed 180 ms, attenuated 12 dB and low-pass
  filtered, loud enough that today's raw-frame barge-in fires. With echo cancellation on: no barge-in, the reply plays
  to its end. Then the same echo plus a near-end "speaker" signal (a different synthetic voice) from mid-reply: a
  barge-in, within 300 ms + one frame of the speaker's start. Plant: make `aec.rs` return the input as its residual ->
  the key test fails on a barge-in during the echo-only part.
- `aec.rs` unit tests on synthetic signals: the delay estimate within ±10 ms for delays of 100, 180 and 300 ms; a delay
  change mid-call is tracked within 2 s; the residual on echo alone at least 15 dB below the input after convergence;
  near-end speech survives in the residual (its energy within 3 dB). Plant: freeze the filter's taps (no update).
- With `echo_cancel = false`, the key scenario's events equal today's exactly (the echo-only part barges, as today).
- Silence from Theseus: the listener's frames pass through bit for bit.

## The checks for this row (these replace the preamble's gate line)
- `cargo fmt --all --check`; `bash scripts/shape.sh`; `cargo clippy -p theseus-voice -p theseus-discord --all-targets -- -D warnings`.
- `cargo nextest run -p theseus-voice -p theseus-discord`; the example-config template tests if you add the key.
- The full gate once before the report if time allows; if not, say so. theseus-voice builds libopus with cmake: the
  first cold build takes a few minutes.

## After the join (the maintainer's, not yours)
A live call on laptop speakers: Theseus reads a long reply; the owner stays silent for the first half (expect no
barge-in), then says "wait, stop" mid-sentence (expect a barge-in within about 300 ms). Count false barge-ins and
missed interrupts against the same call with `echo_cancel = false`. Give those steps, and the journal lines to read,
in the report.

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

