#!/usr/bin/env python3
"""The keel guard (theseus-pw1q.1): fail a range that deletes or loosens a test, a budget or a ceiling.

Nothing else in the gate stops a branch from passing by removing what would have failed it. This reads the
range's net change, base tree against head tree, and names every erosion in it, one line each:

    <path>:<line>: <rule>: <old> -> <new>

A finding passes only when a commit in the range whose signature verifies against a key in the BASE's
scripts/keel-signers carries a trailer for it:

    Keel: <rule> <path>[, <path>...] — <why>

(`--` or ` - ` may stand for the dash; a path may be a glob). A line whose rule is not one of the rules below, whose
path holds a space, or that is no trailer (a `Keel:` line above the message's last paragraph, which git never reads)
is refused, saying so. An unsigned commit's trailer counts for nothing, so a
branch can't ack itself; nor does one signed with an ssh or x509 key, whatever it verifies against: GPG keys only. The signer list is read from the base, never the head, so a branch can't list its own key.

The range (`THESEUS_KEEL_BASE=<rev>` or `--base` overrides its base, and the gate finds it as below in its own shell,
then passes it as `--base` to the base's copy of this script, so a branch's copy decides neither; `--head` names a commit to judge instead of the
working tree):
  - main is `origin/main`, or the local `main` when there is no `origin/main`: a join's local main is ahead of
    origin/main by what its gate is judging;
  - on a merge commit whose first parent is on main and which is not on origin/main yet (the join's gate):
    HEAD^1..HEAD, the merge judged whole;
  - otherwise merge-base(HEAD, main)..HEAD, so a join whose fix is a commit after its merge is judged whole too;
  - with no `--head`, the head side is the working tree (tracked changes and untracked files), so the gate before a
    commit judges what the commit will hold. Acks are read from the commits in base..HEAD.

The rules, by the name the report and the ack use:
  test-removed    a test function at the base (Rust: #[test], #[tokio::test], #[rstest], #[test_case], any
                  attribute whose last segment is `test`; Python: a `def test_*`; the cockpit's TypeScript, in a
                  test/ directory or a *.test.ts: a `test('name', ...)` or `it(...)`, keyed with its describes'
                  names) that is not at the head under the
                  same path and name. A test that moved file or module keeping its name, or renamed with the same
                  body, is matched and judged by the rules below instead.
  test-ignored    #[ignore] (or a cfg_attr's ignore) added to a test; in Python a skip decorator (unittest's skip,
                  skipIf, skipUnless, expectedFailure, pytest's skip marks) on the test or its class, or a
                  self.skipTest / SkipTest added to its body; in TypeScript test.skip/.todo/.only, a { skip: },
                  { todo: } or { only: } option, or a t.skip( / t.todo( added to its body.
  assert-removed  a test with fewer assertions than at the base (Rust: every `...assert...!` macro, assert!,
                  assert_eq!, debug_assert_ne!, prop_assert!, and a wait that fails the test, wait_for(, wait_until(,
                  .until(, so an assertion made a wait keeps its check; Python: `assert` statements and
                  self.assert*/fail calls, pytest.raises; TypeScript: assert(, assert.*(, expect(). Counted per test,
                  comments left out: a rewritten assertion is not a finding.
  assert-weakened an assertion of a test loosened in place, which keeps the count (theseus-pw1q.18): an equality made a
                  truth test with no comparison in it (assert_eq! to assert!, assertEqual to assertTrue, assert.equal
                  to assert.ok; one that compared with a bool or None is not), a constant bound loosened (raised on the
                  upper side of < or <=, lowered on the lower side; a Duration's or a tolerance's number too; Python's
                  assertLess/assertGreater), an assertion made vacuous (assert!(true)), commented out, or its line
                  made a discard (let _ =, _ =, void).
  allow-added     a #[allow(...)], #![allow(...)] or #[expect(...)] (an expect silences a lint as an allow does:
                  the shape budget's marks) added for a lint on an item that had none, counted per lint and item
                  across the changed files, so a moved item keeps its marks; a lint level lowered or removed in a
                  Cargo.toml [lints] or [workspace.lints] table (or a crate's `workspace = true` dropped); a
                  clippy.toml threshold raised or removed, or an allow-* switch turned on.
  budget-raised   a numeric literal raised, or its line removed, in:
                    crates/theseus-sim/src/lifecycle.rs   fn budget_ms, fn margin_ms, const CANCEL_FRAMES
                    crates/theseus-sim/src/perf.rs        const PLAIN_TURN_FRAMES, TOOL_TURN_FRAMES, BINARY_MB,
                                                          BATCH_OVER_ONE_CALL_MS (the batch turn's allowance)
                    crates/theseus-sim/src/jobs.rs        const START_P95_MS
                    crates/theseus-core/src/first_request.rs  const CLI_TASK_TOKENS, DM_TOKENS, CHANNEL_TOKENS,
                                                          HEADLESS_TOKENS (the first request's token budgets: the
                                                          owner's signed yes, 2026-10-15)
                    scripts/gate.sh                       THESEUS_GATE_BENCH_ALLOWANCE's default
                    crates/theseus-sim/src/**/*.rs but its tests  a const naming BUDGET, MARGIN, SLACK, TOLERANCE or
                                                          ALLOWANCE, across the changed files (one moved keeps its
                                                          value; a new one is no finding); only in the sim crate,
                                                          since elsewhere such names are not bench limits
                  a new arm of a keel fn's match is judged against the base's fall-through for it.
  cap-raised      a spend or loop cap raised, removed, or its mode loosened (end/ask to notify, or a mode named for
                  the first time as notify or off):
                    bench/**/*.toml                       keys naming max_loops, max_turns, spend, budget, usd
                    bench/**/*.py but test_*.py           lines naming max_budget_usd, max_turns, max_loops,
                                                          MAX_LOOPS, SPEND_LIMIT, spend_limit, BUDGET with a number
                    crates/theseus-core/config/theseus.example.toml  keys naming spend, usd, per_day, daily,
                                                          max_loops (commented lines too)
                    any Rust file                         a cap's code default: a `fn default_*` naming usd, spend,
                                                          per_day, daily, day_ceiling, max_loops or max_turns, or a
                                                          const naming CAP/LIMIT/CEILING/BUDGET and USD/MICROS/SPEND/
                                                          LOOPS/TURNS, across the changed files (one moved keeps its
                                                          value; a new one is no finding)
  ceiling-raised  scripts/long-files.txt: a ceiling raised or an entry added; scripts/shape.sh: `limit=` raised;
                  .config/nextest.toml: retries added or raised, slow-timeout's period or terminate-after widened
                  (or terminate-after dropped), leak-timeout widened, an override added (the flaky list given a
                  test).
  ci-loosened     .github/workflows/*.yml: a step's command gone or changed (each `&&`/`;` part of a run, a uses'
                  action without its version, a command:), continue-on-error turned on, an `if:` that is false, or
                  the workflow gone. A step added, or an action's version bumped, is no finding.
  keel-file       any change to the guard itself when it exists at the base: this script, scripts/keel-signers,
                  scripts/test_keel_guard.py, and scripts/gate.sh's lines that name the keel or its door; and a
                  phase of scripts/gate.sh gone (by name). Adding them is not one; adding a key to keel-signers is.
                  The same for the sealed scorer's code, its runner and relay, and the exam set (theseus-pw1q.21.2):
                  the files of KEEL_FILES after the guard's three (`sealed::KEEL_PATHS` in theseus-core lists them
                  too, and a Rust test holds the lists together) and any file under KEEL_PREFIXES
                  (crates/theseus-exam/exam/); there a file added is a finding too.
  sealed-reach    an added line, in a Rust file outside those and outside cfg(test) code, that names the sealed set:
                  `sealed_dir`, `.sealed.dir` (`[self.sealed] dir`), `sealed::` or `Sealed::`. Defence in depth (L1 is
                  the boundary for a candidate's code): how a self branch linked into the daemon could read the set.

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

RULES = ("test-removed", "test-ignored", "assert-removed", "assert-weakened", "allow-added", "budget-raised",
         "cap-raised", "ceiling-raised", "ci-loosened", "keel-file", "sealed-reach")
# The first three are the guard itself; the rest are the sealed scorer's code, its runner and relay, and the exam set,
# which `sealed::KEEL_PATHS` lists too (a Rust test holds the two lists together: theseus-pw1q.21.2).
KEEL_FILES = (
    "scripts/keel-guard.py",
    "scripts/keel-signers",
    "scripts/test_keel_guard.py",
    "crates/theseus-core/src/rsi/sealed.rs",
    "crates/theseus-core/src/rsi/sealed/check.rs",
    "crates/theseus-core/src/rsi/sealed/filter.rs",
    "crates/theseus-core/src/rsi/sealed/daemon.rs",
    "crates/theseus-core/src/rsi/sealed/runner.rs",
    "crates/theseus-core/src/rsi/sealed/relay.rs",
    "crates/theseus-core/src/toolrun/guarded.rs",
    "crates/theseus-core/src/config/self_improve.rs",
    "crates/theseus-core/src/rsi/sealed/tests.rs",
    "crates/theseus-core/src/rsi/sealed/tests_reach.rs",
)
KEEL_PREFIXES = ("crates/theseus-exam/exam/",)
GUARD_ITSELF = KEEL_FILES[:3]
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
    # origin/main when there is one, else main (theseus-pw1q.18). A join's local main is ahead of origin/main by its
    # merge and any fix committed after it, none of which a gate has judged: taking local main there judged the empty
    # range HEAD..HEAD whenever a fix followed the merge (the people join's gate, 2026-10-09).
    remote = rev(repo, "origin/main")
    main = remote or rev(repo, "main")
    if main is None:
        raise GuardError("no main and no origin/main to judge against: set THESEUS_KEEL_BASE=<rev>")
    name = "origin/main" if remote else "main"
    parents = git(repo, "rev-list", "--parents", "-n", "1", head_commit).split()[1:]
    if len(parents) >= 2:
        first = parents[0]
        on_main = git(repo, "merge-base", "--is-ancestor", first, main, check=False) is not None
        # A merge already on origin/main was judged at its join: a branch cut there (its working tree changed, no
        # commit yet) is judged from it, not as the join again with the join's base's signers.
        landed = remote is not None and git(repo, "merge-base", "--is-ancestor", head_commit, remote,
                                            check=False) is not None
        if on_main and not landed:
            return first, head_arg, head_commit, "a merge: HEAD^1..HEAD"
    mb = git(repo, "merge-base", head_commit, main, check=False)
    if not mb:
        raise GuardError(f"HEAD shares no history with {name}: set THESEUS_KEEL_BASE=<rev>")
    return mb.strip(), head_arg, head_commit, f"a branch: merge-base(HEAD, {name})..HEAD"


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
                        "lang": "rs",
                        "code": code,
                        "raw": body,
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
                "lang": "py",
                "code": "\n".join("" if x.strip().startswith("#") else x for x in body.split("\n")),
                "raw": body,
            }
            decos = []
            i = j
            continue
        decos = []
        i += 1
    return tests


# ---------------------------------------------------------------- TypeScript (the cockpit's tests, theseus-pw1q.18)

TS_TOKEN = re.compile(r"""//[^\n]*|/\*.*?\*/|"(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*'|`(?:\\.|[^`\\])*`""", re.S)
TS_TEST = re.compile(r"""^[ \t]*(test|it|describe)(?:\.(skip|todo|only))?\s*\(\s*(['"`])((?:\\.|(?!\3).)*?)\3""", re.M)
TS_ASSERT = re.compile(r"\bassert(?:\.\w+)?\s*\(|\bexpect\s*\(")
TS_SKIP_OPT = re.compile(r"\b(skip|todo|only)\s*:")
TS_SKIP_CALL = re.compile(r"\bt\.(?:skip|todo)\s*\(")
TS_TEST_FILE = re.compile(r"(?:^|/)(?:test|tests|__tests__)/.*\.(?:ts|tsx|mts|js|mjs)$|\.(?:test|spec)\.(?:ts|tsx|mts|js|mjs)$")


