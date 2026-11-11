#!/usr/bin/env python3
"""Every per-file numeric invariant in this tree, with its headroom (theseus-4vsrz, 1.0.5 B7).

**The class.** A numeric invariant is a property of the BATCH, not of the branch, exactly when three things
hold at once: the written-down number and the quantity it bounds live apart (so git merges both edits with no
conflict), the quantity is a SUM over the tree (so two branches' contributions add), and branches move the
quantity TOWARD the bound. Where all three hold, every branch in a cut passes alone and the merge fails. Where
any one fails, a merge cannot break the invariant silently: `assert_eq!(SWEEPS.len(), 7)` sits beside its own
table, so a second branch adding a sweep conflicts textually; `complete::MustBeComplete` compares a set against
a walk of the same tree, so both sides move together; a `least` floor with slack is moved AWAY from by the
branches that grow the walk.

Two reds on 2026-11-08 were the first kind. `scripts/long-files.txt`'s ceiling on `turn.rs` was 3,525 with 25
lines of headroom; p5/footer-session-labels added 7 and p5/harness-nudge-not-posted added 23, each fitting
alone, and the 23:00 departure died in its shape phase in 13 seconds (68f8fe2b, which raised it to 3,560). The
operator census found 26 call-site files where the list held 25, from main moving under the branch (95023099),
and 27 against 26 from a sibling in the same cut (9d9aa6b8).

**So this command exists to be run on the MERGED tree, before the gate.** The headroom a branch has at its own
base says nothing; the headroom the cut has after the merge is the number that decides. `shape.sh` and clippy
both hold these limits already and both run at the join -- but they run after a nine-minute build, and they
answer "over" rather than "two lines left". This answers the second question in about a second, and `--since`
says which part of a range spent the room, so a cut can defer one branch instead of discovering the collision
at the gate.

    python3 scripts/invariant-pressure.py [--margin N] [--since REV]

**It always exits 0 on a report**, for the reason `scripts/line-pressure.py` gives for refusing to be a gate:
making a warning instrument into a failure would be a second ceiling that the next merge has to argue with,
and the ceilings in `long-files.txt` and `clippy.toml` are the ones that hold. It exits 2 only when it read
nothing, because an empty scan must not look like no pressure.

**It never re-implements an invariant.** It reads numbers that are already written down (`long-files.txt`'s
ceilings, `shape.sh`'s default, `clippy.toml`'s threshold through `line-pressure.py`) and measures the tree the
cheap way. The completeness censuses (`crates/complete.rs`'s `MustBeComplete`) are LISTED and not measured: a
second walk of the same world here would be a second copy of the rule, free to disagree with the test that
holds it. The roster is what a cut needs -- the tests to re-run on the staged merge when a branch touched one
of the worlds they walk.
"""
import argparse
import importlib.util
import os
import re
import signal
import subprocess
import sys

DEFAULT_LIMIT = 2500          # scripts/shape.sh's `limit`
LIST = "scripts/long-files.txt"
MARGIN = 50                   # lines of headroom worth reporting on a file
HERE = os.path.dirname(os.path.abspath(__file__))


