#!/usr/bin/env python3
"""Mark today's offenders of the shape budget (theseus-goa8; review 2's C1).

Reads `cargo clippy --workspace --all-targets --message-format=json` on stdin, and for each function clippy says
is over `too_many_lines` or `cognitive_complexity`, inserts an attribute line above its `fn`, one for each lint:

    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]

Each is short enough for rustfmt to leave on one line. Nothing is rewritten, and the list can only get shorter:
`expect` fails, as an unfulfilled lint expectation, once the function is under the limit. Idempotent: a function
already marked is not marked again. Run it again on the rebased tree, instead of resolving a conflict in the
attributes by hand, and to mark the new offenders after a threshold in clippy.toml is lowered.

    cargo clippy --workspace --all-targets --message-format=json -q 2>/dev/null | python3 scripts/shape-expect.py
"""
import collections
import json
import os
import re
import sys

LINTS = ("clippy::too_many_lines", "clippy::cognitive_complexity")
REASON = "shape budget: split it"


def main() -> int:
    root = "."
    if len(sys.argv) > 2 and sys.argv[1] == "--root":
        root = sys.argv[2]
    hits = collections.defaultdict(lambda: collections.defaultdict(set))  # file -> line -> {lint}
    for raw in sys.stdin:
        try:
            m = json.loads(raw)
        except ValueError:
            continue
        if m.get("reason") != "compiler-message":
            continue
        msg = m["message"]
        code = (msg.get("code") or {}).get("code")
        if code not in LINTS:
            continue
        primary = [s for s in msg["spans"] if s.get("is_primary")]
        if not primary:
            continue
        span = primary[0]
        hits[span["file_name"]][span["line_start"]].add(code)

    marked, manual = 0, []
    for file, lines in sorted(hits.items()):
        path = os.path.join(root, file)
        with open(path, encoding="utf-8") as f:
            text = f.read().split("\n")
        # From the bottom up, so earlier line numbers stay true.
        for line in sorted(lines, reverse=True):
            i = line - 1
            target = text[i]
            if not re.search(r"\bfn\b", target):
                manual.append(f"{file}:{line}: not a fn line: {target.strip()[:80]}")
                continue
            lints = sorted(lines[line])
            # Already marked: an `expect` for it among the attributes just above.
            j, have = i - 1, set()
            while j >= 0 and (text[j].lstrip().startswith("#[") or text[j].lstrip().startswith("///")):
                have.update(re.findall(r"clippy::\w+", text[j]) if "expect(" in text[j] else [])
                j -= 1
            lints = [l for l in lints if l not in have]
            if not lints:
                continue
            indent = re.match(r"\s*", target).group(0)
            for lint in reversed(lints):
                text.insert(i, f'{indent}#[expect({lint}, reason = "{REASON}")]')
            marked += 1
        with open(path, "w", encoding="utf-8") as f:
            f.write("\n".join(text))
    print(f"shape-expect: marked {marked} function(s) in {len(hits)} file(s)")
    for line in manual:
        print(f"shape-expect: MANUAL {line}")
    return 1 if manual else 0


if __name__ == "__main__":
    sys.exit(main())