def strip_ts_comments(text):
    out, last = [], 0
    for m in TS_TOKEN.finditer(text):
        t = m.group(0)
        if t.startswith("//") or t.startswith("/*"):
            out.append(text[last:m.start()])
            out.append("\n" * t.count("\n"))
            last = m.end()
    out.append(text[last:])
    return "".join(out)


def parse_ts(src):
    """node:test's and the like's tests, keyed by their describes' names and their own: test('name', ...)."""
    code = strip_ts_comments(src)
    calls = []
    for m in TS_TEST.finditer(code):
        open_ = code.index("(", m.start(1))
        calls.append((m, scan_to(code, open_, QUOTES["ts"])))
    describes = [(m.start(), end, m.group(4)) for m, end in calls if m.group(1) == "describe"]
    tests = {}
    for m, end in calls:
        if m.group(1) == "describe":
            continue
        name = m.group(4)
        key = " > ".join([d[2] for d in describes if d[0] < m.start() < d[1]] + [name])
        body = code[m.start():end]
        head = body[:min([x for x in (body.find("=>"), body.find("function")) if x >= 0] or [len(body)])]
        marks = [f".{m.group(2)}"] if m.group(2) else [f"{{ {x.group(1)}: }}" for x in TS_SKIP_OPT.finditer(head)]
        tests[key] = {
            "name": name,
            "line": line_of(code, m.start()),
            "attrs": marks,
            "ignored": bool(marks),
            "skips": len(TS_SKIP_CALL.findall(body)),
            "asserts": len(TS_ASSERT.findall(body)),
            "norm": " ".join(body.replace(m.group(0).strip(), "test(_", 1).split()),
            "head": src.split("\n")[line_of(code, m.start()) - 1].strip(),
            "lang": "ts",
            "code": body,
            "raw": "\n".join(src.split("\n")[line_of(code, m.start()) - 1:line_of(code, end)]),
        }
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
            if TS_TEST_FILE.search(path):
                if "(" not in src:
                    continue
                t = parse_ts(src)
            elif rust:
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
            new = next((a for a in h["attrs"] if is_ignore_attr(a) or PY_SKIP_DECO.match(a) or h.get("lang") == "ts"),
                       "a skip in its body")
            out.append(Finding("test-ignored", hk[0], h["line"], b["head"], f"{new} {h['head']}"))
        if h["asserts"] < b["asserts"]:
            out.append(Finding("assert-removed", hk[0], h["line"],
                               f"{b['head']} ({b['asserts']} assertions)", f"{h['asserts']} assertions"))
        weakened_rule(b, h, hk[0], out)
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


