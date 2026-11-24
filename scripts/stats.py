#!/usr/bin/env python3
"""Write STATS.md at the repository's root: the size of the code and of what it builds (issue 45).

    scripts/stats.py [--sizes-from TARGET/PROFILE] [--installed DIR]... [--root DIR]
    scripts/stats.py --range BEFORE..AFTER [--root DIR]
    scripts/stats.py --prepare-landing [--sizes-from TARGET/PROFILE] [--root DIR]
    scripts/stats.py --sizes-from TARGET/PROFILE --record-sizes FILE [--root DIR]
    scripts/stats.py --count-head [--root DIR]

`--range` writes nothing: it counts the trees of two commits from git's objects (no checkout) and prints one line, the
change in lines a landing's note states (code, test code, comments; before -> after). A landing's range is its merge's
first parent and the merge: `scripts/stats.py --range <merge>^1..<merge>`.

Lines: every file git tracks, by the rule printed at the top of STATS.md, per crate under `crates/` and per
top-level directory for code that is in no crate. `vendor/` is shown but kept out of the total: it is not ours.

Sizes: `--sizes-from` reads an optimised build's profile directory (`$CARGO_TARGET_DIR/release`): each crate's
`.rlib` (the newest, when several builds left one) and its executables, and the commit from the build's
`.theseus-build-stamp`. `--installed` reads the executables of an install directory named for its commit
(`/usr/local/lib/theseus/<commit>`), and may be given more than once (the root helper's directory too). A part of the sizes not asked for is copied
from the STATS.md already there, so a recount at a landing leaves the sizes measured at the last install alone.

`--record-sizes FILE` writes nothing in the tree: it writes the "What each crate builds" part of the build `--sizes-from`
names to FILE, and refuses a build whose stamp is not `clean` (a build of edits, not of one commit). `scripts/build.sh`
records each release build so, in `$THESEUS_STATS_SIZES` or `~/.cache/theseus/stats-sizes.md`. With no `--sizes-from`, a
count and `--prepare-landing` take that record when it is newer and of main: its build's commit is an ancestor of HEAD
(the landing's first parent) and a descendant of the build the sizes there name. Else the sizes there are kept, so a
landing never moves them back to an older build, or to a build of a commit main does not hold.

`--prepare-landing` is a step of preparing a landing, before its merge is committed: with a merge onto `main` prepared
(`git merge --no-commit`), it counts the PREPARED TREE (the index's, `git write-tree`, read from git's objects, never
the working files), writes STATS.md and stages it, and nothing else. It refuses when HEAD is not `main`, when no merge
is in progress, when the index holds unmerged paths, or when anything but STATS.md would change. The landing's own
commit, signed by whoever lands, then holds the count, and every gate judges the tree that lands; no commit is ever
rewritten. A merge in progress is named by MERGE_HEAD and HEAD, the parents it will have.

`--count-head` is the self-join's step (`crates/theseus-core/src/rsi/join/stats.rs`), run from main's copy frozen at the
join's base: with HEAD a made merge (two parents, no merge in progress), it counts HEAD's tree from git's objects and
writes STATS.md in the working tree, keeping both size parts, and stages nothing. The join checks that nothing else
changed, stages it and folds it into its own unsigned merge.

Nothing here reads a clock: the same tree and the same arguments give the same bytes.
Python 3, standard library only.
"""
import argparse
import bisect
import functools
import io
import os
import re
import subprocess
import sys
import tokenize

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
KINDS = {".rs": "rs", ".ts": "js", ".tsx": "js", ".js": "js", ".mjs": "js", ".cjs": "js", ".c": "js", ".h": "js",
         ".css": "css", ".py": "py", ".sh": "sh", ".bash": "sh"}
CODE = {"rs": re.compile(r'//|/\*|\n|\bb?r#*"|"|\''), "js": re.compile(r'//|/\*|\n|["\'`]'),
        "css": re.compile(r'/\*|\n|["\']')}
