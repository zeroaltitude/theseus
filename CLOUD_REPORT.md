# CLOUD_REPORT: term-render (theseus-rn7s)

Branch `cloud/20261013-term-render`. Commits: 255a633 (the CLI renderer, `--plain`, the watch prompt), 4e357f6 (the TUI pane), then the footer commit (see step 3; its hash is the branch head before this report).

## Step 1: one renderer (style.rs, md.rs + md/{block,inline,table}.rs, code.rs)
**Found.** `ask`, `history` and the pane printed raw text. **Changed (255a633).** `Markdown::new(width, Mode)`, `push`, `finish`, `Markdown::render`; `StyledLine::ansi()/plain()`; OSC 8 links; ten-language lexer (Rust, Python, JS/TS, shell, JSON, TOML, YAML, Go, C/C++, diff; unknown prints plain); wrap by display width (`unicode-width` added to the CLI manifest with a comment; Cargo.lock gains one dependency edge, no package). Prose wraps one column short of the terminal (a line that fills the last column is what F10's S2d reads as a cut word). Code lines wider than the block fold behind `↪`. Tables are aligned in box lines, cut with `…`, never wrapped. Used by `ask`, `ask --no-stream`, `watch`, the turn `confirm` follows, `history` (a reply's text, cut at 300 characters unless `--full`) when stdout is a tty, `NO_COLOR` unset or empty, `TERM` not `dumb`, no `--plain` (new global flag; it changes `--help`, no golden held it). Otherwise the bytes are today's.
**Design choices to hear about.** (a) A newline inside a paragraph is a line break as typed, not CommonMark's space (a model's single-newline lists and addresses keep their shape; the owner may revisit, it is one function, `Draw::paragraph`). (b) Streaming is by complete lines and a paragraph/list/quote/table is drawn when the line that ends it arrives, so a long paragraph appears when its blank line comes. (c) Not read: indented code, setext headings, HTML, footnotes.
**Proof.** `cargo nextest run -p theseus -p theseus-tui`: 463 passed. New: unit tests in style/code/md/*; `tests/markdown.rs` (goldens `tests/golden_md/*.golden.txt`, 9 inputs x 80/100/120 x plain/ANSI; seeded random delta cuts at piece sizes 1/3/17/200, 4 seeds each, byte-identical to the whole; no line wider than the width; no escape in Plain; cost); `tests/markdown_cli.rs` (the binary on a pty and on a pipe: pipe, `NO_COLOR`, `TERM=dumb`, `--plain` print no escape; a pty draws heading, bold, code, list, fenced Rust; `--no-stream`; `history`).
**Planted reverts.** Wrap forced off: 6 unit tests failed (md::tests heading/list/quote, inline wrap tests). Streaming boundary bug (a push's tail drawn as a line): `a_reply_cut_at_seeded_random_points_draws_as_the_whole` failed. Escapes on a pipe (`styled_stdout` ignoring the tty): 3 of 5 `markdown_cli` tests failed. Each file restored and touched.
**FAST.** Timed test `rendering_costs_little_per_hundred_lines`: debug build, 240 lines, median of 5 = 2.6 ms, **1.09 ms per 100 lines** (bound in the test: 2 ms per 100). `theseus --version`: 20 runs in 0.12 s on the debug build (6 ms each; the path is untouched). Binary growth: **not measured** (no release-thin baseline built; disk). Debug `theseus` is 39.2 MB.

## Step 2: the TUI pane (4e357f6)
A run of `Reply` lines is one reply, drawn by the same renderer at the pane's width (`theseus-tui/src/styled.rs`; spans converted to ratatui there; the CLI crate still does not link ratatui). A history reply is a header row and its text. Touched beyond detail.rs: `ui.rs` (the row-drawing loop, about 20 lines), `main.rs` (`mod styled`, `mod tests_markdown`), and two existing tests whose expected rows changed (history reply now two rows; `tests_cite` first-row assertions rewritten, same count). New test `tests_markdown.rs` (streamed reply drawn, bold read from the buffer).

## Step 3: footer wrap and the item-6 fix
**watch --interactive** (in 255a633): a parked turn's footer printed below the `approve?` prompt. Now the prompt is said again below any event line printed while a question waits. Test `herdr.rs::the_prompt_is_the_last_line_after_a_parked_turns_footer`; planted revert (the re-prompt disabled) timed out waiting for the second prompt.
**Footer wrap** (last code commit): F10's S2d at 80x24 was failing on the status line under the reply (cut at column 80), not on the reply. On a terminal it is wrapped at spaces (`print::footer`, `md::wrap_text`; test `a_status_line_breaks_at_spaces_and_reads_no_markup`). This footer commit was checked by fmt, clippy (-p theseus -p theseus-tui, -D warnings), the 463 tests and the F10 run, **not by a second full gate** (see Gate).

## F10 (debug build, so S1b and B1 timing do not count; `--bin-dir target/debug`)
Match 13 of 42 at both sizes (main: 11); beats 5 of 5. Newly passing: **S2a** (both sizes) and **S2d** (both). Everything that passed before still passes. Per step (120x40 = 80x24 unless noted), main -> branch where known: S1 1/4; S2 **2/4** (main 1/4: S2a, S2d added at both, S2b and S2c fail); S3 3/4; S4 0/4; S5 0/3; S6 2/4; S7 1/3; S8 2/3; S9 0/3; S10 1/3; S11 1/4; S12 0/3. (Main's per-step table is in docs/benchmarks/f10; the 11 it passed were S1d, S3a, S3b, S3d, S6c, S6d, S7c, S8a, S8c, S10b, S11a.) Scorecards: /tmp/f10b/scorecard-theseus.md (session-local). No arm edits were needed. No check is thought wrong.

## Live check (maintainer, with release-thin)
`python3 scripts/f10/driver.py theseus --bin-dir target/release-thin --out /tmp/f10` should show 13 of 42 and S1b/B1 timed. On a scratch daemon with the stand-in scripted to answer a heading, table, list and Rust block: `theseus ask Q` in a terminal shows them drawn; `theseus ask Q | cat` and `NO_COLOR=1 theseus ask Q` and `theseus --plain ask Q` show the text as it came; `theseus history` draws the reply. The 80x24 capture from the F10 run (escapes as `\e`):
```
\e[1;4mPort Wenlow tides\e[0m

A \e[1msmall library\e[0m for an invented harbour's tide table. It reads \e[36mHH:MM height\e[39m
lines, finds the high and low waters between their neighbours, and works out
...
\e[1mIts checks\e[0m

\e[90m• \e[39mone \e[3munit test\e[0m for each function, against a single invented day of readings
```
I did not script a table/Rust-block reply on a live daemon; the pty tests and goldens cover those (tests/golden_md/tables, code).

## Gate
First full gate (255a633 + 4e357f6 tree): keel ok, fmt, shape, clippy, cockpit, suite: failures only the known L1 (theseus-sandbox contract/bench, theseusd::sandbox) plus `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer` (fails alone too; discord does not depend on the CLI; main has since fixed it, commits b495389/a6bc001). `cargo deny --offline check` ok. The first attempt was invalid: the disk filled (ENOSPC) and failed ~300 tests; rerun with `CARGO_INCREMENTAL=0`. Benches skipped (`THESEUS_GATE_NO_BENCH=1`). Keel guard: 0 findings. Keel findings expected: none.

## Left / uncertain
Binary growth unmeasured. `watch` also draws replies now (not asked; one `Printer::new` condition). Docs to change at review: spec Part III item, status.md, `docs/design` note on the pane. A sibling (`chat`) can call `Markdown::new(width, Mode::Ansi)` from `reply_lines`. `Cargo.lock` is one edge, `theseus -> unicode-width`.
