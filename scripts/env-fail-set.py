#!/usr/bin/env python3
"""The cloud routine VM's environmental-failure set, derived from the tree (theseus-4vsrz, 1.0.5 B7).

WHICH HOST THIS DESCRIBES, because getting it wrong discounts a real red: the **4-core cloud routine VM**,
15 GB of RAM, no swap, running as **root**. There, `RLIMIT_NPROC` behaves differently and the sandbox cannot
build the view it asserts about, so every case in the three places below fails for a reason that is not the
branch's. **It does NOT describe a fleet box.** A fleet gate is a `c7i.8xlarge` and `ec2.sh run <lane> gate`
starts the gate in a delegated `systemd --user` unit, which is what the cgroup and sandbox tests need: a
fleet gate comes back with `clause_01` to `clause_12` green and `reds:` empty. Anything written about this
set must name its host or it will mislead the next reader into waving away a real failure.

WHY THIS SCRIPT EXISTS. The set is a hand-maintained number in the cloud sessions' prompt preamble, and the
count has been wrong in four successive briefs: four, then thirty-five, then forty, then thirty-six. Each
wrong count cost a session real time, either chasing an environmental failure or - far worse - filing a real
one as environmental. The count was wrong every time for the same reason: it is an enumeration of the tree
that nothing ties to the tree, so it goes stale the moment a test lands. This script ties it to the tree.
Run it when a wave's preamble is written, and paste `--markdown`'s block instead of editing the number:

    python3 scripts/env-fail-set.py                 # the report, with every name
    python3 scripts/env-fail-set.py --markdown      # the preamble's block, ready to paste
    python3 scripts/env-fail-set.py --names         # the bare names, one per line
    python3 scripts/env-fail-set.py --expect 36     # exit 1 when the tree no longer totals 36

WHAT IT IS NOT. It derives the set from the SOURCES, which is a count of the cases in the three places, not a
measurement of what fails on that VM. The two are the same number only while the stated cause holds (the
sandbox cannot build its view, so every case in those places fails). That is the preamble's own claim, and it
is what makes the set derivable at all; if a case in these places ever passes on that VM, the cause has
changed and the preamble needs a measurement, not this script. **So this is not a gate.** It is deliberately
not wired into scripts/gate.sh: an `--expect` in the gate would fail a branch over a host's measured number
that nobody has recalibrated, which is the second-threshold mistake line-pressure.py's phase exists to avoid.

THE THREE PLACES, and why they need three different parsers - the third reason the hand count drifts:
  - `crates/theseus-sandbox/tests/contract.rs` - `harness = false`, a custom harness; its cases are the
    `Case { name: "..." }` entries of `const CASES`. `const IGNORED`'s entries run only when asked
    (`--ignored`), so they are NOT in the set and are listed apart.
  - `crates/theseus-sandbox/tests/bench.rs` - the same harness, its cases inline in `fn main()`.
  - `crates/theseusd/tests/sandbox.rs` - an ordinary nextest file: `#[test]` and `#[tokio::test]` functions.
    An `#[ignore]`d one does not run, so it is listed apart too.
"""
import argparse
import os
import re
import sys

# Each place: the path, and which parser reads it. The order is the preamble's.
PLACES = [
    ("crates/theseus-sandbox/tests/contract.rs", "theseus-sandbox::contract", "harness"),
    ("crates/theseus-sandbox/tests/bench.rs", "theseus-sandbox::bench", "harness"),
    ("crates/theseusd/tests/sandbox.rs", "theseusd::sandbox", "nextest"),
]
CASE_NAME = re.compile(r'Case\s*\{[^{}]*?name:\s*"([^"]+)"', re.S)
TEST_ATTR = re.compile(r"^\s*#\[(?:tokio::)?test\b")
IGNORE_ATTR = re.compile(r"^\s*#\[ignore\b")
FN_NAME = re.compile(r"^\s*(?:pub\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)")


def ignored_span(src):
    """The byte span of `const IGNORED: &[Case] = &[ ... ];`, or None. Found by matching the brackets from
    its `&[`, so a `]` inside one of its strings cannot end it early."""
    m = re.search(r"const\s+IGNORED\s*:[^=]*=\s*&\[", src)
    if not m:
        return None
    depth, i = 1, m.end()
    while i < len(src) and depth:
        if src[i] == "[":
            depth += 1
        elif src[i] == "]":
            depth -= 1
        i += 1
    return (m.start(), i)


