"""The async bench's Layer 2 tasks, offline (theseus-2wxa): no Harbor, no Docker.

    python3 -m unittest discover -s bench/async

Each family's oracle runs on this host under a scratch `ASYNC_ROOT` at a
small time scale and earns reward 1, leaving its TMPDIR empty; a planted
wrong effect per family earns 0; a control (and a mixed family) whose
dependent step started before its prerequisite ended earns 0 even when its
answer is right; and the ideal, the overlap and the order violations are
read from the ledgers the oracles leave.
"""

from __future__ import annotations

import json
import os
import sys
import tomllib
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / "tools"))
sys.path.insert(0, str(HERE))

import asyncbench as ab  # noqa: E402
import families  # noqa: E402
import layer2 as l2  # noqa: E402
import sync  # noqa: E402
from test_tasks import HOW, SCALE, TASKS, Trial  # noqa: E402

NAMES = [f.name for f in families.FAMILIES]


class Layout(unittest.TestCase):
    def test_twenty_families_in_their_shapes(self):
        shapes = [f.shape for f in families.FAMILIES]
        self.assertEqual(len(NAMES), 20)
        self.assertEqual(len(set(NAMES)), 20)
        self.assertEqual({s: shapes.count(s) for s in families.SHAPES},
                         {"programs": 8, "reads": 4, "writes": 2, "control": 4, "mixed": 2})
        self.assertEqual(sorted(f.name for f in families.FAMILIES if f.shape == "control"), sorted(l2.CONTROLS))
        self.assertEqual(sorted(l2.CHECKS), sorted(NAMES))
        for f in families.FAMILIES:
            self.assertIsNotNone(l2.ideal(f.name, [{"kind": "start", "tool": t, "step": "x", "pid": 1, "seq": 0,
                                                    "duration": 10.0} for t in f.tools]), f.name)

    def test_every_task_is_syncs_and_says_what_not_how(self):
        expected = sync.expected()
        for f in families.FAMILIES:
            t = TASKS / f.name
            cfg = tomllib.loads((t / "task.toml").read_text())
            self.assertEqual(cfg["metadata"]["async"], {"family": f.name, "layer": 2, "shape": f.shape})
            for rel in ("solution/solve.sh", "tests/test.sh"):
                self.assertTrue(os.access(t / rel, os.X_OK), f"{f.name}/{rel}")
            for tool in f.tools:
                self.assertIn(t / f"environment/async/bin/{tool}", expected)
                self.assertIn(tool, l2.TOOLS)
                lo, hi = l2.DURATIONS[tool]
                self.assertTrue(10 <= lo <= hi <= 45, tool)
            text = (t / "instruction.md").read_text().lower()
            for word in HOW + ["at once", "at the same time", "simultaneous", "parallel"]:
                self.assertNotIn(word, text, f"{f.name}: {word!r}")

    @unittest.skipIf(not os.environ.get("ASYNC_HARBOR"), "set ASYNC_HARBOR=1 under Harbor's python")
    def test_harbor_reads_each_task(self):
        from harbor.models.task.config import TaskConfig

        for f in NAMES:
            TaskConfig.model_validate(tomllib.loads((TASKS / f / "task.toml").read_text()))


class Oracles(unittest.TestCase):
    def test_each_oracle_earns_reward_1_in_order_and_leaves_nothing(self):
        for f in NAMES:
            with self.subTest(family=f):
                t = Trial(f)
                try:
                    r = t.run()
                    self.assertEqual(r.returncode, 0, r.stderr)
                    self.assertEqual(t.left(), [])
                    got = t.check()
                    self.assertEqual((got["reward"], got["problems"]), (1, []))
                    rec = t.ledger()
                    self.assertEqual(l2.order_violations(f, rec), [])
                    self.assertGreater(l2.ideal(f, rec), 0)
                finally:
                    t.close()

    def test_the_oracles_overlap_independent_steps_and_not_dependent_ones(self):
        """At a larger scale, so the steps outlast the processes' starts:
        six digests' worth of independent steps overlap several times over;
        a control's chain runs one step at a time."""
        for f, lo, hi in (("host-logs", 2.0, 6.05), ("doc-questions", 3.0, 8.05), ("migrations", 0.0, 1.0),
                          ("pipeline", 0.0, 1.0)):
            with self.subTest(family=f):
                t = Trial(f, "0.1")
                try:
                    t.run(timeout=600)
                    tools = set(families.BY_NAME[f].tools)
                    o = l2.overlap(t.ledger(), tools)
                    self.assertTrue(lo <= o["overlap"] <= hi, o)
                finally:
                    t.close()


