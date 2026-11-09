"""Tests for scripts/line-pressure.py (theseus-4vsrz, 1.0.5 B7).

Run: python3 -m unittest -q scripts/tests/test_line_pressure.py

CALIBRATION is the script's count against clippy's own, taken on the base commit by lowering
`too-many-lines-threshold` in a scratch copy of clippy.toml (never committed) and reading the numbers clippy
printed ("this function has too many lines (N/T)"). The real functions are read from the working tree, so a
function that has since changed makes its row fail: re-measure it and update the row, do not loosen the test.
"""
import importlib.util
import os
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
spec = importlib.util.spec_from_file_location("line_pressure", os.path.join(HERE, "..", "line-pressure.py"))
lp = importlib.util.module_from_spec(spec)
spec.loader.exec_module(lp)

# (path, function name, clippy's count)
CALIBRATION = [
    ("crates/theseus-ontology/src/compose.rs", "compose", 95),
    ("crates/theseus-ontology/src/kind.rs", "interpreted_seeds", 96),
    ("crates/theseus-ontology/src/ontology.rs", "load", 96),
    ("crates/theseus-ontology/src/person.rs", "merge", 98),
    ("crates/theseus-store/src/store.rs", "commit", 95),
    ("crates/theseus-follow/src/lib.rs", "read_into", 96),
    # Plain `async fn`s: clippy counts 2 more than the body alone (the script adds 2). Measured at base 85697868
    # with `too-many-lines-threshold = 85`: sync 207 of 207 exact, `#[tokio::test]` 72 of 72, plain async 85 of 85
    # exactly 2 short before the rule. Re-measure a moved row; never loosen it.
    ("crates/theseus-core/src/card/run.rs", "card_run", 89),
    ("crates/theseus-core/src/rpc/server.rs", "dispatch", 92),
    ("crates/theseus-core/src/turn/compile_step.rs", "compile_step", 88),
]


def long_fn(name, code_lines, attrs=""):
    body = "".join(f"    let v{i} = {i};\n" for i in range(code_lines))
    return f"{attrs}fn {name}() {{\n{body}}}\n"


class Counter(unittest.TestCase):
    def test_blank_and_comment_lines_are_free_and_a_string_counts_whole(self):
        body = (
            '\n  let a = 1;\n\n  // a note\n  /* block\n     comment */\n'
            '  let s = "one\ntwo\n// not a comment line\nthree";\n  let b = 2; // trailing\n'
        )
        # code: `let a`, `let s = "one`, `two`, `three";`, `let b`; the `//` line inside the string is read as a
        # comment, as clippy's textual scan reads it.
        self.assertEqual(lp.clippy_lines(body), 5)

    def test_braces_in_strings_and_comments_do_not_move_the_end_of_a_body(self):
        src = 'fn a() {\n let s = "}";\n // }\n let c = \'}\';\n let r = r#"}"#;\n}\nfn b() {\n let x = 1;\n}\n'
        got = {n: lp.clippy_lines(b) for n, _, b in lp.functions(src)}
        self.assertEqual(got, {"a": 3, "b": 1})


