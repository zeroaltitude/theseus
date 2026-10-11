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
import efficiency as ef  # noqa: E402
import sampler as smp  # noqa: E402
import theseus_bench as tb  # noqa: E402

LIB = str(HERE / "tools/asyncbench.py")
SAMPLER = Path(smp.__file__).resolve()


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
# with a result event STANDIN_ANSWER_S after it is read; EOF ends it, and a
# message still being answered then is cut, never answered. It keeps what it
# read, when it answered each, and when its input closed, and its pid.
STANDIN_CLAUDE = """#!/usr/bin/env python3
import json, os, queue, sys, threading, time
args = sys.argv[1:]
assert "--input-format=stream-json" in args and "--print" in args, args
open(sys.argv[0] + ".pid", "w").write(str(os.getpid()))
lines, eof = queue.Queue(), []
def read():
    for line in sys.stdin:
        lines.put(line)
    eof.append(time.monotonic())
    lines.put(None)
threading.Thread(target=read, daemon=True).start()
delay = float(os.environ.get("STANDIN_ANSWER_S", "0"))
seen, answered = [], []
while (line := lines.get()) is not None:
    seen.append(json.loads(line)["message"]["content"])
    time.sleep(delay)
    if eof and delay:
        break  # the input closed while this message was being answered
    print(json.dumps({"type": "assistant", "n": len(seen)}), flush=True)
    # In the real CLI's key order (2.1.288): the result's type is not its first key.
    print(json.dumps({"duration_ms": 1, "type": "result", "subtype": "success", "total_cost_usd": 0.01},
                     separators=(",", ":")), flush=True)
    answered.append(time.monotonic())
while not eof:
    time.sleep(0.01)
open(sys.argv[0] + ".seen", "w").write(json.dumps(seen))
open(sys.argv[0] + ".answers", "w").write(json.dumps({"answered": answered, "eof": eof[0]}))
"""


def standin(script: str) -> str:
    """A stand-in's text with this Python as its interpreter, named directly,
    so its `comm` is its own name, as a real binary's is (through `env` it
    would be `python3`), and the sampler sorts it by that name."""
    return script.replace("#!/usr/bin/env python3", f"#!{sys.executable}", 1)


def fifo_pids(d: Path, fifo: str) -> set[int]:
    """The FIFO run's processes, by pid: the stand-in CLI (its pid file), its
    tee (whose command line names the run's directory), and the FIFO's
    holder (its pid file, until the run's end removes it)."""
    pids = set()
    for f in (d / "bin/claude.pid", Path(fifo + ".holder")):
        try:
            pids.add(int(f.read_text().strip()))
        except (OSError, ValueError):
            pass
    for proc in Path("/proc").iterdir():
        if not proc.name.isdigit() or int(proc.name) == os.getpid():
            continue
        try:
            argv = (proc / "cmdline").read_bytes().split(b"\0")
        except OSError:
            continue
        if argv and Path(argv[0].decode(errors="replace")).name == "tee" and str(d).encode() in b" ".join(argv):
            pids.add(int(proc.name))
    return pids


def stop_pids(pids: set[int]) -> None:
    """Each of `pids` killed, pass or fail: a FIFO test leaves nothing running."""
    for pid in pids:
        try:
            os.kill(pid, 9)
        except OSError:
            pass


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
            claude.write_text(standin(STANDIN_CLAUDE))
            claude.chmod(stat.S_IRWXU)
            log, fifo = d / "claude-code.txt", str(d / "stdin")
            env = {"HARBOR_CLAUDE_CODE_INSTRUCTION_0F": "Train the model.", "HOME": str(d),
                   "PATH": f"{d / 'bin'}:{os.environ['PATH']}"}
            cmd, env = driver.claude_stdin(HARBOR_RUN.format(log=log), env, fifo)
            fake = FakeEnv(d)
            pids: set[int] = set()

            async def go():
                async def state():
                    return driver.log_state((await fake.exec(driver.log_command(str(log)))).stdout)

                run = asyncio.create_task(fake.exec(cmd, env=env, timeout_sec=60))
                while (await state())[0] == 0:
                    await asyncio.sleep(0.05)
                pids.update(fifo_pids(d, fifo))
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

            try:
                sent, done, late = asyncio.run(asyncio.wait_for(go(), 60))
            finally:
                stop_pids(pids | fifo_pids(d, fifo))
            self.assertEqual(sent.return_code, 0)
            self.assertEqual(done.return_code, 0, done.stdout + done.stderr)
            self.assertEqual(json.loads(Path(str(claude) + ".seen").read_text()),
                             ["Train the model.", "Also count the tickets."])
            # Once the CLI is gone, a message is refused: the cell is "not measurable".
            self.assertEqual(late.return_code, 3)
            self.assertFalse(os.path.exists(fifo))


