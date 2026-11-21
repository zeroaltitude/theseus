#!/usr/bin/env python3
"""Write STATS.md at the repository's root: the size of the code and of what it builds (issue 45).

    scripts/stats.py [--sizes-from TARGET/PROFILE] [--installed DIR]... [--root DIR]

Lines: every file git tracks, by the rule printed at the top of STATS.md, per crate under `crates/` and per
top-level directory for code that is in no crate. `vendor/` is shown but kept out of the total: it is not ours.

Sizes: `--sizes-from` reads an optimised build's profile directory (`$CARGO_TARGET_DIR/release`): each crate's
`.rlib` (the newest, when several builds left one) and its executables, and the commit from the build's
`.theseus-build-stamp`. `--installed` reads the executables of an install directory named for its commit
(`/usr/local/lib/theseus/<commit>`), and may be given more than once (the root helper's directory too). A part of the sizes not asked for is copied
from the STATS.md already there, so a recount at a landing leaves the sizes measured at the last install alone.

Nothing here reads a clock: the same tree and the same arguments give the same bytes.
Python 3, standard library only.
"""
import argparse
import bisect
import functools
import os
import re
import subprocess
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
KINDS = {".rs": "rs", ".ts": "js", ".tsx": "js", ".js": "js", ".mjs": "js", ".cjs": "js", ".c": "js", ".h": "js",
         ".css": "css", ".py": "py", ".sh": "sh", ".bash": "sh"}
CODE = {"rs": re.compile(r'//|/\*|\n|\bb?r#*"|"|\''), "js": re.compile(r'//|/\*|\n|["\'`]'),
        "css": re.compile(r'/\*|\n|["\']')}
BLOCK, NEWLINE = re.compile(r"/\*|\*/|\n"), re.compile(r"\n")
CHAR = re.compile(r"'(?:\\[^\n]+?|[^\\\n])'")  # a Rust char literal; a lifetime (`'a`) does not match
CFG_TEST = re.compile(r"#\[cfg\(test\)\]")
MOD_OPEN = re.compile(r"\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{")
SIZES = ("### What each crate builds", "### Executables that are installed and run")
RULE = """\
- **Code**: a line that is not blank and not only comment (code with a comment after it is code). **Test code**: files under a `tests/` or `test/` directory, files named `tests…`, `test_…` or `*.test.*`, and Rust's `#[cfg(test)] mod … { … }` blocks.
- **Comments**: `//`, `///`, `//!` and `/* … */` (nested, in Rust) in Rust, TypeScript, JavaScript, C and CSS; `#` lines in Python and shell, and Python's `\"\"\"` strings that begin a line. A marker inside a string literal is not a comment.
- **Not counted**: blank lines, and files that are not code (Markdown, TOML, JSON, data). `vendor/` is third-party and kept out of the total.
"""


@functools.lru_cache(maxsize=None)
def string_end(close, raw):
    return re.compile(("" if raw else r"\\[^\n]|") + r"\n|" + re.escape(close))


def scan(text, kind):
    """Per line of TEXT: (its code, with comments removed and string bodies blanked; whether it held comment)."""
    if kind in ("py", "sh"):
        return scan_hash(text, kind == "py")
    lines, cur, com, i, mode, depth, end, oneline = [], [], False, 0, "code", 0, None, False
    while True:
        rx = {"code": CODE[kind], "block": BLOCK, "line": NEWLINE}.get(mode, end)
        m = rx.search(text, i)
        seg = text[i:m.start() if m else len(text)]
        if mode == "code":
            cur.append(seg)
        elif mode == "str":
            cur.append("s" if seg else "")
        if not m:
            break
        t, i = m.group(0), m.end()
        if t == "\n":
            lines.append(("".join(cur), com))
            cur, com = [], mode == "block"
            if mode == "line" or (mode == "str" and oneline):
                mode = "code"
        elif mode == "code":
            if t == "//":
                mode, com = "line", True
            elif t == "/*":
                mode, depth, com = "block", 1, True
            elif t == "'" and kind == "rs":
                c = CHAR.match(text, m.start())
                cur.append("'c'" if c else "'")
                i = c.end() if c else i
            else:  # a string opens: `"`, a raw `r#"`, or in JavaScript `'` and a template's backtick
                close = '"' + "#" * t.count("#") if kind == "rs" else t
                mode, end, oneline = "str", string_end(close, t.endswith('"') and t != '"'), kind != "rs" and t != "`"
                cur.append(t)
        elif mode == "block":
            depth += 1 if t == "/*" and kind == "rs" else -1 if t == "*/" else 0
            mode = "code" if depth == 0 else mode
        elif t.startswith("\\"):  # an escape inside a string
            cur.append("s")
        else:
            cur.append(t)
            mode = "code"
    lines.append(("".join(cur), com))
    return lines


