"""The async bench's Harbor agents (theseus-7gir.16): each arm by its own
async means, the driver's second message delivered as its CLI allows.

    harbor run -p bench/async/tasks -a async_agents:TheseusAsync -m anthropic/claude-sonnet-5-5
    harbor run -p bench/async/tasks -a async_agents:ClaudeCodeAsync -m anthropic/claude-sonnet-5-5
    harbor run -p bench/async/tasks -a async_agents:PiAsync -m anthropic/claude-sonnet-5-5

with bench/harbor and bench/async on PYTHONPATH (bench/async/README.md).

- **TheseusAsync**: the adapter's agent on a daemon of its own for the trial
  (`theseus_bench.daemon_script`), at the product's `proc_sync_secs` (60 s,
  THESEUS_ASYNC_PROC_SYNC), so a long command goes on as a background job
  whose result comes back as a continuation. The instruction is the first
  `ask -s`, the injection a second (queued behind a turn that runs). The
  trial ends when nothing is busy and no wake is pending
  (`driver.Theseus.settle`), or at the task's timeout; then every session
  is stopped, the daemon's records read, and the daemon stopped cleanly.
  Its spend is every `provider.call` row in the daemon's ledger, tasks'
  sessions included, and every `provider.cut` row (a call a stop cut, at
  its estimate). The harness sampler runs from before the daemon to
  after its clean stop, and the efficiency record is the ledger's
  (`efficiency.theseus_ledger_record`), not the first ask's turn alone.
- **ClaudeCodeAsync**: Claude Code measured as bench/harbor measures it
  (`claude_code_agent.MeasuredClaudeCode`: Harbor's own, with the harness
  sampler around its run and the efficiency record), its command reading
  stdin from a FIFO in the CLI's stream-json input mode
  (`driver.claude_stdin`), where the driver writes the injection; its input
  is closed once every message is sent and the CLI has answered each. Its
  record counts the stream's results (`efficiency.claude_code_async_record`).
- **PiAsync**: Pi measured as bench/harbor measures it
  (`pi_agent.MeasuredPi`), in its RPC mode with its commands on a FIFO
  (`driver.pi_stdin`); the injection is a steering prompt, and its input is
  closed once Pi has settled after every message. Its record is Pi's,
  from its session log, with the runs the stream settled.

Each leaves `agent/async-driver.json`: the family, the trigger that fired,
when the message went, and how the trial ended.
"""

from __future__ import annotations

import asyncio
import json
import os
import time
import tomllib
from pathlib import Path
from typing import Any, override

from harbor.agents.installed.base import NonZeroAgentExitCodeError, with_prompt_template
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

import claude_code_agent as cca
import driver
import efficiency as ef
import pi_agent as pa
import sampler as smp
import theseus_agent as ta
import theseus_bench as tb


def _task_dir(environment: BaseEnvironment) -> Path:
    return Path(environment.environment_dir).parent


def _write(logs_dir: Path, report: dict[str, Any]) -> None:
    try:
        (logs_dir / driver.REPORT).write_text(json.dumps(report, indent=2))
    except OSError:
        pass


