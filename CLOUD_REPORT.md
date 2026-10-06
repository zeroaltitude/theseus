# Cloud report: scrub-encodings (theseus-nlvx, theseus-cjyt)

Branch `cloud/20261006-scrub-encodings`, from main at d279767f (store format 23, unchanged: nothing stored changed).
Started 19:44 UTC, finished 21:25 UTC on 2026-10-06. Setup: `cargo deny fetch` succeeded, so the gate's deny phase ran.

| Step | Commit | What |
|---|---|---|
| 1 | 6ef9243e | a second decode, composed maps (nlvx) |
| 2 | 196c8656 | YAML's and Python repr's escapes (nlvx) |
| 3 | 28a1e11f | base64 in the decoded text (cjyt) |
| 4 | 297ea919 | its cost: the decoded base64 pass gated, three new `scrub_cost` outputs, the AGENTS.md Secrets line |

Files: `crates/theseus-core/src/scrub.rs`, `scrub/escaped.rs`, `scrub/tests_escaped.rs` (only `scrub_cost`), the new
`scrub/tests_nested.rs`, `tests_outside_text.rs`, and theseus-core's `AGENTS.md`. `Scrubber::scrub`'s signature is
unchanged, so none of its callers changed. The judge's own scrub (`judge/mod.rs`'s `ScrubWith(Arc<Scrubber>)`) wraps
the same `Scrubber`, so it gets these forms with no edit; I didn't touch it.

## What the code says (checked against the brief)

All of the brief's statements hold:
- `escaped::spans` returns at once when the output has no backslash. `decode` keeps a backslash that starts no
  escape, and keeps a lone surrogate.
- The verbatim pass runs first, longest value first. The escaped spans join base64's and percent's in one `splice`.
- Values and the text are whole characters. Every new escape still decodes to exactly one character (a bytes repr's
  `\xc3\xa9` is one 8-byte escape for `é`), so a match still never ends inside an escape's character.

Three details the brief doesn't state:
- **`splice` drops more than overlaps.** A span is also dropped when it starts where a kept span starts and is
  shorter: spans are sorted by start, longest first. Duplicates from two passes collapse this way, so the count stays
  right.
- **`with_values` doesn't filter.** Values from the board are trimmed, and any shorter than `MIN_VALUE` are skipped.
  `with_values` (tests only) does neither.
- **`base64_runs` is a large share of every scrub.** It allocates a `Vec` for every word it reads, which is part of
  the ~740 µs a 64 KB plain-text output costs even on main. This is not new, but it's a likely cheap follow-up (see
  step 4).

## Step 1: a second decode (nlvx), 6ef9243e

**Found.** One decode leaves `\"` and `\\` in JSON inside a JSON string, so a value holding a quote, a backslash, a
control character or (under an ASCII-only encoder) any non-ASCII character got through. On main, every test of the
new `tests_nested.rs` that prints JSON twice fails.

**Changed.** `escaped::spans` now decodes up to `LEVELS = 2` times:
- It decodes a second time only when the first decoded text still holds a backslash, and stops when a decode finds
  no escape.
- A match at level k maps back through each level's escapes, innermost first (`start` and `end` composed).
- Every value is matched at every level, so a value holding a literal backslash-n still matches at level 1, where
  level 2 would read a line break. A test holds this.
- The second buffer is made only for outputs whose first decode leaves a backslash.

**A third level** is a change of the constant, but I don't think it's worth it now. The forms the brief names are two
levels deep. Three takes JSON inside JSON inside a JSON string (for example a CloudWatch Logs `message` holding a
line that itself holds a serialized document). That's possible but rare, and every Windows-path output already pays
for the second decode. If an operator meets one, raise `LEVELS`.

**Tests** (`scrub/tests_nested.rs`):
- `a_value_in_json_inside_a_json_string_is_withheld_compact_and_pretty`: each of tests_escaped.rs's six values in a
  serialized object inside a JSON object, compact and pretty, and ASCII-only at both levels (a `json.dumps` of a
  `json.dumps`, written by hand).
