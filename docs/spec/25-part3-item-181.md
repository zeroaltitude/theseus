# The Ship of Theseus, chapter 25: Part III, A4's Items 181 to 193 ([index](README.md))
### Item 181. Route gaps: a routed session keeps the base it was moved from and follows a change of that base, each loop records one `context.compiled` and one `loop.started`, and two route rules get their tests (theseus-0j2.17, theseus-d13v and theseus-g1gl; step 25e's follow-ups, from Items 139 and 150; the seventh cloud batch's route-gaps session, fired 2026-10-05 01:35 from 80ef1dea, Opus 5.5, its report at 03:21; ad5e62a1, 84250c80, 2f98fdc5 and 995f47d5; reviewed 11:29 to 13:35 by local reviewer R14, stack G, over a relaunch after the account's weekly limit, and accepted at 13:51; joined 14:05 at c9d000d7, a signed merge onto 60dd55e0 (spec v0.82's docs join) with three join fixes, by the stack-G/Z joiner; store format 20 to 21; installed 2026-10-05 16:48 at c4f79e9f, install #5)

**Why.** Three gaps left by `route.v1` (step 25e, Item 139) and by routing's sticky-state fix (Item 150):
- **theseus-0j2.17 (P2).** `Routed` held only `profile` and `hold`, so nothing knew where a session had been moved
  from. Item 150 made a routed session run on its own profile again once `route.v1` stops acting, but a change of that
  profile itself (`[model] live` for a session that follows it, or a place's bound profile) did not un-route it; and
  the CLI pane carries the profile its last turn ran on, which `turn_submit` read, when it was the routed one, as naming
  nothing, so once routing stopped a `-P` pane fell back to the live profile, not to its own.
- **theseus-d13v (P3).** `compile_step` recorded `context.compiled`, continue.v1's mark at the compile and
  `loop.started` on the first compile even while `defer_persist` held that compilation back for the verdict, and a
  switch's routed compile recorded them again: a switched turn's loop 0 had two of each, the first naming a compilation
  never stored, and a detour added a second `loop.started`.
- **theseus-g1gl (P3).** Two of 25e's rules had no test: a place's ceiling profile capping a hard question, and
  thinking sent back only to the model that wrote it.

**What landed** (`theseus-core`'s `routing.rs`, `session.rs`, `rpc/methods.rs`, `turn/route_step.rs` and
`turn/compile_step.rs`, the store's format line, theseusd's `tests/versions.rs`, four new test files; the merge 16
files, +689 −52; no package, protocol type or config key).
- **The base** (ad5e62a1, with 84250c80). `routing::Routed.from: Option<String>` (serde default, skipped when
  `None`), set in `route_to` at a switch with `get_or_insert`, so a later switch keeps the first base; a switch back to
  `from` ends the move. `turn_submit`: a pane carrying only its routed profile (the existing `only_routed` match) now
  stands for `routed.from`, if that profile is still configured. `route_base` calls `same_base` before the mode read:
  for an unpinned person's message of a routed session whose `from` differs from the turn's target, the move is cleared
  in the turn's own session write and the turn runs on the new base; a record from before `from` takes the turn's base
  at that turn, so it reads as before. Nothing new is read from the store. The base is compared by name, as an unrouted
  session follows its profile by name.
  - **A stored field, so a format bump:** 18 on the session's base (17 there), renumbered at the merge to main's plus
    one, **21** ("/// 21 = a session's routed base, `routed.from`"); `tests_layouts.rs` gains a hand-written routed
    record of formats 15 to 20.
  - **`SessionRecord.routed` is `Option<Box<Routed>>`** (84250c80): with `from` added, the golden overflowed a debug
    build's 2 MiB test stack on 80ef1dea (it passed at `RUST_MIN_STACK=2110000`, about 13 KB over), since the record
    rides by value in the turn's futures; boxed, the record is smaller than before, and its bytes on disk are the same.
    `ad5e62a1` alone is therefore not green.
  - **A behaviour change:** after `profile.use`, a routed `-P` pane runs on its `-P` profile, as an unrouted pane
    does; main ran it on the new live profile. A session that sends no profile still follows `profile.use`.
- **One row of each a loop** (2f98fdc5). While `defer_persist` is set, `compile_step` keeps the `ContextCompiled`
  summary and its trace times in `RouteState.deferred` (boxed); a new `compiled_rows` records `context.compiled`, the
  judge's mark and `loop.started` together. `keep_first` persists the compilation and records the held rows; a switch
  or a detour drops them, and a switch's routed compile records its own. In a routed turn's frame the first compile's
  rows now follow `route.decided`. **A detour's loop records `loop.started` alone**: its compilation is never stored, so
  a `context.compiled` would name nothing. continue.v1's mark is asked once for each loop that records a compile, so a
  detour's loop no longer asks it (checked by reading: the route rig cannot plant a mark deterministically).
- **Two rules tested** (995f47d5). `tests_route_cap.rs`: a private place with a ceiling profile (`sonnet`) and a
  `sophisticated` verdict runs on sonnet, the `route.decided` row says `capped`, and the session is not moved.
  `tests_thinking_writer.rs` (at the crate level: `compiler.rs` is at its ceiling): Sonnet 5.5's thinking renders in
  none of Opus 5.5's requests but its own, and in an Opus 5.5 request whose `fallback` is Sonnet 5.5 (Item 154's `also`).
- Tests: `tests_route_base.rs` 4, `tests_route_rows.rs` 2, `tests_route_cap.rs` 1, `tests_thinking_writer.rs` 1;
  theseus-core's AGENTS.md route.v1 entry names the base, the deferred rows and the tests.

**How it is proven.**
- **The session's tests:** a `-P glm` pane routed to Opus comes back to glm once routing stops, by each of routing off,
  shadow, the judge off, Jev's key gone and the ladder's rollback; a changed live profile and a rebound place's profile
  each clear the move of the session that follows them, while the pane keeps its own; an old record reads as before; a
  switched turn and the next record loop 0's rows once each, every `compilation_id` reading back; a detour records one
  `loop.started` and no `context.compiled`. Each of its five plants failed (the carried profile naming nothing again;
  `same_base` always true; the first rows recorded at once; the cap dropped; the writer check dropped). Its gate on each
  commit: on 995f47d5, 2,570 run, 2,537 passed, 33 failed, all L1 refusing a root daemon's job in the cloud VM
  (theseus-pv6i); frames 5 and 9.