# ---------------------------------------------------------------- weakened assertions (theseus-pw1q.18)
#
# assert-removed counts, so an assertion loosened in place kept its count and passed. A test's assertions at the base
# and at the head are paired in order (difflib over their texts); a pair changed is a finding when the new one is
# weaker: an equality made a truth test with no comparison in it (assert_eq! to assert!, assertEqual to assertTrue,
# assert.equal to assert.ok; not when the equality compared with a bool or None, which clippy itself asks to write
# as assert!), a bound loosened (a number raised on the upper side of `<`/`<=`, or lowered on the lower side, where
# that side is a constant: a number, a Duration of one, a tolerance; Python's assertLess and assertGreater the same),
# or made vacuous (assert!(true)). And on its lines: an assertion commented out (a comment that starts with one, more
# than at the base), or an assertion's line replaced by a discard (`let _ =`, `_ =`, `void`).

QUOTES = {"rs": '"', "py": "\"'", "ts": "\"'`"}
CALLS = {
    "rs": re.compile(r"\b(\w*assert\w*)!\s*[(\[{]"),
    "py": re.compile(r"\bself\.(assert\w*|fail)\s*\(|^[ \t]*(assert)\b", re.M),
    "ts": re.compile(r"\b(assert(?:\.\w+)?)\s*\("),
}
EQ_KINDS = re.compile(
    r"(?:^|_)assert_(?:eq|ne)$|^assert(?:Equal|NotEqual|Is|IsNot|In|NotIn|CountEqual|ListEqual|DictEqual|SetEqual"
    r"|TupleEqual|SequenceEqual|MultiLineEqual|Regex|NotRegex|IsInstance)$"
    r"|^assert\.(?:equal|notEqual|strictEqual|notStrictEqual|deepEqual|notDeepEqual|deepStrictEqual"
    r"|notDeepStrictEqual|match|doesNotMatch)$")
PLAIN_KINDS = re.compile(r"(?:^|_)assert$|^assert(?:True|False|IsNotNone)$|^assert\.ok$")
LITERAL = re.compile(r"^(?:true|false|True|False|None|null|undefined)$")
COMPARES = {"rs": re.compile(r"==|!=|\bmatches!"), "ts": re.compile(r"==|!="),
            "py": re.compile(r"==|!=|\bin\b|\bis\b")}
VACUOUS = {"true", "True", "!false", "not False"}
ORDER_KINDS = {"assertLess": True, "assertLessEqual": True, "assertGreater": False, "assertGreaterEqual": False}
# What a constant bound may hold besides its numbers: a Duration's or a timedelta's constructor, a cast, arithmetic.
BOUND_REST = re.compile(
    r"\b(?:Duration|std|time|from_millis|from_secs|from_micros|from_nanos|from_secs_f64|from_secs_f32|as|f64|f32"
    r"|u64|u32|u128|usize|i64|i32|Some|Ok|timedelta|seconds|milliseconds|microseconds)\b|::|[\s()+\-*/,&]"
    r"|-?\d[\d_]*(?:\.\d+)?(?:e[-+]?\d+)?(?:[a-z_]\w*)?")