def harness_cases(src):
    """(cases, ignored) for a `harness = false` binary: the `Case { name: "..." }` entries outside
    `const IGNORED`, and those inside it."""
    span = ignored_span(src)
    cases, ignored = [], []
    for m in CASE_NAME.finditer(src):
        inside = span is not None and span[0] <= m.start() < span[1]
        (ignored if inside else cases).append(m.group(1))
    return cases, ignored


def nextest_cases(src):
    """(cases, ignored) for an ordinary test file: each `#[test]`/`#[tokio::test]` function, by the name of
    the first `fn` under its attribute block. An `#[ignore]` anywhere in that block takes it out of the set."""
    lines = src.split("\n")
    cases, ignored, armed, skip = [], [], False, False
    for line in lines:
        if TEST_ATTR.match(line):
            armed = True
            continue
        if not armed:
            continue
        if IGNORE_ATTR.match(line):
            skip = True
            continue
        m = FN_NAME.match(line)
        if m:
            (ignored if skip else cases).append(m.group(1))
            armed, skip = False, False
        elif line.strip() and not line.strip().startswith(("#[", "//", "///")):
            armed, skip = False, False  # something else followed the attribute
    return cases, ignored


def read(root, path):
    full = os.path.join(root, path)
    if not os.path.exists(full):
        return None
    with open(full, encoding="utf-8", errors="replace") as f:
        return f.read()


def collect(root):
    """[(label, path, cases, ignored)] for each place, and a list of the places that are missing."""
    found, missing = [], []
    for path, label, kind in PLACES:
        src = read(root, path)
        if src is None:
            missing.append(path)
            continue
        cases, ignored = harness_cases(src) if kind == "harness" else nextest_cases(src)
        found.append((label, path, cases, ignored))
    return found, missing


def main(argv=None):
    ap = argparse.ArgumentParser(description="the cloud routine VM's environmental-failure set, from the tree")
    ap.add_argument("--markdown", action="store_true", help="the preamble's block, ready to paste")
    ap.add_argument("--names", action="store_true", help="the bare names, one per line")
    ap.add_argument("--expect", type=int, help="exit 1 when the total is not this")
    ap.add_argument("--root", default=None, help="the tree (default: this script's repository)")
    a = ap.parse_args(argv)
    root = a.root or os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
    found, missing = collect(root)
    if missing:
        for p in missing:
            print(f"env-fail-set: {p} is not in this tree", file=sys.stderr)
        if not found:
            print("env-fail-set: none of the three places is here: it read nothing, so it says nothing",
                  file=sys.stderr)
            return 2
    total = sum(len(c) for _, _, c, _ in found)

    if a.names:
        for label, _, cases, _ in found:
            for n in cases:
                print(f"{label} {n}")
    elif a.markdown:
        print(f"**{total} tests fail on this VM for reasons that are not yours, in "
              f"{len(found)} places, from one cause.** The cause: this VM runs as root and `RLIMIT_NPROC` "
              "behaves differently here, so the sandbox refuses to build the view it asserts about. Derived "
              "from the tree by `python3 scripts/env-fail-set.py`, so it is this tree's set and not an "
              "older wave's; re-run it rather than editing the number.\n")
        for label, path, cases, ignored in found:
            print(f"- **`{label}` — {len(cases)} cases** (`{path}`)")
            for n in cases:
                print(f"  - `{n}`")
            for n in ignored:
                print(f"  - _(not in the set: `{n}` runs only when asked)_")
        print(f"\nThat is the whole known set, **{total}**. A failure outside those places is yours to "
              "explain, and you must still name every failing test by its full name and say for each one "
              "why it is or is not yours.")
    else:
        print("env-fail-set: the 4-core cloud routine VM, running as root. NOT a fleet box: a fleet gate's "
              "clause_01 to clause_12 are green and its reds are empty.")
        for label, path, cases, ignored in found:
            print(f"\n{label}  {len(cases)} cases  ({path})")
            for n in cases:
                print(f"  {n}")
            for n in ignored:
                print(f"  (not in the set) {n}")
        print(f"\nenv-fail-set: {total} cases in {len(found)} places, derived from the tree")

    if a.expect is not None and a.expect != total:
        print(f"env-fail-set: the tree totals {total}, not the {a.expect} asked for: the brief that says "
              f"{a.expect} is stale, so regenerate it (--markdown) rather than trusting the number",
              file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
