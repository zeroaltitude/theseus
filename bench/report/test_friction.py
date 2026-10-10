"""Tests for T4 (tool friction) and T5 (cost and calls per solve), on small
fixtures of invented ATIF trajectories (theseus-w052 5).

    python3 -m unittest discover -s bench/report

Standard library only.
"""

from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("bench_report_friction", HERE / "friction.py")
fr = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = fr
spec.loader.exec_module(fr)


def step(calls: list[tuple[str, str, str, dict | None]], source: str = "agent") -> dict:
    """An agent step with calls `(id, tool, result text, result extra)`."""
    return {"step_id": 1, "source": source, "message": "m",
            "tool_calls": [{"tool_call_id": i, "function_name": t, "arguments": {}} for i, t, _, _ in calls],
            "observation": {"results": [{"source_call_id": i, "content": text, **({"extra": x} if x else {})}
                                        for i, _, text, x in calls]}}


ERR = {"status": "error"}
THESEUS = {"steps": [
    {"step_id": 1, "source": "user", "message": "do it"},
    # a command's own failure: not the tool's
    step([("a", "proc.run", "[exit code 2] make: *** error", ERR),
          ("b", "fs.read", "contents", {"status": "ok"}),
          # malformed: the JSON the tool refused
          ("c", "fs.write", '{"INVALID_JSON": "{\\"path\\": "}', ERR),
          # a field the schema lacks
          ("d", "fs.read", "Invalid input: unknown field `lines`, expected one of `path`, `offset`", ERR),
          # a tool the arm does not offer
          ("e", "fs.grep", "Unknown tool `fs.grep`. Available: fs.read, fs.write.", ERR),
          # an ordinary refusal at the tool: a tool-level error, no finer
          ("f", "fs.read", "no such file: /x", ERR)]),
    # a user step's results are never read
    step([("z", "proc.run", "Unknown tool `x`", ERR)], source="user"),
]}
CLAUDE = {"steps": [
    step([("a", "Bash", "Exit code 1\nboom", {"tool_result_is_error": True}),
          ("b", "Read", "<tool_use_error>InputValidationError: file_path is required</tool_use_error>",
           {"tool_result_is_error": True}),
          ("c", "Edit", "<tool_use_error>InputValidationError: An unexpected parameter `lines` was provided</tool_use_error>",
           {"tool_result_is_error": True}),
          ("d", "Grep2", "Error: No such tool available: Grep2", {"tool_result_is_error": True}),
          ("e", "Read", "fine", {"tool_result_is_error": False}),
          ("f", "Bash", "<tool_use_error>Exit code 3</tool_use_error>", {"is_error": True})]),
]}
PI = {"steps": [
    step([("a", "bash", "command exited with 1", {"isError": True}),
          ("b", "edit", 'Validation failed for tool "edit":\n  - oldText: required', {"is_error": True}),
          ("c", "read", "must NOT have additional properties", {"is_error": True}),
          ("d", "nope", "Tool nope not found", {"is_error": True})]),
]}


class Scan(unittest.TestCase):
    def test_a_commands_own_failure_is_not_the_tools(self):
        s = fr.scan(THESEUS)
        self.assertEqual(s["calls"], 6)
        self.assertEqual(s["tool_errors"], 4, "the exit code, and the user step's result, are not counted")
        self.assertEqual(s["malformed"], {"fs.write": 1})
        self.assertEqual(s["invented"], {"fs.read": 1, "fs.grep": 1})

    def test_a_result_is_an_error_by_its_status_or_either_flag(self):
        self.assertTrue(fr.is_error({"extra": {"status": "error"}}))
        self.assertTrue(fr.is_error({"extra": {"is_error": True}}))
        self.assertTrue(fr.is_error({"extra": {"tool_result_is_error": True}}))
        self.assertTrue(fr.is_error({"extra": {"toolName": "bash", "isError": True}}), "Pi's own key")
        self.assertFalse(fr.is_error({"extra": {"status": "ok", "is_error": False}}))
        self.assertFalse(fr.is_error({"content": "Unknown tool"}))
        self.assertFalse(fr.is_error({}))

    def test_claude_codes_refusals_are_told_apart_and_its_exit_codes_are_the_commands(self):
        s = fr.scan(CLAUDE)
        self.assertEqual(s["calls"], 6)
        self.assertEqual(s["tool_errors"], 3)
        self.assertEqual(s["malformed"], {"Read": 1})
        self.assertEqual(s["invented"], {"Edit": 1, "Grep2": 1})

    def test_pis_refusals_are_told_apart(self):
        s = fr.scan(PI)
        self.assertEqual((s["tool_errors"], s["malformed"], s["invented"]),
                         (4, {"edit": 1}, {"read": 1, "nope": 1}))
        self.assertEqual(fr.kind("command exited with 1"), "error", "Pi words a failed command no way we can tell")

    def test_a_trial_without_a_trajectory_is_none_and_one_with_is_read(self):
        with tempfile.TemporaryDirectory() as d:
            self.assertIsNone(fr.scan_dir(Path(d)))
            (Path(d) / "agent").mkdir()
            (Path(d) / "agent" / "trajectory.json").write_text(json.dumps(THESEUS))
            self.assertEqual(fr.scan_dir(Path(d))["tool_errors"], 4)
            (Path(d) / "agent" / "trajectory.json").write_text("{")
            self.assertIsNone(fr.scan_dir(Path(d)))


