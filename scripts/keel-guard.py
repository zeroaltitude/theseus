#!/usr/bin/env python3
"""The keel guard (theseus-pw1q.1): fail a range that deletes or loosens a test, a budget or a ceiling.

Nothing else in the gate stops a branch from passing by removing what would have failed it. This reads the
range's net change, base tree against head tree, and names every erosion in it, one line each:

    <path>:<line>: <rule>: <old> -> <new>

A finding passes only when a commit in the range whose signature verifies against a key in the BASE's
scripts/keel-signers carries a trailer for it:

    Keel: <rule> <path>[, <path>...] — <why>

(`--` or ` - ` may stand for the dash; a path may be a glob). An unsigned commit's trailer counts for nothing, so a
branch can't ack itself. The signer list is read from the base, never the head, so a branch can't list its own key.

The range (`THESEUS_KEEL_BASE=<rev>` or `--base` overrides its base, and the gate finds it as below in its own shell,
then passes it as `--base` to the base's copy of this script, so a branch's copy decides neither; `--head` names a commit to judge instead of the
working tree):
  - on a merge commit whose first parent is on main (the join's gate): HEAD^1..HEAD, the merge judged whole;
  - otherwise merge-base(HEAD, main)..HEAD, with `origin/main` when there is no local `main`;
  - with no `--head`, the head side is the working tree (tracked changes and untracked files), so the gate before a
    commit judges what the commit will hold. Acks are read from the commits in base..HEAD.

The rules, by the name the report and the ack use:
  test-removed    a test function at the base (Rust: #[test], #[tokio::test], #[rstest], #[test_case], any
                  attribute whose last segment is `test`; Python: a `def test_*`) that is not at the head under the
                  same path and name. A test that moved file or module keeping its name, or renamed with the same
                  body, is matched and judged by the rules below instead.
  test-ignored    #[ignore] (or a cfg_attr's ignore) added to a test; in Python a skip decorator (unittest's skip,
                  skipIf, skipUnless, expectedFailure, pytest's skip marks) on the test or its class, or a
                  self.skipTest / SkipTest added to its body.
  assert-removed  a test with fewer assertions than at the base (Rust: every `...assert...!` macro, assert!,
                  assert_eq!, debug_assert_ne!, prop_assert!, and a wait that fails the test, wait_for(, wait_until(,
                  .until(, so an assertion made a wait keeps its check; Python: `assert` statements and
                  self.assert*/fail calls, pytest.raises). Counted per test, comments left out: a rewritten assertion is not a finding.
  allow-added     a #[allow(...)], #![allow(...)] or #[expect(...)] (an expect silences a lint as an allow does:
                  the shape budget's marks) added for a lint on an item that had none, counted per lint and item
                  across the changed files, so a moved item keeps its marks; a lint level lowered or removed in a
                  Cargo.toml [lints] or [workspace.lints] table (or a crate's `workspace = true` dropped); a
                  clippy.toml threshold raised or removed, or an allow-* switch turned on.
  budget-raised   a numeric literal raised, or its line removed, in:
                    crates/theseus-sim/src/lifecycle.rs   fn budget_ms, fn margin_ms, const CANCEL_FRAMES
                    crates/theseus-sim/src/perf.rs        const PLAIN_TURN_FRAMES, TOOL_TURN_FRAMES, BINARY_MB
                    crates/theseus-sim/src/jobs.rs        const START_P95_MS
                    scripts/gate.sh                       THESEUS_GATE_BENCH_ALLOWANCE's default
  cap-raised      a spend or loop cap raised, removed, or its mode loosened (end/ask to notify, or a mode named for
                  the first time as notify or off):
                    bench/**/*.toml                       keys naming max_loops, max_turns, spend, budget, usd
                    bench/**/*.py but test_*.py           lines naming max_budget_usd, max_turns, max_loops,
                                                          MAX_LOOPS, SPEND_LIMIT, spend_limit, BUDGET with a number
                    crates/theseus-core/config/theseus.example.toml  keys naming spend, usd, per_day, daily,
                                                          max_loops (commented lines too)
  ceiling-raised  scripts/long-files.txt: a ceiling raised or an entry added; scripts/shape.sh: `limit=` raised;
                  .config/nextest.toml: retries added or raised, slow-timeout's period or terminate-after widened
                  (or terminate-after dropped), leak-timeout widened, an override added (the flaky list given a
                  test).
  keel-file       any change to the guard itself when it exists at the base: this script, scripts/keel-signers,
                  scripts/test_keel_guard.py, and scripts/gate.sh's lines that name the keel. Adding them is not
                  one; adding a key to keel-signers is.

Exit status: 0 nothing unacked; 1 a finding not acked; 2 the guard could not run (no base, not a repository).
Standard library only.
"""

import argparse
import difflib
import fnmatch
import os
import re
import subprocess
import sys
import time
from collections import Counter, defaultdict

RULES = ("test-removed", "test-ignored", "assert-removed", "allow-added", "budget-raised", "cap-raised",
         "ceiling-raised", "keel-file")
KEEL_FILES = ("scripts/keel-guard.py", "scripts/keel-signers", "scripts/test_keel_guard.py")
SIGNERS = "scripts/keel-signers"


class GuardError(Exception):
    pass


def git(repo, *args, check=True, data=None):
    p = subprocess.run(["git", "-C", repo, *args], input=data, capture_output=True)
    if check and p.returncode != 0:
        raise GuardError(f"git {' '.join(args)}: {p.stderr.decode(errors='replace').strip()}")
    return p.stdout.decode(errors="replace") if p.returncode == 0 else None


