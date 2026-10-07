"""Claude Code as Harbor runs it, measured as Theseus is (theseus-7gir.12).

    harbor run -a claude_code_agent:MeasuredClaudeCode \\
        -m anthropic/claude-sonnet-5-5 --ak max_budget_usd=2.0 --ak max_turns=200 …

with this directory on PYTHONPATH. A thin subclass of Harbor's own
`ClaudeCode` (`-a claude-code`): its install, its command line, its options
(`max_budget_usd`, `max_turns`, …), its trajectory, and its `name()` are
Harbor's, so the results read as Claude Code's. It adds only:

- **the version**: pinned at `PINNED_VERSION` unless `--ak version=…` names
  another, so every trial installs the same Claude Code (Harbor's installs
  the latest release, whatever npm serves that day; theseus-7gir.23);
- **the effort**: `--ak reasoning_effort=…` defaults to `measure.EFFORT`
  (medium), as the other arms' are, where Harbor's leaves it to a host's
  `CLAUDE_CODE_EFFORT_LEVEL` (theseus-n6p5);
- **install**: the harness sampler (`sampler.py`) uploaded beside it, and
  the version the container reads written to the logs (`version.txt`);
- **run**: the sampler started before Harbor's run (its CLI, `claude` as
  `/proc/<pid>/comm` shows it, is the harness; what it runs is work) and
  stopped after it, in a `finally`, so Harbor's timeout stops it too. At
  that timeout the agent is stopped first (`measure.stop_agent`): Harbor's
  Docker environment ends only its exec client, and the CLI would run on
  while the verifier does (theseus-sgpx);
- **populate_context_post_run**: after Harbor's own, the trial's efficiency
  record (`agent/efficiency.json`, and `metadata["efficiency"]`;
  `efficiency.py`), from the stream-json `result` event, the session log,
  and the trajectory, with the effort asked for and the version read.

The sampler's interval is BENCH_SAMPLE_MS (250 by default), from the
environment of `harbor run`.
"""

from __future__ import annotations

import asyncio
import os
import shlex
from pathlib import Path
from typing import override

from harbor.agents.installed.claude_code import ClaudeCode
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

import efficiency as ef
import measure
import sampler as smp

DIR = "/installed-agent/measure"
SAMPLER = f"{DIR}/sampler.py"
STATE = f"{DIR}/state"
ARM = ef.ARMS["claude-code"]
# The Claude Code every trial installs (npm's `@anthropic-ai/claude-code`, or
# the native installer's release): the version whose requests were captured
# (2026-10-06). The maintainer moves it, here, with a run's report.
PINNED_VERSION = "2.1.290"


class MeasuredClaudeCode(ClaudeCode):
    def __init__(self, *args, version: str | None = None, reasoning_effort: str | None = None, **kwargs):
        super().__init__(*args, version=version or PINNED_VERSION,
                         reasoning_effort=reasoning_effort or measure.EFFORT, **kwargs)

    @override
    async def install(self, environment: BaseEnvironment) -> None:
        await super().install(environment)
        await measure.record_version(environment, self.get_version_command(),
                                     self.environment_logs_dir.as_posix())
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
        except asyncio.CancelledError:
            # Harbor's timeout: stop the CLI and what it started before the
            # sampler's stop, and before the verifier starts.
            await measure.stop_agent(environment, ARM["names"])
            raise
        finally:
            await environment.exec(command=smp.stop_script(logs, STATE), timeout_sec=30)

    @override
    def populate_context_post_run(self, context: AgentContext) -> None:
        super().populate_context_post_run(context)
        try:
            rec = ef.stamp(self.logs_dir, ef.claude_code_record(self.logs_dir),
                           self.options.reasoning_effort, self.version(), self.parse_version)
            ef.write(self.logs_dir, rec)
        except Exception as e:  # noqa: BLE001: a record never fails the trial
            rec = {"schema": ef.SCHEMA, "arm": "claude-code", "error": f"{type(e).__name__}: {e}"}
        context.metadata = ef.metadata(context.metadata, rec)
