"""Claude Code as Harbor runs it, measured as Theseus is (theseus-7gir.12).

    harbor run -a claude_code_agent:MeasuredClaudeCode \\
        -m anthropic/claude-sonnet-5-5 --ak max_budget_usd=2.0 --ak max_turns=200 …

with this directory on PYTHONPATH. A thin subclass of Harbor's own
`ClaudeCode` (`-a claude-code`): its install, its command line, its options
(`max_budget_usd`, `max_turns`, …), its trajectory, and its `name()` are
Harbor's, so the results read as Claude Code's. It adds only:

- **install**: the harness sampler (`sampler.py`) uploaded beside it;
- **run**: the sampler started before Harbor's run (its CLI, `claude` as
  `/proc/<pid>/comm` shows it, is the harness; what it runs is work) and
  stopped after it, in a `finally`, so Harbor's timeout stops it too;
- **populate_context_post_run**: after Harbor's own, the trial's efficiency
  record (`agent/efficiency.json`, and `metadata["efficiency"]`;
  `efficiency.py`), from the stream-json `result` event, the session log,
  and the trajectory.

The sampler's interval is BENCH_SAMPLE_MS (250 by default), from the
environment of `harbor run`.
"""

from __future__ import annotations

import os
import shlex
from pathlib import Path
from typing import override

from harbor.agents.installed.claude_code import ClaudeCode
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

import efficiency as ef
import sampler as smp

DIR = "/installed-agent/measure"
SAMPLER = f"{DIR}/sampler.py"
STATE = f"{DIR}/state"
ARM = ef.ARMS["claude-code"]


class MeasuredClaudeCode(ClaudeCode):
    @override
    async def install(self, environment: BaseEnvironment) -> None:
        await super().install(environment)
        await self.exec_as_root(environment, command=f"mkdir -p {STATE}")
        await environment.upload_file(Path(smp.__file__), SAMPLER)
        owner = ""
        if environment.default_user is not None:
            owner = f" && chown {shlex.quote(str(environment.default_user))} {STATE}"
        await self.exec_as_root(environment, command=f"chmod 755 {DIR} {SAMPLER}{owner}")

    @override
    async def run(
        self, instruction: str, environment: BaseEnvironment, context: AgentContext
    ) -> None:
        # Harbor's own run renders the prompt template; this one must not.
        logs = self.environment_logs_dir.as_posix()
        sample_ms = int(os.environ.get("BENCH_SAMPLE_MS", str(smp.INTERVAL_MS)))
        start = smp.start_script(SAMPLER, logs, STATE, ARM["names"], ARM["wrapper_args"], sample_ms)
        # As the agent's user, as Harbor runs the CLI; never the trial's failure.
        await environment.exec(command=f"{start}; true", timeout_sec=30)
        try:
            await super().run(instruction, environment, context)
        finally:
            await environment.exec(command=smp.stop_script(logs, STATE), timeout_sec=30)

    @override
    def populate_context_post_run(self, context: AgentContext) -> None:
        super().populate_context_post_run(context)
        try:
            rec = ef.claude_code_record(self.logs_dir)
            ef.write(self.logs_dir, rec)
        except Exception as e:  # noqa: BLE001: a record never fails the trial
            rec = {"schema": ef.SCHEMA, "arm": "claude-code", "error": f"{type(e).__name__}: {e}"}
        context.metadata = ef.metadata(context.metadata, rec)
