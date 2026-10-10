# CLOUD_REPORT: tool-input-fit (theseus-9dt2)

Branch `cloud/20261010-tool-input-fit`. Code commit: fa08f4f. Report commit follows it.

## What I found
- `theseus health` does not count invalid-JSON inputs by tool. The `tool.invalid_input` ledger row carries the tool, but nothing counted it. I added the count to `tool.list` (`ToolInfo.invalid_json`, since the daemon started), and `theseus tools` prints a line under a tool that has any. Health itself is unchanged: its type is in a file at its line ceiling.
- The system note told the model how to code ("pass [bash, -c, ...] only when a shell is truly needed"). I removed that clause. The schema now carries it.
- serde_json's map is sorted here, so the "takes ..." field lists come out alphabetical, read from the real schemas.

## What changed
- tools crate:
  - `proc.rs`: `command`, exactly one of argv/steps/command, the new description.
  - `fit.rs` (new): `Registry::fit`, `recover_line`, `field_hint`, `nearest`.
  - `fs.rs`: serde aliases only; the schemas are unchanged.
  - `lib.rs`: `streams_input`, so eager streaming is only on fs_write, fs_edit and fs_patch.
- core:
  - `toolrun/fit.rs` (new): `fit_uses` runs where a call is taken from the model (`run_tools` in turn.rs, and `resume`), so the gate, ledger and resume all see the fitted call. It also holds the result note, the unknown-tool and invalid-JSON words, the field hint, and the directory of the first line.
  - `session_dir()` is the one function for "the session's directory". It returns `ctx.cwd` today, so session-dir's join swaps its body.
  - MCP tools no longer get `eager_input_streaming`.
  - The fake provider treats a scripted string input as unparsed JSON (test scaffolding).
- protocol: `ToolInfo` and `ToolListResult` moved out of lib.rs into `toolinfo.rs`. lib.rs got shorter and no ceiling was raised. The generated TS for `ToolInfo` is regenerated.
- cockpit: `resultWords` accepts `[exit code N · in DIR]`, with a test.
- Aliases added, each tested (`the_fs_tools_read_the_names_a_model_reaches_for`):
  - `file`, `filename` and `file_path` for `path`, on read, write, edit, glob, grep and list.
  - `file_text` and `contents` for write's `content`.
  - `old_str`/`old_text` and `new_str`/`new_text` for edit.
  - `diff` for patch.
- Acceptance cases:
  - 1: `argv` given as a string runs as a shell line.
  - 2: an unparsed `{"argv": line}` is recovered only when no other key is present; otherwise the error shows the two shapes.
  - 3: path aliases (see above).
  - 4: `timeout_ms` on `term_open` gives "`timeout_ms` is term_read's and term_send's; term_open takes argv, cols, cwd, quiet_ms, rows".
  - 5: an unknown tool names its nearest one or two. A small table of slips (bash, cat, grep, ...) maps to the real tool.
- Existing assertions rewritten, same counts:
  - tests_m3 eager assertion;
  - the exit-code first-line assertions in tests_m3 and tests_steps;
  - the "not both" and "argv must name a program" messages in proc.rs;
  - the golden's one `tool.ended` preview line.

## Proof
- `cargo nextest run --workspace`: 3701 run, 3668 passed, 33 failed. All 33 are the known L1 tests (theseus-sandbox contract, spawn_100, theseusd sandbox). Nothing else failed.
- New tests: 10 whole-core tests in `tests_fit.rs`, plus tests in the tools crate (`fit.rs`, `proc.rs`).
- fmt, shape and clippy (`-D warnings`) are clean. Cockpit lint, 306 tests and build pass. `cargo deny check` is ok.
- Planted reverts, each restored and touched:
  - `proc.run` added to the eager list: `only_the_write_tools_ask_for_eager_input_streaming` failed.
  - `command` not rewritten to its argv: `the_gate_judges_command_as_the_bash_c_argv_it_runs_as` failed.
  - The `file_text` alias removed: `fs_write_accepts_the_names_a_model_reaches_for` and `the_fs_tools_read_the_names_a_model_reaches_for` failed.
- Not run under load: no timing-sensitive code was added.

## Live check for the maintainer (not run here)
I did not run a scratch daemon, because I did not know the stand-in daemon's scripting flags. The same three scripted calls run through whole cores in `tests_fit.rs`:
`cargo nextest run -p theseus-core tests_fit`. It should show 10 passing.
- `bash {command: "pwd && ls"}` answers starting `[ran as proc_run {command}: there is no bash tool]`.
- `proc_run {command: "echo $((1+1))"}` answers `[exit code 0 · in DIR]` then `2`.
- `fs_write {file, file_text}` writes the file.
- On a scratch daemon, run the same three and then `theseus tools`. No tool should show an invalid-JSON line.

## Left or uncertain
- The result note (`[ran as ...]`) is in memory, keyed by tool_use id. A restart drops it from a result written after it, never the run itself.
- An unparsed input recovered from its raw text is not recovered again on resume after an approval. It is then an input error, a rare path.
- Late results of a long-running `proc_run` do carry the directory in their first line.
- `steps`' non-last step lines keep `[exit code 0, N ms]`, as the brief said only the last step.

## Gate
`scripts/gate.sh` failed in its keel phase, which stops at once. The findings are not from this branch's changes. The range base a9ad950 gives:
- a tui test assertion removed;
- two `#[allow]`s added (in index vectors.rs and protocol tests_work_join.rs);
- ceilings raised: protocol lib.rs 2726 to 2733, and a new golden.rs entry.
I touched none of those; the list is below. I ran the rest by hand: fmt, shape, clippy, cockpit, the suite and deny. All pass except the known L1 failures.

**Keel findings expected** (none from this task):
- assert-removed, crates/theseus-tui/src/tests.rs, the renamed a_finished_task test.
- allow-added, crates/theseus-index/src/vectors.rs.
- allow-added, crates/theseus-protocol/src/tests_work_join.rs.
- ceiling-raised, scripts/long-files.txt (the lines above).

I lowered the protocol lib.rs line count (2733 to 2688), so its ceiling could now be lowered.