COMMENTED = {"rs": re.compile(r"^\s*//+\s*\w*assert\w*!\s*[(\[{]"),
             "py": re.compile(r"^\s*#\s*(?:assert\s+\S|self\.assert\w*\s*\()"),
             "ts": re.compile(r"^\s*//+\s*assert(?:\.\w+)?\s*\(")}
DISCARD = {"rs": re.compile(r"^\s*let\s+_\s*="), "py": re.compile(r"^\s*_\s*="), "ts": re.compile(r"^\s*void\s")}


def scan_to(text, i, quotes, stop_at_newline=False):
    """The index just past the bracket closing text[i] (or, with stop_at_newline, of the first newline outside any
    bracket from i), strings and backslash escapes skipped."""
    depth, j, q = 0, i, None
    while j < len(text):
        ch = text[j]
        if ch == "\\":
            j += 2
            continue
        if q:
            if ch == q:
                q = None
        elif ch in quotes:
            q = ch
        elif ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
            if depth == 0 and not stop_at_newline:
                return j + 1
        elif ch == "\n" and stop_at_newline and depth <= 0:
            return j
        j += 1
    return len(text)


def split_args(text, quotes):
    out, depth, q, last, j = [], 0, None, 0, 0
    while j < len(text):
        ch = text[j]
        if ch == "\\":
            j += 2
            continue
        if q:
            if ch == q:
                q = None
        elif ch in quotes:
            q = ch
        elif ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
        elif ch == "," and depth == 0:
            out.append(text[last:j])
            last = j + 1
        j += 1
    out.append(text[last:])
    return [" ".join(a.split()) for a in out if a.strip()]


def assertions(t):
    """[(kind, [args], pos)] of a test's assertions, in order (pos is in t["code"])."""
    code, lang = t["code"], t["lang"]
    quotes, out = QUOTES[lang], []
    for m in CALLS[lang].finditer(code):
        if lang == "py" and m.group(2):
            end = scan_to(code, m.end(), quotes, stop_at_newline=True)
            out.append(("assert", split_args(code[m.end():end], quotes), m.start(2)))
            continue
        end = scan_to(code, m.end() - 1, quotes)
        out.append((m.group(1), split_args(code[m.end():end - 1], quotes), m.start()))
    return out


def top_split(expr, quotes, sep):
    """expr split at the top-level, spaced operators `sep` matches, and the operators."""
    parts, ops, depth, q, last, j = [], [], 0, None, 0, 0
    while j < len(expr):
        ch = expr[j]
        if ch == "\\":
            j += 2
            continue
        if q:
            if ch == q:
                q = None
        elif ch in quotes:
            q = ch
        elif ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
        elif ch == " " and depth == 0:
            m = sep.match(expr, j)
            if m:
                parts.append(expr[last:j])
                ops.append(m.group(1))
                last = j = m.end()
                continue
        j += 1
    parts.append(expr[last:])
    return parts, ops


CMP_OP = re.compile(r" (<=|>=|<|>) ")
LOGIC_OP = re.compile(r" (&&|\|\||and|or) ")


def comparisons(expr, quotes):
    """[(operands, operators)] of each top-level `&&`/`||`/`and`/`or` part's spaced `<`, `<=`, `>`, `>=`."""
    return [top_split(part, quotes, CMP_OP) for part in top_split(expr, quotes, LOGIC_OP)[0]]


def is_bound(operand):
    return bool(NUM.search(operand)) and not BOUND_REST.sub("", operand).strip()


def loosened(kind, old, new, quotes):
    """old -> new, said, when a bound in it was loosened."""
    if NUM.sub("#", " ".join(old)) != NUM.sub("#", " ".join(new)):
        return None
    sides = []  # (old operand, new operand, is it the upper bound's side)
    if kind in ORDER_KINDS and len(old) >= 2:
        less = ORDER_KINDS[kind]
        sides = [(old[0], new[0], not less), (old[1], new[1], less)]
    elif old:
        co, cn = comparisons(old[0], quotes), comparisons(new[0], quotes)
        if len(co) != len(cn):
            return None
        for (po, ops), (pn, opn) in zip(co, cn):
            if ops != opn:
                continue
            for k in range(len(po)):
                upper = (k > 0 and ops[k - 1] in ("<", "<=")) or (k < len(ops) and ops[k] in (">", ">="))
                lower = (k > 0 and ops[k - 1] in (">", ">=")) or (k < len(ops) and ops[k] in ("<", "<="))
                if upper != lower:
                    sides.append((po[k], pn[k], upper))
    for a, b, upper in sides:
        if not (is_bound(a) and is_bound(b)):
            continue
        for x, y in zip([num(v) for v in NUM.findall(a)], [num(v) for v in NUM.findall(b)]):
            if x is not None and y is not None and (y > x if upper else y < x):
                return f"{a} -> {b} ({'an upper' if upper else 'a lower'} bound)"
    return None


def weaker(o, n, lang):
    """Why assertion n is weaker than o, its counterpart at the base, or None."""
    (ok, oa, _), (nk, na, _) = o, n
    if na and na[0] in VACUOUS and not (oa and oa[0] in VACUOUS):
        return "made vacuous"
    if EQ_KINDS.search(ok) and PLAIN_KINDS.search(nk):
        if any(LITERAL.match(a) for a in oa[:2]) or (na and COMPARES[lang].search(na[0])):
            return None
        return "an equality made a truth test"
    if ok == nk:
        why = loosened(ok, oa, na, QUOTES[lang])
        if why:
            return "a bound loosened: " + why
    return None


def align(olds, news, text):
    """The pairs of a changed block of assertions, in order, most alike first: an assertion added among changed ones
    pairs with nothing, never with its neighbour's counterpart (the work-types join, 4ffcc282, added an assert! among
    assert_eq!s whose arguments all changed). Only pairs at least half alike count."""
    score = [[difflib.SequenceMatcher(None, text(o), text(n), autojunk=False).ratio() for n in news] for o in olds]
    best = [[0.0] * (len(news) + 1) for _ in range(len(olds) + 1)]
    for i in range(len(olds) - 1, -1, -1):
        for j in range(len(news) - 1, -1, -1):
            take = score[i][j] + best[i + 1][j + 1] if score[i][j] >= SIMILAR else -1.0
            best[i][j] = max(take, best[i + 1][j], best[i][j + 1])
    pairs, i, j = [], 0, 0
    while i < len(olds) and j < len(news):
        if score[i][j] >= SIMILAR and best[i][j] == score[i][j] + best[i + 1][j + 1]:
            pairs.append((olds[i], news[j]))
            i, j = i + 1, j + 1
        elif best[i][j] == best[i + 1][j]:
            i += 1
        else:
            j += 1
    return pairs