def rev(repo, name):
    out = git(repo, "rev-parse", "--verify", "-q", name + "^{commit}", check=False)
    return out.strip() if out else None


# ---------------------------------------------------------------- the range

def resolve_range(repo, base_arg, head_arg):
    """(base, head, head_commit, how): head is None for the working tree."""
    head_commit = rev(repo, head_arg or "HEAD")
    if head_commit is None:
        raise GuardError(f"no commit {head_arg or 'HEAD'}")
    if base_arg:
        base = rev(repo, base_arg)
        if base is None:
            raise GuardError(f"no commit {base_arg} (THESEUS_KEEL_BASE or --base)")
        return base, head_arg, head_commit, f"{base_arg}..{head_arg or 'the working tree'}"
    local, remote = rev(repo, "main"), rev(repo, "origin/main")
    main = local or remote
    if local and remote and local != remote:
        # The newer of the two when one contains the other (a clone whose local main was never pulled).
        if git(repo, "merge-base", "--is-ancestor", local, remote, check=False) is not None:
            main = remote
    if main is None:
        raise GuardError("no main and no origin/main to judge against: set THESEUS_KEEL_BASE=<rev>")
    parents = git(repo, "rev-list", "--parents", "-n", "1", head_commit).split()[1:]
    if len(parents) >= 2:
        first = parents[0]
        on_main = git(repo, "merge-base", "--is-ancestor", first, main, check=False) is not None
        if on_main:
            return first, head_arg, head_commit, "a merge: HEAD^1..HEAD"
    mb = git(repo, "merge-base", head_commit, main, check=False)
    if not mb:
        raise GuardError("HEAD shares no history with main: set THESEUS_KEEL_BASE=<rev>")
    return mb.strip(), head_arg, head_commit, "a branch: merge-base(HEAD, main)..HEAD"


class Tree:
    """File contents at the base, and at the head (a commit, or the working tree), read through one cat-file."""

    def __init__(self, repo, base, head):
        self.repo, self.base, self.head = repo, base, head
        self.proc = subprocess.Popen(["git", "-C", repo, "cat-file", "--batch"], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE)
        self.cache = {}

    def changed(self):
        if self.head:
            out = git(self.repo, "diff", "--no-renames", "--name-only", "-z", self.base, self.head)
            return sorted(set(filter(None, out.split("\0"))))
        out = git(self.repo, "diff", "--no-renames", "--name-only", "-z", self.base)
        names = set(filter(None, out.split("\0")))
        untracked = git(self.repo, "ls-files", "--others", "--exclude-standard", "-z")
        names.update(filter(None, untracked.split("\0")))
        return sorted(names)

    def blob(self, spec):
        self.proc.stdin.write(spec.encode() + b"\n")
        self.proc.stdin.flush()
        header = self.proc.stdout.readline().decode().split()
        if len(header) < 3 or header[1] == "missing":
            return None
        size = int(header[2])
        data = self.proc.stdout.read(size)
        self.proc.stdout.read(1)
        if header[1] != "blob":
            return None
        return data.decode(errors="replace")

    def old(self, path):
        key = ("old", path)
        if key not in self.cache:
            self.cache[key] = self.blob(f"{self.base}:{path}")
        return self.cache[key]

    def new(self, path):
        key = ("new", path)
        if key not in self.cache:
            if self.head:
                self.cache[key] = self.blob(f"{self.head}:{path}")
            else:
                full = os.path.join(self.repo, path)
                try:
                    if os.path.isfile(full) and not os.path.islink(full):
                        with open(full, encoding="utf-8", errors="replace") as f:
                            self.cache[key] = f.read()
                    else:
                        self.cache[key] = None
                except OSError:
                    self.cache[key] = None
        return self.cache[key]

    def close(self):
        self.proc.stdin.close()
        self.proc.wait()


class Finding:
    def __init__(self, rule, path, line, old, new):
        self.rule, self.path, self.line, self.old, self.new = rule, path, line, old, new

    def __str__(self):
        return f"{self.path}:{self.line}: {self.rule}: {clip(self.old)} -> {clip(self.new)}"


def clip(s, n=160):
    s = " ".join(str(s).split())
    return s if len(s) <= n else s[: n - 1] + "…"


def line_of(src, pos):
    return src.count("\n", 0, pos) + 1


# ---------------------------------------------------------------- Rust

RUST_TOKEN = re.compile(
    r"""//[^\n]*
      | /\*.*?\*/
      | b?r(?P<hash>\#*)".*?"(?P=hash)
      | b?"(?:\\.|[^"\\])*"
      | b?'(?:\\u\{[0-9a-fA-F]+\}|\\.|[^\\'\n])'
      | (?P<attr>\#!?\[)
      | \bfn\s+(?P<fn>[A-Za-z_]\w*)
      | \bmod\s+(?P<mod>[A-Za-z_]\w*)\s*(?=\{)
      | (?P<punct>[{}\[\];])
    """,
    re.X | re.S,
)
# An assertion, or a wait that fails the test when its condition never comes (wait_for, wait_until, `.until(`):
# an assertion made a wait keeps its check.
ASSERT_RS = re.compile(r"\b\w*assert\w*!|\bwait_(?:for|until)\w*\s*\(|\.until\s*\(")
TEST_ATTR = re.compile(r"#\[\s*([\w:]+)")
TEST_NAMES = {"test", "rstest", "test_case", "test_matrix", "quickcheck", "wasm_bindgen_test"}
ITEM_AFTER = re.compile(
    r"(?:\s|//[^\n]*|\#\[[^\]]*\]|pub(?:\([^)]*\))?|async|unsafe|default|extern\s+\"[^\"]*\")*"
    r"(fn|mod|struct|enum|union|impl|trait|type|static|const|let|use|macro_rules!)\s*([\w:<>]*)")