def scan_hash(text, py):
    """The `#` languages, line by line: a line that starts with `#` is comment; in Python, a triple-quoted string
    that begins a line is comment to its close, and any other one is code (a `#` in it is not a comment)."""
    lines, open_q = [], None  # open_q: (the open triple quote, whether it is a docstring)
    for line in text.split("\n"):
        s = line.strip()
        if open_q:
            lines.append(("" if open_q[1] else s, open_q[1]))
            open_q = None if open_q[0] in line else open_q
            continue
        q = next((d for d in ('"""', "'''") if d in line), None) if py else None
        doc = bool(q) and s.startswith(q)
        if q and line.count(q) % 2:
            open_q = (q, doc)
        comment = s.startswith("#") or doc
        lines.append(("" if comment else s, comment))
    return lines


def test_lines(codes):
    """The line numbers inside `#[cfg(test)] mod … { … }` blocks, brace-matched on code with strings blanked."""
    joined, starts, out = "\n".join(codes), [0], set()
    for c in codes:
        starts.append(starts[-1] + len(c) + 1)
    for m in CFG_TEST.finditer(joined):
        mm = MOD_OPEN.match(joined, m.end())
        if not mm:
            continue
        depth, j = 0, mm.end() - 1
        for j in range(mm.end() - 1, len(joined)):
            depth += {"{": 1, "}": -1}.get(joined[j], 0)
            if depth == 0:
                break
        out.update(range(bisect.bisect_right(starts, m.start()) - 1, bisect.bisect_right(starts, j)))
    return out


def is_test_file(path):
    parts = path.split("/")
    name = parts[-1]
    return bool({"tests", "test"} & set(parts[:-1])) or name.startswith(("tests", "test_")) or ".test." in name


def count(text, kind, test_file):
    """(code, test code, comment) lines of one file's TEXT."""
    lines = scan(text, kind)
    tests = test_lines([c for c, _ in lines]) if kind == "rs" and not test_file else set()
    code = test = comment = 0
    for k, (raw, (c, com)) in enumerate(zip(text.split("\n"), lines)):
        if not raw.strip():
            continue
        if c.strip():
            test, code = (test + 1, code) if test_file or k in tests else (test, code + 1)
        elif com:
            comment += 1
    return code, test, comment


def kind_of(path, full):
    kind = KINDS.get(os.path.splitext(path)[1])
    if kind or "." in os.path.basename(path):
        return kind
    with open(full, "rb") as f:
        first = f.readline(200)
    return "py" if first.startswith(b"#!") and b"python" in first else "sh" if first.startswith(b"#!") else None


def area(path):
    parts = path.split("/")
    if len(parts) == 1:
        return "(files at the root)"
    if parts[0] == "crates":
        return "crates/" + parts[1] if len(parts) > 2 else "crates/ (in no crate)"
    return parts[0] + "/"


def git(root, *args):
    return subprocess.run(["git", "-C", root, *args], check=True, capture_output=True, text=True).stdout


def count_tree(root):
    rows = {}
    for path in sorted(p for p in git(root, "ls-files", "-z").split("\0") if p):
        full = os.path.join(root, path)
        if os.path.islink(full) or not os.path.isfile(full):
            continue
        kind = kind_of(path, full)
        if kind:
            got = count(read(full), kind, is_test_file(path))
            rows[area(path)] = [a + b for a, b in zip(rows.get(area(path), (0, 0, 0)), got)]
    return rows


def read(path):
    with open(path, encoding="utf-8", errors="replace") as f:
        return f.read()


def mb(n):
    return f"{n / 1e6:,.2f} MB"


def crate_targets(crate_dir):
    """The library name (or None) and the executable names a crate builds, from its Cargo.toml and its layout."""
    names, head = {}, None
    with open(os.path.join(crate_dir, "Cargo.toml"), encoding="utf-8") as f:
        for line in f:
            h, n = re.match(r"\s*\[\[?([\w.-]+)\]", line), re.match(r'\s*name\s*=\s*"([^"]+)"', line)
            head = h.group(1) if h else head
            if n and not h:
                names.setdefault(head, []).append(n.group(1))
    pkg = names.get("package", ["?"])[0]
    src = os.path.join(crate_dir, "src")
    has_lib = "lib" in names or os.path.isfile(os.path.join(src, "lib.rs"))
    bins = set(names.get("bin", [])) | ({pkg} if os.path.isfile(os.path.join(src, "main.rs")) else set())
    if os.path.isdir(os.path.join(src, "bin")):
        bins |= {os.path.splitext(b)[0] for b in os.listdir(os.path.join(src, "bin")) if b.endswith(".rs")}
    lib = (names.get("lib") or [pkg])[0].replace("-", "_") if has_lib else None
    return lib, sorted(bins)