def weakened_rule(b, h, path, out):
    if "lang" not in b or b.get("lang") != h.get("lang"):
        return
    lang = h["lang"]
    olds, news = assertions(b), assertions(h)
    text = lambda a: f"{a[0]}({', '.join(a[1])})"
    sm = difflib.SequenceMatcher(None, [text(a) for a in olds], [text(a) for a in news], autojunk=False)
    for tag, i1, i2, j1, j2 in sm.get_opcodes():
        if tag != "replace":
            continue
        for o, n in align(olds[i1:i2], news[j1:j2], text):
            why = weaker(o, n, lang)
            if why:
                line = h["line"] + h["code"].count("\n", 0, n[2])
                out.append(Finding("assert-weakened", path, line, text(o), f"{text(n)} ({why})"))
    # On its lines: an assertion commented out, or its line made a discard.
    braw, hraw = b["raw"].split("\n"), h["raw"].split("\n")
    gone = [i for i, x in enumerate(hraw) if COMMENTED[lang].match(x)]
    if len(gone) > sum(1 for x in braw if COMMENTED[lang].match(x)):
        i = next((i for i in gone if hraw[i].strip() not in {x.strip() for x in braw}), gone[-1])
        out.append(Finding("assert-weakened", path, h["line"] + i, "an assertion", f"{hraw[i].strip()} (commented out)"))
    sm = difflib.SequenceMatcher(None, [x.strip() for x in braw], [x.strip() for x in hraw], autojunk=False)
    for tag, i1, i2, j1, j2 in sm.get_opcodes():
        if tag != "replace" or not any(CALLS[lang].search(x) for x in braw[i1:i2]):
            continue
        for j in range(j1, j2):
            if DISCARD[lang].match(hraw[j]):
                old = next(x.strip() for x in braw[i1:i2] if CALLS[lang].search(x))
                out.append(Finding("assert-weakened", path, h["line"] + j, old, f"{hraw[j].strip()} (discarded)"))
                break


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
                                       ("const", "BINARY_MB"), ("const", "BATCH_OVER_ONE_CALL_MS")],
    "crates/theseus-sim/src/jobs.rs": [("const", "START_P95_MS")],
    # The first request's budgets in tokens (theseus-to0xw): raising one is the owner's signed change (his "Yes!" of
    # 2026-10-15 09:50 to "make any raise of the 8,000 / 15,000 limits need your signed OK").
    "crates/theseus-core/src/first_request.rs": [("const", "CLI_TASK_TOKENS"), ("const", "DM_TOKENS"),
                                                 ("const", "CHANNEL_TOKENS"), ("const", "HEADLESS_TOKENS")],
}


# One arm's head: `"a" | "b"`, an optional `if guard` (on the pattern's line or the next, as rustfmt breaks a long
# one), then `=>`; or the fall-through `_ =>`. A head starts a line or follows the comma that ends an arm, so two arms on
# one line are both read. The text is read whole, so a head, a guard or a value may span lines.
ARM_HEAD = re.compile(
    r'(?m)(?:^|(?<=,))[ \t]*(?:(?P<pats>"[^"]*"(?:\s*\|\s*"[^"]*")*)(?:\s+if\s+(?P<guard>(?:(?!=>)[^;{}])+?))?|(?P<fall>_))\s*=>')
RS_STRING = re.compile(r'"(?:\\.|[^"\\])*"|\'(?:\\[^\']*|[^\'\\])\'')
# Words a value may use without naming a constant: the wrapper, the types, the keywords.
VALUE_WORDS = {"Some", "None", "Ok", "Err", "as", "if", "else", "true", "false", "f64", "f32", "u64", "u32", "u16",
               "u8", "usize", "i64", "i32", "isize", "cfg", "debug_assertions", "min", "max"}


def arm_value_end(text, pos):
    """Where the value that starts at `pos` ends: at its comma at the arm's own depth, at the close of a leading
    `{ ... }` block, or at the bracket that closes the match (not consumed)."""
    depth, i, n = 0, pos, len(text)
    while i < n and text[i].isspace():
        i += 1
    block = i < n and text[i] == "{"
    while i < n:
        m = RS_STRING.match(text, i)
        if m:
            i = m.end()
            continue
        c = text[i]
        if c in "([{":
            depth += 1
        elif c in ")]}":
            if depth == 0:
                return i
            depth -= 1
            if depth == 0 and block and c == "}":
                # The block is the whole value unless a method call or `?` follows it: `{ x }.min(Some(2.0))`.
                rest = text[i + 1:].lstrip()
                if rest[:1] not in (".", "?"):
                    return i + 1
                block = False
        elif c == "," and depth == 0:
            return i
        i += 1
    return n


def squash(value):
    """A value without the whitespace and trailing commas a reformat moves, outside its string literals."""
    out, last = [], 0
    for m in RS_STRING.finditer(value):
        out += [re.sub(r"\s+", "", value[last:m.start()]), m.group()]
        last = m.end()
    out.append(re.sub(r"\s+", "", value[last:]))
    return "".join(re.sub(r",(?=[)\]}])", "", part) if k % 2 == 0 else part for k, part in enumerate(out))


def unguarded(pat):
    """`"a" if cond` -> `"a"`: the arm's key without its guard."""
    return re.match(r'"[^"]*"', pat).group()


def names_constant(expr):
    """True when a value with no number names something (a constant, a local, a call) that holds its number."""
    words = re.findall(r"[A-Za-z_]\w*", RS_STRING.sub('""', expr))
    return any(w not in VALUE_WORDS for w in words)