def strip_rs_comments(text):
    out, last = [], 0
    for m in RUST_TOKEN.finditer(text):
        t = m.group(0)
        if t.startswith("//") or t.startswith("/*"):
            out.append(text[last:m.start()])
            out.append("\n" * t.count("\n"))
            last = m.end()
    out.append(text[last:])
    return "".join(out)


def is_test_attr(attr):
    m = TEST_ATTR.match(attr)
    if not m:
        return False
    name = m.group(1)
    return name.split("::")[-1] in TEST_NAMES


def is_ignore_attr(attr):
    body = attr.lstrip("#![ ").rstrip("] ")
    return body.startswith("ignore") or (body.startswith("cfg_attr") and re.search(r"\bignore\b", body) is not None)


def parse_rust(src):
    """Tests (key -> dict) and allow marks ([(lint, item, line, text)]) of one Rust file."""
    tests, marks = {}, []
    stack = []  # ("mod", name) | ("test", info) | ("other",)
    pending = []  # attributes before the next item
    pending_fn = None
    attr_start = None
    attr_depth = 0
    for m in RUST_TOKEN.finditer(src):
        t = m.group(0)
        if attr_start is not None:
            if t == "[":
                attr_depth += 1
            elif t == "]":
                attr_depth -= 1
                if attr_depth == 0:
                    text = src[attr_start:m.end()]
                    pending.append((text, attr_start))
                    allow_mark(src, text, attr_start, m.end(), stack, marks)
                    attr_start = None
            continue
        if m.group("attr"):
            attr_start, attr_depth = m.start(), 1
            continue
        if not (m.group("fn") or m.group("mod") or m.group("punct")):
            continue  # a comment, a string or a char
        if m.group("fn"):
            attrs = [a for a, _ in pending]
            if any(is_test_attr(a) for a in attrs):
                pending_fn = (m.group("fn"), attrs, pending[0][1] if pending else m.start(), m.start())
            else:
                pending_fn = None
            continue
        if m.group("mod"):
            pending.clear()
            stack.append(("pmod", m.group("mod")))
            continue
        if t == "{":
            if stack and stack[-1][0] == "pmod":
                stack[-1] = ("mod", stack[-1][1])
            elif pending_fn:
                stack.append(("test", pending_fn, m.start()))
                pending_fn = None
            else:
                stack.append(("other",))
            pending.clear()
        elif t == "}":
            if stack:
                top = stack.pop()
                if top[0] == "test":
                    (name, attrs, astart, fstart), _ = top[1], top[2]
                    mods = [s[1] for s in stack if s[0] == "mod"]
                    key = "::".join(mods + [name])
                    body = src[fstart:m.end()]
                    code = strip_rs_comments(body)
                    tests[key] = {
                        "name": name,
                        "line": line_of(src, fstart),
                        "attrs": attrs,
                        "ignored": any(is_ignore_attr(a) for a in attrs),
                        "asserts": len(ASSERT_RS.findall(code)),
                        "norm": " ".join(code.replace("fn " + name, "fn _", 1).split()),
                        "head": src[fstart:src.find("\n", fstart)].strip(),
                    }
            pending.clear()
            pending_fn = None
        elif t == ";":
            pending.clear()
            pending_fn = None
    return tests, marks


ALLOW_RE = re.compile(r"#!?\[\s*(allow|expect)\s*\((.*)\)\s*\]\s*$", re.S)
CFG_ALLOW_RE = re.compile(r"\b(allow|expect)\s*\(([^()]*)\)")


def allow_mark(src, text, start, end, stack, marks):
    m = ALLOW_RE.match(text)
    lints = []
    if m:
        lints = m.group(2)
    elif text.lstrip("#![ ").startswith("cfg_attr"):
        cm = CFG_ALLOW_RE.search(text)
        if not cm:
            return
        lints = cm.group(2)
    else:
        return
    lints = [x.strip() for x in split_top(lints)]
    lints = [x for x in lints if x and "=" not in x]
    if text.startswith("#!"):
        mods = [s[1] for s in stack if s[0] == "mod"]
        item = "file " + "::".join(mods)
    else:
        im = ITEM_AFTER.match(src, end)
        if im:
            item = f"{im.group(1)} {im.group(2)}"
        else:  # a field's or an expression's: the rest of its line names it
            item = " ".join(src[end:src.find("\n", end)].split()) or "item"
    for lint in lints:
        marks.append((lint, item, line_of(src, start), " ".join(text.split())))


def split_top(s):
    out, depth, cur, q = [], 0, [], False
    for ch in s:
        if ch == '"':
            q = not q
        if not q:
            if ch in "([{":
                depth += 1
            elif ch in ")]}":
                depth -= 1
            elif ch == "," and depth == 0:
                out.append("".join(cur))
                cur = []
                continue
        cur.append(ch)
    out.append("".join(cur))
    return out


# ---------------------------------------------------------------- Python