class TheseusAsync(ta.Theseus):
    @staticmethod
    @override
    def name() -> str:
        return "theseus-async"

    @override
    @with_prompt_template
    async def run(
        self, instruction: str, environment: BaseEnvironment, context: AgentContext
    ) -> None:
        started = time.monotonic()
        provider, _, model = (self.model_name or "anthropic/claude-sonnet-5-5").rpartition("/")
        if provider not in ("", "anthropic"):
            raise ValueError(f"the bench profile runs Anthropic models, not {self.model_name}")
        workdir = (await environment.exec(command="pwd")).stdout.strip() or "/app"
        access = self.model_connection
        values = tb.settings(
            model,
            workdir,
            max_loops=int(os.environ.get("THESEUS_BENCH_MAX_LOOPS", "200")),
            spend_limit_usd=float(os.environ.get("THESEUS_BENCH_SPEND_LIMIT", "2.0")),
            proc_sync_secs=int(os.environ.get("THESEUS_ASYNC_PROC_SYNC", "60")),
            api_base=access.configured_base_url,
        )
        await self._upload_config_text(
            environment, content=tb.profile(tb.PROFILE.read_text(), values),
            remote_path=ta.CONFIG, filename="theseus.toml",
        )
        env = {
            "ANTHROPIC_API_KEY": access.api_key or "",
            "THESEUS_CONFIG": ta.CONFIG,
            "THESEUS_STATE_DIR": ta.STATE,
            "THESEUS_LOG": "info",
        }
        logs = self.environment_logs_dir.as_posix()
        trial = driver.Theseus(ta.BIN, ta.STATE, logs)
        meta = driver.task_of(_task_dir(environment))
        inj = driver.injection_of(_task_dir(environment))
        timeout = float(self._task_timeout(environment))
        report: dict[str, Any] = {"arm": self.name(), "family": meta.get("family"),
                                  "proc_sync_secs": values[("tools", "proc_sync_secs")]}
        sample_ms = int(os.environ.get("BENCH_SAMPLE_MS", str(smp.INTERVAL_MS)))
        first = asyncio.create_task(self.exec_as_agent(
            environment, command=tb.daemon_script(ta.BIN, ta.STATE, logs, ta.SAMPLER, sample_ms),
            env={**env, "THESEUS_BENCH_INSTRUCTION": instruction},
        ))
        session = injected = None
        try:
            session = (await environment.exec(command=trial.session_command())).stdout.strip() or None
            sent = 0

            async def deliver(message: str) -> Any:
                nonlocal sent
                sent += 1
                r = await environment.exec(command=trial.ask_command(session, sent + 1),
                                           env={**env, "ASYNC_MESSAGE": message})
                return {"exit_code": int((r.stdout or "").strip() or -1)}

            if inj and session:
                injected = asyncio.create_task(driver.fire(inj, environment, deliver, started=started))
            failure: BaseException | None = None
            try:
                await first
            except NonZeroAgentExitCodeError as e:
                failure = e
            if injected:
                report["injection"] = await injected
            report["ended"] = (await trial.settle(environment, started + timeout - 30)
                               if session else "no session")
            if failure:
                code = await self._exit_code(environment, logs)
                error = ta.ERRORS.get(code) if code is not None else None
                if error is None:
                    raise failure
                raise error(f"theseus ask: {tb.ended(code)} (exit {code})\n{failure}") from failure
        except asyncio.CancelledError:
            report["ended"] = "timeout"
            raise
        finally:
            first.cancel()
            if injected:
                injected.cancel()
            # The finish stops the sampler after the daemon; with no
            # session there is no finish, and the sampler stops alone.
            end = trial.finish_script(session, ta.SAMPLER) if session else trial.stop_sampler_script()
            await asyncio.shield(environment.exec(command=end, timeout_sec=60))
            report["wall_s"] = round(time.monotonic() - started, 3)
            _write(self.logs_dir, report)

    def _task_timeout(self, environment: BaseEnvironment) -> float:
        """The task's agent timeout, by which the trial must have ended."""
        try:
            cfg = tomllib.loads((_task_dir(environment) / "task.toml").read_text())
            return float(cfg.get("agent", {}).get("timeout_sec") or 900)
        except (OSError, ValueError):
            return 900.0

    @override
    def populate_context_post_run(self, context: AgentContext) -> None:
        super().populate_context_post_run(context)
        rows = [r for f in ("theseus-calls.json", "theseus-cuts.json")
                for r in (ta._json(self.logs_dir / f) or {}).get("rows", [])]
        if rows:
            s = driver.spend(rows)
            context.n_input_tokens = s["input_tokens"]
            context.n_cache_tokens = s["cache_tokens"]
            context.n_output_tokens = s["output_tokens"]
            context.cost_usd = s["cost_usd"]
            context.metadata = {**(context.metadata or {}), "spend_from": "ledger",
                                "provider_calls": s["calls"], "cut_calls": s["cut_calls"]}
        # The efficiency record from the ledger and the sampler, in place of
        # the inherited one (the first ask's turn alone). Never the trial's
        # failure.
        if (self.logs_dir / "theseus-calls.json").exists():
            report = ta._json(self.logs_dir / driver.REPORT) or {}
            try:
                rec = ef.stamp(self.logs_dir,
                               ef.theseus_ledger_record(self.logs_dir, wall_s=report.get("wall_s")),
                               tb.profile_effort(), None, self.parse_version)
                ef.write(self.logs_dir, rec)
            except Exception as e:  # noqa: BLE001
                rec = {"schema": ef.SCHEMA, "arm": "theseus", "error": f"{type(e).__name__}: {e}"}
            context.metadata = ef.metadata(context.metadata, rec)


