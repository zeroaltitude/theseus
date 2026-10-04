# Cloud report: diagnostics in edit results, step L3 (theseus-n88g.9)

Branch `cloud/20261004-lsp-diagnostics`, from `main` at 94e0182 (with L2 merged). Started 11:30 UTC, report at
12:40 UTC. Three steps, three commits, each gated:

| Step | Commit |
|---|---|
| 1. The hook: announce, wait within the bound, the block and `meta.lsp` | a1f966f |
| 2. Pending diagnostics on the session's next edit or `lsp.*` result | ea388d7 |
| 3. `[lsp] edit_diagnostics`, `edit_wait_ms`, `start_on_edit`; the template; health's count | 91ac216 |

## Step 1: the hook (a1f966f)

**What I found.** `toolrun::run_inproc` has the toollet's result in hand before it builds the result node that rides in
the completion's frame, so the hook fits between them, on the turn's task after the toollet has left its core. L2's
board already had what was needed: `server_for`, the `up` map, `Board::call` (span plus timeout handling), and
`Client::file_changed` and `Client::diagnostics` from L1. The fs tools' `meta` gives the files written: `path` for
`fs.write` and `fs.edit`, `files[].path` for `fs.patch`, and `wrote[]` for L2's `lsp.rename`.

**What I changed.**
- `crates/theseus-core/src/lsp/edits.rs` (new): `Board::attach`, `Board::after_edit`, the render, and
  `ToolRuntime::lsp_onto`, the hook `run_inproc` calls. It does nothing unless the call's place is private (the place
  rule) and the call was not settled by a cancel. For each file written whose root has a server up: the server's other
  open documents are synced first (so what it pushes about them during the wait can be counted), then the file is
  announced (`file_changed`), then one task per file waits for `diagnostics(path, request_timeout)` through
  `Board::call`, so the request is an `lsp.request` span under the call. The hook waits on these tasks up to the bound
  (1,500 ms in this step). Then it renders the block:
  ```
  Errors after this edit:
  /w/a.fake (fake): 26 errors
    /w/a.fake:3:1 error [F1]: planted error (fake)
    … (20 lines at most, errors first, then warnings; hints and notes left out)
  …[6 more not shown: lsp_diagnostics with a path lists a file's]
  Other files: 1 new error during this edit (lsp_diagnostics lists the open files').
  ```
  A clean file reads `/w/a.fake (fake): no errors`. A file the bound beat reads `…: pending: fake had not answered for
  it within 1500 ms`. `meta.lsp` holds `server`, `freshness` (`pulled`, `pushed`, `stale`, `pending`, `failed`, or
  `mixed`), `errors`, `errors_shown`, `warnings`, `other_errors`, `waited_ms`, and `files[]` (path, server, freshness,
  errors, warnings), so L5 can count it from the record.
- `toolrun.rs`: one call (`self.lsp_onto(…)`) and `meta` made `mut`. I tried an inline `if let` first, and it pushed
  `run_inproc` over clippy's cognitive-complexity limit (27/25), which is why the logic lives in a method.
- `theseus-lsp/src/diagnostics.rs`: `Client::pushed_errors()`, the error count of each document's last push, read
  before and after the wait to count other files' new errors. This is a small addition to L1's crate. Only pushes are
  counted: a pull-only server (ty, tsgo) never reports other files, so its count is 0.
- `lsp/tools.rs`: `place` made `pub(super)` so the hook can reuse it. `tests_lsp.rs`: its fake spawner and project
  fixtures made `pub(crate)` for the new suite.

