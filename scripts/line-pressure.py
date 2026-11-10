#!/usr/bin/env python3
"""Functions near clippy's `too_many_lines` ceiling (theseus-4vsrz, 1.0.5 B7).

`clippy.toml` sets `too-many-lines-threshold = 100` and the gate runs clippy with `-D warnings`, so a function at
100 lines is green and one at 101 reddens the gate. Nothing else says which functions sit at 99: two branches that
each add three lines to a function at 98 pass alone and fail merged. This script lists every function within
MARGIN lines of the threshold (default 5), closest first, as `path:line fn_name N/100`, so the branch editing a
function sees the pressure before the join.

    python3 scripts/line-pressure.py [--margin N] [--threshold N] [PATH ...]

**It always exits 0 on a report.** It is a warning instrument, not a gate failure: making it one would be a second
ceiling that the next merge has to argue with, and clippy's own is the one that holds. It exits 2 only when it could
not read anything (no Rust files, or no function found), because an empty scan must not look like "no pressure".

What it counts is clippy's rule, copied from `clippy_lints/src/functions/too_many_lines.rs`: the function's body
without its outer braces, trimmed, then each line that holds code outside a comment (blank lines and comment-only
lines are free; a multi-line string's lines count whole). Where it can disagree with clippy:
  - clippy's comment scan is textual (`//` or `/*` inside a string literal on a line is read as a comment); this
    script does the same on purpose, since matching clippy matters more than being right about Rust. Its function
    FINDER is a real tokenizer (strings, raw strings, chars, lifetimes, nested block comments), so braces in
    strings do not move a body's end;
  - functions made by a macro (`macro_rules!` bodies, derives) are not seen by this script and are skipped by
    clippy, so a macro-expanded function is simply absent here;
  - a trait method without a body is not a function here; a closure is part of its function, as in clippy;
  - a plain `async fn` is 2 lines longer by clippy than its body says (measured, below): the desugared body is
    wrapped in a block that clippy counts, so this script adds 2 for an `async fn` unless its attributes include a
    `test` one (`#[tokio::test]` rewrites it into a sync fn, which counts exactly). Measured on the base with
    `too-many-lines-threshold = 85` in a scratch copy of clippy.toml, 364 flagged functions by `path:line`: sync
    `fn` 207 of 207 exact, `#[tokio::test] async fn` 72 of 72 exact, plain `async fn` 85 of 85 exactly 2 short
    before the rule; at exactly 100, clippy 29 and the script 19 before it. A `async` that is not on the `fn`'s own
    line (a multi-line signature) is not seen, and counts as sync;
  - a function whose attributes `allow` or `expect` too_many_lines is skipped (clippy does not judge it); only the
    contiguous attribute and doc-comment block directly above the `fn` is read, never a neighbour's;
  - `#[cfg(test)]` code is included (the gate runs clippy with `--all-targets`).
The calibration against clippy's own numbers is in scripts/tests/test_line_pressure.py's header.
"""
import argparse
import os
import re
import subprocess
import sys

THRESHOLD = 100
MARGIN = 5
IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")


def tokens(src):
    """Yield (kind, start, end) for the code tokens that matter: idents, '{', '}', ';', '(' ')' ; strings and
    comments are skipped whole."""
    i, n = 0, len(src)
    while i < n:
        c = src[i]
        if c == "/" and src.startswith("//", i):
            j = src.find("\n", i)
            i = n if j < 0 else j
        elif c == "/" and src.startswith("/*", i):
            depth, i = 1, i + 2
            while i < n and depth:
                if src.startswith("/*", i):
                    depth, i = depth + 1, i + 2
                elif src.startswith("*/", i):
                    depth, i = depth - 1, i + 2
                else:
                    i += 1
        elif c == '"' or (c in "rbc" and re.match(r'(?:br|rb|r|b|c|cr)(#*)"', src[i:i + 12])):
            m = re.match(r'(?:br|rb|r|b|c|cr)?(#*)"', src[i:i + 12])
            raw = "r" in src[i:i + 3].split('"')[0]
            hashes = m.group(1)
            i += m.end()
            if raw:
                end = '"' + hashes
                j = src.find(end, i)
                i = n if j < 0 else j + len(end)
            else:
                while i < n and src[i] != '"':
                    i += 2 if src[i] == "\\" else 1
                i += 1
        elif c == "'":
            m = re.match(r"'(?:\\.[^']*|[^\\'])'", src[i:i + 16])
            i += m.end() if m else 1  # a lifetime is just a quote
        elif c.isalpha() or c == "_":
            m = IDENT.match(src, i)
            yield ("id", m.start(), m.end())
            i = m.end()
        elif c in "{};()":
            yield (c, i, i + 1)
            i += 1
        else:
            i += 1


