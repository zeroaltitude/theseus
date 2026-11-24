"""bench/view's measuring kit on the REAL daemon and CLI (issue 46): one test per tool, each on an empty scratch
store the test fills by script on a stand-in model, so the kit cannot drift from the CLI it reads unnoticed again
(`replay.py` once asked `view preview --turns`, a form no build had, and only a fake daemon ever answered it).

The binaries are chosen by `this_trees_bins` (its text says what it checks and what it does not prove). When it
cannot admit them, every test here skips, saying why. Nothing here calls a model provider: the model is a local
stand-in, and the yardstick runs `--dry-run`. The kit reads TOML with `tomllib`, so on a Python older than 3.11 the
whole file says it skipped.
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
import unittest.mock
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

BUILD = "build this tree's (cargo build -p theseus -p theseusd)"
STAMP_TIMEOUT = 30
STAMP = re.compile(rb"theseus-build-stamp:([0-9a-f]{40}):")


def run(argv: list[str], **kw) -> tuple[str | None, str]:
    """`(stdout, "")` when `argv` ran and exited 0, else `(None, why)`: a process error or a timeout is a reason,
    never an exception."""
    try:
        r = subprocess.run(argv, capture_output=True, text=True, timeout=STAMP_TIMEOUT, **kw)
    except subprocess.TimeoutExpired:
        return None, f"{argv[0]} did not answer in {STAMP_TIMEOUT}s (timed out)"
    except OSError as e:
        return None, f"{argv[0]} could not run ({e.strerror or type(e).__name__})"
    except subprocess.SubprocessError as e:
        return None, f"{argv[0]} could not run ({type(e).__name__})"
    if r.returncode != 0:
        return None, f"{' '.join(argv[:4])} exited {r.returncode}"
    return r.stdout.strip(), ""


def this_trees_bins(root: Path = ROOT, bins: Path | None = None) -> Path:
    """`bins` (default: `debug` under `CARGO_TARGET_DIR`, else under `root/target`) when its `theseus` and
    `theseusd` carry the commit `root` is checked out at, and nothing in `root` differs from that commit (a
    tracked change or an untracked file; ignored files, such as the build's own, are not read). Otherwise it
    raises `unittest.SkipTest` with the reason. It FAILS CLOSED: a check it cannot make (git cannot say the
    commit or the tree's state, a binary cannot run or does not answer in STAMP_TIMEOUT, a file is empty or
    holds no stamp, or holds a second) is a skip with its reason, never a pass and never a crash.

    What it does NOT prove: that the binaries were built from this tree. A stamp names the commit the tree was
    at when it was built (`crates/theseus/build.rs`, `crates/theseusd/build.rs`), not what was compiled: a
    binary built from an edited tree that was then restored bears the same stamp, and passes; so does one built
    with `THESEUS_COMMIT` set to this commit, which overrides the stamp. Build inputs outside the checkout (the
    toolchain, cargo's settings in the home directory, the environment) are not read at all. This is an
    identification check, not build provenance."""
    if bins is None:
        bins = Path(os.environ.get("CARGO_TARGET_DIR") or root / "target") / "debug"
    head, why = run(["git", "-C", str(root), "rev-parse", "HEAD"])
    if not head:
        raise unittest.SkipTest(f"{root}: no commit to hold a binary to ({why})")
    dirty, why = run(["git", "-C", str(root), "status", "--porcelain", "--untracked-files=normal"])
    if dirty is None:
        raise unittest.SkipTest(f"{root}: git status failed, so the tree's state is unknown ({why})")
    if dirty:
        raise unittest.SkipTest(f"{root} differs from its commit {head}: no binary's stamp can say it holds that")
    for b in ("theseus", "theseusd"):
        if not (bins / b).is_file():
            raise unittest.SkipTest(f"no {bins / b}: {BUILD}")
    cli, why = run([str(bins / "theseus")], env={"THESEUS_CLI_STAMP": "1", "PATH": "/usr/bin:/bin"})
    if cli is None:
        raise unittest.SkipTest(f"{bins / 'theseus'}: its stamp could not be read: {why}")
    if cli != head:
        raise unittest.SkipTest(f"{bins / 'theseus'} was built from {cli or 'no commit'}, this tree is at {head}: "
                                f"{BUILD}")
    try:
        stamps = STAMP.findall((bins / "theseusd").read_bytes())
    except OSError as e:
        raise unittest.SkipTest(f"{bins / 'theseusd'} could not be read ({e.strerror or e})") from None
    if stamps != [head.encode()]:
        held = ", ".join(s.decode() for s in stamps) or "none (an empty file, or no stamp)"
        raise unittest.SkipTest(f"{bins / 'theseusd'} carries the stamp {held}, this tree is at {head}: {BUILD}")
    return bins


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
        cls.bins = this_trees_bins()
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


OTHER = "0" * 40


def scratch_repo(tmp: Path) -> tuple[Path, str]:
    """A clean git checkout of one commit, made with no user or system git settings; its root and its commit."""
    root = tmp / "repo"
    root.mkdir()
    env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": str(tmp), "GIT_CONFIG_NOSYSTEM": "1",
           "GIT_CONFIG_GLOBAL": os.devnull, "GIT_AUTHOR_NAME": "kit", "GIT_AUTHOR_EMAIL": "kit",
           "GIT_COMMITTER_NAME": "kit", "GIT_COMMITTER_EMAIL": "kit"}
    (root / "f").write_text("f\n")
    for args in (["init", "-q"], ["add", "f"], ["commit", "-q", "-m", "f"]):
        subprocess.run(["git", "-C", str(root), *args], env=env, check=True, capture_output=True)
    head = subprocess.run(["git", "-C", str(root), "rev-parse", "HEAD"], env=env, check=True,
                          capture_output=True, text=True).stdout.strip()
    return root, head


def fake_pair(d: Path, cli: str, daemon: bytes) -> Path:
    """`theseus` (a shell script whose body is `cli`) and `theseusd` (the bytes `daemon`) in `d`."""
    d.mkdir(parents=True, exist_ok=True)
    (d / "theseus").write_text(cli)
    (d / "theseusd").write_bytes(daemon)
    for b in ("theseus", "theseusd"):
        os.chmod(d / b, 0o755)
    return d


@unittest.skipUnless(NEW_ENOUGH, "the view kit reads TOML with tomllib: Python 3.11 or newer")
class Binaries(unittest.TestCase):
    """The choice of binary: a copy of another tree's is refused, with the reason; and a check that cannot be
    made is a skip with its reason, never a pass and never a crash."""

    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="view-kit-bins-"))
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.root, self.head = scratch_repo(self.tmp)

    def refused(self, bins: Path) -> str:
        """The helper's reason for refusing `bins` for the scratch checkout; a failure if it accepts them."""
        with self.assertRaises(unittest.SkipTest) as cm:
            this_trees_bins(self.root, bins)
        return str(cm.exception)

    def test_a_binary_of_another_commit_is_named_and_refused(self):
        bins = fake_pair(self.tmp / "bin", f"#!/bin/sh\necho {OTHER}\n", f"theseus-build-stamp:{OTHER}:".encode())
        self.assertIn(f"was built from {OTHER}", self.refused(bins))
        self.assertIn("no ", self.refused(self.tmp / "none"))
        self.assertEqual(this_trees_bins(self.root, fake_pair(
            self.tmp / "good", f"#!/bin/sh\necho {self.head}\n", f"theseus-build-stamp:{self.head}:".encode())),
            self.tmp / "good")

    def test_a_daemon_of_another_commit_is_refused_though_the_cli_is_this_trees(self):
        bins = fake_pair(self.tmp / "bin", f"#!/bin/sh\necho {self.head}\n", f"theseus-build-stamp:{OTHER}:".encode())
        self.assertIn("theseusd", self.refused(bins))

    def test_a_failed_git_status_is_a_skip_not_a_clean_tree(self):
        bins = fake_pair(self.tmp / "bin", f"#!/bin/sh\necho {self.head}\n",
                         f"theseus-build-stamp:{self.head}:".encode())
        (self.root / ".git" / "index").write_bytes(b"not an index")  # `rev-parse` still answers; `status` fails
        self.assertIn("status", self.refused(bins))

    def test_a_stamp_check_that_times_out_is_a_skip(self):
        bins = fake_pair(self.tmp / "bin", "#!/bin/sh\nexec sleep 60\n", f"theseus-build-stamp:{self.head}:".encode())
        with unittest.mock.patch.object(sys.modules[__name__], "STAMP_TIMEOUT", 1, create=True):
            self.assertIn("time", self.refused(bins))

    def test_a_cli_that_cannot_run_is_a_skip(self):
        bins = fake_pair(self.tmp / "bin", "no interpreter line: not a program\n",
                         f"theseus-build-stamp:{self.head}:".encode())
        self.assertIn("theseus", self.refused(bins))

    def test_an_empty_daemon_file_is_a_skip(self):
        bins = fake_pair(self.tmp / "bin", f"#!/bin/sh\necho {self.head}\n", b"")
        self.assertIn("theseusd", self.refused(bins))


if __name__ == "__main__":
    unittest.main()
