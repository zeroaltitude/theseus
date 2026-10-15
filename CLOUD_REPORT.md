# CLOUD_REPORT: watch-replies (HZ7, theseus-rlnbf)

Branch `cloud/20261013-watch-replies`. No store change; no new dependency; no new config key.

## Step 1: the `reply` check

**Found.** `watches` had joined. A watch is a pending wake with a `Probe` run, so the reply check rides the same record
(kind `reply`; `place`, `from`, `words` in the subject JSON). The place rule for *setting* one already exists:
`watch.create` is `judge_act(Act::Watch)` (the owner, from a private place). The Discord `runtime.rs` was at 3,498 of
3,500 lines: the change there is two lines (`mod reply;` and one call), the rest is in `runtime/reply.rs`.

**Changed** (ba68896a, and the gateway test commit after it):
- protocol: `WatchCheck::Reply { place, from?, words? }`; `WatchCreateParams.until` now `#[serde(default)]`; TS regenerated.
- kernel: `ProbeNews.detail` (extra fields for the verdict's ledger row).
- core `watch/reply.rs`: matchers (`from`: user id, `discord:<id>` or a name, any case, contained; `words`: text in any
  case or `/pattern/`), `Core::watch_reply_seen` / `watch_reply_met`, `Entry::replied`, `resolve_place` (a bound place's
  name, its target, or a channel id). `watch/board.rs`: the place index lives in the board (kept by insert/insert_new/remove,
  so a restart rebuilds it with `watch_load`); `reply_matches` is one atomic load with none open and one map lookup with none
  for the place. `spec.rs`: no `every`/`until` for a reply, `then` must be notice. `step.rs`: a reply is seeded due at its
  expiry (its one look) and never checks. `run.rs`: the notice target for a reply is private only (`reply_target`: the
  watch's own place if private, else Home), `WatchInfo` says `on arrival`.
- model tool `watch`: `kind: reply` (asks the owner once, like a `run` watch); `until` no longer required.
- Discord: `runtime/reply.rs` tells the core of every message before routing (reads only).
- CLI: `theseus watches add "harbour master replies" --reply '#ops' --from 'harbour master' --words … --for 5d`.
- AGENTS.md of core, discord, CLI.

**Proved.**
- `cargo nextest run -p theseus-core -E 'test(/watch/)'`: 44 passed (7 new in `watch/tests_reply.rs`: named person fires and
  another's/another place's does not; words as text and pattern, with `from` both must hold, bad pattern / `then: turn` /
  unknown place refused; restart rebuilds the index and a reply after it fires; never due on the tick and expiry;
  notice never into a shared place even when the watch lives in that place's session; a shared-place connection (and the
  owner *from* the shared place) is refused `watch.create`, the owner from the DM is allowed; a 1,000-message burst).
- Planted reverts: (1) `Board::insert_new` not indexing (index not rebuilt at start) → `a_restart_rebuilds_the_index_and_a_reply_after_it_fires` failed; (2) `reply_target` returning the session's target first (notice into the shared place) → `the_notice_never_goes_into_a_shared_place` failed. Both restored and touched.
- FAST: 1,000 messages through `watch_reply_seen` took well under the test's 50 ms bound with no watch open and with one open for another place (the test prints both). I did not do a before/after on the live binding.
- Live check on the fake Discord rig: `theseus-discord` `tests_gateway::reply::…` (passes): a watch on shared `#lab` for ben; cy's message fires nothing; ben's (ben may not drive Theseus there) puts the notice in ana's DM with the quote, nothing in `#lab`. Commands for the maintainer: `cargo nextest run -p theseus-discord -E 'test(/tests_gateway::reply/)'`; and on the owner's machine, with a scratch daemon bound to a test guild: `theseus watches add "harbour master replies" --reply '#ops' --from 'harbour master' --for 5d` then `theseus watches` (shows `waiting for a reply`, `on arrival`) and have another account post in the channel.
- Full suite (`cargo nextest run --workspace`): 4,993 run, 4,960 passed, 33 failed, all the known L1 ones (theseus-sandbox contract + `spawn_100`, theseusd sandbox). clippy `-D warnings`, fmt, shape: clean. Cockpit lint/test/build: pass (lint warnings are pre-existing).

**Design choices for the owner.**
- A reply watch can only *notice* (`then: turn|task` is refused). Quoted words are outside text; a turn or task would act on them and I could not hold the session in the met-frame without a store/hold path. The row carries `external: true` and the notice quotes at most 200 scrubbed characters, one line. Easy to revisit.
- `from` matches a user id, `discord:<id>` or a name (contained, any case); it does not expand a held *person* (their other handles). Left out for time; the people lookup would go in `reply::from_match` at set time.
- The call sees every message the bot hears, in a bound place or not, so a watch can wait on an unbound channel (given its id); a bot-authored message never reaches it.
- The model's `watch` tool schema/description grew a little: the turn bench's first-request budget (42 KB, temporary) was not run here (benches are off in the lane gate); it may need checking.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` fails in **keel** (and so in the door), on a finding that is **not mine**: `crates/theseus-sim/src/lifecycle.rs:155 budget-raised ("seed" => Some(250.0))` from main's own commits, whose `Keel:` ack signature cannot be verified on this VM (`%G? is E`: no key). I ran the later phases by hand: fmt, shape, clippy, protocol TS (regenerated and committed), cockpit, the full suite (only the 33 known L1 failures), as above. `cargo deny` could not run (`--offline` with no fetched database; `cargo deny fetch` was not done). Benches not run.

**Keel findings expected:** none from this branch.

Environment note: the VM's disk allowance fills with a debug `target/`; I built with `CARGO_INCREMENTAL=0` and debug info off for the later runs.