def functions(src):
    """Yield (name, line_of_fn, body_text_between_braces) for each `fn` with a body."""
    toks = list(tokens(src))
    for k, (kind, s, e) in enumerate(toks):
        if kind != "id" or src[s:e] != "fn" or k + 1 >= len(toks) or toks[k + 1][0] != "id":
            continue
        name = src[toks[k + 1][1]:toks[k + 1][2]]
        depth = 0  # paren depth, so a `{` in a where-clause type can't be taken before the body
        j = k + 2
        while j < len(toks):
            tk = toks[j][0]
            if tk == "(":
                depth += 1
            elif tk == ")":
                depth -= 1
            elif tk == ";" and depth == 0:
                j = -1  # a declaration with no body
                break
            elif tk == "{" and depth == 0:
                break
            j += 1
        if j < 0 or j >= len(toks):
            continue
        open_at, d, m = toks[j][1], 0, j
        while m < len(toks):
            if toks[m][0] == "{":
                d += 1
            elif toks[m][0] == "}":
                d -= 1
                if d == 0:
                    break
            m += 1
        if m >= len(toks):
            continue
        yield name, src.count("\n", 0, s) + 1, src[open_at + 1:toks[m][1]]


def clippy_lines(body):
    """clippy's too_many_lines count for a body (braces already removed)."""
    count, in_comment = 0, False
    for line in body.strip().split("\n"):
        code = False
        while True:
            line = line.lstrip()
            if not line:
                break
            if in_comment:
                i = line.find("*/")
                if i >= 0:
                    line, in_comment = line[i + 2:], False
                    continue
            else:
                n = len(line)
                multi = line.find("/*")
                single = line.find("//")
                multi = n if multi < 0 else multi
                single = n if single < 0 else single
                code |= multi > 0 and single > 0
                if multi < single:
                    line, in_comment = line[multi + 2:], True
                    continue
            break
        count += code
    return count


def attribute_block(src, line):
    """The contiguous attribute and doc-comment lines directly above the `fn` on `line` (1-based), nearest
    last. It stops at the previous item: a line ending in `;`, `{` or `}`, so a short neighbour's attributes
    (which belong to the neighbour) are never read as this function's."""
    lines = src.split("\n")
    out = []
    for l in reversed(lines[max(0, line - 40):line - 1]):
        t = l.strip()
        if t.endswith((";", "{", "}")):
            break
        out.append(l)
    return list(reversed(out))


def clippy_extra(src, line):
    """2 for a plain `async fn` (see the header), else 0: `async` on the `fn`'s own line, and no `test`
    attribute (`#[test]`, `#[tokio::test]`, ...) in its attribute block."""
    head = src.split("\n")[line - 1]
    m = re.search(r"\bfn\b", head)
    head = head[:m.start()] if m else head
    if not re.search(r"\basync\b", head):
        return 0
    # The attribute's own path ends in `test`: `#[test]`, `#[tokio::test(..)]`. A `#[cfg(test)]` or
    # `#[cfg(not(test))]` gates the item and leaves it `async`.
    if re.search(r"#\[(?:\w+::)*test\b", "\n".join(attribute_block(src, line))):
        return 0
    return 2


def exempt(src, line):
    """True when the attributes just above the `fn` on `line` allow or expect too_many_lines: clippy does not
    judge such a function, and an `expect` is the sweep's deliberate mark (theseus-goa8), not pressure."""
    text = "\n".join(l for l in attribute_block(src, line) if not l.strip().startswith("//"))
    for m in re.finditer(r"#\[(?:allow|expect)\((.*?)\)\]", text, re.S):
        if "clippy::too_many_lines" in m.group(1):
            return True
    return False


def rust_files(paths):
    if paths:
        roots = paths
    else:
        out = subprocess.run(["git", "ls-files", "--cached", "--others", "--exclude-standard", "--", "crates/*.rs"],
                             capture_output=True, text=True, check=True).stdout
        return [f for f in out.split("\n") if f and os.path.isfile(f)]
    found = []
    for r in roots:
        if os.path.isfile(r):
            found.append(r)
        for d, _, fs in os.walk(r):
            found += [os.path.join(d, f) for f in fs if f.endswith(".rs")]
    return sorted(found)


def scan(paths, threshold=THRESHOLD, margin=MARGIN):
    """Return (functions_scanned, [(lines, path, line, name)]) for functions at threshold-margin or more."""
    scanned, near = 0, []
    for path in rust_files(paths):
        with open(path, encoding="utf-8", errors="replace") as f:
            src = f.read()
        for name, line, body in functions(src):
            scanned += 1
            n = clippy_lines(body) + clippy_extra(src, line)
            if exempt(src, line):
                continue
            if n >= threshold - margin:
                near.append((n, path, line, name))
    near.sort(key=lambda t: (-t[0], t[1], t[2]))
    return scanned, near


def main(argv=None):
    ap = argparse.ArgumentParser(description="functions near clippy's too_many_lines threshold (never fails on a report)")
    ap.add_argument("--margin", type=int, default=MARGIN)
    ap.add_argument("--threshold", type=int, default=THRESHOLD)
    ap.add_argument("paths", nargs="*")
    a = ap.parse_args(argv)
    if not a.paths:
        os.chdir(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
    scanned, near = scan(a.paths, a.threshold, a.margin)
    if scanned == 0:
        print("line-pressure: found no functions: it read nothing, so it says nothing", file=sys.stderr)
        return 2
    for n, path, line, name in near:
        flag = "  OVER" if n > a.threshold else ""
        print(f"{path}:{line} {name} {n}/{a.threshold}{flag}")
    at = sum(1 for t in near if t[0] == a.threshold)
    print(f"line-pressure: {scanned} functions scanned, {len(near)} within {a.margin} of {a.threshold} "
          f"({at} at exactly {a.threshold}); a warning only, clippy holds the ceiling")
    return 0


if __name__ == "__main__":
    sys.exit(main())