def match_arms(lines):
    """A fn's `match` over string literals, read as {pattern: (numbers, line, text, value)} and the fall-through's
    (numbers, line, text, value), with the lines the arms cover; None when it has no `_ =>` arm. A pattern keeps its
    guard in its name (`"a" if cond`), so a changed guard is a different arm; an arm whose head or value spans lines
    is read whole, to its comma or closing bracket at its own depth, and an arm inside a value is part of it."""
    text = "\n".join(t for _, t in lines)
    nos = [n for n, _ in lines]
    arms, rows, fall, pos = {}, set(), None, 0
    while True:
        m = ARM_HEAD.search(text, pos)
        if not m:
            break
        end = arm_value_end(text, m.end())
        pos = max(end, m.end())
        value = " ".join(text[m.end():end].split())
        nums = [num(x) for x in NUM.findall(value)]
        head = m.start("pats") if m.group("pats") else m.start("fall")
        first = text.count("\n", 0, head)
        last = text.count("\n", 0, max(len(text[:end].rstrip()) - 1, head))
        shown = " ".join(text[head:end].split())
        if m.group("fall"):
            fall = (nums, nos[first], shown + " (the fall-through)", value)
            continue
        rows.update(nos[first:last + 1])
        guard = " if " + " ".join(m.group("guard").split()) if m.group("guard") else ""
        for pat in re.findall(r'"[^"]*"', m.group("pats")):
            arms[pat + guard] = (nums, nos[first], shown, value)
    return (arms, fall, rows) if fall else None


def compare_arms(path, base, head, out):
    """Each pattern's value at the head against its value at the base: the arm that names it, else the fall-through.
    A head arm is judged on the patterns it names, so a new arm is judged against the fall-through it replaced and a
    split or merged arm keeps its value; a base arm with a pattern no numbered head arm names is gone, as before. A
    pattern the base gave no number is unbudgeted there: a new budget for it is no finding, unless its value named a
    constant (`Some(surfaces::BUDGET_MS)`): that arm, rewritten to another value, is a finding, and so is one gone."""
    (barms, bfall, _), (harms, _, _) = base, head
    for line in sorted({v[1] for v in harms.values()}):
        for p in [p for p, v in harms.items() if v[1] == line]:
            new, _, ntext, nval = harms[p]
            # A pattern the base names is judged by its own arm. One it does not name is judged by the fall-through
            # it replaced and, when it is guarded, first by the unguarded arm it now shadows (rustc takes the first).
            shadowed = barms.get(unguarded(p))
            judges = [(barms[p], True)] if p in barms else [(shadowed, True), (bfall, False)]
            hit = next((otext for (old, _, otext, oval), named in (j for j in judges if j[0])
                        if (old and any(b > a for a, b in zip(old, new)))
                        or (named and not old and names_constant(oval) and squash(oval) != squash(nval))), None)
            if hit:
                out.append(Finding("budget-raised", path, line, hit, ntext))
                break
    for line in sorted({v[1] for v in barms.values()}):
        if any((barms[p][0] and not harms.get(p, ([],))[0]) or (not barms[p][0] and names_constant(barms[p][3])
                                                                 and p not in harms)
               for p, v in barms.items() if v[1] == line):
            out.append(Finding("budget-raised", path, line, next(v[2] for v in barms.values() if v[1] == line),
                               "(gone or rewritten)"))


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
        base, head = (match_arms(old), match_arms(new)) if kind == "fn" else (None, None)
        if base and head:
            compare_arms(path, base, head, out)
            old, new = [r for r in old if r[0] not in base[2]], [r for r in new if r[0] not in head[2]]
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


# A cap's code default (theseus-pw1q.18): the value a spend or loop cap has when no config names it, in the Rust that
# defines it: a `fn default_*` whose name says a spend or loop cap (serde's defaults), or a constant whose name says a
# cap and its unit. Judged across the changed Rust files, so one moved keeps its value; a new one is no finding.
CAP_DEFAULT_FN = re.compile(r"\bfn\s+(default_\w*(?:usd|spend|per_day|daily|day_ceiling|max_loops|max_turns)\w*)\s*\(")
CAP_CONST = re.compile(r"\bconst\s+([A-Z0-9_]*(?:CAP|LIMIT|CEILING|BUDGET)[A-Z0-9_]*)\s*:")
CAP_UNIT = re.compile(r"USD|MICROS|SPEND|LOOPS|TURNS")


def cap_items(src):
    if not src or ("default_" not in src and "const" not in src):
        return []
    code = strip_rs_comments(src)
    items = [("fn", m.group(1)) for m in CAP_DEFAULT_FN.finditer(code)]
    return items + [("const", m.group(1)) for m in CAP_CONST.finditer(code) if CAP_UNIT.search(m.group(1))]


# The sim crate's budget terms by name (theseus-qfohm): a const naming BUDGET, MARGIN, SLACK, TOLERANCE or ALLOWANCE in
# crates/theseus-sim/src/ outside its tests, judged as cap_code_rule judges a cap's. Only there: elsewhere such names
# (a token's REFRESH_MARGIN) are not bench limits, and raising some of them tightens.
SIM_BUDGET_CONST = re.compile(r"\bconst\s+([A-Z0-9_]*(?:BUDGET|MARGIN|SLACK|TOLERANCE|ALLOWANCE)[A-Z0-9_]*)\s*:")


def sim_budget_items(src):
    return [("const", m.group(1)) for m in SIM_BUDGET_CONST.finditer(strip_rs_comments(src or ""))]


def cap_code_rule(tree, paths, out, rule="cap-raised", items=cap_items):
    base, head = defaultdict(list), defaultdict(list)
    for p in paths:
        for kind, name in items(tree.old(p)):
            base[name].append((p, kind))
        for kind, name in items(tree.new(p)):
            head[name].append((p, kind))
    for name, places in base.items():
        for p, kind in places:
            old = shape_rows(rust_item_lines(tree.old(p), kind, name))
            if not old:
                continue
            there = [x for x in head.get(name, []) if x[0] == p] or head.get(name, [])
            if not there:
                out.append(Finding(rule, p, old[0][2], f"{kind} {name}: {old[0][3]}", "(gone)"))
                continue
            hp, hk = there[0]
            compare_rows(rule, hp, old, shape_rows(rust_item_lines(tree.new(hp), hk, name)), out)


