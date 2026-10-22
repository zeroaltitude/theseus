# CLOUD_REPORT: tail2-mailauth

Branch `cloud/20261021-tail2-mailauth`, based on `integ/tail` at `cb19957a`. No ceiling was exceeded, and the first-section note about stale line ceilings does not apply: none of the touched files is listed.

## What changed, per Beads id

- **theseus-jt4nr** (`d21145b`): `watch/mail_auth.rs`'s `aligned` now compares organizational domains with the `psl` crate (option a). Both inputs are lower-cased and lose a trailing dot. A name with no organizational domain (a public suffix, or empty) aligns with nothing. `dmarc`'s exact `header.from` check is untouched. The module doc names the rule.
- **theseus-by0vj** (`5bbc8b2`): the template's `trusted_senders` value is byte for byte as before. Only the words after the second `#` changed, to say that a task starts only when Gmail authenticated the sender. `config::tests_google` passes.
- **theseus-l6u2q** (`a580a12`): `wait_status::post` now runs inside the same `theseus_store::blocking` closure as `clock.beat`, after the beat's facts, and only when the beat returned `Some`. `post`'s doc comment says its caller runs it there. This is a pure refactor with no plant.
- **theseus-1qexl, repo part** (`ab469bc`): `crates/theseus-core/AGENTS.md` and `docs/status.md`, plus one narration word (below).

## The alignment rule: (a), the psl crate

`psl` 2.1.244, MIT/Apache-2.0, with one transitive crate, `psl-types` 2.0.11. Those are the only two new packages in `Cargo.lock`. `cargo deny check` printed `advisories ok, bans ok, licenses ok, sources ok`. Its one warning was the existing `license-not-encountered` for `Unicode-DFS-2016`. The list is compiled in and includes the private section, so `github.io` is a suffix. Unknown top-level domains such as `.test` follow the default rule: the organizational domain of `mail.example.test` is `example.test`.

## Tests and plants

- `a_pass_for_a_public_suffix_never_aligns` (new) covers `co.uk` against `shop.example.co.uk` (false), `github.io` against `site.github.io` (false), `other.github.io` against `site.github.io` (false), and `mail.example.co.uk` against `example.co.uk` (true).
- `hosts_of_one_organization_align` (new) covers `news.example.test` against `shop.example.test` (true), `example.test.evil.test` against `example.test` (false), and case plus a trailing dot (true).
- `authentication_results_are_read_as_gmail_wrote_them` passes unchanged. None of its assertions was edited.
- **Planted revert:** I restored the dot-suffix `aligned`, then restored the file and touched it.
  - `a_pass_for_a_public_suffix_never_aligns` failed with `assertion failed: !at("co.uk", "dana@shop.example.co.uk")`.
  - `hosts_of_one_organization_align` failed with `assertion failed: at("news.example.test", "dana@shop.example.test")`.
  - `git status` after the restore showed only the intended changes.
- `tests_wait_cost::beat::{a_silent_jobs_beat_posts_one_status_line_to_its_place, no_status_line_without_a_beat, the_status_line_is_scrubbed}` pass: 4 of 4 with the config's range test. The task named them in `tests_wait_beat.rs`; they live in `tests_wait_cost::beat`.
- Also run: `nextest run -p theseus-core tests_output checkpoint route` gave 186 passed. `cargo clippy -p theseus-core --all-targets -- -D warnings` is clean. `cargo fmt -p theseus-core` changed only the two files I edited.

## The turn checkpoint's narration

In `fact/route.rs`, `narrate` said "the turn moves" for a `turn checkpoint` source. A checkpoint move goes through `route_to`, which writes `session.routed` (`turn/route_step.rs`, near :906), so the next message starts on the moved profile until the step-down. The move outlives the turn, so I changed the word to "the conversation moves". The task checkpoint's "the task moves" is unchanged. No test or golden asserted the old string (`git grep "turn moves from"` finds none), and the output golden still passes.

## AGENTS.md hunks (crates/theseus-core/AGENTS.md)

- In "A running task's checkpoints", before its "Tests:" sentence: the effort without a move, and the ineligible turn's state staying `Off`. `tests_checkpoint_effort.rs` is named.
- A new paragraph, "A routed profile's effort floor", directly before "The effort". The lines from "The effort" through "A turn's effort keeps the cache" are untouched.
- Theseus-53ujg.8 (restart-adopted job handles) is already in AGENTS.md ("A job a restart found alive gets its short id too"). The recall scorer's abstention fix (53ujg.7) is already in `bench/recall/README.md`. Both parts are done.

## What could not run, or ran differently

- **The whole gate (`scripts/gate.sh`) was not run.** I ran the targeted tests and phases listed above. The VM's disk allowance ran out once during the build; I removed `target/debug/incremental` (inside the repository) and built with `CARGO_INCREMENTAL=0`. The maintainer should run `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` and `cargo nextest run -p theseus-core`.
- **Commits are unsigned.** The VM has no key.
- The trailer is `Co-Authored-By: Tabitha/Claude <noreply@anthropic.com>`, as the task's rules asked. The task's general trailer line named a different form; I followed the row's rule.

## Keel guard

`python3 scripts/keel-guard.py --base origin/integ/tail --head HEAD --quiet` printed nothing and exited 0.

## Store format

Unchanged. This row changes no stored record.

## Follow-ups

- The spec's Part III items and the rescoring of the kept recall v1 run are the maintainer's.
- The live checks for theseus-rslim (a forged `Authentication-Results` mail and an in-domain trusted sender) and for `job_wait` (`sleep 660`, one notice near 600 s) stay with the owner.
- `psl` embeds the list at its release date. A new public suffix needs a crate update.