PY_SKIP_DECO = re.compile(r"@\s*(?:unittest\.|pytest\.mark\.)?(skip\w*|expectedFailure)\b")
PY_SKIP_CALL = re.compile(r"\bself\.skipTest\s*\(|\braise\s+(?:unittest\.)?SkipTest\b|\bpytest\.skip\s*\(")
PY_ASSERT = re.compile(r"^\s*assert\b|\bself\.assert\w*\s*\(|\bself\.fail\s*\(|\bpytest\.raises\s*\(", re.M)
PY_DEF = re.compile(r"^(\s*)(?:async\s+)?def\s+(test\w*)\s*\(")
PY_CLASS = re.compile(r"^(\s*)class\s+(\w+)")


def indent_of(line):
    return len(line) - len(line.lstrip())


def parse_python(src):
    tests = {}
    lines = src.split("\n")
    classes = []  # (indent, name, skipped)
    decos = []
    i = 0
    while i < len(lines):
        line = lines[i]
        s = line.strip()
        if not s or s.startswith("#"):
            i += 1
            continue
        ind = indent_of(line)
        while classes and ind <= classes[-1][0]:
            classes.pop()
        if s.startswith("@"):
            decos.append(s)
            i += 1
            continue
        cm = PY_CLASS.match(line)
        if cm:
            classes.append((ind, cm.group(2), any(PY_SKIP_DECO.match(d) for d in decos)))
            decos = []
            i += 1
            continue
        dm = PY_DEF.match(line)
        if dm and (not classes or ind > classes[-1][0]):
            j = i + 1
            while j < len(lines) and (not lines[j].strip() or indent_of(lines[j]) > ind):
                j += 1
            body = "\n".join(lines[i:j])
            code = "\n".join(x for x in body.split("\n") if not x.strip().startswith("#"))
            name = dm.group(2)
            key = ".".join([c[1] for c in classes] + [name])
            tests[key] = {
                "name": name,
                "line": i + 1,
                "attrs": list(decos),
                "ignored": any(PY_SKIP_DECO.match(d) for d in decos) or any(c[2] for c in classes),
                "skips": len(PY_SKIP_CALL.findall(code)),
                "asserts": len(PY_ASSERT.findall(code)),
                "norm": " ".join(code.replace("def " + name, "def _", 1).split()),
                "head": s,
            }
            decos = []
            i = j
            continue
        decos = []
        i += 1
    return tests


# ---------------------------------------------------------------- the rules

def tests_rule(tree, paths, out):
    base_tests, head_tests = {}, {}
    base_marks, head_marks = [], []
    for path in paths:
        rust = path.endswith(".rs")
        for side, store, marks in (("old", base_tests, base_marks), ("new", head_tests, head_marks)):
            src = getattr(tree, side)(path)
            if src is None:
                continue
            if rust:
                if "#[" not in src and "#![" not in src:
                    continue
                t, mk = parse_rust(src)
                marks.extend((path,) + x for x in mk)
            else:
                if "def test" not in src:
                    continue
                t = parse_python(src)
            for k, v in t.items():
                v["path"] = path
                store[(path, k)] = v
    removed = [k for k in base_tests if k not in head_tests]
    added = [k for k in head_tests if k not in base_tests]
    by_name, by_body = defaultdict(list), defaultdict(list)
    for k in added:
        by_name[head_tests[k]["name"]].append(k)
        by_body[head_tests[k]["norm"]].append(k)
    claimed = set()
    pairs = [(k, k) for k in base_tests if k in head_tests]
    for k in removed:
        b = base_tests[k]
        cands = [c for c in by_name.get(b["name"], []) if c not in claimed]
        same_body = [c for c in cands if head_tests[c]["norm"] == b["norm"]]
        match = (same_body or cands or [c for c in by_body.get(b["norm"], []) if c not in claimed] or [None])[0]
        how = "moved" if match is not None else ""
        if match is None:
            match = rewritten(b, k[0], [c for c in added if c not in claimed], head_tests)
            how = "renamed and rewritten"
        if match is not None:
            MATCHED.append(f"{k[0]}::{k[1]} -> {match[0]}::{match[1]} ({how})")
        if match is None:
            out.append(Finding("test-removed", k[0], b["line"], b["head"], "(gone)"))
        else:
            claimed.add(match)
            pairs.append((k, match))
    for bk, hk in pairs:
        b, h = base_tests[bk], head_tests[hk]
        if h["ignored"] and not b["ignored"] or h.get("skips", 0) > b.get("skips", 0):
            new = next((a for a in h["attrs"] if is_ignore_attr(a) or PY_SKIP_DECO.match(a)), "a skip in its body")
            out.append(Finding("test-ignored", hk[0], h["line"], b["head"], f"{new} {h['head']}"))
        if h["asserts"] < b["asserts"]:
            out.append(Finding("assert-removed", hk[0], h["line"],
                               f"{b['head']} ({b['asserts']} assertions)", f"{h['asserts']} assertions"))
    allow_rule(base_marks, head_marks, out)


MATCHED = []  # the tests that went and came back, said in the report


def rewritten(b, path, cands, head_tests):
    """A test renamed and rewritten with it: the added test most like it, at least half alike, its own file first."""
    words = b["norm"].split()
    best, score = None, 0.0
    for same_file in (True, False):
        for c in cands:
            if (c[0] == path) != same_file:
                continue
            h = head_tests[c]["norm"].split()
            sm = difflib.SequenceMatcher(None, words, h, autojunk=False)
            if sm.real_quick_ratio() <= score or sm.quick_ratio() <= score:
                continue
            r = sm.ratio()
            if r > score:
                best, score = c, r
        if score >= SIMILAR:
            return best
    return None


# How alike (difflib's ratio over the bodies' words) a test that went and one that came must be to count as one
# test renamed and rewritten: the replay of main's last 50 merges set it (scripts/keel-guard.py's tests hold it).
SIMILAR = 0.5


