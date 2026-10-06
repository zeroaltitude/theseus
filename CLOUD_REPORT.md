# Cloud report: the scrubber withholds a secret printed JSON-escaped (theseus-ubp7)

Branch `cloud/20261005-scrub-escaped`, cloned from main at 4a449460 (store format 22, unchanged: nothing stored
changes). Started 03:07 UTC; report at 04:25 UTC.

## Step 1: the escaped matcher (e915c13)

**Found.** `Scrubber::scrub` matched each board value verbatim, in base64, and percent-encoded; a value holding a
quote, a backslash, a control character, or a non-ASCII character, printed by a JSON encoder, was not matched. What
the code says matches the brief, with one difference: the brief says the new work "can skip every value" with no
quote, backslash, control or non-ASCII character. It can't safely: a value with `/` (PHP's `\/`), `<`, `>`, `&` (Go's
`<`…) changes too, and any character may be written as a `\u` escape. So the matcher searches every value in
the decoded text, and skips only outputs with no backslash (and outputs with a backslash but no valid escape).

**Changed.** `crates/theseus-core/src/scrub/escaped.rs` (new, 130 lines), one call in `scrub.rs`'s `encoded`, after
the base64 and percent spans and into the same `splice`, so it runs before the shapes and its matches count in what
`scrub` returns. The theseus-core AGENTS.md "Secrets" invariant names it.

**The choice: decode, not fixed forms.** On an output holding a backslash, the text is decoded once, left to right,
as JSON decodes it: `\"`, `\\`, `\/`, `\b`, `\f`, `\n`, `\r`, `\t`, `\u` with four hex digits in either case, and a
high surrogate followed by a low one as one character. Each escape's place in the text and in the decoded text is
kept (a short list, one entry per escape, not a map per byte). Each value is searched in the decoded text
(`match_indices`), and a match is mapped back: a start inside an escape to the escape's start, an end to the escape's
end, so the span replaced is the whole escaped text and no half of an escape remains. A backslash that starts no
escape (`\x`, `\u12`, a trailing one) and a lone surrogate are kept as they are.
- What decoding misses: escapes that are not JSON's (`\'`, `\xNN`, `\0`, `\UXXXXXXXX`, `\u{…}`, shell's `\ `), and a
  value escaped twice (JSON inside a JSON string), since it decodes one level. See step 3.