class Plants(unittest.TestCase):
    """A planted wrong effect per family, and the problem the check names."""

    def planted(self, family: str, script: str, problem: str, scale: str = SCALE) -> dict:
        t = Trial(family, scale)
        try:
            t.run(script)
            got = t.check()
            self.assertEqual(got["reward"], 0, got)
            self.assertTrue(any(problem in p for p in got["problems"]), got["problems"])
            return got
        finally:
            t.close()

    def test_suites_with_one_suite_never_run(self):
        self.planted("suites", """
for p in auth billing catalog; do run-suite "$p" >> "$APP/all" & done; wait
awk '$1 == "FAIL" {print $2}' "$APP/all" > "$APP/failing.txt"
""", "run-suite search never finished")

    def test_ci_checks_with_a_count_misread(self):
        self.planted("ci-checks", """
lint > /dev/null & typecheck > /dev/null & unit-tests > /dev/null & wait
printf 'lint 99\\ntypes 99\\ntests 0/0\\n' > "$APP/summary.txt"
""", "summary.txt's lint")

    def test_build_configs_with_a_size_left_out(self):
        self.planted("build-configs", """
for c in debug release minsize; do build-config "$c" >> "$APP/all" & done; wait
sed -n 's/^built \\([a-z]*\\): .*, \\([0-9]*\\) bytes$/\\1 \\2/p' "$APP/all" | grep -v minsize > "$APP/sizes.txt"
""", "sizes.txt's minsize")

    def test_bench_settings_with_the_slowest_chosen(self):
        self.planted("bench-settings", """
for b in 16 32 64 128; do bench-batch "$b" >> "$APP/all" & done; wait
sed -n 's/^batch size \\([0-9]*\\): \\([0-9]*\\) ops\\/s$/\\2 \\1/p' "$APP/all" | sort -n | head -1 \\
  | awk '{print "batch_size = " $2}' > "$APP/bench.toml"
""", "not the best")

    def test_host_logs_with_a_clean_host_named(self):
        self.planted("host-logs", """
for h in web-1 web-2 web-3 web-4 web-5 web-6; do fetch-log "$h" > "$APP/$h.log" & done; wait
basename "$(grep -L ' ERROR ' "$APP"/*.log | head -1)" .log > "$APP/culprit.txt"
""", "culprit.txt names")

    def test_repos_behind_with_every_repository_listed(self):
        self.planted("repos-behind", """
for r in api web worker infra docs; do repo-status "$r" > /dev/null & done; wait
printf 'api\\nweb\\nworker\\ninfra\\ndocs\\n' > "$APP/behind.txt"
""", "behind.txt names")

    def test_csv_merge_with_an_export_left_out(self):
        self.planted("csv-merge", """
for e in jan feb mar apr may; do convert-export "$e" > /dev/null & done; wait
{ echo id,amount; for e in jan feb mar apr; do tail -n +2 "$APP/exports/$e.csv"; done; } > "$APP/merged.csv"
""", "merged.csv holds")

    def test_health_restart_with_every_service_restarted(self):
        self.planted("health-restart", """
for s in auth billing search mail; do health "$s" > "$APP/$s.h" & done; wait
for s in auth billing search mail; do restart "$s" > /dev/null & done; wait
bad=$(basename "$(grep -l 503 "$APP"/*.h)" .h); health "$bad" > /dev/null; echo "$bad" > "$APP/restarted.txt"
""", "a healthy service")

    def test_doc_questions_answered_from_one_note(self):
        self.planted("doc-questions", """
archive-get notes-01 > /dev/null
printf '1 AMBER\\n2 7\\n3 5432\\n' > "$APP/answers.txt"
""", "was never read")

    def test_api_summary_with_the_private_functions_listed(self):
        self.planted("api-summary", """
for m in core io net auth cache util; do
  echo "## $m"; module-source "$m" | sed -n 's/^def \\([a-z_]*\\)(.*/- \\1/p'
done > "$APP/API.md"
""", "lists the private")

    def test_advisories_with_columns_swapped(self):
        self.planted("advisories", """
for a in 1001 1002 1003 1004 1005; do advisory "ADV-$a" > "$APP/$a" & done; wait
for a in 1001 1002 1003 1004 1005; do
  echo "ADV-$a $(sed -n 's/^  fixed in: //p' "$APP/$a") $(sed -n 's/^  severity: //p' "$APP/$a")"
done > "$APP/advisories.txt"
""", "advisories.txt's ADV-1001")

    def test_config_diff_with_every_setting_named(self):
        self.planted("config-diff", """
for e in dev staging prod perf; do env-config "$e" > "$APP/$e" & done; wait
cut -d= -f1 "$APP/dev" > "$APP/differs.txt"
""", "differs.txt names")

    def test_scaffold_with_a_file_changed_after_its_check(self):
        self.planted("scaffold", """
mkdir -p "$APP/config"
awk -v d="$APP/config" '/^## / {f = d "/" $2} /^- / && f {sub(/^- /, ""); print > f}' "$APP/spec.md"
for f in "$APP"/config/*.yaml; do validate-config "$(basename "$f")" > /dev/null & done; wait
echo "debug: true" >> "$APP/config/api.yaml"
""", "config/api.yaml as it stands was never validated")

    def test_rename_left_undone_in_one_module(self):
        self.planted("rename", """
for i in 1 2 3 4 5; do sed -i 's/\\bfetch_rows\\b/load_rows/g' "$APP/src/m$i.py"; done
run-tests > /dev/null
""", "src/m6.py still names fetch_rows")

    def test_bisect_naming_the_good_oldest_commit(self):
        self.planted("bisect", """
for c in $(awk '{print $1}' "$APP/commits.txt"); do (make-worktree "$c" && test-commit "$c") > /dev/null & done
wait; awk 'NR == 1 {print $1}' "$APP/commits.txt" > "$APP/first-bad.txt"
""", "first-bad.txt names")

    def test_link_with_a_made_up_artifact(self):
        self.planted("link", """
build-lib core > /dev/null & build-lib ui > /dev/null & wait; link-app > /dev/null
echo app-0000000000 > "$APP/artifact.txt"
""", "artifact.txt holds")


