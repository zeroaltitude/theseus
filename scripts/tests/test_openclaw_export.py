"""The exporter's tests (python3 -m unittest scripts/tests/test_openclaw_export.py from
the repository root; theseusd's tests/import_sync.rs runs them in the gate)."""

import hashlib
import io
import json
import os
import shutil
import sys
import tempfile
import unittest
from contextlib import redirect_stdout

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.dirname(HERE))
import importlib.util

import openclaw_fixture as fx

spec = importlib.util.spec_from_file_location("exporter", os.path.join(os.path.dirname(HERE), "openclaw-export.py"))
exporter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(exporter)

FLOOR = fx.T0 - 1000


def run(state, cursor_in=None, extra=()):
    """Run the exporter: its episode lines and its report."""
    tmp = tempfile.mkdtemp()
    try:
        rep = os.path.join(tmp, "report.json")
        argv = ["--from", state, "--report", rep, "--now-ms", str(fx.T0 + 86_400_000),
                "--owner-id", "U-OWNER"]
        if cursor_in:
            cj = os.path.join(tmp, "cursor.json")
            with open(cj, "w") as f:
                json.dump(cursor_in, f)
            argv += ["--cursor-in", cj]
        else:
            argv += ["--since-ms", str(FLOOR)]
        buf = io.StringIO()
        with redirect_stdout(buf):
            exporter.main(argv + list(extra))
        eps = [json.loads(l) for l in buf.getvalue().splitlines() if l.strip()]
        with open(rep) as f:
            return eps, json.load(f)
    finally:
        shutil.rmtree(tmp)


