"""The TTL reaper's inline code, run offline against a fake ECS (theseus-hands.yaml's Reaper).

    python3 -m unittest discover -s infra/aws/test -p 'test_*.py'
"""
import contextlib
import datetime
import io
import json
import os
import sys
import types
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import rules  # noqa: E402

HANDS = Path(__file__).resolve().parent.parent / "theseus-hands.yaml"
CLUSTER = "theseus-hands"
NOW = datetime.datetime.now(datetime.timezone.utc)


def stamp(delta_minutes, zone="Z"):
    t = NOW + datetime.timedelta(minutes=delta_minutes)
    text = t.strftime("%Y-%m-%dT%H:%M:%S")
    return text + zone if zone is not None else text


def task(n, owner="theseus", ttl=None):
    tags = []
    if owner is not None:
        tags.append({"key": "theseus:owner", "value": owner})
    if ttl is not None:
        tags.append({"key": "theseus:ttl", "value": ttl})
    return {"taskArn": f"arn:aws:ecs:us-west-2:123456789012:task/{CLUSTER}/{n:032x}", "tags": tags}


class FakeEcs:
    """ListTasks in pages of 150, DescribeTasks of at most 100, and StopTask recorded."""

    def __init__(self, tasks):
        self.tasks = {t["taskArn"]: t for t in tasks}
        self.stopped = []

    def get_paginator(self, name):
        assert name == "list_tasks", name
        arns = list(self.tasks)

        class Pages:
            def paginate(self, cluster, desiredStatus):
                assert (cluster, desiredStatus) == (CLUSTER, "RUNNING")
                for i in range(0, max(len(arns), 1), 150):
                    yield {"taskArns": arns[i:i + 150]}

        return Pages()

    def describe_tasks(self, cluster, tasks, include):
        assert cluster == CLUSTER and include == ["TAGS"] and 0 < len(tasks) <= 100
        return {"tasks": [self.tasks[a] for a in tasks]}

    def stop_task(self, cluster, task, reason):
        assert cluster == CLUSTER and len(reason) <= 255 and reason.startswith("theseus ttl reaper")
        self.stopped.append(task)


def run_reaper(tasks, mode="act", logs=None):
    """Runs the template's own inline code against a FakeEcs, as Lambda would import it. Its log
    lines (JSON, one per line) are appended to logs, when given."""
    code = rules.load(HANDS)["Resources"]["Reaper"]["Properties"]["Code"]["ZipFile"]
    ecs = FakeEcs(tasks)
    boto3 = types.ModuleType("boto3")
    boto3.client = lambda service: ecs if service == "ecs" else None
    namespace = {"__name__": "index"}
    out = io.StringIO()
    with mock.patch.dict(sys.modules, {"boto3": boto3}), mock.patch.dict(os.environ, {"CLUSTER": CLUSTER, "MODE": mode}):
        with contextlib.redirect_stdout(out):
            exec(compile(code, "index.py", "exec"), namespace)  # noqa: S102
            summary = namespace["handler"]({}, None)
    if logs is not None:
        logs.extend(json.loads(line) for line in out.getvalue().splitlines())
    return ecs, summary


class ReaperTests(unittest.TestCase):
    def test_stops_only_expired_theseus_tasks(self):
        expired = task(1, ttl=stamp(-5))
        naive_expired = task(2, ttl=stamp(-5, zone=None))
        offset_expired = task(3, ttl=stamp(-5, zone="+00:00"))
        not_yet = task(4, ttl=stamp(+30))
        foreign = task(5, owner=None, ttl=stamp(-60))
        other_owner = task(6, owner="nimbus", ttl=stamp(-60))
        no_ttl = task(7)
        bad_ttl = task(8, ttl="tomorrow")
        ecs, summary = run_reaper([expired, naive_expired, offset_expired, not_yet, foreign, other_owner, no_ttl, bad_ttl])
        self.assertEqual(
            sorted(ecs.stopped),
            sorted(t["taskArn"] for t in (expired, naive_expired, offset_expired)),
        )
        self.assertEqual(summary, {"seen": 8, "expired": 3, "mode": "act"})

    def test_report_mode_stops_nothing(self):
        logs = []
        expired = task(1, ttl=stamp(-5))
        ecs, summary = run_reaper([expired, task(2, ttl=stamp(+5))], mode="report", logs=logs)
        self.assertEqual(ecs.stopped, [])
        self.assertEqual(summary, {"seen": 2, "expired": 1, "mode": "report"})
        self.assertEqual([line.get("would_reap") for line in logs[:-1]], [expired["taskArn"]])
        self.assertEqual(logs[-1], summary)

    def test_logs_each_stop(self):
        logs = []
        expired = task(1, ttl=stamp(-5))
        run_reaper([expired], logs=logs)
        self.assertEqual(logs[0]["reaped"], expired["taskArn"])

    def test_pages_and_batches(self):
        # 250 tasks: two ListTasks pages (150 + 100), DescribeTasks in batches of at most 100.
        tasks = [task(n, ttl=stamp(-1 if n % 2 else +60)) for n in range(250)]
        ecs, summary = run_reaper(tasks)
        self.assertEqual(summary["seen"], 250)
        self.assertEqual(len(ecs.stopped), 125)

    def test_empty_cluster(self):
        ecs, summary = run_reaper([])
        self.assertEqual((ecs.stopped, summary["seen"]), ([], 0))


if __name__ == "__main__":
    unittest.main()
