# CLOUD_REPORT: import-a (theseus-o73y)

Branch `cloud/20261011-import-a`. Feature commit: 4e18a6b.

## Step 1: `theseus import claude-code|codex [DIR]`

**What I found.** The episode format has the shape both sources need. Three things had to be decided:
- **Hash and tag.** An episode's hash covers its `import_tag`, so a rerun on another day under a date-named default tag would be refused as "another hash". The default tag is therefore the live tag already holding the source, else `<source>-<today>`; `--tag` overrides.
- **`book_hint`.** It must be one of the seven books, so it cannot carry the project directory. The project directory is the place's name instead (kind `cli`), the git branch is the place's id, the topic label is `projects/<dir name>`, and the book hint is `casebook`. Sensitivity is `company-confidential`.
- **Sources list.** `claude-cli`, the pipeline's old source name, is untouched. The new sources are `HARNESS_SOURCES` (`claude-code`, `codex`), kept apart from `SOURCES` so the existing tests that iterate `SOURCES` don't change.

**What I changed.**
- `crates/theseus/src/harness/` holds the converters (mod, `claude_code`, `codex`, `run`, tests). `mod.rs`'s module comment is the design.
- Messages: user text is `operator`, assistant text and tool calls are `agent`, tool results are author `tool` with integrity `outside`. Recall therefore keeps results out unless `[memory] include_external` is on.
- Bounds: a result is cut at 2,000 characters, a call's input at 400, other text at 32,000, each cut saying how much was kept.
- Left out and counted by type: thinking, summaries, snapshots, meta lines, compact summaries, unknown types, Codex `event_msg`/`turn_context`, injected context.
- Sidechain lines and `subagents/` files are skipped and counted. The parent session already holds what a subagent returned, and the subagent's working would only repeat it at length.
- A malformed or non-UTF-8 line is rejected with its number. A file with no message is skipped and counted.
- `theseus-protocol/src/import.rs` now holds the canonical-JSON writer, moved from `episode.rs`, which re-exports it. The CLI needs it to hash episodes. A new module line would have breached `lib.rs`'s ceiling.
- The CLI gains a `sha2` dependency in its own `Cargo.toml` (no new package in `Cargo.lock`).
- The operator-methods table label is now "theseus import" (it was "theseus import openclaw").

**Where the scrub runs, and why.** In the daemon, in `import::write::scrubbed`, on each message, summary and place name of a harness-source episode. It runs after the episode's hash check and before anything is written, using the daemon's own `Scrubber` (`core.tools.scrubber`). The daemon holds the vault's resolved values (every encoding) and the shape list, and the CLI holds none. The stored per-message `sha256` is recomputed over the scrubbed text. The episode hash, a digest and not text, stays the idempotency key. `import_batch_unless` (tests only) uses an empty scrubber, which still applies the shapes. The openclaw source is not scrubbed here (its pipeline removes credentials), so its behaviour is unchanged.

**Grown file (decision).** The episode id is the file's stem, so a file that grew is the same id with another hash. The daemon refuses it ("imported before with another hash … not overwritten"), and the CLI reports it as "files changed since they were imported" and writes nothing. A new episode that superseded the old one would duplicate every earlier message in recall. To take the longer text: erase the tag, then import again. A resumed Claude Code session is a new file holding copied history, so it is a separate episode.

**How I proved it.**
- `nextest -p theseus`: 263 passed, including 13 new `harness::tests`: fixtures per format, tool lines, counts, rejected line numbers, empty file, hash match, cut bounds, grown file, tag choice, refused in a job.
- `nextest -p theseus-core -E 'test(import::)'`: 60 passed, including `import/tests_harness.rs`:
  - the secret is absent from the WAL, spool and every file under the store dir, with a positive control;
  - a vault value is scrubbed in verbatim, base64 and percent encodings;
  - a second run writes 0 frames;
  - a grown file is refused;
  - erase by tag;
  - a shared place never recalls a harness import, and a tool result is dropped as `untrusted`.
- `theseusd/tests/import_harness.rs`: the real CLI against a real scratch daemon on fixture dirs built at run time (the secret built from parts), 2 tests passed:
  - claude-code counts, rejected "line 2: not JSON", `future-v9 1` and `thinking 1` skipped, 1 empty file, 1 subagent file left out;
  - `import list` shows 2 sessions, 7 nodes;
  - the second run is 0 imported, 2 skipped;
  - the grown file is reported;
  - codex imports;
  - erase by tag, and the state dir holds no secret;
  - the import is refused inside a job (`THESEUS_SESSION`).
- Planted reverts, each restored and `touch`ed:
  - scrub skipped: 2 core tests and the daemon test failed ("a secret reached the store");
  - digest check off (same hash no longer skips): the core second-run test and the daemon test failed;
  - thinking imported: `claude_code_lines_become_messages…` and the daemon test failed.
- `theseus-sim bench lifecycle --runs 10 --check`: LIFECYCLE OK (debug build; serving p50 12.4 ms).

## Live check for the maintainer
On a copy of a few real session files first:
```
mkdir /tmp/cc && cp -r ~/.claude/projects/<one-project> /tmp/cc/
theseus import claude-code /tmp/cc        # counts per file; check "rejected", "skipped"
theseus import list                       # claude-code-<date>: N sessions
theseus import claude-code /tmp/cc        # 0 imported, N skipped
theseus memory search "<a phrase you said>"   # an imported message; header names the import
theseus import erase --tag claude-code-<date>
```
Then `theseus import claude-code` and `theseus import codex` with no directory (whole history, about 19,000 files and 9.3 GB), timed. Expect one `import.episodes` batch per 256 sessions or 4 MiB, 20 ms apart. The CLI holds one session file in memory at a time, so the largest file bounds its memory.

## Left or uncertain
- I could not run a recall on a real index here: the VM has no models, so the tender is off. Recall is proved in-core with the stand-in index, as the openclaw tests do.
- The default-tag reuse takes the most recently updated live tag holding the source. If the owner wants a tag per run, pass `--tag`.
- Secrets-shapes (theseus-oyrt) joins independently: the scrubber gains its shapes when both land, with no edit here.
- Docs to update at review: spec Part III item, status.md, roadmap row R11, `docs/design` row import-a.

## Keel findings expected
None from this change.

## The gate
- `scripts/gate.sh` failed at its **keel** phase, and I did not change any of the files involved:
  - findings from the base range, all in files I did not touch: `crates/theseus-tui/src/tests.rs` (assert-removed), `theseus-index/src/vectors.rs` and `theseus-protocol/src/tests_work_join.rs` (allow-added), `scripts/long-files.txt` (ceilings raised);
  - the guard also refused earlier owner acks whose signatures do not verify (`%G? is E`) and says `scripts/keel-signers` lists no key at the base.
- The later phases I ran by hand:
  - `cargo fmt --check`: ok;
  - `scripts/shape.sh`: ok;
  - `cargo clippy --workspace --all-targets -D warnings`: clean (after splitting four functions past clippy's limits);
  - `cargo nextest run --workspace`: 3,765 run, 3,732 passed, 33 failed. All 33 are the known L1 failures (root daemon, theseus-pv6i): `theseus-sandbox::contract` (19), `bench spawn_100` and `theseusd::sandbox` (13). No timing test failed.
- `cargo deny check` (run by hand after the gate): advisories, bans, licenses and sources ok.
- Not run: the gate's cockpit phase and the bench phases (the brief says to leave budgets alone).