class Fair(unittest.TestCase):
    """What the instruction says may be trusted: bisect's oldest commit is good."""

    def test_a_search_that_trusts_the_oldest_commit_earns_reward_1(self):
        t = Trial("bisect")
        try:
            # The draw the rule matters for: the second commit is the first bad one.
            st = t.root / "var/lib/async/state"
            st.mkdir(parents=True)
            (st / "world-bisect.json").write_text(json.dumps({"first_bad": 1}))
            t.run("""
for c in $(awk 'NR > 1 {print $1}' "$APP/commits.txt"); do
  (make-worktree "$c" && test-commit "$c") > "$APP/$c.t" & done; wait
for c in $(awk 'NR > 1 {print $1}' "$APP/commits.txt"); do
  if grep -q FAIL "$APP/$c.t"; then echo "$c" > "$APP/first-bad.txt"; break; fi
done
""")
            got = t.check()
            self.assertEqual((got["reward"], got["problems"]), (1, []))
            self.assertNotIn(l2.COMMITS[0], {r["step"] for r in ab.by(t.ledger(), "start", "test-commit")})
        finally:
            t.close()


class Order(unittest.TestCase):
    """A dependent step started before its prerequisite ended is reward 0,
    even when the run then does it right and its answer is right. The rule
    is judged by the ledger's order, so each plant starts the dependent step
    before its prerequisite has run at all: a batch that starts both at once
    breaks the same rule whichever starts first, and this way no load can
    make the plant start late (theseus-2wxa: under four busy loops, a plant
    that started the step while its 1 to 3 s prerequisite ran started it
    after the prerequisite's end)."""

    def early(self, family: str, script: str, problem: str) -> None:
        t = Trial(family, "0.1")
        try:
            t.run(script, timeout=600)
            got = t.check()
            self.assertEqual(got["reward"], 0, got)
            self.assertTrue(any(p.startswith("order violation: " + problem) for p in got["problems"]),
                            got["problems"])
            # The rest of the outcome holds: only the order cost it.
            rest = [p for p in got["problems"] if not p.startswith("order violation")]
            self.assertEqual(rest, [], rest)
            self.assertTrue(l2.order_violations(family, t.ledger()))
        finally:
            t.close()

    def test_pipeline_running_the_app_before_it_is_built(self):
        """The early run is yesterday's binary; the run then builds, runs and
        checks in order, and its verdict is right."""
        self.early("pipeline", """
run-app > /dev/null
build > /dev/null
run-app > /dev/null
check-output | sed -n 's/.*verdict \\([0-9a-f]*\\)$/\\1/p' > "$APP/verdict.txt"
""", "run-app run started")

    def test_service_queried_before_it_starts(self):
        self.early("service", """
query-service stock
start-service > /dev/null
query-service stock | sed -n 's/^in stock: //p' > "$APP/stock.txt"
""", "query-service stock started")

    def test_migrations_out_of_order(self):
        self.early("migrations", """
migrate-db 002
migrate-db 001 > /dev/null
migrate-db 002 > /dev/null
migrate-db 003 | sed -n 's/.*schema checksum //p' > "$APP/schema.txt"
""", "migrate-db 002 started")

    def test_edit_test_testing_before_the_codemod(self):
        self.early("edit-test", """
test-suite
codemod > /dev/null
sed -i 's/legacy_mode/compat_mode/g' "$APP/tests/test_flags.py"
test-suite > /dev/null
""", "test-suite tests started")

    def test_link_before_the_libraries_are_built(self):
        self.early("link", """
link-app
build-lib core > /dev/null & build-lib ui > /dev/null & wait
link-app | sed -n 's/^linked out\\///p' > "$APP/artifact.txt"
""", "link-app link started")

    def test_bisect_testing_before_the_worktree_is_made(self):
        self.early("bisect", """
c1=$(awk 'NR == 1 {print $1}' "$APP/commits.txt")
test-commit "$c1"
for c in $(awk '{print $1}' "$APP/commits.txt"); do (make-worktree "$c" && test-commit "$c") > "$APP/$c.t" & done
wait
for c in $(awk '{print $1}' "$APP/commits.txt"); do
  if grep -q FAIL "$APP/$c.t"; then echo "$c" > "$APP/first-bad.txt"; break; fi
done
""", "test-commit")


