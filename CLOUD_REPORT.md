# Cloud report: the language-server board and its tools, step L2 (theseus-n88g.8)

Branch `cloud/20261004-lsp-board`, from `main` at `f1fccec`. The work started at 08:28 UTC and the report was written
at about 09:50 UTC, well before the five-hour deadline. All five steps landed, the rename included.

## Commits

- `056e0fd` core: the language-server board and the lsp.* tools (theseus-n88g.8)

**This is one commit, not five.** I built the five steps together, and they share the board, its tests, and the gate's
step, so no earlier cut of them would have been green on its own. I didn't commit half a step. Each step is still
proved on its own, below.

## What I found

- The client (theseus-lsp) needed nothing. `Client::start`, `request_within`'s `Outstanding` (dropping it sends
  `$/cancelRequest`), `stop`, `locate`, and the fake were enough. When a call's task is aborted, its request future is
  dropped, so **cancel and `/stop` came for free**: toolrun's `Stops::track` already aborts an async call's task.
- `places::offered` already keeps `lsp.*` out of a shared place, because they are not in `public_tool` or
  `files_tool`. A test now holds that.
- No MCP board is in this clone yet. I didn't touch MCP. ToolCtx is unchanged, and so is the `Tool` trait.
- The new ledger kinds (`lsp.*`) are strings on rows that already exist, not a new record kind or a new field, so
  **`MANIFEST_FORMAT` stays at 7** and there is no new layout sample.
- `LspServerConfig`'s derived `Default` had `enabled = false`, which silently dropped every preset. A test caught it,
  and it now has a manual `Default`.

## What I changed, and how each step is proved

### 1. `[lsp]` and the board

- **Config:** `config/lsp.rs` holds `[lsp]` (`enabled` false, `idle_stop_mins` 10.0, `request_timeout_secs` 30) and
  `[lsp.servers.<name>]` (`command`, `extensions`, `roots` (the root markers), `settings`, `enabled`).
  - Each key has its default and its template entry. The template un-comments to a valid `[lsp]`
    (`the_templates_lsp_section`).
  - A name that is not a preset needs both a command and extensions.
- **Choosing a server:** a file's server is the first in `[lsp.servers]` order, then the preset order, that serves
  its extension and whose program is on the job's `PATH`. The preset order is rust-analyzer; ty, pyright,
  basedpyright; tsgo, typescript-language-server. typescript-language-server gets `<root>/node_modules/typescript/lib/tsserver.js`
  when the project has one.
- **The board** (`lsp/mod.rs`): lazy, one server per (server, root).
  - The root rules, as specified: the nearest marker inside the workspace root that holds the file. For Cargo, the
    nearest `[workspace]` manifest, else the topmost `Cargo.toml`.
  - Spawn: `children::spawn(Kind::Owned)`, `process_group(0)`, `env_clear` plus the job environment, and stderr to a
    capped 1 MiB `<state>/lsp/<server>-<hash>.log`.
  - A start runs in a task of its own, so a cancelled call doesn't cancel a start that later calls will use.
  - The events channel is drained, and a close the board didn't ask for becomes `lsp.failed`.
- **Facts** (`fact/lsp.rs`) are `lsp.started`, `lsp.ready` (with `ready_ms`), `lsp.stopped` (`idle` | `timeout` |
  `daemon`), and `lsp.failed`. They are written off the workers, through the index tender's ledger hook.
- **Health:** `HealthResult.lsp` is a new protocol type, `lsp::LspServerStatus`; the TypeScript is regenerated.
  The CLI's line is `render/lsp.rs`.
- **The daemon's stop:** `Core::stop_lsp` sends SIGTERM to each group and never waits. Both the socket path and the
  `--stdio` path call it.

Tests in `tests_lsp.rs` (the fake served in-process):
- `the_read_tools_answer_and_a_server_starts_at_its_roots_first_call`: no spawn before the first call, one per root,
  and the facts and health.
- `a_files_root_is_its_nearest_marker_and_cargos_workspace`.
- `an_idle_server_stops_and_the_next_call_starts_it_again`: tokio's paused clock. The server is still up at 9
  minutes, stops at 10 with `why: idle`, and the next call spawns again.
- `a_crashed_server_fails_and_the_next_call_starts_it_again`.
- `lsp_off_is_no_board_and_no_tools`.

The daemon's test is `tests/lsp.rs`, with the real `theseusd` and the real `theseus-lsp-fake`:
- health's `lsp` is `[]` once secrets settle;
- one model call to `lsp_definition`, whose result reaches the model with `a.fake:1:5` and the context lines;
- the server is ready, in its own process group, with one log, and with `lsp.started` and `lsp.ready` rows;
- after **`kill -9` of the daemon, the server ends within 10 s.** Its stdin closes. I didn't use `PR_SET_PDEATHSIG`:
  it fires when the spawning *thread* exits, and `theseus_store::blocking` moves workers to new threads.

