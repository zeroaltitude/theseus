# CLOUD_REPORT: sealed-holdouts (theseus-pw1q.5)

Branch `cloud/20261013-sealed-holdouts`, from main at 9fa84735. Store format unchanged (32): a new `LedgerKind`
(`self.exam.counted`) and a new optional field on a wire type, nothing stored.

## Step 1: sealed holdout sets (commit 54d28ce)

**Found.**
- `theseus-exam` depends on `theseus-core`, so core cannot reuse its `check.rs`. I took the part of the exam's check
  language that reads a reply (`reply has`, `reply lacks`, `has word`, `or`, `|`, the same normalisation) into
  `rsi/sealed/check.rs`. `calls`, `file` and regex lines are refused at `put`, never scored wrong. To reuse the whole
  language, `check.rs` and `words.rs` would move to a crate both depend on; that is a follow-up, not done here.
- The gate has one refusal hook for every tool in every place, `ToolRuntime::refusal` (used by `gate`, the batch
  `judge`, and `policy.explain`), so the sealed denial sits first in it. The floor only *asks*; the sealed dir is
  refused.
- `toolrun.rs` was at 2,499 of 2,500 lines. I moved `guarded_paths` and `sandbox_for` to `toolrun/guarded.rs`
  (byte-identical code, `pub use` keeps the path) and added the sealed line there.
- AWS hands (`aws.hands.run`) have no file access (Lambda/Fargate workers, completions over SQS), so there is nothing
  to deny them; the only local tool they reach is the gate, which refuses the call that names the dir.

**Changed.**
- `[self] sealed_dir` (config/rsi.rs, template line before `[self.budget]`, template tests): absolute, and outside
  every `[tools]` root, `cwd`, the state dir, the repository (nearest `.git` above cwd) and every `[sandbox]
  ro_paths`, neither inside nor holding one, symlinks resolved; refused at `Config::validate` as `[self] sealed_dir
  is inside …` / `holds …`.