def allow_rule(base_marks, head_marks, out):
    before = Counter((lint, item) for _, lint, item, _, _ in base_marks)
    after = Counter((lint, item) for _, lint, item, _, _ in head_marks)
    for key, n in after.items():
        if n > before.get(key, 0):
            hits = [x for x in head_marks if (x[1], x[2]) == key]
            olds = [x for x in base_marks if (x[1], x[2]) == key]
            path, _, item, line, text = hits[-1]
            old = f"{len(olds)} on {item}" if olds else f"no {key[0]} on {item}"
            out.append(Finding("allow-added", path, line, old, text))


LEVELS = {"forbid": 4, "deny": 3, "warn": 2, "allow": 0}


def toml_entries(src):
    """{(section, key): (value, line)} for simple `key = value` lines; array tables keyed by their index."""
    out, section, counts = {}, "", Counter()
    for n, raw in enumerate(src.split("\n"), 1):
        s = raw.strip()
        if s.startswith("[["):
            name = s.strip("[] ")
            counts[name] += 1
            section = f"{name}#{counts[name]}"
            continue
        if s.startswith("["):
            section = s.split("]")[0].strip("[ ")
            continue
        m = re.match(r"([\w\-\"\.]+)\s*=\s*(.+?)\s*(?:#.*)?$", s)
        if m:
            out[(section, m.group(1).strip('"'))] = (m.group(2), n)
    return out


def lints_rule(tree, path, out):
    old, new = tree.old(path), tree.new(path)
    if old is None:
        return
    a, b = toml_entries(old), toml_entries(new or "")
    for (sec, key), (val, line) in a.items():
        if not (sec == "lints" or sec.startswith("lints.") or sec.startswith("workspace.lints")):
            continue
        if sec == "lints" and key == "workspace":
            if (sec, key) not in b or "true" not in b[(sec, key)][0]:
                out.append(Finding("allow-added", path, line, f"[{sec}] {key} = {val}", "(gone)"))
            continue
        lvl = level_of(val)
        if (sec, key) not in b:
            out.append(Finding("allow-added", path, line, f"[{sec}] {key} = {val}", "(gone)"))
            continue
        nv, nl = b[(sec, key)]
        if level_of(nv) < lvl:
            out.append(Finding("allow-added", path, nl, f"{key} = {val}", f"{key} = {nv}"))


def level_of(val):
    m = re.search(r"(forbid|deny|warn|allow)", val)
    return LEVELS[m.group(1)] if m else 0


NUM = re.compile(r"(?<![\w.])-?\d[\d_]*(?:\.\d+)?(?:e[-+]?\d+)?")


def num(s):
    try:
        return float(s.replace("_", ""))
    except ValueError:
        return None


def shape_rows(lines):
    """[(shape, [numbers], line, text)] for lines holding a number; the shape is the line with numbers as #."""
    rows = []
    for n, text in lines:
        nums = [num(x) for x in NUM.findall(text)]
        if not nums:
            continue
        rows.append((NUM.sub("#", " ".join(text.split())), nums, n, text.strip()))
    return rows


def compare_rows(rule, path, old_rows, new_rows, out, removal=True):
    pool = defaultdict(list)
    for r in new_rows:
        pool[r[0]].append(r)
    for shape, nums, line, text in old_rows:
        if pool.get(shape):
            nshape, nnums, nline, ntext = pool[shape].pop(0)
            if any(b > a for a, b in zip(nums, nnums)):
                out.append(Finding(rule, path, nline, text, ntext))
        elif removal:
            out.append(Finding(rule, path, line, text, "(gone or rewritten)"))


def rust_item_lines(src, kind, name):
    """The comment-free lines of `fn name`'s body or `const name`'s line, numbered."""
    if src is None:
        return []
    code = strip_rs_comments(src)
    if kind == "const":
        m = re.search(r"\bconst\s+" + name + r"\b[^;]*;", code)
    else:
        m = re.search(r"\bfn\s+" + name + r"\b", code)
        if m:
            i = code.find("{", m.end())
            depth, j = 0, i
            while j < len(code):
                if code[j] == "{":
                    depth += 1
                elif code[j] == "}":
                    depth -= 1
                    if depth == 0:
                        break
                j += 1
            m = re.compile(r".*", re.S).match(code, m.start(), j + 1) if i >= 0 else None
    if not m:
        return []
    first = line_of(code, m.start())
    return [(first + k, t) for k, t in enumerate(m.group(0).split("\n"))]


BUDGETS = {
    "crates/theseus-sim/src/lifecycle.rs": [("fn", "budget_ms"), ("fn", "margin_ms"), ("const", "CANCEL_FRAMES")],
    "crates/theseus-sim/src/perf.rs": [("const", "PLAIN_TURN_FRAMES"), ("const", "TOOL_TURN_FRAMES"),
                                       ("const", "BINARY_MB")],
    "crates/theseus-sim/src/jobs.rs": [("const", "START_P95_MS")],
}


def budget_rule(tree, path, out):
    if path == "scripts/gate.sh":
        pick = lambda src: [(n, t) for n, t in enumerate((src or "").split("\n"), 1)
                            if "THESEUS_GATE_BENCH_ALLOWANCE:-" in t]
        compare_rows("budget-raised", path, shape_rows(pick(tree.old(path))), shape_rows(pick(tree.new(path))), out)
        return
    for kind, name in BUDGETS[path]:
        old = rust_item_lines(tree.old(path), kind, name)
        if not old:
            continue
        new = rust_item_lines(tree.new(path), kind, name)
        compare_rows("budget-raised", path, shape_rows(old), shape_rows(new), out)