class Export(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.mkdtemp()
        self.db = fx.build(self.dir)

    def tearDown(self):
        self.db.close()
        shutil.rmtree(self.dir)

    def by_unit(self, eps):
        return {e["messages"][0]["unit"].split("/")[1].split("#")[0]: e for e in eps}

    def test_each_session_is_one_episode_with_its_place_and_its_authors(self):
        eps, rep = run(self.dir)
        got = self.by_unit(eps)
        self.assertEqual(set(got), {"s-dm", "s-ch", "s-cron", "s-zstd", "s-gone"})
        dm = got["s-dm"]
        self.assertEqual((dm["place"]["kind"], dm["labels"]["sensitivity"]), ("dm", "personal"))
        self.assertEqual(dm["place"]["name"], "wren")
        self.assertEqual([(m["idx"], m["author"], m["integrity"]) for m in dm["messages"]],
                         [(2, "operator", "operator"), (3, "agent:main", "agent")])
        self.assertEqual(dm["messages"][0]["text"], "Where does the tide log live?")
        ch = got["s-ch"]
        self.assertEqual((ch["place"]["kind"], ch["labels"]["sensitivity"]), ("slack-channel", "company-confidential"))
        self.assertEqual([(m["author"], m["integrity"]) for m in ch["messages"]],
                         [("person:Quill", "outside"), ("operator", "operator"), ("agent:main", "agent")])
        self.assertEqual(got["s-cron"]["place"]["kind"], "cron")
        self.assertEqual(got["s-cron"]["messages"][0]["author"], "schedule")
        self.assertEqual(got["s-zstd"]["place"]["kind"], "cli")
        for e in eps:
            self.assertIsNone(e["labels"]["partner"])
            self.assertIsNone(e["summary"])
            self.assertEqual(e["source"], "openclaw-sync")
            self.assertEqual(e["hash"], exporter.canonical_hash(e))
            for m in e["messages"]:
                self.assertEqual(m["sha256"], hashlib.sha256(m["text"].encode()).hexdigest())

    def test_the_id_is_the_sessions_and_the_tag_is_the_runs_month(self):
        eps, _ = run(self.dir)
        dm = self.by_unit(eps)["s-dm"]
        want = "ep_" + hashlib.sha256(b"openclaw-sync\0main\0s-dm").hexdigest()
        self.assertEqual(dm["episode_id"], want)
        self.assertEqual(dm["import_tag"], "openclaw-sync-2026-09")

    def test_skipped_events_are_counted_and_an_unknown_one_too(self):
        _, rep = run(self.dir)
        sk = rep["agents"]["main"]["skipped"]
        self.assertEqual(sk["custom"], 1)
        self.assertEqual(sk["session"], 2)  # s-dm's header and the archive's
        self.assertEqual(sk["thinking_level_change"], 1)
        self.assertEqual(sk["brand_new_thing"], 1)

    def test_a_deleted_session_is_read_from_its_archive_once(self):
        eps, rep = run(self.dir)
        gone = self.by_unit(eps)["s-gone"]
        self.assertEqual([m["idx"] for m in gone["messages"]], [1, 2])
        self.assertEqual(rep["agents"]["main"]["archives"], 1)
        eps2, _ = run(self.dir, rep["cursor"])
        self.assertEqual(eps2, [])

    def test_a_zstd_event_is_decoded(self):
        if fx.zstd_blob("x") is None:
            self.skipTest("no node to make a zstd frame")
        eps, rep = run(self.dir)
        z = self.by_unit(eps)["s-zstd"]
        self.assertEqual(z["messages"][0]["text"], "A compressed question about the reef survey.")
        self.assertEqual(rep["zstd_failed"], 0)

    def test_an_event_that_cannot_be_decoded_keeps_its_session_out_and_is_an_error(self):
        if fx.zstd_blob("x") is None:
            self.skipTest("no node to make a zstd frame")
        real = exporter.Zstd.decode
        exporter.Zstd.decode = lambda self, blobs: [None] * len(blobs)
        try:
            eps, rep = run(self.dir)
        finally:
            exporter.Zstd.decode = real
        self.assertNotIn("s-zstd", self.by_unit(eps))
        self.assertIn("s-zstd", " ".join(rep["agents"]["main"]["errors"]))
        # The other sessions were exported whole.
        self.assertIn("s-dm", self.by_unit(eps))

    def test_the_planted_secret_is_passed_on_for_the_daemon_to_scrub(self):
        eps, _ = run(self.dir)
        text = self.by_unit(eps)["s-dm"]["messages"][1]["text"]
        self.assertIn(fx.token(), text)

    def test_a_second_run_with_the_cursor_reads_nothing(self):
        _, rep = run(self.dir)
        eps, rep2 = run(self.dir, rep["cursor"])
        self.assertEqual((eps, rep2["episodes"]), ([], 0))
        self.assertEqual(rep2["cursor"], rep["cursor"])
        self.assertEqual(rep2["agents"]["main"]["sessions"], 0)

    def test_the_cursor_reads_only_the_sessions_that_changed(self):
        _, rep = run(self.dir)
        fx.event(self.db, "s-dm", 5, fx.msg("user", fx.envelope("U-OWNER", "Wren", "And the quay's?")), fx.T0 + 99)
        eps, rep2 = run(self.dir, rep["cursor"])
        self.assertEqual(len(eps), 1)
        self.assertEqual(rep2["agents"]["main"]["sessions"], 1)
        # The whole session again, its new message last: the daemon appends it.
        self.assertEqual([m["idx"] for m in eps[0]["messages"]], [2, 3, 5])
        self.assertEqual(eps[0]["episode_id"], self.by_unit(run(self.dir)[0])["s-dm"]["episode_id"])
        self.assertGreater(rep2["cursor"]["agents"]["main"]["events_rowid"],
                           rep["cursor"]["agents"]["main"]["events_rowid"])

    def test_a_rewritten_event_is_exported_again_with_its_new_text(self):
        _, rep = run(self.dir)
        ev = fx.msg("user", fx.envelope("U-OTHER", "Quill", "Can anyone move the quay survey today?"))
        self.db.execute("UPDATE transcript_events SET event_json=? WHERE session_id='s-ch' AND seq=0", (json.dumps(ev),))
        self.db.execute("INSERT INTO transcript_rewrite_watermarks VALUES ('s-ch','g2',?)", (fx.T0 + 500,))
        eps, rep2 = run(self.dir, rep["cursor"])
        self.assertEqual(len(eps), 1)
        self.assertIn("today?", eps[0]["messages"][0]["text"])

    def test_nothing_before_the_floor_is_exported(self):
        eps, rep = run(self.dir)
        # The floor is the first run's start, kept in the cursor.
        self.assertEqual(rep["cursor"]["floor_ms"], FLOOR)
        later = fx.T0 + 15
        d2 = tempfile.mkdtemp()
        try:
            db2 = fx.build(d2)
            tmp = tempfile.mkdtemp()
            rep_path = os.path.join(tmp, "r.json")
            buf = io.StringIO()
            with redirect_stdout(buf):
                exporter.main(["--from", d2, "--since-ms", str(later), "--report", rep_path, "--owner-id", "U-OWNER"])
            got = [json.loads(l) for l in buf.getvalue().splitlines()]
            units = {e["messages"][0]["unit"] for e in got}
            self.assertFalse(any("s-dm" in u or "s-ch" in u for u in units), units)
            self.assertTrue(any("s-cron" in u for u in units))
            db2.close()
            shutil.rmtree(tmp)
        finally:
            shutil.rmtree(d2)

    def test_a_first_run_needs_its_start_and_a_cursor_refuses_another(self):
        with self.assertRaises(SystemExit):
            exporter.main(["--from", self.dir, "--report", os.devnull])
        _, rep = run(self.dir)
        with self.assertRaises(SystemExit):
            run(self.dir, rep["cursor"], ["--since-ms", "5"])

    def test_the_store_is_opened_read_only_and_left_as_it_was(self):
        path = os.path.join(self.dir, "agents/main/agent/openclaw-agent.sqlite")
        self.db.close()
        def digest():
            with open(path, "rb") as f:
                return hashlib.sha256(f.read()).hexdigest()

        before = digest()
        run(self.dir)
        self.assertEqual(digest(), before)
        self.db = fx.sqlite3.connect(path, isolation_level=None)

    def test_a_dry_run_counts_and_writes_no_episode(self):
        eps, rep = run(self.dir, None, ["--dry-run"])
        self.assertEqual(eps, [])
        self.assertEqual(rep["episodes"], 5)

    def test_an_agent_filter_and_a_missing_agent(self):
        eps, rep = run(self.dir, None, ["--agents", "other"])
        self.assertEqual((eps, rep["agents"]), ([], {}))


if __name__ == "__main__":
    unittest.main()
