# CLOUD_REPORT: herdr-task-return (theseus-m638c)

## Step: prefix+m, prefix+i, the pane header (commit a77f08da)
**Found.** herdr already had the follower, a manifest of actions/popups and a key table tied by one test. `up`'s state ignores any pane whose kind is not task/conversation, so a `main` kind token is safe beside the board tab.

**Changed.**
- `herdr_main.rs` (new): `theseus herdr main` lists panes, focuses the one with `kind=main`, else `layout.apply`s a `main` tab (argv `theseus --socket S chat`) once, then `pane.focus`. Talks to herdr alone (`HerdrCmd::Main`, `is_own`). `task_header` reads the keys from `herdr_setup::KEYS`.
- `herdr_keys.rs`: popup `inspect` (POPUPS 5→6); `inspect_for` turns the focused pane's task token into the session (refuses non-task panes); `herdr_socket_or_default` made pub.
- `herdr_setup.rs`: KEYS 5→7 (`prefix+m` theseus.main, `prefix+i` theseus.inspect).
- Manifest: actions `main`, `inspect`; popup pane `inspect` runs `theseus chat --task "$THESEUS_SESSION_ARG" --inspect; sleep 1`.
- `interactive.rs`: a task's herdr pane prints `── ⚙ task a1b2c3 of <parent> · prefix+m main · prefix+i inspect ──` to stderr at start.
- `herdr_up.rs`/`wire.rs`: `wire` made pub(crate), `MAIN_KIND`. AGENTS.md of crates/theseus updated.
- I did NOT add `joinable`/`task_redirect_line` (no use in this row).

**Proved.**
- `cargo test -p theseus --test herdr_main`: 2 passed. Whole `cargo test -p theseus`: all pass (one run showed `cmd::tests::external_text_lines_and_the_trust_target` failing once; passes alone and on rerun: an env race, not mine). `cargo clippy -p theseus --all-targets` clean; `cargo fmt` applied.
- Planted revert 1: always `layout.apply` in `herdr_main::run` → `prefix_m_makes_the_main_tab_once_and_then_focuses_it` FAILED (3 layout.apply). Restored, touched.
- Planted revert 2: `inspect_for` ignores kind → `prefix_i_opens_the_inspect_popup...` FAILED (conversation pane accepted). Restored, touched.
- Popup argv/manifest tied by `herdr_setup::tests::the_inspect_popup_and_the_main_key_run_their_commands`.

**Live check (needs herdr).** `theseus herdr plugin` / link, add the two keys from `theseus herdr setup`; in a task pane press prefix+m twice: one `main` tab, focused both times; prefix+i: popup runs `theseus chat --task <sid> --inspect` (works once row 2's flags merge; until then the popup shows clap's usage).

**Left / uncertain.** The chat's `--task/--inspect` flags are row 2's. "Home conversation" is bare `theseus chat` (the chat's own thread choice); the exe path is baked into the tab's argv like the other tabs. The header says the keys by their default names, not the owner's rebinding. herdr's `pane.focus` on a pane in another tab is assumed to switch tabs (as `herdr next` assumes).

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` FAILED in its first phase (keel/door): the keel guard rejects pre-existing commits on this base (signatures unverifiable on this VM, `%G? is E`) and flags lifecycle.rs/keel-guard.py erosions that are not in my diff. Nothing after it ran, so I ran fmt, clippy and the crate's tests myself (above). Full workspace suite, deny and cockpit phases were not run.