# CI's workflows (theseus-pw1q.18): a step's command (each `&&`/`;` part of a run, a uses' action, a command:) gone or
# changed, continue-on-error turned on, or an `if:` that is false. A step added, or an action's version, is no finding.
CI_FILE = re.compile(r"^\.github/workflows/[^/]+\.ya?ml$")


def ci_model(src):
    items, lines, flags = {}, src.split("\n"), Counter()
    i = 0
    while i < len(lines):
        raw = lines[i]
        s = "" if raw.strip().startswith("#") else re.sub(r"\s+#.*$", "", raw).strip()
        m = re.match(r"(?:-\s*)?(run|uses|command|continue-on-error|if):\s*(.*)$", s)
        i += 1
        if not m:
            continue
        key, val, line = m.group(1), m.group(2).strip(), i
        if key == "continue-on-error":
            flags["continue-on-error"] += val == "true"
            continue
        if key == "if":
            flags["if: false"] += bool(re.fullmatch(r"(?:\$\{\{\s*)?false(?:\s*\}\})?", val))
            continue
        if key == "run" and val in ("|", ">", "|-", ">-", "|+", ">+"):
            ind, block = indent_of(raw), []
            while i < len(lines) and (not lines[i].strip() or indent_of(lines[i]) > ind):
                block.append(lines[i].strip())
                i += 1
            val = "\n".join(block)
        if key == "uses":
            items.setdefault(f"uses {val.split('@')[0]}", line)
        elif key == "command":
            items.setdefault(f"command {' '.join(val.split())}", line)
        else:
            for part in re.split(r"&&|;|\n", val):
                if part.strip():
                    items.setdefault(f"run {' '.join(part.split())}", line)
    return items, flags


def ci_rule(tree, path, out):
    old, new = tree.old(path), tree.new(path)
    if old is None:
        return
    if new is None:
        out.append(Finding("ci-loosened", path, 1, "(the workflow)", "(gone)"))
        return
    (a, fa), (b, fb) = ci_model(old), ci_model(new)
    for item, line in a.items():
        if item not in b:
            out.append(Finding("ci-loosened", path, line, item, "(gone or changed)"))
    for flag in ("continue-on-error", "if: false"):
        if fb[flag] > fa[flag]:
            out.append(Finding("ci-loosened", path, 1, f"{fa[flag]} {flag}", f"{fb[flag]} {flag}"))


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


GATE_PHASE = re.compile(r"""^\s*(?:if .*; then\s+)?phase\s+("[^"]+"|'[^']+'|\S+)""")


def keel_rule(tree, path, out):
    old, new = tree.old(path), tree.new(path)
    if path == "scripts/gate.sh":
        # The keel's and the door's lines (theseus-pw1q.18: the door is the keel's), any change to them.
        pick = lambda s: [t.strip() for t in (s or "").split("\n") if re.search(r"keel|door", t, re.I)]
        a, b = pick(old), pick(new)
        if a and a != b:
            line = next((n for n, t in enumerate((new or "").split("\n"), 1)
                         if re.search(r"keel|door", t, re.I) and t.strip() not in a), 1)
            gone = [t for t in a if t not in b]
            out.append(Finding("keel-file", path, line, gone[0] if gone else "the keel step's lines",
                               next((t for t in b if t not in a), "(gone)")))
        # A phase of the gate gone (by its name): a check the gate no longer makes. A phase's command changed, or
        # one added, is no finding.
        names = lambda s: {m.group(1): n for n, t in enumerate((s or "").split("\n"), 1)
                           for m in [GATE_PHASE.match(t)] if m}
        before, after = names(old), names(new)
        for name, line in before.items():
            if name not in after:
                out.append(Finding("keel-file", path, line, f"phase {name}", "(gone)"))
        return
    if old is None and new is not None and path not in GUARD_ITSELF:
        # The sealed scorer's code and the exam set: a file added there is the owner's too (the runner and the relay
        # are born under this rule).
        out.append(Finding("keel-file", path, 1, "(none)", "(added)"))
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


# The sealed set's reach (theseus-pw1q.21.2). Defence in depth: L1 is the boundary for running a candidate's code, and
# the in-daemon reach (a self branch's code linked into the daemon) is what this watches. A line added outside the
# keel files and outside cfg(test) code that names the sealed dir's config (`sealed_dir`, `.sealed.dir` for
# `[self.sealed] dir`) or the sealed module or type (`sealed::`, `Sealed::`: its `open`, `guard_of`, `Sealed::new`,
# `put`, `score`, `counts`, `Guard`) is how such a branch could quietly read the set. Comments and strings are not
# code; a hit moved within a file is no new hit.
SEALED_REACH = re.compile(r"\bsealed_dir\b|\.sealed\.dir\b|\b[Ss]ealed::")
CFG_TEST = re.compile(r"#(!?)\[\s*cfg\(\s*test\s*\)\s*\]")
TEST_FILE = re.compile(r"(^|/)(tests?(_\w+)?\.rs|tests/[^/]+\.rs)$")


def code_only(src):
    """src with comments and string literals blanked, lines kept."""
    out, last = [], 0
    for m in RUST_TOKEN.finditer(src):
        t = m.group(0)
        if t.startswith(("//", "/*", '"', "b\"", "r", "br")) and not m.group("attr"):
            out.append(src[last:m.start()])
            out.append(re.sub(r"[^\n]", " ", t))
            last = m.end()
    out.append(src[last:])
    return "".join(out)


