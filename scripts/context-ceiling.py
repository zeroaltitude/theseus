#!/usr/bin/env python3
"""The context-file byte ceiling (theseus-4vsrz, 1.0.5 B7): no instruction file the loader reads may pass
`context_files::MAX_BYTES` unless `scripts/context-files.txt` lists it.

    python3 scripts/context-ceiling.py            # check, and print every file's headroom
    python3 scripts/context-ceiling.py --quiet     # check, print violations and one summary line only

**Why this exists.** `crates/theseus-core/src/context_files.rs`'s `section()` truncates a context file to
`MAX_BYTES` and appends `[Cut: only the first 65,536 bytes of this file are included.]`. The session is told
that a cut happened; it is never told what was in the tail. On 2026-11-10 `scripts/AGENTS.md` stood at
71,910 bytes -- 6,374 over -- and everything from its `## setup.sh` section to the end of the file had
stopped reaching any session that read it. **Nothing measured it**: not a gate phase, not a test, not
`shape.sh`, not `long-files.txt`. It was found by hand during a merge, after one branch crossed the ceiling
alone and a batch of six tripled the breach in an afternoon. That is the failure this script exists to make
loud, and it is a failure of measurement rather than of care: the file carried a warning telling readers to
`wc -c` it, and the warning itself went stale two commits after it was written.

**The ceiling and the file names are READ OUT OF THE RUST SOURCE, never copied here.** `MAX_BYTES` comes from
`context_files.rs` and the names come from `project_context.rs`'s `NAMES`, so a change to either moves this
check with it and the two cannot drift. If this script cannot find them it exits 2 and says so, because a
scan that read nothing must not look like a clean scan.

**What it covers.** Every file tracked by git whose basename is one of `NAMES`, anywhere in the tree.
`project_context.rs`'s `chain()` reads both names in every directory from the repository root down to the
session's own working directory, so any directory's pair is read by a session working in or under it, and
there is no shorter honest list. It also covers the user-level file, which is the same two-name rule applied
to the state directory (`project_context::USER_FILE`) -- that file is outside the repository and cannot be
checked here, which is named in "What it does not cover" below.

**What it does not cover**, deliberately:
  - files outside the repository: the owner's user-level `AGENTS.md` under the state directory, and anything
    `[context] files` or `[personas.<name>] files` names in a live `theseus.toml`. Those are the owner's and
    are not in any tree this script can see;
  - the PROJECT level's second ceiling. `project_context::MAX_TOKENS` (8,000 by default) is a budget over the
    whole level, and a file inside `MAX_BYTES` can still be `Fate::Cut` or `Fate::Dropped` by it. That is a
    different instrument with a different unit and it is not held here;
  - whether a file's content is any good, or whether a file close to the ceiling should be split anyway. The
    headroom column is the warning half; this script's hard half is only the ceiling.

**The list can only shrink or be raised on purpose**, exactly as `scripts/shape.sh` holds `long-files.txt`:
  - a file over the ceiling with no entry fails: move its depth into a sibling document, or list it with an
    allowance and the reason;
  - a listed file past its allowance fails: trim it, or raise the allowance in the same commit and say why;
  - a listed file at or under the ceiling fails: its entry is stale, delete it.

**Raising `MAX_BYTES` is not a fix and is not available to a branch.** The constant is the model's own
request budget; moving it to make a paperwork file fit spends context on every session for every file. The
answer to a file over the ceiling is always to move depth out of it.
"""
import argparse
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
LIST = "scripts/context-files.txt"
CORE = "crates/theseus-core/src/context_files.rs"
PROJ = "crates/theseus-core/src/project_context.rs"
# `pub const MAX_BYTES: usize = 64 * 1024;` -- the arithmetic is read, not assumed.
MAX_BYTES_RE = re.compile(r"pub const MAX_BYTES:\s*usize\s*=\s*([0-9*_ ]+);")
NAMES_RE = re.compile(r"pub const NAMES:\s*\[&str;\s*\d+\]\s*=\s*\[([^\]]*)\];")


def ceiling(root):
    """`MAX_BYTES` as context_files.rs declares it, or None. Only `N` and `N * M` are accepted: anything
    else is a shape this script was not written against, and guessing at it would be the drift the whole
    design is meant to prevent."""
    try:
        with open(os.path.join(root, CORE), encoding="utf-8") as f:
            src = f.read()
    except OSError:
        return None
    m = MAX_BYTES_RE.search(src)
    if not m:
        return None
    total = 1
    for part in m.group(1).replace("_", "").split("*"):
        part = part.strip()
        if not part.isdigit():
            return None
        total *= int(part)
    return total


def names(root):
    """`project_context::NAMES`, the basenames the loader reads in every directory, or None."""
    try:
        with open(os.path.join(root, PROJ), encoding="utf-8") as f:
            src = f.read()
    except OSError:
        return None
    m = NAMES_RE.search(src)
    if not m:
        return None
    found = re.findall(r'"([^"]+)"', m.group(1))
    return found or None