class Walker(unittest.TestCase):
    def test_a_planted_long_function_is_named_with_its_count_and_the_scan_is_not_empty(self):
        with tempfile.TemporaryDirectory() as d:
            with open(os.path.join(d, "planted.rs"), "w") as f:
                f.write(long_fn("at_ninety_eight", 98))
                f.write(long_fn("at_the_ceiling", 100))
                f.write(long_fn("far_below", 20))
                f.write(long_fn("marked", 140, '#[expect(\n    clippy::too_many_lines,\n    reason = "x"\n)]\n'))
            scanned, near = lp.scan([d], threshold=100, margin=5)
            # The positive bar: an enumeration that read nothing would pass "nothing near" on an empty set.
            self.assertEqual(scanned, 4)
            self.assertEqual([(n, name) for n, _, _, name in near], [(100, "at_the_ceiling"), (98, "at_ninety_eight")])
            self.assertTrue(all(p.endswith("planted.rs") for _, p, _, _ in near))

    def test_the_repository_scan_reads_a_large_tree(self):
        scanned, near = lp.scan([os.path.join(ROOT, "crates", "theseus-protocol")])
        self.assertGreater(scanned, 100)
        self.assertTrue(all(n >= 95 for n, _, _, _ in near))

    def test_an_empty_scan_exits_nonzero_instead_of_reporting_no_pressure(self):
        with tempfile.TemporaryDirectory() as d:
            self.assertEqual(lp.scan([d])[0], 0)
            self.assertEqual(lp.main([d]), 2)

    def test_a_marked_short_neighbour_does_not_hide_the_function_below_it(self):
        with tempfile.TemporaryDirectory() as d:
            with open(os.path.join(d, "neighbours.rs"), "w") as f:
                f.write('#[expect(clippy::too_many_lines, reason = "x")]\nfn short_marked() { let a = 1; }\n\n')
                f.write(long_fn("tall_unmarked", 99))
            scanned, near = lp.scan([d], threshold=100, margin=5)
            self.assertEqual(scanned, 2)
            self.assertEqual([(n, name) for n, _, _, name in near], [(99, "tall_unmarked")])

    def test_a_plain_async_fn_counts_two_more_and_a_tokio_test_does_not(self):
        with tempfile.TemporaryDirectory() as d:
            body = "".join(f"    let v{i} = {i};\n" for i in range(96))
            with open(os.path.join(d, "asyncs.rs"), "w") as f:
                f.write(f"pub async fn plain() {{\n{body}}}\n")
                f.write(f"#[tokio::test]\nasync fn a_test() {{\n{body}}}\n")
                f.write(f"fn sync() {{\n{body}}}\n")
            scanned, near = lp.scan([d], threshold=100, margin=5)
            self.assertEqual(scanned, 3)
            self.assertEqual(sorted((n, name) for n, _, _, name in near), [(96, "a_test"), (96, "sync"), (98, "plain")])

    def test_a_cfg_test_gate_is_not_a_test_attribute_so_its_plain_async_fn_still_counts_two_more(self):
        # Only an attribute whose own path ends in `test` (`#[test]`, `#[tokio::test(..)]`) turns an `async fn`
        # into a sync one; `#[cfg(test)]` and `#[cfg(not(test))]` gate an item and leave it `async` (clippy counts
        # it 2 higher), so the script must too. Six such functions were read 2 short before this.
        with tempfile.TemporaryDirectory() as d:
            body = "".join(f"    let v{i} = {i};\n" for i in range(96))
            with open(os.path.join(d, "gated.rs"), "w") as f:
                f.write(f"#[cfg(test)]\nasync fn gated() {{\n{body}}}\n")
                f.write(f"#[cfg(not(test))]\nasync fn not_gated() {{\n{body}}}\n")
                f.write(f'#[tokio::test(flavor = "multi_thread")]\nasync fn multi() {{\n{body}}}\n')
            scanned, near = lp.scan([d], threshold=100, margin=5)
            self.assertEqual(scanned, 3)
            self.assertEqual(sorted((n, name) for n, _, _, name in near), [(96, "multi"), (98, "gated"), (98, "not_gated")])


class Calibration(unittest.TestCase):
    def test_the_count_matches_clippys_on_real_functions(self):
        self.assertGreaterEqual(len(CALIBRATION), 3)
        for path, name, want in CALIBRATION:
            with open(os.path.join(ROOT, path), encoding="utf-8") as f:
                src = f.read()
                counts = [lp.clippy_lines(b) + lp.clippy_extra(src, ln) for n, ln, b in lp.functions(src) if n == name]
            self.assertIn(want, counts, f"{path} {name}: clippy said {want}, script said {counts}")


if __name__ == "__main__":
    unittest.main()
