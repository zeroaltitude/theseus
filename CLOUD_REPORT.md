# CLOUD_REPORT: threads-links (theseus-98bz.11)

Branch `cloud/20261021-threads-links`, from `86d0af6` (integ/threads with C10 merged). Two commits besides the task
and this report:

- `c6a7fad` part A: the link (store format 48, `thread.link` / `thread.unlink`, the lens, the CLI, the cockpit).
- `86680a1` part B: Jev offers a link once (`[threads] link_proposals`).

Both parts are done. No file is over its ceiling: `crates/theseus-protocol/src/lib.rs` is still 2,730 lines (the two
methods went onto the existing `THREAD_*` line), and `scripts/long-files.txt` is untouched.

## Part A: the link

**Found.** `ThreadScene::waived` was only `named(board, message)`, so a linked pair had nowhere to be said. The lens
(`ThreadLens::judge`) already checks the seal before `waived`, so a link needs no lens change and a seal set after a
link holds without new code.

**Changed.**
- `ThreadInfo.links: Vec<String>` after `split_from` (skipped when empty). `MANIFEST_FORMAT` 47 to 48 with its history
  line in `crates/theseus-store/src/store.rs`. `THREAD_AT_47` (a seal, a `split_from`, no `links`) and its byte-for-byte
  test, plus the same record with links, in `tests_layouts.rs`; `THREAD_AT_33` still round-trips. The tests that named
  47 now name 48 (`tests_layouts.rs`, `store.rs`, `theseusd/tests/versions.rs`, whose newer-format case is now
  "is format 49" / "formats 2 to 48"). I wrote the number as a literal only in those places; the maintainer renumbers at
  the join.