class ClaudeCodeAsync(cca.MeasuredClaudeCode):
    """Claude Code, measured, with its input on a FIFO, in stream-json. The
    sampler starts through `environment.exec`, not `exec_as_agent`, so only
    Harbor's run command is rewritten."""

    @staticmethod
    @override
    def name() -> str:
        return "claude-code-async"

    def fifo_path(self) -> str:
        """Where the CLI's stdin is: one FIFO in the container."""
        return "/tmp/async-claude-stdin"

    @override
    async def exec_as_agent(self, environment: BaseEnvironment, command: str,
                            env: dict[str, str] | None = None, cwd: str | None = None,
                            timeout_sec: int | None = None) -> Any:
        rewritten = driver.claude_stdin(command, env, self.fifo_path())
        if rewritten is not None:
            command, env = rewritten
        return await super().exec_as_agent(environment, command, env=env, cwd=cwd,
                                           timeout_sec=timeout_sec)

    @override
    async def run(self, instruction: str, environment: BaseEnvironment, context: AgentContext) -> None:
        started = time.monotonic()
        fifo = self.fifo_path()
        log = (self.environment_logs_dir / "claude-code.txt").as_posix()
        inj = driver.injection_of(_task_dir(environment))
        report: dict[str, Any] = {"arm": self.name(),
                                  "family": driver.task_of(_task_dir(environment)).get("family")}
        run = asyncio.create_task(super().run(instruction, environment, context))
        # The stream's line count when the last message was sent: the
        # instruction's is 0.
        mark = 0

        async def deliver(message: str) -> Any:
            nonlocal mark
            r = await environment.exec(command=driver.send_command(fifo, message))
            if r.return_code != 0:
                raise RuntimeError("the CLI's input is closed: not measurable")
            mark = driver.log_state((await environment.exec(command=driver.log_command(log))).stdout)[1]
            return {"sent": True, "after_line": mark}

        async def close_when_answered() -> None:
            if inj:
                report["injection"] = await driver.fire(inj, environment, deliver, started=started)
            # The stream is read every 2 s: the CLI says nothing to the driver.
            while not run.done():
                last, _ = driver.log_state((await environment.exec(command=driver.log_command(log))).stdout)
                if last > mark:
                    break
                await asyncio.sleep(2)
            await environment.exec(command=driver.close_command(fifo))

        closer = asyncio.create_task(close_when_answered())
        try:
            await run
            report["ended"] = "settled"
        except asyncio.CancelledError:
            report["ended"] = "timeout"
            raise
        finally:
            closer.cancel()
            await asyncio.shield(environment.exec(command=driver.close_command(fifo)))
            report["wall_s"] = round(time.monotonic() - started, 3)
            _write(self.logs_dir, report)

    @override
    def populate_context_post_run(self, context: AgentContext) -> None:
        super().populate_context_post_run(context)
        # The measured record, with the results the stream held. Never the
        # trial's failure.
        try:
            rec = ef.stamp(self.logs_dir, ef.claude_code_async_record(self.logs_dir),
                           self.options.reasoning_effort, self.version(), self.parse_version)
            ef.write(self.logs_dir, rec)
        except Exception as e:  # noqa: BLE001
            rec = {"schema": ef.SCHEMA, "arm": "claude-code", "error": f"{type(e).__name__}: {e}"}
        context.metadata = ef.metadata(context.metadata, rec)


