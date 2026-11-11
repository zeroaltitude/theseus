#!/usr/bin/env python3
"""The map of what the compiler reads as test code, and two reports over it
(theseus-4vsrz).

**Why a map at all.** Three different pieces of work have now wanted the
question "is this line test code?", and each answered it from the thing
nearest to hand: the file's own text, or a `tests_`-shaped path. Both are
wrong here. `crates/theseus-core/src/aws/value_shape.rs` is production code
whose header talks about tests; `crates/theseus-core/src/tests_fan_out_cue.rs`
is named like test code and the compiler never reads it. The only sound
answer is the declaration the compiler reads, so that is what this walks.

**What the walk follows**, with the Rust rule each case needed:

* `#[cfg(test)] mod N;` — resolved to `N.rs` or `N/mod.rs` against the
  declaring file's own module directory (a `lib.rs`, `main.rs` or `mod.rs`
  resolves against its directory; any other file against `<stem>/`).
* `#[cfg(test)] mod N { .. }` — the brace-matched line range of the block.
  Brace counting is done on a form of the file with comments and string, raw
  string and char literal bodies blanked, carrying state across lines: a
  `r#"{ .. }"#` fixture at `crates/theseus-aws-catalog/src/lib.rs:361` closed
  its `mod tests` block 190 lines early and hid 32 tests from a line-local
  version of this walk.
* `#[path = "F"] mod N;` — `F` against the directory holding the declaring
  file, not the module directory. This is how `turn/tests_admission.rs` is
  declared (`crates/theseus-core/src/turn/admission.rs:1059`), and missing it
  hid 376 test functions.
* a `mod N;` written *inside* an inline module — against
  `<module dir>/<inline mod>/`, and it is test code when any enclosing inline
  module is `#[cfg(test)]` even where the declaring file is not. This is how
  `push/tests/{seed,short,why,work}.rs` (`push.rs:622`-`:625`) and
  `tests_m3/parallel/{order,runs}.rs` (`tests_m3.rs:6134`) are declared.
* `tests/*.rs` and `tests/*/main.rs` of a crate whose manifest does not set
  `autotests = false`, plus any explicit `[[test]] path =`: cargo's
  auto-discovery IS the declaration for those.
* `crates/complete.rs`, which is reached by `include!` and belongs to no
  crate.

**The two reports.**

`--orphans` (default) is exact: a file holding `#[test]`/`#[tokio::test]`
that the walk never reached. There is no judgment in it — the compiler either
reads a file or it does not — so a hit is a defect: tests that do not run.

`--sweep` is a heuristic for the pattern `crates/complete.rs` names, **a test
that documents the defect as the design**: it prints test-code comment lines
carrying one of that pattern's tells ("has to be", "whatever X says",
"never re-", and the "deliberately / on purpose / for now" family) with no
decision, row, date or owner quote in the same comment paragraph. Every hit
must be read by a person. On the suite as it stood at the 1.0.5 kick-off the
sweep's hits were all false: *"whatever X says"* is this codebase's idiom for
**strengthening** an invariant ("the result is Y whatever X says"), which is
the opposite of the pattern, and the two closest hits were the prescribed
correct shape. So this report is a reading aid and nothing more.

**Both reports exit 0.** The sweep's tells are prose and will have false
positives by construction, and a false positive that reds a departure costs
more than the pattern does; `shape.sh` owns the hard ceilings. Exit 2 means
it scanned nothing, which is the one thing that makes the report a lie — the
same bar `line-pressure.py` holds, and the same reason: "zero missing" is met
by an empty set.

Standard library only. Run it from the repository root, or pass the root.
"""

import argparse
import collections
import os
import re
import sys

# --- the walk --------------------------------------------------------------

MOD_DECL = re.compile(r'^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*(;|\{)')
CFG_TEST = re.compile(r'#\[cfg\([^)]*\btest\b')
PATH_ATTR = re.compile(r'#\[path\s*=\s*"([^"]+)"\s*\]')
# `#[test]`, `#[tokio::test]`, `#[tokio::test(flavor = "multi_thread")]`. The
# path segment must END in `test`, or `#[cfg(test)]` matches and every
# production file reads as an orphan.
TEST_FN = re.compile(r'^\s*#\[\s*(?:[A-Za-z_][\w]*::)*test\s*[\](]')


