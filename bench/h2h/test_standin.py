"""Tests for the stand-in's rules and its log's reader (theseus-7gir.13).

    python3 -m unittest discover -s bench/h2h -p 'test_*.py'
"""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import standin  # noqa: E402

LOG = HERE / "fixtures" / "standin.jsonl"


class ReadLog(unittest.TestCase):
    def test_every_whole_line_in_arrival_order(self):
        entries = standin.read_log(LOG)
        self.assertEqual(len(entries), 7)  # the torn last line is skipped
        arrivals = [e["arrival_ns"] for e in entries]
        self.assertEqual(arrivals, sorted(arrivals))

    def test_a_line_without_the_optional_keys_takes_their_defaults(self):
        old = next(e for e in standin.read_log(LOG) if e["seq"] == 9)
        self.assertEqual((old["side"], old["tools"], old["opening"], old["step"], old["conn_req"]),
                         (False, [], "", 0, 0))

    def test_a_missing_log_is_no_requests(self):
        self.assertEqual(standin.read_log(HERE / "fixtures" / "no-such.jsonl"), [])


class Rules(unittest.TestCase):
    def test_both_arms_get_the_same_steps_in_their_own_tools(self):
        notes = Path("/work/notes.txt")
        th, cc = standin.rules("theseus", notes), standin.rules("claude-code", notes)
        self.assertEqual([r["when"] for r in th], [r["when"] for r in cc])
        self.assertEqual([len(r["steps"]) for r in th], [len(r["steps"]) for r in cc])
        task_th = next(r for r in th if r["when"] == "h2h-task3")["steps"]
        task_cc = next(r for r in cc if r["when"] == "h2h-task3")["steps"]
        self.assertEqual([s["calls"][0]["name"] for s in task_th[:3]], ["fs_read", "fs_edit", "proc_run"])
        self.assertEqual([s["calls"][0]["name"] for s in task_cc[:3]], ["Read", "Edit", "Bash"])
        self.assertEqual(task_th[0]["calls"][0]["input"], {"path": "/work/notes.txt"})
        self.assertEqual(task_cc[0]["calls"][0]["input"], {"file_path": "/work/notes.txt"})
        self.assertEqual(task_th[2]["calls"][0]["input"], {"argv": ["wc", "-l", "/work/notes.txt"]})
        self.assertIn("{marker}", task_th[-1]["text"])
        self.assertTrue(task_th[-1]["text"].startswith("task-{marker} "))

    def test_step_kinds_label_the_tool_calls(self):
        self.assertEqual(standin.step_kinds("h2h-task3"), ["read", "edit", "shell"])
        self.assertEqual(standin.step_kinds("h2h-hello"), [])

    def test_the_rules_file_is_json_with_the_timing(self):
        with tempfile.TemporaryDirectory() as d:
            p = standin.write_rules(Path(d) / "r.json", "theseus", Path("/w/notes.txt"), ttfb_ms=50, chunks=4,
                                    chunk_ms=10)
            rules = json.loads(p.read_text())
            self.assertTrue(all(r["ttfb_ms"] == 50 and r["chunks"] == 4 for r in rules))
            self.assertEqual(rules[-1]["when"], "")  # the fallback last: first match wins


if __name__ == "__main__":
    unittest.main()