BLOCK, NEWLINE = re.compile(r"/\*|\*/|\n"), re.compile(r"\n")
CHAR = re.compile(r"'(?:\\[^\n]+?|[^\\\n])'")  # a Rust char literal; a lifetime (`'a`) does not match
CFG_TEST = re.compile(r"#\[cfg\(test\)\]")
MOD_OPEN = re.compile(r"\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{")
TEST_NAME = re.compile(r"(tests?|fixtures|benches)$|tests?_|.*_tests?$")
PY_DOC = re.compile(r"[rRuUbBfF]*(\"\"\"|\'\'\')")
PY_LEAD = {tokenize.NEWLINE, tokenize.INDENT, tokenize.DEDENT}
PY_SKIP = {tokenize.NEWLINE, tokenize.NL, tokenize.INDENT, tokenize.DEDENT, tokenize.ENDMARKER}
SIZES = ("### What each crate builds", "### Executables that are installed and run")
BUILT_OF = re.compile(r"^From the `[^`]*` build of ([0-9a-f]{7,40}) ", re.M)
RECORD = os.path.join("~", ".cache", "theseus", "stats-sizes.md")
RULE = """\
- **Code**: a line that is not blank and not only comment (code with a comment after it is code). **Test code**: a file in a directory, or with a name (before its first dot), that is `test`, `tests`, `fixtures`, `benches`, `test_…`, `tests_…`, `…_test` or `…_tests`; files named `*.test.*`; and Rust's `#[cfg(test)] mod … { … }` blocks.
- **Comments**: `//`, `///`, `//!` and `/* … */` (nested, in Rust) in Rust, TypeScript, JavaScript, C and CSS; in Python, comment tokens and triple-quoted strings that begin a statement (read with Python's own tokenizer); in shell, a `#` that begins a word outside quotes. A marker inside a string literal is not a comment.
- **Not counted**: blank lines, and files that are not code (Markdown, TOML, JSON, data). `vendor/` is third-party and kept out of the total.
"""


@functools.lru_cache(maxsize=None)
def string_end(close, raw):
    return re.compile(("" if raw else r"\\[^\n]|") + r"\n|" + re.escape(close))


def scan(text, kind):
    """Per line of TEXT: (its code, with comments removed and string bodies blanked; whether it held comment)."""
    if kind in ("py", "sh"):
        return scan_py(text) if kind == "py" else scan_sh(text)
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


def scan_py(text):
    """Python, by its own tokenizer: a comment token, or a triple-quoted string that begins a statement, is comment;
    any other token is code, and code wins on a line holding both. A file that does not tokenize falls back to its
    `#` lines as comment and the rest as code."""
    n = text.count("\n") + 1
    code, com, prev = [False] * n, [False] * n, tokenize.NEWLINE
    try:
        for tok in tokenize.generate_tokens(io.StringIO(text).readline):
            span = range(tok.start[0] - 1, min(tok.end[0], n))
            if tok.type == tokenize.COMMENT:
                com[tok.start[0] - 1] = True
            elif tok.type == tokenize.STRING and prev in PY_LEAD and PY_DOC.match(tok.string):
                com[span.start:span.stop] = [True] * len(span)
            elif tok.type not in PY_SKIP:
                code[span.start:span.stop] = [True] * len(span)
            prev = prev if tok.type in (tokenize.COMMENT, tokenize.NL) else tok.type
    except (tokenize.TokenError, SyntaxError):
        return [("", True) if t.strip().startswith("#") else (t, False) for t in text.split("\n")]
    return [("x" if c else "", m) for c, m in zip(code, com)]


def scan_sh(text):
    """Shell, character by character: a `#` that begins a word outside quotes is comment to the line's end, and a
    quote still open at a line's end stays open on the next (its lines are code)."""
    lines, q = [], None
    for line in text.split("\n"):
        code, com, prev, i = False, False, " ", 0
        while i < len(line):
            ch = line[i]
            if q:
                q = None if ch == q else q
                i += 1 if q == '"' and ch == "\\" else 0
            elif ch == "#" and prev in " \t;|&()":
                com = True
                break
            elif ch == "\\":
                i += 1
            elif ch in "'\"":
                q = ch
            code, prev, i = code or not ch.isspace(), ch, i + 1
        lines.append(("x" if code else "", com))
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
    """Test code by its path: any directory, or the file's name before its first dot, named as TEST_NAME says."""
    *dirs, name = path.split("/")
    return any(TEST_NAME.match(p) for p in [*dirs, name.split(".")[0]]) or ".test." in name


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


def kind_of(path, full=None, first=None):
    """The kind of the file at PATH, by its extension, else by its first line (read from FULL, or given as FIRST)."""
    kind = KINDS.get(os.path.splitext(path)[1])
    if kind or "." in os.path.basename(path):
        return kind
    if first is None:
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


def count_rev(root, rev):
    """count_tree's rows for the tree of commit REV, read from git's objects: no checkout, no working tree."""
    listed = git(root, "ls-tree", "-r", "-z", rev).split("\0")
    blobs = []
    for entry in (e for e in listed if e):
        meta, path = entry.split("\t", 1)
        mode, typ, sha = meta.split()
        if typ == "blob" and mode != "120000":
            blobs.append((path, sha))
    proc = subprocess.run(["git", "-C", root, "cat-file", "--batch"], check=True, capture_output=True,
                          input="".join(f"{sha}\n" for _, sha in blobs).encode())
    out, i, rows = proc.stdout, 0, {}
    for path, _ in blobs:
        nl = out.index(b"\n", i)
        size = int(out[i:nl].split()[2])
        body = out[nl + 1:nl + 1 + size]
        i = nl + 1 + size + 1
        kind = kind_of(path, first=body.split(b"\n", 1)[0][:200])
        if kind:
            got = count(body.decode("utf-8", errors="replace"), kind, is_test_file(path))
            rows[area(path)] = [a + b for a, b in zip(rows.get(area(path), (0, 0, 0)), got)]
    return rows


