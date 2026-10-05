"""The async bench's driver, offline (theseus-7gir.16): no Harbor, no Docker.

    python3 -m unittest discover -s bench/async

A fake environment runs each command on this host under a scratch
`ASYNC_ROOT`; a stand-in `claude` reads its stream-json input from the FIFO;
a stand-in `theseus` plays a daemon whose job outlives the first turn.
"""

from __future__ import annotations

import asyncio
import json
import os
import stat
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from types import SimpleNamespace

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE / "tools"))
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / "harbor"))

import asyncbench as ab  # noqa: E402
import driver  # noqa: E402
import theseus_bench as tb  # noqa: E402

LIB = str(HERE / "tools/asyncbench.py")


class FakeEnv:
    """Harbor's environment, as the driver uses it: `exec` runs bash here."""

    def __init__(self, root: Path, extra: dict[str, str] | None = None):
        self.env = dict(os.environ, ASYNC_ROOT=str(root), ASYNC_TIME_SCALE="0.01", **(extra or {}))
        self.commands: list[str] = []

    async def exec(self, command, cwd=None, env=None, timeout_sec=None):
        self.commands.append(command)
        p = await asyncio.create_subprocess_exec(
            "bash", "-c", "set -o pipefail; " + command, env={**self.env, **(env or {})},
            stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
        out, err = await asyncio.wait_for(p.communicate(), timeout_sec or 60)
        return SimpleNamespace(stdout=out.decode(), stderr=err.decode(), return_code=p.returncode)


class Injections(unittest.TestCase):
    def test_a_family_with_an_injection_reads_it_from_its_task(self):
        inj = driver.injection_of(HERE / "tasks/interrupt")
        self.assertEqual((inj.after_tool, inj.after_kind, inj.delay_s, inj.at_s),
                         ("train-model", "start", 20.0, 120.0))
        self.assertIn("/app/tickets.txt", inj.message)
        self.assertEqual(driver.injection_of(HERE / "tasks/cancel").after_tool, "migrate")
        self.assertIsNone(driver.injection_of(HERE / "tasks/parallel"))
        self.assertEqual(driver.task_of(HERE / "tasks/fanout")["family"], "fanout")

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.env = FakeEnv(self.root)
        self.delivered: list[tuple[float, str]] = []

    def tearDown(self):
        self.tmp.cleanup()

    async def deliver(self, message: str):
        self.delivered.append((time.monotonic(), message))
        return {"exit_code": 0}

    def test_the_injection_waits_for_its_event_and_the_delay_after_it(self):
        inj = driver.Injection("Also count the tickets.", "train-model", "start", 0.5, 30)

        async def go():
            started = time.monotonic()
            await asyncio.sleep(0.3)
            job = await asyncio.create_subprocess_exec(
                sys.executable, LIB, "tool", "train-model", env=self.env.env,
                stdout=asyncio.subprocess.DEVNULL)
            got = await driver.fire(inj, self.env, self.deliver, started=started, lib=LIB)
            await job.wait()
            return got

        got = asyncio.run(asyncio.wait_for(go(), 60))
        rec = ab.read(self.root / "var/lib/async/ledger.jsonl")
        start = ab.by(rec, "start", "train-model")[0]
        inject = ab.by(rec, "inject")[0]
        self.assertEqual(got["trigger"], {"by": "event", "tool": "train-model", "kind": "start",
                                          "seq": start["seq"], "event_mono": start["mono"],
                                          "delay_s": 0.5})
        self.assertEqual(inject["trigger"], got["trigger"])
        self.assertEqual(inject["message"], "Also count the tickets.")
        # The injection fires the delay after its event, and is delivered after it is recorded.
        self.assertGreaterEqual(inject["mono"], start["mono"] + 0.5)
        self.assertGreaterEqual(self.delivered[0][0], inject["mono"])
        self.assertEqual(self.delivered[0][1], "Also count the tickets.")
        self.assertEqual(ab.verify_chain(rec), [])

    def test_with_no_event_the_injection_fires_at_its_time(self):
        inj = driver.Injection("Cancel it.", "migrate", "start", 5, 0.8)

        async def go():
            started = time.monotonic()
            return started, await driver.fire(inj, self.env, self.deliver, started=started, lib=LIB)

        started, got = asyncio.run(asyncio.wait_for(go(), 60))
        self.assertEqual(got["trigger"], {"by": "time", "at_s": 0.8})
        self.assertGreaterEqual(got["sent_after_s"], 0.8)
        self.assertGreaterEqual(self.delivered[0][0] - started, 0.8)
        self.assertEqual(ab.by(ab.read(self.root / "var/lib/async/ledger.jsonl"), "inject")[0]["trigger"],
                         {"by": "time", "at_s": 0.8})

    def test_a_delivery_that_fails_is_reported_not_raised(self):
        inj = driver.Injection("Cancel it.", "migrate", "start", 0, 0.1)

        async def refuse(message):
            raise RuntimeError("the CLI's input is closed: not measurable")

        got = asyncio.run(driver.fire(inj, self.env, refuse, started=time.monotonic(), lib=LIB))
        self.assertEqual(got["error"], "RuntimeError: the CLI's input is closed: not measurable")


# Harbor 0.23's Claude Code command (claude_code.py's run), as its exec_as_agent gets it.
HARBOR_RUN = (
    'export PATH="$HOME/.local/bin:$PATH"; harbor_claude_code_instruction_0f="$HARBOR_CLAUDE_CODE_INSTRUCTION_0F"; '
    'unset HARBOR_CLAUDE_CODE_INSTRUCTION_0F; printf "%s" "$harbor_claude_code_instruction_0f" | '
    "claude --verbose --output-format=stream-json --permission-mode=bypassPermissions --print 2>&1 | "
    "tee {log}"
)

# A stand-in `claude`: each stream-json line on stdin is a turn, answered
# with a result event; EOF ends it. It keeps what it read.
STANDIN_CLAUDE = """#!/usr/bin/env python3
import json, sys
args = sys.argv[1:]
assert "--input-format=stream-json" in args and "--print" in args, args
seen = []
for line in sys.stdin:
    m = json.loads(line)
    seen.append(m["message"]["content"])
    print(json.dumps({"type": "assistant", "n": len(seen)}), flush=True)
    print(json.dumps({"type": "result", "subtype": "success", "total_cost_usd": 0.01}, separators=(",", ":")), flush=True)
open(sys.argv[0] + ".seen", "w").write(json.dumps(seen))
"""


class ClaudeStdin(unittest.TestCase):
    def test_only_harbors_run_command_is_rewritten(self):
        env = {"HARBOR_CLAUDE_CODE_INSTRUCTION_0F": "Train the model."}
        cmd, new = driver.claude_stdin(HARBOR_RUN.format(log="/logs/agent/claude-code.txt"), env, "/tmp/f")
        self.assertIn("--permission-mode=bypassPermissions --input-format=stream-json --print < /tmp/f", cmd)
        self.assertIn("| tee /logs/agent/claude-code.txt", cmd)
        self.assertEqual(json.loads(new[driver.FIRST]),
                         {"type": "user", "message": {"role": "user", "content": "Train the model."},
                          "parent_tool_use_id": None})
        self.assertIsNone(driver.claude_stdin("mkdir -p $CLAUDE_CONFIG_DIR/debug", env, "/tmp/f"))

    @unittest.skipIf(not os.environ.get("ASYNC_HARBOR"), "set ASYNC_HARBOR=1 under Harbor's python")
    def test_it_matches_the_command_harbor_builds(self):
        os.environ.setdefault("ANTHROPIC_API_KEY", "sk-invented")
        from harbor.agents.installed.claude_code import ClaudeCode
        from harbor.models.agent.context import AgentContext

        class Env(FakeEnv):
            async def exec(self, command, cwd=None, env=None, timeout_sec=None, user=None):
                self.commands.append((command, env))
                return SimpleNamespace(stdout="", stderr="", return_code=0)

            async def upload_file(self, *a, **k):
                pass

        with tempfile.TemporaryDirectory() as d:
            e = Env(Path(d))
            agent = ClaudeCode(logs_dir=Path(d), model_name="anthropic/claude-sonnet-5-5")
            asyncio.run(agent.run("Train the model.", e, AgentContext()))
        rewritten = [driver.claude_stdin(c.removeprefix("set -o pipefail; "), env, "/tmp/f")
                     for c, env in e.commands]
        self.assertEqual(sum(r is not None for r in rewritten), 1)

    def test_the_cli_reads_each_message_from_its_fifo_and_ends_when_closed(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            (d / "bin").mkdir()
            claude = d / "bin/claude"
            claude.write_text(STANDIN_CLAUDE)
            claude.chmod(stat.S_IRWXU)
            log, fifo = d / "claude-code.txt", str(d / "stdin")
            env = {"HARBOR_CLAUDE_CODE_INSTRUCTION_0F": "Train the model.", "HOME": str(d),
                   "PATH": f"{d / 'bin'}:{os.environ['PATH']}"}
            cmd, env = driver.claude_stdin(HARBOR_RUN.format(log=log), env, fifo)
            fake = FakeEnv(d)

            async def go():
                async def state():
                    return driver.log_state((await fake.exec(driver.log_command(str(log)))).stdout)

                run = asyncio.create_task(fake.exec(cmd, env=env, timeout_sec=60))
                while (await state())[0] == 0:
                    await asyncio.sleep(0.05)
                first_result = (await state())[0]
                sent = await fake.exec(driver.send_command(fifo, "Also count the tickets."))
                mark = (await state())[1]
                self.assertGreaterEqual(mark, first_result)
                while (await state())[0] <= first_result:
                    await asyncio.sleep(0.05)
                self.assertEqual(await state(), (4, 4))
                await fake.exec(driver.close_command(fifo))
                done = await run
                late = await fake.exec(driver.send_command(fifo, "Too late."))
                return sent, done, late

            sent, done, late = asyncio.run(asyncio.wait_for(go(), 60))
            self.assertEqual(sent.return_code, 0)
            self.assertEqual(done.return_code, 0, done.stdout + done.stderr)
            self.assertEqual(json.loads(Path(str(claude) + ".seen").read_text()),
                             ["Train the model.", "Also count the tickets."])
            # Once the CLI is gone, a message is refused: the cell is "not measurable".
            self.assertEqual(late.return_code, 3)
            self.assertFalse(os.path.exists(fifo))


# A stand-in `theseus` (and `theseusd`): a daemon whose first turn leaves a
# job running. `ask` records its message; `wait --after` on the busy
# session is the job's completion (its continuation), after STANDIN_JOB_S.
STANDIN_THESEUS = r"""#!/usr/bin/env python3
import json, os, sys, time
d = os.environ["STANDIN_DIR"]
state_path = os.path.join(d, "state.json")
def load():
    try: return json.load(open(state_path))
    except FileNotFoundError: return {"position": 1, "outstanding": 0, "asks": [], "calls": []}
def save(s): json.dump(s, open(state_path, "w"))
name = os.path.basename(sys.argv[0])
args = [a for a in sys.argv[1:] if a != "--json"]
s = load(); s["calls"].append(args); save(s)
if name == "theseusd":
    open(os.path.join(d, "daemon.env"), "w").write(json.dumps({k: os.environ.get(k) for k in ("THESEUS_SOCKET", "THESEUS_CONFIG")}))
    while not os.path.exists(os.path.join(d, "shutdown")): time.sleep(0.05)
    sys.exit(0)
cmd = args[0]
def view(): return {"execution_id": "exe_1", "session_id": "ses_invented", "state": "waiting", "outstanding": s["outstanding"],
                    "queued_results": 0, "waiting_on": {"on": "actions", "correlation_ids": ["cor_1"]} if s["outstanding"] else {"on": "input"},
                    "position": s["position"]}
if cmd == "health": print("{}")
elif cmd == "sessions": print("ses_invented")
elif cmd == "ask":
    s["asks"].append(sys.stdin.read()); s["outstanding"] = 1 if len(s["asks"]) == 1 else s["outstanding"]; s["position"] += 1; save(s)
    print(json.dumps({"stop_reason": "no_tool_calls"}))
elif cmd == "executions": print(json.dumps({"executions": [view()]}))
elif cmd == "wakes": print(json.dumps({"wakes": []}))
elif cmd == "wait":
    if "--after" in args and s["outstanding"]:
        time.sleep(float(os.environ.get("STANDIN_JOB_S", "0.5")))
        s["outstanding"] = 0; s["position"] += 1; s["job_done_at"] = time.monotonic(); save(s)
    print(json.dumps({"reached": "settled", "already": "--after" not in args, "execution": view(), "confirms": []}))
elif cmd == "ledger": print(json.dumps({"rows": [{"kind": "provider.call", "data": {"cost_usd": 0.01}}], "total": 1}))
elif cmd == "shutdown": open(os.path.join(d, "shutdown"), "w").close(); print("{}")
else: print(json.dumps({"cmd": cmd}))
"""


class TheseusTrial(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        d = Path(self.tmp.name)
        self.bin, self.state, self.logs = d / "bin", d / "state", d / "logs"
        for p in (self.bin, self.state, self.logs):
            p.mkdir()
        for name in ("theseus", "theseusd"):
            (self.bin / name).write_text(STANDIN_THESEUS)
            (self.bin / name).chmod(stat.S_IRWXU)
        self.env = FakeEnv(d, {"STANDIN_DIR": str(d), "THESEUS_CONFIG": "/invented/theseus.toml"})
        self.trial = driver.Theseus(str(self.bin), str(self.state), str(self.logs))

    def tearDown(self):
        (Path(self.tmp.name) / "shutdown").touch()
        self.tmp.cleanup()

    def standin(self) -> dict:
        return json.loads((Path(self.tmp.name) / "state.json").read_text())

    def test_the_trial_runs_on_its_own_daemon_and_ends_only_when_its_job_has(self):
        async def go():
            script = tb.daemon_script(str(self.bin), str(self.state), str(self.logs))
            first = await self.env.exec(script, env={"THESEUS_BENCH_INSTRUCTION": "Train the model."})
            session = (await self.env.exec(self.trial.session_command())).stdout.strip()
            asked = await self.env.exec(self.trial.ask_command(session, 2),
                                        env={"ASYNC_MESSAGE": "Also count the tickets."})
            ended = await self.trial.settle(self.env, time.monotonic() + 30)
            settled_at = time.monotonic()
            await self.env.exec(self.trial.finish_script(session))
            return first, session, asked, ended, settled_at

        first, session, asked, ended, settled_at = asyncio.run(asyncio.wait_for(go(), 60))
        self.assertEqual(first.return_code, 0, first.stderr)
        self.assertEqual(session, "ses_invented")
        self.assertEqual(asked.stdout.strip(), "0")
        s = self.standin()
        self.assertEqual(s["asks"], ["Train the model.", "Also count the tickets."])
        # The daemon got the trial's socket and config, and the turn's result is where the adapter reads it.
        daemon = json.loads((Path(self.tmp.name) / "daemon.env").read_text())
        self.assertEqual(daemon, {"THESEUS_SOCKET": f"{self.state}/theseus.sock",
                                  "THESEUS_CONFIG": "/invented/theseus.toml"})
        self.assertEqual(json.loads((self.logs / tb.TURN).read_text())["stop_reason"], "no_tool_calls")
        self.assertEqual((self.logs / tb.EXIT).read_text().strip(), "0")
        # Settled only once the job its first turn left running had completed, by a wait the daemon owns.
        self.assertEqual(ended, "settled")
        self.assertGreaterEqual(settled_at, s["job_done_at"])
        waits = [c for c in s["calls"] if c[:1] == ["wait"]]
        self.assertTrue(any("--after" in c for c in waits), waits)
        # The finish: each session stopped, the records read, the daemon stopped.
        done = [c[0] for c in s["calls"] if c]
        for cmd in ("stop", "history", "ledger", "tasks", "health", "shutdown"):
            self.assertIn(cmd, done)
        self.assertLess(done.index("stop"), done.index("shutdown"))
        self.assertEqual(json.loads((self.logs / "theseus-calls.json").read_text())["total"], 1)
        self.assertFalse(Path(f"{self.state}/theseus.sock").exists())

    def test_a_wake_pending_keeps_the_trial_open_until_it_is_due(self):
        self.assertTrue(driver.busy({"state": "waiting", "waiting_on": {"on": "due_at", "at_ms": 1}}))
        self.assertTrue(driver.busy({"state": "waiting", "waiting_on": {"on": "execution", "execution_id": "x"}}))
        self.assertTrue(driver.busy({"state": "queued"}))
        self.assertFalse(driver.busy({"state": "waiting", "outstanding": 0, "waiting_on": {"on": "input"}}))
        self.assertFalse(driver.busy({"state": "complete", "outstanding": 0}))

    def test_a_trial_that_never_settles_ends_at_its_deadline(self):
        async def go():
            await self.env.exec(tb.daemon_script(str(self.bin), str(self.state), str(self.logs)),
                                env={"THESEUS_BENCH_INSTRUCTION": "Train the model."})
            t0 = time.monotonic()
            return await self.trial.settle(self.env, t0 + 1.5), time.monotonic() - t0

        self.env.env["STANDIN_JOB_S"] = "5"
        ended, took = asyncio.run(asyncio.wait_for(go(), 60))
        self.assertEqual(ended, "timeout")
        self.assertLess(took, 8)


E2E = os.environ.get("ASYNC_E2E_BIN")


@unittest.skipIf(not E2E, "set ASYNC_E2E_BIN to a directory with theseus, theseusd and theseus-sim")
class EndToEnd(unittest.TestCase):
    """The interrupt family on this workspace's daemon, its model a scripted
    stand-in (`theseus-sim fake-model --rules`) making the oracle's
    `proc.run` calls: the long job goes on in the background past
    `proc_sync_secs`, the injection is answered while it runs, and the trial
    settles only once the job's result has come back."""

    def test_the_injection_is_answered_while_the_job_runs(self):
        import socket

        bins = Path(E2E)
        task = HERE / "tasks/interrupt"
        with tempfile.TemporaryDirectory(prefix="ab") as d:
            d = Path(d)
            root, state, logs = d / "root", d / "st", d / "logs"
            for p in (root / "app", state, logs):
                p.mkdir(parents=True)
            tools = task / "environment/async/bin"
            run = ["env", f"ASYNC_ROOT={root}", "ASYNC_TIME_SCALE=0.05",
                   f"PATH={tools}:{os.environ['PATH']}", "sh", "-c"]
            rules = [
                {"when": "Train the model", "calls": [{"name": "proc_run", "input": {
                    "argv": run + ["train-model | sed -n 's/^final score: //p' > \"$ASYNC_ROOT/app/score.txt\""],
                    "timeout_secs": 600}}]},
                {"when": "support tickets", "calls": [{"name": "proc_run", "input": {
                    "argv": run + ["ticket-count | sed -n 's/^open tickets: //p' > \"$ASYNC_ROOT/app/tickets.txt\""]}}]},
            ]
            (d / "rules.json").write_text(json.dumps(rules))
            with socket.socket() as s:
                s.bind(("127.0.0.1", 0))
                port = s.getsockname()[1]
            fake = subprocess.Popen([str(bins / "theseus-sim"), "fake-model", "--addr", f"127.0.0.1:{port}",
                                     "--rules", str(d / "rules.json")], stdout=subprocess.DEVNULL)
            try:
                values = tb.settings("claude-sonnet-5-5", str(root / "app"), proc_sync_secs=2,
                                     api_base=f"http://127.0.0.1:{port}")
                (d / "theseus.toml").write_text(tb.profile(tb.PROFILE.read_text(), values))
                env = FakeEnv(root, {"THESEUS_CONFIG": str(d / "theseus.toml"), "THESEUS_STATE_DIR": str(state),
                                     "ANTHROPIC_API_KEY": "sk-invented", "HOME": str(d)})
                trial = driver.Theseus(str(bins), str(state), str(logs))
                inj = driver.Injection(driver.injection_of(task).message, "train-model", "start", 1, 30)
                got = asyncio.run(asyncio.wait_for(self.trial(env, trial, inj, task, logs), 120))
            finally:
                fake.kill()
                fake.wait()
                if os.environ.get("ASYNC_E2E_KEEP"):
                    import shutil

                    shutil.copytree(d, os.environ["ASYNC_E2E_KEEP"], dirs_exist_ok=True,
                                    ignore=shutil.ignore_patterns("*.sock"))
            first, ended, injected = got
            self.assertEqual(first.return_code, 0, (logs / "theseus.log").read_text()[-3000:])
            self.assertEqual(ended, "settled")
            self.assertEqual(injected["delivered"], {"exit_code": 0})
            self.assertEqual(injected["trigger"]["by"], "event")
            check = subprocess.run(["bash", str(task / "tests/test.sh")], capture_output=True, text=True,
                                   env=dict(os.environ, ASYNC_ROOT=str(root), ASYNC_TIME_SCALE="0.05"))
            self.assertIn("interrupt: reward 1", check.stdout)
            rec = ab.read(root / "var/lib/async/ledger.jsonl")
            tickets = ab.by(rec, "end", "ticket-count")[0]
            train = ab.by(rec, "end", "train-model")[0]
            # The second request was answered while the long job still ran.
            self.assertLess(tickets["mono"], train["mono"])
            calls = json.loads((logs / "theseus-calls.json").read_text())["rows"]
            self.assertGreaterEqual(len(calls), 4)
            health = (logs / "theseus-health.json").read_text()
            self.assertIn("cgroup", health)

    async def trial(self, env, trial, inj, task, logs):
        started = time.monotonic()
        first = asyncio.create_task(env.exec(
            tb.daemon_script(trial.bin_dir, trial.state, trial.logs),
            env={"THESEUS_BENCH_INSTRUCTION": (task / "instruction.md").read_text()}, timeout_sec=100))
        session = (await env.exec(trial.session_command())).stdout.strip()
        try:
            async def deliver(message):
                r = await env.exec(trial.ask_command(session, 2), env={"ASYNC_MESSAGE": message})
                return {"exit_code": int(r.stdout.strip())}

            injected = await driver.fire(inj, env, deliver, started=started, lib=LIB)
            done = await first
            ended = await trial.settle(env, started + 90)
        finally:
            await env.exec(trial.finish_script(session))
        return done, ended, injected


class Spend(unittest.TestCase):
    def test_a_trials_spend_is_every_model_call_in_its_daemons_ledger(self):
        rows = [
            {"kind": "provider.call", "session_id": "ses_conversation",
             "data": {"cost_usd": 0.02, "usage": {"input_tokens": 100, "output_tokens": 40,
                                                  "cache_read_input_tokens": 1000,
                                                  "cache_creation_input_tokens": 500}}},
            # A task's call: its own session, the same trial.
            {"kind": "provider.call", "session_id": "ses_task",
             "data": {"cost_usd": 0.01, "usage": {"input_tokens": 50, "output_tokens": 20}}},
            {"kind": "turn.ended", "data": {"cost_usd": 9.0}},
        ]
        self.assertEqual(driver.spend(rows), {"input_tokens": 1650, "cache_tokens": 1000,
                                              "output_tokens": 60, "cost_usd": 0.03, "calls": 2})


@unittest.skipIf(not os.environ.get("ASYNC_HARBOR"), "set ASYNC_HARBOR=1 under Harbor's python")
class Agents(unittest.TestCase):
    def test_both_arms_load_as_harbor_agents(self):
        import async_agents

        self.assertEqual(async_agents.TheseusAsync.name(), "theseus-async")
        self.assertEqual(async_agents.ClaudeCodeAsync.name(), "claude-code-async")
        self.assertEqual(async_agents.ClaudeCodeAsync.fifo_path(None), "/tmp/async-claude-stdin")


if __name__ == "__main__":
    unittest.main()