- `a_secret_in_kubectls_last_applied_annotation_is_withheld`: a Secret whose `stringData` is in the
  `last-applied-configuration` annotation, with its `data` as base64 beside it. The count is 2.
- `a_value_holding_a_backslash_matches_at_its_own_level`: the literal-backslash value, once and twice escaped, and
  nested escapes that hold no value left as they were.
- `tests_outside_text`'s `a_value_never_comes_through` has a new way: the value twice escaped (`\\\/`, `\\u002B`).

**Plant** (`LEVELS = 1`): four tests fail.
- `a_secret_in_kubectls_last_applied_annotation_is_withheld`, `a_value_in_json_inside_a_json_string_is_…` and
  `a_value_holding_a_backslash_matches_at_its_own_level` each fail on `assertion left == right` with a count of 0.
- `tests_outside_text::a_value_never_comes_through` fails too.

Restored, `touch`ed, and `git status` was clean.

## Step 2: YAML's and Python repr's escapes (nlvx), 196c8656

**Changed.** `escape_at` gains these arms:
- YAML's `\0 \a \e \v \N \_ \L \P`, plus `\ ` and `\<tab>`, which YAML also has and cost one line each.
- `\UNNNNNNNN` (YAML and repr), `\'` (repr), and `\xNN`.
- `hex4` became `hex(b, at, n)`. Every read is a bounds-checked `get`, so a cut `\x`, `\xc3\xa` or `\U0001F6` decodes
  nothing and can't panic. A test covers this.

