"""bench/view's measuring kit on the REAL daemon and CLI (issue 46): one test per tool, each on an empty scratch
store the test fills by script on a stand-in model, so the kit cannot drift from the CLI it reads unnoticed again
(`replay.py` once asked `view preview --turns`, a form no build had, and only a fake daemon ever answered it).

The binaries must be THIS tree's: `target/debug/theseus` and `theseusd` are used only when both carry this tree's
commit (the CLI prints it under `THESEUS_CLI_STAMP`; the daemon holds `theseus-build-stamp:<commit>:`) and no Rust
source differs from that commit. Otherwise every test here skips, saying which of those failed. Nothing here calls a
model provider: the model is a local stand-in, and the yardstick runs `--dry-run`. The kit reads TOML with
`tomllib`, so on a Python older than 3.11 the whole file says it skipped.
"""

from __future__ import annotations

import contextlib
import io
import json
import mmap
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
VIEW = ROOT / "bench" / "view"
sys.path[:0] = [str(VIEW), str(ROOT / "bench" / "recall")]
NEW_ENOUGH = sys.version_info >= (3, 11)
if NEW_ENOUGH:
    import cases as vc  # noqa: E402
    import proof  # noqa: E402
    import replay  # noqa: E402
    import scratch  # noqa: E402
    import standin  # noqa: E402
    import yardstick as ys  # noqa: E402

BINS = ROOT / "target" / "debug"
BUILD = "build this tree's (cargo build -p theseus -p theseusd)"


def git(*args: str) -> str | None:
    r = subprocess.run(["git", "-C", str(ROOT), *args], capture_output=True, text=True)
    return r.stdout.strip() if r.returncode == 0 else None


def this_trees_bins(bins: Path = BINS) -> tuple[Path | None, str]:
    """`(bins, "")` when `theseus` and `theseusd` in `bins` were built from this tree, else `(None, why)`."""
    head = git("rev-parse", "HEAD")
    if not head:
        return None, f"{ROOT} is not a git checkout: there is no commit to hold a binary to"
    if git("status", "--porcelain", "--", "crates", "Cargo.toml", "Cargo.lock"):
        return None, f"this tree's Rust differs from its commit {head}: no binary's stamp can say it holds that"
    for b in ("theseus", "theseusd"):
        if not (bins / b).is_file():
            return None, f"no {bins / b}: {BUILD}"
    r = subprocess.run([str(bins / "theseus")], env={"THESEUS_CLI_STAMP": "1", "PATH": "/usr/bin:/bin"},
                       capture_output=True, text=True, timeout=30)
    cli = r.stdout.strip() if r.returncode == 0 else ""
    if cli != head:
        return None, f"{bins / 'theseus'} was built from {cli or 'no commit'}, this tree is at {head}: {BUILD}"
    with open(bins / "theseusd", "rb") as f, mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ) as m:
        if m.find(f"theseus-build-stamp:{head}:".encode()) < 0:
            return None, f"{bins / 'theseusd'} carries no stamp of {head}: {BUILD}"
    return bins, ""


LINES = ("The staging bucket is called amber-lattice.", "Keep build logs for fourteen days.",
         "Add a lint step before the tests run.")


class Keep(scratch.Scratch if NEW_ENOUGH else object):
    """A scratch daemon whose store is kept when it stops: the tests read it back from a copy."""

    def stop(self):
        saved, self.state = self.state, self.dir / "kept-not-removed"
        try:
            return super().stop()
        finally:
            self.state = saved