CAP_KEY = re.compile(r"max_loops|max_turns|spend|budget|usd|per_day|daily")
LOOSER = {"end": 2, "ask": 2, "refuse": 2, "restrict": 2, "notify": 1, "off": 0}


def cap_toml(tree, path, out, commented):
    old, new = tree.old(path), tree.new(path)
    if old is None:
        return

    def entries(src):
        if src is None:
            return {}
        if commented:
            src = re.sub(r"(?m)^(\s*)#\s?(?=[\w\-]+\s*=)", r"\1", src)
            src = re.sub(r"(?m)^#\s?(\[[^\]\s]+\])\s*(?:#.*)?$", r"\1", src)
        return {k: v for k, v in toml_entries(src).items() if CAP_KEY.search(k[1])}

    a, b = entries(old), entries(new)
    for k, (val, line) in a.items():
        sec, key = k
        if k not in b:
            out.append(Finding("cap-raised", path, line, f"[{sec}] {key} = {val}", "(gone)"))
            continue
        nv, nl = b[k]
        if key.endswith("_mode"):
            ov, nw = LOOSER.get(val.strip('"\' '), 1), LOOSER.get(nv.strip('"\' '), 1)
            if nw < ov:
                out.append(Finding("cap-raised", path, nl, f"{key} = {val}", f"{key} = {nv}"))
            continue
        x, y = num(val), num(nv)
        if x is not None and (y is None or y > x):
            out.append(Finding("cap-raised", path, nl, f"[{sec}] {key} = {val}", f"{key} = {nv}"))
    # A cap's mode named for the first time as one that only notifies: the cap no longer stops anything, where the
    # code's default did (budgets-notify, 2026-10-08, made both caps notify by default and wrote this line).
    for k, (nv, nl) in b.items():
        if k not in a and k[1].endswith("_mode") and LOOSER.get(nv.strip('"\' '), 2) <= 1:
            out.append(Finding("cap-raised", path, nl, f"[{k[0]}] {k[1]} (unnamed: the code's default)",
                               f"{k[1]} = {nv}"))


PY_CAP = re.compile(r"max_budget_usd|max_turns|max_loops|MAX_LOOPS|MAX_TURNS|SPEND_LIMIT|spend_limit|BUDGET")


def cap_family(text):
    return "loops" if re.search(r"loops|turns", text, re.I) else "spend"


def cap_lines(src):
    if src is None:
        return []
    return [(n, t.split("#")[0]) for n, t in enumerate(src.split("\n"), 1) if PY_CAP.search(t.split("#")[0])]


def cap_python(tree, paths, out):
    """bench/'s Python caps, judged across its changed files at once, so a default moved to a constant elsewhere
    (an adapter's `"2.0"` become theseus_bench.SPEND_LIMIT_USD) is no finding."""
    head_all = []
    for p in paths:
        head_all.extend(shape_rows(cap_lines(tree.new(p))))
    for path in paths:
        old = shape_rows(cap_lines(tree.old(path)))
        if not old:
            continue
        pool = defaultdict(list)
        for r in shape_rows(cap_lines(tree.new(path))):
            pool[r[0]].append(r)
        for shape, nums, line, text in old:
            if pool.get(shape):
                _, nnums, nline, ntext = pool[shape].pop(0)
                if any(b > a for a, b in zip(nums, nnums)):
                    out.append(Finding("cap-raised", path, nline, text, ntext))
                continue
            # A line that went: a finding unless a line of the same cap (spend, or loops and turns) with a number as
            # low is in the head of a changed file.
            fam = cap_family(PY_CAP.search(text).group(0))
            if not any(cap_family(PY_CAP.search(r[3]).group(0)) == fam and min(r[1]) <= min(nums) for r in head_all):
                out.append(Finding("cap-raised", path, line, text, "(gone or rewritten)"))


def long_files_rule(tree, path, out):
    def entries(src):
        e = {}
        for n, raw in enumerate((src or "").split("\n"), 1):
            parts = raw.split(None, 2)
            if len(parts) >= 2 and not parts[0].startswith("#") and parts[1].isdigit():
                e[parts[0]] = (int(parts[1]), n)
        return e

    a, b = entries(tree.old(path)), entries(tree.new(path))
    for p, (c, line) in b.items():
        if p not in a:
            out.append(Finding("ceiling-raised", path, line, "(no entry)", f"{p} {c}"))
        elif c > a[p][0]:
            out.append(Finding("ceiling-raised", path, line, f"{p} {a[p][0]}", f"{p} {c}"))


def shape_sh_rule(tree, path, out):
    pick = lambda src: [(n, t) for n, t in enumerate((src or "").split("\n"), 1) if re.match(r"\s*limit=\d", t)]
    compare_rows("ceiling-raised", path, shape_rows(pick(tree.old(path))), shape_rows(pick(tree.new(path))), out)


def seconds(v):
    m = re.search(r"(\d+(?:\.\d+)?)\s*(ms|s|m|h)?", v or "")
    if not m:
        return None
    x = float(m.group(1))
    return x * {"ms": 0.001, "s": 1, "m": 60, "h": 3600, None: 1}[m.group(2)]