**How `\xNN` is read.** A `\xNN` whose byte is a UTF-8 lead byte, followed by the right number of `\xNN` continuation
escapes that together form a valid character, is read as that character (a bytes repr's `\xc3\xa9` is `é`). Any other
`\xNN` is U+00NN, as YAML and a string repr mean it (`\xE9` is `é`).

The cost of this choice: a YAML or string-repr value holding the two characters `Ã©`, written `\xC3\xA9`, reads as
`é`, so that value is missed. That would take a mojibake pair inside a secret, which is far rarer than a bytes repr of
a non-ASCII secret. Decoding both readings would need a third buffer. I didn't build it, and the owner may want to
weigh in on that choice.

**Tests:**
- `a_value_in_yamls_escapes_is_withheld`: `\xE9`/`\xe9`, `\U0001F600`/`\U0001f600`, ESC as `\e` and as `\x1B`,
  `\N \_ \L`, and `\"` and `\x09`, each inside a YAML mapping.
- `a_value_in_pythons_repr_is_withheld`: `\'` in `repr(v)` and in `repr(v.encode())`, `\x00` and `\x7f`, a bytes repr
  of the accented value (both hex cases) and of the astral one (4 bytes), a backslash in a bytes repr, and a `\xNN`
  run that is no character's UTF-8 (`\xc3\x28` read as `Ã(`). Cut escapes are left unchanged.
- `SCRUB_PIECES` gains `\\\"`, `\x`, `\xc3`, `\xf0\x9f`, `\U`, `\U0001F6`, `\e`, `\'` and `\N`.
- `a_value_never_comes_through` has a new way: the value as `\x2f`/`\x2B`.

**Plants:**
- YAML arms off (the 11 arms `\0`…`\U` removed): `a_value_in_yamls_escapes_is_withheld` fails with
  `assertion left == right failed: Inv\U0001F600nted-Astral6 … 0)`.
- Repr arms off (`\'` removed, and `\x` runs read byte by byte): `a_value_in_pythons_repr_is_withheld` fails with
  `'Inv\'ent"ed-Both12' … 0)`.
- The UTF-8 run alone off: the same test fails with `b'Inv\xc3\xa9nt\xc3\xa9d-Accent5' … 0)`.

Each plant was restored and `touch`ed.

**Left open: PyYAML's line folding.** In double-quoted style, PyYAML folds a long string at width 80 with a `\`, a
line break and indentation (an escaped line break, which decodes to nothing). A value split by such a fold still gets
through. My `Escape` maps one escape to one character; an escape that decodes to nothing needs `start` and `end` to
handle a zero-width decoded span. That's a follow-up issue if wanted.

## Step 3: base64 in the decoded text (cjyt), 28a1e11f

**Found.** `base64_runs` ended a run at the backslash of `\n` (or `\/`), and the `n` (or `/`) began the next run. So
a value's base64 that crossed a wrap got through. And a match on a line after an escaped break left a stray
backslash in front of its marker, which broke the JSON.

**Changed:**
- The base64 match moved into `base64_spans(text, needles, keep)`, which the raw pass and every decoded level share.
- `escaped::spans` runs it over each level's decoded text, keeping only runs with one of that level's escapes inside.
  The level before already read every other run as it is.
- It maps each match's lines back through the same composed maps, so the span covers the lines the match touches,
  with the escapes between them.
- An escape's letter (`n` of `\n`, `/` of `\/`, after an odd count of backslashes) no longer starts a raw run. A match
  wholly on a line after `\n` then has the same span in both passes, and no stray backslash is left.

**Tests:**
- `a_values_base64_wrapped_by_escaped_line_breaks_is_withheld`: two values, each in the first line, across the first
  wrap, and wholly in the third line, at all three offsets each (3, 4, 5 / 30, 31, 32 / 95, 96, 97 bytes before).
  Each is tried with `\n`, `\r\n`, and `\n` plus PHP's `\/`, and the expected output is exact JSON.
- `a_values_base64_in_encodebytes_or_a_pem_in_json_is_withheld`: wrapping at 76 (encodebytes) and at 64 (a PEM in a
  JSON string).
- `a_value_never_comes_through` has a new way: wrapped base64 in a JSON string, with neither touched line allowed
  through.

**Plants:**
- The decoded pass off (`inside` false): both new tests fail with a count of 0, and the property test fails with
  `minimal failing input: before = "", after = "", how = 7`.
- The escape-letter skip off: both tests fail, one with the stray backslash
  (`Error("invalid escape", line: 1, column: 194)` on re-parsing the output).
- After step 4, the `joins` gate forced off: both tests and the property test fail.

All restored and `touch`ed.

## Step 4: the cost, 297ea919

All numbers are from `cargo test --profile release-thin -p theseus-core --lib scrub_cost -- --ignored --nocapture`,
on this 4-core VM with 64 KB outputs and ten board values. "main" is d279767f with the new `scrub_cost` copied in,
built in a worktree.

| output | main (run 1 / 2) | branch, ungated (step 3) | branch, final (run 1 / 2) |
|---|---|---|---|
| plain text, no backslash | 739 / 748 µs | 760 µs | 765 / 728 µs |
| pretty JSON, no backslash | 1,655 / 1,645 µs | 1,566 µs | 1,656 / 1,637 µs |
| ~7,800 escapes | 839 / 823 µs | 1,337 µs | 909 / 846 µs |
| a few escapes (new) | 748 / 752 µs | 1,108 µs | 794 / 772 µs |
| twice-escaped JSON (new) | 800 / 805 µs | 1,369 µs | 943 / 815 µs |
| base64 with `\n` every 60 in JSON (new, 67.7 KB) | 4,588 / 4,658 µs | 4,909 µs | 4,873 / 4,641 µs |

- **The ungated pass was too costly.** It scanned every decoded text with `base64_runs`: about +360 µs on a few
  escapes and +500 µs on many.
- **The gate.** The final code runs the decoded base64 pass only when one of the level's escapes stands for a line
  break or a base64 character between base64 characters (`escaped::joins`).
- **After the gate.** The second decode plus the gate costs between noise and about +140 µs on the twice-escaped
  output (run to run, the noise here is about ±70 µs). The wrapped-base64 output costs at most about +6%, its run
  matched once more after joining.
- **No backslash is still one scan for it:** the plain and pretty rows are unchanged.

**A follow-up worth having.** Most of the 4.6 ms on the base64 output, and a large part of every row, is
`base64_runs`' per-word `Vec` and each run's joined `String`, already on main. Reusing one buffer, or taking a word
under 10 characters without allocating, would likely cut every scrub. I didn't touch it here; it's outside this step.

## Proof, offline

- **Every test across the workspace whose name says scrub, redact or secret:**
  `TZ=America/Phoenix cargo nextest run --workspace -E 'test(/scrub|redact|secret/)'` gave 68 run, 68 passed.
- **The scrubber's property tests, 20 times, fresh seeds:** `cargo nextest run -p theseus-core --lib tests_outside_text`
  passed 20 of 20 after step 2, again after step 3, and again after step 4 (8 tests each run).
- **theseus-core's suite whole** ran in each gate: every theseus-core test passed in all four.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` ran before each of the four commits:
- fmt, shape, features, clippy, cockpit, test build and the reader rule passed.
- The suite failed only on the known 33 L1 tests: theseus-sandbox's contract tests, its bench's `spawn_100`, and
  theseusd's `sandbox` tests (root with no job cgroup, theseus-pv6i). Last run: 2,996 run, 2,963 passed, 33 failed,
  24 skipped.
- I then checked the phases after the suite myself: `cockpit/src/protocol.gen` was unchanged, and no crate was
  compiled under the lock (no `NOTE`). The benches are skipped under `THESEUS_GATE_NO_BENCH`.
- No timing test from the known list failed, and no negative assertion failed.
- Clippy failed twice before green (`needless_question_mark`, `needless_range_loop`, then `precedence` in
  `scrub_cost`). Each was fixed before its commit.

**Disk:** the debug incremental cache reached 16 GB and left 1.9 GB of this VM's allowance, so I deleted
`target/debug/incremental` (inside the repository) before the release builds.

## The live check (the maintainer's)

1. **Write the secret file** (an invented value with both quotes, a backslash and an accented letter):
   ```
   D=$(mktemp -d)
   printf '%s' "Inv'ent\"ed\\Prébe-77" > $D/probe.txt
   ```
   Use `printf` with a literal `é` if your shell doesn't expand `\u`.
2. **Start a scratch daemon** with Discord and the web off, and in its config:
   ```
   [secrets]
   probe = "file:<D>/probe.txt"
   ```
   `theseus --socket <its socket> health` should show `probe` resolved.
3. **Have the stand-in model** (`theseus-sim fake-model --rules`) call `proc_run` with each of these, where
   `v = open("<D>/probe.txt").read()`:
   - `python3 -c 'import json;v=…;print(json.dumps({"doc": json.dumps({"v": v})}))'`
   - `python3 -c 'import yaml;v=…;print(yaml.dump({"v": v}))'` if PyYAML is installed (it writes
     `"Inv'ent\"ed\\Pr\xE9be-77"`), else print that literal.
   - `python3 -c 'v=…;print(repr(v));print(repr(v.encode()))'`
   - `python3 -c 'import json,base64;v=…;print(json.dumps({"content": base64.encodebytes(("x"*50+v).encode()).decode()}))'`
4. **Check each turn:** `theseus --socket <its socket> history <session>` should show `[redacted:probe]` and no form
   of the value, whether plain, escaped, `\xE9`, `\xc3\xa9`, or any base64 line holding its bytes. On main, each of
   the four turns shows the value.

## Left open, and choices the owner should hear about

- **`\xNN` ambiguity:** a UTF-8-valid run is read as UTF-8, so a YAML or string-repr `Ã©` is missed (step 2).
- **PyYAML's folded long double-quoted strings** (an escaped line break that decodes to nothing) are not decoded
  (step 2).
- **No third decode level** (step 1).
- **`base64_runs` allocates per word,** a likely cheap speed-up for every scrub (step 4).
- **Docs:** the spec's §3.9 (secrets) and `docs/status.md` could name the new forms. AGENTS.md already does.