**How I proved it.** `crates/theseus-core/src/tests_lsp_edits.rs` uses the fake in-process (`InProcess`), with whole
turns through `Core` and a scripted provider:
- `an_edit_that_makes_an_error_gets_the_block_and_a_clean_one_says_none`: an `fs_edit` that adds 25 errors (26 with the
  file's own) shows exactly 20 lines, "…[6 more", and "Other files: 1 new error". The other error comes from `b.fake`,
  which was open and changed on disk behind the server's back. It also checks `meta.lsp` and that the tool's own meta is
  kept. A second edit leaves 1 error and other_errors 0; the fix shows "no errors"; one spawn in all.
- `no_server_up_is_no_block_and_no_spawn`.
- `a_shared_places_edit_gets_no_block`: a session bound to `channel:…` with `public_paths` covering the tree, a server
  up, and the edit runs with no block.
- `a_server_that_never_answers_costs_the_bound_and_is_pending`: on tokio's paused clock, a pull fake with
  `slow_ms = 5000`. The wait is ≥ 1,500 ms and < 1,550 ms, and the file is pending.
- `the_block_adds_no_frame`: the same edit loop writes the same frame count with the block as without it.
- `lsp::edits::tests`: the render (errors before the warning, the hint left out, the cap, the count line, `meta`),
  and `written` from each tool's meta.
- `tests_m3::a_plain_turn_stays_within_its_frame_budget` and the turn bench (`frames_plain 5`, `frames_tool 9`)
  unchanged.

**Planted reverts.**
- The block in a shared place (`lsp_onto`'s place check removed; at this point the check was the hook's `if let` on
  `PlaceClass::Private` in `toolrun.rs`, which I changed to `_`): `a_shared_places_edit_gets_no_block` failed at its
  "Errors after this edit" assertion. File restored, touched, and `git status` clean.
- The wait without the bound (`timeout_at(deadline, &mut task)` replaced with a plain `.await`):
  `a_server_that_never_answers_costs_the_bound_and_is_pending` failed with "waited 5s". File restored, touched, and
  `git status` clean.

## Step 2: pending diagnostics (ea388d7)

**What I changed.** A file the bound beat keeps its wait task, which runs within the server's request timeout, in the
board's per-session map (`Pendings`: at most 32 per session, the oldest dropped). Its task is also bound to the call
for spans. `Board::attach` now takes the session. Any edit or `lsp.*` result in that session first takes the finished
waits and renders them under "Diagnostics that arrived since an earlier edit:", with `meta.lsp.arrived` (or
`meta.lsp = {arrived}` on a result with no block of its own). Each wait is taken once. A later edit of the same file
aborts its old wait, so the client sends `$/cancelRequest`, and the new edit's own wait replaces it. Another session
never sees it. The code says pending waits live in memory and are best effort: a restart loses them, and the record
keeps the edit's result, which said the file was pending.

**How I proved it.** The paused-clock test goes on: another session's next result carries nothing, the same session's
next result carries nothing before the answer, and after 5 s the session's next `lsp.hover` result carries
"Diagnostics that arrived…" with `a.fake (fake): 1 error` and `arrived.freshness = "pulled"`, once.
`a_later_edit_supersedes_a_pending_wait`: two edits of a slow file, then the next result carries one arrived file with
"no errors" (the second edit's text), not the first's error.

## Step 3: the config, start_on_edit, and health's count (91ac216)

**What I changed.**
- `config/lsp.rs`: `edit_diagnostics` (true), `edit_wait_ms` (1,500, checked 1 to 30,000), and
  `[lsp.servers.<name>] start_on_edit` (false for every preset), with their template lines in
  `config/theseus.example.toml` (the uncommented template turns `start_on_edit` on for ty, and
  `the_templates_lsp_section` asserts the three).
- The board reads them. With `start_on_edit`, a file whose server is not up gets a wait task that starts it first
  (`Board::live`), all within the same bound. A start slower than the bound leaves the file pending, and the start goes
  on.
- The gate: `lsp::edits::gate`, called in `toolrun`'s gate right after L2's `lsp::gate`. For an `fs.write`,
  `fs.edit`, or `fs.patch` in a private place, when a written file's server starts on edit and none was started for
  its root, it judges the start at `proc.run`'s posture for the server's argv, the stricter winning, once per root.
  L2's start judgment moved unchanged into `lsp::judge_start`, which both steps call. `lsp.rename` stays L2's.
- Health: `LspServerStatus.edit_blocks` (`#[serde(default)]`) counts per server the edit results that carried its
  diagnostics. The CLI's lsp line says "N edit results with its errors". `cockpit/src/protocol.gen/LspServerStatus.ts`
  is regenerated.

**How I proved it.** `an_edit_that_starts_its_server_is_judged_as_proc_run`: with `proc.run = approve`, an `fs.edit`
on a start-on-edit file is `approve` with "starts fake on" in its reason. It keeps its own `open` in a shared place and
with the knob off. `start_on_edit_starts_the_server_and_health_counts_the_block` covers one spawn, the block, and
`health()[0].edit_blocks == 1`. `edit_diagnostics_off_is_no_block`. `edit_wait_ms_is_the_bound` (300 ms on the paused
clock, and "within 300 ms" in the text). The config test covers the defaults, `edit_wait_ms = 0` and `30001` refused,
and every preset off. The CLI's `the_lsp_line_…` test covers the count.

## Runs under load

AGENTS.md's recipe: four busy loops at nice 0, killed by their pids afterwards, and at nice 19 five runs of
`(package(theseus-core) & (test(lsp) | test(frame_budget))) | package(theseus-lsp) | (package(theseusd) & binary(lsp))`.
All five runs passed 61 of 61 tests (19.9 to 22.7 s each).

## The gate

`TZ=America/Los_Angeles THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, before each commit. fmt, shape, features, clippy,
cockpit, the test build, and the reader rule all pass. The suite fails only on cases that are not this change's. I then
ran the phases after it myself: the protocol types are committed, `theseus-sim bench turn --check --runs 5 --burst 0`
gives `frames_plain` 5 and `frames_tool` 9, both ok, and `cargo deny --offline check` is ok (the advisory database was
fetched at setup).

The last suite (step 3): 2093 run, 2060 passed, 33 failed. The failures:
- `theseus-sandbox::contract` (all of its cases), `theseus-sandbox::bench spawn_100`, and every L1 test in
  `theseusd::sandbox`: the VM runs as root, and L1 refuses a root daemon ("Linux exempts root from RLIMIT_NPROC";
  theseus-pv6i, listed as known).
- One flaky test that passed on its retry: `theseus-sim::sim the_kernel_holds_its_invariants_under_seeded_faults`.

**A finding not mine:** `tests_output::the_cores_output_matches_its_golden` fails on this VM under `TZ=UTC`, on `main`
as cloned too (I checked with my changes stashed). The golden holds a wake's time with a negative UTC offset
(`-#:#`), and UTC renders `+#:#`. It passes under `TZ=America/Los_Angeles`, which is why I ran the gate with that `TZ`.
The golden's masking could fold the offset's sign, or the test could pin a timezone.

## The live check, for the maintainer

On a scratch daemon of this build, with a fresh state dir, `[web]` and `[discord]` off, and the model's key:

```bash
S=/tmp/l3-check; mkdir -p $S/py $S/rs
cp -r crates/theseus-lsp/fixtures/python/. $S/py/ && touch $S/py/pyproject.toml   # ty, on PATH
cp -r crates/theseus-lsp/fixtures/rust/.   $S/rs/                                # rust-analyzer, rustup's cargo first on PATH
# config: the operator's usual profile and keys, plus
#   [tools]  projects_dir = "/tmp/l3-check"
#   [lsp]    enabled = true
theseusd --config $S/theseus.toml --socket $S/sock --state-dir $S/state &
theseus --socket $S/sock ask "In py/main.py, where is total defined? Use lsp_definition."   # starts ty
theseus --socket $S/sock ask -s <session> "Change line 4 of py/main.py so total gets a string: total(\"x\")."
theseus --socket $S/sock ask -s <session> "Fix that call so it passes a list of ints again."
theseus --socket $S/sock ask -s <session> "In rs/src/lib.rs, where is ledger::total defined? Use lsp_definition."   # starts rust-analyzer
theseus --socket $S/sock ask -s <session> "In rs/src/lib.rs, change ledger::total(&[1, 2]) to ledger::total(\"x\")."
theseus --socket $S/sock ask -s <session> "Read rs/src/lib.rs again."   # any edit or lsp call; carries what arrived
theseus --socket $S/sock health          # lsp: ty on …/py (…, N edit results with its errors) · rust-analyzer on …/rs (…)
theseus --socket $S/sock history <session> --json | jq '.. | .meta?.lsp? // empty'
theseus --socket $S/sock shutdown
```

What each should show:
1. The first ask starts ty: `lsp.started` in the ledger, and health lists ty on `…/py`.
2. The `fs_edit` result ends with "Errors after this edit:" and a ty error on line 4 (an argument type), and its
   `meta.lsp` has `server: "ty"`, `freshness: "pulled"`, and `errors ≥ 1`. The fixture's own line-3 error
   (`count: int = "three"`) is listed too.
3. The fix's result shows the file with only that fixture error left. Fix line 3 as well to see "no errors".
4. On the Rust crate (whose fixture already holds one error, `let count: i32 = "three"`), the edit's result probably shows rust-analyzer's own diagnostics as `pushed`, or says pending if
   `cargo check` was still running. When it was pending, the next result (an `lsp_*` call, or the model's next edit)
   carries "Diagnostics that arrived since an earlier edit:" with `cargo check`'s type error. **Uncertain:** see below.

The only cost is the model's tokens.

## What is left, or uncertain

- **rust-analyzer and pending.** The design expects a Rust edit to say pending and the next result to carry `cargo
  check`'s error. As built, the wait accepts the first push that is current for the new version. rust-analyzer pushes
  its own diagnostics quickly on a change, so its edit may get `pushed` (its native errors) without waiting for
  `cargo check`, whose later push nothing then delivers. Whether `experimental/serverStatus` stops being quiescent
  while flycheck runs (the client waits for quiescence first) decides it. I could not run rust-analyzer here. If step 4
  shows no pending, a follow-up could treat a server that reports status as current only after flycheck's own push
  (for example, by its source `rustc`/`clippy`) or after quiescence following the save.
- **`start_on_edit` is off for every preset**, as asked. Turning it on for ty and TypeScript 7 (tsgo) would cost the
  first edit in a project a start of tens of milliseconds and about 40 to 50 MB resident each, while the server is up
  (until `idle_stop_mins`). Under the template's postures the start is a notice, as L2's first start is, since it is
  judged as `proc.run`. rust-analyzer should stay off: gigabytes and seconds on a large workspace.
- **Other files' count** comes from pushes only, so it is always 0 for the pull-only servers (ty, tsgo).
- **Paths:** a file is matched to its root by `starts_with` on the board's canonical roots. A written path reached
  through a symlink would miss an up server and get no block (L2's tools have the same property).
- **`lsp.rename`** gets the block too. It already tells the up servers its files changed, and the hook announces them
  again, which costs only a save and a watched-files notice.

## Docs the maintainer should change

- `crates/theseus-core/AGENTS.md`, "Language servers": add L3. `lsp/edits.rs`: the hook after `run_inproc`'s toollet
  (`ToolRuntime::lsp_onto`), private places only, the block in the result node (no frame), `meta.lsp`, pending waits
  in memory per session (best effort), `edits::gate` for `start_on_edit`. Tests: `tests_lsp_edits.rs`.
- `crates/theseus-lsp/AGENTS.md`: `Client::pushed_errors`, which L3 reads.
- The spec's Part III item for L3, `docs/status.md`, and the config template's lines are already in the template.
