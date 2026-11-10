"""`scripts/gate.sh --bench-bins`: that the test build made the binaries the
benches run (theseus-1eemp).

The check was written by theseus-7ykr to catch a stale bench binary, and it
compared cargo's reported executable with `target/debug/<bin>` under the tree.
A self build's tree is a worktree whose CARGO_TARGET_DIR is
`<state>/self/target/<item>`, outside the checkout (rsi/tree.rs), so those are
two different paths and the comparison was false whatever cargo had built: 22
self builds in a row died in the `test build` phase, each naming a binary cargo
had just made, before one test of their change ran.

So the check reads the directory cargo builds into. These plant both layouts -
a self-shaped tree whose target directory is elsewhere, and an ordinary one -
and drive the real check over a planted cargo message file. The last two hold
what theseus-7ykr added: a binary the build did not make still fails.
"""

import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
GATE = REPO / "scripts" / "gate.sh"
BINS = ("theseusd", "theseus-sim", "theseus-index")


def plant(debug: Path, names: tuple[str, ...] = BINS) -> Path:
    """A debug directory holding the bench binaries."""
    debug.mkdir(parents=True, exist_ok=True)
    for b in names:
        (debug / b).write_text("#!/bin/sh\n")
    return debug


def messages(at: Path, debug: Path, names: tuple[str, ...] = BINS) -> Path:
    """What `cargo nextest run --no-run --cargo-message-format json` writes:
    one JSON object a line, an `executable` on each artifact it linked."""
    at.write_text(
        "".join(
            json.dumps(
                {"reason": "compiler-artifact", "executable": str(debug / b)},
                separators=(",", ":"),
            )
            + "\n"
            for b in names
        )
    )
    return at


def check(tree: Path, msgs: Path, target: Path | None) -> subprocess.CompletedProcess:
    env = dict(os.environ, THESEUS_GATE_ROOT=str(tree))
    env.pop("CARGO_TARGET_DIR", None)
    if target is not None:
        env["CARGO_TARGET_DIR"] = str(target)
    return subprocess.run(
        ["bash", str(GATE), "--bench-bins", str(msgs)],
        capture_output=True,
        text=True,
        env=env,
        cwd=str(tree),
    )


class BenchBins(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.tree = self.root / "tree"
        self.tree.mkdir()
        self.addCleanup(self.tmp.cleanup)

    def test_a_self_shaped_tree_builds_outside_its_checkout(self):
        """The shape every self build has: no `target/` in the tree at all."""
        debug = plant(self.root / "self-target" / "debug")
        r = check(self.tree, messages(self.root / "m.json", debug), debug.parent)
        self.assertEqual(r.returncode, 0, f"{r.stdout}{r.stderr}")

    def test_a_self_shaped_tree_whose_target_link_is_broken(self):
        """The tree is given a `target` symlink to its build tree, so the old
        comparison could resolve; when that link is missing or stale the check
        read nothing at all. The build tree is still where cargo built."""
        debug = plant(self.root / "self-target" / "debug")
        (self.tree / "target").symlink_to(self.root / "gone")
        r = check(self.tree, messages(self.root / "m.json", debug), debug.parent)
        self.assertEqual(r.returncode, 0, f"{r.stdout}{r.stderr}")

    def test_an_ordinary_tree_builds_into_its_own_target(self):
        """No CARGO_TARGET_DIR and no cargo workspace to ask: `./target`."""
        debug = plant(self.tree / "target" / "debug")
        r = check(self.tree, messages(self.root / "m.json", debug), None)
        self.assertEqual(r.returncode, 0, f"{r.stdout}{r.stderr}")

    def test_a_binary_from_another_build_fails(self):
        """theseus-7ykr's guarantee: the benches would run a stale binary."""
        plant(self.root / "self-target" / "debug")
        stale = plant(self.root / "older" / "debug")
        r = check(self.tree, messages(self.root / "m.json", stale), self.root / "self-target")
        self.assertEqual(r.returncode, 1, f"{r.stdout}{r.stderr}")
        self.assertIn("did not build", r.stdout)
        self.assertIn("self-target/debug/theseusd", r.stdout)

    def test_a_binary_the_build_never_named_fails(self):
        debug = plant(self.root / "self-target" / "debug")
        msgs = messages(self.root / "m.json", debug, ("theseusd", "theseus-index"))
        r = check(self.tree, msgs, debug.parent)
        self.assertEqual(r.returncode, 1, f"{r.stdout}{r.stderr}")
        self.assertIn("theseus-sim", r.stdout)