def crate_sizes(root, prof):
    """The `What each crate builds` part, from the profile directory PROF of an optimised build."""
    stamp = os.path.join(prof, ".theseus-build-stamp")
    commit = read(stamp).split()[0][:8] if os.path.isfile(stamp) else "an unrecorded commit"
    out = [SIZES[0], "", f"From the `{os.path.basename(prof)}` build of {commit} (an optimised build; a library's "
           "newest `.rlib` when the build directory holds several). An `.rlib` carries metadata and code for "
           "link-time optimisation, so it is far larger than what its crate adds to an executable: "
           "`scripts/size-breakdown.py` gives that.", "",
           "| Crate | Library (.rlib) | Executables |", "|---|---:|---|"]
    deps = os.path.join(prof, "deps")
    rlibs = sorted(os.listdir(deps)) if os.path.isdir(deps) else []
    for crate in sorted(d for d in os.listdir(os.path.join(root, "crates"))
                        if os.path.isfile(os.path.join(root, "crates", d, "Cargo.toml"))):
        lib, bins = crate_targets(os.path.join(root, "crates", crate))
        found = [os.path.join(deps, r) for r in rlibs if lib and r.startswith(f"lib{lib}-") and r.endswith(".rlib")]
        newest = max(found, key=lambda p: (os.stat(p).st_mtime_ns, p)) if found else None
        exes = [f"{b} {mb(os.path.getsize(os.path.join(prof, b)))}" for b in bins
                if os.path.isfile(os.path.join(prof, b))]
        size = mb(os.path.getsize(newest)) if newest else "—" if not lib else "not built"
        out.append(f"| crates/{crate} | {size} | {'; '.join(exes)} |")
    return "\n".join(out) + "\n"


def installed_sizes(dirs):
    """The `Executables that are installed and run` part: the `theseus…` executables of install directories,
    each named for its commit (a directory that is not, such as the root helper's, says so)."""
    out = [SIZES[1], "", "| Executable | Size | Installed from |", "|---|---:|---|"]
    total = 0
    for d in sorted(dirs):
        commit = os.path.basename(os.path.normpath(d))
        commit = commit if re.fullmatch(r"[0-9a-f]{7,40}", commit) else "an unrecorded commit"
        for name in sorted(os.listdir(d)):
            full = os.path.join(d, name)
            if name.startswith("theseus") and os.path.isfile(full) and os.access(full, os.X_OK):
                total += os.path.getsize(full)
                out.append(f"| {name} | {mb(os.path.getsize(full))} | {commit} |")
    out.append(f"| **Total** | **{mb(total)}** | |")
    return "\n".join(out) + "\n"


def kept(old, heading):
    """A sizes part copied from the STATS.md already there, or a note that it was never measured."""
    m = re.search(r"(?ms)^" + re.escape(heading) + r"\n.*?(?=^##|\Z)", old)
    if m:
        return m.group(0).rstrip("\n") + "\n"
    return f"{heading}\n\nNot yet measured: `scripts/stats.py --sizes-from <target dir>/release --installed <dir>`.\n"


def render(root, rows, old, sizes_from, installed):
    # A merge is named by its parents: a landing folds STATS.md into the merge it counted, which changes the merge's
    # own hash but not its parents, so the line stays true of the commit that carries it.
    head, *parents = git(root, "log", "-1", "--abbrev=8", "--format=%h %p", "HEAD").split()
    at = f"the merge of {parents[1]} into {parents[0]}" if len(parents) == 2 else f"commit {head}"
    dirty = git(root, "status", "--porcelain", "--untracked-files=no", "--", ".", ":!STATS.md").strip()
    out = ["# STATS", "", "The size of Theseus's code and of what it builds. Written by `scripts/stats.py`; "
           "do not edit it by hand.", "", f"Counted at {at}" + (" with uncommitted changes" if dirty else "")
           + ", over every file git tracks:", "", RULE, "## Lines", "",
           "| Where | Code | Test code | Comments |", "|---|---:|---:|---:|"]
    total = [0, 0, 0]
    for name in sorted(r for r in rows if r != "vendor/"):
        out.append(f"| {name} | " + " | ".join(f"{n:,}" for n in rows[name]) + " |")
        total = [a + b for a, b in zip(total, rows[name])]
    out.append("| **Total** | " + " | ".join(f"**{n:,}**" for n in total) + " |")
    if "vendor/" in rows:
        out.append("| vendor/ (third-party, not in the total) | " + " | ".join(f"{n:,}" for n in rows["vendor/"]) + " |")
    out += ["", "## Build sizes", "",
            crate_sizes(root, sizes_from) if sizes_from else kept(old, SIZES[0]),
            installed_sizes(installed) if installed else kept(old, SIZES[1])]
    return "\n".join(out).rstrip("\n") + "\n"


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--root", default=ROOT, help="the repository to count (default: this script's)")
    ap.add_argument("--sizes-from", metavar="DIR", help="an optimised build's profile directory, e.g. target/release")
    ap.add_argument("--installed", metavar="DIR", action="append", default=[], help="an install directory")
    args = ap.parse_args(argv)
    path = os.path.join(args.root, "STATS.md")
    old = read(path) if os.path.isfile(path) else ""
    text = render(args.root, count_tree(args.root), old, args.sizes_from, args.installed)
    if text != old:
        with open(path, "w", encoding="utf-8") as f:
            f.write(text)
    print(f"stats: {'wrote' if text != old else 'unchanged'} {path}", file=sys.stderr)


if __name__ == "__main__":
    main()
