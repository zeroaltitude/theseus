# CLOUD_REPORT: trim-effort (theseus-rwpbg.4)

Branch `cloud/20261017-trim-effort`. Code commit `c08a66d8`.

## What I found
- Non-Fable models rewrote the messages' cache whenever Jev's level differed from the previous request's; keep-warm built its read from the profile's effort, so it differed from the last real request.
- `effort::default_of` was a hard-coded `match` on `claude-fable-5-1`.

## What I changed (c08a66d8)
- **(c)** `CatalogEntry.per_message_effort` / `CatalogRow.per_message_effort` (serde default false, `over` merges), true for `claude-fable-5-1` only; `BUILTIN_VERSION` 2026-10-18.1. `default_of(entry)`, `takes(spec, entry, model)`, `place(..., entry, ...)`, `retarget(..., entry)` read the catalog. The default level (`high`) stays in code: it is the same for every such model.
- **(b)** `routing::effort::sticky` (pure) plus `TurnRunner::stick` (`turn/effort_step.rs`), called from `route_effort`, so the decision is made before `route.decided` is written. Rule: hold at the previous request's level unless no previous (first request, restart, model switch), request estimate < `[cache] keep_warm_min_tokens`, or a gap of 2+ steps. The bounds and the 0.70 evidence bar act first. Only for models whose `takes()` is false and whose row `caches`. The floor measure is the compile's own `est_tokens` (the whole request estimate, the same measure keep-warm applies to the same key); no count pass added.
- **Record:** `EffortReason::Sticky`, `EffortDecision.sent`. `route.decided` gains `effort_sent` (only when sticky; rows are otherwise byte-identical). `effort_ran` is Jev's level, `effort_reason` is `sticky`. `TurnRoute.effort_applied` is the level sent. The doc in protocol `route.rs` is updated and the TS regenerated; no wire field was added.
- **(a)** `KeepWarm::sent` / `last_effort` (in memory, keyed by session, model-checked), noted after each answered call (`TurnRunner::note_sent`, not for a detour), and `keep_warm_request` uses it in place of the profile's effort. Without a record (restart, other model) today's behaviour.

## Proof (cargo test -p theseus-core --lib; nextest could not be installed here)
- New: `tests_effort_sticky.rs`, 7 tests, all pass (the six named, plus `sticky_is_one_step_at_most`, a table over all level pairs).
- Neighbours pass: `tests_turn_effort`, `tests_route_effort`, `catalog::`, `routing::effort`. Protocol suite passes (130 + others); clippy is clean on core and protocol.
- Planted reverts, each restored and `touch`ed:
  - keep-warm uses the profile's effort: `keep_warm_replays_the_last_effort` fails.
  - sticky never holds: `sticky_is_one_step_at_most`, `sticky_effort_holds_a_one_level_change...`, `the_route_records_asked_and_sent_effort` fail.
  - hard-coded Fable match in `default_of`: `per_message_effort_is_catalog_data` fails.
  - "always apply" for the 2-step and floor tests is the same code path as the sticky revert (those two tests assert applying and pass either way, by design: they guard against over-holding).
- Full lib run: 3047 pass, 4 fail: `card::tests_job::*` (3) and `import::tests_catalog::a_quiet_stretch...`. All fail identically on the base (checked with the change stashed), so they are not mine.

## Proof nothing was lost
- Per-message models unchanged: the `takes()` branch is untouched; `tests_turn_effort` (Fable) passes.
- A turn with no applied effort, or a short conversation: unchanged (`sticky_effort_applies_under_the_prefix_floor`); route row JSON is unchanged unless sticky (`effort_sent` absent).
- The answer runs at most one step from Jev's pick, and only on a long conversation (`sticky_is_one_step_at_most`).
- Keep-warm's read equals the last request's system, tools, top-level effort and messages prefix (`keep_warm_replays_the_last_effort` compares them).
- Keel guard against the true base (432bbbeb): empty. Against the current origin/main it lists Discord and herdr test removals that come from main moving, not this branch.

## Not run here
The full gate (`scripts/gate.sh`) and the suite under nextest: the VM's disk filled during setup (I cleared `target/` and rebuilt only theseus-core's tests), and `cargo-nextest` / `cargo-deny` were not installed. The remaining gate phases and a live check are the maintainer's.

## Live check
Needs the owner's keys: with a long (20k+ tokens) Sonnet 5.5 conversation, two turns whose Jev levels differ by one step: `theseus ledger` / `route.decided` should show `effort_reason: sticky`, `effort_sent` = the earlier level, and the second call's `cache_read` close to its prefix with `cache_creation` small.

## Probe outcomes (one-line edits at the join)
- Sonnet 5.5 takes per-message effort: in `catalog.rs`, the `claude-sonnet-5-5` insert becomes `CatalogEntry { per_message_effort: true, ..claude(...) }` (as the Fable row now is). On the owner's machine: `[catalog."claude-sonnet-5-5"]` with `per_message_effort = true`.
- Opus 5.5: the same on `claude-opus-5-5`.
(A refusal at run time still marks the model off for the daemon's life, `provider.effort_refused`.)

## Uncertain / for the owner
- Sticky is in memory: the first request after a daemon restart applies Jev's level as asked (one rewrite).
- A turn where Jev is unsure/unclear sends the profile's own effort, which can still differ from the previous request's; sticky covers only a Jev-applied level (task scope).
- Stale doc comment places for the maintainer: `docs/` Part III item and the `AGENTS.md` effort paragraph in theseus-core ("Fable 5.1 alone") should say the catalog's `per_message_effort`.
- Left alone: Discord's copies of `clip` / date math (contract 7).