**Planted revert 1:** a server started at the daemon's start (in `Board::set_ledger`). `tests/lsp.rs` failed with
`no server at the daemon's start: [{… "server":"fake","state":"ready"…}]`. I restored the file, touched it, and
`git status` was clean.

### 2. The read tools

`lsp/tools.rs` holds `lsp.definition` (each location with 2 lines before and 4 after, numbered), `lsp.references`
(grouped by file, at most 200, and it says what it left out), `lsp.hover`, `lsp.symbols` (`{path}` for an outline,
`{query}` with or without a path), and `lsp.diagnostics {path?}` (with no path, every file the tools opened).
- Each is a `Read`, `Backend::Async` tool, addressed by path, a 1-based line, `symbol`, and `occurrence`.
- The deadline is three request timeouts plus 30 s, which covers a first start.
- `config.rs` takes `lsp::NAMES`, and the template has a `[policy.tools]` line for each tool. The count test went from
  30 to 37.

Proved by `the_read_tools_answer…` (each tool through its plan and `run_async`, and the locate errors), and end to
end in `tests/lsp.rs`.

### 3. The start's posture

`lsp::gate` runs in `toolrun`'s gate right after `sandbox::decide`; it is a one-line hook. When the call would start
a server for a (server, root) with no start yet in this daemon's life:
- it computes `policy.decide_with(proc.run, Plan{Exec root, argv})`, with proc.run's tightening;
- the stricter decision wins, and its reason and notice name the server, the root, and the argv;
- it never loosens a decision.

Tests:
- `the_first_start_is_judged_as_proc_run_and_not_again_for_that_root`: proc.run set to approve makes an open
  `lsp.hover` wait; after a start there it's open; another root waits again.
- `the_first_start_under_the_template_is_a_notice`: notify, and the notice names `fake`.

**Planted revert 3** (not asked for): the step switched off (`started_before(..) || true`). Both tests failed
(`lsp.hover — open …`). I restored the file, touched it, and `git status` was clean.

### 4. The rename

**The design choice you asked to hear: aws/stack.rs's pattern, not an async plan step.** Two tools:
- `lsp.rename.plan {path, line, symbol, new_name, occurrence?}` is a read.
  - It asks for the definition first. If that is one place inside the roots and somewhere else, it renames there and
    says so. This handles TypeScript renaming an imported name at its import.
  - It refuses an edit that creates, renames, or deletes files.
  - It applies the edits at their UTF-16 columns and refuses overlapping edits.
  - It shows a unified diff and keeps the edit by digest (the newest 32, for this daemon's life).
- `lsp.rename {digest}` is `Write` and `NonRepeatable`.
  - Its plan's resources are every file it writes, as `Write`, so the floor and the approve list judge it as an
    `fs.patch` of each.
  - `lsp::gate` also makes a write outside the roots wait.
  - Its summary names every file, and so does its notice.
  - It writes only if every file still has the text the plan read, using `write_atomic` with the operator's umask.
    Then it tells the servers (`file_changed`) and returns the diff. A digest applies once.

**This differs from the brief**: it has a seventh tool, and `lsp.rename` takes `{digest}`, not `{path, line, symbol,
new_name}`. Those arguments moved to `lsp.rename.plan`. An async plan step would have meant changing the shared
`Tool` trait and toolrun's synchronous gate, which other changes are reshaping too.

Tests:
- `a_rename_is_gated_on_every_file_it_writes`: 3 edits in 2 files; the plan writes nothing; approve when `b.fake` is
  on the approve list; approve when a file is outside the roots; the apply writes exactly the edit; a digest is
  refused the second time; a file changed since the plan is refused, and nothing is written.
- `a_renames_notice_names_every_file_it_writes`.
- `edits_apply_at_utf16_columns_in_any_order`.

**Planted revert 2:** the gate skipped the rename's files (the plan's resources filtered out). The test failed with
`lsp.rename — notify (enforcement = notify)` where approve was wanted. I restored the file, touched it, and the 3
rename tests passed again.

### 5. Cancel, `/stop`, the span, and the metric

- **Cancel and `/stop`:** see "What I found". `a_cancel_sends_cancel_request_and_the_server_stays_up` aborts the
  call's task mid-request against a slow fake. The client counts 1 cancel sent, the fake's `fake/seen` shows it, the
  server is still up, and there is no second spawn. **Not proved through a whole turn's `/stop` (`Core::stop_execution`).**
  It goes through the same abort as `tests_cancel.rs`'s async-tool cancel.
- **The span:** `Board::call` times each request, keyed by its tokio task id. `toolrun` binds the task id to the
  call's `tool_use_id` (2 lines), and `turn::trace_calls` adds `Board::spans` under the call's span. That is
  `lsp.request`, kind `lsp`, with `lsp.server`, `lsp.method`, and `outcome`. Seen in `a_calls_requests_are_its_spans`
  and in the daemon test's real turn trace.
