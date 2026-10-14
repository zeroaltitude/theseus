# CLOUD_REPORT: standing-surface (theseus-slvl.4)

Branch `cloud/20261013-standing-surface`, from main as cloned (store format 33; this build is **34**). Started 06:52 UTC, 2026-10-14;
the code was pushed at 08:36 (`4f1f9ca`). Every commit subject carries `theseus-slvl.4`. The trailer names Claude and the session
but not the model: this session's rules keep a model identifier out of anything pushed to a repository, and they win over the
brief's `(cloud session, Sonnet 5.5)` wording. Change the trailers at the merge if you want the brief's exact text.

## Commits

| Commit | Step |
|---|---|
| `5b38fc2` | 1. `StandingScope::Surface`, rendered in the turn's tail; store format 34; `turn.submit`'s `surface`; `--surface`; `standing.list`'s `surface` |
| `7031da7` | 2. the summary's clause on format, length and tone |
| `8b2018b` | 4. `standing.repeated {record, node}` |
| `bf2b78e` | 3a. the TUI's `📌 N` pill and `p` |
| `7c973c0` | 3b. the cockpit's Standing tab on the Context page |
| `4f1f9ca` | 1b. Discord's `/orders add:` with `kind:` and `surface:` |

## 1. The surface scope

**Found.** The standing block is the first thing in the system's second block, so a record scoped to a surface rendered there
would make the system differ by surface (a cache write on each). The turn knew no surface: `TurnRequest` had none, and the node
does not record one (its `origin` says operator/mcp/import; `author` is a free label). Voice turns come through the Discord
connection with no mark. The task view and the commitments already ride the request's last message
(`task_graph::view::attach_text`), after the cached prefix, so the surface block uses the same lane.