class Ideal(unittest.TestCase):
    """The ideal is the critical path of the drawn durations."""

    def rec(self, *steps: tuple[str, str, float]) -> list[dict]:
        out = []
        for i, (tool, step, d) in enumerate(steps):
            out.append({"seq": 2 * i, "kind": "start", "tool": tool, "step": step, "pid": i, "duration": d})
            out.append({"seq": 2 * i + 1, "kind": "end", "tool": tool, "step": step, "pid": i})
        return out

    def test_independent_steps_take_the_longest_and_a_chain_its_sum(self):
        self.assertEqual(l2.ideal("suites", self.rec(("run-suite", "auth", 30.0), ("run-suite", "search", 41.0))),
                         41.0)
        self.assertEqual(l2.ideal("pipeline", self.rec(("build", "build", 20.0), ("run-app", "run", 12.0),
                                                       ("check-output", "check", 11.0))), 43.0)
        self.assertEqual(l2.ideal("link", self.rec(("build-lib", "core", 30.0), ("build-lib", "ui", 22.0),
                                                   ("link-app", "link", 15.0))), 45.0)
        self.assertEqual(l2.ideal("bisect", self.rec(("make-worktree", "a", 12.0), ("test-commit", "a", 20.0),
                                                     ("make-worktree", "b", 14.0), ("test-commit", "b", 30.0))),
                         44.0)

    def test_a_rerun_and_a_refusal_add_nothing(self):
        rec = self.rec(("migrate-db", "001", 10.0), ("migrate-db", "002", 12.0), ("migrate-db", "002", 19.0),
                       ("migrate-db", "003", 11.0))
        rec.insert(2, {"seq": 99, "kind": "start", "tool": "migrate-db", "step": "002", "pid": 50, "duration": 2.0,
                       "refused": True})
        self.assertEqual(l2.ideal("migrations", rec), 33.0)


class Overlap(unittest.TestCase):
    def test_overlap_is_the_slow_steps_time_over_the_slow_phases_wall(self):
        rec = []
        for i, (a, b) in enumerate([(0.0, 10.0), (1.0, 11.0), (2.0, 12.0), (30.0, 40.0)]):
            rec.append({"kind": "start", "tool": "fetch-log", "step": f"h{i}", "pid": i, "mono": 100 + a})
            rec.append({"kind": "end", "tool": "fetch-log", "step": f"h{i}", "pid": i, "mono": 100 + b})
        # Work outside the slow phase: before it, and a tool not counted.
        rec.insert(0, {"kind": "start", "tool": "other", "step": "x", "pid": 9, "mono": 0.0})
        rec.append({"kind": "end", "tool": "other", "step": "x", "pid": 9, "mono": 500.0})
        o = l2.overlap(rec, {"fetch-log"})
        self.assertEqual((o["busy_s"], o["phase_s"], o["overlap"]), (40.0, 40.0, 1.0))
        rec[1:3] = []
        o = l2.overlap(rec, {"fetch-log"})
        self.assertEqual((o["steps"], o["busy_s"], o["phase_s"], o["overlap"]), (3, 30.0, 39.0, 0.769))
        self.assertIsNone(l2.overlap([], {"fetch-log"}))


if __name__ == "__main__":
    unittest.main()