# A stand-in `theseus` (and `theseusd`): a daemon whose first turn leaves a
# job running. The daemon answers `health` only once it is up, after
# STANDIN_START_S, and exits at `shutdown` or once its directory is gone.
# `ask` records its message; `wait --after` on the busy session is the job's
# completion (its continuation), after STANDIN_JOB_S, or its --timeout first; `wakes` lists one wake
# due STANDIN_WAKE_S after the first look (1 s) until it is due. Its state
# is read and written under a lock: the CLI and the daemon run at once.
STANDIN_THESEUS = r"""#!/usr/bin/env python3
import fcntl, json, os, sys, time
d = os.environ["STANDIN_DIR"]
state_path = os.path.join(d, "state.json")
lock = open(os.path.join(d, "state.lock"), "a")
def load():
    try: return json.load(open(state_path))
    except FileNotFoundError: return {"position": 1, "outstanding": 0, "asks": [], "calls": []}
def save(s): json.dump(s, open(state_path + ".new", "w")); os.replace(state_path + ".new", state_path)
def change(f):
    fcntl.flock(lock, fcntl.LOCK_EX)
    try:
        s = load(); f(s); save(s); return s
    finally:
        fcntl.flock(lock, fcntl.LOCK_UN)
name = os.path.basename(sys.argv[0])
args = [a for a in sys.argv[1:] if a != "--json"]
s = change(lambda s: s["calls"].append([name] + args if name == "theseusd" else args))
if name == "theseusd":
    time.sleep(float(os.environ.get("STANDIN_START_S", "0")))
    change(lambda s: s.update(daemon_up_at=time.monotonic()))
    open(os.path.join(d, "daemon.env"), "w").write(json.dumps({k: os.environ.get(k) for k in ("THESEUS_SOCKET", "THESEUS_CONFIG")}))
    while os.path.isdir(d) and not os.path.exists(os.path.join(d, "shutdown")): time.sleep(0.05)
    sys.exit(0)
cmd = args[0]
def view(s): return {"execution_id": "exe_1", "session_id": "ses_invented", "state": "waiting", "outstanding": s["outstanding"],
                     "queued_results": 0, "waiting_on": {"on": "actions", "correlation_ids": ["cor_1"]} if s["outstanding"] else {"on": "input"},
                     "position": s["position"]}
if cmd == "health":
    if not os.path.exists(os.path.join(d, "daemon.env")): sys.exit(1)
    print("{}")
elif cmd == "sessions":
    change(lambda s: s.update(opened_at=time.monotonic()))
    print("ses_invented")
elif cmd == "ask":
    text = sys.stdin.read()
    def ask(s):
        s["asks"].append(text); s["position"] += 1
        if len(s["asks"]) == 1: s["outstanding"] = 1
    change(ask)
    print(json.dumps({"stop_reason": "no_tool_calls"}))
elif cmd == "executions": print(json.dumps({"executions": [view(s)]}))
elif cmd == "wakes":
    s = change(lambda s: s.setdefault("wake_due", time.time() + float(os.environ.get("STANDIN_WAKE_S", "1"))))
    due = s["wake_due"]
    print(json.dumps({"wakes": [{"wake_id": "wak_1", "session_id": "ses_invented", "due_at_ms": int(due * 1000)}] if time.time() < due else []}))
elif cmd == "wait":
    reached = "settled"
    if "--after" in args and s["outstanding"]:
        # As the daemon does: the wait ends at its --timeout, the job still running.
        job, limit = float(os.environ.get("STANDIN_JOB_S", "0.5")), float(args[args.index("--timeout") + 1].rstrip("s"))
        time.sleep(min(job, limit))
        if job <= limit:
            def done(s):
                s["outstanding"] = 0; s["position"] += 1; s["job_done_at"] = time.monotonic()
            s = change(done)
        else:
            reached = "timeout"
    print(json.dumps({"reached": reached, "already": "--after" not in args, "execution": view(s), "confirms": []}))
elif cmd == "ledger":
    kind = args[args.index("-k") + 1] if "-k" in args else None
    rows = [{"kind": "provider.call", "data": {"cost_usd": 0.01}},
            {"kind": "provider.cut", "data": {"estimated": True, "input_tokens": 900, "output_tokens": 30, "cost_usd": 0.004}}]
    print(json.dumps({"rows": [r for r in rows if kind in (None, r["kind"])], "total": 7}))
elif cmd == "tasks": print(json.dumps({"tasks": [{"task_id": "ses_taskinvented", "state": "complete"}], "records": []}))
elif cmd == "history":
    tools = 2 if args[1:2] == ["ses_taskinvented"] else 1
    print(json.dumps({"nodes": [{"kind": "tool_call"}] * tools + [{"kind": "assistant_message"}]}))
elif cmd == "shutdown": open(os.path.join(d, "shutdown"), "w").close(); print("{}")
else: print(json.dumps({"cmd": cmd}))
"""


def left_running(*dirs: Path, wait_s: float = 10.0) -> list[str]:
    """The processes, other than this one, whose command line names one of
    `dirs` and that still run `wait_s` after the first look: a stand-in left
    running. One on its way out gets that long: the sampler touches its
    `done` before its interpreter exits, which a starved nice-19 process can
    take seconds to do."""
    marks = [str(d).encode() for d in dirs]
    deadline = time.monotonic() + wait_s
    while True:
        out = []
        for proc in Path("/proc").iterdir():
            if not proc.name.isdigit() or int(proc.name) == os.getpid():
                continue
            try:
                cmd = (proc / "cmdline").read_bytes()
            except OSError:
                continue
            if any(m in cmd for m in marks):
                out.append(proc.name + ": " + cmd.replace(b"\0", b" ").decode(errors="replace"))
        if not out or time.monotonic() >= deadline:
            return out
        time.sleep(0.1)


def classes_now(*dirs: Path) -> list[tuple[str, str]]:
    """The sampler's classes, now, of the processes whose command line names
    one of `dirs`: (comm, class) each, by the Theseus arm's names."""
    arm = ef.ARMS["theseus"]
    t = smp.Tracker(arm["names"], arm["wrapper_args"])
    procs = smp.read_procs("/proc", lambda st: True)
    classes = t.classify(procs)
    marks = [str(d) for d in dirs]
    return [(p["comm"], classes[pid]) for pid, p in procs.items()
            if any(m in " ".join(p.get("argv") or []) for m in marks)]


def wait_gone(pid: int, secs: float = 10) -> bool:
    """Whether `pid` is gone (or a zombie) within `secs`."""
    deadline = time.monotonic() + secs
    while time.monotonic() < deadline:
        try:
            if Path(f"/proc/{pid}/stat").read_text().rpartition(")")[2].split()[0] == "Z":
                return True
        except (OSError, IndexError):
            return True
        time.sleep(0.02)
    return False