- Out of reach (`rsi/sealed.rs`, `Guard`): refuses a plan whose resources resolve into it, an argv/steps/url that
  names it or an argument that reaches it from the call's directory (`..`, `~`, links), and any call whose input
  writes its path anywhere (a shell string, a terminal's keys). The refusal names no path. It is also a floor path
  (`toolrun/guarded.rs`), so `Sandbox::job_view().hidden` has it, and a job, an MCP server (`mcp/l1.rs`) and a
  proposed extension (both start from `job_view`) never see it.
- `rsi::sealed::Sealed`: `put(set, family, bytes) -> ItemId` and `score(set, &dyn Runner) -> Scores`. `ItemId` is
  16 hex digits of an HMAC-SHA256 under a key the sealed dir holds (`.key`, 0600); no call returns bytes. `Scores`:
  per-family items/passed/failed/errors/rate, cost, and `f_<hmac>` ids for failures. `Filter` refuses an output that
  holds any task text, any check pattern of 8+ characters, or any item's `canary` (any length). Every error is a
  fixed sentence (`SealedError`). Item: `{"v":1,"task","check","canary"?}`, 256 KiB at most.
- `Core::sealed_put` (writes the `self.exam.counted` row, `sealed: true`, item id and family and reviewer only,
  `fact/rsi_sealed.rs`, `LedgerKind::SelfExamCounted`) and `Core::sealed_score`. Both are Rust calls: no RPC method,
  so nothing a job can reach.
- Health: `SelfState.sealed: Option<SealedState {sets: {name: count}}>` (counts only; absent when unset, so today's
  bytes are unchanged); the `self` line says `no sealed dir set` or `sealed dir set: core 40 items, …`. Counts are
  read from the dir once, on first ask, and kept by `put` (no scan per health call).
- Docs: `crates/theseus-core/AGENTS.md` and `crates/theseus-protocol/AGENTS.md`. `Cargo.toml` of core gains the
  dev-dependency `tracing-subscriber` (workspace member already; the lock gains no package, only core's dependency
  list).

**Proved.**
- `cargo nextest run -p theseus-core -E 'test(sealed) or test(a_sealed_dir)'`: 20 passed. They cover: every
  registered tool, given the sealed path in every string field (twice, the dir and a path inside), is refused or
  invalid, never open (the test floors the refused count at 20); `proc.run` argv, `sh -c`, cwd, steps and a `..`
  argument; a terminal's keys; a symlink from a workspace read/list/grep/glob/write, and `fs.grep` from the
  workspace never walks through it; the L1 view's `hidden` and the floor hold the path; unset: no guard, no health
  entry, judgments as before; config refusals (root, projects_dir itself, state dir, repo, repo itself, holding a
  root, a link into a root, `ro_paths`, relative, empty); `put` returns an opaque id and no bytes, 0600/0700, same
  bytes same id, fixed errors; the scores match a hand-computed seeded set (6 items, 2 families: 3 pass, 3 fail, 1
  run error, $1.25, stable keyed failure ids that differ under another key); a canary planted in the task, the
  check and the `canary` field, echoed by the candidate, appears in none of the output, the errors (`{:?}` too), the
  `self.exam.counted` rows, the health wire, or the captured logs (the test asserts the logs are not empty).
- Planted reverts (each restored with `touch`, `git status` clean after): plan-refusal off → symlink test and
  shell/keys test fail; input scan off → every-tool and shell/keys tests fail; filter off → both filter tests fail;
  config check off → the config test fails; sealed path left out of the floor → the L1 view test fails; the hook in
  `ToolRuntime::refusal` off → symlink and shell/keys tests fail. Not plantable: the guard's own canonicalisation of
  resource paths (the tools' `paths::resolve` already canonicalise, so reverting the guard's copy changes nothing).
- Gate (`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`): door, keel (0 findings), keel tests, fmt,
  shape, features, clippy `-D warnings`, cockpit, test build all pass. The suite phase: 4,352 tests, only the failures
  below. The phases after it I ran by hand: protocol types (generated TS added, `git diff` clean), the turn bench
  `--check --runs 5 --burst 0` (a plain turn 4 frames; the tool-call turn unchanged), `cargo deny --offline check`
  (advisories, bans, licenses, sources ok). Lifecycle and jobs benches skip under `THESEUS_GATE_NO_BENCH`.
- `python3 scripts/keel-guard.py`: `keel: ok … 0 findings`.

**Gate failures, all outside this change.**
- The known L1 set (root VM): theseus-sandbox `contract` (20), `bench::spawn_100`, and theseusd `sandbox` (13).
- `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`: times out at
  21 s. It is **not on the known list**. It fails identically on the task commit with my work stashed (run alone, 21 s
  timeout), so it is main's, not this branch's. A finding for the maintainer.
- Two existing assertions were rewritten, not deleted, because health's `self` line gained its sealed clause:
  `render/rsi.rs` tests (`…; no sealed dir set`). Counts of assertions went up.

**Live check for the maintainer** (scratch daemon, fresh state dir, never the operator's):
1. `mkdir -p /tmp/sealed-live && theseusd example-config > /tmp/live.toml`, add `[self]\nsealed_dir = "/tmp/sealed-live"`
   and a scratch `--state-dir`/`--socket`; start it. `theseus --socket S health` → the `self:` line ends `; no sealed
   dir set` without the key, and `sealed dir set, no set in it yet` with it.
2. A `sealed_dir` inside the state dir or a `[tools]` root: the daemon refuses to start, naming `[self] sealed_dir`.
3. With a model: ask it to `fs_read` `/tmp/sealed-live` and to `proc_run` `ls /tmp/sealed-live`: both come back
   `Not run: … sealed: the sealed holdout set is out of reach …` with no approval card, at any posture.
4. On a non-root user with L1 on (`[sandbox] default = "l1"`): in an L1 `proc_run`, `ls /tmp/sealed-live` fails and
   `ls /tmp` does not list `sealed-live` (the sealed path is in `Sandbox::job_view().hidden`, as an extension's
   server sees it too). The sandbox contract tests cannot run on this root VM.

**Left / uncertain, and the owner should hear.**
- An L0 job runs as the owner's uid and the gate never parses a command, so a command that builds the path from
  pieces (`$(printf …)`) gets past the gate at L0, as with the floor's paths. L1 hides the path; L0 stays a speed
  bump, said in the module doc. If the owner wants more at L0, the dir should be a different uid or a separate mount.
- The candidate (a `Runner`) must see an item's task; only its reply is kept in the scorer, and nothing of it
  leaves. self-exams brings the real runner (a scratch daemon on the candidate's build).
- The filter ignores strings under 8 characters (other than a named canary) so a short pattern like `7433` cannot
  refuse every output by chance. Items worth guarding at that length should name a `canary`.
- `put` is idempotent by bytes; there is no delete or replace call (an item is changed by sealing a new one).

## Keel findings expected

None. `scripts/keel-guard.py` reports 0 findings.

**Paths the maintainer should add to the keel guard's `KEEL_FILES`** (never edited here; also `sealed::KEEL_PATHS`):
- `crates/theseus-core/src/rsi/sealed.rs`
- `crates/theseus-core/src/rsi/sealed/check.rs`
- `crates/theseus-core/src/rsi/sealed/filter.rs`
- `crates/theseus-core/src/rsi/sealed/daemon.rs`
- `crates/theseus-core/src/rsi/sealed/tests.rs`
- `crates/theseus-core/src/rsi/sealed/tests_reach.rs`
- `crates/theseus-core/src/toolrun/guarded.rs`
- `crates/theseus-core/src/config/rsi.rs` (the `sealed_dir` check)

(`sealed::KEEL_PATHS` lists all of them.)

## Docs the maintainer writes

- Spec Part III: a sealed-holdouts item (this report's "Changed" and "Left"); the version line.
- `docs/status.md`: the roadmap row for theseus-pw1q.5, and "Updated".
- `docs/design/rsi-plan.md`: seam S5 is built; the check language is a subset of the exam's (above).