- What a fixed set of forms per value would miss: every mix an encoder makes of escaping some characters and not
  others (Go's `<>&`, PHP's `/`, Python's ASCII-only non-ASCII, a `\u` for any character), and hex case per digit.
  The count of forms grows with the value's escapable characters; decoding has one form.
- One overlap rule is inherited from `splice`: where an escaped match overlaps a base64 or percent match, the one
  that starts first is kept. The verbatim pass has already taken every match with no escape in it.

**Proved.**
- New tests, `scrub/tests_escaped.rs` (a file of its own: scrub.rs would have passed 1,000 lines):
  `a_value_serde_json_prints_is_withheld_compact_and_pretty` (six values: a quote, a backslash, a tab, a newline, an
  accented letter, U+1F600; each inside an object by `serde_json::to_string` and `to_string_pretty`; the output equals
  the same object printed with `[redacted:<name>]` in its place, count 1, and parses back);
  `a_value_python_prints_ascii_only_is_withheld_in_either_case_of_hex` (Python's `json.dumps` literals, lowercase and
  uppercase hex, the surrogate pair both cases, and characters `\u`-escaped that need not be);
  `a_value_with_an_escaped_slash_or_gos_escapes_is_withheld` (`\/`, Go's `<>&`);
  `every_occurrence_is_counted_and_nothing_else_changes` (two escaped occurrences and a verbatim one count 3, a
  Windows path, a lone surrogate, `\uzzzz` and other escapes left byte for byte; outputs with backslashes and no
  value unchanged, count 0).
- The outside-text property test (`tests_outside_text::a_value_never_comes_through`) plants the value JSON-escaped
  too (`\/` and `+`), and its pieces now include `\`, `\"`, `\u00`, `\ud83d`, so the no-panic test also sees
  broken escapes.
- **Planted revert** (the pass given no values: `escaped::spans(out, &values[..0])`): 5 failed, 8 passed:
  `a_value_serde_json_prints…` (`left: ("{\"before\":…\"secret\":\"Inv\\\"ented-Quote1\",…}", 0)`, right the
  redacted text and 1); `a_value_with_an_escaped_slash…` (`"token":"Inv\\/ented\\/Slash7"`, 0);
  `every_occurrence…` (`left: 1, right: 3`); `a_value_python_prints…` (`"secret": "Inv\\\"ented-Quote1"`, 0);
  `tests_outside_text::a_value_never_comes_through` (`minimal failing input: before = "", after = "", how = 4`).
  The existing eight scrub tests passed. Restored, touched, `git status` clean but for the intended files; 20 of 20
  pass.
- Every test whose name says scrub, redact, or secret, across the workspace
  (`cargo nextest run --workspace -E 'test(/scrub|redact|secret/)'`): 58 of 58 passed.
- The judge's own scrub (judge/mod.rs's `ScrubWith`) and theseus-discord wrap the same `Scrubber`, so they gain the
  matcher with no edit. toolrun.rs, secrets.rs and judge/mod.rs are untouched.

## Step 2: its cost (94ed9fc)

`scrub::tests_escaped::scrub_cost`, `#[ignore]`d, run by hand:
`cargo test --profile release-thin -p theseus-core --lib scrub_cost -- --ignored --nocapture`. Ten board values
(seven plain 30-odd-character tokens; a quote's, a backslash's, an accent's), 2,000 calls an output, per-call time,
and beside it a bare `contains('\\')` scan. Before is the planted revert above (the pass off), three runs each, this
4-core VM:

| output (64 KB)                    | before (µs a call)  | after (µs a call)   | one scan for `\` |
|-----------------------------------|---------------------|---------------------|------------------|
| plain text                        | 728, 697, 729       | 740, 724, 723       | about 4 µs       |
| pretty JSON, no secret, no `\`    | 1,720, 1,652, 1,717 | 1,736, 1,741, 1,666 | about 4 µs       |
| JSON with backslashes throughout  | 610, 635, 632       | 792, 818, 817       | (first at byte 20)|

With no backslash the pass costs its one scan (about 4 µs, inside the runs' noise). With escapes throughout (about
7,800 in 64 KB) it costs about 185 µs a call more (+30%): the decode into a second buffer and ten searches of it.

An aside for the owner, not this step's: `scrub` costs 0.7 ms on 64 KB of plain text and 1.7 ms on pretty JSON
before this change, far more than a scan. The base64 run finder and the per-value percent scan
(`percent_spans` walks every byte for every value when the output holds a `%`) look like where it goes; I did not
profile it.

## Step 3: other encodings, a report (and one test)

One test only: `rusts_debug_form_is_caught_where_it_shares_jsons_escapes` (c1394b3), since the matcher catches
Rust's `{:?}` for free. Planted revert: it fails (`left: ("Config { token: \"Inv\\\"ented-Quote1\", port: 8080 }", 0)`).

| form | how likely in a tool's output here | what matching it would cost | add it? |
|---|---|---|---|
| Rust `{:?}` | likely (a Rust tool's debug log, `dbg!`, an error) | free: `\"`, `\\`, `\n`, `\r`, `\t` are JSON's, and printable non-ASCII prints as is. Missed: `\0` and `\u{…}` for other control characters | caught now (the test above) |
| JSON in a JSON string (escaped twice) | common: `kubectl get -o json`'s `last-applied-configuration`, CloudWatch and CloudTrail events, an API Gateway event's `body`, `docker inspect` labels | small: decode the decoded text again when it still holds an escape, and compose the two maps (the same code twice); one more buffer only for such outputs | **yes, next**: the most likely gap |
| YAML double quotes | moderate (`kubectl -o yaml`, PyYAML) | JSON's escapes plus `\xNN`, `\UXXXXXXXX`, `\0`, `\e`, `\N`, `\_`: a few arms in `escape_at`. PyYAML's default writes é as `\xE9` and U+1F600 as `\U0001F600`, which are missed today | yes, with `\xNN` and `\U`; cheap |
| Python `repr` | moderate (a script's print of a dict or an exception) | `\'` (used when a value holds both quotes), `\xNN` (controls; and every non-ASCII byte in a `bytes` repr), `\uXXXX`/`\UXXXXXXXX` for non-printables. A value with only `"`, a backslash, tab or newline is caught today (`\\`, `\t`, `\n` are JSON's) | yes, together with YAML's arms (`\'`, `\x`, `\U`) |
| Shell quoting (`'\''`, `printf %q`, `set -x`, `${v@Q}`) | moderate (`set -x` traces in CI scripts) | a second, shell view: `\X` for any X and `'\''` as `'`. `set -x` prints a value verbatim unless it holds `'` or whitespace, which secrets rarely do | later, low priority |
| HTML entities | `http.fetch` already decodes entities in its HTML reader (`web/html.rs`), so its text is matched verbatim. Raw HTML through `proc.run curl` is not | an entity decoder view like this one | no, for now |
| A form body's `+` for a space | low (a value with a space: passphrases) | one arm in `percent_spans`: `+` for a space | yes, trivial, if values with spaces are expected |
| Hex dumps | low (`xxd`, `od`, `hexdump -C` of a credentials file) | contiguous hex in both cases is a needle per value, as base64's; a dump's layout (offsets, columns, the ASCII gutter) is a parser of its own | contiguous hex maybe; dumps no |

## The live check (the maintainer's)

```sh
d=$(mktemp -d)
python3 -c 'import sys; open(sys.argv[1],"w").write("Inv\"ent\\ed-pröbe-9")' "$d/probe.txt"
# config: Discord and web off, [secrets] probe = "file:<d>/probe.txt", the stand-in model at the fake's port
theseusd --config "$d/theseus.toml" --socket "$d/sock" --state-dir "$d/state" &
theseus --socket "$d/sock" health        # the secret `probe` resolved
```

The value is `Inv"ent\ed-pröbe-9` (a quote, a backslash, an accented letter; invented). Give
`theseus-sim fake-model --rules` three rules, each calling `proc_run` with one of:

```
python3 -c 'import json; print(json.dumps({"v": open("<d>/probe.txt").read()}))'
python3 -c 'import json; print(json.dumps({"v": open("<d>/probe.txt").read()}, indent=2))'
python3 -c 'import json; print(json.dumps({"v": open("<d>/probe.txt").read()}, ensure_ascii=False))'
```

For each turn, `theseus --socket "$d/sock" history <session>` shows `{"v": "[redacted:probe]"}` (the second over
three lines) in the tool's result, and `grep -c` of `Inv\"ent\\ed`, `pr\u00f6be`, `Inv"ent\ed` and `pröbe` over the
whole history is 0. On main all three show the value escaped (`Inv\"ent\\ed-pr\u00f6be-9`, and with
`ensure_ascii=False` `Inv\"ent\\ed-pröbe-9`). Stop it with `theseus --socket "$d/sock" shutdown`.

## Left, and uncertain

- JSON escaped twice is not caught (step 3: recommended next, small).
- The pass runs per output, so a value split across two outputs (or a cut result's edge through an escape) is not
  matched; the verbatim pass has the same limit.
- base64 or percent encoding *inside* an escaped string with `\n` line breaks (a JSON string holding a wrapped
  base64 blob as `\n`) is not joined into a run; base64 runs are found on the raw text only.
- Docs the maintainer may want to touch: the spec's §3.9 list of scrubbed forms, and docs/status.md, gain "JSON-
  escaped". The theseus-core AGENTS.md line is updated here.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`, before each commit: fmt, shape, features, clippy,
cockpit, the test build, the reader rule pass; the suite's only failures are the 33 known L1 tests of this root VM
(theseus-sandbox's 21 contract tests and `spawn_100`, theseusd's `tests/sandbox.rs`, theseus-pv6i). The phases after
the suite, run by hand: protocol types unchanged; `theseus-sim bench turn --check --runs 5 --burst 0`: 5 and 9
frames, both at budget. The lifecycle and jobs benches are skipped by `THESEUS_GATE_NO_BENCH`. `cargo deny fetch`
succeeded at setup.

One run of the gate (before the second commit) failed 102 tests across jobs, continuations and tasks: the VM's disk
allowance was spent (608 MB left) after the release-thin build for step 2. I removed `target/release-thin` and
`target/debug/incremental` (caches), and the rerun failed only the 33 L1 tests. No test was retried away.