def line_pressure():
    """scripts/line-pressure.py as a module (its name has a hyphen, so not by `import`)."""
    spec = importlib.util.spec_from_file_location("line_pressure", os.path.join(HERE, "line-pressure.py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def rust_files():
    """The files shape.sh judges: tracked and untracked Rust under crates/."""
    out = subprocess.run(["git", "ls-files", "--cached", "--others", "--exclude-standard", "--", "crates/*.rs"],
                         capture_output=True, text=True, check=True).stdout
    return [f for f in out.split("\n") if f and os.path.isfile(f)]


def ceilings():
    """{path: (ceiling, reason)} from scripts/long-files.txt, in the file's own order."""
    out = {}
    with open(LIST, encoding="utf-8") as f:
        for line in f:
            if not line.strip() or line.startswith("#"):
                continue
            path, c, reason = (line.split(None, 2) + ["", ""])[:3]
            out[path] = (int(c), reason.strip())
    return out


def lines_of(path):
    with open(path, "rb") as f:
        return f.read().count(b"\n")


def grown(rev):
    """{path: added_lines} for a range, from `git diff --numstat REV..` plus the working tree."""
    out = {}
    for args in (["git", "diff", "--numstat", f"{rev}..HEAD"], ["git", "diff", "--numstat", "HEAD"]):
        r = subprocess.run(args, capture_output=True, text=True)
        for line in r.stdout.split("\n"):
            bits = line.split("\t")
            if len(bits) == 3 and bits[0].isdigit() and bits[1].isdigit():
                out[bits[2]] = out.get(bits[2], 0) + int(bits[0]) - int(bits[1])
    return out


def file_rows(margin):
    """[(left, path, n, limit, note)] for every file within `margin` of its limit, closest first."""
    cs = ceilings()
    rows = []
    for path in rust_files():
        n = lines_of(path)
        if path in cs:
            limit, _ = cs[path]
            note = ""
            if n <= DEFAULT_LIMIT:
                note = f"at or under {DEFAULT_LIMIT}: shape.sh wants its entry in {LIST} deleted"
        else:
            limit, note = DEFAULT_LIMIT, ""
            if DEFAULT_LIMIT - margin <= n <= DEFAULT_LIMIT:
                note = f"no entry, and shape.sh refuses one at or under {DEFAULT_LIMIT}: it cannot legally grow past it"
        if limit - n <= margin:
            rows.append((limit - n, path, n, limit, note))
    rows.sort(key=lambda r: (r[0], r[1]))
    return rows


CENSUS = re.compile(r"MustBeComplete\s*\{(.{0,2600}?)\}", re.S)
FIELD = re.compile(r'\b(item|least|stale_fails)\s*:\s*(.*?)\s*,\s*(?:\n|$)', re.S)
# `least,` and `stale_fails,` in Rust's field shorthand: the value is a binding, so the number is derived at
# test time and there is nothing written down to go stale. That is the merge-stable form of a count, so say so.
SHORTHAND = re.compile(r"\b(least|stale_fails)\s*,")


def censuses():
    """[(path, line, item, least, stale_fails)] for each `MustBeComplete` spec in the tree, located not judged."""
    out = []
    for path in rust_files():
        with open(path, encoding="utf-8", errors="replace") as f:
            src = f.read()
        for m in CENSUS.finditer(src):
            got = dict(FIELD.findall(m.group(1)))
            if "item" not in got:
                continue
            item = " ".join(re.findall(r'"((?:[^"\\]|\\.)*)"', got["item"])).replace("\\\n", "")
            item = re.sub(r"\s+", " ", item).strip()
            short = set(SHORTHAND.findall(m.group(1)))
            out.append((path, src.count("\n", 0, m.start()) + 1, item,
                        got.get("least", "derived" if "least" in short else "?"),
                        got.get("stale_fails", "derived" if "stale_fails" in short else "?")))
    return out


def report(margin, since):
    moved = grown(since) if since else {}
    rows = file_rows(margin)
    print(f"files: {len(rows)} within {margin} lines of a limit "
          f"({len(ceilings())} ceilings in {LIST}, the rest at shape.sh's default of {DEFAULT_LIMIT})")
    for left, path, n, limit, note in rows:
        room = f"OVER by {-left}" if left < 0 else f"{left} left"
        d = f"  (+{moved[path]} in this range)" if moved.get(path) else ""
        print(f"  {path} {n}/{limit}  {room}{d}" + (f"  -- {note}" if note else ""))

    mod = line_pressure()
    os.chdir(os.path.join(HERE, ".."))
    scanned, near = mod.scan([], mod.THRESHOLD, 0)
    at = [t for t in near if t[0] >= mod.THRESHOLD]
    print(f"functions: {len(at)} at or over clippy's {mod.THRESHOLD} of {scanned} scanned; "
          f"`line-pressure.py --margin N` lists the rest")
    for n, path, line, name in at:
        d = f"  (+{moved[path]} in this range)" if moved.get(path) else ""
        print(f"  {path}:{line} {name} {n}/{mod.THRESHOLD}" + ("  OVER" if n > mod.THRESHOLD else "") + d)

    cen = censuses()
    strict = [c for c in cen if c[4] == "true"]
    print(f"censuses: {len(cen)} completeness lists, {len(strict)} of them with stale_fails -- "
          f"NOT measured here; re-run their tests on the merged tree")
    for path, line, item, least, stale in cen:
        d = "  (this range touched it)" if moved.get(path) else ""
        print(f"  {path}:{line} least={least} stale_fails={stale}  {item}" + d)
    return rows, at, cen


def main(argv=None):
    ap = argparse.ArgumentParser(description="per-file numeric invariants and their headroom (never fails on a report)")
    ap.add_argument("--margin", type=int, default=MARGIN, help=f"lines of headroom worth reporting (default {MARGIN})")
    ap.add_argument("--since", help="a base revision: mark what this range added to each invariant it moved")
    a = ap.parse_args(argv)
    # A report is long and people pipe it into `head`: die on the closed pipe instead of a traceback.
    if hasattr(signal, "SIGPIPE"):
        signal.signal(signal.SIGPIPE, signal.SIG_DFL)
    os.chdir(os.path.join(HERE, ".."))
    rows, at, cen = report(a.margin, a.since)
    if not rows and not at and not cen:
        print("invariant-pressure: it found no invariant at all: it read nothing, so it says nothing",
              file=sys.stderr)
        return 2
    over = [r for r in rows if r[0] < 0] + [t for t in at if t[0] > 100]
    print(f"invariant-pressure: {len(rows)} files and {len(at)} functions within reach of a limit, "
          f"{len(over)} already over; a warning only, shape.sh and clippy hold the ceilings")
    return 0


if __name__ == "__main__":
    sys.exit(main())
