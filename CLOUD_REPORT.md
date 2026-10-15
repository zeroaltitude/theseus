# CLOUD_REPORT: herdr-plugin (theseus-pkaq)

Branch `cloud/20261013-herdr-plugin`. Work commit: 28e9bde4.

## What I found
- herdr's docs are not reachable from this VM, so I built the manifest, config keys and API calls from herdr's
  published source (raw GitHub: `src/api/schema/plugins.rs`, `src/config/*`, its CHANGELOG). Manifest file is
  `herdr-plugin.toml`; `[[startup]]`, `[[actions]]`, `[[panes]]` (placement `popup`), `[[link_handlers]]`; plugin
  action keys are `[[keys.command]] type = "plugin_action"`.
- `theseus herdr up --boot` is NOT on main at this base: the startup hook is `up --boot 2>/dev/null || sync`.
- `task.start` / `theseus task start` is on main: `prefix+n` is in. `theseus agenda` is not: `prefix+h` is a join note.

## What I changed (28e9bde4)
`crates/theseus/src/herdr_answer.rs` (the card), `herdr_keys.rs` (next, stop, popup, link, plugin, OSC 8 links),
`herdr_setup.rs` (snippet and `--config` check), `herdr_sync.rs` (the new `HerdrCmd` variants), `interactive.rs`
(links, only when `HERDR_ENV=1`), `main.rs` (a `run_or_own` wrapper: the commands that connect nothing first),
`contrib/herdr/theseus/` (manifest, README), `docs/herdr.md`, the crate's AGENTS.md. No daemon change, no config key,
no store change. `toml` is now a CLI dependency (already in Cargo.lock; the lock gains no package, one dependency line).

## How I proved it
- `cargo nextest run -p theseus`: 388 passed (21 new: 12 unit in herdr_answer/keys/setup tests, 6 binary tests in
  `tests/herdr_answer.rs` against a scripted daemon and a fake herdr, and the manifest/snippet/reporter-token tie).
  Covers: focused, named, oldest, none, gone; answered elsewhere meanwhile; the note path; `work.answer` params with
  author `herdr`; escape sequences inside herdr and none outside; snippet parses as TOML and every `$token` is in `TOKENS`.
- Planted reverts: (1) the answer link opens the `tui` popup instead of the card -> `a_click_on_answer_opens_the_card_and_answers_nothing` FAILED;
  (2) `$bogus` token in the snippet -> `the_snippet_parses_and_names_only_tokens_the_reporter_sends` FAILED ("$bogus is not a token the reporter sends").
  Both restored and touched; `git status` clean of them.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`: clean. `scripts/shape.sh`: ok.
  `tests_registry` (reader rule): 15 passed.
- Not measured: the answer card's start under 20 ms (nothing resident; it connects within 1.5 s and paints after 2 reads; no bench row added).

## Live check for the maintainer (I could not run herdr: the binary I fetched was refused by this session's guard)
```
theseus herdr plugin                     # prints the plugin path and `herdr plugin link <path>`
herdr plugin link <path>; herdr config check   # on a scratch copy with: theseus herdr setup >> config.toml
theseus herdr setup --config <scratch>/config.toml   # should list nothing
```
Then, with a scratch daemon holding one question: `prefix+a` opens the card and `y` answers; `prefix+shift+a` focuses the
pane; Ctrl-click the `[answer]` link under a question (or `herdr api pane.link.activate`) opens the card and answers nothing;
restart herdr and check `herdr plugin log theseus` for the startup hook.

## Unverified / uncertain
- `command = "theseus.answer"` as the value of a `plugin_action` key (I used `<plugin id>.<action id>`, herdr's `qualified_id`).
- Plugin pane commands rely on `$THESEUS_*` set through `plugin.pane.open`'s `env`; `$HERDR_ACTIVE_PANE_ID`, `$HERDR_PLUGIN_CLICKED_URL` exist in herdr's binary strings.
- `agent.focus {target: pane_id}` (falls back to `pane.focus`).
- The link handler's regex is checked only as equal to the code's `LINK_PATTERN` (no regex engine in the CLI); `parse_link` is the strict reader.

## Join notes / docs
- `prefix+h` (`theseus agenda --watch` popup): when `horizon-board` lands, add an action `agenda` and a pane (three lines each in the manifest) and a `KEYS` entry.
- When `herdr-follow` lands, the hook's fallback is already in place.
- Docs for the maintainer: status.md / spec Part III item for pkaq; root AGENTS.md is 20,374 bytes (over its own 20 KB note, not from this change).

## Keel findings expected
None from this change. The gate's keel phase fails on the base itself: a `Keel: budget-raised crates/theseus-sim/src/lifecycle.rs` ack
on 1c2beb2747 whose signature does not verify here (no signer key), plus a renamed store test; both are in `merge-base..HEAD`, not in my diff.
`python3 scripts/keel-guard.py` lists nothing under crates/theseus.

## Gate
`scripts/gate.sh` stopped at door/keel on those base findings (so no later phase ran through it). I ran the later phases by hand:
fmt, shape, clippy, `-p theseus` suite (388), registry: green. `cargo deny check`: advisories FAILED (bans, licenses, sources ok);
I did not investigate (no `cargo deny fetch` was run, it may need the advisory DB). Suite of other crates, cockpit, benches: not run.