- Wire: `THREAD_LINK`, `THREAD_UNLINK` on the existing method line; `ThreadLinkParams { slug, other, author, discord }`
  in `threads.rs` (added to the existing line of `ts.rs`'s type list, no new line); the result is `slug`'s `ThreadRow`.
  `ThreadChanged.change` gains the words `linked` and `unlinked`. TypeScript regenerated (`ThreadInfo.ts`,
  `ThreadChanged.ts`, `ThreadLinkParams.ts`, `index.ts`).
- Core: `rpc/thread_link.rs` (one `mod` line, one dispatch arm in `rpc_threads`). Judged with `judge_thread_pair`;
  `threads_on()` first; refused: the same thread twice, an unknown slug, a merged end ("the thread X was merged into Y:
  link that one"), a sealed end, a seal-pending end, and a ninth link (`MAX_LINKS = 8`, the number in the message).
  One `threads.write`: both records (sorted, unique) and one `thread.linked` row in one frame; a link that holds and an
  unlink that is absent write nothing and answer the thread as it is. Unlink is allowed whatever the seal. Each end is
  announced with `announce_thread`. No model tool was added or changed.
- Facts: `ThreadLinked`, `ThreadUnlinked` in `fact/thread.rs`, registered in `fact/mod.rs`; ledger kinds after
  `ThreadUnsealed`.
- Lens: `Board::linked_with(here)` (in `threads.rs`): the ends of `here`'s own `links` and every thread whose `links`
  hold `here`, each with the live thread a merge left it in (both the stored slug and `live_of`, so a merged thread's old
  notes, which the merge leaves under its own slug, stay waived). `thread_scene` unions it into `waived` only when
  `distance` is on and the session has a thread. Unit-tested through `a_link_follows_a_merged_thread`.
- CLI: `thread link SLUG OTHER`, `thread unlink SLUG OTHER` (also in `client.rs`'s `OPERATORS`, so a job's shell is
  refused, and its test); `thread show` lists `linked with`. Cockpit: `historyLinks` gives one `Linked with` entry per
  link (`Threads.tsx`'s key is now label plus slug); `threadboard.test.ts` extended.
- Docs: `crates/theseus-core/AGENTS.md` (a "Linked threads" paragraph), `crates/theseus-memory/AGENTS.md` (the
  `waived` sentence).

**Proved.** `cargo nextest run -p theseus-core -E 'test(/thread_link/)'`: 11 passed (8 for A, 3 for B), all in
`rpc/tests_thread_link.rs`:
`a_linked_threads_note_passes_the_lens_unpaid` (key; whole turns against the fake Jev: dropped `thread` before the link,
asked about; admitted after it with `fused` 0.097 unchanged and no `recall_cross.v1` request naming it; dropped again
after the unlink), `a_linked_note_ranks_at_its_own_score` (the lens alone: rank 1 above the asking thread's lesser note,
which a paid score would not be), `a_seal_after_a_link_still_holds_the_notes_out` (drops `sealed`; none of the sealed
thread's title, goal, card or note is in any request the model got or any request Jev got),
`linking_a_sealed_or_pending_thread_is_refused` (both orders, no frame, no row),
`a_link_is_written_on_both_threads_in_one_frame` (reads the stored META records; one frame; twice writes nothing;
unlink of a sealed thread is one frame; absent unlink nothing), `only_the_owner_links` (a Discord stranger is
`REFUSED`, link and unlink), `a_link_follows_a_merged_thread`,
`a_link_to_self_unknown_or_merged_is_refused_and_eight_is_the_most`. Also the layout, CLI (`link_and_unlink_parse_and_say_how_to_undo`,
`show_names_the_threads_it_is_linked_with`) and cockpit (`npm test`: 481 passed) tests.

**Planted reverts** (each file restored with `touch`, `git status` checked; the groups below were run in one build each):
- `thread_scene` without `board.linked_with(h)` in `waived`: failed `a_linked_threads_note_passes_the_lens_unpaid`
  (the key test), `a_linked_note_ranks_at_its_own_score`, `a_link_follows_a_merged_thread`.
- In a scratch copy of `theseus-memory/src/threads.rs`, the seal check made to give way to `waived`: failed
  `a_seal_after_a_link_still_holds_the_notes_out` alone.
- One build with these five together, each failing the test named for it: the sealed check dropped
  (`linking_a_sealed_or_pending_thread_is_refused`); only `slug`'s end written (`a_link_is_written_on_both_threads_in_one_frame`);
  the judge skipped (`only_the_owner_links`); the cap dropped (`a_link_to_self_unknown_or_merged_is_refused_and_eight_is_the_most`);
  `live_of` dropped from `linked_with` (`a_link_follows_a_merged_thread`).

## Part B: Jev offers a link once

**Changed.** `thread_link_seen.rs` (new), a call in `turn/cross_step.rs` (`cross_judged`, after Jev's yes is admitted),
`ThreadScene::link_pairs` in `thread_recall.rs`, `ThreadLinkProposed` (`thread.link_proposed`, after `ThreadUnlinked`),
`[threads] link_proposals = true` (last in the struct, its default and the template, with its config test at the end of
the tests module), the AGENTS paragraph.
- In `cross_judged`, the threads Jev's yes drew on are read through `lens.thread_of_node`; the place's own conversation
  has none and a held thread is excluded. `link_pairs` keeps the live, unsealed, unmerged, not-done, not already linked
  ones (the asking thread must be unsealed and unmerged too). A set, so a pair counts once a recall.
- `TurnRunner::link_seen` scrubs the sentence, then `spawn_blocking`s the count: META `thread.link_seen.<a>.<b>` (slugs
  sorted) `{recalls, asked}`, read and written under a process-wide mutex (two turns can admit one pair). At the second
  recall one `store.append` holds the record with `asked: true`, a `thread.link_proposed` row and the outbox post; then
  `outbox.posted`. No frame is added to any turn: the count is in its own task, after the recall's request has left.
- With no owner DM bound the pair is counted and never proposed (so `asked` stays false until a DM exists).

**Post kind: `onto_card`**, with `undo: false` and no `write`. I read `courier.rs`'s `jev_post` arm: it is the kind that
is Jev's own filing offer (it sorts sessions into topics), it renders `body["text"]` as plain text in the owner's DM,
and with `undo` false `onto_card::write_of` gives no button (`Buttons::Keep`). `initiative` would have wired a 👍/👎 to
`initiative.rate` for an id with no `initiative.spoke` row, `privacy_warning` and `jev_notice` say the wrong thing.
Text: `🔗 Notes of «B title» answered questions in «A title» twice. If the two belong together: theseus thread link a b`
(A is the asking thread). No new post kind, no buttons, nothing in `theseus-discord`.

**Proved.** `a_pair_admitted_twice_is_proposed_once` (two recalls: one post, `asked` true, one row; the third posts
nothing and counts nothing), `a_linked_or_held_pair_is_never_proposed` (a linked pair is never counted; a pair whose end
is sealed after its first count is not counted again and not proposed), `with_the_switch_off_nothing_is_counted_or_posted`
(off: no META key, no post, no row; and the turn's trace `frames` attribute for two turns is identical with the switch on
and off).
Plants (one build): the `asked` check dropped failed `a_pair_admitted_twice_is_proposed_once`; the switch ignored failed
`with_the_switch_off_nothing_is_counted_or_posted`.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 CARGO_INCREMENTAL=0 scripts/gate.sh` on the tree of `86680a1`: door, keel,
fmt, shape, features, clippy (both), cockpit and test build passed. The suite phase: 7,330 tests, 7,297 passed, 33 failed,
all L1 on this root VM (known, theseus-pv6i): `theseus-sandbox::contract` 19, `theseus-sandbox::bench spawn_100`,
`theseusd::sandbox` 13. Then, by hand, the phases after the suite: `git diff --quiet -- cockpit/src/protocol.gen` (ok),
`cargo deny check` after `cargo deny fetch` (advisories, bans, licenses, sources ok; it warns of one unmatched
licence allowance, `Unicode-DFS-2016`, which this change did not touch), and `theseus-sim bench turn --check --runs 5 --burst 0` (every turn 4
frames, tool-call turns 8, first requests within budget: cli 7,332, dm 9,411, task 6,585 tokens at the p95). The
lifecycle and jobs benches were not run (`THESEUS_GATE_NO_BENCH`, and they are the owner's machine's). The size step said
"skipped: no release build".

Two earlier gate runs on the part A tree: one failed also `tests_horizon::the_agenda_expands_a_series_by_the_kernels_arithmetic_and_skips_what_is_paused`
(passes alone, `1 passed`; a load flake, not near this change), and the first failed also `theseus::markdown
rendering_costs_little_per_hundred_lines` (a timing test; it passed in the next two runs) and
`theseusd::versions a_store_of_a_newer_format_is_refused…`, which was mine: it named 48 as the newer format; fixed in
`c6a7fad`.

Keel guard: `python3 scripts/keel-guard.py --base 86d0af6 --head HEAD --quiet` printed nothing (exit 0). The ref
`origin/integ/threads` is not in this clone, so the base is the commit the branch started from.

## Live check

Done here on a scratch daemon (`env:` secret, fresh state dir, no model call; the daemon was shut down with
`theseus shutdown`): `thread new alpha`, `thread new beta`, `thread link alpha beta` printed the line with the undo;
`thread show alpha` / `show beta` each printed `linked with: <the other>`; linking again from the other end printed the
line again (no frame); `thread link alpha alpha` was refused ("cannot be linked to itself"); `--json thread unlink alpha
beta` printed alpha's row with no `links`; after `thread seal beta`, `thread link alpha beta` was refused ("the thread
beta is sealed: a sealed thread's notes never reach another thread, so it cannot be linked").

The maintainer's, on the owner's keys, for the part that needs a real index and Jev (use a copy of the store and a scratch
daemon, never the operator's):
1. `theseusd --config <scratch.toml> --socket S --state-dir <copy>`, then in two threads that share a kind of fact
   (for example two with a port in their notes), `theseus --socket S thread link a b`, and `theseus --socket S ask
   --thread a "<a question whose answer is b's port>"`. Then `theseus --socket S why` (or `memory.recalls`) should show
   b's note admitted with no `thread` drop, and `theseus judge list` no `recall_cross.v1` call naming it.
2. `theseus --socket S thread unlink a b` and ask again: b's note drops as `thread` (or goes to `recall_cross.v1`).
3. For part B, with the owner's DM bound: ask in `a` twice something Jev reads as about `b`'s notes; the DM gets one
   sentence ending `theseus thread link a b`, `theseus ledger` has one `thread.link_proposed`, and a third ask posts
   nothing. `[threads] link_proposals = false` in the scratch config: none of it.

## Left, and what the owner should hear about

- **Follow-ups (theseus-98bz.12):** a Discord card with Link / Not needed buttons for the proposal, and a Discord
  `/thread link` command (`runtime.rs` is at its ceiling). The cockpit only shows links. A threadbench arm with a linked
  pair is the maintainer's.
- **Merged links.** `linked_with` waives both the merged-away slug and the thread it now lives in, so a link keeps
  working through a merge and the old thread's notes (which stay under its slug) stay reachable. Unlinking a merged-away
  thread works by its old slug.
- **A link to a thread that is later sealed** stays in both records (the owner's word is kept) and is held out by the
  seal; unsealing brings it back. The proposal never counts a held thread.
- **Staleness.** The link pairs are read from the scene's board snapshot, taken when the recall began; an owner's link
  or seal made in those milliseconds is seen at the next recall. The one real race is the count's read-modify-write,
  which is under a mutex.
- **A restart between the recall and the count** loses that one count (it is off the turn's path by design). It would
  only delay a proposal by a recall.
- **ts.rs** needed the new type added to the one line (`threads::ThreadLinkParams`), as the task said.
- **Store format:** 48 here; the renumber at the join touches the constant, its history line, the layouts' `["format"]`
  assertions, `store.rs`'s test, and `theseusd/tests/versions.rs` (three strings).
- Doc changes for the maintainer to make (I did not touch the spec, status or design): Part III's item for this row,
  the version line, status.md's roadmap row for 98bz.11.
