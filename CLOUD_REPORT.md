# Cloud report: cloud/20261005-reader (theseus-g7qp, theseus-t7ra)

Started 09:36 UTC, finished 10:30 UTC on 2026-10-05, from main at faaa9df6 (store format 17, unchanged: no
stored record changes). Three steps, each one commit, plus this report.

## Differences from the brief and the issue (the code wins)

- The install list is in the repo twice, as the brief says: `scripts/build.sh`'s `shipped` and
  `scripts/setup.sh`'s `SHIPPED`, each holding the same five packages in the same order. The test reads both
  as text. I wrote no third list and changed neither script.
- theseus-exam is the one tool no install ships. I gave it a third marker key, `run_from_tree = "<why, and who
  runs it>"`, under `[package.metadata.theseus]` beside its `tool`, so the claim stays with the crate the way
  the other markers do.
- Hole 2 is live on main, as the brief says. With step 1's scan, deleting theseus-core's three reads of
  `SameEntity` still passed: theseus-memory's `match` in activation.rs counted for it. With step 2's scan the
  same deletion fails. On main the stricter rule passes, because theseus-core reads all three variants itself
  (check.rs, recall.rs, reach.rs).
- Batch 6's activation branch is **not in this clone**. `git branch -r` lists only `origin/main` and this
  branch, so I could not test against main merged with it. Instead I built fixtures of that file's shape:
  `use theseus_memory::{Adjacency, EdgeKind, SpreadParams}`, then `graph::EdgeKind::X` for ours, under three
  imports of the module: `use crate::graph;`, `use crate::{graph, …};` and `use crate::graph::{self, …};`.

## Step 1: tools are checked against the install list (g7qp, issue hole 2): b7cdaf8a

**What I found.** A `tool` marker makes a crate its own reader (`reached`). The test could not know whether
the binary is installed.

**What I changed.** In `tests_registry.rs`:
- `shell_list` reads a line that starts `<name>=(…)`, as plain text with no shell.
- `install_problems(members, build, setup)` fails in these cases:
  - the two lists differ (it names the packages that are in one list only, or says the order differs);
  - a shipped package is no workspace member;
  - a shipped package is neither a root nor a tool;
  - a root is not shipped;
  - a tool is not shipped and has no `run_from_tree`;
  - a shipped tool still says `run_from_tree` (stale);
  - a crate says `run_from_tree` with no `tool`.

  Each message names the crate, the file and the fix.
- New tests:
  - `every_tool_is_installed_or_run_from_the_tree` runs the check on the real tree;
  - `the_install_lists_fail_each_disagreement` covers eight fixture cases;
  - `an_install_list_is_read_from_its_line` tests the line reader.
- `member()` accepts the `run_from_tree` key.
- `print_the_reserved_list` prints the exam's reason under its tool line:
  ```
  - `theseus-exam`: run by hand beside theseusd: … (step 34b)
    - run from the tree, not installed: the exam is a measurement, not a part of a running Theseus: whoever runs it builds it from a checkout (`cargo run -p theseus-exam`), so an install never copies it
  ```
  The reserved table itself is unchanged (empty; `RESERVED` is still empty).

**How I proved it.** The registry module passed 12/12. Planted reverts, each restored, `touch`ed, and followed
by a clean `git status`:
- I added `theseus-exam` to setup.sh's `SHIPPED` alone. `every_tool_is_installed_or_run_from_the_tree` failed
  with: "scripts/setup.sh's `SHIPPED` (… theseus-exam) differs from scripts/build.sh's `shipped` (…):
  `theseus-exam` only in scripts/setup.sh. … make it match".
- I removed theseus-exam's `run_from_tree`. The same test failed with: "crate `theseus-exam` says `tool` in
  crates/theseus-exam/Cargo.toml, but is not shipped: … add it there; or, … `run_from_tree = "<why, and who
  runs it>"`".

## Step 3: `theseus-index --version` (t7ra): 25c7ae1a

**What I changed.** I added `version` to the `#[command(...)]` in `crates/theseus-index/src/main.rs`, as
theseus-sim's has it. The new test `crates/theseus-index/tests/version.rs` runs the binary and checks that it
prints exactly `theseus-index <CARGO_PKG_VERSION>`.

**How I proved it.** The test passed 1/1. Planted revert: with `version` dropped, it failed with
"exit Some(2): error: unexpected argument '--version' found".

## Step 2: a reader counts only where the type is ours (g7qp, issue hole 4): f31c38d4

**What I changed.**
- `Home` records each counted type's home: its crate, the modules a path may name it through, and the file
  that defines it.
  - `EDGE_KIND`: theseus_core, `graph`, `graph.rs`.
  - `EVENT` and `LEDGER_KIND`: theseus_protocol, through the crate root or their private module.
- `uses_in` makes one pass over the file's tokens. It collects every `use` tree wherever it sits, with
  `crate` resolved, `{self}`, aliases and nesting; globs are ignored. It also collects every type definition of
  the same name and every site. Then it decides each site:
  - a path counts when it resolves to the home (`crate::…` within the home crate, `theseus_core::graph::…`,
    or the first segment through an imported module such as `graph::`);
  - a bare name counts only in a file that imports the home's type by name and no other type of that name, or
    in the home's own file;
  - anything else goes to `Uses.unowned`, with the file and the reason.
- When an edge kind has no reader, its message now cites that use: "… crates/theseus-memory/src/activation.rs
  names `EdgeKind::SameEntity`, but the file defines a type of that name, so it counts for nothing: if it
  means theseus-core's, name it there as `graph::EdgeKind::SameEntity` (after `use crate::graph;`)."