- **The metric:** `theseus.lsp.request.duration`, a histogram in ms, by `lsp.server`, `lsp.method`, and
  `theseus.outcome`. It is made from those spans at the turn's end. Test:
  `telemetry::tests::a_calls_lsp_requests_are_timed_by_server_method_and_outcome`. The count of servers up is
  health's, not a metric.
- **A timeout** stops the server (`lsp.stopped` `timeout`) and says the next call starts it again.

### Under load

Five rounds of `nice -n 19 cargo nextest run --workspace -E 'test(lsp) | binary(lsp)'`, beside four busy loops at
nice 0 (killed by their pids). 17 of 17 tests passed each round.

## The gate

`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: fmt, shape, features, clippy, cockpit, test build, and the reader rule
passed. **The suite failed**: 1957 tests ran, 1923 passed (1 flaky passed on its retry), and 34 failed. None of the
failures is this change's:
- the 32 sandbox tests (`theseus-sandbox::contract …`, `theseus-sandbox::bench spawn_100`, `theseusd::sandbox …`):
  this VM runs everything as root (theseus-pv6i);
- `theseus-core tests_output::the_cores_output_matches_its_golden`: a wake's time renders `+#:#` in this VM's UTC
  where the golden has `-#:#`. It passes with `TZ=America/Los_Angeles`. This is a time-zone dependence of the golden
  on this machine.

I then ran the phases after the suite by hand:
- protocol types: the regenerated TypeScript is committed;
- `theseus-sim bench turn --check --runs 5 --burst 0`: plain 5 frames and tool 9 frames, ok;
- `cargo deny --offline check`: ok (the advisory database was fetched at setup).

The lifecycle and jobs benches were skipped, as `NO_BENCH` does. Cargo.lock gained no package: only `theseus-lsp` and
`similar`, both already locked, as dependencies of theseus-core.

## The live check (the maintainer's)

On a scratch daemon with a fresh state dir, over a small Rust project with rust-analyzer installed (or Python with
ty or pyright):

```sh
mkdir -p /tmp/lsp-live/{state,proj} && cd /tmp/lsp-live
cargo new --lib proj/demo && cat > proj/demo/src/lib.rs <<'EOF2'
pub fn total(xs: &[u32]) -> u32 { xs.iter().sum() }
pub fn report(xs: &[u32]) -> String { format!("{}", total(xs)) }
EOF2
theseusd example-config > config.toml
# edit config.toml: [tools] projects_dir = "/tmp/lsp-live/proj"; [web] enabled = false;
# [discord] enabled = false; and add:
#   [lsp]
#   enabled = true
#   idle_stop_mins = 1
theseusd --config config.toml --state-dir state --socket sock &
theseus --socket sock health | grep '^lsp:'        # "lsp: none up (...)": nothing at the start
theseus --socket sock ask "Where is total defined in demo/src/lib.rs, and who calls it? Use the lsp tools."
theseus --socket sock health | grep '^lsp:'        # "lsp: rust-analyzer on /tmp/lsp-live/proj/demo (pid N, ready in X s, N MB, …)"
theseus --socket sock ask "Rename total to sum with lsp_rename_plan, then apply it with lsp_rename."
#   the plan shows the diff and a digest; the apply's notice (or card) names lib.rs; its result is the diff
sleep 75; theseus --socket sock health | grep '^lsp:'   # "lsp: none up": the idle stop
theseus --socket sock shutdown
```

What each step should show:
- The first ask's first `lsp_*` call is a notice naming `rust-analyzer` and its argv: the start, judged as proc.run.
  The calls after it are not.
- `ls state/lsp/` shows `rust-analyzer-<hash>.log`.
- The rows `lsp.started`, `lsp.ready` (with `ready_ms`), and `lsp.stopped` (with `why: idle`) are in the ledger.

## What is left, or uncertain

- **One commit, not five** (see above).
- **The rename is two tools** (see step 4). Open question for you: one tool with an optional `digest` instead?
  Either way the preview would take the write's posture.
- **The "first start" set is in memory.** A daemon restart judges each root's first start again: one notice per root
  per daemon life.
- **A start that failed is tried again by the next call, with no backoff.** A server that dies at once costs each call
  one spawn.
- **`/stop` over a whole turn** isn't tested end to end (see step 5).
- **Health's memory** is the process group's RSS, read from `/proc` on each health call.
- **Docs for you to write** (I edited none):
  - Part III's item, and `docs/status.md`.
  - `docs/design/README.md` lists theseus-lsp among the reserved crates; it is read now, and its `reserved_for` is
    gone.
  - The brief's design isn't in docs/design. It should say what changed: the two-call rename, and that the first
    start is judged once per root per daemon life.
- **AGENTS.md:** theseus-core's now has a "Language servers" bullet. theseus-lsp's no longer says it is merged ahead
  of its reader.