def totals(rows):
    """Code, test code and comment lines in all, `vendor/` left out as STATS.md leaves it out."""
    return [sum(r[k] for name, r in rows.items() if name != "vendor/") for k in range(3)]


def range_line(root, spec):
    """The line a landing's note states for BEFORE..AFTER: each total before -> after, and its change."""
    before, after = spec.split("..", 1)
    names = [git(root, "rev-parse", "--short=8", r).strip() for r in (before, after)]
    old, new = totals(count_rev(root, before)), totals(count_rev(root, after))
    parts = [f"{what} {a:,} -> {b:,} ({b - a:+,})" for what, a, b in zip(("code", "test code", "comments"), old, new)]
    return f"lines {names[0]}..{names[1]}: " + "; ".join(parts)


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


def record_sizes(root, prof, out):
    """`--record-sizes`: the sizes part of the build at PROF, written to OUT (whole, by a rename); none of a build
    whose stamp is not `clean`."""
    stamp = os.path.join(prof, ".theseus-build-stamp")
    words = read(stamp).split() if os.path.isfile(stamp) else []
    if len(words) < 2 or words[1] != "clean":
        raise SystemExit(f"stats: no sizes recorded: {prof} is not a clean build of one commit "
                         f"(its stamp: {' '.join(words[:2]) or 'none'})")
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
    with open(f"{out}.tmp", "w", encoding="utf-8") as f:
        f.write(crate_sizes(root, prof))
    os.replace(f"{out}.tmp", out)


def commit_named(root, part):
    """The commit a sizes PART names as its build's, in full; None when it names none, or one this repository lacks."""
    m = BUILT_OF.search(part)
    if not m:
        return None
    return subprocess.run(["git", "-C", root, "rev-parse", "-q", "--verify", f"{m.group(1)}^{{commit}}"],
                          capture_output=True, text=True).stdout.strip() or None


def is_ancestor(root, a, b):
    return subprocess.run(["git", "-C", root, "merge-base", "--is-ancestor", a, b], capture_output=True).returncode == 0


def recorded(root, old, tip):
    """The recorded sizes part (`--record-sizes`), when it is newer and of main: its build's commit is an ancestor of
    TIP and a descendant of the build OLD's sizes name (any, when they name none this repository has). None otherwise,
    and the sizes in OLD are kept."""
    path = os.environ.get("THESEUS_STATS_SIZES") or os.path.expanduser(RECORD)
    if os.path.islink(path) or not os.path.isfile(path) or os.path.getsize(path) > 65536:
        return None
    text = read(path)
    if not text.startswith(SIZES[0] + "\n"):
        return None
    new, now = commit_named(root, text), commit_named(root, kept(old, SIZES[0]))
    if not new or not is_ancestor(root, new, tip) or (now and not is_ancestor(root, now, new)):
        return None
    return kept(text, SIZES[0])


def merge_head(root):
    """The merge in progress (`git merge --no-commit`), short, or ''."""
    return subprocess.run(["git", "-C", root, "rev-parse", "-q", "--verify", "--short=8", "MERGE_HEAD"],
                          capture_output=True, text=True).stdout.strip()


def render(root, rows, old, sizes_from, installed, prepared=False, record=None):
    # A merge is named by its parents. A merge in progress is the landing being prepared, named by the parents it
    # will have (HEAD and MERGE_HEAD), whatever HEAD itself is: HEAD may be an earlier landing's merge.
    head, *parents = git(root, "log", "-1", "--abbrev=8", "--format=%h %p", "HEAD").split()
    merging = merge_head(root)
    if merging:
        at = f"the merge of {merging} into {head}"
        # A prepared count is of the index's tree, so no working file is in it; else the merge's own changes are
        # staged, and only what is not staged is uncommitted.
        dirty = "" if prepared else git(root, "diff", "--name-only", "--", ".", ":!STATS.md").strip()
    else:
        at = f"the merge of {parents[1]} into {parents[0]}" if len(parents) == 2 else f"commit {head}"
        dirty = "" if prepared else git(root, "status", "--porcelain", "--untracked-files=no", "--", ".",
                                        ":!STATS.md").strip()
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
            crate_sizes(root, sizes_from) if sizes_from else record or kept(old, SIZES[0]),
            installed_sizes(installed) if installed else kept(old, SIZES[1])]
    return "\n".join(out).rstrip("\n") + "\n"


