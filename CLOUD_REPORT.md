# Cloud report: trim-docs (theseus-rwpbg.8)

Branch `cloud/20261017-trim-docs`.

## Found
- 73 top-level commands (plus `help`). 28 `about` lines (22 top-level, 6 subcommands: import claude-code/codex/people/sync, ontology person/proposals, persona import, thread import/switch, route) carried ids such as `theseus-x9swu`, `(M6)`, `(M7 43a, 43b)`.
- No help text lives in any golden (`rg Usage crates/theseus/tests/golden*` is empty), so no golden changed.

## Changed
- Commit "cli: internal ids out of the one-line help; docs/cli.md and docs/glossary.md" (see `git log`): ids removed from `about` lines in main.rs, import.rs, persona_cmd.rs, thread.rs, own_bench.rs (34 lines +, 32 -). Ids were dropped, not moved. Arg-level and long help were left alone.
- `docs/cli.md` (every top-level command, grouped, one example each), `docs/glossary.md` (39 terms; thread x3, mode x7, surface/place spellings stated plainly; no rename proposed), `docs/README.md` one link line.
- `crates/theseus/tests/help_docs.rs`: three tests.

## Proof
- `cargo test -p theseus --test help_docs`: 3 passed.
- Plants: an id put back on `voice` -> the id test failed ("voice: theseus-slvl"); `peek` dropped from cli.md -> "docs/cli.md lacks: [peek]". Both restored and touched.
- Help dump before/after for all 232 subcommand pages (diff touched only the id lines).
- `cargo nextest run -p theseus`: 624 passed. `cargo clippy -p theseus --all-targets -D warnings` and `cargo fmt --check`: clean.

## Proof nothing was lost
Every command still runs with its `--help` (the walk test calls each one); only id text changed; the guide test equals `theseus help`'s list.

## Gate
`scripts/gate.sh` stopped at its door phase, before any build: keel-guard reports erosions and unverifiable signed acks in the base history (lifecycle.rs seed, keel-guard.py, test_keel_guard.py), none in this diff, which touches no test, budget or ceiling. I ran fmt, clippy and the crate's nextest by hand instead. cargo deny and the other gate phases were not run. `keel-guard.py --base origin/main` prints the same pre-existing findings.

## Left / for the maintainer
- Link cli.md and glossary.md from the README and docs/status.md (not edited here).
- The glossary's census `harness` place, the standing-order surface names and `PlaceKind` stay as the code spells them.
- No live check needed.