**Changed.**
- `StandingScope::Surface(String)` with `SURFACES = terminal | discord | voice | cockpit | mcp` (protocol `standing.rs`).
- `TurnRequest.surface: Option<&'static str>`, set by `turn.submit` from the connection (`standing::surface_name`: CLI and TUI
  `terminal`, web `cockpit`, Discord `discord`, MCP `mcp`, an unnamed connection none). A continuation, a wake and a task's turn
  name none, so no surface record holds on them. `turn.submit` gained an optional `surface`: the Discord binding sends `voice`
  for a spoken turn, and the core reads it from a Discord connection alone (a CLI that says `voice` is still the terminal;
  an unknown value is the connection's own). `TurnRequest` moved to `turn/request.rs` (turn.rs was 1 line under its ceiling).
- `Block::here` renders the surface's records as `Standing here:` with the same kind headings and the same privacy rule
  (a shared place gets the orders' text and the withheld line, never the text of a preference or a fact). The system's `Block::of`
  leaves surface records out. `Turn.standing` is now `standing_step::Held {block, here, surface}`.
- The tail attaches in `compile_step` after the task view and the commitments, and on the detour path (route_step), counted in the
  estimate. `standing::missing_here` refuses a request whose last message dropped a shown record (`context_unadmitted`,
  `standing_missing`), and the withheld line likewise. `task_graph::view::is_head` skips the block (a stand-in model that reads
  the last user text).
- `context.compiled` names what the tail carried (`standing_here`, absent when none); `Manifest.standing` stays the system's
  (the compilation is surface-independent). The tally of `standing.list` therefore counts only the system's records: a surface's
  show "in the tail of that surface's turns" in the cockpit and have 0/0 in the list.
- `standing.list { session_id, surface }` (the asker's surface, so the TUI's pill counts what holds here); `standing.add`
  refuses an unknown surface name. `theseus standing add --surface NAME` is `--scope surface:NAME` (and `--scope surface:NAME`
  works); an unknown name is refused before anything is sent.
- **Store format 34** (`MANIFEST_FORMAT`): a new variant of a stored enum, `{"kind":"surface","of":"terminal"}`. A body variant owes
  no old-layout sample, but the brief asked for one: `tests_layouts` has the format-33 record as a literal (it reads and writes back
  byte for byte), and a format-33 store holding a record that opens, takes a surface record and moves its manifest to 34. The four
  existing `moved["format"] == 33` asserts, `store.rs`'s `a_write_moves_an_older_store_to_this_builds_format` and theseusd's
  `versions.rs` (its "newer store" is now 35) follow the number. The two goldens the brief names (`core_output.txt`,
  `kernel_frames.txt`) did not change: neither holds the format.
- Cost, in tokens: **the system header +0**; nothing when no surface record is live; with one, about 20 tokens in the request's
  tail per request (`Standing here:`, `Preferences:`, `- std_xxxxxxxx: <the text>`; about 5 more a line), never cached, never a
  cache write, never a recompile. Why it earns them: it is the owner's own sentence, said once, honoured on that surface only.

**Proved.** `tests_standing/surface.rs` (8 tests, the repeat's among them; all pass):
`a_surface_record_rides_the_tail_of_its_surface_and_the_prefix_stays_identical` (the tail on the terminal only, after the person's
words, the last block with no breakpoint; system and tools byte-identical on the discord turn, the no-surface turn and the
terminal turn again; `new_session` is the only recompile, also after a revoke),
`a_fresh_session_has_the_same_system_on_every_surface` (terminal, discord, cockpit, mcp: the system and tools are the same bytes),
`a_surface_record_in_a_shared_place_is_withheld_by_the_same_rule`, `a_request_that_drops_a_surface_record_is_refused_and_one_that_carries_it_says_so`
(the refusal names the record, nothing is sent, `context.unadmitted` is written; and the `standing_here` of the row),
`a_turn_submit_takes_its_surface_from_its_connection` (through real connections: Cli, Web, Discord, Discord + `voice`, Cli + `voice`,
Discord + an unknown name), `a_surface_scope_is_added_for_a_known_surface_and_listed_by_it`, `a_connections_surface_names_the_scope_it_holds`.
The CLI's goldens `standing_add_surface.txt` (the request carries the scope) and `standing_add_bad_surface.txt` (refused unsent).

**Planted reverts** (each file restored and `touch`ed, `git status` checked):
- the surface record rendered in the system block (`Block::of` filter off): `a_surface_record_rides_the_tail_of_its_surface…`
  (the system holds the record) and `a_fresh_session_has_the_same_system_on_every_surface` failed;
- `missing_here` made to pass: `a_request_that_drops_a_surface_record_is_refused…` failed (`unwrap_err()` on `Ok`);
- `Scope::holds` returning true for every surface scope, and the web UI named `terminal`: six of the eight surface tests failed
  (the discord turn carried the terminal record; the connection test saw the web turn on `terminal`).

## 2. The summary's clause

**Changed.** `turn/compaction.rs` INSTRUCTIONS gains "Keep any instruction the operator gave about format, length or tone, in
their words." (about 17 tokens, on the summary call only; nothing on a turn's path).
**Proved.** `tests_compaction::a_style_request_survives_a_compaction_in_the_operators_words`: a session says "for now, two
sentences max", a forced compaction rings, the stand-in summarizer (it quotes the operator's style sentences only when its
instructions carry the clause) is shown the message whole, and the next request's head is a summary that says it in their words
while the message itself is gone. **Honest limit:** this proves the plumbing and the clause, not that a real model obeys it; the
live check for that is a real compaction on a real summary model (below). Planted revert: the clause removed; the test failed at
the call's instructions (`contains(KEEP_CLAUSE)`).

## 3. The records where the owner looks

**TUI** (`crates/theseus-tui/src/standing.rs`). The header shows `📌 N` for the live records that hold in the open session on this
surface; `p` opens a box listing them (id, kind, scope, text; any key closes). Asked with `standing.list {session_id, surface:
"terminal"}` when a session is focused, after a turn this TUI sent, at a reconnect and when `p` opens: no timer. An older daemon's
refusal says nothing. 4 tests; revert (the pill returns nothing): `the_header_counts_the_records_that_hold_in_the_open_session`
failed. The pin is two cells wide, so a test screen reads `📌  2`.
**Cockpit** (`StandingRecords.tsx` over `src/lib/standing.ts`): a Standing tab on the Context page: the live records in three
groups, each with scope (`on terminal` for a surface), since, source, how many compiles carried it, lapse time, and Revoke
(`standing.revoke` after a confirm; the core judges it as the owner's act from the cockpit's private connection). It reads only
while shown. The brief says "cockpit component test": the app tests only pure modules (`node --test`, no DOM), so the tests are
`test/standing.test.ts` (4) on the pure parts, and `explainers.test.ts` holds the panel to an explainer (`context.standing`,
added). `npm test` 381 pass, `npm run lint` no new warning, `tsc -b` and `vite build` clean. Revert (ended records included):
the grouping test failed.
**Discord**: `/orders add:<text> [kind:] [surface:]` (options on the one command, so a bare `/orders` is a list as before, instead
of subcommands that would have broken it) sends `standing.add` as the presser; the core judges it. Test: the options become the
params; revert (surface ignored) failed it. **Not driven end to end**: the stand-in Discord does not run slash commands (the
AGENTS says so), so the interaction itself is untested here.

## 4. `standing.repeated {record, node}`

`LedgerKind::StandingRepeated`, its fact (ids only) and `Core::standing_repeated(session, record, node)`, which refuses a record
that is not live. The kind is "written" by its fact (the registry test passes) but **nothing calls the function yet**: the capture
row will. That is a declared-before-its-reader row, as the brief asked; the reader rule's reader is that row and the exam. One test
(two repeats are two rows of ids scoped to the session; a revoked record is refused).

## Live check (a scratch daemon on stand-ins, this VM)

`theseus-sim discord rig` laid out a daemon (fake Discord with its gateway; `#lab` bound private, the owner in it) and a small
logging stand-in for the Messages API (`logmodel.py`, which keeps each request's system block digests and last message; not
committed). Then, from the debug build of this branch:

```
theseus --socket $D/sock standing add --kind preference --surface terminal "code first, no pleasantries"
theseus --socket $D/sock standing add --kind order --surface discord "no links in replies"
theseus --socket $D/sock standing add --kind order "never push to main"
(cd $D/projects && theseus --socket $D/sock ask "second hello from the terminal")
theseus-sim discord say --fake 127.0.0.1:9447 --channel 900000000000000010 --user 900000000000000101 --name ana "second hello from discord"
```
- the terminal turn's last message ends `Standing here:\nPreferences:\n- std_986dc441: code first, no pleasantries`, and not the
  discord order;
- the Discord turn's ends `Standing here:\nOrders:\n- std_aa04a338: no links in replies`, and not the terminal preference;
- both requests' system blocks: digests `['149ea498b6', 'f2b1ad32f1', 'ed677169d3']` on both, whole-system digest `567c14751308543b`
  on both, tools digest identical. (A first pair differed only because the CLI session's directory block named the CLI's cwd:
  the third system block is the session's directory, as designed. Run both from the daemon's own directory to compare.)
- `ledger --kind context.compiled`: `standing_here: ["std_986dc441"]` on the terminal session's compiles, `["std_aa04a338"]` on
  the Discord session's, an append included.
The daemon and the stand-ins were stopped by pid. **For the maintainer, on the owner's machine with a real model:** repeat the
above with `theseus standing add --surface terminal …` and compare `theseus context explain`/the digests across a CLI and a
Discord turn; then run a real long session to a compaction after "for now, two sentences max" and read the summary
(`theseus history`) for the sentence.

## Speed

- `theseus-sim bench turn --check --runs 5 --burst 0`: frames plain 4 (budget 4), tool 8 (8), batch 14 (14), syncs 2 (2),
  first request 38.56 KB (40), all `ok` (before and after the final commits).
- `theseus-sim bench lifecycle --runs 10 --check`: **LIFECYCLE OK** (twice; SIGKILL restart p95 39.8 ms, binary swap p95 30.2 ms,
  clean shutdown p95 30.4 ms), debug binaries on this VM.
- No new timer, no new call or frame on a turn's path: `stand()` already read the records once a turn; it now builds two blocks
  from the same list (the second is empty unless a surface record is live).

## The gate

`CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` on the committed tree (`4f1f9ca`): door, keel, keel
tests, fmt, shape, features, clippy (`-D warnings`), cockpit (lint, tests, tsc, vite), test build, and the reader rule pass. The
suite: **4,490 run, 4,456 passed, 34 failed, 47 skipped**. The 34:
- 33 are the known L1 failures (root VM, theseus-pv6i): theseus-sandbox's 19 contract tests and `bench spawn_100`, and 13 of
  theseusd's `sandbox` tests (one waits 30 s).
- 1 is `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer`. It is **not mine and not
  in the known list**: it fails the same way run alone (a hold on the process message's create waits for a text that never
  comes, "timed out: the thinking's create, held, and the reply's post"). Its fix is `b495389` (theseus-wpv2g: "the thinking-answer
  test holds the process message's create whatever its text, and asserts its scenario happened"), which is in main later than this
  branch's base (`git merge-base --is-ancestor b495389 HEAD` is false). I touched no Discord code it reads (`orders.rs`, and a
  `surface` field on the submit params).
Seen once in an earlier full run and not again: `theseusd::gone_jobs a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran`
(6.1 s; passes alone). The suite stops there, so I ran the phases after it by hand: protocol types (clean: the TypeScript is
committed), the turn step (above), deny (`advisories ok, bans ok, licenses ok, sources ok`). Benches were skipped by
`THESEUS_GATE_NO_BENCH`; the lifecycle bench was run by hand (above).

**Python under bench/**: untouched.

## Keel

`python3 scripts/keel-guard.py` on the branch: `keel: ok (… 0 findings acked)`. Its one line of output,
`the same test: crates/theseus-store/src/store/tests.rs::a_checkpoint_for_close_is_made_durable_by_the_close -> …_is_durable_without_the_close (renamed and rewritten)`,
is main's, not this branch's (its range is `merge-base(HEAD, origin/main)`, which here predates my base). No test deleted, no
`#[allow]`/`#[expect]`, no ceiling raised (`turn.rs` shrank by moving `TurnRequest` out), no cap changed.
**Keel findings expected: none.** Changed assertions, for the record: `moved["format"] == 33` became 34 in four places of
`tests_layouts.rs`, in `store.rs`'s `a_write_moves_an_older_store…`, and in theseusd's `versions.rs`; the newer-store test there uses 35.

## Things the owner should hear about

1. **The surface block is in the tail, and a surface's records are not in any manifest.** The proof that a request carried one is
   `context.compiled.standing_here`, the situation check, and the tests; `compilation.list`'s manifests cannot show it. The
   `standing.list` tally reads 0/0 for them. `context.explain` does not count them yet (nor the system's records: that was
   already open).
2. **Voice is a client's word within Discord's connection.** There is no other way to tell a spoken turn at the daemon; any other
   connection that says `voice` is ignored. The label is a convenience, like `author`, not an authority.
3. **A tail block can differ per turn without a recompile**, so the same session on two surfaces shares one compilation and one
   cache, which is the point; but a stand-in model that echoes the last user text sees the block (the task view's skip covers it).
4. **`/orders add:` is options on `/orders`, not a subcommand** (see 3). If you want `/orders add` as a literal subcommand, `list`
   becomes a subcommand too and the bare command stops working.
5. **No config keys were added** (nothing to put in the template). No new dependency.
6. **Disk on this VM.** The session's disk is about 40 GB, and the workspace's test build plus its incremental caches (12 GB) filled
   it twice. I ran the builds with `CARGO_INCREMENTAL=0` and deleted `target/debug/incremental`. In one gate run I also deleted
   old workspace artifacts by hand while the suite ran, which removed binaries it was about to start; that run's failures are mine
   and are not in the table above. The final run (`4f1f9ca`) is clean of that.

## Docs that should change at the merge (I may not edit them)

- Spec, Part III: a Part III item for slvl.4 (the surface scope, the tail block, store format 34 and its reason; the summary clause;
  `standing.repeated`; the TUI pill and the cockpit tab), and the version line in `docs/spec/01-front.md`.
- `docs/status.md`: the "Updated" line, the recently landed step, the roadmap row for the personality plan's row 4.
- The personality plan's row 4: three of its pieces are built; what is left is the capture row (it calls `standing_repeated`) and
  the owner card; `context.explain`'s totals still do not count the standing blocks.
- I updated the directory guides (`crates/theseus-core`, `-protocol`, `-tui`, `-discord`, `theseus`, `cockpit`).
