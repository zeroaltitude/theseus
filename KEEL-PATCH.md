# KEEL-PATCH: two edits to scripts/keel-guard.py (a `KEEL_FILES` entry), drafted, not applied

`scripts/keel-guard.py` is guarded (`keel-file`), and a cloud session cannot sign or quote the owner. Neither edit is
in the tree. Each is proved outside it: `scripts/keel-batch-repro.sh [guard.py]` builds throwaway repositories and
judges them with the guard it is given. Trailer to carry both (or one per item):

    Keel: keel-file scripts/keel-guard.py — <THE OWNER'S WORDS GO HERE>

Below, one unified diff against base 4683adb8; apply with `patch -p0` from the repository root. Item 3 is the one
changed line in `rewritten` (the first hunk); item 4 is the rest (the second and third hunks).

## Item 3: a rewrite pairs within its own file only

Reproduced: `scripts/keel-batch-repro.sh` with the tree's guard. Branch `a` alone: `routing/test_effort.py:1:
test-removed`. `a` merged with `b` (which adds an unrelated test to `voice/test_mute.py`): `voice/test_mute.py:4:
assert-removed ... (3 assertions) -> 1 assertions`, a file with no erosion, and a trailer naming the real file no longer
covers it. The ack commit stays in the judged range (`acks` reads `base..HEAD`); what moves is the finding's rule and
path, which `acked` matches exactly. With the patch the batched run reports `routing/test_effort.py:1: test-removed`
again. The guard's own suite (`scripts/test_keel_guard.py`, 185 tests), run from a scratch copy with and without the
patch, gives identical results (2 errors both ways, from `docs/` files the scratch copy lacks; no regression).
A same-file rename-and-rewrite still pairs, and a moved test keeps pairing by name or body.

## Item 4: say which half fired, and say when it looks like base drift

Each finding line ends `[where]` (`keel-file`, `locked-flow`: the file is guarded) or `[what]` (every erosion rule: the
edit erodes, in any file), with a legend line saying a file outside `KEEL_FILES` is not exempt. When the base is not an
ancestor of HEAD it prints `BASE DRIFT?` with the count of test-removed findings and the check
(`--base $(git merge-base <base> <head>)`). That test is exact: a base in HEAD's history cannot drift.

```diff
--- scripts/keel-guard.py	2026-11-09 07:52:53.607799599 +0000
+++ scripts/keel-guard.py	2026-11-09 07:58:26.857162902 +0000
@@ -686,7 +686,9 @@
     """A test renamed and rewritten with it: the added test most like it, at least half alike, its own file first."""
     words = b["norm"].split()
     best, score = None, 0.0
-    for same_file in (True, False):
+    # A rewrite pairs within its own file only (theseus-4vsrz): across files the pairing reclassified a real
+    # test-removed as an assert-removed in a file with no erosion, which un-acked a correct trailer in a batch.
+    for same_file in (True,):
         for c in cands:
             if (c[0] == path) != same_file:
                 continue
@@ -1854,6 +1856,9 @@
     return any(rule == f.rule and any(fnmatch.fnmatchcase(f.path, p) for p in paths) for rule, paths, _ in ack_list)
 
 
+WHERE_OR_WHAT = {r: "where" for r in ("keel-file", "locked-flow")}
+
+
 def main(argv=None):
     ap = argparse.ArgumentParser(description="The keel guard: erosions of tests, budgets and ceilings in a range.")
     ap.add_argument("--repo", default=".")
@@ -1893,7 +1898,15 @@
         return 0
     print(f"keel: {len(open_)} erosions of a test, a budget or a ceiling ({how}, {base[:10]}):")
     for f in open_:
-        print(f"  {f}")
+        print(f"  {f}   [{WHERE_OR_WHAT.get(f.rule, 'what')}]")
+    print("keel: [where] = the file is guarded, whatever the edit; [what] = the edit erodes a test, budget or ceiling, "
+          "in ANY file: a file outside KEEL_FILES is not exempt from the [what] rules")
+    if git(repo, "merge-base", "--is-ancestor", base, head_commit, check=False) is None:
+        n = sum(f.rule == "test-removed" for f in open_)
+        print(f"keel: BASE DRIFT? {base[:10]} is not an ancestor of {head_commit[:10]}, so tests the base's side added "
+              f"read as removed here ({n} test-removed). Re-judge at the branch's own base: --base "
+              f"$(git merge-base {base[:10]} {head_commit[:10]}), and compare; a finding in a file the branch never "
+              "touched is drift, one in a file it did touch is an erosion either way")
     groups = defaultdict(list)
     for f in open_:
         if f.path not in groups[f.rule]:
```