def nextest_rule(tree, path, out):
    old, new = tree.old(path), tree.new(path)
    if old is None:
        return

    def model(src):
        """Each profile's and override's settings: {name: {setting: (value, line)}}."""
        blocks, cur, counts = {}, "", Counter()
        for n, raw in enumerate((src or "").split("\n"), 1):
            s = raw.split("#")[0].strip()
            if s.startswith("[["):
                name = s.strip("[] ")
                counts[name] += 1
                cur = f"{name}#{counts[name]}"
                blocks[cur] = {"_line": n}
                continue
            if s.startswith("["):
                cur = s.strip("[] ")
                blocks.setdefault(cur, {"_line": n})
                continue
            m = re.match(r"([\w\-]+)\s*=\s*(.+)$", s)
            if not m:
                continue
            k, v = m.group(1), m.group(2)
            b = blocks.setdefault(cur, {"_line": n})
            if k == "slow-timeout":
                pm = re.search(r"period\s*=\s*\"([^\"]+)\"", v) or re.match(r"\"([^\"]+)\"", v)
                tm = re.search(r"terminate-after\s*=\s*(\d+)", v)
                b["slow-period"] = (seconds(pm.group(1)) if pm else None, n, v)
                b["terminate-after"] = (int(tm.group(1)) if tm else None, n, v)
            elif k == "retries":
                cm = re.search(r"count\s*=\s*(\d+)", v) or re.match(r"(\d+)", v)
                b["retries"] = (int(cm.group(1)) if cm else 0, n, v)
            elif k == "leak-timeout":
                lm = re.search(r"period\s*=\s*\"([^\"]+)\"", v) or re.match(r"\"([^\"]+)\"", v)
                b["leak-timeout"] = (seconds(lm.group(1)) if lm else None, n, v)
            elif k == "filter":
                b["filter"] = (v, n, v)
        # Overrides are keyed by their filter, so reordering them is no change.
        keyed = {}
        for name, b in blocks.items():
            if "#" in name and "filter" in b:
                keyed["override " + b["filter"][0]] = b
            else:
                keyed[name] = b
        return keyed

    a, b = model(old), model(new)
    for name, nb in b.items():
        ob = a.get(name)
        if ob is None:
            if name.startswith("override "):
                out.append(Finding("ceiling-raised", path, nb["_line"], "(no override)", name))
            elif "retries" in nb:
                out.append(Finding("ceiling-raised", path, nb["retries"][1], "(none)", f"retries = {nb['retries'][2]}"))
            continue
        for k in ("retries", "slow-period", "leak-timeout", "terminate-after"):
            if k not in nb:
                if k == "terminate-after" and ob.get(k, (None,))[0] is not None and "slow-period" in nb:
                    pass
                continue
            nv, nl, ntext = nb[k]
            ov = ob.get(k, (0 if k == "retries" else None, None, "(none)"))
            if k == "terminate-after":
                if ov[0] is not None and (nv is None or nv > ov[0]):
                    out.append(Finding("ceiling-raised", path, nl, f"slow-timeout = {ov[2]}", f"slow-timeout = {ntext}"))
                continue
            if ov[0] is None or nv is None:
                continue
            if nv > ov[0]:
                out.append(Finding("ceiling-raised", path, nl, f"{k} = {ov[2]}", f"{k} = {ntext}"))


def clippy_toml_rule(tree, path, out):
    old, new = tree.old(path), tree.new(path)
    if old is None:
        return
    a, b = toml_entries(old), toml_entries(new or "")
    for k, (val, line) in a.items():
        if k not in b:
            out.append(Finding("allow-added", path, line, f"{k[1]} = {val}", "(gone)"))
            continue
        nv, nl = b[k]
        x, y = num(val), num(nv)
        if x is not None and y is not None and y > x:
            out.append(Finding("allow-added", path, nl, f"{k[1]} = {val}", f"{k[1]} = {nv}"))
        elif k[1].startswith("allow-") and val.strip() == "false" and nv.strip() == "true":
            out.append(Finding("allow-added", path, nl, f"{k[1]} = {val}", f"{k[1]} = {nv}"))
    for k, (nv, nl) in b.items():
        if k not in a and k[1].startswith("allow-") and nv.strip() == "true":
            out.append(Finding("allow-added", path, nl, "(none)", f"{k[1]} = {nv}"))


def keel_rule(tree, path, out):
    old, new = tree.old(path), tree.new(path)
    if path == "scripts/gate.sh":
        pick = lambda s: [t.strip() for t in (s or "").split("\n") if re.search(r"keel", t, re.I)]
        a, b = pick(old), pick(new)
        if a and a != b:
            line = next((n for n, t in enumerate((new or "").split("\n"), 1)
                         if re.search(r"keel", t, re.I) and t.strip() not in a), 1)
            gone = [t for t in a if t not in b]
            out.append(Finding("keel-file", path, line, gone[0] if gone else "the keel step's lines",
                               next((t for t in b if t not in a), "(gone)")))
        return
    if old is None or old == new:
        return
    if new is None:
        out.append(Finding("keel-file", path, 1, "(the guard's file)", "(gone)"))
        return
    ol, nl = old.split("\n"), new.split("\n")
    line = next((i + 1 for i, (x, y) in enumerate(zip(ol, nl)) if x != y), min(len(ol), len(nl)) + 1)
    out.append(Finding("keel-file", path, line, ol[line - 1] if line <= len(ol) else "(end)",
                       nl[line - 1] if line <= len(nl) else "(end)"))