def prepare_landing(root, sizes_from, installed):
    """`--prepare-landing`: count the prepared landing's tree and stage STATS.md alone; refuse plainly otherwise."""
    def refuse(why):
        raise SystemExit(f"stats: not counted: {why}")
    if subprocess.run(["git", "-C", root, "symbolic-ref", "-q", "HEAD"], capture_output=True,
                      text=True).stdout.strip() != "refs/heads/main":
        refuse("HEAD is not main; a landing is prepared on main")
    if not merge_head(root):
        refuse("no merge is in progress; prepare the landing with `git merge --no-commit --no-ff <branch>` first")
    if git(root, "ls-files", "--unmerged").strip():
        refuse("the merge has unmerged paths; resolve them and stage the result first")
    before = git(root, "write-tree").strip()
    path = os.path.join(root, "STATS.md")
    old = read(path) if os.path.isfile(path) else ""
    record = None if sizes_from else recorded(root, old, "HEAD")
    text = render(root, count_rev(root, before), old, sizes_from, installed, prepared=True, record=record)
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)
    git(root, "add", "--", "STATS.md")
    after = git(root, "write-tree").strip()
    changed = [p for p in git(root, "diff-tree", "-r", "--name-only", before, after).split("\n") if p]
    if changed not in ([], ["STATS.md"]):
        git(root, "read-tree", before)
        refuse(f"staging STATS.md would change {', '.join(changed)} too; the index is put back")
    print(f"stats: counted the prepared landing; STATS.md {'staged' if changed else 'unchanged'}", file=sys.stderr)


def count_head(root):
    """`--count-head`: count the made merge at HEAD from its objects and write STATS.md in the working tree, alone,
    staging nothing; refuse plainly when HEAD is not a made merge."""
    if merge_head(root):
        raise SystemExit("stats: not counted: a merge is in progress; --count-head counts a made merge")
    if len(git(root, "rev-list", "--parents", "-n", "1", "HEAD").split()) != 3:
        raise SystemExit("stats: not counted: HEAD is not a merge of two parents")
    old = subprocess.run(["git", "-C", root, "cat-file", "blob", "HEAD:STATS.md"], capture_output=True,
                         text=True).stdout
    text = render(root, count_rev(root, "HEAD"), old, None, [], prepared=True)
    path = os.path.join(root, "STATS.md")
    if os.path.lexists(path):
        os.unlink(path)  # never written through a link
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)
    print(f"stats: counted HEAD; STATS.md {'unchanged' if text == old else 'written'}", file=sys.stderr)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--root", default=ROOT, help="the repository to count (default: this script's)")
    ap.add_argument("--sizes-from", metavar="DIR", help="an optimised build's profile directory, e.g. target/release")
    ap.add_argument("--installed", metavar="DIR", action="append", default=[], help="an install directory")
    ap.add_argument("--range", metavar="BEFORE..AFTER", help="print the change in lines between two commits; write nothing")
    ap.add_argument("--prepare-landing", action="store_true",
                    help="count a prepared merge onto main from its index tree, and stage STATS.md alone")
    ap.add_argument("--record-sizes", metavar="FILE",
                    help="write the sizes of the build --sizes-from names to FILE, outside the tree; nothing else")
    ap.add_argument("--count-head", action="store_true",
                    help="count the made merge at HEAD and write STATS.md in the working tree, staging nothing")
    args = ap.parse_args(argv)
    if args.record_sizes:
        if not args.sizes_from:
            ap.error("--record-sizes takes --sizes-from")
        record_sizes(args.root, args.sizes_from, args.record_sizes)
        return
    if args.count_head:
        count_head(args.root)
        return
    if args.prepare_landing:
        prepare_landing(args.root, args.sizes_from, args.installed)
        return
    if args.range:
        if ".." not in args.range:
            ap.error("--range takes BEFORE..AFTER")
        print(range_line(args.root, args.range))
        return
    path = os.path.join(args.root, "STATS.md")
    old = read(path) if os.path.isfile(path) else ""
    record = None if args.sizes_from else recorded(args.root, old, "HEAD")
    text = render(args.root, count_tree(args.root), old, args.sizes_from, args.installed, record=record)
    if text != old:
        with open(path, "w", encoding="utf-8") as f:
            f.write(text)
    print(f"stats: {'wrote' if text != old else 'unchanged'} {path}", file=sys.stderr)


if __name__ == "__main__":
    main()
