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
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261021-tail4-voice`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Your task: tail4-voice, a reply's later sentence keeps its first word (theseus-9d9aq)

Branch: `cloud/20261021-tail4-voice`. Every commit's subject carries `theseus-9d9aq`. Deadline for the report: 3 hours
after you start.

**Why.** The owner's live voice re-test on install #69 (verbatim): "That went much better! The second message still
sort of missed saying the first word, so something about the buffer fill and intro pause was not enough for a second
message, possibly under a loaded server, to fully get the lead in words audible. But this is already so much better!
Maybe some still slightly more conservative timings, but NICE!"

**The cause.** `crates/theseus-voice/src/songbird_io.rs` sends the lead-in silence only when a clip starts a new
**spell**: `Spell::begin` (near :119) leads in only when no clip plays and the bot has been quiet for at least
`SPELL_GAP` (500 ms, near :90). A later sentence of the same reply that starts less than 500 ms after the previous clip
ended gets no lead-in at all, and nothing is sent in that gap. When no track plays, songbird stops sending audio, so
Discord's speaking state can drop and the listener's jitter buffer drains; the next sentence's head is then eaten.
Under load, synthesis gaps between sentences grow toward that 500 ms edge, which is exactly when it bites.

**Risk: medium-low.** The seam's send side only; the engine, its turn-taking and the echo canceller's timing must stay
correct (the canceller is told how much silence led each clip: `VoiceIo::lead_in`).

**Prerequisite: base `origin/main` at `8260dbab`.** Your branch joins the new lane `integ/tail4`. Run the keel guard
against `origin/main`.

## Your row
| Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|
| `tail4-voice` | theseus-9d9aq | keep the stream warm inside a spell; a short lead-in after any gap; `lead_in_ms` 400 -> 600; a new `gap_lead_in_ms` | `crates/theseus-voice/src/songbird_io.rs` (+ its tests), a new `crates/theseus-voice/src/warm.rs` if the logic is more than a few lines, `crates/theseus-voice/src/io.rs` (doc only, if `lead_in`'s meaning widens), `crates/theseus-core/src/voice.rs` (the config key, default, range, test), `crates/theseus-discord/src/runtime/voice.rs` (the `SongbirdIo::join` call near :428, nothing else), `crates/theseus-voice/AGENTS.md`, `docs/status.md` (one sentence) | M |

## What exists (at `8260dbab`; search by name)
- **The spell.** `songbird_io.rs`: `SPELL_GAP` (500 ms); `led_in(clip, frames)`; `struct Spell { lead_in: u32 (frames),
  clip: Option<ClipId>, quiet_since: Option<Instant> }`; `Spell::new(lead_in: Duration)` rounds up to whole 20 ms
  frames; `begin(id, now) -> bool` (lead or not); `end(id, now)` (only the current clip's end counts).
- **The seam.** `impl VoiceIo for SongbirdIo` (near :560-630): `next` calls `spell.end` on `Heard::Ended`; `play` calls
  `spell.begin`, sets `self.lead_in` (samples of silence put before this clip, read by the engine for the echo
  canceller at `engine.rs` near :721 `self.echo.play(&clip, self.io.lead_in(), Instant::now())`), builds the input with
  `clip_input(&led_in(..))` and `self.call.lock().await.play_input(input)`, and keeps `self.track`; `stop` ends the spell's
  clip and stops the track. The engine plays one clip at a time and starts the next only after the last one's `Ended`
  (`io.rs`, `VoiceIo::play` doc).
- **songbird.** Tracks played on one `Call` are mixed. A silent input (`clip_input` of an all-zero `Audio`) played as its
  own track keeps songbird sending frames; when every track has ended, songbird sends its few trailing silence frames
  and then stops sending. Read the vendored or registry source of songbird's mixer/`play_input` before you rely on
  any of this, and say in the report what you found.
- **Tests.** In `songbird_io.rs`'s test module: `the_first_clip_after_quiet_leads_in_and_the_next_sentence_does_not`
  and `a_spell_leads_in_with_the_configured_silence`. The fake seam is `io.rs`'s `WavIo` (it logs what is played, with
  times, on tokio's clock).
- **Config.** `crates/theseus-core/src/voice.rs`: `VoiceConfig.lead_in_ms` (`default_lead_in_ms()` 400, refused over
  1,000 near :211), test `the_pause_the_lead_in_and_the_merge_have_defaults_and_ranges` (near :465, asserts 400).
  The Discord side passes `Duration::from_millis(voice.cfg.lead_in_ms)` to `SongbirdIo::join` in
  `crates/theseus-discord/src/runtime/voice.rs` near :428.

## What to build
1. **Warm inside a spell.** Between two clips of one spell (the gap after a clip ends, until the next clip starts or
   `SPELL_GAP` passes), the seam keeps sending silence, so Discord's speaking state and the listener's jitter buffer
   never drop mid-reply. The likely shape: when a clip ends, start a silent "warm" track (a few hundred ms of zeros,
   replayed or long enough to cover `SPELL_GAP`) and stop it when the next clip starts or the spell ends; the warm track
   never sends `Heard::Ended` to the engine (the engine must see only its own clips' ends). If songbird offers a simpler
   correct way (a looped silent input, a driver setting), take it and say why. A `stop` (barge-in) ends the warm track
   too.
2. **A short lead-in after any gap.** A clip that starts after the bot sent nothing for any time at all (the warm
   track failed, or the gap was longer than the warm track) gets `gap_lead_in_ms` of silence; a clip that starts a
   spell still gets the full `lead_in_ms`. A clip that starts while the warm track plays needs no lead-in. Whatever
   silence leads a clip, `VoiceIo::lead_in` reports it in samples, so the echo canceller's alignment stays right.
3. **Config.** `[voice] lead_in_ms` default 400 -> **600** (the owner: "slightly more conservative timings"); a new
   `[voice] gap_lead_in_ms`, default **200**, 0 to 1,000 (refused above, like `lead_in_ms`), 0 is off. Pass both to
   `SongbirdIo::join` (a small settings struct if the argument list grows past two durations). Keep the keys' docs in
   `voice.rs` in the same style.
4. **Docs.** `crates/theseus-voice/AGENTS.md`'s songbird_io paragraph (near :33) says the new rule in two sentences;
   one `docs/status.md` sentence under the voice entry (the 400 ms line near :87 becomes 600, plus the warm gap).

## Tests (each with a planted revert; the first is the key test)
- **`a_later_sentence_after_a_short_gap_is_kept_warm_and_led_in`** (pure, in `songbird_io.rs` or `warm.rs`): two clips
  of one spell, the second starting 300 ms after the first ended. The decision logic says: warm silence covers the
  gap (or the second clip carries `gap_lead_in` frames if the warm path is off), and the second clip's first audible
  sample is not the first sample sent after the gap. Plant: restore today's rule (no lead-in, nothing in the gap):
  it fails.
- `a_spell_after_quiet_still_gets_the_full_lead_in`: after `SPELL_GAP` of quiet, the next clip gets `lead_in` frames,
  not `gap_lead_in`. Plant: give every clip the short lead-in.
- `a_stop_ends_the_warm_silence`: a stop during the warm gap leaves nothing playing and sends the engine no extra
  `Ended`. Plant: skip the warm track's stop.
- `lead_in_reports_the_silence_that_led_the_clip`: `VoiceIo::lead_in()` after a gap-led clip is `gap_lead_in` frames
  in samples; after a warm-started clip it is 0. Plant: report 0 always.
- Config: extend `the_pause_the_lead_in_and_the_merge_have_defaults_and_ranges` for 600 and the new key's default and
  range (change the asserted 400 to 600: that is the requested default change, not a weakened test; say so).
- The existing spell tests pass; if one's assertion must change because the rule changed, say which and why in the
  report's first section, with old and new assertions side by side.
- `cargo nextest run -p theseus-voice`, `cargo nextest run -p theseus-core voice`, and `cargo build -p theseus-discord`
  (with the `voice` feature if the crate gates songbird behind it: check `Cargo.toml`) must pass.

## Not in this row
- The engine's turn-taking, the merge window, VAD and the echo canceller's internals (`engine.rs`, `echo.rs`, `aec.rs`):
  touch none of them. If the canceller needs to know about warm silence beyond `lead_in`, say so in the report.
- The live check on the owner's call: the owner's, after install.
- `crates/theseus-discord/src/runtime.rs` (3,499 / 3,500) and anything in Discord beyond the one `join` call.

## Rules for this row
- Model roles: you design the warm path and write it; keep it minimal. Every line you add should earn its place.
- Every behaviour change gets a test and a planted revert. Break it, see the named test fail, restore and `touch`, then
  `git status`. List each plant in the report with the test that failed.
- **Line ceilings:** `songbird_io.rs` is 1,074 lines; keep it under 1,300 (new logic goes in `warm.rs`). Never raise a
  ceiling.
- **Rows beside you on `main` 8260dbab, joining `integ/tail4` with you:** tail4-discord (Discord approval card, forum),
  tail4-tui (TUI board, chat footer, herdr), tail4-route (core `thread_route.rs`, `turn/compile_step.rs`), tail4-tests
  (test helpers, `bench/recall`), tail4-onto (core `judge/onto/`, `theseus-ontology`). Stay out of their files.
  `integ/tail3` changed `crates/theseus-voice/src/engine.rs` and `tests/turns.rs` (the merge hold's bounds): you touch
  neither.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. A cloud row cannot carry
  a `Keel:` trailer. Before your last commit run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet`
  and paste its output in the report. It must be empty.
- **Dependencies.** None new. **Store format** 49, unchanged (a config key is not stored).
- **Commits.** Sign every commit (`git commit -S`; if the VM has no key, say so). Put the Beads id in each subject. End
  each message with the same `Co-Authored-By:` trailer line the base's recent commits carry (copy it from
  `git log -3 --format=%B origin/main`; do not retype it).
- **Secrets and names.** No person's name anywhere: write "the owner". Fixture ids are invented.
- Read `crates/theseus-voice/AGENTS.md` and `crates/theseus-core/AGENTS.md`'s voice passage first. Run
  `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` before the last commit.

## Your report
Commit `CLOUD_REPORT.md`, written with your editor tools and never with a shell heredoc. Check its size after the
commit. It holds: what you found about songbird's sending when no track plays (with the source file you read); the
warm path's shape and why; the lead-in rules as a small table (spell start, short gap, warm start); each test and its
plant with the failing line; the keel guard's output; the store format (unchanged); what the owner should listen for
on the live check; follow-ups.

Push, and end with a final message that starts `CLOUD REPORT COMPLETE`.
