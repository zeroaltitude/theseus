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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261024-p2f-ops-act`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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

## Read this first: the state of your base for this wave (it wins over anything below about `main`)

Your branch starts from the lane `integ/ops` at `3796ea45` (2026-10-23). That is your base: **not** `main`. Line numbers
below are at `3796ea45` but may be off by a few: search by name. Run the keel guard against `3796ea45`.

**This wave is patch2 (release 1.0.2), its follow-ups.** The lane `integ/ops` was cut from `main` `23328545` (release
1.0.1). At `3796ea45` it holds the Operations page's six rows, merged with their join fixes:
- **ops-cal**: the calendar cache, `calendar.window`, the sensitive level;
- **ops-shell**: `/ops`, its lenses, queue, inspector, key table, parity table;
- **ops-verbs**: `session.profile`, `task.rehome`, `task.reschedule`, the data marks, `ProfileInfo.origin`. The floor's
  refusal is `crates/theseus-core/src/data_mark.rs` (`uncleared`, `refusal_of`, `profile_refusal`);
- **ops-timeline**: the Timeline lens;
- **ops-graph**: the Graph lens, the selection;
- **ops-act** (the row you follow up): the inspector's Live, the Join pane, the model and mode menus, the palette, the
  dock, the floor's shared table. Join fix `40199d68`: the menus carry their own session.

**Store format on your base: 49** (`const MANIFEST_FORMAT`, `crates/theseus-store/src/store.rs` ~682). This row stores
nothing and needs no bump. If a part truly needs one, stop that part and say so in your report's first section.

**Line ceilings.** Never raise one.
- `crates/theseus-protocol/src/lib.rs` is 2,577 lines against a ceiling of 2,733. You add about 6 lines there. Put new
  types in `job_follow.rs`.
- Every other file you touch is unlisted (cap 2,500) and far under: `rpc/methods.rs` 1,949, `views/SessionDeck.tsx`
  789, `jobs_tools.rs` 662, `views/Ops.tsx` 499, the `cockpit/src/ops/*` files under 200.

**Two other rows of lane `integ/p2-follow` share your base.** Stay additive:
- **p2f-ops-ui** owns the Timeline, Graph, Board, Bridge, parity and shell files (`TimelineLens.tsx`, `parity.ts`,
  `Shell.tsx`, ...).
- **p2f-ops-daemon** owns Rust only: `crates/theseus/src/client.rs` (`OPERATORS`), `rpc/session_profile.rs`, the
  rehome handlers, `calendar_cache.rs`.
- Shared: `views/Ops.tsx`, `ops/keys.ts`, `cockpit/AGENTS.md`, `crates/theseus-core/AGENTS.md`. Keep your hunks there
  small and separate.

**Your report is the deliverable.** Commit `CLOUD_REPORT.md` (written with your editor tools, never a shell heredoc;
check its size after the commit, it must not be empty), push, and end with a short final message that starts
`CLOUD REPORT COMPLETE`. No person's name anywhere in the repository, your commits or your report.

## Your task: p2f-ops-act, the model menu greys by the daemon's own floor answer, and the Operations acts' follow-ups (theseus-dyukm, theseus-xzf71)

Branch: `cloud/20261024-p2f-ops-act`.

**Commits.** Every subject carries its Beads id (`theseus-dyukm` or `theseus-xzf71`). End each message with the
`Co-Authored-By:` trailer the base's commits carry: copy it from `git log -3 --format=%B 3796ea45`. You cannot sign
here.

**Time.** Budget: about 5 hours. Work in the order below. If time runs short, stop at an item boundary with the gate
green, and list what is left.

## Your row

### theseus-dyukm (IN in full): grey a profile by the daemon's floor answer, not by origin alone

**The gap.** The cockpit greys a profile only by `ProfileInfo.origin === 'us'` (`cockpit/src/ops/floor.ts` `greyOf`,
~14). The daemon is stricter:
- It clears a profile only when the origin is US **and** the catalog's own row says `us` (`catalog.rs`
  `cleared_for_sensitive` ~167, "the vouch alone clears nothing").
- That check runs through `Config::profile_cleared_for_sensitive` (~2026), then `data_mark::uncleared`, then
  `profile_refusal` (~63).

The wire does not carry that answer:
- `ProfileInfo.origin` is `origin_of` alone (`rpc/methods.rs` `profile_list` ~1173).
- On a host other than Anthropic's or loopback, `origin_of` is the provider's vouch.
- So a vouched-`us` provider serving a model the catalog does not know reads `us`. It is not greyed, and the daemon
  refuses it on a private or sensitive session.
- The toast shows that refusal (fail-safe), but the floor must "never surprise".

Two more places have the gap:
- `tests_profile_floor.rs` (~33) passes `cleared = origin == "us"`, so it cannot catch this.
- `cockpit/src/ops/newtask.ts` `profileChoices` (~47) checks `usOnly && p.origin !== 'us'`.

**What to change.**
1. **Protocol.** Add `cleared: Option<bool>` to `ProfileInfo` (`lib.rs` ~2033), with the same serde and ts
   attributes as `origin`.
   - Doc: "Whether the floor clears this profile for private and sensitive content: US origin and a catalog row of
     `us`."
   - An Option keeps the CLI goldens (`crates/theseus/tests/golden/profile*.txt`) byte-identical.
   - In `tests/wire.rs` `the_ops_fields_are_optional_on_the_wire` (~671), add `assert_eq!(p.cleared, None)`.
2. **One place computes it.** In `data_mark.rs`, add `pub(crate) fn cleared(cfg, catalog, profile) -> bool`. Make
   `uncleared` (so `profile_refusal`) and `profile_list` (`cleared: Some(...)`) use it. Keep `profile_refusal`'s
   signature.
3. **Regenerate `cockpit/src/protocol.gen/`** by running theseus-protocol's tests
   (`the_cockpits_types_are_generated_from_the_rust_ones`). `git add` the result. Never edit it by hand.
4. **Cockpit.**
   - In `floor.ts`, `FloorProfile` gains `cleared?: boolean`. The order in `greyOf`:
     - no profile: "not configured";
     - mark open (or none): null;
     - `cleared === true`: null;
     - otherwise, the refusal words.
     A missing `cleared` greys (fail closed).
   - Correct the comment "A profile of US origin runs anywhere": a profile the daemon clears runs anywhere, and a
     provider's vouch alone clears nothing.
   - In `newtask.ts`, the `usOnly` check reads `p.cleared !== true`.
   - Check that the callers (`menus.ts` `profileRows`, `palette.ts` ~114) pass the whole `ProfileInfo`.
5. **The shared table.** `cockpit/test/fixtures/profile_floor.json` keeps its 24 `rows`; existing tests and counts
   stay. Add an array `"vouched_uncatalogued"`: origin `us`, `known: true`, marks open, private and sensitive, refused
   `false`, `true`, `true`. Extend the `note`.
6. **The Rust test, through the real function.** Keep `the_floors_refusals_match_the_shared_table` as it is. Add a
   test, for example `profile_list_says_what_the_floor_refuses_for_every_row`, driving every row of both arrays
   through `data_mark::profile_refusal` on a real config.
   - Use `crate::tests_route::rig`, as `turn/tests_sticky.rs` does. `glm` is the `zai` provider's.
   - The helpers are in `config.rs` (~1840-1910). For origin X: `serve_glm_unrestricted(); vouch_zai(X)`. For a
     vouched row: `serve_glm_unrestricted(); unprice_profile("glm")`, a model the catalog does not know. For
     `known: false`: `"no-such"`.
   - Assert three things for each row:
     - `cfg.profile_origin("glm", catalog)` is the row's origin;
     - `profile_refusal(...).is_some() == refused`;
     - `core.profile_list()` gives `glm` `cleared == Some(us && catalogued)`.
7. **The cockpit tests.**
   - In `test/ops-floor.test.ts`, the first test feeds `cleared: r.origin === 'us'`, the daemon's answer for a
     catalogued profile.
   - A new test: `{ origin: 'us', cleared: false }` is greyed on private and sensitive, never on open.
   - In `ops-newtask.test.ts`: with `usOnly`, that profile is grey.

The daemon's refusal words ("... is not a model of US origin") are wrong for this case. They live in
`session_profile.rs`, another row's file. Leave them, and name them as a follow-up.

### theseus-xzf71: Join pane and menus nits, job.stop, SessionDeck live, watches in the palette (each checked at the base)

- **F3 (IN)**, `ops/JoinPane.tsx` `send` (~53-60).
  - **The bug.** `setTurnProfile(sid, null)` runs before `turn.submit` answers. A refused or failed send loses the
    "this turn only" choice, and a steer carries the profile into a turn already running on another model.
  - **The fix.** Add a pure `submitOf({ sid, thread, input, turnProfile, steer })` → `{ params, clearOnOk }` in
    `ops/composer.ts`. A steer sends no `profile` and keeps the choice. A send clears it only in `.then`.
  - **The chip** (~124) reads `· the next turn` while a turn runs, else `· this turn`.
- **F4 (IN)**: timed reads that are not pushed. `profile.list` is read every 30 s (`ModelMenu.tsx` ~70, `NewTask.tsx`
  ~28, `Dock.tsx` ~29, `views/Ops.tsx` ~193). `catalog.list` is read every 60 s (`ModelMenu.tsx` ~72, `Ops.tsx`
  ~194). Neither is in `PUSHED`.
  - Use interval `0`.
  - Check that three things still bring fresh reads: `profile.changed`, which invalidates (`Heartbeat.tsx` ~220,
    always mounted); `ModelMenu.set`'s invalidate; and the Dock's `profile.use` through `act`.
  - Do not add the reads to `PUSHED`.
- **F5 (IN)**, `views/Ops.tsx` ~283: `M` needs `focusSession.kind === 'conversation'`, so on a task it does nothing.
  The overlay says "On a session or a task" (`keys.ts` ~117, row ~67).
  - Read how a task's gate takes its mode (`rpc/modes.rs`, `policy::modes` `mode_of`).
  - If a task has its own mode, allow `M` on it, here and in `Inspector.tsx` ~140.
  - If it does not, show a `setFlash` with true words, and fix the KEYS row's words.
  - Say which way you went.
- **F6 (IN)**, `ModelMenu.tsx` `set` (~78): `post` skips `useAct`'s busy guard. A double click sends two
  `session.profile` calls, and the Undo then undoes only to the first choice.
  - Add a local busy flag to `set` and the header's `revert` (~54). Calls return at once while one is out, and the
    rows are disabled.
  - Leave `useOpsAct().post` as it is: the bulk fan-out relies on it.
- **F7 (OUT, maintainer live check)**: the `↪ steered` badge (`components/Transcript.tsx`). Do not change it.
- **F8 (IN)**, `ops/ui.ts` `hold` (~12) and `menus.ts` `stickyWith` (~61): `held.at` is the browser's `Date.now()`,
  compared with the daemon's `at_unix_ms`.
  - Add a pure `heldAt(rows, sid)` in `menus.ts`: the newest `session.profile_set` row's `at_unix_ms` for the session
    (0 if none).
  - `hold(sid, to, at)` stores that value, so both sides of the comparison use the daemon's clock.
  - `stickyWith` and its existing tests stay unchanged.
- **job.stop (IN): a new protocol method.**
  - **What the base has.** The model's tool `job_stop` (`toolrun/jobs_tools.rs` `stop`, ~545) runs
    `kernel.stop_call`, then `terminate_all`, records the verdicts, and reports the verified end. There is no RPC.
    The cockpit's `job_stop` row carries `missing` (`ops/verbs.ts` ~49). The Stop button in `Inspector.tsx`
    `JobWhole` (~172-180) does nothing.
  - **One helper.** Move the stop body into a `pub(crate)` function (for example `stop_job(core, corr, by) ->
    Stopped { ended, verified, action }`). The tool calls it, and its words stay byte-identical
    (`tests_job_handles.rs`).
  - **Protocol.** Add `JobStopParams { id }` (the `act_…` id, as `work.peek` takes) and `JobStopResult { id, ended,
    verified, words }` to `job_follow.rs`. Add `JOB_STOP = "job.stop"` on the `WORK_PEEK` line's block in `lib.rs`
    (~316).
  - **The arm.** Serve it beside `work.peek` (async arm in `rpc/work_board.rs` ~39), with `rpc/peek.rs` `job_for`'s
    place rule: a connection that reads no private text gets `NOT_FOUND` for any job but a shared place's.
    - `by` is `conn.actor(None)`.
    - A job that has already ended answers `ended: true`, with words that say so.
    - Keep `dispatch` within clippy's length. `tests_registry` must pass.
    - Not in `client::OPERATORS`: a stop is protective, like `task.cancel`, and that file is p2f-ops-daemon's.
  - **Cockpit.** Drop `missing`. Wire Stop through `useOpsAct` at posture `arm` (press twice). The toast shows the
    answer's `words`.
  - **Docs.** One line in `crates/theseus-core/AGENTS.md`, beside `work.peek`.
- **thread.asks (OUT, does not fit 5 h).** This would list the router's held messages (`thread_flow::State.asks`).
  Building it needs:
  - a second new method;
  - `Pending` (`thread_flow.rs` ~61) to keep the `ThreadAsk` it sent;
  - an access decision for message text on the wire;
  - a change to router state the threads lane also works in.
  Leave `ops/asks.ts`. Name it as a follow-up.
- **SessionDeck live (IN)**: `views/SessionDeck.tsx` `useLive` (~230) is a line-for-line copy of `ops/live.ts`
  `liveOf`. Replace it with `useMemo(() => liveOf(events, id), …)` over `usePush((s) => s.events)`. Import `liveOf`
  and `LiveTurn`. Nothing the deck shows changes.
- **Watches in the palette (IN)**: not at the base (`palette.ts` `Entity.kind` has no `watch`).
  - **The entities.** Build them from `lib/agenda.ts` `watchRows(views)`, using the views `Ops.tsx` holds (~98), and
    pass them to `entitiesOf` (~380). Hint: `watch · <check> · until <until>`, plus `· failing` when it is failing.
  - **Choosing one** opens the Timeline lens, whose Watches panel lists it. That is a new `go: { lens: 'timeline' }`
    with its branch in Ops.tsx's `go` (~382).
  - **"Cancel the watch"** runs the verb table's `watch_cancel` (`wake.cancel`, posture ask), with `verbsFor`'s params
    for a watch (`lib/agenda.ts` ~252).
  - Do not edit `TimelineLens.tsx`.
- **The maintainer's live check (OUT)**: focus A, join B, press the pane's model button; the title must name B.

## Tests (each red at the base, with a planted revert)
| fix | test, where | red at the base because |
|---|---|---|
| dyukm daemon | the new `tests_profile_floor.rs` test: both arrays through `profile_refusal` and `profile_list`'s `cleared` | no field, no vouched array |
| dyukm wire | `wire.rs` `the_ops_fields_are_optional_on_the_wire`: `cleared` is None and not serialized | no field |
| dyukm cockpit | `ops-floor.test.ts` vouched case; `ops-newtask.test.ts` `usOnly` case | `greyOf` reads origin |
| F3 | `ops-composer.test.ts` on `submitOf`: a steer keeps the profile; a send clears only on ok | no helper |
| F4 | `ops-page.test.ts`: no `useRpc('profile.list'` or `'catalog.list'` with a non-zero interval in `src/ops` or `views/Ops.tsx` | 30_000 / 60_000 |
| F5, F6 | `ops-page.test.ts` needles: the `mode_menu` handler on a task; `set` returns while busy | silent; no guard |
| F8 | `ops-menus.test.ts` on `heldAt`: a browser clock an hour ahead keeps no stale hold | `Date.now` |
| job.stop | in `rpc/tests_peek.rs` (reuse its `Rig` and its running job): stopped, action settled; a second call says it had ended; unknown id `NOT_FOUND`; a no-private connection `NOT_FOUND` for a private job | method not found |
| job.stop UI | `ops-verbs.test.ts` (each method is in the protocol); a needle that Stop calls `job.stop` | `missing`, empty onClick |
| deck live | a source test: no `function useLive` in `SessionDeck.tsx`; it imports `liveOf` | the copy |
| watches | `ops-palette.test.ts`: a watch entity with its hint; Cancel plans `wake.cancel`, ask | no kind |

**Planted reverts.** Plant each of these, watch its test go red, and name the failing line in the report:
- `greyOf` back to `origin === 'us'`;
- `cleared` from origin;
- clearing before the answer;
- one interval back to 30_000;
- `heldAt` → `Date.now()`;
- the `job.stop` arm removed;
- the watch kind dropped.

Restore each with a fresh mtime.

## Not in this row
- **p2f-ops-ui:** the Timeline, Graph, Board, Bridge, parity and shell files.
- **p2f-ops-daemon:** `client.rs`, `session_profile.rs`, the rehome handlers, `calendar_cache.rs`. Join note: it may
  edit `session_profile.rs`, which calls `profile_refusal`. That signature is unchanged.
- **p2f-turn, p2f-caps, p2f-threads, p2f-onto, p2f-bench, p2f-fanout** (other bases) touch none of your files.
- **OUT here:** `thread.asks`; F7; the J1 live check; the daemon's refusal words.

## Rules for this row
- **The DM flow is locked.** `crates/theseus-discord/src/golden/dm_flow.txt` stays byte-identical (`git diff
  --exit-code 3796ea45 -- crates/theseus-discord/src/golden/dm_flow.txt`). If a fix would change any message of that
  flow, stop that part and write `OWNER CALL: <what and why>, before/after text`.
- **Keel.**
  - No removed, weakened or renamed test: the floor test keeps its name and its 24-row count.
  - No raised budget, cap or ceiling. No edit of `scripts/keel-guard.py` or its tests.
  - Run `python3 scripts/keel-guard.py --base 3796ea45 --head HEAD --quiet` and paste its output. It must be empty.
- **The data floor is never loosened.** Nothing private goes to a China-origin model; private and sensitive go only to
  cleared models. The page now greys more, never less. One function computes `cleared`, and a missing value greys.
- **The repo is public.** No person's name, real organisation, address, calendar id or email. Fixtures are invented,
  under `example.test`. Write "the owner".
- **How to fix.**
  - Red first. Real behaviour, never a benchmark special case.
  - Right over compatible.
  - One helper, not two: `data_mark::cleared`, the moved `stop_job`, `liveOf`.
- **Protocol types change**, so regenerate `protocol.gen` and commit it. The maintainer regenerates again at the join.
- **Read first** the root, `cockpit/`, `crates/theseus-core/` and `crates/theseus-protocol/` `AGENTS.md`. Add a short
  paragraph to `cockpit/AGENTS.md`'s Operations section: `cleared`, `job.stop`, and watches in the palette.
- **The gate.**
  - `cargo fmt --check`.
  - `cargo clippy --workspace --all-targets -- -D warnings`.
  - `cargo nextest run -p theseus-protocol -p theseus-core`, then `cargo test -p theseus` (the CLI goldens).
  - In `cockpit/`: `npm ci && npm run lint && npm test && npm run build`.
  - Once at the end: `THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`.
  If the disk fills, say what ran.
- **No new dependency.**

## Your report
Commit `CLOUD_REPORT.md` with your editor tools and check that it is non-empty. Push, then end with
`CLOUD REPORT COMPLETE`. Sections, in order:
1. **For the maintainer first:**
   - the store format (49);
   - the protocol changes, and that `protocol.gen` was regenerated;
   - `lib.rs`'s new length against 2,733;
   - any OWNER CALL.
2. **Per item:**
   - dyukm, F3, F4, F5 (which way), F6, F8, job.stop, deck live, watches: done or not, the test, the plant and its
     failing line;
   - the OUT items, each with its reason.
3. **The gate:** each command and its result.
4. **Keel:** the guard's output.
5. **The DM golden:** the diff result.
6. **Join notes:** your hunks in `Ops.tsx`, `keys.ts` and the two AGENTS.md files; `data_mark.rs` beside
   `session_profile.rs`; the new method.
7. **Follow-ups:** `thread.asks`, the refusal words, anything found.
8. **Data-routing review** (last):
   - what the floor answers now;
   - why nothing is loosened;
   - the fail-closed default;
   - that `job.stop` sends no content beyond its verdict words, under `job_for`'s place rule.