def read(path):
    with open(path, encoding="utf-8", errors="replace") as handle:
        return handle.read().splitlines()


def code_only(lines):
    """`lines` with comments and literal bodies blanked, state carried across
    lines, so brace counting is sound over raw strings and `'{'`."""
    out = []
    state = None  # None | "block" | "str" | ("raw", hashes)
    for line in lines:
        buf = []
        i, n = 0, len(line)
        while i < n:
            if state == "block":
                at = line.find("*/", i)
                if at < 0:
                    i = n
                else:
                    state, i = None, at + 2
                continue
            if isinstance(state, tuple):
                close = '"' + "#" * state[1]
                at = line.find(close, i)
                if at < 0:
                    i = n
                else:
                    state, i = None, at + len(close)
                continue
            if state == "str":
                while i < n:
                    if line[i] == "\\":
                        i += 2
                        continue
                    if line[i] == '"':
                        state = None
                        i += 1
                        break
                    i += 1
                continue
            ch = line[i]
            if ch == "/" and i + 1 < n and line[i + 1] == "/":
                break
            if ch == "/" and i + 1 < n and line[i + 1] == "*":
                state, i = "block", i + 2
                continue
            raw = re.match(r'(?:b|c)?r(#*)"', line[i:])
            if raw:
                hashes = len(raw.group(1))
                i += raw.end()
                close = '"' + "#" * hashes
                at = line.find(close, i)
                if at < 0:
                    state, i = ("raw", hashes), n
                else:
                    i = at + len(close)
                continue
            if ch == '"':
                state, i = "str", i + 1
                continue
            if ch == "'":
                lit = re.match(r"'(?:\\.|[^'\\])'", line[i:])
                if lit:
                    i += lit.end()
                    continue
            buf.append(ch)
            i += 1
        out.append("".join(buf))
    return out


def block_end(code, start):
    """0-based index of the line closing the brace opened on line `start`."""
    depth = 0
    for j in range(start, len(code)):
        depth += code[j].count("{") - code[j].count("}")
        if (j > start or "{" in code[j]) and depth <= 0:
            return j
    return len(code) - 1


def module_dir(path):
    base = os.path.basename(path)
    if base in ("lib.rs", "main.rs", "mod.rs"):
        return os.path.dirname(path)
    return os.path.join(os.path.dirname(path), base[:-3])