@unittest.skipUnless(NEW_ENOUGH, "the view kit reads TOML with tomllib: Python 3.11 or newer")
class RealKit(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.bins, why = this_trees_bins()
        if cls.bins is None:
            raise unittest.SkipTest(why)
        cls.tmp = Path(tempfile.mkdtemp(prefix="view-kit-real-"))
        # One place, three turns, on an empty store: the store the replay and the yardstick read, from a copy.
        model = standin.StandIn([], "Noted.")
        place = cls.tmp / "places" / "here"
        place.mkdir(parents=True)
        try:
            with contextlib.redirect_stderr(io.StringIO()), \
                    Keep(cls.bins, cls.tmp / "build", api_base=model.base, model=standin.MODEL) as d:
                v = d.json("rpc", "session.open", json.dumps({"label": "here", "dir": str(place.resolve())}))
                sid = v.get("result", v).get("session_id") or v.get("result", v)["id"]
                for line in LINES:
                    r = d.cli("--json", "ask", "-s", sid, "-", input=line, cwd=place)
                    assert r.returncode == 0, f"ask exit {r.returncode}"
                cls.place = scratch.ledger_rows(d, replay.TURN_KIND)[-1]["data"]["place"]
        finally:
            model.close()
        cls.copy = cls.tmp / "copy"
        shutil.copytree(cls.tmp / "build" / "daemon" / "state", cls.copy)

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.tmp, True)

    def quiet(self, f, *a):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(io.StringIO()):
            r = f(*a)
        return r, buf.getvalue()

    def test_cases_builds_and_scores_v2_cases_on_the_real_daemon(self):
        """A case here and a case elsewhere: the second is held only if the builder's lines are marked the
        owner's (his words elsewhere are slot F's), wherever this runs, under an agent or with no terminal."""
        want = ("here-01", "else-01")
        cs = [c for c in vc.load(VIEW / "cases" / "synthetic.v2.json")["cases"] if c["id"] in want]
        out = self.tmp / "cases"
        r, _ = self.quiet(vc.build, {"format": vc.FORMAT, "set": "two", "cases": cs}, self.bins, out)
        self.assertEqual(r["view_not_built"], [])
        self.assertEqual(r["scores"]["view"]["by_class"],
                         {"callback_here": {"held": 1}, "callback_elsewhere": {"held": 1}})
        self.assertEqual(r["held_folded"], {})
        self.assertEqual(json.loads((out / "held-view.json").read_text())["held"],
                         {"here-01": ["h1", "h2", "h3"], "else-01": ["h1", "h2"]})

    def test_replay_reads_each_turns_view_record_on_the_real_daemon(self):
        out = self.tmp / "replay"
        self.quiet(replay.main, ["--bin-dir", str(self.bins), "--store", str(self.copy), "--place", self.place,
                                 "--turns", "5", "--out", str(out)])
        s = json.loads((out / "summary.json").read_text())
        self.assertEqual((s["exchanges"], s["with_a_view_record"], s["no_view_record"]), (3, 3, 0))
        self.assertEqual(s["previous_exchange"]["view"], {"no_previous": 1, "held": 2})
        self.assertGreater(s["tokens"]["view"]["sum"]["total"], 0)
        self.assertGreater(s["pieces_by_kind"]["exchanges"], 0)

    def test_yardstick_on_a_store_reads_the_view_record_on_the_real_daemon(self):
        _, printed = self.quiet(ys.main, ["--store", str(self.copy), "--bin-dir", str(self.bins), "--place",
                                          self.place, "--out", str(self.tmp / "yard"), "--dry-run",
                                          "--cap-usd", "1"])
        v = json.loads(printed)
        self.assertEqual((v["rules_basis"], v["probes"], v["no_view_record"]), ("view_record", 2, 0))
        with contextlib.redirect_stderr(io.StringIO()), \
                scratch.Scratch(self.bins, self.tmp / "yard-probes", store=self.copy) as d:
            probes, _, _ = ys.probes_from_store(d, self.place, 5, 5)
        # The newest turn's view laid both exchanges before it.
        self.assertEqual(probes[-1]["rules"], set(probes[-1]["ids"]))

    def test_proof_runs_one_case_in_one_context_on_the_real_daemon(self):
        out = self.tmp / "proof"
        code, printed = self.quiet(proof.main, ["--bin-dir", str(self.bins), "--out", str(out), "--cases",
                                                "message_and_follow_up", "--cost-turns", "2", "--expect-cut"])
        self.assertEqual(code, 0, printed)
        self.assertIn("case message_and_follow_up: ok", printed)
        self.assertIn("accepted: every driven case ok", printed)
        self.assertEqual(len(json.loads((out / "proof.json").read_text())["cost"]["rows"]), 2)


@unittest.skipUnless(NEW_ENOUGH, "the view kit reads TOML with tomllib: Python 3.11 or newer")
class Binaries(unittest.TestCase):
    """The choice of binary: a copy of another tree's is refused, with the reason."""

    def test_a_binary_of_another_commit_is_named_and_refused(self):
        tmp = Path(tempfile.mkdtemp(prefix="view-kit-bins-"))
        self.addCleanup(shutil.rmtree, tmp, True)
        (tmp / "theseus").write_text("#!/bin/sh\necho 0000000000000000000000000000000000000000\n")
        (tmp / "theseusd").write_bytes(b"theseus-build-stamp:0000000000000000000000000000000000000000:")
        for b in ("theseus", "theseusd"):
            os.chmod(tmp / b, 0o755)
        bins, why = this_trees_bins(tmp)
        self.assertIsNone(bins)
        if git("rev-parse", "HEAD") and not git("status", "--porcelain", "--", "crates", "Cargo.toml",
                                                "Cargo.lock"):
            self.assertIn("was built from 0000000000000000000000000000000000000000", why)
        self.assertEqual(this_trees_bins(tmp / "none")[0], None)


if __name__ == "__main__":
    unittest.main()