class TheseusTrial(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        d = Path(self.tmp.name)
        self.bin, self.state, self.logs = d / "bin", d / "state", d / "logs"
        for p in (self.bin, self.state, self.logs):
            p.mkdir()
        for name in ("theseus", "theseusd"):
            (self.bin / name).write_text(standin(STANDIN_THESEUS))
            (self.bin / name).chmod(stat.S_IRWXU)
        self.env = FakeEnv(d, {"STANDIN_DIR": str(d), "THESEUS_CONFIG": "/invented/theseus.toml"})
        self.trial = driver.Theseus(str(self.bin), str(self.state), str(self.logs))

    def tearDown(self):
        """A sampler the test did not stop, stopped; the stand-in daemon told
        to stop, and waited for by its pid; then nothing of the test's may
        still run."""
        d = Path(self.tmp.name)
        subprocess.run(["sh", "-c", self.trial.stop_sampler_script()], timeout=30)
        (d / "shutdown").touch()
        try:
            pid = int((self.state / "theseusd.pid").read_text())
        except (OSError, ValueError):
            pid = None
        if pid is not None:
            self.assertTrue(wait_gone(pid), "the stand-in daemon did not stop")
        left = left_running(d)
        self.tmp.cleanup()
        self.assertEqual(left, [])

    def standin(self) -> dict:
        return json.loads((Path(self.tmp.name) / "state.json").read_text())

    def daemon(self) -> str:
        return tb.daemon_script(str(self.bin), str(self.state), str(self.logs), str(SAMPLER), 50)

    def test_the_trial_runs_on_its_own_daemon_and_ends_only_when_its_job_has(self):
        seen: list[tuple[str, str]] = []

        async def look():
            # The sampler's classes while the driver waits: its polls are outside.
            while True:
                seen.extend(classes_now(Path(self.tmp.name)))
                await asyncio.sleep(0.05)

        async def go():
            first = await self.env.exec(self.daemon(), env={"THESEUS_BENCH_INSTRUCTION": "Train the model."})
            session = (await self.env.exec(self.trial.session_command())).stdout.strip()
            asked = await self.env.exec(self.trial.ask_command(session, 2),
                                        env={"ASYNC_MESSAGE": "Also count the tickets."})
            looking = asyncio.create_task(look())
            ended = await self.trial.settle(self.env, time.monotonic() + 30)
            settled_at, settled_wall = time.monotonic(), time.time()
            looking.cancel()
            await self.env.exec(self.trial.finish_script(session, str(SAMPLER)))
            return first, session, asked, ended, settled_at, settled_wall

        first, session, asked, ended, settled_at, settled_wall = asyncio.run(asyncio.wait_for(go(), 60))
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
        # Settled only once the job its first turn left running had completed, by a wait the daemon owns,
        # and its wake was due.
        self.assertEqual(ended, "settled")
        self.assertGreaterEqual(settled_at, s["job_done_at"])
        self.assertGreaterEqual(settled_wall, s["wake_due"])
        waits = [c for c in s["calls"] if c[:1] == ["wait"]]
        self.assertTrue(any("--after" in c for c in waits), waits)
        # The finish: each session stopped, the records read, the daemon stopped.
        done = [c[0] for c in s["calls"] if c]
        for cmd in ("stop", "history", "ledger", "tasks", "health", "shutdown"):
            self.assertIn(cmd, done)
        self.assertLess(done.index("stop"), done.index("shutdown"))
        self.assertEqual(json.loads((self.logs / "theseus-calls.json").read_text())["total"], 7)
        # Each task session's history was read, after the tasks.
        self.assertIn(["history", "ses_taskinvented"], s["calls"])
        self.assertLess(done.index("tasks"), s["calls"].index(["history", "ses_taskinvented"]))
        self.assertFalse(Path(f"{self.state}/theseus.sock").exists())
        # The sampler ran around it all, its daemon harness, the driver's polls outside.
        summary = json.loads((self.logs / smp.SUMMARY).read_text())
        self.assertEqual(summary["status"], "ok")
        self.assertGreaterEqual(summary["classes"]["harness"]["processes"], 1)
        self.assertFalse((self.state / "sampler.pid").exists())
        self.assertIn(("theseusd", "harness"), seen)
        self.assertIn(("async-driver", "outside"), seen)
        self.assertNotIn(("async-driver", "harness"), seen)
        # The record: the ledger's rows, the sampler's numbers.
        rec = ef.theseus_ledger_record(self.logs, wall_s=1.0)
        self.assertEqual((rec["spend_from"], rec["model_calls"], rec["cost_usd"], rec["sampler"]["status"]),
                         ("ledger", 1, 0.014, "ok"))
        self.assertIsNotNone(rec["harness"])
        # The call a stop cut, at its estimate and apart; the task's tool calls with the conversation's;
        # and reads short of the cap.
        self.assertEqual((rec["cut_calls"], rec["cut_cost_usd"], rec["billed_usd"]), (1, 0.004, 0.01))
        self.assertEqual((rec["tool_calls"], rec["tool_calls_from"]), (3, "conversation and tasks"))
        self.assertFalse(rec["truncated"])

    def test_settle_waits_for_a_pending_wake_after_the_job(self):
        async def go():
            await self.env.exec(self.daemon(), env={"THESEUS_BENCH_INSTRUCTION": "Train the model."})
            ended = await self.trial.settle(self.env, time.monotonic() + 30)
            return ended, time.time()

        self.env.env.update(STANDIN_JOB_S="0", STANDIN_WAKE_S="1.5")
        try:
            ended, settled_wall = asyncio.run(asyncio.wait_for(go(), 60))
        finally:
            subprocess.run(["bash", "-c", self.trial.finish_script("ses_invented", str(SAMPLER))],
                           env=self.env.env, timeout=60)
        self.assertEqual(ended, "settled")
        # The job ended at once; the wake was due 1.5 s after settle's first look.
        self.assertGreaterEqual(settled_wall, self.standin()["wake_due"])

    def test_the_daemon_is_asked_for_nothing_before_it_answers_health(self):
        async def go():
            return await self.env.exec(self.daemon(), env={"THESEUS_BENCH_INSTRUCTION": "Train the model."})

        self.env.env["STANDIN_START_S"] = "0.5"
        try:
            first = asyncio.run(asyncio.wait_for(go(), 60))
        finally:
            subprocess.run(["bash", "-c", self.trial.finish_script("ses_invented", str(SAMPLER))],
                           env=self.env.env, timeout=60)
        self.assertEqual(first.return_code, 0, first.stderr)
        s = self.standin()
        self.assertGreaterEqual(s["opened_at"], s["daemon_up_at"])
        self.assertEqual(s["asks"], ["Train the model."])

    def test_a_wake_pending_keeps_the_trial_open_until_it_is_due(self):
        self.assertTrue(driver.busy({"state": "waiting", "waiting_on": {"on": "due_at", "at_ms": 1}}))
        self.assertTrue(driver.busy({"state": "waiting", "waiting_on": {"on": "execution", "execution_id": "x"}}))
        self.assertTrue(driver.busy({"state": "queued"}))
        self.assertFalse(driver.busy({"state": "waiting", "outstanding": 0, "waiting_on": {"on": "input"}}))
        self.assertFalse(driver.busy({"state": "complete", "outstanding": 0}))

    def test_a_trial_that_never_settles_ends_at_its_deadline(self):
        async def go():
            await self.env.exec(self.daemon(), env={"THESEUS_BENCH_INSTRUCTION": "Train the model."})
            t0 = time.monotonic()
            return await self.trial.settle(self.env, t0 + 1.5), time.monotonic() - t0

        # No finish: tearDown stops the daemon (and the sampler) itself.
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
            run = ["env", f"ASYNC_ROOT={root}", "ASYNC_TIME_SCALE=0.2",
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
                                   env=dict(os.environ, ASYNC_ROOT=str(root), ASYNC_TIME_SCALE="0.2"))
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
            # Measured: the sampler ran from before the daemon to after its stop, saw theseusd as the
            # harness and the driver's polls outside it, and the record is the ledger's.
            summary = json.loads((logs / smp.SUMMARY).read_text())
            self.assertEqual(summary["status"], "ok", summary.get("reason"))
            self.assertIn(("theseusd", "harness"), self.seen)
            self.assertIn(("async-driver", "outside"), self.seen)
            self.assertNotIn(("async-driver", "harness"), self.seen)
            rec = ef.theseus_ledger_record(logs, wall_s=1.0)
            self.assertEqual((rec["spend_from"], rec["model_calls"], rec["sampler"]["status"]),
                             ("ledger", len(calls), "ok"))
            # No task here: the rows are the conversation's answers, each one (the ledger's `total`
            # is every row of every kind, not the read's).
            answers = [n for n in json.loads((logs / tb.HISTORY).read_text())["nodes"]
                       if n["kind"] == "assistant_message"]
            self.assertEqual(len(calls), len(answers))
            # Nothing was cut, the reads were short of the cap, and the tasks (none) read: every tool call
            # is the conversation's, each `proc_run` the rules made.
            self.assertEqual((rec["cut_calls"], rec["truncated"], rec["tool_calls_from"]),
                             (0, False, "conversation and tasks"))
            self.assertEqual(json.loads((logs / "theseus-cuts.json").read_text())["rows"], [])
            self.assertEqual(rec["tool_calls"], 2)
            self.assertGreater(rec["harness"]["cpu_s"], 0)
            self.assertGreater(rec["harness"]["peak_rss_kb"], 0)
            self.assertEqual(left_running(d), [])

    async def trial(self, env, trial, inj, task, logs):
        started = time.monotonic()
        self.seen: list[tuple[str, str]] = []

        async def look():
            # The sampler's classes as the trial runs, by the arm's names.
            while True:
                self.seen.extend(classes_now(Path(trial.state), Path(trial.bin_dir)))
                await asyncio.sleep(0.1)

        looking = asyncio.create_task(look())
        first = asyncio.create_task(env.exec(
            tb.daemon_script(trial.bin_dir, trial.state, trial.logs, str(SAMPLER), 100),
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
            looking.cancel()
            await env.exec(trial.finish_script(session, str(SAMPLER)))
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
            # A call a stop cut, at its estimate; a failed one, with no usage or cost.
            {"kind": "provider.cut", "session_id": "ses_conversation",
             "data": {"estimated": True, "input_tokens": 300, "output_tokens": 12, "cost_usd": 0.002}},
            {"kind": "provider.error", "session_id": "ses_conversation", "data": {"error": "overloaded"}},
        ]
        self.assertEqual(driver.spend(rows), {"input_tokens": 1950, "cache_tokens": 1000, "output_tokens": 72,
                                              "cost_usd": 0.032, "calls": 2, "cut_calls": 1})


# A task of the interrupt family's shape whose injection fires by time, 5 s in:
# after the measured arm's sampler start, which waits up to 3 s for its first
# sample before Harbor's run command starts the CLI and its FIFO.
TASK_TOML = """[metadata.async]
family = "interrupt"

[metadata.async.injection]
message = "Also count the tickets."
after_tool = "train-model"
after_kind = "start"
delay_s = 0
at_s = 5.0
"""


@unittest.skipIf(not os.environ.get("ASYNC_HARBOR"), "set ASYNC_HARBOR=1 under Harbor's python")
class ClaudeCodeAsyncRun(unittest.TestCase):
    """`ClaudeCodeAsync.run` itself, on a fake environment: Harbor's own run
    issues its command (`HARBOR_RUN`), the arm rewrites it onto the FIFO,
    the sampler runs around it, and the stand-in `claude` answers each
    message STANDIN_ANSWER_S after reading it."""

    def test_both_messages_are_answered_before_the_input_closes(self):
        from unittest import mock

        os.environ.setdefault("ANTHROPIC_API_KEY", "sk-invented")
        import async_agents
        import claude_code_agent as cca
        from harbor.agents.installed.claude_code import ClaudeCode
        from harbor.models.agent.context import AgentContext

        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            for sub in ("bin", "logs", "task/environment", "root", "measure"):
                (d / sub).mkdir(parents=True)
            (d / "task/task.toml").write_text(TASK_TOML)
            claude = d / "bin/claude"
            claude.write_text(standin(STANDIN_CLAUDE))
            claude.chmod(stat.S_IRWXU)
            fifo = str(d / "stdin")

            class Env(FakeEnv):
                environment_dir = d / "task/environment"
                default_user = None

                async def exec(self, command, cwd=None, env=None, timeout_sec=None, user=None):
                    # The tools' library where this checkout keeps it.
                    return await super().exec(command.replace(driver.LIB, LIB), cwd, env, timeout_sec)

            class Arm(async_agents.ClaudeCodeAsync):
                def fifo_path(self):
                    return fifo

            async def harbor_run(self, instruction, environment, context):
                await self.exec_as_agent(environment, command=HARBOR_RUN.format(
                    log=f"{self.environment_logs_dir}/claude-code.txt"),
                    env={"HARBOR_CLAUDE_CODE_INSTRUCTION_0F": instruction})

            env = Env(d / "root", {"PATH": f"{d / 'bin'}:{os.environ['PATH']}", "HOME": str(d),
                                   "STANDIN_ANSWER_S": "0.5"})
            agent = Arm(logs_dir=d / "logs", model_name="anthropic/claude-sonnet-5-5",
                        environment_logs_dir=d / "logs")
            try:
                with mock.patch.object(ClaudeCode, "run", harbor_run), \
                        mock.patch.object(cca, "SAMPLER", str(SAMPLER)), \
                        mock.patch.object(cca, "STATE", str(d / "measure")):
                    asyncio.run(asyncio.wait_for(agent.run("Train the model.", env, AgentContext()), 60))
                agent.populate_context_post_run(AgentContext())
            finally:
                stop_pids(fifo_pids(d, fifo))
                subprocess.run(["sh", "-c", smp.stop_script(str(d / "logs"), str(d / "measure"))], timeout=30)
            self.assertEqual(json.loads(Path(str(claude) + ".seen").read_text()),
                             ["Train the model.", "Also count the tickets."])
            answers = json.loads(Path(str(claude) + ".answers").read_text())
            # Each message's result came before the driver closed the input.
            self.assertEqual(len(answers["answered"]), 2, answers)
            self.assertTrue(all(a < answers["eof"] for a in answers["answered"]), answers)
            report = json.loads((d / "logs" / driver.REPORT).read_text())
            self.assertEqual(report["ended"], "settled")
            self.assertEqual(report["injection"]["delivered"]["sent"], True)
            # Measured: the sampler ran around the CLI, which it saw as the harness.
            summary = json.loads((d / "logs" / smp.SUMMARY).read_text())
            self.assertEqual(summary["status"], "ok")
            self.assertGreaterEqual(summary["classes"]["harness"]["processes"], 1)
            rec = json.loads((d / "logs" / ef.RECORD).read_text())
            self.assertEqual((rec["result_events"], rec["sampler"]["status"]), (2, "ok"))
            self.assertEqual(left_running(d), [])


@unittest.skipIf(not os.environ.get("ASYNC_HARBOR"), "set ASYNC_HARBOR=1 under Harbor's python")
class Agents(unittest.TestCase):
    def test_both_arms_load_as_harbor_agents(self):
        import async_agents

        self.assertEqual(async_agents.TheseusAsync.name(), "theseus-async")
        self.assertEqual(async_agents.ClaudeCodeAsync.name(), "claude-code-async")
        self.assertEqual(async_agents.ClaudeCodeAsync.fifo_path(None), "/tmp/async-claude-stdin")
        # Measured as bench/harbor measures Claude Code; Theseus by its ledger.
        import claude_code_agent as cca

        self.assertTrue(issubclass(async_agents.ClaudeCodeAsync, cca.MeasuredClaudeCode))

    def test_the_claude_arm_records_the_effort_it_was_asked_for_not_the_default(self):
        import async_agents
        from harbor.models.agent.context import AgentContext

        for asked, want in ((None, "medium"), ("high", "high")):
            with tempfile.TemporaryDirectory() as d:
                kw = {} if asked is None else {"reasoning_effort": asked}
                agent = async_agents.ClaudeCodeAsync(logs_dir=Path(d), model_name="anthropic/claude-sonnet-5-5", **kw)
                agent.populate_context_post_run(AgentContext())
                rec = json.loads((Path(d) / ef.RECORD).read_text())
                self.assertEqual(rec["effort"], want)

    def test_theseus_records_its_ledger_not_its_first_turn(self):
        import async_agents
        from harbor.models.agent.context import AgentContext

        rows = [{"kind": "provider.call", "data": {"model": "claude-sonnet-5-5", "cost_usd": c,
                                                   "usage": {"input_tokens": 10, "output_tokens": 5}}}
                for c in (0.01, 0.0068, 0.009, 0.0075, 0.009)]
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            # The first ask's turn: two calls, $0.0168; the ledger: five, $0.0423.
            (d / tb.TURN).write_text(json.dumps({"cost_usd": 0.0168, "loops": 2, "model": "claude-sonnet-5-5",
                                                 "usage": {"input_tokens": 20, "output_tokens": 10}}))
            (d / tb.EXIT).write_text("0\n")
            (d / "theseus-calls.json").write_text(json.dumps({"rows": rows, "total": len(rows)}))
            (d / "theseus-cuts.json").write_text(json.dumps({"rows": [{"kind": "provider.cut", "data": {
                "model": "claude-sonnet-5-5", "estimated": True, "input_tokens": 40, "output_tokens": 2,
                "cost_usd": 0.0007}}], "total": len(rows)}))
            (d / driver.REPORT).write_text(json.dumps({"wall_s": 42.0}))
            agent = async_agents.TheseusAsync(logs_dir=d, model_name="anthropic/claude-sonnet-5-5")
            context = AgentContext()
            agent.populate_context_post_run(context)
            rec = json.loads((d / ef.RECORD).read_text())
            self.assertEqual((rec["spend_from"], rec["model_calls"], rec["cost_usd"], rec["cut_calls"]),
                             ("ledger", 5, 0.043, 1))
            self.assertEqual((rec["wall_s"], rec["wall_from"]), (42.0, "agent"))
            self.assertEqual(context.metadata["efficiency"], rec)
            self.assertEqual((context.cost_usd, context.metadata["cut_calls"]), (0.043, 1))


if __name__ == "__main__":
    unittest.main()


# Harbor 0.23's Pi command (pi.py's run), as its exec_as_agent gets it.
HARBOR_PI_RUN = (
    ". ~/.nvm/nvm.sh; pi --print --mode json --session-dir {sessions} --provider anthropic "
    "--model claude-sonnet-5-5 {instruction} 2>&1 </dev/null | grep -v '\"type\":\"message_update\"' | "
    "stdbuf -oL tee {log}"
)

# A stand-in `pi --mode rpc`: each JSONL command on stdin is a prompt,
# accepted at once and answered STANDIN_ANSWER_S after it is read (an answer
# with its usage, in its session log too), then `agent_settled`; EOF ends it,
# and a prompt still being answered then is cut, never answered. It keeps
# what it read, when it settled each, and when its input closed, and its pid.
STANDIN_PI = r"""#!/usr/bin/env python3
import json, os, queue, sys, threading, time
from pathlib import Path
args = sys.argv[1:]
assert args[:2] == ["--mode", "rpc"] and "--print" not in args, args
sessions = Path(args[args.index("--session-dir") + 1])
sessions.mkdir(parents=True, exist_ok=True)
log = sessions / "2026-10-06T12-00-00-000Z_standin.jsonl"
open(sys.argv[0] + ".pid", "w").write(str(os.getpid()))
lines, eof = queue.Queue(), []
def read():
    for line in sys.stdin:
        lines.put(line)
    eof.append(time.monotonic())
    lines.put(None)
threading.Thread(target=read, daemon=True).start()
delay = float(os.environ.get("STANDIN_ANSWER_S", "0"))
seen, kinds, answered = [], [], []
out = lambda x: print(json.dumps(x, separators=(",", ":")), flush=True)
with log.open("a") as f:
    f.write(json.dumps({"type": "session", "version": 3, "id": "standin"}) + "\n")
while (line := lines.get()) is not None:
    cmd = json.loads(line)
    seen.append(cmd["message"])
    kinds.append(cmd.get("streamingBehavior"))
    out({"type": "response", "command": "prompt", "success": True, "data": {"disposition": "started"}})
    out({"type": "agent_start"})
    time.sleep(delay)
    if eof and delay:
        break  # the input closed while this prompt was being answered
    msg = {"role": "assistant", "content": [{"type": "text", "text": "ok"}], "model": "claude-sonnet-5-5",
           "stopReason": "stop", "usage": {"input": 10, "output": 5, "cacheRead": 0, "cacheWrite": 0,
                                           "cost": {"total": 0.01}}}
    with log.open("a") as f:
        f.write(json.dumps({"type": "message", "id": f"a{len(seen)}", "message": msg}) + "\n")
    out({"type": "message_end", "message": msg})
    out({"type": "agent_settled"})
    answered.append(time.monotonic())
while not eof:
    time.sleep(0.01)
open(sys.argv[0] + ".seen", "w").write(json.dumps({"seen": seen, "kinds": kinds}))
open(sys.argv[0] + ".answers", "w").write(json.dumps({"answered": answered, "eof": eof[0]}))
"""


def pi_pids(d: Path, fifo: str) -> set[int]:
    """The Pi FIFO run's processes: the stand-in (its pid file), the holder,
    and its tee, as `fifo_pids` finds Claude Code's."""
    pids = fifo_pids(d, fifo)
    try:
        pids.add(int((d / "bin/pi.pid").read_text().strip()))
    except (OSError, ValueError):
        pass
    return pids


class PiStdin(unittest.TestCase):
    def test_only_harbors_run_command_is_rewritten(self):
        quoted = "'Train the model, don'\"'\"'t wait.'"
        cmd, new = driver.pi_stdin(HARBOR_PI_RUN.format(sessions="/logs/agent/pi/sessions", instruction=quoted,
                                                        log="/logs/agent/pi.txt"),
                                   {"ANTHROPIC_API_KEY": "sk-invented"}, "/tmp/f")
        self.assertIn("pi --mode rpc --session-dir /logs/agent/pi/sessions --provider anthropic "
                      "--model claude-sonnet-5-5 < /tmp/f 2>&1 | stdbuf -oL grep -v", cmd)
        self.assertIn("| stdbuf -oL tee /logs/agent/pi.txt &", cmd)
        self.assertNotIn("don", cmd, "the instruction goes through the FIFO, never the command line")
        self.assertEqual(json.loads(new[driver.PI_FIRST]),
                         {"type": "prompt", "message": "Train the model, don't wait."})
        self.assertEqual(new["ANTHROPIC_API_KEY"], "sk-invented")
        self.assertEqual(json.loads(driver.rpc_prompt("Also.", steer=True)),
                         {"type": "prompt", "message": "Also.", "streamingBehavior": "steer"})
        self.assertIsNone(driver.pi_stdin("mkdir -p /tmp/harbor-pi-agent", {}, "/tmp/f"))

    @unittest.skipIf(not os.environ.get("ASYNC_HARBOR"), "set ASYNC_HARBOR=1 under Harbor's python")
    def test_it_matches_the_command_harbor_builds(self):
        os.environ.setdefault("ANTHROPIC_API_KEY", "sk-invented")
        os.environ.pop("ANTHROPIC_BASE_URL", None)
        from harbor.agents.installed.pi import Pi
        from harbor.models.agent.context import AgentContext

        class Env(FakeEnv):
            async def exec(self, command, cwd=None, env=None, timeout_sec=None, user=None):
                self.commands.append((command, env))
                return SimpleNamespace(stdout="", stderr="", return_code=0)

        for text in ("Train the model.", "Train it, don't wait: $HOME `x`"):
            with tempfile.TemporaryDirectory() as d:
                e = Env(Path(d))
                agent = Pi(logs_dir=Path(d), model_name="anthropic/claude-sonnet-5-5", thinking="low")
                asyncio.run(agent.run(text, e, AgentContext()))
            rewritten = [driver.pi_stdin(c.removeprefix("set -o pipefail; "), env, "/tmp/f")
                         for c, env in e.commands]
            self.assertEqual(sum(r is not None for r in rewritten), 1)
            new = next(r for r in rewritten if r is not None)
            self.assertEqual(json.loads(new[1][driver.PI_FIRST])["message"], text)
            self.assertIn("--thinking low < /tmp/f", new[0])

    def test_pi_reads_each_message_from_its_fifo_and_ends_when_closed(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            (d / "bin").mkdir()
            pi = d / "bin/pi"
            pi.write_text(standin(STANDIN_PI))
            pi.chmod(stat.S_IRWXU)
            log, fifo = d / "pi.txt", str(d / "stdin")
            env = {"HOME": str(d), "PATH": f"{d / 'bin'}:{os.environ['PATH']}"}
            cmd, env = driver.pi_stdin(HARBOR_PI_RUN.format(sessions=d / "sessions", instruction="'Train the model.'",
                                                            log=log).replace(". ~/.nvm/nvm.sh; ", ""), env, fifo)
            fake = FakeEnv(d)
            pids: set[int] = set()

            async def go():
                async def state():
                    return driver.log_state((await fake.exec(driver.pi_log_command(str(log)))).stdout)

                run = asyncio.create_task(fake.exec(cmd, env=env, timeout_sec=60))
                while (await state())[0] == 0:
                    await asyncio.sleep(0.05)
                pids.update(pi_pids(d, fifo))
                first = (await state())[0]
                sent = await fake.exec(driver.pi_send_command(fifo, "Also count the tickets."))
                while (await state())[0] <= first:
                    await asyncio.sleep(0.05)
                self.assertEqual(await state(), (8, 8))
                await fake.exec(driver.close_command(fifo))
                done = await run
                late = await fake.exec(driver.pi_send_command(fifo, "Too late."))
                return sent, done, late

            try:
                sent, done, late = asyncio.run(asyncio.wait_for(go(), 60))
            finally:
                stop_pids(pids | pi_pids(d, fifo))
            self.assertEqual(sent.return_code, 0)
            self.assertEqual(done.return_code, 0, done.stdout + done.stderr)
            self.assertEqual(json.loads(Path(str(pi) + ".seen").read_text()),
                             {"seen": ["Train the model.", "Also count the tickets."], "kinds": [None, "steer"]})
            # Once Pi is gone, a message is refused: the cell is "not measurable".
            self.assertEqual(late.return_code, 3)
            self.assertFalse(os.path.exists(fifo))


@unittest.skipIf(not os.environ.get("ASYNC_HARBOR"), "set ASYNC_HARBOR=1 under Harbor's python")
class PiAsyncRun(unittest.TestCase):
    """`PiAsync.run` itself, on a fake environment: Harbor's own run issues
    its command (`HARBOR_PI_RUN`), the arm rewrites it onto the FIFO, the
    sampler runs around it, and the stand-in `pi` answers each prompt
    STANDIN_ANSWER_S after reading it."""

    def test_both_messages_are_answered_before_the_input_closes(self):
        from unittest import mock

        import async_agents
        import pi_agent as pa
        from harbor.agents.installed.pi import Pi
        from harbor.models.agent.context import AgentContext

        self.assertTrue(issubclass(async_agents.PiAsync, pa.MeasuredPi))
        self.assertEqual((async_agents.PiAsync.name(), async_agents.PiAsync.fifo_path(None)),
                         ("pi-async", "/tmp/async-pi-stdin"))
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            for sub in ("bin", "logs", "task/environment", "root", "measure"):
                (d / sub).mkdir(parents=True)
            (d / "task/task.toml").write_text(TASK_TOML)
            pi = d / "bin/pi"
            pi.write_text(standin(STANDIN_PI))
            pi.chmod(stat.S_IRWXU)
            fifo = str(d / "stdin")

            class Env(FakeEnv):
                environment_dir = d / "task/environment"
                default_user = None

                async def exec(self, command, cwd=None, env=None, timeout_sec=None, user=None):
                    # The tools' library where this checkout keeps it.
                    return await super().exec(command.replace(driver.LIB, LIB), cwd, env, timeout_sec)

            class Arm(async_agents.PiAsync):
                def fifo_path(self):
                    return fifo

            async def harbor_run(self, instruction, environment, context):
                await self.exec_as_agent(environment, command=HARBOR_PI_RUN.format(
                    sessions=f"{self.environment_logs_dir}/pi/sessions", instruction=f"'{instruction}'",
                    log=f"{self.environment_logs_dir}/pi.txt").replace(". ~/.nvm/nvm.sh; ", ""))

            env = Env(d / "root", {"PATH": f"{d / 'bin'}:{os.environ['PATH']}", "HOME": str(d),
                                   "STANDIN_ANSWER_S": "0.5"})
            agent = Arm(logs_dir=d / "logs", model_name="anthropic/claude-sonnet-5-5",
                        environment_logs_dir=d / "logs")
            try:
                with mock.patch.object(Pi, "run", harbor_run), \
                        mock.patch.object(pa, "SAMPLER", str(SAMPLER)), \
                        mock.patch.object(pa, "STATE", str(d / "measure")):
                    asyncio.run(asyncio.wait_for(agent.run("Train the model.", env, AgentContext()), 60))
                ctx = AgentContext()
                agent.populate_context_post_run(ctx)
            finally:
                stop_pids(pi_pids(d, fifo))
                subprocess.run(["sh", "-c", smp.stop_script(str(d / "logs"), str(d / "measure"))], timeout=30)
            self.assertEqual(json.loads(Path(str(pi) + ".seen").read_text())["seen"],
                             ["Train the model.", "Also count the tickets."])
            answers = json.loads(Path(str(pi) + ".answers").read_text())
            # Each prompt settled before the driver closed the input.
            self.assertEqual(len(answers["answered"]), 2, answers)
            self.assertTrue(all(a < answers["eof"] for a in answers["answered"]), answers)
            report = json.loads((d / "logs" / driver.REPORT).read_text())
            self.assertEqual(report["ended"], "settled")
            self.assertEqual(report["injection"]["delivered"]["sent"], True)
            # Measured: the sampler ran around Pi, which it saw as the harness.
            summary = json.loads((d / "logs" / smp.SUMMARY).read_text())
            self.assertEqual(summary["status"], "ok")
            self.assertGreaterEqual(summary["classes"]["harness"]["processes"], 1)
            rec = json.loads((d / "logs" / ef.RECORD).read_text())
            self.assertEqual((rec["arm"], rec["settled_runs"], rec["model_calls"], rec["cost_usd"]),
                             ("pi", 2, 2, 0.02))
            self.assertEqual(ctx.metadata["efficiency"], rec)
            self.assertEqual(left_running(d), [])


@unittest.skipIf(not os.environ.get("ASYNC_HARBOR"), "set ASYNC_HARBOR=1 under Harbor's python")
class AsyncTimeoutStopsTheAgent(unittest.TestCase):
    """Harbor's timeout on the async arms (theseus-sgpx): they run the
    measured arm through `super().run`, so its stop of the agent, then the
    sampler's, comes before the driver's own end. A recording environment, so
    nothing here signals a process on this host."""

    class Env:
        default_user = None

        def __init__(self, task: Path):
            self.environment_dir = task / "environment"
            self.calls: list[tuple[str, str | None]] = []
            self.envs: list[tuple[str, dict | None]] = []

        async def exec(self, command, cwd=None, env=None, timeout_sec=None, user=None):
            self.calls.append((command, user))
            self.envs.append((command, env))
            return SimpleNamespace(stdout="", stderr="", return_code=0)

        async def upload_file(self, source_path, target_path):
            pass

    def check(self, arm, harbor_run_of, names, state):
        import measure
        from unittest import mock

        from harbor.models.agent.context import AgentContext

        async def harbors(self_, instruction, environment, context):
            await asyncio.sleep(3600)  # until Harbor's timeout cancels it

        with tempfile.TemporaryDirectory() as d:
            task = Path(d) / "task"
            (task / "environment").mkdir(parents=True)
            (task / "task.toml").write_text('[metadata.async]\nfamily = "interrupt"\n')
            env = self.Env(task)
            logs = Path(d) / "logs"
            logs.mkdir()
            agent = arm(logs_dir=logs, model_name="anthropic/claude-sonnet-5-5", environment_logs_dir=logs)
            with mock.patch.object(harbor_run_of, "run", harbors), \
                    self.assertRaises(asyncio.TimeoutError):
                asyncio.run(asyncio.wait_for(agent.run("Train the model.", env, AgentContext()), 1.0))
            commands = [c for c, _ in env.calls]
            stop = measure.stop_agent_script(names, baseline=f"{state}/pids.before")
            sampler_stop = smp.stop_script(str(logs), state)
            self.assertEqual(commands.count(stop), 1, commands)
            self.assertEqual(commands.count(sampler_stop), 1, commands)
            self.assertLess(commands.index(stop), commands.index(sampler_stop))
            self.assertEqual(dict(env.calls)[stop], "root")
            self.assertIn("sampler.py", commands[0])  # the sampler started first

    def test_pi_async_runs_pi_offline_in_rpc_mode_too(self):
        """The async arm rewrites Harbor's run command onto a FIFO in RPC
        mode before the measured arm adds Pi's environment (theseus-a5we): the
        offline lines must reach that command, and no other."""
        from unittest import mock

        import async_agents
        from harbor.agents.installed.pi import Pi
        from harbor.models.agent.context import AgentContext

        async def harbors(agent_, instruction, environment, context):
            await agent_.exec_as_agent(environment, command=HARBOR_PI_RUN.format(
                sessions="/logs/agent/pi/sessions", instruction="'Train the model.'", log="/logs/agent/pi.txt"),
                env={"ANTHROPIC_API_KEY": "sk-invented"})

        with tempfile.TemporaryDirectory() as d:
            task = Path(d) / "task"
            (task / "environment").mkdir(parents=True)
            (task / "task.toml").write_text('[metadata.async]\nfamily = "interrupt"\n')
            env = self.Env(task)
            logs = Path(d) / "logs"
            logs.mkdir()
            agent = async_agents.PiAsync(logs_dir=logs, model_name="anthropic/claude-sonnet-5-5",
                                         environment_logs_dir=logs)
            with mock.patch.object(Pi, "run", harbors), \
                    mock.patch.object(async_agents.PiAsync, "fifo_path", lambda self: "/tmp/x-fifo"):
                try:
                    asyncio.run(asyncio.wait_for(agent.run("Train the model.", env, AgentContext()), 3.0))
                except asyncio.TimeoutError:
                    pass
        runs = [e for c, e in env.envs if "pi --mode rpc" in c]
        self.assertEqual(len(runs), 1, [c for c, _ in env.envs])
        self.assertEqual({k: runs[0][k] for k in ("PI_OFFLINE", "PI_SKIP_VERSION_CHECK", "PI_TELEMETRY")},
                         {"PI_OFFLINE": "1", "PI_SKIP_VERSION_CHECK": "1", "PI_TELEMETRY": "0"})
        self.assertEqual(runs[0]["ANTHROPIC_API_KEY"], "sk-invented")

    def test_harbors_timeout_still_stops_the_sampler_of_claude_code_async(self):
        os.environ.setdefault("ANTHROPIC_API_KEY", "sk-invented")
        import async_agents
        import claude_code_agent
        from harbor.agents.installed.claude_code import ClaudeCode

        self.check(async_agents.ClaudeCodeAsync, ClaudeCode, ("claude",), claude_code_agent.STATE)

    def test_harbors_timeout_still_stops_the_sampler_of_pi_async(self):
        import async_agents
        import pi_agent
        from harbor.agents.installed.pi import Pi

        self.check(async_agents.PiAsync, Pi, ("pi",), pi_agent.STATE)