class Walk:
    def __init__(self, root):
        self.root = root
        self.regions = []  # (relpath, first, last, kind)
        self.seen = set()

    def rel(self, path):
        return os.path.relpath(path, self.root).replace(os.sep, "/")

    def resolve(self, cands, gated):
        for cand in cands:
            if os.path.isfile(cand):
                self.file(cand, gated)
                return

    def file(self, path, gated):
        if path in self.seen:
            return
        self.seen.add(path)
        lines = read(path)
        code = code_only(lines)
        if gated:
            self.regions.append((self.rel(path), 1, len(lines), "cfg(test) file module"))
        base = module_dir(path)
        stack = []  # (name, end_idx, is_test)
        i = 0
        while i < len(code):
            while stack and i > stack[-1][1]:
                stack.pop()
            here = os.path.join(base, *[entry[0] for entry in stack])
            inner = gated or any(entry[2] for entry in stack)

            if CFG_TEST.search(code[i]):
                j, override = i + 1, None
                while j < len(code) and (code[j].strip().startswith("#[") or not code[j].strip()):
                    # the RAW line: code_only blanks the string body, so
                    # `#[path = "x.rs"]` arrives as `#[path = ]`
                    found = PATH_ATTR.search(lines[j])
                    if found:
                        override = found.group(1)
                    j += 1
                decl = MOD_DECL.match(code[j]) if j < len(code) else None
                if decl:
                    name, tail = decl.group(1), decl.group(2)
                    if tail == ";":
                        cands = []
                        if override:
                            cands += [os.path.join(os.path.dirname(path), override),
                                      os.path.join(here, override)]
                        cands += [os.path.join(here, name + ".rs"),
                                  os.path.join(here, name, "mod.rs")]
                        self.resolve(cands, True)
                    else:
                        end = block_end(code, j)
                        if not gated:
                            self.regions.append(
                                (self.rel(path), j + 1, end + 1, "cfg(test) inline mod"))
                        stack.append((name, end, True))
                        i = j + 1
                        continue

            decl = MOD_DECL.match(code[i])
            if decl:
                name, tail = decl.group(1), decl.group(2)
                if tail == "{":
                    stack.append((name, block_end(code, i), False))
                elif not (i and CFG_TEST.search(code[i - 1])):
                    override, k = None, i - 1
                    while k >= 0 and code[k].strip().startswith("#["):
                        found = PATH_ATTR.search(lines[k])
                        if found:
                            override = found.group(1)
                        k -= 1
                    cands = []
                    if override:
                        cands += [os.path.join(os.path.dirname(path), override),
                                  os.path.join(here, override)]
                    cands += [os.path.join(here, name + ".rs"),
                              os.path.join(here, name, "mod.rs")]
                    self.resolve(cands, inner)
            i += 1

    def crates(self, crates_dir):
        for crate in sorted(os.listdir(crates_dir)):
            cdir = os.path.join(crates_dir, crate)
            manifest_path = os.path.join(cdir, "Cargo.toml")
            if not os.path.isfile(manifest_path):
                continue
            with open(manifest_path, encoding="utf-8", errors="replace") as handle:
                manifest = handle.read()
            for entry in ("src/lib.rs", "src/main.rs"):
                path = os.path.join(cdir, entry)
                if os.path.isfile(path):
                    self.file(path, False)
            for found in re.finditer(r'path\s*=\s*"([^"]+\.rs)"', manifest):
                path = os.path.join(cdir, found.group(1))
                if os.path.isfile(path):
                    self.file(path, True if "/tests/" in found.group(1) else False)
            if re.search(r'autotests\s*=\s*false', manifest):
                continue
            tdir = os.path.join(cdir, "tests")
            if not os.path.isdir(tdir):
                continue
            for entry in sorted(os.listdir(tdir)):
                path = os.path.join(tdir, entry)
                if entry.endswith(".rs") and os.path.isfile(path):
                    self.regions.append(
                        (self.rel(path), 1, len(read(path)), "[[test]] target"))
                    self.seen.add(path)
                    self.file(path, False)
                elif os.path.isdir(path):
                    for sub in sorted(os.listdir(path)):
                        spath = os.path.join(path, sub)
                        if sub.endswith(".rs") and os.path.isfile(spath):
                            kind = "[[test]] target" if sub == "main.rs" else "test helper"
                            self.regions.append((self.rel(spath), 1, len(read(spath)), kind))
                            self.seen.add(spath)
        shared = os.path.join(crates_dir, "complete.rs")
        if os.path.isfile(shared):
            self.regions.append(
                (self.rel(shared), 1, len(read(shared)), "include!()-reached"))
            self.seen.add(shared)


# --- the sweep -------------------------------------------------------------

TELLS = [
    ("has-to-be", re.compile(r'\bhas to be\b|\bhave to be\b|\bmust be a\b', re.I)),
    ("whatever-says", re.compile(r'\bwhatever\b[^.]{0,60}\b(?:says|say|said)\b', re.I)),
    ("never-re", re.compile(r'\bnever re-\w+', re.I)),
    ("no-second", re.compile(
        r'\b(?:has no|have no|there is no|with no|and no)\s+\w*\s*'
        r'(?:second|other|further|another|way|path|hook|field|place|chance|test|guard|check)\b',
        re.I)),
    ("by-design", re.compile(
        r'\bby design\b|\bintentional\w*\b|\bdeliberat\w+\b|\bon purpose\b'
        r'|\bis not a bug\b|\bfor now\b|\bknown (?:gap|limitation)\b', re.I)),
]
# A real constraint has a source; an accidental one only has prose.
SOURCE = re.compile(
    r'theseus-[a-z0-9]{3,}'      # a row id
    r"|\bthe owner('s)?\b"       # an owner quote or ruling
    r'|\bKeel:'                  # an ack
    r'|\bItem \d+|\bdocs/|\bspecs?/'
    r'|https?://'
    r'|\bRFC\s*\d+|\bPOSIX\b|\bCVE-'
    r'|\b\d{4}-\d{2}-\d{2}\b',   # a dated decision
    re.I)
COMMENT = re.compile(r'^\s*(?://|/?\*)')
BLANK_COMMENT = ("//", "//!", "///", "*")