- **Applied to `Event` and `LedgerKind` too.** It cost nothing, and the messages of those two checks are
  unchanged. On main the scan now leaves out exactly these, all of them other crates' types or private ones:
  - the tender's private `Event::{Exited, Stop}`;
  - theseus-lsp's `Event::Closed`;
  - theseus-mcp's `Event::{PromptListChanged, ToolListChanged, Reinitialized}`;
  - aws::trail's `Event::of`.

  No `LedgerKind` use is left out (199 writers counted). For `EdgeKind`, only activation.rs's nine are left
  out.
- The inline-source tests:
  - `the_scan_counts_a_use_only_of_the_homes_own_type`: theseus-memory's shape (its own type and its reads,
    `crate::EdgeKind`, its re-export) and the adjacency file's shape under three module imports.
  - `the_scan_counts_ours_imported_anywhere_or_named_by_its_path`: a nested `use`, a path spelled out,
    `theseus_core::graph::…` from another crate, and `crate::graph::…` from outside theseus-core, which
    counts for nothing.
  - `a_bare_name_counts_only_beside_one_import_of_ours`: no import, a glob, two imports, graph.rs itself, the
    tender's private `Event`, and a sender that imports the protocol's `Event`.
  - The existing `the_scan_tells_a_build_from_a_read_and_skips_tests` now imports what it names.
- crates/theseus-core/AGENTS.md: the `tests_registry.rs` bullet says both rules.

**How I proved it.**
- The registry module passed 15/15 on main.
- Planted revert: I made a bare name count again. `the_scan_counts_a_use_only_of_the_homes_own_type` failed
  (activation.rs read `SameEntity` and `DerivedFrom`), and so did
  `a_bare_name_counts_only_beside_one_import_of_ours`.
- I also ran the live-check analog offline, in this tree and never committed: I deleted the `SameEntity`
  read in recall.rs's arm and in check.rs's and reach.rs's patterns.
  - With step 1's scanner the registry module passed 12/12, so the hole was open.
  - With step 2's scanner `every_edge_kind_has_its_reader` failed, naming `same_entity` and activation.rs.
- **Speed.** I timed five warm runs of the gate's registry command (`cargo nextest run … tests_registry::`).
  - Step 2: 1.79, 1.70, 1.72, 1.74 and 1.87 s.
  - main's file: 1.71, 1.75, 1.73, 1.73 and 1.75 s.
  - Most of each figure is cargo and nextest overhead. In the gate's own table the reader-rule phase took 1 s
    on step 1 and 2 s on step 2 (whole seconds). `every_edge_kind_has_its_reader` takes about 0.23 to 0.33 s
    either way.
  - Note: main's file exits 100 against this branch's manifests, because it rejects the new
    `run_from_tree` key. That is expected.

## Live check (the maintainer's)

1. On the merged tree: `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`. The "reader rule" phase
   should be green, at about 1 to 2 s.
2. In a throwaway worktree, once of main and once of main with this branch merged:
   - Delete `Some(EdgeKind::SameEntity) => LinkKind::SameEntity,` in crates/theseus-core/src/recall.rs, and
     change its `Some(EdgeKind::DerivedFrom) | None => continue` to `_ => continue`.
   - In reach.rs, change `Some(EdgeKind::SameEntity | EdgeKind::Supersedes) => {}` to `Some(_) => {}`.
   - In check.rs, change `Some(EdgeKind::SameEntity | EdgeKind::Supersedes) | None => {}` to
     `Some(_) | None => {}`.
   - Run `cargo nextest run --workspace -E 'package(theseus-core) & kind(lib) & test(/^tests_registry::/)'`.
   - Before the merge it should pass. After it, `every_edge_kind_has_its_reader` should fail, naming
     `same_entity`. Never commit this.
3. With batch 6 merged too, the same run should still pass unchanged. adjacency.rs's `graph::EdgeKind::X`
   reads count and its bare reads don't.
4. `~/.local/bin/theseus-index --version` from the install build should print `theseus-index 0.0.1` (the
   workspace version).

## Left, uncertain, and for the owner

- **Batch 6's adjacency.rs is untested against its real source.** I tested fixtures of its shape. If it
  reaches `graph` some other way, for example `super::super::graph` or a `use crate::graph as g;` alias, the
  scan resolves aliases but not `self::`/`super::`. Those count for nothing, and the check fails closed with a
  message.
- **Imports are counted per file, not per scope.** Two functions that import different `EdgeKind`s make
  every bare name in the file count for nothing. That fails closed, by design.
- **Docs to update** (not edited by me):
  - root AGENTS.md's reader-rule bullet, which has about 175 bytes left under its 20 KB cap, so one clause:
    "a `tool` is held to build.sh's list, and a reader counts only where theseus-core's type is named";
  - the Part III item for g7qp/t7ra;
  - docs/design/README.md, if it describes the crate markers, which should name the third key,
    `run_from_tree`.
- I made no new dependencies, no protocol or config changes, and no store format bump.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on f31c38d4's tree:
- fmt, shape, features, clippy, cockpit, test build and the reader rule (15/15) passed.
- The suite ran 2,580 tests: 2,547 passed, 33 failed, 19 skipped. The 33 failures are exactly the known L1
  ones from running as root (theseus-pv6i): theseus-sandbox's 19 contract tests and its bench's `spawn_100`,
  plus 13 of theseusd's `sandbox` tests.
- No other test failed, and none was retried.
- I ran the phases after the suite myself: protocol types ok; `cargo deny --offline check` passed
  (advisories, bans, licences and sources). The benches were skipped (`THESEUS_GATE_NO_BENCH`).
- The same result held on step 1 and step 3's tree (2,577 tests, the same 33 failures).