def tracked(root, wanted):
    """Every git-tracked file whose basename is one of `wanted`, with its size on disk."""
    out = subprocess.run(["git", "ls-files", "-z"], cwd=root, capture_output=True, text=True, check=True).stdout
    found = []
    for path in out.split("\0"):
        if not path or os.path.basename(path) not in wanted:
            continue
        full = os.path.join(root, path)
        if not os.path.isfile(full):
            continue
        found.append((path, os.path.getsize(full)))
    found.sort(key=lambda t: (-t[1], t[0]))
    return found


def listed(root):
    """`scripts/context-files.txt` as {path: (allowance, reason)}. A malformed line is a failure, not a
    skip: a typo that silently drops an entry would un-hold a file nobody meant to un-hold."""
    out, bad = {}, []
    path = os.path.join(root, LIST)
    if not os.path.exists(path):
        return out, bad
    with open(path, encoding="utf-8") as f:
        for n, line in enumerate(f, 1):
            line = line.rstrip("\n")
            if not line.strip() or line.lstrip().startswith("#"):
                continue
            parts = line.split(None, 2)
            if len(parts) < 3 or not parts[1].isdigit():
                bad.append((n, line))
                continue
            out[parts[0]] = (int(parts[1]), parts[2])
    return out, bad


def check(root, quiet=False, show=None):
    cap = ceiling(root)
    if cap is None:
        print(f"context-ceiling: could not read MAX_BYTES from {CORE}: it read nothing, so it says nothing",
              file=sys.stderr)
        return 2
    wanted = names(root)
    if wanted is None:
        print(f"context-ceiling: could not read NAMES from {PROJ}: it read nothing, so it says nothing",
              file=sys.stderr)
        return 2
    files = tracked(root, set(wanted))
    if not files:
        print(f"context-ceiling: found no {' or '.join(wanted)} in the tree: it read nothing, so it says nothing",
              file=sys.stderr)
        return 2
    allow, bad = listed(root)

    status = 0
    for n, line in bad:
        print(f"context-ceiling: {LIST}:{n} is not `<path> <allowance> <reason>`: {line!r}")
        status = 1

    seen = set()
    for path, size in files:
        a = allow.get(path)
        if a is None:
            if size > cap:
                print(f"context-ceiling: {path} is {size:,} bytes, over the {cap:,} ceiling by {size - cap:,}, "
                      f"and has no entry in {LIST}: move depth into a sibling document, or list it with an "
                      f"allowance and the reason")
                status = 1
            continue
        seen.add(path)
        if size <= cap:
            print(f"context-ceiling: {path} is {size:,} bytes, at or under the {cap:,} ceiling: "
                  f"delete its entry in {LIST}")
            status = 1
        elif size > a[0]:
            print(f"context-ceiling: {path} is {size:,} bytes, over its allowance of {a[0]:,} in {LIST} "
                  f"by {size - a[0]:,}: trim it, or raise the allowance in this commit and say why")
            status = 1

    for path in allow:
        if path not in seen and path not in {p for p, _ in files}:
            print(f"context-ceiling: {LIST} lists {path}, which is not a tracked context file any more: "
                  f"delete its entry")
            status = 1

    if not quiet:
        print(f"context-ceiling: {len(files)} context files, ceiling {cap:,} bytes "
              f"(MAX_BYTES, read from {CORE}); names {', '.join(wanted)} (read from {PROJ})")
        for path, size in files[:show] if show else files:
            if path in allow:
                mark = f"  LISTED, allowance {allow[path][0]:,}"
            elif size > cap:
                mark = "  OVER"
            else:
                mark = ""
            left = cap - size
            room = f"{left:,} left" if left >= 0 else f"OVER by {-left:,}"
            print(f"{path} {size:,}/{cap:,}  {room}{mark}")
        if show and len(files) > show:
            print(f"context-ceiling: {len(files) - show} more, largest first; all of them: "
                  f"python3 scripts/context-ceiling.py")

    held = [p for p, s in files if s > cap and p in allow]
    if status == 0:
        # The listed breaches are left out of "closest": they are already over, and a negative headroom
        # reported as the closest margin would make the warning half useless.
        room = [cap - s for p, s in files if p not in allow]
        closest = f"; closest unlisted has {min(room):,} bytes left" if room else ""
        print(f"context-ceiling: ok ({len(files)} context files, none over {cap:,} bytes but the "
              f"{len(held)} listed{closest})")
    return status


def main(argv=None):
    ap = argparse.ArgumentParser(description="context files over context_files::MAX_BYTES (fails the gate)")
    ap.add_argument("--quiet", action="store_true", help="violations and the summary line only")
    ap.add_argument("--show", type=int, default=None, help="list only the N largest")
    ap.add_argument("--root", default=None, help="the repository to check (default: this script's own)")
    a = ap.parse_args(argv)
    root = a.root or os.path.join(HERE, "..")
    return check(os.path.abspath(root), quiet=a.quiet, show=a.show)


if __name__ == "__main__":
    sys.exit(main())
