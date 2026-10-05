# CLOUD_REPORT: cloud/20261005-cli-tests

## 1. theseus-xiaz: health's 1-hour cache writes
- Found: as briefed. `health_lines` printing the bare `cache_creation_input_tokens` passed every test.
- Changed: commit "cli: health's golden carries 1-hour cache writes" (test and golden only). `health_says_where_the_push_stands` gives `usage_total` 300 one-hour writes; health_push.txt's diff is the tokens line alone, now `cache-write 1200 (1h 300)`. The other health goldens still read `cache-write 1200` (the zero case).
- Proof: planted `h.usage_total.cache_creation_input_tokens` in render.rs line 1766: `health_says_where_the_push_stands` fails (9 of 10 health tests pass). Restored, touched, `git status` clean.
- Live check: stand-in Messages API whose usage has `cache_creation_input_tokens` and `cache_creation.ephemeral_1h_input_tokens`, one turn, then `theseus health`: the tokens line ends `cache-write N (1h M)`.

## 2. theseus-w38g: `judge prove`'s bytes
- Found: as briefed; no golden ran the command.
- Changed: commit "cli: goldens for theseus judge prove's bytes". tests/golden.rs, in a block before `health_prints_every_line` (not at the file's end): `judge_prove_prints_the_markdown_as_it_is` (golden `judge_prove.txt` plus byte-for-byte assert of stdout against `markdown`), `judge_prove_records_to_stdout_are_the_records` (golden `judge_prove_records.txt` plus assert against `records`), `judge_prove_records_to_a_file_are_the_records` (file equals `records`, stdout the Markdown). Fixtures are invented; stderr is the read lines.
- Proof: `println!` for the Markdown print fails the markdown test and the file test; `println!` for `--records -` fails the records test. Each restored, touched, tree clean. Passes with THESEUS_GOLDEN unset.
- Live check: `theseus judge prove > a.md`; `theseus judge prove --records r.jsonl > b.md`; `theseus-judge prove r.jsonl --markdown - > c.md`; `cmp a.md c.md` and `cmp b.md c.md` silent; `theseus judge prove --records - | cmp - r.jsonl` silent. With no finished tasks the result is "insufficient".

## 3. theseus-1n2l: watch's last line
- Found: confirmed; the loop ended with no settle.
- Changed: commit "cli: watch ends the reply's open line at EOF": one `printer.settle()` after the loop in `cmd::watch`. Diffs, each one newline at stdout's end:
  - watch_lost: ` there.--- stderr` becomes ` there.` / `--- stderr`
  - watch_shapes: `Low water at 14:10.--- stderr` becomes `Low water at 14:10.` / `--- stderr`
- `watch --all` and `--json` print every line with `println!`, so they already end their last line; unchanged. (`--json` never touches the printer.)
- Proof: settle removed: `watch_says_what_it_lost_and_where_to_read_it` and `ask_watch_and_no_stream_print_the_printers_other_shapes` fail. Restored, touched, tree clean.
- Live check: `theseus watch <session> > w.txt` while a reply streams, stop the daemon; `tail -c 1 w.txt | od -c` shows `\n`.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (run on the first commit and the last): fmt, shape, features, clippy, cockpit, reader rule pass; the suite fails only on the known L1 tests (theseus-sandbox contract tests, theseusd's sandbox tests). Run 1 also had `theseus-core tests_rerank::a_reranked_turn_keeps_its_frame_budget` fail under load; it passed alone and is not touched by this branch. The later machine_checks phases (protocol types, benches) were not run; nothing here changes protocol types. `cargo nextest run -p theseus`: 157 passed, goldens written once then compared clean. git diff on tests/golden/ shows health_push, watch_lost, watch_shapes and the two new judge_prove goldens only. Gate was not run per commit but on the first and last (the middle commit is tests-only; fmt and clippy were run for it).

No difference from the brief besides the judge prove tests landing before `health_prints_every_line`. No docs to change.
