# Cloud report: herdr-follow (theseus-fuw5d)

Branch `cloud/20261013-herdr-follow`. Code commit: bece8a12.

## What I found
- herdr-reporter and the work board (`work.watch`) are on main, so the follower uses `work.watch` (limit 0, not interactive).
- herdr is not installed on this VM and could not be fetched (403 from the proxy). Its schema was not available, so **the request shapes
  in `herdr_up/wire.rs` (`layout.apply`, `events.subscribe`, `workspace.report_metadata`, `agent.view.set`, and the layout answer's
  ids) are my reading of the brief, not checked against `herdr api schema --json`.** Each is built in one function, so a mismatch is a
  one-place fix. The event parser accepts the name under `event`/`type`/`method` and `_` or `.`.
- `up --follow`/`--boot` not given `--here` delegates: it makes the `board` tab (pane argv `theseus ... herdr up --follow --here`, token
  `kind=board`) and exits; the follower is the board tab's process. This is how I read "the board tab's own". `--boot` also works with `--here`.

## What I changed
- New `crates/theseus/src/herdr_up.rs`, `herdr_up/state.rs` (pure rules, takes `now`), `herdr_up/wire.rs`; `HerdrCmd::Up`; an early
  dispatch in `main.rs` (a `connect` helper keeps `run()` at clippy's 100 lines); `work.rs` `view_line`/`change_line` made `pub(crate)`;
  `default_socket` made `pub(crate)`; AGENTS.md bullet.
- Flags: `--follow --boot --here --pin S --finished-grace SECS (default 600) --herdr-socket`. No config keys were added, so nothing for the template.
- `sync` is **unchanged except pins** (theseus-d5oc): `--pin` writes a `pinned` token on the session's existing pane and a later sync reads
  it. The brief says sync "stays as up without --follow"; I kept sync's own pane path so its tests and goldens stay as they are. The
  owner may want sync to call `up` instead. A pin on a session with no pane yet is not remembered by sync (it is by `up`).
- Closes: cancelled at once; finished a grace after its tab/pane was focused; a conversation's the moment it stops needing you; unseen
  finished tabs stay. A task already finished at the first read gets a tab only if it ended within the grace. A tab with a session token the
  board does not hold is adopted as ended-and-seen and closes after the grace (a risk if another daemon shares the workspace).
- A pane that exits is rebuilt, held back 5 s per session so a watch that dies at once is not restarted in a loop.

## How I proved it
- `cargo nextest run -p theseus`: 383 passed (8 new integration tests in `tests/herdr_up.rs`, 8 unit tests in `state.rs`/`wire.rs`).
  Integration: snapshot -> tabs, one `layout.apply` for a new task, counts `●2 ◐3`; finished+focused+grace and cancelled -> `tab.close`
  (grace measured >= 1.9 s of 2 s; unseen stays); `pane.exited` -> rebuild; `--boot` over restored shells -> closes before layout;
  `--boot` makes the board tab and a second boot makes nothing; a second `up` sends only `workspace.list`/`pane.list`; a pin survives a second
  `up`; daemon lost -> exactly one notice, back -> one, nothing re-laid. The binary tests use real time (2 s grace); the pure state tests
  use made-up instants, so no paused clock was needed.
- Planted reverts: `pane.exited` no longer clears the tab -> `a_pane_that_exits_is_swept_and_its_tab_made_again` failed; grace ignored
  (`seen_at.is_some()`) -> the state unit test and `a_finished_tab_closes_a_grace...` failed. Both restored and touched; `git status` clean.
- Idle CPU: a follower against a scripted daemon and herdr, 150 s: 0 clock ticks in `/proc/<pid>/stat` (not the full 10 minutes).
- `cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `scripts/shape.sh`: clean. Cargo.lock unchanged.

## Live check for the maintainer (not done here: no herdr)
1. A scratch herdr session; a scratch daemon with 20 tasks. `theseus --socket S herdr up --boot --herdr-socket H`. Expect a `board` tab, then
   20 task tabs with nothing typed; each herdr toast names its task.
2. Restart herdr and run the same command: leftovers closed, tabs rebuilt. Look at every tab; after the grace only `board` remains.
3. Check `layout.apply`, `events.subscribe`, `workspace.report_metadata`, `agent.view.set` against `herdr api schema --json`.

## Gate
`scripts/gate.sh` fails in **keel**, on a commit of the base, not mine: the `Keel:` ack on 1c2beb2747 (a budget raise in
`crates/theseus-sim/src/lifecycle.rs`) does not verify on this VM (`%G? is E`). My range has no keel finding. Guard output:
```
keel: the same test: crates/theseus-store/src/store/tests.rs::a_checkpoint_for_close_is_made_durable_by_the_close -> crates/theseus-store/src/store/tests.rs::a_checkpoint_for_close_is_durable_without_the_close (renamed and rewritten)
keel: refused the ack `Keel: budget-raised crates/theseus-sim/src/lifecycle.rs — the owner, 2026-10-13 23:24: "i'm happy for yes to all until we're at testing time to eval" (seed tightened by store size; push budget above the 20 us target until theseus-oy9zq)` on 1c2beb2747: its signature does not verify (%G? is E)
keel: 1 erosions of a test, a budget or a ceiling (a branch: merge-base(HEAD, origin/main)..HEAD, 9fa84735c0):
  crates/theseus-sim/src/lifecycle.rs:155: budget-raised: "seed" => Some(250.0), -> (gone or rewritten)
keel: each passes only with the owner's yes: a commit in the range, signed by a key in scripts/keel-signers (at the base), whose message carries these trailers, each with its reason:
  Keel: budget-raised crates/theseus-sim/src/lifecycle.rs — <why>
```
I ran the later phases by hand: fmt, shape, clippy and the theseus tests pass. `cargo deny check`: advisories FAILED (bans, licenses, sources
ok); no dependency changed, so I take it as the advisory database state here; not investigated. Other crates' suites were not re-run (only
the theseus crate changed).

## Left / uncertain
- The request shapes above. The grace is a CLI flag only. The board tab prints the queue at each snapshot plus `work.rs`'s change lines;
  it does not redraw.
- Docs to update at review: the spec's Part III item, status.md, north-star item 60.
