"""Tests for the task catalogue (theseus-hbh4m).

    python3 -m unittest discover -s bench/report

The fixture is an invented two-task tree built in a temporary directory (the real one lives outside the repository):
a second build must be byte-identical, `--no-summaries` must drop every summary and nothing else, a `task.toml` with
no `[metadata]` must still list its task, and no author is ever copied.
"""

from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent


def load(name: str, file: str):
    spec = importlib.util.spec_from_file_location(name, HERE / file)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[name] = mod
    spec.loader.exec_module(mod)
    return mod


tasks = load("bench_report_tasks_t", "tasks.py")

ALPHA = """version = "1.0"

[metadata]
author_name = "An Invented Person"
difficulty = "hard"
category = "software-engineering"
tags = ["parsing", "c"]
expert_time_estimate_min = 60
junior_time_estimate_min = 240

[verifier]
timeout_sec = 900.0

[agent]
timeout_sec = 1800.0

[environment]
cpus = 2
memory = "4G"
"""


def tree(root: Path) -> Path:
    t = root / "tree"
    (t / "alpha-task").mkdir(parents=True)
    (t / "alpha-task" / "task.toml").write_text(ALPHA)
    (t / "alpha-task" / "instruction.md").write_text("# Heading\n\nWrite a parser for the invented\nformat. Then test it. More words follow.\n")
    (t / "beta-task").mkdir()
    (t / "beta-task" / "task.toml").write_text("version = \"1.0\"\n")
    (t / "beta-task" / "instruction.md").write_text("x" * 500)
    return t


class Tasks(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        self.t = tree(self.root)

    def tearDown(self):
        self._tmp.cleanup()

    def test_a_second_build_is_byte_identical(self):
        a, b = tasks.build(self.t), tasks.build(self.t)
        self.assertEqual(a, b)
        self.assertTrue(a.endswith("}\n"))
        self.assertEqual(a, json.dumps(json.loads(a), indent=1, ensure_ascii=False, sort_keys=True) + "\n")

    def test_the_facts_go_in_whole_and_no_author_is_copied(self):
        doc = json.loads(tasks.build(self.t))
        a = doc["tasks"]["alpha-task"]
        self.assertEqual(doc["version"], 1)
        self.assertEqual((a["difficulty"], a["category"], a["tags"]), ("hard", "software-engineering", ["c", "parsing"]))
        self.assertEqual((a["expert_min"], a["junior_min"], a["timeout_s"], a["cpus"], a["memory"]), (60, 240, 1800.0, 2, "4G"))
        self.assertNotIn("Invented", json.dumps(doc))

    def test_the_summary_is_the_first_sentence_cut_to_200(self):
        doc = json.loads(tasks.build(self.t))
        self.assertEqual(doc["tasks"]["alpha-task"]["summary"], "# Heading Write a parser for the invented format.")
        s = doc["tasks"]["beta-task"]["summary"]
        self.assertEqual(len(s), 200)
        self.assertTrue(s.endswith("…"))

    def test_no_summaries_drops_every_summary_and_nothing_else(self):
        with_s, without = json.loads(tasks.build(self.t)), json.loads(tasks.build(self.t, summaries=False))
        self.assertTrue(all("summary" not in e for e in without["tasks"].values()))
        for k, e in with_s["tasks"].items():
            e = dict(e)
            e.pop("summary", None)
            self.assertEqual(e, without["tasks"][k])
        self.assertEqual({k: v for k, v in with_s.items() if k != "tasks"}, {k: v for k, v in without.items() if k != "tasks"})

    def test_a_task_without_metadata_is_listed_not_failed(self):
        beta = json.loads(tasks.build(self.t, summaries=False))["tasks"]["beta-task"]
        self.assertEqual(beta, {"title": "beta task"})
        (self.t / "beta-task" / "task.toml").write_text("not = [valid")
        self.assertIn("beta-task", json.loads(tasks.build(self.t))["tasks"])
        (self.t / "beta-task" / "task.toml").unlink()
        self.assertIn("beta-task", json.loads(tasks.build(self.t))["tasks"])

    def test_source_is_written_only_from_an_explicit_base(self):
        self.assertTrue(all("source" not in e for e in json.loads(tasks.build(self.t))["tasks"].values()))
        got = json.loads(tasks.build(self.t, source_base="https://example.test/tasks/"))["tasks"]
        self.assertEqual(got["alpha-task"]["source"], "https://example.test/tasks/alpha-task")

    def test_the_command_writes_the_file(self):
        out = self.root / "tasks.json"
        self.assertEqual(tasks.main(["build", "--tasks", str(self.t), "--out", str(out), "--no-summaries"]), 0)
        self.assertEqual(out.read_text(), tasks.build(self.t, summaries=False))
        self.assertEqual(tasks.main(["build", "--tasks", str(self.root / "nope"), "--out", str(out)]), 1)


if __name__ == "__main__":
    unittest.main()
