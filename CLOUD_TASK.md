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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-voice-knobs`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of your base (it wins over anything below)

Your branch starts from the integration branch `integ/tail` at `7f428787` (2026-10-21: `main` `6918e3c1` plus the
patch1 tail rows tail-hold, tail-mail, job-wait-status, tail-discord, tail-onto, tail-flakes, tail-tooling,
tail-herdr-cancel, tail-effort-400, tail2-mailauth, tail2-google). That branch, not `main`, is your base, and your
branch joins back into it. Line numbers below are at `7f428787`; search by name if one has moved.

**Store format on your base: 48** (`const MANIFEST_FORMAT` in `crates/theseus-store/src/store.rs`). This row changes
no stored record. If you find you must, bump to 49 and add the old layout's literal sample to
`crates/theseus-core/src/tests_layouts.rs` as AGENTS.md says; never write the number as a literal anywhere but the
constant and its tests.

**Line ceilings** (`scripts/long-files.txt`): none of your files is listed. Each must stay at or under 2,500 lines
(`theseus-discord/src/runtime/voice.rs` is 1,888, `theseus-voice/src/engine.rs` 1,776, `songbird_io.rs` 1,052,
`theseus-core/src/voice.rs` 510, the template 1,756). Put new code in a new file if a module would grow past that.
Never raise a ceiling.

**Work in flight beside you:** no other row is coding in `crates/theseus-voice/`, `crates/theseus-core/src/voice.rs`
or `crates/theseus-discord/src/runtime/voice*` tonight. Rows coding elsewhere join `integ/threads` and `integ/cn`;
stay out of `turn.rs`, `turn/*`, the China-origin data rule files, `thread_*` and the TUI.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report: write "the owner".

---

## Your task: voice-knobs, a natural pause before a turn ends and a whole first word, as `[voice]` keys (theseus-bp0lp)

Branch: `cloud/20261021-voice-knobs`. Every commit's subject carries `theseus-bp0lp`. Deadline for the report: 3 hours
after you start.

**Why.** The owner's live voice re-test on the installed daemon (2026-10-21), verbatim: "Much much better on voice!
The first "go ahead" sounds like "gaaah", so at the beginning there's a little clipping or jitter. Sometimes, it
doesn't wait as long during a pause as would be natural, so it ends up chopping up sentences. Are those tunable
theseus knobs, tunable deepgram knobs?"

Both are Theseus constants today: no config key reaches them, and neither is a Deepgram setting (Theseus uses no
Deepgram endpointing; its own voice-activity detector decides where an utterance ends).
1. **Sentences are chopped.** `Config::new` in `crates/theseus-voice/src/engine.rs` (~:117) sets
   `end_of_utterance: Duration::from_millis(700)`; `Config::vad` (~:136) turns it into `VadSettings::quiet_frames`
   (35 frames of 20 ms), and `vad.rs` (~:110) closes an utterance on that many quiet frames. A natural mid-sentence
   pause runs 0.7 to 1.2 s, so 700 ms closes the utterance mid-thought and the rest becomes a second turn.
2. **The first word is clipped.** Two candidates; find which, and say so in the report:
   - **The head.** `songbird_io.rs` (~:89-101) sends `LEAD_IN_FRAMES = 5` (100 ms) of silence before the first clip of
     a speaking spell, so the listener's jitter buffer and Discord's speaking-state flip don't eat its head (the
     comment cites "Theseus here" heard as "seus here", theseus-yf9ct). 100 ms is evidently not always enough for the
     greeting, the first clip after the join.
   - **The tail.** `deepgram.rs` (~:36-44, ~:280-305): Aura-2 sometimes ends a short sentence mid-word ("Go ahead."
     garbled 3 of 8 on 2026-10-18, theseus-sro04); it is asked again up to `SYNTHESES = 3` times, and a clip still
     cut off is faded out over `FADE` (30 ms). "Go ahead" heard as "gaaah" may be a cut-off "ahead" whose fade ate
     the rest, not a clipped head. The owner said "at the beginning", so the head is the likely one, but read both.
     `FADE` is a fade-*out* on a cut-off clip, not a fade-in.

**Risk: low.** Two numbers become config with new defaults, plus validation and docs. The longer pause adds about
0.3-0.4 s of latency to every turn: say that in the template comment, so the owner can trade it back.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `voice-knobs` | theseus-bp0lp | `[voice] pause_ms` and `[voice] lead_in_ms` as config with new defaults, validated, plumbed into the engine and the songbird IO; template and docs; the tail cut-off examined | `crates/theseus-core/src/voice.rs` (+ its tests), `crates/theseus-core/config/theseus.example.toml` (the `[voice]` block only), `crates/theseus-discord/src/runtime/voice.rs` (`engine_config` and the `SongbirdIo` join's call only), `crates/theseus-discord/src/runtime/voice/tests_config.rs`, `crates/theseus-voice/src/songbird_io.rs`, `crates/theseus-voice/src/engine.rs` (the default only, if you change it), `crates/theseus-voice/AGENTS.md` (the lead-in line ~:33 and the pause), `crates/theseus-core/AGENTS.md` (its `[voice]` paragraph, if it lists the keys), `docs/status.md` (one dated entry) | S |

## What exists (at `7f428787`)
- **`[voice]`** is `VoiceConfig` in `crates/theseus-core/src/voice.rs`: serde defaults per field (`default_judge_ms`
  is the model for a millisecond key), `impl Default`, and `Config::validate_voice` (~:150), which bails with the key,
  the value, the range and the default, as `judge_ms` does (`1..=2_000`, "(250 is the default)"). Its tests are in the
  same file (~:330-410); one lists every boolean switch so that none reads false unseen (~:339-346).
- **The call's engine settings** come from `engine_config` in `crates/theseus-discord/src/runtime/voice.rs` (~:895):
  `EngineConfig::new(users)` and then the keys `[voice]` owns (`join_guard`, `echo_cancel`, `judge_ms`). Its tests
  are `runtime/voice/tests_config.rs` (58 lines).
- **The engine's own default** (700 ms) is what the engine's own tests read (`vad.rs`'s
  `an_utterance_closes_on_the_35th_quiet_frame_without_its_silence` uses its own `VadSettings`, not `Config::new`).
- **The lead-in** is a `pub const` in `songbird_io.rs`, read by `Spell::begin` (`LEAD_IN_FRAMES > 0`), `led_in`, and two
  tests (~:584, ~:765-771). `SongbirdIo::join(&songbird, guild, chan)` is called once, at `runtime/voice.rs` ~:428;
  the `SongbirdIo` struct (~:326) holds the `Spell` (~:481).

## What to build
1. **`[voice] pause_ms`** (default **1000**): how long a quiet listener closes an utterance. Validate it as `judge_ms`
   is: a range with a reason (say `300..=3_000`; below 300 every breath is a turn, above 3 s a reply waits too long),
   the bail naming the key, the value, the range and the default. `engine_config` sets `end_of_utterance` from it.
   Leave `engine::Config::new`'s 700 ms as it is, so the engine's own tests read as they did; this is the one place the
   config sets it, as for `join_guard`.
2. **`[voice] lead_in_ms`** (default **300**): the silence before the first clip of a speaking spell. `0` turns it off.
   Validate `0..=1_000`. Replace `LEAD_IN_FRAMES` with a value the `SongbirdIo` is given at its join (a parameter or a
   small settings struct; whichever reads simplest) in whole 20 ms frames, rounded up, as `engine::frames` does. Keep
   the name `lead_in` everywhere: one right name, no alias of the old const. Its two tests take the value they set.
3. **The tail.** Read the cut-off path in `deepgram.rs` and say in the report whether "Go ahead." can still reach the
   listener cut off (all `SYNTHESES` asks cut off). Change nothing there unless you find a plain bug; if you do, it is
   its own commit and its own test with a plant. A fade-in key is **not** in this row.
4. **Template.** Add `pause_ms` and `lead_in_ms` to the commented `[voice]` block of
   `crates/theseus-core/config/theseus.example.toml`, in its style (one line each and a continuation line if needed):
   what each does, the default, and the trade ("longer: fewer chopped sentences, each reply a little later";
   "longer: a whole first word, a later first sound"). Comments only: no key goes live in the template.
5. **Docs.** `crates/theseus-voice/AGENTS.md` ~:33 names `LEAD_IN_FRAMES (5, 100 ms; 0 is off)`: make it the config
   key and its default; add a sentence for the pause. One dated entry in `docs/status.md`'s voice section.
6. **Optional, only after 1-5 are green and committed:** a follow-on utterance that starts within ~1.5 s of one whose
   transcript looks unfinished (no terminal punctuation) is merged into it before it becomes a turn. Do it only if it
   fits cleanly into the engine's existing turn path without new waits on the hot path and without touching the judge;
   otherwise leave it out and write it up as a follow-up with the seam you would use. Do not half-build it.

## Tests (each with a planted revert; the first is the key test)
- **`a_call_closes_an_utterance_after_the_configured_pause`** (in `tests_config.rs`): a config with no `[voice]`
  section gives the call's engine `end_of_utterance` 1000 ms; `[voice]\npause_ms = 1400` gives 1400 ms. Plant: drop
  the line in `engine_config` that sets it. The test must fail; paste the failing assertion.
- **`a_spell_leads_in_with_the_configured_silence`** (in `songbird_io.rs`'s tests, beside the existing lead-in test): a
  lead-in of 300 ms puts 15 silent frames before the clip; 0 puts none and `Spell::begin` reports no lead. Plant: keep
  a hard-coded 5 frames. Paste the failure.
- In `voice.rs`'s tests: both defaults (1000, 300); a set value read back; each out-of-range value refused with the key
  named (as `judge_ms`'s test does). Plant: remove the range check for `pause_ms`.
- The existing lead-in test (~:765-771) and the `vad.rs` tests pass. Do not weaken an assertion; if one must change
  because the value is now a parameter, say which and why.
- Run `cargo nextest run -p theseus-voice`, `cargo nextest run -p theseus-core voice`, `cargo nextest run -p
  theseus-discord voice`, then the template tests (`git grep -n EXAMPLE_TOML crates/theseus-core/src` finds them).

## Not in this row
- The live check (the owner says "go ahead" back after a `/join`, and talks with natural pauses): the owner's, on the
  installed daemon. Write the exact things to listen for in the report.
- Deepgram's own endpointing (`endpointing`, `utterance_end_ms`): Theseus does not stream to it; not this row.
- `voice/judged.rs`, the judge, `heard.v1`, echo cancel, the join guard.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- No new always-loaded model text and no new tool.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer: if a change would need one, leave it out and say so. Before your last commit run
  `python3 scripts/keel-guard.py --base origin/integ/tail --head HEAD --quiet` and paste its output in the report. It
  must be empty.
- **Dependencies.** None new.
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so in the report). Put `theseus-bp0lp` in
  each subject. End each message with the same `Co-Authored-By: Tabitha/Claude` trailer the branch's recent commits
  carry (`git log -3 --format=%B origin/integ/tail` shows it).
- **Names.** No person's name anywhere in the repo, in commit messages or in the report: write "the owner".
- Read `crates/theseus-voice/AGENTS.md` and the `[voice]` paragraph of `crates/theseus-core/AGENTS.md` before you edit.
  Run `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit; do not
  run `cargo fmt` in a way that moves lines in files you were not asked to edit. L1 tests need a non-root daemon with a
  job cgroup and fail on this VM; this row needs none of them, so say which tests you ran.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit: it must not be empty. It holds:
- what changed, by step;
- the two keys: name, default, range, and the template lines as written;
- what you found about the head and the tail of "Go ahead." (step 3), with the code you read;
- whether step 6 was built, and if not, the seam and a follow-up;
- each test and its plant, with the failing output's line;
- what could not run on the VM, with the exact commands for the maintainer;
- the owner's live-check script (what to say, what to listen for);
- the keel guard's output;
- the store format (unchanged);
- follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