def judge(tree, paths):
    out = []
    tests_rule(tree, [p for p in paths if p.endswith((".rs", ".py"))], out)
    cap_python(tree, [p for p in paths if p.startswith("bench/") and p.endswith(".py")
                      and not os.path.basename(p).startswith("test_")], out)
    for p in paths:
        if p.endswith("Cargo.toml"):
            lints_rule(tree, p, out)
        if p in BUDGETS or p == "scripts/gate.sh":
            budget_rule(tree, p, out)
        if p.startswith("bench/") and p.endswith(".toml"):
            cap_toml(tree, p, out, commented=False)
        if p == "crates/theseus-core/config/theseus.example.toml":
            cap_toml(tree, p, out, commented=True)
        if p == "scripts/long-files.txt":
            long_files_rule(tree, p, out)
        if p == "scripts/shape.sh":
            shape_sh_rule(tree, p, out)
        if p == ".config/nextest.toml":
            nextest_rule(tree, p, out)
        if p == "clippy.toml":
            clippy_toml_rule(tree, p, out)
        if p in KEEL_FILES or p == "scripts/gate.sh":
            keel_rule(tree, p, out)
    return out


# ---------------------------------------------------------------- the acks

ACK = re.compile(r"^\s*(?P<rule>[\w-]+)\s+(?P<paths>.+?)\s+(?:—|--|-)\s+(?P<why>\S.*)$")


def signers(tree):
    src = tree.old(SIGNERS) or ""
    keys = set()
    for raw in src.split("\n"):
        s = raw.split("#")[0].strip().replace(" ", "").upper()
        if s:
            keys.add(s)
    return keys


def acks(repo, base, head_commit, keys):
    """[(rule, [path globs], sha)] from the range's commits whose signature a listed key made."""
    out = git(repo, "log", "--format=%H%x00%(trailers:key=Keel,valueonly,unfold)%x1e", f"{base}..{head_commit}")
    found, refused = [], []
    for rec in out.split("\x1e"):
        rec = rec.strip("\n")
        if not rec:
            continue
        sha, _, trailers = rec.partition("\0")
        lines = [x for x in trailers.split("\n") if x.strip()]
        if not lines:
            continue
        verdict = git(repo, "log", "-1", "--format=%G?%x00%GF%x00%GP", sha, check=False) or "N\0\0"
        mark, fpr, primary = (verdict.strip("\n").split("\0") + ["", ""])[:3]
        ok = mark in ("G", "U") and keys and (fpr.upper() in keys or primary.upper() in keys)
        for x in lines:
            m = ACK.match(x)
            if not m:
                refused.append((sha, x, "not of the form `<rule> <path>[, <path>…] — <why>`"))
                continue
            if not ok:
                why = "unsigned" if mark == "N" else (
                    f"signed by {fpr or 'an unknown key'}, not a key in {SIGNERS} at the base" if mark in ("G", "U")
                    else f"its signature does not verify (%G? is {mark})")
                refused.append((sha, x, why))
                continue
            paths = [p.strip() for p in m.group("paths").split(",") if p.strip()]
            found.append((m.group("rule"), paths, sha))
    return found, refused


def acked(f, ack_list):
    return any(rule == f.rule and any(fnmatch.fnmatchcase(f.path, p) for p in paths) for rule, paths, _ in ack_list)


def main(argv=None):
    ap = argparse.ArgumentParser(description="The keel guard: erosions of tests, budgets and ceilings in a range.")
    ap.add_argument("--repo", default=".")
    ap.add_argument("--base", default=os.environ.get("THESEUS_KEEL_BASE") or None)
    ap.add_argument("--head", default=None, help="a commit to judge; the working tree when left out")
    ap.add_argument("--quiet", action="store_true", help="print findings only")
    args = ap.parse_args(argv)
    began = time.monotonic()
    try:
        repo = git(args.repo, "rev-parse", "--show-toplevel").strip()
        base, head, head_commit, how = resolve_range(repo, args.base, args.head)
        tree = Tree(repo, base, head)
        try:
            paths = tree.changed()
            findings = judge(tree, paths)
            keys = signers(tree)
            ack_list, refused = acks(repo, base, head_commit, keys)
        finally:
            tree.close()
    except GuardError as e:
        print(f"keel: the guard could not run: {e}", file=sys.stderr)
        return 2
    open_ = [f for f in findings if not acked(f, ack_list)]
    passed = [f for f in findings if acked(f, ack_list)]
    took = time.monotonic() - began
    if not args.quiet:
        for m in MATCHED:
            print(f"keel: the same test: {m}")
    for f in passed:
        print(f"keel: acked: {f}")
    for sha, line, why in refused:
        print(f"keel: refused the ack `Keel: {line}` on {sha[:10]}: {why}")
    if not open_:
        if not args.quiet:
            print(f"keel: ok ({how}, {base[:10]}; {len(paths)} files changed, {len(passed)} findings acked, "
                  f"{took:.1f} s)")
        return 0
    print(f"keel: {len(open_)} erosions of a test, a budget or a ceiling ({how}, {base[:10]}):")
    for f in open_:
        print(f"  {f}")
    groups = defaultdict(list)
    for f in open_:
        if f.path not in groups[f.rule]:
            groups[f.rule].append(f.path)
    print("keel: each passes only with the owner's yes: a commit in the range, signed by a key in "
          f"{SIGNERS} (at the base), whose message carries these trailers, each with its reason:")
    for rule in RULES:
        if rule in groups:
            print(f"  Keel: {rule} {', '.join(groups[rule])} — <why>")
    if not keys:
        print(f"keel: {SIGNERS} lists no key at the base, so no ack can pass yet")
    return 1


if __name__ == "__main__":
    sys.exit(main())