def paragraph(lines, at, low, high):
    """The comment paragraph line `at` sits in, bounded by a blank comment
    line or by a line that is not a comment, within [low, high]."""
    first = at
    while first > low and COMMENT.match(lines[first - 2]) \
            and lines[first - 2].strip() not in BLANK_COMMENT:
        first -= 1
    last = at
    while last < high and COMMENT.match(lines[last]) \
            and lines[last].strip() not in BLANK_COMMENT:
        last += 1
    return "\n".join(lines[first - 1:last])


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("root", nargs="?", default=".", help="repository root")
    ap.add_argument("--sweep", action="store_true",
                    help="the defect-as-design tell sweep (heuristic; read every hit)")
    ap.add_argument("--map", action="store_true",
                    help="print every region, as PATH:FIRST-LAST<TAB>KIND")
    args = ap.parse_args()

    root = os.path.abspath(args.root)
    crates_dir = os.path.join(root, "crates")
    if not os.path.isdir(crates_dir):
        print(f"test-code-map: no crates/ under {root}", file=sys.stderr)
        return 2

    walk = Walk(root)
    walk.crates(crates_dir)
    if not walk.regions:
        print("test-code-map: the walk reached no test code at all, which cannot be "
              "right: it read the wrong tree, and a report over nothing is a lie.",
              file=sys.stderr)
        return 2

    spans = collections.defaultdict(list)
    for path, first, last, kind in walk.regions:
        spans[path].append((first, last, kind))

    if args.map:
        for path, first, last, kind in sorted(walk.regions):
            print(f"{path}:{first}-{last}\t{kind}")
        return 0

    by_kind = collections.Counter(kind for _, _, _, kind in walk.regions)
    print(f"test-code regions: {len(walk.regions)} in {len(spans)} files")
    for kind, count in by_kind.most_common():
        print(f"  {count:5d}  {kind}")
    print()

    if args.sweep:
        hits, cleared = [], 0
        for path in sorted(spans):
            full = os.path.join(root, path)
            if not os.path.isfile(full):
                continue
            lines = read(full)
            for first, last, _ in spans[path]:
                high = min(last, len(lines))
                for n in range(first, high + 1):
                    line = lines[n - 1]
                    if not COMMENT.match(line):
                        continue
                    for name, rx in TELLS:
                        if not rx.search(line):
                            continue
                        if SOURCE.search(paragraph(lines, n, first, high)):
                            cleared += 1
                        else:
                            hits.append((path, n, name, line.strip()))
                        break
        print(f"comment lines carrying a tell, with a source beside them: {cleared}")
        print(f"comment lines carrying a tell and NO source, to read by hand: {len(hits)}")
        for name, count in collections.Counter(h[2] for h in hits).most_common():
            print(f"  {count:5d}  {name}")
        print()
        for path, n, name, text in hits:
            print(f"{path}:{n}\t[{name}]\t{text}")
        print()
        print("Every line above is a reading aid, not a finding: the tells are prose. "
              "Judge each by whether the sentence explains a NARROWER behaviour as "
              "settled with nothing to cite for it.")

    orphans = collections.defaultdict(list)
    for dirpath, _, filenames in os.walk(crates_dir):
        if "target" in dirpath.split(os.sep):
            continue
        for name in filenames:
            if not name.endswith(".rs"):
                continue
            full = os.path.join(dirpath, name)
            path = walk.rel(full)
            covered = spans.get(path, [])
            for n, line in enumerate(read(full), 1):
                if TEST_FN.match(line) and not any(a <= n <= b for a, b, _ in covered):
                    orphans[path].append(n)

    if orphans:
        print(f"ORPHANED test code: {sum(len(v) for v in orphans.values())} test "
              f"functions in {len(orphans)} files that NO declaration reaches, so "
              f"the compiler never reads them and they never run:")
        for path in sorted(orphans):
            at = ", ".join(str(n) for n in orphans[path])
            print(f"  {path}: {len(orphans[path])} at line {at}")
        print("  Declaring one changes what the suite runs, so each is its own piece "
              "of work: the tests may not pass, and may not be meant to.")
    else:
        print("ORPHANED test code: none. Every #[test] the tree holds is reached by a "
              "declaration the compiler reads.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