class T4(unittest.TestCase):
    def test_counts_and_shares_over_the_trials_with_a_trajectory(self):
        clean = fr.scan({"steps": [step([("a", "fs.read", "ok", {"status": "ok"})])]})
        t = fr.t4([fr.scan(THESEUS), fr.scan(THESEUS), clean, None])
        self.assertEqual((t["trials"], t["with_trajectory"]), (4, 3))
        self.assertEqual(t["trials_with_tool_error"], 2)
        self.assertAlmostEqual(t["share_tool_error"], 2 / 3)
        self.assertEqual((t["tool_errors"], t["malformed"], t["invented"]), (8, 2, 4))
        self.assertEqual(t["malformed_by_tool"], {"fs.write": 2})
        self.assertEqual(t["invented_by_tool"], {"fs.read": 2, "fs.grep": 2})
        self.assertEqual((t["trials_with_malformed"], t["trials_with_invented"]), (2, 2))
        self.assertIsNone(fr.t4([None])["share_tool_error"])

    def test_the_table_names_each_row(self):
        t = fr.t4([fr.scan(THESEUS)])
        md = fr.t4_table({"theseus": t}, {"theseus": "Theseus"})
        self.assertIn("| Trials with at least one tool-level error | 1 (100.0%) |", md)
        self.assertIn("| Malformed inputs, by tool | fs.write 1 |", md)
        self.assertIn("| Invented names, by tool | fs.read 1, fs.grep 1 |", md)


def trial(task: str, solved: bool, cost: float, calls: int, out: int, wall: float) -> dict:
    return {"task": task, "solved": solved, "wall_s": wall,
            "record": {"cost_usd": cost, "model_calls": calls, "tokens": {"output": out}}}


class T5(unittest.TestCase):
    def test_only_the_pairs_every_arm_solved_count(self):
        # Two attempts of two tasks. Pairs both arms solved: (a,1) and (b,2).
        theseus = [[trial("a", True, 0.10, 10, 1000, 60), trial("a", False, 9, 99, 1, 1)],
                   [trial("b", False, 9, 99, 1, 1), trial("b", True, 0.30, 20, 3000, 120)]]
        claude = [[trial("a", True, 0.20, 20, 4000, 100), trial("a", True, 5, 50, 1, 1)],
                  [trial("b", True, 7, 70, 1, 1), trial("b", True, 0.40, 40, 2000, 200)]]
        t = fr.t5({"theseus": theseus, "claude-code": claude})
        self.assertEqual(t["pairs"], [{"task": "a", "attempt": 1}, {"task": "b", "attempt": 2}])
        a, c = t["arms"]["theseus"], t["arms"]["claude-code"]
        self.assertEqual(a["pairs"], 2)
        self.assertAlmostEqual(a["cost_mean"], 0.20)
        self.assertAlmostEqual(a["cost_median"], 0.20)
        self.assertEqual(a["calls_mean"], 15)
        self.assertEqual(a["agent_s_mean"], 90)
        self.assertAlmostEqual(a["output_per_call"], 4000 / 30)
        self.assertAlmostEqual(c["cost_mean"], 0.30)
        self.assertEqual(c["calls_mean"], 30)
        self.assertEqual(c["agent_s_mean"], 150)
        self.assertAlmostEqual(c["output_per_call"], 6000 / 60)

    def test_the_median_is_not_the_mean(self):
        arm = [[trial("a", True, 0.1, 1, 1, 1)], [trial("b", True, 0.2, 1, 1, 1)], [trial("c", True, 3.0, 1, 1, 1)]]
        t = fr.t5({"x": arm})
        self.assertAlmostEqual(t["arms"]["x"]["cost_median"], 0.2)
        self.assertAlmostEqual(t["arms"]["x"]["cost_mean"], 1.1)

    def test_nothing_solved_by_all_is_an_empty_table_not_an_error(self):
        t = fr.t5({"x": [[trial("a", True, 1, 1, 1, 1)]], "y": [[trial("a", False, 1, 1, 1, 1)]]})
        self.assertEqual(t["pairs"], [])
        self.assertIsNone(t["arms"]["x"]["cost_mean"])
        self.assertIn("| Pairs every arm solved | 0 | 0 |", fr.t5_table(t, {"x": "X", "y": "Y"}))


if __name__ == "__main__":
    unittest.main()