class PiAsync(pa.MeasuredPi):
    """Pi, measured as bench/harbor measures it (`pi_agent.MeasuredPi`), in
    its RPC mode with its commands on a FIFO (`driver.pi_stdin`): the
    instruction is the first `prompt`, the driver's message a steering
    `prompt` (taken before Pi's next model call while it runs, a run of its
    own while it is idle), and its input is closed once Pi has settled
    after the last message. The sampler starts through `environment.exec`,
    not `exec_as_agent`, so only Harbor's run command is rewritten."""

    @staticmethod
    @override
    def name() -> str:
        return "pi-async"

    def fifo_path(self) -> str:
        """Where Pi's stdin is: one FIFO in the container."""
        return "/tmp/async-pi-stdin"

    @override
    async def exec_as_agent(self, environment: BaseEnvironment, command: str,
                            env: dict[str, str] | None = None, cwd: str | None = None,
                            timeout_sec: int | None = None) -> Any:
        rewritten = driver.pi_stdin(command, env, self.fifo_path())
        if rewritten is not None:
            command, env = rewritten
        return await super().exec_as_agent(environment, command, env=env, cwd=cwd,
                                           timeout_sec=timeout_sec)

    @override
    async def run(self, instruction: str, environment: BaseEnvironment, context: AgentContext) -> None:
        started = time.monotonic()
        fifo = self.fifo_path()
        log = (self.environment_logs_dir / "pi.txt").as_posix()
        inj = driver.injection_of(_task_dir(environment))
        report: dict[str, Any] = {"arm": self.name(),
                                  "family": driver.task_of(_task_dir(environment)).get("family")}
        run = asyncio.create_task(super().run(instruction, environment, context))
        # The stream's line count when the last message was sent: the
        # instruction's is 0.
        mark = 0

        async def deliver(message: str) -> Any:
            nonlocal mark
            r = await environment.exec(command=driver.pi_send_command(fifo, message))
            if r.return_code != 0:
                raise RuntimeError("Pi's input is closed: not measurable")
            mark = driver.log_state((await environment.exec(command=driver.pi_log_command(log))).stdout)[1]
            return {"sent": True, "after_line": mark}

        async def close_when_answered() -> None:
            if inj:
                report["injection"] = await driver.fire(inj, environment, deliver, started=started)
            # The stream is read every 2 s: Pi says nothing to the driver.
            while not run.done():
                last, _ = driver.log_state((await environment.exec(command=driver.pi_log_command(log))).stdout)
                if last > mark:
                    break
                await asyncio.sleep(2)
            await environment.exec(command=driver.close_command(fifo))

        closer = asyncio.create_task(close_when_answered())
        try:
            await run
            report["ended"] = "settled"
        except asyncio.CancelledError:
            report["ended"] = "timeout"
            raise
        finally:
            closer.cancel()
            await asyncio.shield(environment.exec(command=driver.close_command(fifo)))
            report["wall_s"] = round(time.monotonic() - started, 3)
            _write(self.logs_dir, report)

    @override
    def populate_context_post_run(self, context: AgentContext) -> None:
        super().populate_context_post_run(context)
        # The measured record (its spend from Pi's session log, every run's),
        # with the runs its stream settled. Never the trial's failure.
        rec = context.metadata.get("efficiency") if context.metadata else None
        if isinstance(rec, dict) and "error" not in rec:
            try:
                stream = (self.logs_dir / "pi.txt").read_text(encoding="utf-8", errors="replace")
            except OSError:
                stream = ""
            rec["settled_runs"] = sum(1 for e in ef.read_jsonl_text(stream) if e.get("type") == "agent_settled")
            ef.write(self.logs_dir, rec)
            context.metadata = ef.metadata(context.metadata, rec)