def reach_hits(path, src):
    """Counter of the normalized lines of src's code that name the sealed set, outside cfg(test) items."""
    hits = {}
    if not src or TEST_FILE.search(path):
        return hits
    code = code_only(src)
    skip = []
    for m in CFG_TEST.finditer(code):
        if m.group(1):
            return hits  # #![cfg(test)]: the whole file
        i = m.end()
        while True:  # past any further attributes, to the item's first `{` or `;`
            i = re.compile(r"\s*").match(code, i).end()
            if not code.startswith("#[", i):
                break
            depth = 0
            while i < len(code):
                depth += {"[": 1, "]": -1}.get(code[i], 0)
                i += 1
                if depth == 0:
                    break
        j = re.compile(r"[{;]").search(code, i)
        if not j:
            continue
        end, depth = j.end(), 1 if j.group() == "{" else 0
        while depth and end < len(code):
            depth += {"{": 1, "}": -1}.get(code[end], 0)
            end += 1
        skip.append((m.start(), end))
    for m in SEALED_REACH.finditer(code):
        if any(a <= m.start() < b for a, b in skip):
            continue
        n = line_of(code, m.start())
        hits.setdefault(" ".join(src.split("\n")[n - 1].split()), []).append(n)
    return hits


def sealed_reach_rule(tree, paths, out):
    for p in paths:
        if p in KEEL_FILES:
            continue  # already the owner's, as a keel-file
        before, after = reach_hits(p, tree.old(p)), reach_hits(p, tree.new(p))
        for text, lines in sorted(after.items(), key=lambda kv: kv[1]):
            if len(lines) > len(before.get(text, [])):
                out.append(Finding("sealed-reach", p, lines[len(before.get(text, []))], "(none)", text))


def judge(tree, paths):
    out = []
    tests_rule(tree, [p for p in paths if p.endswith((".rs", ".py")) or TS_TEST_FILE.search(p)], out)
    cap_code_rule(tree, [p for p in paths if p.endswith(".rs")], out)
    cap_code_rule(tree, [p for p in paths if p.startswith("crates/theseus-sim/src/") and p.endswith(".rs")
                         and not TEST_FILE.search(p) and "/tests/" not in p], out, "budget-raised", sim_budget_items)
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
        if CI_FILE.match(p):
            ci_rule(tree, p, out)
        if p in KEEL_FILES or p.startswith(KEEL_PREFIXES) or p == "scripts/gate.sh":
            keel_rule(tree, p, out)
    sealed_reach_rule(tree, [p for p in paths if p.endswith(".rs")], out)
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


# A signature's kind, from the armour in the commit's gpgsig header. The keel takes a GPG (OpenPGP) key's only
# (theseus-pw1q.18): scripts/keel-signers lists GPG fingerprints, and an ssh signature verifies against whatever
# gpg.ssh.allowedSignersFile the machine names, with its SHA256 fingerprint as %GF; an x509 one against gpgsm's keys.
SIG_KINDS = {"PGP SIGNATURE": "gpg", "SSH SIGNATURE": "ssh", "SIGNED MESSAGE": "x509"}


def signature_kind(repo, sha):
    head = (git(repo, "cat-file", "commit", sha, check=False) or "").split("\n\n", 1)[0]
    m = re.search(r"^gpgsig(?:-sha256)? -----BEGIN ([A-Z0-9 ]+)-----", head, re.M)
    return SIG_KINDS.get(m.group(1), m.group(1).lower()) if m else None


def ack_fault(m):
    """What makes a well-formed ack line name nothing: a rule that is not one, or a path with a space in it."""
    if m.group("rule") not in RULES:
        return f"`{m.group('rule')}` is not a rule (the rules: {', '.join(RULES)})"
    spaced = next((p.strip() for p in m.group("paths").split(",") if re.search(r"\s", p.strip())), None)
    return spaced and f"a path holds a space: `{spaced}` (paths are comma-separated)"


def acks(repo, base, head_commit, keys):
    """[(rule, [path globs], sha)] from the range's commits whose signature a listed key made."""
    out = git(repo, "log", "--format=%H%x00%(trailers:key=Keel,valueonly,unfold)%x00%B%x1e", f"{base}..{head_commit}")
    found, refused = [], []
    for rec in out.split("\x1e"):
        rec = rec.strip("\n")
        if not rec:
            continue
        sha, _, rest = rec.partition("\0")
        trailers, _, body = rest.partition("\0")
        lines = [x for x in trailers.split("\n") if x.strip()]
        # A Keel: line above the message's last paragraph is no trailer: git never reads it (94eb8a7d's was one).
        for x in re.findall(r"(?m)^Keel:[ \t]*(\S.*?)\s*$", body):
            if not any(t.strip().startswith(x) for t in lines):
                fault = ACK.match(x) and ack_fault(ACK.match(x))
                refused.append((sha, x, "not a trailer: git reads trailers from the message's last paragraph only "
                                        "(put the Keel: lines beside Co-Authored-By, with no blank line between)"
                                        + (f"; and {fault}" if fault else "")))
        if not lines:
            continue
        verdict = git(repo, "log", "-1", "--format=%G?%x00%GF%x00%GP", sha, check=False) or "N\0\0"
        mark, fpr, primary = (verdict.strip("\n").split("\0") + ["", ""])[:3]
        kind = signature_kind(repo, sha)
        ok = kind == "gpg" and mark in ("G", "U") and keys and (fpr.upper() in keys or primary.upper() in keys)
        for x in lines:
            m = ACK.match(x)
            if not m:
                refused.append((sha, x, "not of the form `<rule> <path>[, <path>…] — <why>`"))
                continue
            if not ok and kind not in (None, "gpg"):
                refused.append((sha, x, f"an {kind} signature: the keel takes a GPG key's only ({SIGNERS} lists GPG "
                                        "fingerprints)"))
                continue
            if not ok:
                why = "unsigned" if mark == "N" else (
                    f"signed by {fpr or 'an unknown key'}, not a key in {SIGNERS} at the base" if mark in ("G", "U")
                    else f"its signature does not verify (%G? is {mark})")
                refused.append((sha, x, why))
                continue
            if ack_fault(m):
                refused.append((sha, x, ack_fault(m)))
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