- **The review** (R14, on main d50e6f2f at format 20; the first run stopped at 12:31 on the account's weekly usage
  limit, its detached plant run finishing on its own at 13:04; a relaunch at 12:40 never started on OpenClaw's cap of 16
  busy CLI sessions; the relaunch from 13:10 finished at 13:35). The build, clippy, protocol 31 and shape clean; **the
  whole workspace suite 2,768 of 2,768** (`--retries 0`, no `RUST_MIN_STACK`, at load 17 to 22). **7 of 8 planted
  reverts caught**; not caught, a probe: each switch overwriting the base (`insert` for `get_or_insert`) passed 44 tests,
  since no test makes a second switch or a switch back; the code is right (live, below): theseus-zvpl (P2). **The
  golden's stack:** main passes at 1,888 KiB, the merge at 1,872 (aborting at 1,856), so the branch costs no stack and
  leaves about 176 to 192 KiB under 2 MiB; unboxed it passes at 1,904 KiB on today's main (retention, Item 157, had
  boxed the golden's halves), so the box is worth 32 KiB and is kept. **Under 16 busy loops** the golden failed 2 of 3 on
  both arms, its 30 s wake wait each time (main's: theseus-23wh, P2), and `tests_route` 5 of 24 on main and 4 of 24 on
  the branch, verdicts read late (main's: theseus-biy3, P2).
- **Live** (R14; scratch daemons of the frozen review merge, B, and of main, A, on fresh state dirs, the stand-in
  model, Jev a loopback stand-in scripting route.v1's `mode`, `max_wait_ms = 5000`, profiles haiku, sonnet (live), opus
  and fable; every turn through `turn.submit` as the pane sends it). **B: 20 of 20 checks.** A `-P haiku` pane's hard
  question moved it to opus with one of each row for loop 0 (A: two of each); a trivial message detoured to haiku with
  one `loop.started` and no `context.compiled` (A: two and one); after `[model] live = "fable"` the session following
  the live profile ran on fable, its move cleared (A: still opus); with routing off the pane ran on **haiku**, its `-P`
  profile (A: fable); after `profile.use fable` the pane stayed haiku and a session with no profile ran on fable (both
  arms); a second switch kept the first base and a switch back ended the move (A: fable each time). **The store's
  upgrade, 6 of 6:** A wrote a routed session at format 20; B served a copy at once, at 21 by its first answer, the old
  record's base filled in; A then refused the store (exit 1, "is format 21", "this build reads formats 2 to 20",
  "install the newer theseusd") and left every file's size and hash as they were.
- **FAST** (frozen debug builds, main as A and the merge as B, alternating, PSI beside each run): frames 5 and 9 on every
  run; plain p50 A 78.9, B 79.7 ms (+0.7), tool call A 160.1, B 159.6 (−0.5); the lifecycle, rerun once the neighbours'
  builds let it, B's median p50 at or under A's in every phase (cold start A 37.6, B 27.5 ms; the quietest pair 23.6
  and 22.1). `same_base`
  is one string compare at a routed turn, and nothing is added to the start.

**What the review found.** **theseus-490i (P3, main's):** a routed turn's trace root keeps the model the turn started
on, while its `route` and `provider.call` spans say the routed one, so a switched or detoured turn's `theseus.turns`
point pairs the routed profile with the base's model. **theseus-3urn (P3, main's; the report's "Left, and
uncertain"):** a switch's routed compile runs `recall_compiled` again after the first compile took the recall's
`drops`, so the stored compilation's budget names none. **For Eddie, each recommended and joined as built:** `Routed.from`
stored rather than derived (deriving it from the ledger would add a read to each routed turn's start); the `profile.use`
change accepted and said in the docs; the deferred rows accepted (no reader depends on the order: R14 read the
cockpit's lists and chart and the CLI's watch); a detour's loop with no `context.compiled` accepted (the context chart
then shows no point for a detour, rightly).

**The join** (stack G's only branch, alone: a format bump on the turn path). R14's dry runs met the same four files on
d50e6f2f, 60b43fb6 and the DM thread's 43345eaa; the joiner's on 60dd55e0 (13:53:39) gave tree 43c30c81. The lock
`cloud-route-gaps-join` and the merge in one guarded call at 13:56:08: four conflicted files, and **rerere replayed R14's
recorded resolution of all four** (theseus-core `lib.rs`'s `mod` lines, keep-both; the format in theseus-store
`store.rs`, theseus-core `store.rs`'s pin and theseusd `versions.rs`, each side differing only in its numbers);
`resolve.py`, which reads main's `MANIFEST_FORMAT` from HEAD, never a literal, renumbered the strings in four
unconflicted files (AGENTS.md, routing.rs's docs, the layout sample's label, the old record's doc), leaving no 18 of the
branch's. **Three join fixes** (`joinfix.py`, none changing behaviour): `off: false` in `PackRollbackParams`, learn-loop's
field (Item 164); the writer test's nodes as hydrated stubs for tiering's `&[(u64, Stub)]` (Item 162); and tiering's
`#[expect(clippy::too_many_lines)]` on `compile_step` removed, since `compiled_rows` split it under 100 lines. The staged
tree, 16 files, +689 −52, equalled the dry run's byte for byte; the goldens print no store format and did not move. The
warm (13:56:45 to 13:58:16) clean; 93 of 93 of the route, routing, layout, format and golden tests at the default stack.
The signed merge **c9d000d7** (60dd55e0 and c44f35e9), 13:59:49. Its gate (ok at 14:05:37): **2,797 of 2,797** (1 slow,
21 skipped); the reader rule 9 of 9; lifecycle in every budget at the first run (cold start p50 21.9 / p95 32.7 ms;
from the config copy 22.5 / 23.1; clean shutdown 31.4 / 47.8; SIGKILL then restart 26.4 / 36.6; binary swap 45.9 /
52.9; L1 start 6.28 / 6.54); turn plain **5 frames**, 74.5 / 81.0 ms, tool call **9 frames**, 154.1 / 171.8 ms (2.0 and
1.4 ms over docs-v082's gate, inside the run-to-run spread); fdatasync p50 6.5 ms. Pushed 14:05:56; the branch deleted;
done line 14:06:03; theseus-0j2.17, d13v and g1gl closed with the hash. **The store went from format 20 to 21.**

**The install** (2026-10-05 16:48 at c4f79e9f, install #5, with situations (Item 183), whose format 22
followed this one's 21: Eddie's store went from 20 to 22 at the first write, after the install's backup, and install
#4's build would refuse it, so a rollback is the backup). No config key; after `profile.use`, a routed `-P` pane runs on
its `-P` profile. Health after the restart (16:48:09): check exit 0, 9 secrets ready 1.03 s after the start, startup
serving at 43.2 ms (store 8.4, kernel 31.0 ms, inside the 50 ms budget, the load 8 from the build that had just ended),
Discord ready, the judge's live packs as before, memory live on the `baseline` arm, the unit active with NRestarts 0,
and no error or warning in the journal.

**Divergences.** The base is compared by name, not by provider and model. `ad5e62a1` and `84250c80` are one change.
A detour's loop records `loop.started` alone, and continue.v1's mark is no longer asked for a compile the call
discards. In a routed turn's frame the first compile's rows follow `route.decided`. The one case the carried profile
cannot tell apart: an owner who wants to stay on the routed profile names it with `-P`.

**Known gaps.** theseus-zvpl (P2), the tests for a second switch and a switch back; theseus-490i and theseus-3urn (P3);
theseus-biy3 (P2): all four taken by batch 9's route-tests (Item 213). theseus-23wh (P2), the golden's 30 s
wake wait under starvation, taken by batch 9's core-waits (Item 212). theseus-b4sf (P2), the golden's stack
margin, taken by turn-stack (Item 192).

### Item 182. The reader rule's two cheap closures: a `tool` held to the install lists, a read counted only where the type it names is ours, and `theseus-index --version` (theseus-g7qp holes 2 and 4, and theseus-t7ra; roadmap v1.1's lane `reader`; the seventh cloud batch's second part, its reader session, fired 2026-10-05 02:35 from faaa9df6, Opus 5.5, its report at 03:30; b7cdaf8a, 25c7ae1a and f31c38d4; reviewed 12:05 to 13:47 by local reviewer R15, stack Z, over a relaunch after the account's weekly limit, and accepted at 13:51; joined 14:21 at ae1202af, a signed merge onto c9d000d7, by the stack-G/Z joiner; installed 2026-10-05 16:48 at c4f79e9f, install #5)

**Why.** The reader rule's registry test (Part II's rule 2 and 3, Item 32; `tests_registry` in theseus-core, run by
the gate before the suite) let four spellings pass though nothing reads the item (theseus-g7qp, carried from Item
21). Roadmap v1.1 gave lane `reader` the two cheap ones:
- **Hole 2.** A `tool` marker makes a crate its own reader, but the test could not know whether the binary is
  installed, so a tool nothing ships counted as read.
- **Hole 4.** A read counted wherever a bare name matched: on main, theseus-memory's own `EdgeKind::SameEntity` in a
  `match` in its `activation.rs` counted as theseus-core's reader, so deleting theseus-core's three reads of that edge
  kind still passed.

And theseus-t7ra: `theseus-index --version` exited 2 ("unexpected argument '--version' found").

**What landed** (`theseus-core`'s `tests_registry.rs`, 1,028 to 1,788 lines, unlisted and under 2,500;
`theseus-exam`'s manifest; one clap attribute in `theseus-index` and its new `tests/version.rs`; the merge 5 files,
+841 −49; nothing on the daemon's path: `tests_registry` is `#[cfg(test)]`; no store format, protocol type, config
key or package).
- **Tools held to the install lists** (b7cdaf8a, hole 2). The install list is in the repo twice, `scripts/build.sh`'s
  `shipped` and `scripts/setup.sh`'s `SHIPPED`, each the same five packages in the same order; the test reads both as
  text (`shell_list` reads a line that starts `<name>=(…)`, with no shell) and adds no third list. `install_problems`
  fails seven cases, each message naming the crate, the file and the fix: the two lists differ (the packages in one
  only, or the order); a shipped package is no workspace member; a shipped package is neither a root nor a tool; a
  root is not shipped; a tool is not shipped and has no `run_from_tree`; a shipped tool still says `run_from_tree`
  (stale); a crate says `run_from_tree` with no `tool` (orphan). theseus-exam, the one tool no install ships, gets a
  third marker key beside its `tool` under `[package.metadata.theseus]`: `run_from_tree = "<why, and who runs it>"`
  ("the exam is a measurement, not a part of a running Theseus: whoever runs it builds it from a checkout"), which
  `print_the_reserved_list` prints under its tool line. New tests: `every_tool_is_installed_or_run_from_the_tree` (the
  real tree), `the_install_lists_fail_each_disagreement` (eight fixtures), `an_install_list_is_read_from_its_line`.
- **A read counts only where the type is ours** (f31c38d4, hole 4). `Home` gives each counted type its crate, the
  module paths that may name it and its defining file (`EDGE_KIND`: theseus_core, `graph`, `graph.rs`; `EVENT` and
  `LEDGER_KIND`: theseus_protocol, through the crate root or their private module). `uses_in` makes one pass over the
  file's tokens, collecting every `use` tree wherever it sits (`crate` resolved, `{self}`, aliases and nesting; globs
  ignored), every type definition of the same name, and every site, then decides each site: a path counts when it
  resolves to the home (`crate::…` within the home crate, `theseus_core::graph::…`, or a first segment through an
  imported module such as `graph::`); a bare name counts only in a file that imports the home's type by name and no
  other type of that name, or in the home's own file; anything else goes to `Uses.unowned`, with the file and the
  reason. A missing reader's message cites it: "… crates/theseus-memory/src/activation.rs names
  `EdgeKind::SameEntity`, but the file defines a type of that name, so it counts for nothing: if it means
  theseus-core's, name it there as `graph::EdgeKind::SameEntity` (after `use crate::graph;`)". Applied to `Event` and
  `LedgerKind` too, at no cost and with their messages unchanged: on main the scan leaves out exactly other crates'
  or private types (the tender's private `Event::{Exited, Stop}`, theseus-lsp's `Event::Closed`, theseus-mcp's three,
  `aws::trail`'s `Event::of`); no `LedgerKind` use is left out (199 writers counted). Three new inline-source tests,
  and the existing `the_scan_tells_a_build_from_a_read_and_skips_tests` now imports what it names.
- **`theseus-index --version`** (25c7ae1a, t7ra). `version` in its `#[command(...)]`, as theseus-sim has it; the test
  runs the binary and wants exactly `theseus-index <CARGO_PKG_VERSION>`.
- theseus-core's AGENTS.md `tests_registry.rs` bullet states both rules.

**How it is proven.**
- **The session's tests:** the registry module 15 of 15 on main (12 after step 1), the version test 1 of 1. Its
  plants each failed with the message meant for it: theseus-exam added to setup.sh's list alone ("`theseus-exam` only
  in scripts/setup.sh … make it match"); theseus-exam's `run_from_tree` removed ("says `tool` … but is not shipped");
  a bare name counting again (two scan tests, activation.rs reading `SameEntity` and `DerivedFrom`); `version`
  dropped ("exit Some(2)"). The live-check analog offline: with theseus-core's three `SameEntity` reads deleted, step
  1's scanner passed 12 of 12 (the hole open) and step 2's failed `every_edge_kind_has_its_reader`, naming
  `same_entity` and activation.rs. Its gate on f31c38d4 (`THESEUS_GATE_NO_BENCH=1`): the reader rule 15 of 15; 2,580
  tests run, 2,547 passed, 33 failed, all the known L1 cases of a root daemon in the cloud VM (theseus-pv6i), 19
  skipped; none retried.
- **The review** (R15; the first run 12:05 to 12:31, stopped by the account's weekly usage limit, its detached suite
  finishing at 12:38; a relaunch at 12:40 never started on OpenClaw's cap of 16 busy CLI sessions; the relaunch from
  13:10 finished at 13:47; review commits af06c428 on 4d7cd561 and c5304a48 on 60b43fb6). rustfmt, the test build,
  clippy `-D warnings` and shape clean on both merges; the registry module 15 of 15 and theseus-index's 68 of 68 on
  both; **the whole workspace suite 2,783 of 2,783** on af06c428 (`--retries 0`, 346.6 s). **6 of 7 planted reverts
  caught**; the seventh, a glob counted in `use_tree`'s `*` arm, was void, not a gap: proc-macro2 marks the second
  `:` of `::*` joint (checked in a scratch crate), so `use a::b::*;` never reaches that arm and is read as `use
  a::b;`; plant 4b put the same claim where the scan really meets a glob, and the glob fixture and
  `every_ledger_kind_is_written` caught it. The `{self}` plant (the real adjacency file's `use crate::graph::{self,
  Edge};` importing nothing) was caught by a fixture only: the real tree's other reads of the same kinds would hide it.
- **Live** (R15). The merged tree's `theseus-index --version` printed `theseus-index 0.0.1`. **Hole 4 on today's
  main:** activation's `recall/adjacency.rs` (Item 159, joined after the session's base) reads
  `Some(graph::EdgeKind::SameEntity)` through `use crate::graph::{self, Edge};`, and the stricter scan counts it as
  ours; with the report's three deletions both scans pass (adjacency.rs still reads it), and with that arm deleted
  too, or rewritten as a bare read of theseus-memory's type, **main's scan passes (9 of 9) and reader's fails**,
  naming `same_entity` and activation.rs. build.sh's and setup.sh's lists agree with the `tool` markers on main
  (theseusd, theseus, theseus-tui, theseus-sim, theseus-index; tools theseus-tui, theseus-sim, theseus-index, all
  shipped, and theseus-exam, which says `run_from_tree`).
- **The registry rows** (the stricter scan against each branch that adds a kind, an event, an edge reader, an import
  or a marker): main itself (health-words' three `disk.*` kinds, prove-wire-in's four `LedgerKind` writes) 15 of 15;
  situations (reader's commits cherry-picked onto R7's review commit) 15 of 15, its `ContextUnadmitted` writer
  counted; route-gaps adds nothing the scan reads.
- **FAST.** Nothing on the start path or the turn, so no bench A/B. The gate's reader-rule phase on frozen test
  binaries, ten runs each in palindrome order at load 5 to 6: main's 9 tests median 0.211 s, reader's 15 tests 0.222 s,
  +11 ms; through cargo, as the gate runs it, the phase stays at about a second.

**The join** (stack Z's only branch). The joiner's linuxbrew `merge-tree --write-tree` on 60dd55e0 was clean (tree
a7215cd9, as R15 found), and on route-gaps' dry tree too; redone on c9d000d7 at 14:06:50, tree 581881b8. The lock
`cloud-reader-join` and the merge in one guarded call at 14:06:57, behind route-gaps' done line: theseus-core's
AGENTS.md auto-merged, the CLOUD files dropped, 5 files, +841 −49, the staged tree a5c2fb74 equal to the merge-tree's
less those files: **no conflict, no resolve.py, no join fix**. The warm waited for the situations lane's own gate to
leave its locked part (14:02:53 to 14:12:43, review-step.sh's design) and ran 14:12:43 to 14:14:47; rustfmt clean;
83 of 83 (the registry module 15 of 15 on the merged tree with route-gaps, theseus-index's 68 of 68). The signed merge
**ae1202af** (c9d000d7 and f1af6d5b), 14:15:18. Its gate (lock after 0 s, held 299 s; ok at 14:20:56): the reader rule
**15 of 15** (main had 9); **2,804 of 2,804** (1 slow, 21 skipped: route-gaps' 2,797 plus six registry tests and the
version test); lifecycle in every budget at the first run (cold start p50 21.8 / p95 25.9 ms; from the config copy
21.7 / 27.5; clean shutdown 32.3 / 49.5; SIGKILL then restart 25.7 / 29.8; binary swap 47.7 / 58.5; L1 start 5.82 /
8.75); turn plain 5 frames, 73.9 / 78.3 ms; tool call 9 frames, 149.7 / 161.8 ms; fdatasync p50 6.3 ms. Pushed 14:21:07;
the branch deleted; done line 14:21:15. theseus-t7ra closed with the hash; **theseus-g7qp retitled** to its holes 1
and 3 ("a sender or reader in code that never runs, a never-true target cfg") and left open. The store stays at
format 21.

**The install** (2026-10-05 16:48 at c4f79e9f, install #5). Nothing on Eddie's daemon: test code, one clap attribute
and the exam's manifest metadata. Install #4's `theseus-index` exited 2 on `--version`; install #6's read printed
`theseus-index 0.0.1` (chain log, 10-06 10:20). Health after the restart (16:48:09): check exit 0, 9 secrets ready
1.03 s after the start, startup serving at 43.2 ms (store 8.4, kernel 31.0 ms, the load 8 from the build), Discord
ready, the judge's live packs as before, memory live on the `baseline` arm, the unit active with NRestarts 0, and no
error or warning in the journal.

**Divergences.** No third install list: the test reads the two scripts as text. theseus-exam's exemption is a third
marker key in its own manifest, as `reserved_for` is, rather than a table in the test. The stricter scan covers
`Event` and `LedgerKind` too, not only edge kinds. Globs are ignored by the leaf branch, not by the `*` arm the code's
comment names (plant 4's finding; no effect on what counts).

**Known gaps.** theseus-g7qp keeps holes 1 (a sender or reader in code that never runs, such as a pub fn nobody calls)
and 3 (a target cfg that is never true), P3. R15's "For Eddie", each recommended and kept as built: imports are
counted per file, not per scope (two imports of different types under one name make every bare name in the file count
for nothing, which fails closed with a message; no non-test file does so today); `self::` and `super::` paths are not
resolved (they count for nothing; none is used that way today); `run_from_tree` accepted. Root AGENTS.md was at
19,910 of its 20,000 bytes on ae1202af, so R15's 81-byte clause (" A `tool` is held to the install list; a read
counts only where the type is ours.") or nothing; the `*` arm's comment may be reworded at a docs commit ("`::*` never
gets here: it records the module before it, which adds no type"), or never.

### Item 183. Situations: what a compile is for, a compiler input with a table of what each admits and a check that fails a turn before anything is sent, enforcing from day one with its two false positives fixed; the precedence line, testimony headers and volatile values as of a date (theseus-3nk.1, with theseus-783a; M6 step 35a, roadmap row 62; the sixth cloud batch's situations session, fired 2026-10-04 18:08 from d5a4b808, Opus 5.5, its report at about 20:00; babe497a, 2286a2cb, 93f72549 and 4fc7e91b; reviewed 2026-10-05 00:03 to 01:58 by local reviewer R7, stack J, and not accepted as built (two false positives, theseus-783a); Eddie's decision of 12:17, enforce from day one; the `situations` lane, a subagent of the DM thread, spawned 12:20 and relaunched at about 13:09 after the account's weekly limit, with the two fixes c91306ff and b096fcce and seven join fixes over its merge onto main and three merges of main into the lane; joined 14:33 at c4f79e9f, a signed merge onto ae1202af, by the lane; reviewed 14:51 by the DM thread; store format 21 to 22; installed 2026-10-05 16:48 at c4f79e9f, install #5)

**Why.** M6's step 35a (m6 §2.11, "Never silently thinner, testimony, precedence"). `compile()` had no idea what a
compile was for: its trigger string (`new_session`, `system_changed`, …) appeared only after the fact, and nothing
checked what a request carried. On the way the session found a small bug: a detour's request (`compile_detour`) is built
from the last exchanges with an empty `Sources`, so a `Recall` node among them rendered as "(its source, …, cannot be
read)" and a `Summary` there would render first. The design also asked for a fixed precedence line, headers that say
whose words, where and when, and volatile values marked as of their date.

**What landed** (`theseus-core`'s new `compiler/situation.rs` (520 lines), `turn/situation_step.rs` and
`fact/situation.rs`, `recall/render.rs`, `compiler/compaction.rs`, `turn/compile_step.rs` and `route_step.rs`;
theseus-protocol's `events.rs`; theseus-exam's oracle; the config template's text; the merge 39 files, +1,852 −212; no
package or config key).
- **The situation** (babe497a). `theseus_protocol::Situation` (in events.rs: lib.rs is at its ceiling), a closed tagged
  set: `conversation_start`, `task_start`, `continuation`, `recompile {trigger}`, `resume`, `detour`, and `unknown` for a
  newer daemon's value; `ContextCompiled.situation` is optional. **The step decides** (`TurnRunner::situation_of`, no new
  read): no compilation yet is a conversation's or a task's first compile; else the session's first compile in this
  daemon's run with nothing new brought (no `UserMessage` with this turn's id, `recall::query_of`'s own rule, so a resume
  cannot recall) is a resume; else a continuation. **The compiler settles it** (`situation::settle`, in the pure
  `compile_with`): a detour stays a detour, a new compilation is a first compile unless the ring cut it
  (`recompile{overflow}`) or another `recompile{trigger}`, an append a continuation or a resume. It is stored on
  `Compilation.situation` and rides `context.compiled`.
- **The table** (`situation::admits`), built from what the code admits, so no turn that passed would fail:

  | Situation | Admits |
  |---|---|
  | conversation start | messages, replies, results, late results, repairs, a new recall note, earlier notes, a summary, the task view |
  | task start | the above less notes and summary, plus its arrangement and a new assembled recall section |
  | continuation | everything in prefix and tail (arrangement, summary, the prefix's written section), plus a new note |
  | recompile (and unknown) | everything but lessons |
  | resume | the prefix and tail as written; no new note or section |
  | detour | messages, replies, results, late results, repairs, and (fix 1) its window's arrangement |

  Lessons are admitted nowhere until 35b. A conversation's first compile admits more than §2.11's row because a detoured
  turn persists no compilation of its own (theseus-5s8j, P3, route's question).
- **The check** (`situation::check`), a pure pass after the compile and the task view. The pieces come from the
  compilation's selection by the render's own rule (`selected`, moved out of `render_request`), each repair and the task
  view. **A piece its situation does not admit** fails as `not_admitted`; **a set that does not close** (a `tool_result`
  not answering the `tool_use` just before it; a `tool_use` with no result) as `unclosed`. The turn fails with class
  `context_unadmitted` before anything is sent, with a `context.unadmitted` row (the situation, the piece, the
  compilation) and a narrative line, and it is not retried. Repairs still come first; after a compaction the step
  re-reads the turn's kept transcript, so the summary and the assembled section are seen. The detour runs the check too,
  its nodes dropping `Recall` and `Summary`.
- **The precedence line** (2286a2cb). `compiler::situation::PRECEDENCE`, §2.11's sentence word for word, a `const` in
  every system header after the persona and, since context-honesty (Item 156), its assembly note: each session pays one
  `system_changed` recompile the day it ships, then appends.
- **Testimony headers** (93f72549). A recalled item's header, frozen in `hold_node`, names its origin (`a message from
  <author>`, or `a reply by <model>`), its place (`TurnRunner::place_name`: the bound place's name, its target, or
  `<session> on the CLI or the web UI`), its UTC time and position: `a reply by glm-5.3-flash in #harbor, 2026-09-30 14:34
  UTC (as of @18231)`. A summary's header gains its range's positions and writing model: `[Summary of 6 earlier
  messages, 2026-09-20 to 2026-09-27 (@120 to @4810), written by glm on glm-5.3-flash]`. Frozen bytes never change; the
  exam's oracle, which must equal the core's render byte for byte, names its unplaced sessions the same way.
- **Volatile values** (4fc7e91b). `render::item_header` adds `, volatile: as of <the source's UTC date>, unverified` when
  the item's shown text holds a volatile value, by the memory pass's own rule over that text, not the source's
  `memory.labeled` row (the pass labels after the fact, so the freshest values, the ones the mark is for, would have no
  row yet); so the `commit:` entity reason, which needs the tender's extractor, cannot fire at recall.
- **The two fixes** (theseus-783a, P1; the lane). **Fix 1** (c91306ff): `Situation::Detour => common || class ==
  Arrangement` (a task's arrangement is written after its brief, and a detour in a short task session holds it; main
  always sent it). **Fix 2** (b096fcce): an assembled `recall_id` with no node is not a failure: nodes are never deleted,
  so such a section was never written (its call never dispatched: the budget's refusal, a `/stop`, a kernel error), and
  the render already leaves it out. Every other closure check and admission stays.
- **The store:** `Compilation.situation`, **format 22** at the join (16 on the session's base; 21 at the lane's first
  merge; 22 once route-gaps (Item 181) took 21).

**How it is proven.**
- **The session's tests:** the table over 13 class rows by 6 situations; `settle` with the wire shape and `unknown`; a
  set that does not close names its piece; through whole cores, each compile records its situation, a resume rebuilds
  its prefix byte for byte and recalls nothing, the precedence line costs one `system_changed` recompile then appends, a
  recalled header names its place, a volatile item is marked; old headers render their stored bytes. Its plants failed
  (the closure check off; the line moved; the mark dropped). The golden: 73 `context.compiled` lines gain `situation`
  (61 continuation, 9 conversation_start, 3 task_start), and without it equal main's byte for byte.
- **R7's review** (on e6f90af3, review commit 4ff0b3fe): **1,182 of 1,182** (all of theseus-core and theseus-exam,
  theseusd's versions); **5 of 6 plants caught**, the sixth (the detour keeping `Recall` and `Summary`) a test gap with
  the code right; a 144-turn sim drive on a scratch daemon, **204 compiles and 0 `context.unadmitted` rows**; live on
  GLM-5.3 Flash with the real index (about $0.01): headers naming the place and a reply's model, the gauge's value
  `volatile: as of 2026-10-05, unverified`, an `append` and `continuation` after a restart, and on a store main's build
  wrote one `recompile/system_changed` then `append`, with the model answering in the line's order.
- **The lane's fixes:** R7's two probes became regression tests
  (`tests_route::a_trivial_message_in_a_task_session_detours_with_its_arrangement`;
  `tests_situation::a_section_never_written_is_left_out_and_the_turn_is_sent` and
  `a_compactions_next_turn_after_its_undispatched_call_is_sent`, which fails a compacting turn's call frame once, as a
  full disk would); R7's untested plant 6 is held by `a_detours_window_leaves_its_recall_note_out`, and enforcement
  keeps a whole-core test, `a_piece_its_situation_does_not_admit_fails_naming_it`. **4 of 4 planted reverts caught**
  (each fix reverted, R7's plants 6 and 1). **The hunt for a third false positive**, path by path on today's main (every
  compile goes through `compile_routed`: first compiles, resumes, continuations, every recompile trigger, compactions,
  route's first compile, switch and `keep_first`, detours and their later loops, transient retries, refusal-fallback's
  retry, tiering's stubs, ring cuts with late results, wakes, reports, files, consolidation's `Synthesis`): **none**,
  and two more triggers of the second, both closed by fix 2 (a task's first turn whose dispatch fails, then the driver's
  retry; a routed turn whose `keep_first` persisted a compaction before its dispatch failed). The suites: 1,326 of 1,326,
  then 1,408 of 1,408 on the final state; the lane gate 2,804 of 2,804.
- **FAST** (frozen debug builds, main 60dd55e0 against the lane, one hold, 12 turn runs alternating): frames 5 and 9 in
  all; p50 medians plain 76.3 against 78.1 ms (Mann-Whitney p 0.13), tool call 161.4 against 164.0 (p 0.59), the
  daemon's own times within 1.5 ms: no difference this noise can show, settling R7's inconclusive tool-call gap (176.5
  against 252 ms at load 10 to 28). Both lane lifecycle runs met every budget; main's own fourth run missed two as a
  neighbour began building. Nothing is on the start, stop or swap path.
- **Live** (scratch daemons of the lane's build, the stand-in model and a stand-in Jev, no key; a probe wrote into a
  stopped daemon's store the state each check needs, since the stand-in's tiny token counts never compact): (a) "thank
  you!" in a task session detoured to glm and was answered, no `context.unadmitted` row; (b) a compilation naming a
  section never written: answered, `append`, `continuation`; (c) a conversation holding a task's arrangement: exit 1, "a
  conversation_start compile does not admit arrangement arr_…. Nothing was sent. [class=context_unadmitted]", one row,
  no provider call; (d) main's build at format 20, the lane's build moving the store at its start's first frame, its
  first turn the precedence line's recompile, and main's build then refusing the store ("… a rollback is a restore of a
  copy taken before the upgrade").

**What the review found** (R7: "not accepted as built; ready once two small fixes go in"). The table's own claim failed
twice on main, each reproduced through the whole core (theseus-783a, P1): **a trivial message in a task session**
detoured with the task's `Arrangement` in its window and failed, where main sent it; and **after a compaction whose
call was never dispatched**, the persisted compilation named a node never written, so every later turn of the session
failed as `unclosed recall_section` until something recompiled it. R7 recommended a week in shadow, with the fixes in
either way. **Eddie chose enforcement from the first day** (12:17: "either way, we're responding to possible failures.
At least with A, the failures come straight to the user. Also, I trust your fixes."): no shadow mode and no config key.
Also **join fix 1**, needed whatever the check did: the merged debug turn future overflowed the golden's 2 MiB test
stack (frame size, not recursion), so the compile step's future is boxed at its three awaits in `compile_routed` and the
compaction's in `compile_step` (theseus-b4sf, P2). Other recompiles (model, system, tools, manual) get no assembled
section, cheap to add but a recall wait on them, left for real recall numbers.

**The join** (the lane's own, through the queue). **Merge 1** (e5bec3ea, on 4d7cd561, store format 21): eleven
conflicted paths, six resolved by R7's `resolve.py` and the rest by hand, each checked against both sides
(`situation::selected` over main's stubs in `render_request`; keep-both `mod` lines; the `compiled_summary` split given
tiering's counters; the golden regenerated). Join fixes 1 to 5: R7's box, rewritten for today's `compile_routed`; the
precedence test reading persona, `ASSEMBLY`, `PRECEDENCE`; the branch's walks taking tiering's `(u64, Stub)` transcript;
two header `format!` strings left with four arguments and three placeholders by the scripted resolution; and **a
semantic conflict**: consolidation (Item 160) builds each synthesis source's header with `recall::render::header`, to
which the branch added the place, so it now passes the source session's place. **Merge 2** (523c61a7): clean. **Join
fix 6** (3be45c83): tiering's `too_many_lines` expectation, unfulfilled once the summary split out. **Merge 3**
(27314596, route-gaps): the format note (main's 21, situations' **22**) and the compile step, where both sides had
split it (route-gaps' `compiled_rows` and `Deferred` with the lane's `compiled_summary`, so a deferred row carries its
situation); `renumber.py` moved the pin, the versions test, the sample's label and AGENTS.md; **join fix 7**:
route-gaps' new `tests_thinking_writer.rs` passes `ConversationStart` in its `CompileInput`. **Merge 4** (0ca3fbc9,
reader): clean, and reader's stricter scan counts the `context.unadmitted` writer. The lock `lane-situations-join` at
14:23:43, `git merge --no-ff -S` at 14:23:44, the message amended at 14:24:01: **c4f79e9f** (ae1202af and 0ca3fbc9).
Its gate (14:28:22 to 14:33:37, ok): **2,819 of 2,819** (1 slow, 21 skipped); lifecycle p50 / p95 cold start 20.8 /
29.6 ms, from the config copy 22.0 / 36.4, clean shutdown 33.6 / 56.5, SIGKILL then restart 27.9 / 31.9, binary swap
49.5 / 57.2; turn plain 5 frames, p50 74.5 ms, tool call 9 frames, 151.2 ms (reader's gate just before: 73.9 and
149.7); fdatasync p50 6.6 ms. Pushed 14:33:44; done line 14:33:48; theseus-783a and theseus-3nk.1 closed with the hash.
**The store went from format 21 to 22.**

**The install** (2026-10-05 16:48 at c4f79e9f, install #5, the join's own commit). Eddie's store went to format 22 at
the start's first write, after the install's backup; an older build then refuses it, so a rollback is the backup. Each
existing session's first turn after the install was one `system_changed` recompile, a full-price write of its prefix,
then appends as before (context-honesty's note had paid its own at install #4: the second and last of the pair). The
check enforces: a turn whose request its situation does not admit fails visibly and says why. Health after the restart
(16:48:09): check exit 0, 9 secrets ready 1.03 s after the start, startup serving at 43.2 ms (store 8.4, kernel 31.0 ms,
inside the 50 ms budget but above install #4's 29.1 ms, the load 8 from the build that had just ended), Discord ready,
the judge's live packs as before, memory live on the `baseline` arm, the unit active with NRestarts 0, no error or
warning in the journal. The chain log: "THE V1 BUILD IS COMPLETE AND INSTALLED: v1's 7-day count starts now."

**Divergences.** The table admits what the code admits, more than §2.11 in two rows (a first compile; a resume admits
the prefix's written notes and section). The check enforces from day one, not after a week in shadow. A missing
assembled section is no failure. A late result needs no call (a self-describing line once the ring drops its call), and
an unreadable recalled source still renders "cannot be read". Volatility is read from the shown text, not the source's
label row; "unless re-derived in the same compile" is not built. A set that does not close could not be staged live
(the render pairs each result with its call by construction), so the live failure shown is the admission's.

**Known gaps.** theseus-5s8j (P3): a detour persists no compilation. theseus-b4sf (P2), the stack margin behind join fix
1, taken by turn-stack (Item 192). The cockpit's context view should show each compile's situation and a
`context.unadmitted` failure with its piece. Recompiles other than a task's first and a compaction get no assembled
section. Summaries are not volatile-marked; LiveFact probes are filed. Lessons (35b) are admitted nowhere yet. For P8's
books: notes (`recall_note`, `recall_section`) and summaries are the classes the diary will admit; no book is built.

### Item 184. Efficiency fixes: the sampler's CPU counted once, an empty trial's calls read as unknown, and the sampler's cost bounded per process (theseus-1xxi and theseus-t412; Item 167's known gaps; the eighth cloud batch's bench-efficiency-fixes session, fired 2026-10-05 13:22 from 60b43fb6, Sonnet 5.5, its report at 14:22; efae2398 and 2c2a87ae; reviewed 17:02 to 17:35 by local reviewer R16, stack B, and accepted with no join fix; joined 18:34 at efda4e91, a signed merge onto c4f79e9f, the first of stack B's three merges under one `bench/` gate, by the stack-B joiner; in the tree at install #6, 2026-10-06 10:12 at 21bf5454, with nothing to install)

**Why.** Two gaps R11 left on bench-efficiency's record (Item 167). **theseus-1xxi:** `efficiency.machine()` subtracted
`sampler.cpu_s` from the work though the sampler already classes its own pid as outside from its first sample, so the
work's cgroup CPU read low by the sampler's CPU (live: 0.268 s against the samples' 0.36); and an old trial with no files
counted 0 model and tool calls, not unknown. **theseus-t412:** R11's join fix had scaled the host test's bound with the
process count, which measured nothing; the real bound is a cost per process.

**What landed** (`bench/harbor`'s `efficiency.py`, `sampler.py`'s head and their tests, bench/report's tests; the merge
5 files, +184 −18; standard library only). **Counted once** (efae2398): the second subtraction is gone, the fixture,
`machine()`'s docstring and sampler.py's head say so, and both spends' empty last lines give `model_calls` and
`tool_calls` `None` (bench/report already skips a `None`). The sampler keeps its pid in `outside`, since `cpu_s` starts
only after the cgroup's start is read; what stays in the work is one start-up read and one final summary. **Bounded per
process** (2c2a87ae): the host test loses its scaled bound; `SamplerCost`, over a fixture `/proc` of N = 50 and 4N, holds
at most 24 µs a process, a ratio to a bare stat read under 2.4, and 4N's cost per process under N's × 1.5 + 5 µs;
`InANamespace` holds 60 sleepers at 250 ms under 1.1% of a core, skipping with its reason where `unshare` is refused.

**How it is proven.** The session's plants each failed (the second subtraction back; the 0 back; `read_procs` reading
every `status` and `cmdline`, caught on the ratio where the absolute bound alone did not). **The review** (R16, no Rust
built): a clean merge; bench/harbor 55 and bench/report 9 under both Pythons; **6 of 6 planted reverts caught**. **The
cgroup rig** (`python:3.12-slim` on cgroup v2): at 250 ms the kernel counted the work at 20.042 s and the record read
**20.028**, main's formula 19.992, low by the sampler's 0.036; at 20 ms, 35.783 against **35.799**, main's 34.991.
**bench/report over b5's 534 trials:** model calls per trial 8.3 / 8.3 / 8.0, the unknowns matching b5's own count; arm
A's published 8.4 counts loops, two budget-stopped trials having made no call in their last loop. **One paid fix-git
trial per arm** (b5's static build and profile): both solved, Theseus $0.0540 with its work 0.185 s by the cgroup against
0.19 sampled, Claude Code $0.0469.

**The join** (stack B's first; the stack's lock, gate and push are in Item 186). **efda4e91** at
18:25:38, "Automatic merge went well", the staged tree equal to R16's review commit; no join fix. Pushed 18:34:04; done
line 18:34:19; theseus-1xxi and t412 closed with the hash.

**The install.** Nothing: `bench/` is a Python harness.

**Divergences.** The sampler keeps its own pid in `outside` rather than being excluded and its CPU subtracted. The
README's sampler-cost sentence was left for review.

**Known gaps.** theseus-99by (P3), R16's findings in test_sampler.py: the namespace test never runs on this machine (not
root; measured through `unshare --user --map-root-user`, the sampler costs 0.88 to 0.99% of a core at 60 processes,
under the 1.1% bound); `ThisHost`'s pre-existing work tolerance fails under heavy load, on main too; the new ratio bound
flaked once at load 14, its two figures timed apart; and a pre-existing test that failed under load left its sampler
running for 32 minutes. The README's sampler cost, R16's wording ("about 0.5% of a core at 60 processes on a quiet VM, up
to 1% on a loaded host"), is owed in bench/README.md. The full measured efficiency run (about $47) waited on a static
build of main (theseus-u8ig, Item 195).

### Item 185. The async bench measured: both arms sampled end to end, the Theseus record its ledger's, the Claude Code arm on MeasuredClaudeCode, and stand-ins that race as a daemon does and leave nothing running (theseus-z5ty and theseus-6xre; Item 168's known gaps; the eighth cloud batch's bench-async-measured session, fired 2026-10-05 13:22 from 60b43fb6, Opus 5.5, its report at 15:07; 23c30398, 7d369168, e98ece38 and d29f3dd0; reviewed 17:26 to 17:54 by local reviewer R16, stack B, and accepted with a join fix; joined 18:34 at 323aa616, a signed merge onto efda4e91, the second of stack B's three merges under one `bench/` gate, by the stack-B joiner; in the tree at install #6, 2026-10-06 10:12 at 21bf5454, with nothing to install)

**Why.** The async bench (Item 168) ran its two arms unmeasured. **theseus-z5ty (P2), the seam with the
efficiency record (Item 167):** TheseusAsync inherited bench-efficiency's `populate_context_post_run`, which recorded
the first `ask` alone ($0.0168 and 2 calls where the ledger held $0.0423), ran no sampler, and the scorer read
top-level `cpu_s` and `peak_rss_mb` keys the nested record never has, so every CPU and RAM cell of the async report was
empty. **theseus-6xre (P3):** two test gaps R11 left (a pending wake in `settle`, never given one; `ClaudeCodeAsync.run`,
which no test ran), a stand-in race seen once under load, and the stand-in daemon's leak found at the join: on main
each `TheseusTrial` run left one stand-in `theseusd` running (3 of 3 runs), since tearDown deleted its directory
before the daemon's next 50 ms look.

**What landed** (`bench/async`'s `score.py`, `driver.py`, `async_agents.py`, tests and README; `bench/harbor`'s
`theseus_bench.py` (`daemon_script`) and new functions at `efficiency.py`'s end, with a new
`test_efficiency_async.py`; the merge 9 files, +859 −90, the join fix's 9 net lines included; standard library and
the Harbor the agents already use).
- **The scorer** (23c30398). `efficiency(trial, result)` reads `agent/efficiency.json`, else `result.json`'s
  `agent_result.metadata["efficiency"]`, only a `bench-efficiency/1` record whose `sampler.status` is `ok` or
  `running` with a `harness` dict (bench/report's `sampled` rule): `harness_cpu_s`, `harness_peak_rss_mb`, and
  `work_cpu_s`. An unsampled trial gets `None`, never zeros. The columns are "Harness CPU s", "Harness peak RSS MB" and
  "Work CPU s".
- **The stand-ins** (7d369168, 6xre's a, c, d and e; tests only). The stand-in `theseus` lists a wake due 1 s after its
  first look, and a test checks `settle` waits for it; `health` exits 1 until the stand-in daemon has written
  `daemon.env`, and a test checks the session opened only after; the stand-in daemon exits once its directory is gone,
  and tearDown waits for its pid and then asserts nothing whose command line names the test's directory runs; the FIFO
  tests kill their CLI, tee and the FIFO's holder by pid; the stand-in's state is read and written under `flock` with
  `os.replace`, since the CLI and the daemon now write it at once.
- **TheseusAsync measured** (e98ece38). `daemon_script` links `<state>/async-driver` to `theseus`, so the driver's own
  polls (settle's `executions`, `wakes`, `wait`; the finish's reads and stops) have a `comm` that is no harness name
  and count outside, while the two asks run as `theseus`; with a sampler it starts it before the daemon through
  `setsid`, so it outlives the first command and Harbor's cancel of it. `finish_script` stops the sampler last, after
  the daemon's clean stop; `TheseusAsync.run`'s shielded `finally` runs the finish or the sampler's stop alone, so
  Harbor's timeout path stops it too. `efficiency.ledger_spend` and `theseus_ledger_record` build the record from the
  trial's `provider.call` rows by model (`spend_from: "ledger"`, the rows as `model_calls`, `cost_usd` their sum,
  `None` when one has no price), the conversation's tool calls, the sampler's summary, and the trial's wall; it
  replaces the inherited first-turn record. The daemon script's own `health` loop and `sessions open` count as harness:
  they are the arm's start, as the headless arm's `--spawn` is.
- **ClaudeCodeAsync on MeasuredClaudeCode** (d29f3dd0, with 6xre's b). The sampler starts through
  `environment.exec`, never `exec_as_agent`, so the FIFO rewrite still sees only Harbor's run command;
  `claude_code_async_record` counts the stream's results and reads the last as the session's bill (`result_reading:
  "session"`; `per_turn=True` would sum them). `ClaudeCodeAsyncRun` runs `ClaudeCodeAsync.run` itself against a fake
  environment, an interrupt-shaped injection, the stand-in `claude` and the real sampler.

**How it is proven.**
- **The session's tests**, each with a plant that failed: the scorer's fixtures made by `efficiency.record` over a
  sampler summary of the real shape (the top-level keys back: three tests, `(None, None, 31.0) != (4.5, 75.0, 31.0)`);
  settle ignoring a pending wake; `health` answering at once; tearDown's wait removed (the old leak came back); the
  record from the first turn again (`('turn', 2, 0.0168) != ('ledger', 5, 0.0423)`); the closer closing right after the
  injection (`1 != 2` answers before the input's end). **EndToEnd** on the session's debug build: reward 1, the
  sampler `ok` (368 samples over 36.7 s), `theseusd` harness and `async-driver` outside while it ran, the record's 5
  calls equal to the ledger's 5 rows and the history's 5 answers, and health's cgroup phase `none` in the VM. Five
  loaded runs of bench/async under Harbor's venv, 40 of 40 each, nothing left running.
- **The review** (R16, review commit 751474bd on efficiency-fixes' 4ff0cdda): a clean merge, `efficiency.py`, the
  stack's one seam, in separate hunks. **7 of 7 planted reverts caught** (the brief's four, and R16's three: the health
  wait removed, the polls through `theseus` again, the finish not stopping the sampler). At load about 27 the new tests
  raced the nice-19 sampler (3 and 4 of 40 failed): **the join fix** (test_driver.py only) gives `left_running` up to
  10 s for a process to exit (the sampler between its `done` marker and its interpreter's exit took up to 1.3 s more;
  a process that never exits is still listed after 10 s) and fires `ClaudeCodeAsyncRun`'s injection at 5 s (the arm's
  sampler start can take 3 s before the CLI's FIFO exists; a real trial fires at 120 s or after a tool). With it, 40 of
  40 under both Pythons, twice, at load 27 to 30.
- **Live** (R16, b5's static musl build and profile in Harbor's containers, since no static build of today's main
  existed: theseus-u8ig). The oracle: reward 1 on all six families. **Theseus, interrupt:** reward 1, settled, wall
  319 s; the record `spend_from: ledger`, **13 calls = 13 rows, $0.0441 = Harbor's cost**, the sampler `ok` (1,273
  samples), harness 0.56 s and 27.5 MB, work 0.456 s by the cgroup. **Claude Code, interrupt:** reward 1, settled in
  **192 s**, well inside the 900 s timeout (the closer's fix of theseus-70vi, Item 168, held live), the injection taken
  mid-turn, $0.0276, harness 2.15 s and 209 MB. The scorer filled every cell (Theseus 0.56 / 27.5 / 0.456; Claude Code
  2.15 / 209 / 2.25; responsiveness 49.2 s and 163 s), and over the stack's three live records it read exactly each
  record's own fields: the seam holds. **The brief's open question, settled live** (Claude Code 2.1.288 in a throwaway
  container, two one-word messages): result 2 less result 1 is exactly result 2's own `usage`, and the session log's
  sum equals result 2's `modelUsage`, so each result is the session's so far and the coded reading is right. $0.0889 in
  all.

**The join** (stack B's second, on efda4e91; the gate and push in Item 186). **323aa616** at
18:25:50: "Auto-merging bench/harbor/efficiency.py" with no conflict; `joinfix.py` "applied" three times; 9 files,
+859 −90; the staged tree equal to R16's 751474bd. In the gate, bench/async passed 40 of 40 under both Pythons (5
Harbor-only tests skipped on the host; none under the venv with `ASYNC_HARBOR=1`), EndToEnd on install #5's c4f79e9f
release-thin build, and no stand-in `theseusd`, `claude`, `tee` or FIFO holder was left running (batch 7's join had
found two leaked stand-ins: 6xre's fix holds). Pushed 18:34:04; done line 18:34:19; theseus-z5ty and theseus-6xre
closed with the hash; theseus-7gir.16 stays open (the measured runs, the OpenClaw arm, CooperBench and BFCL).

**The install.** Nothing: `bench/` is a Python harness, with no daemon code, config key, store format or package.

**Divergences.** 6xre's stand-in fixes went in second, before the measured steps, so their tests stand on stand-ins
that already behave; 6xre (b), the test of `ClaudeCodeAsync.run`, went in with the measured Claude Code arm, since it
asserts the sampler and the record.

**Known gaps.** theseus-eq1a (P3), the Theseus record's three gaps: a call a `/stop` cut is a `provider.cut` row with
estimated tokens and cost, which the record leaves out (fix before publishing the cancel family's dollars); a task
session's tool calls are not counted (one history is read); the ledger read's 1,000-row cap (no async trial comes near
it). theseus-3rjr (P3, the joiner's): bench/async's oracles leave four mktemp dirs in /tmp on every suite run.
docs/benchmarks.md should say, once the measured async run is published, that its CPU columns are the harness's and
the work's, by the sampler. The six families on both arms (about $1) waited on a static build of main (theseus-u8ig,
Item 195).

### Item 186. Recall-bench fixes: the bulk reads sized by the compiler's own rule, plain abstentions admitted, `--stale retracted` as an option, and compactions counted by outcome (theseus-523y; Item 169's smoke finding; the eighth cloud batch's bench-recall-fixes session, fired 2026-10-05 13:22 from 60b43fb6, Opus 5.5, its report at 15:35; b9e4dd6b, d4a12635, 5012648a, b0d50c47, 8e94653f and b34cca61; reviewed 17:39 to 17:58 by local reviewer R16, stack B, and accepted with a join fix; joined 18:34 at ca58d80f, a signed merge onto 323aa616, the last of stack B's three merges under one `bench/` gate, by the stack-B joiner; in the tree at install #6, 2026-10-06 10:12 at 21bf5454, with nothing to install)

**Why.** The recall bench's first live smoke (Item 169; theseus-523y, P2, before the full run) found three things.
The mark's bulk build log, sized at four bytes a token, was a context overage on Theseus (the compiler estimated
24,338 tokens for the request, 34,074 at its upper bound, against 22,154 of room), so turn 10 was refused and the
compaction came a turn late; the admission check missed "I can't tell" and "found no matches"; and the strict stale
rule failed a right answer that gave the new port and retracted the old one. Offline the bench had never seen the
overage: theseus-sim's fake model reports `input_tokens: 40` on every call, and Theseus trusts the provider's count
from a compilation's second call on, so a turn's whole history cost about 50 tokens and the stand-in smoke never rang.

**What landed** (`bench/recall`: `generate.py`, `score.py`, `drive.py`, tests and README, two new modules `tokens.py`
and `standin.py`; the merge 11 files, +1,300 −160, the join fix's 3 lines included; standard library only; no Rust).
- **The bulk sized by the compiler's rule** (b0d50c47, with b34cca61). `tokens.py` mirrors the request's census
  (`ProviderRequest::census`), the rate families (a tool result as JSON at 2.4 bytes a token, text at 3.3), the
  framing (3 tokens a message, 1 a block, 15 a tool id), the estimate's upper bound (× 1.4), the margin, the headroom
  (window − max_tokens − 4,096), the ring's 6/10, a summary's room (`SUMMARY_MAX_TOKENS`, 4,096), the 30,000-character
  result cap and `fs_read`'s 7-byte line prefix, each naming its Rust source, and `TheRustRule` reads each one from
  that file. The session found three things the brief left out: a tool result shows at most 30,000 characters, so one
  read adds at most about 12,500 tokens (main's full mark could not cross by its plan either); a summary is written
  only when the ring's kept turns at their upper bound plus 4,096 fit the budget, else the outcome is `ring`; and **the
  crossing is arithmetic**: at the mark's answer the turns before it count at × 1 and only the read at × 1.4, so
  crossing while the read turn alone stays under the budget needs the turns before the mark to outweigh 40% of the
  system prompt and tools, which no smoke window allows with one read. `plan_bulks` therefore picks the smallest window
  (in thousands) where the turns before each mark fit at `MARGIN` (15%) over their estimate, a session with no mark
  fits whole, each read turn alone stays under `alone_limit` = min(budget / 1.1, budget − 4,096), and the reads cross
  at 15% under the estimate; where the mark's one read cannot cross, **the turn after the mark**, which holds no fact
  or probe, reads one to three more logs and the crossing falls there. `bounds_of` re-derives every bound from the
  progression's own bytes; the bulks draw from a stream of their own, so resizing one moves nothing else.
  `standin.py` is a counting Messages API stand-in in theseus-sim's rule format: it reports each request's own
  estimate as `input_tokens`, answers the tool-less summary call with a summary, and matches rules against the
  person's words before the recall block. `OVERHEAD_TOKENS` was measured on a scratch daemon of the session's tree
  (27 tools): 13,528 (the join fix moved it; below).
- **Plain abstentions admitted** (b9e4dd6b). `ADMIT` takes `tell`, `pins`, `pinned` and `states` after a negator,
  `no match(es)`, `found no`, and `(unable|not able) to (find|tell|say|locate|determine|confirm|see)`; `HEDGE` gains the
  same. The scorer re-derives an abstention's check from its kind each time it scores (`check_of`), so an old run is
  scored by today's rule.
- **`--stale retracted`** (d4a12635), `strict` the default: a reply that fails its check is right, and `old_named`,
  when the new value holds and every sentence naming the old value carries a retraction word. The Supersession table
  gains *Old named*, and the report names which rule scored.
- **Compactions by outcome** (5012648a). `drive.compaction_row` keeps each `context.compacted` row's `outcome`,
  `why`, `messages`, `first` and `last` (run.json `compaction_rows`); both `compaction` and `ring` move a probe (a ring
  drops the same leading turns, with nothing in their place); the report's *Compacted at* names each outcome.
- **A leaking test fixed** (8e94653f): `LeftRunning`'s fixture `sh -c "sleep 30"` forked its `sleep`, `p.kill()`
  killed only the shell, and its child between fork and exec still named the run, so the "nothing left" scan could see
  it (3 of 12 loaded runs failed); each fixture now runs in a session of its own, killed whole, and the scan waits for
  its exec. 12 of 12 under the same load.

**How it is proven.**
- **The session's tests:** 60 at the branch's head with its debug binaries. The stand-in smoke at seed 7 end to end:
  every turn exit 0, compacted at turn 11 with outcome `compaction` (a summary of 38 messages), 9 of 9 facts
  delivered; by hand the same at seeds 8, 11 and 12. **Main's sizing on the counting stand-in** is now a test: turn 10
  exits 1, "estimated at 26,145 tokens (36,603 at the estimate's upper bound) against the 22,154 the window leaves …
  [class=context_overage]", then a compaction at 11, the live smoke's shape. Its plants (the bulk at four bytes a
  token; the alone bound's margin dropped, which also turned the mark's compaction into a ring; `ADMIT` without
  `tell`; `check_of` returning the stored check; the retraction rule accepting any naming; `MOVING_OUTCOMES =
  ("compaction",)`) each failed. Five loaded runs of the Theseus driver tests, 5 of 5. A trap met: a planted Python
  file restored in the same second as its plant kept the plant's bytecode; clearing `__pycache__` fixed it.
- **The review** (R16, review commit a5b67ad8 on async's 751474bd): a clean merge (a directory no other branch
  touches). **But on today's main the branch's own stand-in smoke failed**: the situations lane (Item 183)
  had grown the system prompt from 13,528 to **13,599** tokens (system 693, tools 12,907 for 27 tools), and the smoke's
  read turn alone sat 51 tokens under its limit, so the mark's compaction became a `ring` ("a summary of up to 4,096
  tokens would not fit beside the 24,855 the kept turns take"): a semantic conflict a clean text merge hides. **The join
  fix** (`joinfix.py`): `OVERHEAD_TOKENS = 13700` (today's and about 100 for growth; at exactly 13,599 `plan_bulks`
  ships a plan whose realized logs miss the crossing by 9 tokens, the only miss across four overheads and four seeds),
  `SMOKE_7` re-pinned to **25df5ff56f522723** (window 45000, budget 29,654; two logs of about 4,400 tokens; the read
  turn alone 131 under its limit, the crossing 204 over), and the README's numbers; the full stays at 94000 (budget
  73,904; $11.77 estimated at seed 7; digest 6a022a24498bda8f). With it, bench/recall **60 of 60 under both Pythons,
  no skips**, on today's build. **6 of 6 planted reverts caught**, the join fix's own (the constant back to 13,528)
  included.
- **Live** (R16; the smoke on the Theseus arm, memory arm baseline, today's c4f79e9f release-thin build in a throwaway
  container, $0.7248): **30 turns, every turn exit 0** (the overage is gone), 9 of 9 facts delivered, nothing left
  running; compacted at **11 (compaction)**, the mark's crossing at the turn after it as planned with its summary
  written, **19 (ring)** and **28 (compaction)**: session 2, planned to fit whole, filled twice, since the live replies
  ran longer than the plan's 400 bytes (mean 521) beside its recall notes. Strict scores: recall 83%, abstention 50%.
  p003's "I couldn't find …" is now right; p006 is a right abstention that strict abstention fails (it named the
  workspace's one other ticket only to say it is a different issue); and p002's indirect probe was declined because the
  model read the recalled fact as "an old session's testimony" (the situations lane's headers), which the scorer rightly
  counts as not done.

**The join** (stack B's third and last; the stack's one join and its gate). The lock `cloud-b8-bench-stack-join` and
the first merge in one guarded call at 18:25:29 on c4f79e9f (Items 184 and
185). **ca58d80f** at 18:26:00: clean; `joinfix.py` applied to generate.py, test_generate.py
and README.md; 11 files, +1,300 −160; the staged tree equal to R16's a5b67ad8. After the third merge `HEAD:bench` was
**9fa3f9b0**, R16's review tree and the dry run's, and nothing outside `bench/` changed (the three merges: 24 files,
+2,343 −268). **The gate was `bench/`'s** (18:26:11 to 18:31:12, detached; install #5's c4f79e9f release-thin binaries
copied read-only): harbor 63 (8 skipped on the host, 1 under Harbor's venv), report 9, async 40 under both Pythons, and
recall 60 with 3 skipped on the first run, since the brief's bin-dir list left out `theseus-index`, which the Theseus
driver tests need; with the same build's `theseus-index` added, recall 60 of 60 under both Pythons, none skipped
(18:31:42 to 18:32:49), its stand-in smoke compacting at the mark with outcome `compaction`, so 13,700 fits the c4f79e9f
build. No failure at a load of 9 to 18, no leftover sampler or stand-in. The scrub: 24 files, 2,343 added lines, 2
known false hits (loopback `http`), 0 for names, paths and key shapes. Pushed 18:34:04 (c4f79e9f..ca58d80f); the
three branches deleted; done line 18:34:19; theseus-523y closed with the hash; theseus-7gir.17 stays open (the full
runs, OpenClaw's recall driver, the milestone runs).

**The install.** Nothing: `bench/` is a Python harness, with no daemon code, config key, store format or package.

**Divergences.** The second read at the turn after a mark is a design change the brief did not ask for (the
alternative was a smoke mark that never crosses); it holds no fact or probe, so a compaction at the mark or the turn
after puts every probe in the same bucket. The full's window at 94k is under Claude Code's 100k `--autocompact`, so its
arm gets `/compact` after each mark, as the smoke does: both arms compact at the marks with their own summarizers.

**Known gaps.** theseus-5dey (P3): `--stale retracted` scores an old value given as current right when a retraction
word shares its sentence ("It's 27340, previously 38013."); strict, the default, is sound, and every published report
stays strict until it is fixed. theseus-dp3y (P3): the smoke's plan has no room for overhead growth, and `plan_bulks`
checks no realized bound; the drive should check its daemon's real overhead against the plan. `REPLY_BYTES` (400) and
`MARGIN` are guesses (live replies averaged 521 bytes): R16's call was to run the full as planned and budget about $16
an arm, not $11.77, then set `REPLY_BYTES` from its turns. A recall bin dir needs all four binaries, `theseus-index`
included. The full runs (recall on Theseus first, about $12 to $16 an arm and 1.5 to 3 hours) were Eddie's GO of 16:43,
held by the DM thread for a quiet machine.

### Item 187. Memory checks: the adjacency projection's warm build paced by pressure and never past a clean stop, with tests for retention's shadow rule, activation's additions and a stub's kind (theseus-3edq, theseus-cn0b, theseus-syxg and theseus-q0qe, with theseus-1o8i by join fix 1; the known gaps of Items 157, 159 and 162; the eighth cloud batch's memory-checks session, fired 2026-10-05 13:22 from 60b43fb6, Sonnet 5.5, its report at 14:28; 1a0df0a8, 7234b0d3, 1aa3876b and 39835348; reviewed 17:00 to 18:10 by local reviewer R17, stack M, and accepted with join fix 1, the DM thread's step line at 18:53; joined 19:06 at 85862603, a signed merge onto ca58d80f, by the stack-M joiner; installed 2026-10-06 10:12 at 21bf5454, install #6)

**Why.** Four gaps from the memory joins. **theseus-3edq (P2, Item 159):** the adjacency projection's build walked
every page with no pressure pace; its page walk is shared with `refresh`, which runs inside a turn's deadline and must
never wait, so the build needed a pace of its own before `+activation` meets Eddie's store. **theseus-cn0b (P2, Item
157):** nothing held that a shadow turn never asks retention's projection (R5's plant, a turn under any arm building
it, passed the suite). **theseus-syxg (P2, Item 159):** nothing held that activation's additions skip what the turn
already holds. **theseus-q0qe (P2, Item 162):** nothing held that `stub::Kind::of` agrees with the peek's kind for every
`Body` variant.

**What landed** (`theseus-core`'s `recall/adjacency.rs` (514 lines) and, by join fix 1, `startup.rs`; four new or
extended test files; the merge 8 files, +530 −14; no store format, config key, protocol type or package).
- **The warm build paced** (1a0df0a8). `Projection::build_paced` and `refresh_paced` take a pace closure, called before
  every page after a walk's first (the nodes, the edges, both ledger paths); `build` and `refresh` pass a no-op.
  `Adjacent::build(store, paced)`: the warm build after serving passes true and waits `quiet_blocking(BOUND)`, at most
  10 s, between pages of 4,096 records; a search's own build (`Ask::run`) passes false, since it holds the projection's
  mutex inside a person's deadline. The "is built" log line gains `waited_ms` beside `took_ms`. `warm_labels`'s own walk
  (one kind's rows) was left unpaced.
- **Three tests** (7234b0d3, 1aa3876b, 39835348; tests only).
  `tests_retention::a_shadow_turn_and_a_live_baseline_turn_leave_retention_unasked` (shadow under `+retention`, and live
  under `baseline`: the projection stays `Unasked`). `tests_activation_arm::activations_additions_skip_what_the_turn_already_holds`
  (2 context nodes, 2 index hits and 25 others sharing one commit: 20 additions, all among the others, none held, none
  twice); the session found that a spread never reaches its seeds, so only the context nodes make that plant fail and
  `exclude`'s hold on the hits is a second guard. `tests_stub_kinds::a_stubs_kind_agrees_with_its_body`: one sample per
  `Body` variant from a `match` with no wildcard (a new variant fails to compile there), written, the store reopened,
  each stub read from its bytes (none hydrated, `decodes() == 0`), its kind equal to `Kind::of` and to a hand-written
  table.
- **Join fix 1** (theseus-1o8i, R17's `joinfix.py`): `startup::stop_has_begun()` reads the stop's own `STOP_BEGAN`, which
  `stop_record` sets for a client's `shutdown`, SIGTERM and SIGINT, and `adjacency::pace()` waits with
  `quiet_blocking_unless(BOUND, stop_has_begun)`: once a stop begins, the rest of the walk runs unpaced, as main's whole
  build did. That is main's own rule for paced passes since linux-io (Item 151): the learning tender and consolidation's
  nightly plan wait "never past a stop". Its test, `tests_activation_pace::a_clean_stop_ends_the_warm_builds_waits`,
  reruns the test binary in a user and mount namespace (`unshare -rm`) with `/proc/pressure` faked busy: a three-page
  warm build waits at its first pace, the stop begins at 1.5 s, and the build ends at 2.11 s; where namespaces cannot be
  made it says so and passes.

**How it is proven.**
- **The session's tests**, each with a plant that failed: the pace in `for_each` for every walk ("a search's build
  waits for no one"); `if true {` in `scene()` (retention `Ready`, not `Unasked`); `exclude`'s skip deleted (18
  additions admitted, not 20); `Body::Synthesis => Kind::Summary` in `Kind::of`. With no pressure the pace costs its PSI
  reads (9,000 nodes, debug: 68/48/47 ms unpaced, 88/49/47 paced). Each new test 5 runs of 5 under load.
- **The review** (R17, review commit 2f93df99 on c4f79e9f, with join fix 1 at f3ea03fa). A clean merge (lib.rs keeps both
  sides' `mod` lines); the build, the golden at the default stack, clippy, protocol 31 and shape clean; **all of
  theseus-core, 1,274 of 1,274**, with no `context_unadmitted` in the log. **6 of 7 planted reverts caught**, three of them
  new forms of the claims (the shadow scene taking the config's arm; the turn's own nodes not passed to the exclusion;
  the peek reading a synthesis as a summary, which `tests_tiering` beside it does not catch); not caught, a search's own
  build made to pace: a test gap, the code right (theseus-e21m, P3). Join fix 1 reverted: its test failed after 20.4 s.
- **Live** (R17; scratch daemons of the merged debug build on a synthetic store of 10,000 nodes and edges, the stand-in
  model, theseus-index in BM25, `[memory] mode = "canary"`, each daemon in its own user and mount namespace with
  `/proc/pressure` a bind-mounted stand-in, busy at IO 55% or quiet at 0.5%, so the machine's real pressure never reached
  it). The paced warm build: health `adjacency building`, a turn during it answered in 0.19 s with activation `building`,
  then "is built nodes=10000 edges=10000 … took_ms=20281 waited_ms=20000"; quiet, took_ms 98 and waited_ms 0. A search's
  own build under busy pressure: 0.49 s, `outcome: ran`, waited_ms 0. **A clean stop during the paced build** (the
  finding): the unit ended after 17.00 s and 19.34 s on the branch (main 0.24 s), since theseusd's `drop(rt)` waits for
  the blocking pool and `quiet_blocking` ends at no stop; the hold grows by up to 10 s per 4,096 records of each kind,
  past systemd's 90 s stop timeout on a big store (theseus-1o8i, P2). **With join fix 1 the same stop ended in 0.43 s**
  (the build: waited_ms 4,000, ended at the stop). A search during a paced warm build waited on the mutex for its whole
  recall deadline and answered `deadline` (2.06 s at a 2,000 ms deadline; the default is 250 ms).
- **FAST** (the stack's A/B with synth-headed, frozen debug builds, one hold, load 9 to 14): turn frames 5 and 9 on both
  arms, plain p50 88.2 against 83.4 ms, tool call 195.5 against 189.2; lifecycle cold start p50 39.6 against 41.0 ms and
  every other phase's p50 12 to 28 ms lower on the stack; one run of each arm missed budgets at a neighbour's load.
  Nothing moved onto the start path (the warm build runs from `warm_activation` after serving, and only under
  `+activation` in canary or live) or a turn's (a refresh passes a no-op pace).

**The join** (stack M's first). The DM thread's step line accepted R17's review at 18:53 and spawned the joiner. The
dry run on ca58d80f (18:53:53): a clean merge, join fix 1's four edits applied, `MANIFEST_FORMAT` 22 before and after,
the result R17's tree plus main's 24 `bench/` files. The lock `cloud-memory-checks-join` and the merge in one guarded
call at 18:56:17: lib.rs auto-merged, no conflict; join fix 1 applied; 8 files, +530 −14; the staged tree ec953a5d equal
to the dry run's byte for byte. The warm (18:56:33 to 18:58:35) clean; 30 of 30 of the activation, retention, stub,
tiering and golden tests; join fix 1's test alone in its namespace: "the build ended 2.064646725s after it began; the
stop began at 1.5 s". The signed merge **85862603** (ca58d80f and b73975eb), 18:59:42. Its gate (18:59:47 to 19:06:08,
ok; gaming mode on, one compiling tree): **2,824 of 2,824** (1 slow, 21 skipped; the branch's 4 tests and join fix 1's
1); lifecycle's first run missed one budget (from the config copy, p95 63.5 ms against 57) and the gate's own rerun met
every one (cold start p50 28.6 / p95 38.4 ms; from the config copy 28.8 / 36.8; clean shutdown 51.5 / 59.2; SIGKILL
then restart 48.0 / 55.6; binary swap 95.3 / 105.3); L1 start p50 8.45 ms; turn plain 5 frames, p50 90.0 ms, tool call
9 frames, 185.1 ms (above c4f79e9f's 74.5 and 151.2 under gaming mode; R17's same-hold A/B showed no cost). Pushed about
19:06:20; the branch deleted; done line 19:06:35. theseus-3edq, cn0b, syxg, q0qe and 1o8i closed with the hash; e21m
open. The store stays at format 22.

**The install** (2026-10-06 10:12 at 21bf5454, install #6). Nothing changes on Eddie's daemon at the install: the paced
build runs only under `[memory] arm = "+activation"` in canary or live, and his config runs `baseline`. With 1o8i
closed, naming `+activation` later is his call. Health after the restart (10:12:34): check exit 0, 9 secrets ready
1.05 s after the start, startup serving at 45.3 ms (store 4.6, kernel 36.9 ms, at a load of 11 to 19, not a quiet
reading), Discord ready, the judge's live packs as before, memory live on the `baseline` arm, voice ready, `cgroup:
delegated`, the unit active with NRestarts 0, and no error or warning in the journal; Eddie's store went from format 22
to 23 at the first write (soul-import's, Item 201), after the backup.

**Divergences.** A search's own build is never paced (a person waits on it, and a pace would only make it time out
while the thread works). `warm_labels` stays unpaced. The pace's stop rule came at the join, not from the session: its
prompt pointed at an older pacing pattern.

**Known gaps.** theseus-e21m (P3): no test holds that `Ask::run` passes `false`. A search during a paced warm build
answers `deadline` after its whole recall deadline: R17 noted it on 1o8i, which join fix 1 closed, so the batch-9 writer
filed theseus-6fn.14 and the batch-8 harvest wake 3 filed theseus-zv4x (its duplicate); both were taken by batch 9's
memory-tests (Item 210), with e21m. R17's "For Eddie", each recommended: a shadow daemon on `+retention`
should not warm the retention projection at startup (no shadow turn reads it, and the warm walks every memory row for
no reader; one condition in `Core::warm_retention`, not changed); keep `+activation` unnamed until 1o8i is closed (it
is); leave `warm_labels` unpaced. Not measured: the retention warm on a store with memory rows (`synth-store` writes
none), and the paced build on a large real store with a stop during it. theseus-core's AGENTS.md adjacency bullet does
not yet say the warm build waits while the machine is busy, never past a stop.

### Item 188. A headed synthesis: its leading heading set aside before the checks and kept off its node, and a cluster rejected for its form proposed again, once (theseus-8edz; Item 160's known gap, which kept consolidation's nightly writer off; the eighth cloud batch's synth-headed session, fired 2026-10-05 13:22 from 60b43fb6, Opus 5.5, its report at 15:23; 47fa585c, 604f8455, 5f673224 and ff00fc31; reviewed 17:00 to 18:25 by local reviewer R17, stack M, on memory-checks' review merge, and accepted with no join fix; joined 19:18 at 4a449460, a signed merge onto 85862603, by the stack-M joiner; installed 2026-10-06 10:12 at 21bf5454, install #6, with the nightly writer back on at $0.50 a day)

**Why.** Consolidation's live check with GLM and Jev (Item 160) found glm-5.3-flash heading its entry with a title
("Kestrel relay. The Kestrel relay is a service that listens on port 7714 [1]. …"): the deterministic check rejected it
as "sentence 1 cites no source" before Jev was asked, and the cluster was never proposed again (theseus-8edz, P2). So
install #4 set `[memory] synth_limit_usd_per_day = 0` on Eddie's config by his 12:03 call, and consolidation's nightly
writer stayed off until 8edz was fixed. The session found more forms: a title with no stop on its own line ("Kestrel
relay", "# Kestrel relay", "**Kestrel relay**") was glued to the next sentence, passed when that sentence cited, and
stayed glued in the stored text; and a long Markdown heading ("# The Kestrel relay was moved to a new host in May") was
an uncited claim riding inside a cited sentence.

**What landed** (theseus-memory's `consolidate.rs`; theseus-core's `consolidate/run.rs` (924 lines) and
`tests_consolidate.rs`; both crates' AGENTS.md consolidation entries; the merge 6 files, +534 −25; no store format,
protocol type, config key or package; INSTRUCTIONS unchanged).
- **The heading set aside** (47fa585c, pure). `consolidate::entry(text) -> Entry { heading, text }`. Only the first
  line or the first sentence can be a heading, and a heading never cites (a `[` anywhere in it rules it out). **Marked:**
  a first line that is a Markdown heading (`#` and a space) or wholly bold (`**…**`, `__…__`), of at most
  `HEADING_WORDS` = 8 words, set aside by its form even with nothing after it (the entry is then empty, and `check` says
  `Empty`). **Plain:** a first line (when the text has more than one) or a first sentence, of at most 8 words, every one
  of which appears in the sentence after it (lower case, end punctuation trimmed): a title restates its subject, so a
  sentence that says something new is never set aside. `check` now reads a marked first line that `entry` did not set
  aside (too long, or citing) as a sentence of its own, so the long Markdown heading fails `Uncited(1)` instead of
  passing glued.
- **The run checks the entry** (604f8455). `synthesize` takes `entry(&text)`; `verdict` checks the entry's text, so its
  sentences are numbered from 1 after the heading and Jev is asked about those alone; the node's text and the shadow
  score's tokens are the entry's. **The `synthesis.proposed` row keeps the model's answer whole** (its readers: the
  plan's `done`, where a non-empty text marks a call that answered; the day's spend; `theseus ledger`), so a
  heading-only answer does not look like a failed call proposed on every run. **`synthesis.checked` gains a `heading`
  key**, the heading set aside or null, a key in the row's JSON `data`, so no stored field and no format change. The
  report's `text` (what `theseus memory consolidate` prints) is the entry when kept and the whole answer when rejected.
- **A form rejection comes back once** (5f673224). A form rejection already wrote `judgment: null` and Jev's a
  judgment, so no field was needed. `consolidate_plan` reads the `synthesis.checked` rows too, and `done_clusters`
  joins them to the proposed rows by `synthesis_id`: a cluster with answered rows is done unless every answer was
  rejected for its form and it has fewer than `FORM_TRIES` = 2; Jev's rejection, a kept synthesis, an answer with no
  checked row and a failed call count as before. The plan is read once a run, so a retry never comes in the same run. A
  dry run says why a cluster is back: "proposed again, once: its last answer was rejected for its form (sentence 2 cites
  no source)".
- **A test's snapshot race** (ff00fc31, found while proving). Under load,
  `a_cluster_becomes_one_checked_synthesis_and_a_dry_run_writes_nothing` failed its "a dry run writes nothing" in 3 of
  8 runs: the asking turns' shadow `judge.call` rows (Jev on in that test) landed after `turn()` returned, during the
  dry run. The test now snapshots once the store has held still for 1 s (at most 20 s), and its message names the kinds
  of any rows written after the snapshot; 6 of 6 loaded runs passed after it.

**How it is proven.**
- **The session's tests.** theseus-memory 64 passed, among them `a_leading_heading_is_set_aside` (eight forms) and
  `what_is_not_a_heading_is_still_rejected` (short new-saying sentences, a 9-word restated sentence, a title of
  unrestated words, a long marked line, a citing title kept, …). Through the core:
  `a_synthesis_headed_by_its_title_is_kept_without_it` (the live form with the stand-in model and the fake Jev
  supporting: `supported`, the node's text the entry, the proposed row the answer whole, the checked row `heading:
  "Kestrel relay."`, Jev's request exactly the entry's three sentences) and its Markdown twin;
  `a_cluster_rejected_for_its_form_comes_back_once` (two calls in all, then none); `a_form_rejections_retry_that_passes_is_kept`.
  Its plants (any short uncited first sentence set aside; the set-aside skipped; form rejections never done; a Jev
  rejection read as one of form) each failed. A live run on a scratch daemon of its build: the uncited entry rejected and
  listed again, then the headed entry kept `unchecked` without its heading.
- **The review** (R17, review commit c322012b on memory-checks' join fix f3ea03fa over c4f79e9f). A clean merge
  (`consolidate/run.rs` and both AGENTS.md files auto-merged); **109 of 109** (theseus-memory whole, every core
  consolidation, citation, synthesis, activation, retention, stub and tiering test, the golden); **the stack's whole
  workspace suite 2,830 of 2,830** (`--retries 0`), with no `context_unadmitted`. **7 of 7 planted reverts caught**, five
  new forms of the claims (the node keeping the heading; `FORM_TRIES` at 1 and unbounded; `check`'s marked-line split
  removed; the 8-word cap dropped). Semantic meeting points read side by side: situations' testimony header
  (Item 183) goes into the synthesis request, but Jev's citation input carries the sources' texts only, and the
  synthesis call goes to the provider with no compile step, so situations' check never meets it; route-gaps' row change
  (Item 181) does not shift the dry-run test's count.
- **Live** (R17; scratch daemons of the stack's frozen build, the stand-in model, theseus-index in BM25, `[memory] mode =
  "shadow"`, the judge off, no key). **Story A:** the uncited entry `rejected` ("sentence 1 cites no source"); the dry run
  listed the cluster "proposed again, once"; the headed entry ("Kestrel relay." on its own line, then three cited
  sentences) kept `unchecked`, its text without the heading; the checked rows `heading` null, then "Kestrel relay."; the
  second proposed row the answer whole; the last dry run "0 clusters from 6 recalls · $0.0005 of $0.50 spent today ·
  skipped 1 for synthesized". **Story B:** two uncited answers each `rejected`, then the dry run and a real third run list
  no cluster and make no call (the spend stayed at $0.00048, the proposed rows at 2). No `context.unadmitted` row in
  either store.
- **FAST.** Nothing on the start path or a turn's: consolidation runs after serving (the nightly tender) or on an owner's
  `memory consolidate`; the plan now also pages the checked rows, on its nice-19 `SCHED_IDLE` thread; a form rejection
  costs one more call, once, under the day's limit. The stack's A/B is in Item 187.

**The join** (stack M's second, on memory-checks' 85862603). The lock `cloud-synth-headed-join` and the merge in one
guarded call at 19:07:43: AGENTS.md and `consolidate/run.rs` auto-merged, no conflict, no join fix; 6 files, +534 −25;
the staged tree 9faf8fc9 equal to the dry run's byte for byte. The warm (19:07:48 to 19:11:24) clean; 110 of 110 (64 in
theseus-memory, 46 in theseus-core with the golden). The signed merge **4a449460** (85862603 and 316e2b4c), 19:12:14.
Its gate (19:12:17 to 19:18:22, ok): **2,830 of 2,830** (1 slow, 21 skipped), equal to R17's whole-stack run; lifecycle in
every budget at the first run (cold start p50 29.8 / p95 36.6 ms; from the config copy 28.6 / 38.1; clean shutdown 37.5
/ 54.5; SIGKILL then restart 32.6 / 43.6; binary swap 51.7 / 55.3); L1 start p50 8.59 ms; turn plain 5 frames, p50 80.0
ms, tool call 9 frames, 167.0 ms (under gaming mode). Pushed 19:18:31; the branch deleted; done line 19:18:36;
theseus-8edz closed with the hash. The store stays at format 22.

**The install** (2026-10-06 10:12 at 21bf5454, install #6). With 8edz fixed, consolidation's nightly writer went back
on: the install's config set `[memory] synth_limit_usd_per_day = 0.50` (from 0; Eddie, 2026-10-06 09:16: "50 cents a day
is perfect"), with hour 4 and `synth_profile = "session"` as before. R17's costing: no past form rejections exist on
Eddie's store (install #4 shipped consolidation at a limit of 0, which stops before any call and writes no row), so a
night is one call per new cluster, each reserving at most about $0.025 on Opus 5.5 and typically spending under $0.01,
so $0.50 covers at least about 20 syntheses, each then judged by `citation.v1` in shadow; under `arm baseline` no
synthesis is shown to a model. The install's dry run read "0 clusters from 24 recalls · $0.0000 of $0.50 spent today".
Health after the restart (10:12:34): check exit 0, 9 secrets ready 1.05 s after the start, startup serving at 45.3 ms
(store 4.6, kernel 36.9 ms, at a load of 11 to 19), Discord ready, the judge's live packs as before, memory live on the
`baseline` arm, voice ready, `cgroup: delegated`, the unit active with NRestarts 0, and no error or warning in the
journal; the store from format 22 to 23 at the first write (soul-import's), after the backup.

**Divergences.** Of the issue's three parts, (a) and (b) were built and (c) was not, as the brief said. The proposed row
keeps the answer whole while the node keeps the entry, with the checked row's `heading` saying why they differ. A long
Markdown heading now fails `Uncited(1)` (a tightening, accepted), and by R17's reading so does a second heading under the
first. Past form rejections are proposed once more after the upgrade (none exist on Eddie's store). A heading-only answer
is rejected `Empty` and gets its one retry.

**Known gaps.** The dry-run test's wait narrows the snapshot race rather than closing it: a shadow `judge.call` landing
more than 1 s after the previous write could still meet the dry run (a wait on the judge's pending calls would close it).
A plain unstopped first line that is not a title still glues to the next sentence, and an inline "Kestrel relay: The
Kestrel relay …" is not set aside (both pass glued, as before; splitting every newline would fail hard-wrapped
entries). INSTRUCTIONS is unchanged: R17's call is to look after a week at the checked rows with `verdict: rejected`,
`judgment: null`, `heading: null` and "sentence 1 cites no source" whose answer opens with a title, and add "No title."
only if they are a real share. The live check with the judge on and a real model that titles its entries is the
maintainer's (it needs keys).

### Item 189. Timing flakes: four tests that failed under load wait on what they test, not on the clock (theseus-cs71, theseus-ynia, theseus-1n2y and theseus-qjd6; the gate's known failures under load, among them those Items 158 and 173 filed; the eighth cloud batch's timing-flakes session, fired 2026-10-05 13:22 from 60b43fb6, Opus 5.5, its report at 14:57; b1dbc26e, ac4b747a, 3981a621 and 9c98b540; reviewed 17:00 to 18:10 by local reviewer R18, stack F, and accepted with no join fix; a first gate red at 20:04 in main's own tests across the hour (theseus-5a50), the merge parked; re-gated mid-hour by a second joiner and joined 20:32 at 118b8291, a signed merge onto 4a449460; in the tree at install #6, 2026-10-06 10:12 at 21bf5454, with nothing to install)

**Why.** Four tests failed under a loaded suite and cost every reviewer reruns: **cs71** (P2, Item 173), the setsid
cancel test's 1.5 s round-trip bound (loaded suites read 1.507 and 2.077 s); **ynia**, sh's Ctrl-C test typing `echo
back` before the shell drew its interrupt prompt; **1n2y** (P3), Python's REPL test sending Ctrl-D before the `>>> ` after
`KeyboardInterrupt`, which the REPL then reads as nothing (found with a pty probe); **qjd6** (P3, Item 158), a starved
test runtime answering a post after the tuning's 2 s timeout, so the exporter retried and the receiver counted 4 traces
for 3.

**What landed** (theseusd's `tests/job_approval.rs`, theseus-core's `term/tests.rs` and `telemetry/tests.rs`, 2,491
lines; the merge 3 files, +59 −6; test code only). **cs71** (the issue's option 1): the test bounds the wrapper's own stop,
the verdict's `ms`, under `STOP_GRACE / 2` (1,000 ms), keeps a loose round trip, `STOP_GRACE + ANSWER_WAIT` (5 s, the
daemon's own wait), and prints both each run; a stop that waits out its grace reads 1,999 ms (the loop's `>=` and
`as_millis`' truncation), why a bound of the whole grace missed the plant. **ynia:** after the `sleep` is seen dead, it
reads until `"echo after\n^C\nok> "`, the prompt the interrupt draws, then types (theseus-y6zr's precedent; matching
`back` wherever it lands would pass for the wrong reason). **1n2y:** each key at the state it is meant for: the second
Enter after `"while True: pass\n... "`, Ctrl-C once Python's `/proc/<pid>/schedstat` CPU time has risen 20 ms (`stat`'s
ticks lagged), Ctrl-D after `"KeyboardInterrupt\n>>> "`. **qjd6:** it counts distinct `traceId`s (3), however often each
was posted.

**How it is proven.** The session's runs under the recipe passed 30 of 30 each (cs71's `ms` 10 to 14), and its plants
failed (the stop's early return removed: `1999 ms`; Ctrl-C's byte dropped; a turn's spans split across two trace ids).
**The review** (R18, frozen binaries, each test alone at nice 19 beside 16 nice-0 loops, A B B A, 40 runs an arm, CPU PSI
27 to 60%): **main failed 28 of 160, the branch 0 of 160**: qjd6 11 to 0, 1n2y 16 to 0 (this machine's Python 3.14; the
cloud's 3.11 never failed it), ynia 1 to 0, cs71 0 and 0 (its round trip reached 1.35 s, its `ms` 165). **4 of 5 planted
reverts caught**; every trace posted twice passes qjd6's test by design while nine other telemetry tests fail, so no
coverage is lost. The fifth is cs71's deliberate trade: a 2.5 s stall in the cancel's answer passes the 5 s bound, so the
round-trip promise moves to a bench (theseus-nh1k, P2). The report's "`term.send` returns once its screen settles" is
wrong on main (only `term.open` waits); the 1n2y test polls, so it does not rely on it.

**The join** (stack F's first). **First take** (the stack-F joiner): lock at 19:53:50 on 4a449460; a clean merge, then
`git rerere` died on two 0-byte locks left at 19:51:05 by a background maintenance that OpenClaw's exec tool killed when
the DM thread's fetch ended; removed, the checks finished by hand. The signed merge **118b8291** (4a449460 and fec3a04e),
19:59:02. Its gate **failed in the suite, 2,828 of 2,830**: main's two AWS runaway-mode tests ran from about 19:59:57 to
20:00:00, and the product reads the real clock (the hour's line is `hour_of(now)` to `+ HOUR_MS`), so a turn after 20:00
saw a fresh hour and was admitted; mid-hour the same build passed both. By the rules the merge was parked, main reset and
the done line said "NOT joined" (20:09:14); **theseus-5a50** (P3) filed. **The DM thread's call: re-gate**, with a standing
exception from then on (a gate whose only reds are 5a50's two tests, across an XX:00:00, is re-gated once), a timing guard
(gates start at minute :01 to :52), and `maintenance.auto false` in main's tree. **The re-run** (a second joiner): the lock
re-taken at 20:24:42, main fast-forwarded to the parked 118b8291, re-warmed; its gate (from 20:26:04; ok at 20:31:51):
**2,830 of 2,830** (1 slow, 21 skipped), no hour crossed, both runaway tests green; lifecycle in every budget (cold start
p50 27.7 / p95 29.6 ms; SIGKILL then restart 32.5 / 38.6; binary swap 52.6 / 59.1); turn plain 5 frames, 82.6 ms, tool
call 9 frames, 178.1 ms (under gaming mode). Pushed 20:32:01; the branch and the park branch deleted; done line 20:32:12;
theseus-cs71, ynia, 1n2y and qjd6 closed with the hash.

**The install.** Nothing: test code only.

**Divergences.** cs71 bounds the stop, not the round trip. qjd6's test accepts a trace posted twice: the exporter
delivers at least once (a timed-out post may have arrived, and OTLP does not deduplicate; it takes a receiver slower than
`export_timeout_secs`, 10 s by default), which R18 recommended accepting and saying in §3.20.

**Known gaps.** theseus-nh1k (P2), a cancel round-trip row in the gate's jobs bench, taken by batch 9's daemon-proofs
(Item 214). theseus-5a50 (P3): the runaway tests need a pinned clock (about 1 run in 1,800 per test
until then). The session once saw `tests_resumed`'s counted-once test read 1 for 2 under load (its `sleep 0.4` can
outlast `proc_sync_secs = 1` at nice 19); R18 did not reproduce it. The four left the gate's "known under load" list: a
red in any of them is now real.

### Item 190. Telemetry 3: each cancel counted in `theseus.cancel` where health counts it, the index tender's lag, documents, memory and restarts sampled after serving, telemetry-resumed's two test gaps closed, and a delegated daemon's L0 cancel counted as `l0` (theseus-kxyc, theseus-6xwq, theseus-qdk5 and theseus-gfi4, with theseus-7ydh by join fix 1; Item 180's test gaps and §3.20's two missing metrics; the eighth cloud batch's telemetry3 session, fired 2026-10-05 13:22 from 60b43fb6, Opus 5.5, its report at 15:21; d0b6e453, 62a43087, 86993e3c, b49fd1ff, b20e7e63 and 45f23e07; reviewed 18:06 to 18:50 by local reviewer R18, stack F, on timing-flakes' review merge, and accepted; the DM thread's call at 18:57, 7ydh's fix as join fix 1; joined 20:42 at acf26214, a signed merge onto 118b8291, by the stack-F re-run joiner; installed 2026-10-06 10:12 at 21bf5454, install #6)

**Why.** Two of R12's planted reverts at telemetry-resumed (Item 180) were not caught, test gaps with the code right:
**theseus-kxyc**, `finish`'s late results untraced (under the "moved" counting rule that is the only place such a job
counts, so a regression would drop it silently), and **theseus-6xwq**, `ran_batch`'s index shift dropped (the calls
after the one that asked would be named for the wrong calls). And two of §3.20's metrics were missing: **theseus-qdk5**,
cancels, which health counted but no metric did; **theseus-gfi4**, the index tender, whose lag, size and restarts health
showed but no series carried (no periodic sampler existed, and `TenderStatus.restarts` is cumulative). The batch-8
writer also flagged a probable bug: since theseus-a5nv, health labelled an L0 job stopped through its own cgroup on a
delegated daemon `l1`.

**What landed** (`theseus-core`'s `telemetry/metrics.rs` (`INSTRUMENTS` 34 to 40), `telemetry.rs`, `cancel.rs`,
`rpc/mod.rs`, a new `tender/sample.rs`, new tests `telemetry/tests_cancel.rs` and `tests_index.rs`, more in
`tests_resumed.rs`; theseusd's `main.rs`, one call; the merge 12 files, +904 −22 with join fix 1; no protocol type,
config key, store format or package).
- **kxyc** (d0b6e453, a test). `a_late_result_taken_as_its_turn_finishes_is_traced_there`: a `proc.run` of `sleep 1.3`
  with `proc_sync_secs = 1` answers `background`; a `Held` provider keeps the model's answer to that placeholder until a
  task, draining the spool with `core.heartbeat` every 50 ms as `wait_queued` does, sees the job's completion queued on
  the execution, so `finish`'s `take_late` always takes it and no sleep decides anything. The late span sits at the
  trace's top level (every trace has a `continuation` span, so "no continuation parent" is asserted as "its parent is
  the root"), `late: true`, `ok`, `run_ms` ≥ 1,300, one `theseus.tool.calls` point.
- **6xwq** (62a43087, a test). `a_continuations_fresh_calls_are_traced_each_for_its_own`: calls [A `proc.run` under
  `approve`, B and C `fs.read`], approved; each span named and `tool_use_id`'d for its own call, A under `continuation`,
  the two reads under one `tools` span after A; `proc.run` counts 1 and `fs.read` 2.
- **`theseus.cancel`** (86993e3c, qdk5). `CANCELS`, an IntSum with `theseus.cancel.backend` (`l0`, `l1`, `async`,
  `inproc`, `job` and the hands' backends) and `theseus.cancel.state` (`verified`, `uncertain`, `unsupported`), exactly
  as health names them. A count outside a turn already had a path (`record_push`, `record_judgment`), so no new export
  path: `Stops` holds a `OnceLock<Telemetry>` (`export_to`, set where the judge's is: in `Core::build` with a pipeline in
  the parts, and in `build_telemetry` after serving), and `Stops::count`, health's own counter, records the metric in
  the same call, so health and the metric agree by construction and every stop path (cancel, task cancel, `/stop`, the
  disk's floor, a stop at launch) is covered without finding each one.
- **The tender's gauges** (b49fd1ff and b20e7e63, gfi4). `theseus.index.lag_bytes` (By), `lag_ms` (ms), `documents`
  and `rss_bytes` (By), gauges of the tender's answer, and `theseus.index.restarts`, a counter fed the rise of
  `TenderStatus.restarts` between samples (the first sample creates it at 0; the supervisor gains no hook).
  `Core::sample_index_after_serving` (tender/sample.rs), started from theseusd's `after_serving` beside the tender's own
  start, only with `[telemetry] otlp_endpoint` set and `[index]` on, holds the core by `Weak` and each
  `metrics_interval_secs` records `health_block()` (`Telemetry::record_index`); it asks nothing while telemetry is off. A
  task, not the exporter's tick, so no socket call (up to 100 ms) sits inside an export; a gauge may be one interval
  older than its post. While the tender is down the four gauges keep its last answer, and `restarts` and health tell the
  outage. b20e7e63 made the tests' positive checks hold under load (a sample's one ask under 100 ms gave up before the
  stand-in, on the same niced runtime, answered).
- theseus-core's AGENTS.md names both metrics (45f23e07).
- **Join fix 1** (theseus-7ydh, the first stack-F joiner's `joinfix-7ydh.py`): in `cancel.rs`'s `job_backend`,
  `VerifiedBy::Pidns => "l1"` and `Cgroup | Tree | Group => "l0"`, with its doc saying why (since theseus-gyin no L1
  wrapper writes `Cgroup`, so a stop by its cgroup is an L0 job's on a delegated daemon, and `job_backend` reads only the
  verdicts of the stop just run); the module doc's list of a job's verdicts names L0's cgroup; a new test module,
  `a_cgroup_verdict_counts_as_l0_and_a_pid_namespace_one_as_l1` (`cancel.rs` had none, and no `job_backend` test
  existed).

**How it is proven.**
- **The session's tests:** 71 of telemetry, the tender and the core's cancels, unloaded; under load 70 of 71 three
  times, the miss each time qjd6's known retry (fixed by timing-flakes, Item 189). Its plants each failed
  alone: `take_late` pushing no span (54 of 55 pass); `index: r.index` and `group: r.group` (170 of 171); the cancel's
  feed removed; every cancel fed as `verified`; restarts fed the total (3 for 1); `documents` read from `nodes` (2 for
  3). Live on a scratch daemon of its build: a cancel counted `{l0, verified} 1` with health agreeing, the gauges matching
  `index status`, and `restarts` 1 after a SIGKILL. The session reported the `l1` label without changing it.
- **The review** (R18, review commit 3f447235 on timing-flakes' 4bcca607 over c4f79e9f). A clean merge (AGENTS.md and
  rpc/mod.rs auto-merged); `INSTRUMENTS` declared 40, with 40 entries and 40 consts; the golden as merged, clippy,
  protocol 31 and shape clean; **the whole workspace suite on the stack, 2,828 of 2,828** (`--retries 0`); telemetry3's
  71 of 71 with `unadmitted` nowhere in any test's output (route-gaps' and situations' meeting points); under the load
  recipe 3 of 3, and qjd6's test 0 of 20 on the stack. **4 of 6 plants caught**, R12's two uncaught ones (kxyc, 6xwq)
  among them, only by the new tests. Not caught: **theseus-qqhd** (P2), the daemon's own path (`install_telemetry` →
  `build_telemetry`) no longer handing the stops their telemetry passed 88 tests, since every test core gets its pipeline
  through the parts (the same holds for the judge's and memory's `export_to` beside it); **theseus-fk0g** (P2), a gauge
  frozen at its first answer passed 70, since the stand-in tender always answers the same numbers.
- **Live** (R18; scratch daemons of the stack's build as transient user units, the stand-in model, a loopback OTLP
  sink, `[policy] enforcement = "approve"`, `proc_sync_secs = 1`, `metrics_interval_secs = 2`, `[index]` on). The
  cancel: "⏹️ cancelled proc.run … (verified: process tree, 1 process)" in 0.17 s, health "cancels since the start: l0 1
  verified", the next post `theseus.cancel {l0, verified} 1`. The gauges: a post 14 s after the start held documents 5,
  lag 0 and 0, `restarts` 0 and `rss_bytes` 580,517,888, health's own number. After SIGKILL to the tender: running again
  within 2 s with a new pid, and the next post `theseus.index.restarts` 1. **The `l1` label, live:** on a second unit
  with `Delegate=yes` the same L0 cancel was "verified: cgroup, 1 process", and health and the metric both said `l1`
  (theseus-7ydh, P2): Eddie's `theseusd.service` is delegated, so his health already read `l1` for every L0 cancel.
- **FAST** (the whole stack against c4f79e9f, frozen debug builds, one hold, palindrome order): turn frames 5 and 9 on
  both arms, plain p50 80.6 against 80.8 ms, tool call 172.7 against 170.1; lifecycle with no budget missed, cold start
  25.1 against 22.6 ms. Nothing on the start or turn path: the sampler starts after serving and only with an endpoint and
  `[index]` on, a sample asks at most once under health's 100 ms deadline, and the cancel's metric is one add after
  `Stops::count`'s lock is dropped.

**The join** (stack F's second). The first stack-F joiner did not take it: its gate for timing-flakes had gone red
across 20:00 (Item 189), and telemetry3 was reviewed only on that stack; it wrote join fix 1 and tried it on
the dry run's `cancel.rs` (applied, idempotent, rustfmt clean), unbuilt. **The re-run joiner**: the lock
`cloud-telemetry3-join` and the merge in one guarded call at 20:32:52 on 118b8291: clean (AGENTS.md and rpc/mod.rs
auto-merged); join fix 1 applied ("job_backend: Cgroup counts as l0: applied; the module doc names L0's cgroup:
applied; the test: applied"); 12 files, +904 −22, the review's +872 −17 and the fix's +32 −5; the staged tree equal to
the dry run's but for `cancel.rs`, which equalled the fix as tried. The warm (20:33:04 to 20:35:16) compiled the fix's
test and checked `[&Instrument; 40]`; 74 of 74 (R18's 71, the fix's test, the golden's two), telemetry3's own label test
still `{l0, verified}` (it stops a `group` verdict). The signed merge **acf26214** (118b8291 and c4a1f09c), 20:36:04. Its
gate (from 20:36:08, minute 36; ok at 20:41:57): **2,840 of 2,840** (1 slow, 21 skipped: 2,830, telemetry3's 9, the
fix's 1); lifecycle in every budget (cold start p50 25.8 / p95 31.3 ms; from the config copy 25.2 / 26.9; clean shutdown
36.7 / 55.0; SIGKILL then restart 30.1 / 31.2; binary swap 50.5 / 56.3); L1 start 7.16 ms; turn plain 5 frames, p50 78.0
ms, tool call 9 frames, 161.8 ms, at or under both earlier gates. Pushed 20:42:05; the branch deleted; done line
20:42:13; theseus-kxyc, 6xwq, qdk5, gfi4 and 7ydh closed with the hash; nh1k, qqhd, fk0g and 5a50 open. The
`telemetry-tests` row of batch 9, held for this join, became launchable. The store stays at format 22.

**The install** (2026-10-06 10:12 at 21bf5454, install #6). **Health's "cancels since the start" now reads `l0` for an
L0 job stopped by its delegated cgroup** (join fix 1); his unit has `Delegate=yes`, so until this install it read `l1`
for every L0 cancel; an L1 job's cancel (pid namespace) still reads `l1`. The new metrics post only with an OTLP endpoint,
the tender's only with `[index]` on too. No config key or format change. Health after the restart (10:12:34): check exit 0,
9 secrets ready 1.05 s after the start, startup serving at 45.3 ms (store 4.6, kernel 36.9 ms, at a load of 11 to 19),
Discord ready, the judge's live packs as before, memory live on the `baseline` arm, voice ready, `cgroup: delegated`, the
unit active with NRestarts 0, and no error or warning in the journal; the store from format 22 to 23 at the first write
(soul-import's), after the backup.

**Divergences.** The cancel metric is fed by health's own counter, not at each stop path. The tender is sampled by a
task after serving, not in the exporter's tick. Gauges keep the tender's last answer while it is down. Like main's other
gauges (`theseus.tasks.open`, `theseus.node_cache.bytes`), the new ones go out as a non-monotonic cumulative OTLP `sum`,
which backends read as a gauge. 7ydh's test is a unit test in `cancel.rs`, not one in theseusd's `tests/cgroup.rs`, where
R18 had suggested it; a live test on a delegated scope is qqhd's.

**Known gaps.** theseus-qqhd and theseus-fk0g (P2), the two test gaps, taken by batch 9's telemetry-tests
(Item 211). R18's recommendation, not built: let the four gauges drop out while the tender is down (about
five lines in `Metrics::index` and a test), since a tender in backoff now shows a flat, healthy-looking lag. AGENTS.md's
cancellation line runs to 190 characters where the file wraps at about 120.

### Item 191. CLI tests: health's split cache writes and `judge prove`'s three outputs held by goldens, and `theseus watch` ends a reply's open line when the daemon dies (theseus-xiaz, theseus-w38g and theseus-1n2l; the eighth cloud batch's cli-tests session, fired 2026-10-05 13:22 from 60b43fb6, Sonnet 5.5; 4cba6ded, 2ebdb758 and b450ac67; reviewed 21:43 to 22:08 by local reviewer R19, stack T, and accepted with the stack at 22:52; joined 23:08 at 57fdfd96, a signed merge onto acf26214, by the stack-T joiner; installed 2026-10-06 10:12 at 21bf5454, install #6)

**Why.** Two test gaps R13's planted reverts found in batch 7's stack H, and a bug from step 10a's goldens. Health's
`cache-write N (1h M)` (Item 177) and `theseus judge prove`'s stdout being the generator's Markdown byte for byte (Item
179) were right, but every test passed with the wiring reverted (theseus-xiaz, theseus-w38g, P2). And `theseus watch`
returned at EOF without `printer.settle()`, so when the daemon closed the connection mid-reply the shell's prompt
landed on the reply's last line (theseus-1n2l, P3).

**What landed** (`crates/theseus` only; 7 files, +97 −3; no package, store format, protocol type or config key).
health_push's fixture gains 300 one-hour cache writes, so its tokens line reads `cache-write 1200 (1h 300)` while the
other health goldens keep the zero case (4cba6ded). A canned `JudgeProveResult` and three tests in a block before
`health_prints_every_line` (so the merge with history-pages, Item 193, stays keep-both): the Markdown, and
`--records -`, each against a golden and asserted equal to the result's bytes, and `--records FILE` holding exactly
`records`; `stdout_of` cuts between `--- stdout` and the last `--- stderr`, so no golden rewrite can bless an extra
newline (2ebdb758). One `printer.settle()` after `cmd::watch`'s loop, idempotent, so only watch_lost and watch_shapes
gain their final newline; `--all`, `--json` and `--interactive` already ended their lines (b450ac67).

**How it is proven.** The session: the CLI's 157 tests, its plants (health's bare `cache_creation_input_tokens`;
`println!` for the Markdown and for `--records -`; the `settle()` removed) each failing. **The review** (R19, on main
acf26214): the build, clippy `-D warnings`, protocol and shape clean; **157 of 157** with the goldens compared, not
rewritten; **5 of 5 planted reverts caught**, R19's own (`--records FILE` with one newline more) among them; before this
branch the first three passed every test. **Live** (scratch daemons, R19's stand-in Messages API): health read
`cache-write 1500 (1h 300)`; `theseus judge prove`, `--records r.jsonl` and `theseus-judge prove r.jsonl --markdown -`
gave the same 2,054 bytes; and with the daemon SIGKILLed mid-reply, **this branch's `theseus watch` ended `Halfway\n`,
main's `Halfway`**. FAST: the CLI only; stack T's A/B found no cost.

**The join** (stack T's first). Lock 22:55:11; a clean merge onto acf26214, the staged tree equal to R19's dry run;
**no join fix**; the CLI's 157 of 157. The signed merge **57fdfd96**, 22:56:53. Its gate waited for minute :01 (the
timing guard, theseus-5a50) and ran 23:01:01 to 23:08:25, 94 s of it waiting for the shared lock: **2,843 of 2,843** (1
slow, 21 skipped); lifecycle in every budget (cold start p50 23.0 / p95 23.6 ms; SIGKILL then restart 26.9 / 34.1);
turn frames 5 and 9 (plain p50 73.5 ms, tool call 158.7). Pushed 23:08:43, the branch deleted, done line 23:08:51;
theseus-xiaz, w38g and 1n2l closed. The store stays at format 22.

**The install** (2026-10-06, restart 10:12:34 at 21bf5454, install #6). The CLI only: `theseus watch` ends a reply's open
line when the daemon dies. Health after the restart: `theseusd check` exit 0, 9 secrets ready 1.05 s after the start,
startup serving at 45.3 ms (load 11 to 19, not a quiet reading), Discord and voice ready, the judge's live packs as
before, memory live on the `baseline` arm, `cgroup: delegated`, the unit active with NRestarts 0, no error or warning in
the journal; the store from format 22 to 23 at its first write, after the install's backup.

**Divergences.** None, but the prove tests' place before `health_prints_every_line`.

**Known gaps.** R19, each recommended: a read error from `conn.next()` (rather than EOF) still returns before the
`settle()`, a cosmetic error path left for the next touch of `cmd::watch`; the live prove check covered the empty case
only, the goldens holding the non-empty bytes.

### Item 192. The turn's stack: the turn's future boxed at `TurnRunner::run` and at the loop's largest calls, so the golden conversation needs 576 KiB where it needed 1,856, and a test holds the margin on a fixed 1.5 MiB thread (theseus-b4sf; the eighth cloud batch's turn-stack session, fired 2026-10-05 13:22 from 60b43fb6, Opus 5.5; 76524812 and f9bbc749; reviewed 20:51 to 21:21 by local reviewer R20, stack R, and accepted with the stack at 22:57; joined 23:38 at 591259fe, a signed merge onto 57fdfd96, by the stack-R joiner; installed 2026-10-06 10:12 at 21bf5454, install #6)

**Why.** theseus-b4sf (P2), found by local reviewer R7 at situations' review (Item 183): the review
merge of situations on main made theseus-core's `tests_output::the_cores_output_matches_its_golden` (whole cores on
`#[tokio::test]`'s 2 MiB test thread) abort with "has overflowed its stack", though each side alone passed. Nothing on
the turn path was boxed, so a debug build's turn future needed about 2 MiB of stack, and every step that added a local
across an await in the turn loop could tip it. A debug daemon's tokio workers have 2 MiB stacks too (theseusd sets no
`thread_stack_size`), so scratch daemons, the exam's daemons and the gate's daemon tests sat on the same edge; release
builds were far from it.

**What the session measured first** (on the tree as cloned; `RUST_MIN_STACK` bisected over one test binary at a 64 KiB
grain, three runs at each boundary, all agreeing). The golden overflowed at 1,856 KiB and passed at 1,920;
`tests_m3::a_tool_loop_reads_a_file_and_the_prefix_never_changes` at 832 and 896. Future sizes (throwaway
`size_of_val` prints): `TurnRunner::run` 163,752 bytes, `run_inner` 79,664, `turn_body` 36,984, `catch_up` 17,096,
`run_tools` 16,008, the compile step 11,008, the compaction 5,104, the call step 1,200; the golden's helper `turn`
327,576 (two of `run`'s). Stack-pointer probes at each function's first poll showed where the depth sat: at opt-level 0
**a caller's poll frame keeps a slot the size of every future it builds, and frames share no slots**, so the depth was
poll frames, not futures held inline: `conversation`'s poll frame about 1.1 MiB (a dozen 320 KiB `turn()` futures, each
in its own slot), `run`'s 103 KiB, `turn_body`'s 121, and toolrun's `run_inproc` 170 KiB, the largest frame inside the
turn.

**What landed** (`theseus-core`'s `turn.rs`, `tests_stack.rs`, one `pub(super)` in `tests_output.rs` and a `mod` line;
the merge 4 files, +90 −15; no package, store format change, protocol type or config key).
- **The boxes** (76524812). `TurnRunner::run` is a plain fn returning `Pin<Box<dyn Future<Output =
  Result<TurnSubmitResult>> + Send + '_>>`, its body `Box::pin(self.run_body(req))`; the old body is `run_body` with
  only its signature line changed (so queue-frames' edit at the body's end, Item 194, still merges), and
  every caller's `.run(req).await` is unchanged. A helper, `boxed(|| self.f(..))`, builds a future in its own short
  frame and moves it to the heap, so the caller's poll frame keeps only the closure and a pointer (`Box::pin(f).await`
  in place would keep `f`'s slot). It boxes `run_inner` (now a fn over `run_inner_body`), `turn_body`, `catch_up` and
  `run_tools`. Not boxed: `compile_routed` (route's file, left alone as the brief said) and `call_model` (1.2 KiB). The
  session's bisection after each box: the golden 1,920 → 768 (the entry) → 704 (`run_inner`) → 640 (`turn_body`) → 640
  (`run_tools`) → **576 KiB** (`catch_up`); the tool loop 896 → 640 → 576 → 576 → 512 → **448**. Probes then put
  `conversation`'s poll frame at 88 KiB and the chain from `run` to `run_inproc` at about 410 KiB. The heap cost: about
  46 KiB a turn in the session's sizes (run_body 7,856 B, run_inner 5,704, turn_body 16,136, catch_up 17,096) plus
  16,008 for each loop that runs tools, where 160 KiB sat inline before.
- **The margin's test** (f9bbc749). `tests_stack::the_golden_conversation_runs_on_a_one_and_a_half_mib_stack` runs
  the golden's `conversation` on a `std::thread::Builder` thread with a fixed 1.5 MiB stack and a current-thread runtime
  (`rt.block_on(Box::pin(conversation(&mut out)))`), so no environment variable changes it. Its own boundary is 576 KiB,
  leaving about 960 KiB.

**How it is proven.**
- **The session's tests.** The margin test alone, three runs, 3.5 s each; its boundary (a throwaway knob) overflowing
  at 320 to 512 KiB and passing at 576. Plants: a 512 KiB `black_box` array in `turn_body`'s poll frame, held across no
  await: the test aborts, the golden at its 2 MiB passes; the entry's box alone removed: **the test passes** (with the
  loop's boxes the turn's future is about 8 KiB; only the bisection shows it, golden 576 → 896 KiB). Under the load
  recipe (nice 19 beside four busy loops) 5 of 5 runs failed, none by overflow: each in the golden conversation's own
  "no wake due in 30 s", which the golden fails the same way under that load on the tree as cloned. `theseus-sim bench
  turn --runs 20`, three interleaved pairs: plain p50 73.2, 66.4, 62.7 ms before and 66.8, 64.0, 62.5 after; tool call
  163.4, 161.3, 149.0 and 161.8, 160.4, 146.5; frames 5 and 9 in every run. Its gate: every test but the 33 known L1
  tests the cloud VM cannot run as root.
- **The review** (R20, on main acf26214, store format 22; review commit 2844936f, unsigned): the build, **the golden as
  merged** (no line moved), clippy `-D warnings`, protocol 31 of 31 and shape clean; **theseus-core's whole suite 1,290
  of 1,290** (`--retries 0`; tests_stack 5.1 s, the golden 6.0 s). **The stack beside situations**, the session's step
  1 redone on the merged tree: the golden passes at **576 KiB** (main that day: 1,856, overflowing at 1,792, 192 KiB
  under a test thread's 2 MiB), the tool loop at **448** (main: 832), tests_stack's own thread at 576: exactly the
  report's numbers. **No future grew**: run_body 7,680 B, run_inner_body 5,536, catch_up 17,096, run_tools 16,008; and
  turn_body fell from 16,136 to **4,848** because `compile_routed`'s future, which it holds across its await, fell from
  11,008 to 2,008 B on that main (route-gaps' and situations' reshaping of the compile step). The boxes cost about
  34 KiB of heap a turn, plus 16 KiB for each loop that runs tools. **2 of 3 planted reverts caught** (every box
  removed: tests_stack aborts with SIGABRT in 0.16 s while the golden at 2 MiB passes; a 512 KiB poll-frame local:
  SIGABRT); not caught, as the report itself found: the entry's box alone removed (the conversation then needs 896 KiB,
  inside 1.5 MiB, while the test's doc says unboxing a turn overflows there; filed theseus-2kyc, P2).
- **Live, the daemon's dispatch frames** (R20; the report's open item, now measured): a debug scratch daemon (fresh
  state dir, a transient user unit, Discord, the web, memory and the index off, the stand-in model) with
  `RUST_MIN_STACK` sizing its tokio workers, answering a plain turn and a turn with one in-process `fs.read`, on main's
  build and the whole stack's. At 1,536 and 1,024 KiB both arms answered both turns; **at 768 KiB main's daemon
  overflowed on the plain turn and died, while the stack's answered both**; at 512 KiB the stack's answered the plain
  turn and overflowed on the `fs.read` turn. So a debug daemon's 2 MiB workers keep at least 1.25 MiB spare on the
  stack; main already had about 1 MiB, so the golden's conversation, building a dozen turns in one frame, was the one
  near the limit, never the daemon.
- **FAST** (R20, the whole stack R against acf26214, one hold, palindrome order, PSI beside each run): frames 5 and 9 on
  both arms in every run; plain A 80.6 / B 81.5 ms (+0.8), tool call A 162.5 / B 160.4 (−2.1); the lifecycle's medians
  within ±10 ms with mixed signs. **No cost**: two heap allocations at the turn's entry and one per boxed call.

**The join** (stack R's first; the stack-R joiner, 2026-10-05 22:57 to 2026-10-06 00:45). R20's dry run on acf26214
(22:59) and on cli-tests' 57fdfd96 (23:03): turn-stack conflicts in `lib.rs` alone; the final tree differs from R20's
review tree only by cli-tests' 7 files. Lock `cloud-turn-stack-join` 23:02:24, queued behind cli-tests' (history-pages'
lock came 73 s later, so it waited behind this one); clear at 23:10:33 after 8 min 9 s. The merge onto 57fdfd96: one
conflict, theseus-core's `lib.rs` (situations' `mod tests_situation;` beside `mod tests_stack;`), where **rerere
replayed R20's recorded resolution** (the review tree shares the repository's rr-cache) and `turn-stack/resolve.py`
read "both mod lines in already"; turn.rs auto-merged; 4 files, +90 −15; **the staged tree 209bc828 equal to the dry
run's**; turn.rs 3,491 of its 3,523 ceiling; the core's 79 test `mod` lines sorted. **No join fix.** The warm took 12 min
9 s for the test build at load 20 to 31 (three reviewers' trees building); then theseus-core's whole suite and the
kernel's frames golden, no `RUST_MIN_STACK`, **1,291 of 1,291** (the golden 6.5 s, tests_stack 6.1 s). The signed merge
**591259fe** (57fdfd96 and 81b94f95), 23:27:32. Its gate (23:27:37 to 23:37:48, ok; 201 s waiting for the shared lock
behind two reviewers' steps): **2,844 of 2,844** (1 slow, 21 skipped; cli-tests' 2,843 plus tests_stack), no hour
crossed; lifecycle in every budget (cold start p50 21.5 / p95 23.8 ms; from the config copy 22.0 / 23.6; clean shutdown
31.5 / 50.7; a post in flight 74.6 / 79.2; SIGKILL then restart 25.6 / 39.1; binary swap 47.8 / 64.8; restore 144.4 /
162.1); L1 start 5.92 / 6.26 ms; turn frames 5 and 9 (plain p50 75.4 ms, tool call 155.6). Pushed 23:38:11, the branch
deleted, done line 23:38:19; theseus-b4sf closed with the hash. The store stays at format 22.

**The install** (2026-10-06, restart 10:12:34 at 21bf5454, install #6). Margin, for debug builds above all: the turn's
futures are boxed, about 34 KiB of heap a turn, no cost measured; a release build was never near its limit. No config
key, store format, protocol type or package. Health after the restart: `theseusd check` exit 0, 9 secrets ready 1.05 s
after the start, startup serving at 45.3 ms (store 4.6, kernel 36.9 ms; load 11 to 19, not a quiet reading), Discord
ready, the judge's live packs `security.v3`, `route.v1` and `rerank.v1` as before, memory live on the `baseline` arm,
voice ready, `cgroup: delegated`, the unit active with NRestarts 0, and no error or warning in the journal; the store
from format 22 to 23 at its first write, after the install's backup.

**Divergences.** None from the brief, which allowed the loop's largest futures to be boxed if the depth sat inside
the turn (it did: poll frames, not futures held inline). The issue's "nothing on the turn path is boxed" no longer held
when the session ran (`recall_live` and the golden's two halves already were). The margin test inherits the golden
conversation's 30 s wake wait.

**Known gaps.** theseus-2kyc (P2): tests_stack misses the entry's box alone; R20's fix is a size test (`run`'s result a
fat pointer, 16 bytes, against about 8 KiB unboxed) and a corrected doc. R20's "For Eddie", each recommended: join as
is; leave `run_inproc`'s 170 KiB poll frame (toolrun.rs at 2,494 lines, best done with C6's reshaping) and
`compile_routed` (now 2 KB) unboxed; leave the margin test's 30 s wake wait (the golden's timing under starvation, on
main too); if a later step brings a debug daemon near its 2 MiB, `thread_stack_size(8 << 20)` under
`cfg!(debug_assertions)` is the cheap durable fix.

### Item 193. History pages: `session.history` pages both ways by optional cursors as `ledger.tail` does, a node is reached by its short id, and `theseus history` prints it (theseus-xo0m, theseus-glyw and theseus-kym3's protocol half; v1.1's step V1; the eighth cloud batch's history-pages session, fired 2026-10-05 13:22 from 60b43fb6, Opus 5.5, done 14:40; bbd0cf12, 8c9a5411 and 439ef00b; reviewed 22:09 to 22:48 by local reviewer R19, stack T, and accepted with the stack at 22:52; joined 23:59 at 8ea6669e, a signed merge onto 591259fe, by the stack-T joiner; installed 2026-10-06 10:12 at 21bf5454, install #6)

**Why.** roadmap-v1.1's V1: history and the ledger page both ways, and short node ids. `session.history` had no
cursor: with `n` it read the newest `n` through `nodes_paged`, without it the whole session, and it read the whole
session whenever a question waited in it, for any `n` (theseus-xo0m; the TUI's older pages, theseus-kym3, need its
`before`). `node.reach` took only a whole id, and `theseus history` printed none, while the cockpit already showed
nodes as `msg·e0f1a2` (theseus-glyw). The brief's other half, cursors on `ledger.tail`, was already on main
(`after`/`before` and `next`/`older`), so the session gave `session.history` the same shape.

**What landed** (theseus-protocol's new `history.rs`, theseus-core's `rpc/pages.rs`, `reach.rs` and `rpc/methods.rs`,
theseus-store's `store.rs` and `index.rs`, the CLI's `render/history.rs`, `cmd.rs` and `main.rs`, three regenerated
protocol.gen files; the merge 27 files, +1,132 −82; no package, store format change or config key; one protocol
change, additive).
- **`session.history` pages both ways** (bbd0cf12; theseus-xo0m and kym3's protocol half). Params gain `after` and
  `before`, the result `next` and `older`, all optional and absent unless set, so old bytes hold.
  `Core::history_page` reads a page through the session's tag `s:<session>` with the store's `Page` (which already
  took `after` and `before`, so the store needed no change for it): `after` gives the first `n` past it, oldest first,
  with `next` (the page's last) while more may follow; `before` alone gives the newest `n` before it, with `older`
  (the page's first) while older nodes remain; with both, the page reads between them from `after`, and only `next` is
  set. A cursor without `n` pages 200. Without a cursor the answer is main's: the newest `n`, or the whole session
  without `n`. While the index's shape is being built, the page falls back to `bounded`, the same bounds over the whole
  read. **A question waiting in the session no longer forces a whole read:** for a paged read, `card_nodes` reads the
  session's `tool_call` nodes back from the newest, 32 at a time, until each waiting call's node is found (a budget's,
  an extension's and a promotion's questions need none); only the whole read an old client asks for still reads the
  session whole. The protocol's two types moved to `history.rs`, re-exported, so the protocol's `lib.rs` fell from
  2,727 to 2,712 lines. The CLI gains `theseus history --after P` / `--before P` and a last line naming the next
  page's command (`── older nodes before position P: theseus history <S> --before P`). The session found one
  difference on the ledger side and left it: `ledger.tail` sets `older` from `out.more` whenever `before` is given,
  even with `after` too, when `more` then means newer rows remain.
- **A node's short id** (8c9a5411 and 439ef00b; theseus-glyw). `node.reach` tries the whole id first, exactly as
  before, and only then `reach::resolve`: the last 6 or more characters (`e0f1a2`, `…e0f1a2`), or the cockpit's
  `msg·e0f1a2`, whose prefix must match too (`theseus_protocol::short_id` writes the cockpit's form, shared by the
  daemon's refusal and the CLI). One node is answered as its whole id. Several are refused with `INVALID_PARAMS`,
  naming each with its session (up to 8, then "and N more"; the walk stops at 64 and says "at least 64"). None, or an
  end under 6 characters, is `NOT_FOUND`, in words ("no node's id ends with `abcdef`"; "no node is named `1a46b`: give
  a node's whole id, or at least its id's last 6 characters"; with a prefix, "no node's id starts `tcl_` and ends with
  …"), so `tests_reach`'s contract (an unknown `msg_x` is NOT_FOUND) holds unchanged. theseus-store gains one read,
  `Store::keys_ending(kind, ending, limit)`: the WAL store walks its index's key table for the kind
  (`RedbIndex::keys_ending`; `bykey` is part of the base index, whole when the daemon serves) and reads no record; the
  trait's default reads every record. `theseus history` prints each node's short id after its first line's mark,
  through `render::history::node_lines`; `render::node_lines` is unchanged, so theseus-tui's pane shows no ids.

**How it is proven.**
- **The session's tests:** `rpc/tests_history.rs` (6: a walk forward by `next` over 200 nodes interleaved with another
  session's, 5 written between each of the first 3 pages, reads 215 nodes each once in order; a walk back by `older`
  over 300 reads each node it had once and none of the new ones; without a cursor the answer and JSON are main's, and a
  cursor without `n` pages 200; `bounded` equals the index page for `n` in {1, 7, 50, 1000} at 27 cut points; a page
  decodes 20 nodes in a session of 100 and of 10,000 and never reads the transcript whole, counted by
  `Store::transcript_reads`; a pending confirm's card is on every page, byte for byte the whole read's);
  `rpc/tests_node_names.rs` (3, and an `#[ignore]`d timing over 100,000 nodes: median 35.5 ms in the cloud's debug
  build); the store's `keys_ending_walks_a_kinds_keys`; goldens history.txt and history_full.txt (the diff is the short
  ids alone, read line by line), and the new history_pages.txt, reach_short.txt and reach_ambiguous.txt. Its plants
  (`after` ignored, `before` ignored, a page read as the whole session, the card read from the page alone, an ending
  matched to the first of two) each failed. Under the load recipe, 31 tests, 5 runs: 31 passed each time.
- **The review** (R19, stacked on cli-tests' review commit c1cc766d over main acf26214, store format 22; review commit
  b085aa8a): the build, clippy `-D warnings`, protocol 31 of 31 (protocol.gen as committed), the cockpit (`npm ci
  --offline`, lint, 86 of 86 tests, build) and shape clean; **the whole workspace suite on the stack, 2,856 of 2,856**
  (`--retries 0`, 22:21 to 22:26). An old client: a call with no cursor and no `n` reads the whole session, `n` alone
  the newest `n`, neither result carrying a cursor; every caller on main sends JSON (theseusd's MCP `last` with `n:
  40`, Discord voice, the TUI) and gets main's answer, the MCP call with a question waiting now reading the cards'
  nodes instead of the whole session. **5 of 6 planted reverts caught**: a `before` page read forward; `next` one short
  (a node repeated); `older` one short (a node lost); an end naming two answered as its first; and R19's own, an old
  client's call answered with a page of 200 (caught only by the card test, whose waiting call falls outside the newest
  200). Not caught: the WAL store's `keys_ending` replaced by the trait's read of every record (the answers are the
  same, only every node record is read; filed theseus-gk93, P3).
- **Live** (R19; scratch daemons of the stack's debug build as transient user units, main's CLI as a control, the
  stand-in model). 150 turns made 300 nodes. The walk back by `older` at `-n 20`: 15 pages, 300 nodes each once in
  order, and the 6 nodes written during it on no page; the walk forward by `next` from 0: 16 pages, 312 nodes each once
  in order, the 6 new ones included; each walk's last page carries no cursor. The page lines name the next command.
  An old client's two calls answered as main's, from both CLIs. `after` at the newest: `nodes: []`, no `next`, in 4.0
  to 8.2 ms with the CLI (median 4.8). `reach` by the whole id, its last 6, `msg·…`, `…` and its last 9 printed the
  same bytes; an unknown end, a 5-character end and a wrong prefix were refused in words, exit 1, code −32002. **An
  end that names two, live** (the cloud session could not make one: two of 300 random ids sharing 6 hex characters is
  about 1 in 6,000): with the daemon down, a probe (an uncommitted test, its source removed after) wrote a second
  session holding a message whose id shared the last 6 characters; after a restart, `reach` by those 6 and by
  `msg·…` were refused, code −32602, naming both nodes with their sessions, and 8 characters of each named one. The
  resolve over 100,000 nodes took medians of 30.8, 29.3 and 29.3 ms on this machine's debug build.
- **FAST** (R19, the whole stack T, cli-tests with history-pages, against acf26214; debug builds frozen to /tmp; two
  holds, 22:17 and 22:44, palindrome order, PSI beside each run). The turn: 5 and 9 frames on both arms in both holds;
  median p50 plain A 110.5 / B 116.4 ms and A 81.5 / B 80.9, tool call A 277.3 / B 237.3 and A 184.5 / B 165.4; the
  signs change between holds: no cost. The lifecycle missed cold and vault budgets on both arms in both holds, under
  the neighbours' load (11 to 17, then IO pressure up to 24 %); in hold 2, B's slow run was slower in every phase of the
  daemon's own clock alike, config parse and socket bind included, phases no code here touches. Nothing in the stack
  runs on the start path or in a turn: **no FAST finding**.

**The join** (stack T's second; the stack-T joiner). Lock `cloud-history-pages-join` taken 23:03:37, 73 s after the
stack-R joiner took turn-stack's, so by the rule "wait only on locks taken before yours" it waited about 35 min for
turn-stack (Item 192); the two share no file. R19's dry-run script could not run on the moved main (it merged
`origin/cloud/20261005-cli-tests` first, which the first join had deleted, and then printed "the same tree … yes" from
an empty argument), so the joiner wrote `dryrun-hp.sh`: history-pages alone onto 591259fe (23:39:12), clean, resolve.py
"nothing to do on this base" (soul-import not yet on main), `MANIFEST_FORMAT` 22, and **the change it brings the same
`patch-id --stable` as R19's reviewed patch** (both 26 files, +1,131 −81). The merge at 23:39:17: clean (methods.rs,
rpc/mod.rs, tests_m3.rs, store.rs, cmd.rs and golden.rs auto-merged); `ceiling.py` lowered the protocol `lib.rs` entry
in `scripts/long-files.txt` from 2,727 to **2,712**, with its clause. The take stopped at `git diff --cached --check`
on 7 trailing spaces, six in ts-rs's generated protocol.gen files (272 of main's 346 end lines with a space, and the
protocol test compares them byte for byte) and one in history.txt on a line that already ended with one; the joiner
ran the take's remaining checks (`take-rest.sh`, which accepts trailing spaces only there): all held, 27 files,
+1,132 −82, **the staged tree 7f823e09 differing from the dry run's only in `scripts/long-files.txt`**. No join fix.
The warm (23:40:26 to 23:48:52: the test build 5 min 26 s at load 15 to 24, clippy clean), rustfmt, then the CLI's
goldens, theseus-protocol's tests and R19's selection of theseus-core's and theseus-store's, **237 of 237**. The signed
merge **8ea6669e** (591259fe and ba8605a5), 23:50:19. Its gate started at minute :50 (23:50:27) and waited 134 s for
the shared lock behind three reviewers' steps; the joiner had a script ready to stop its own gate while it still waited
if the lock was not taken by 23:54:30 (a suite across 00:00 crosses an hour and a day line, theseus-5a50), and the
lock came at about 23:53:46. Gate exit 0 at 23:58:56: **2,857 of 2,857** (1 slow, 22 skipped, the 22nd the branch's
`#[ignore]`d timing; turn-stack's 2,844 plus 13), the suite ending about 23:58:21, before the hour; lifecycle in every
budget (cold start p50 21.7 / p95 31.3 ms; from the config copy 22.0 / 22.7; clean shutdown 33.1 / 77.7; SIGKILL then
restart 26.3 / 32.8; binary swap 48.2 / 50.3; restore 133.7 / 135.0); L1 start 5.75 / 6.46 ms; turn frames 5 and 9
(plain p50 76.9 ms, tool call 153.6). Pushed 23:59:12, the branch deleted, done line 23:59:17; theseus-xo0m and glyw
closed with the hash; theseus-kym3 left open for its TUI half. Gate against gate across the stack (acf26214,
57fdfd96, 591259fe, 8ea6669e): no cost. The store stays at format 22.

**The install** (2026-10-06, restart 10:12:34 at 21bf5454, install #6). `session.history` takes optional cursors; old
clients (the TUI's pane, theseusd's MCP `last`, Discord voice) are unchanged. `theseus history` pages with
`--after`/`--before` and prints short ids; `theseus reach` takes a short id. A paged read with a question waiting reads
only the waiting calls' nodes. The cockpit's protocol.gen carries the new optional fields. No config key, store format
or package. Health after the restart: `theseusd check` exit 0, 9 secrets ready 1.05 s after the start, startup serving
at 45.3 ms (store 4.6, kernel 36.9 ms; load 11 to 19, not a quiet reading), Discord ready, the judge's live packs
`security.v3`, `route.v1` and `rerank.v1` as before, memory live on the `baseline` arm, voice ready, `cgroup:
delegated`, the unit active with NRestarts 0, and no error or warning in the journal; the store from format 22 to 23 at
its first write, after the install's backup. The install's restore drill read Eddie's history whole with `--after`
paging to compare it, node for node.

**Divergences.** `ledger.tail`'s cursors, which the brief listed, were already on main; the session paged
`session.history` "as ledger.tail does" and left the ledger alone. An end under 6 characters is `NOT_FOUND`, not
`INVALID_PARAMS`. The TUI's pane shows no short ids (`render::node_lines` unchanged, as the brief asked).

**Known gaps.** Three P3s, none holding the join: **theseus-gk93**, no test holds `keys_ending` to reading keys only
(R19's suggested test needs no counter: payloads made unreadable while the index keeps their keys); **theseus-zf8e**,
the cursors come from the decoded nodes, not the store page's `first` and `last`, which count a refused frame, so a page
made only of refused frames ends a walk early; **theseus-8obx**, `ledger.tail`'s `older` with both cursors (the
report's own observation, confirmed). R19's "For Eddie", each recommended: accept the page of 200 for a cursor without
`n`, `NOT_FOUND` for an end under 6 characters, and the TUI's pane without ids until kym3's TUI half; fix 8obx (one
line, history's rule: `before` alone) before the TUI pages back, and zf8e with it, adding one line to
`without_a_cursor_the_answer_is_todays` (an old client's whole read after its 300 more nodes); carry `-n` into the
next-page line (live, `-n 20 --before …` suggested a command whose page is 200); a wire fixture for the cursors at the
next protocol touch. The cockpit's polls with `after` (`web2`) and the TUI's scrolling with `before` (kym3's TUI half,
`tui2`) are later steps. soul-import joined after this branch (Item 201), so its joiner met the two points R19's
guarded resolve steps covered (protocol `lib.rs`'s module lines and `session_history`) from the other side, resolved
by R24's resolve script the same way (the page kept, its nodes through `import::shown`), and the erased-node point
stands: an erased imported node has two records, so a paged walk can show it twice (`shown` dedupes within one read).

