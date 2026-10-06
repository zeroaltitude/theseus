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
- theseus-core's `tests_output::the_cores_output_matches_its_golden` fails under this VM's UTC clock, because two wake lines carry the offset's sign (theseus-ig6n*). The gate line below sets `TZ=America/Phoenix` for it; set the same when you run the suite yourself, and commit only the golden lines your change moves.
- Under load, these timing tests can fail, and none is on the flaky list: rerun it alone, and name it in the report. Batch 9's branches, under review on the owner's machine and joining main today, fix the ones marked *; batch 8's timing-flakes is on main and fixed those marked † (their subjects name theseus-cs71, ynia, 1n2y and qjd6), so those four don't fail on your clone.
  - theseus-core: the output golden's 30 s wait for "wake due" under CPU starvation (theseus-23wh*);
    `tests_m3::parallel::a_cancel_during_a_batch_leaves_no_call_dispatched` (theseus-t2yb*);
    `tests_judge::a_failing_jev_is_recorded_by_its_class_and_changes_no_turn`'s 3 s bound (theseus-vbju*);
    `tests_route`'s verdicts that come `late` under starvation (theseus-biy3*); `tests_lsp_edits::the_block_adds_no_frame`
    (theseus-xx6w*); `term::tests::sh_runs_a_command_and_ctrl_c_interrupts_one` (theseus-ynia†: typed-ahead input can
    land on the prompt line; it fails alone too, about one run in two on this VM) and
    `term::tests::python3s_repl_computes_on_the_screen` (theseus-1n2y†); `telemetry::tests::a_failed_continuation_is_counted_as_a_failed_turn_is`
    (theseus-qjd6†: the exporter's retry counted as a fourth trace);
  - theseus-store's `tests_pages::a_filtered_page_equals_the_scans_answer` can pass nextest's 120 s kill (theseus-hohs*);
  - theseus-discord's `tests_outbox::a_cards_settle_waits_for_its_create_and_edits_it_by_id` (theseus-0bq1);
  - theseusd's `job_approval::a_cancel_kills_the_jobs_whole_tree_a_setsid_descendant_too` (theseus-cs71†: a 1.5 s wall
    bound on the cancel's round trip).
- A negative assertion ("nothing of X reached Y", "no process is left") that fails even once is a finding, not a flake: keep its output, name it in the report, and don't retry it away.

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine (today: theseusd's stop on a SIGTERM or a SIGINT, theseus-xbtr*, and a clean stop that closes the index). Any other failure is yours to explain.

**What main holds.** You clone main at 79be3213 or later, with **store format 23**. Your clone has, besides v1's milestones and batch 9's base (4a449460: route's gaps, situations, the reader rule's closures, memory's paced warm build and consolidation's heading rule, and bench/'s measured arms):
- **batch 8's joins (2026-10-05 and 06):**
  - timing-flakes and telemetry3: four timing tests fixed at their causes; the `theseus.cancel` metric, the index tender's gauges and restarts;
  - cli-tests and history-pages: health's 1-hour words, `judge prove`'s bytes, `watch`'s last line; `after` and `before` on the history reads, and a node's short id;
  - turn-stack, queue-frames and smalls: the turn's future boxed at `TurnRunner::run`; a late result's wake in its turn's end frame and a completion's `execution.queued` row; the secrets board's settle race, musl's `time_t`, the TUI's message order, a budget question's `loop.ended`;
  - wal-mark-skip, crash-hold and durability-on: a start skips the WAL directory's sync when a mark vouches; a crashed call's held reservation booked as spent; the durability sessions list only their prefix, and health's durability line in every surface;
  - **soul-import (79be3213): `theseus import openclaw|list|erase`, imported sessions with their provenance, erase by tag, recall's provenance; store format 23** (core import/, node.rs's bodies, recall.rs, rpc/import.rs, the index's tender and extract);
- batch 9's bench stack: the async record's spend and calls, the recall bench's retraction rule and its overhead plan (bench/ only).

**Other changes in flight.** About fifteen other changes are reviewed and merging into `main` while you work, one at a time on the owner's machine. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.
- learning-fixes (batch 8): replay's yes-or-no rightness, the audit off the low thread, the prove's learned versions (learning/, rpc/judge_prove.rs);
- discord-live (batch 8): the bindings file read live, the courier's maps bounded (theseus-discord runtime.rs, runtime/live.rs, courier.rs);
- voice-turns (batch 9): a barge-in held until words decide, the floor, `Cut` and `Resumed` (theseus-voice, and the pump in theseus-discord's runtime/voice.rs);
- scrub-escaped, gate-tests, approvals-batch (batch 9): the scrubber matches a secret printed JSON-escaped; the gate's layers held by tests; a declined call ends its batch's waits, and a repeated tool-use id no longer reads as answered (core secrets/scrub, toolrun/, approvals; the core golden moves 74 lines);
- judge-tests, judge-reads, judge-turn-cost (batch 9): judge tests, `judge.list` and the notices' brake reading only what they need, a judge-on turn bench and the ladder's first read off the turn path (theseus-judge, core routing and judge/);
- core-waits, route-tests, daemon-proofs (batch 9): four core tests fixed at their causes (the golden's wake, its UTC offset mask), route's tests proof against load, a stdio daemon's stop as `drop(rt)`, the lifecycle bench's `cancel` phase;
- memory-tests, telemetry-tests (batch 9): memory search and telemetry tests.

These are other cloud sessions like you, batch 10, each on its own branch:
- imported-skip: session lists skip imported sessions without reading them (theseus-7087);
- voice-heard: the next voice turn says what was heard, cut and never said; replies shaped for speech; a failed voice turn said aloud;
- voice-echo: an echo and a closing "yes" told apart from a real answer in a voice call.

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (23 on main today, since soul-import), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,715) and crates/theseus-discord/src/render.rs (3,001); near it, crates/theseus/src/render.rs (3,099 of 3,100), crates/theseus-kernel/src/kernel.rs (3,025 of 3,030), crates/theseus-core/src/compiler.rs (2,546 of 2,560), crates/theseus-core/src/config.rs (2,887 of 2,910), crates/theseus-core/src/turn.rs (3,496 of 3,523), crates/theseus-discord/src/runtime.rs (3,453 of 3,500) and crates/theseus-core/src/tests_m3.rs (7,800 of 8,050). A Rust file the list doesn't name fails past 2,500 lines; near that today are theseus-core's toolrun.rs (2,494) and telemetry/tests.rs (2,491), theseus-sim's kernel_sim.rs (2,465), theseus-store's wal.rs (2,430) and theseus-kernel's tests.rs (2,384). Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tool-run logic in crates/theseus-core/src/toolrun/, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).
- **Python under bench/** imports only the standard library, except where the Harbor adapter already imports Harbor, and the gate doesn't run its tests: run them yourself before each commit (bench/README.md says how), and say so in the report. Harbor 0.23.0 needs Python 3.12 or later: where python3 is older, run Harbor's tests in a venv (`python3.12 -m venv /tmp/hvenv && /tmp/hvenv/bin/pip install harbor==0.23.0`).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone. A commit that changes only Python, Markdown or task files under bench/ changes nothing the gate builds (its one read there is `bench/theseus-bench.toml`, in theseusd's bench_profile test: leave that file as it is). For such a commit, bench/'s suites, as your task names them, are the gate; run `scripts/gate.sh` itself before your first commit and before your last.

The gate's shape phase fails a Rust file over its line ceiling in scripts/long-files.txt, and one it doesn't list past 2,500 lines. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261006-voice-echo`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: an answer that repeats its question, and a "yes" on a question's last word, become turns again in a voice call (theseus-3ug0, theseus-1cz8)

Branch: `cloud/20261006-voice-echo`. Main must hold voice-turns' join (theseus-voice's `heard.rs`, `Utterance.heard_as`,
`HeardAs::Echo`, the echo-prone flag); if it doesn't, stop and say so in the report. Every commit's subject carries the
id of the issue it fixes. Deadline for the report: 3 hours after you start.

**Background.** voice-turns (theseus-9ln5, kpa7) made a barge-in hold until the words over it decide: a wordless sound,
an echo, a backchannel or "go on" resumes the cut sentence; words cut it. The maintainer's reviewer found two cases
where the new rules drop a person's real answer in silence, where main before voice-turns made it a turn. Both are
design gaps in the task's own rules, implemented faithfully; fix the rules.

**1. theseus-3ug0: an answer that repeats its question's words is heard as an echo.** `heard.rs`'s `is_echo` (at least
2 words; at least 60 % of the utterance's distinct words among the words of the sentence playing, or of those that
ended in the last 1.2 s) also matches an answer that reuses the question's words. An echo is no turn, and its speaker
becomes echo-prone for the rest of the call (their 300 ms stop is off; their words cut only at their transcript).
- "Should I deploy it now?" ends; 0.5 s later (inside the echo tail) a speaker says "Yes, deploy it now.": 3 of its 4
  distinct words are the question's, so `heard_as: Echo`, no turn.
- "The daily view." said over "Do you want the daily or the monthly view?" is an echo: the question resumes from its
  start, and the same speaker's later "Hang on, wait a second." over the replay doesn't stop it at 300 ms; it plays on
  over them for 1.3 s, to their transcript.
An answer to an either/or question nearly always repeats the question's words, and people answer within about 0.2 to
0.5 s of a question's end, inside the tail. So the commonest answer shape is dropped, and its speaker loses the fast
stop for the call. A true echo looks different: "Creates those. I'll check what's running." heard back from the
speaker's own microphone is a whole, in-order copy of what was played.

**2. theseus-1cz8: a "yes" that answers a closing question is no turn** in two common cases. The engine counts it as
said over speech, so a backchannel resumes and is dropped. voice-turns' own test (`a_yeah_after_a_closing_question_is_a_turn`)
covers only a "yeah" begun 0.5 s after the question ended with nothing queued.
- "Yes." begun 200 ms before "Should I deploy it now?" ends (too short over it to stop it) is `Over::Saying` the
  question, `heard_as: Backchannel`: no turn. People often start an answer on a question's last syllable.
- One speaker's question-reply ends at 4.465 s; another speaker's turn has a reply queued (synthesizing, not begun).
  The first speaker's "Yeah." from 4.6 s answers the question, but `what_is_over` sees a queue front and calls it over
  speech (`Over::Saying` the other reply, sentence 0, which had not begun): a backchannel, no turn. With nothing queued,
  the same "Yeah." is a turn.

Where: `engine.rs`'s `what_is_over` gives `Overlap::Speech` for any queue front, including an item that hasn't begun
(`opens`); and an utterance's overlap is fixed at its first speech frame, though its transcript is classified after
the queue may have emptied.

**3. The backchannel list is exactly the task's, and too narrow.** "Mm-hmm" passes (as `mm` + `hmm`), but the
speech-to-text provider also writes "Mhmm" and "Mmhmm", and people say "Oh, okay." and "Gotcha". Over speech those
are words, and words cut the reply: the very stop voice-turns removed.

**Read first:** the root AGENTS.md (FAST, EXQUISITE VISIBILITY, the reader rule); theseus-voice's AGENTS.md;
`crates/theseus-voice/src/heard.rs` whole (`words`, `is_echo`, `is_backchannel`, `is_resume`, the `Speech` and `Tail`
classifications), `engine.rs` (`what_is_over`, `Overlap`, the echo-prone flag, where a transcript is classified),
and `tests/turns.rs` (its helpers: WAV fixtures, the stand-in speech, virtual time with `start_paused`).

**What to build,** each a green commit:
1. **3ug0: an echo is a near-whole, in-order copy.** The longest run of the utterance's words found contiguously, in
   order, in one candidate sentence covers at least 80 % of the utterance's words, and at least 3 words. Keep "one word
   is never an echo". A speaker becomes **echo-prone only after two echo verdicts** in the call, so one false verdict
   can't take the stop away for good.
2. **1cz8: a reply that hasn't begun is the tail, not speech.** In `what_is_over`, a queue front that hasn't begun
   counts as the tail (echo only), not speech. An utterance that began over the last queued sentence and closed after
   the queue emptied is classified as the tail too (echo only, else words).
3. **The wider backchannel list:** the provider's spellings ("mhmm", "mmhmm", "mm-hmm", "uh-huh" and the like, as
   `words` normalizes them), "gotcha", and "oh" before a listed word ("oh okay", "oh yeah"). Keep it a list, and keep
   its size modest: a backchannel over a reply never cuts it, so only add words that never carry an answer alone in
   that position.
4. **Tests in `tests/turns.rs`**, with its helpers and invented speaker names, each of these as a behaviour:
   - the either/or answer over the question ("The daily view.") is a turn, and the question doesn't replay;
   - "Yes, deploy it now." 0.5 s after "Should I deploy it now?" ends is a turn;
   - a true echo (the played sentence heard back whole) is still an echo, no turn;
   - one false echo verdict leaves the speaker's 300 ms stop on; a second makes them echo-prone;
   - a "Yes." begun 200 ms before a closing question ends is a turn;
   - a "Yeah." after a closing question while another speaker's reply is queued but not begun is a turn;
   - "Mhmm" and "oh okay" over a long reply resume it and are no turn;
   - voice-turns' existing tests still pass unchanged, unless one asserted the rule you replaced: then say which and
     why in the commit body.

**Proof, offline:** the new tests; theseus-voice's and theseus-discord's suites (`TZ=America/Phoenix`); the whole
workspace suite once; theseus-voice's suite three times under the load recipe (AGENTS.md). Planted reverts, each
naming the test it breaks:
- the echo rule back to 60 % of distinct words: the either/or answer test fails;
- echo-prone after one verdict: the two-verdicts test fails;
- `what_is_over` treating a front that hasn't begun as speech: the queued-reply "Yeah." test fails;
- "mhmm" removed from the list: its resume test fails.

**FAST:** the rules run once per transcript, off the audio path; the stop still comes on the same tick; a resume still
replays the held audio with no new synthesis. Say so with the code paths. No bench is needed.

**The live check is the maintainer's** (in a voice channel, on the owner's daemon). Give him exact steps and what each
should show: an either/or question answered with its words, before and after its end; "yes" on a question's last word;
"mhmm" and "oh okay" over a long answer; a cough over a reply (it still stops and resumes).

**Docs:** don't edit `docs/`. In the report, give the text for m7-surface.md §2.8's echo and backchannel rules and its
§3 test list.

**Leave alone (siblings in flight):** voice-heard (the next voice turn's note of what was heard, cut and never said;
replies shaped for speech; a failed voice turn said aloud: `theseus-discord`'s `runtime/voice.rs` and theseus-voice's
`sentences.rs`). Stay inside `heard.rs`, `engine.rs`'s `what_is_over` and the classification, and `tests/turns.rs`.
discord-live (`theseus-discord`'s runtime.rs) and imported-skip (`theseus-core`'s session lists) are in flight too.
