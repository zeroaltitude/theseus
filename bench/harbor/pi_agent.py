"""Pi, the minimal coding agent, as Harbor runs it, measured as Theseus is
(theseus-jp9p).

    harbor run -a pi_agent:MeasuredPi \\
        -m anthropic/claude-sonnet-5-5 --ak max_budget_usd=2.0 --ak max_turns=200 …

with this directory on PYTHONPATH. A thin subclass of Harbor's own `Pi`
(`-a pi`): its install (Node 22 through nvm, then `npm install -g
--ignore-scripts @earendil-works/pi-coding-agent@<version>`), its command line
(`pi --print --mode json --session-dir /logs/agent/pi/sessions --provider
anthropic --model <id> <instruction>`, the key from Harbor's model
connection), its options (`thinking`), and its `name()` are Harbor's, so the
results read as Pi's. It adds only:

- **the version**: pinned at `PINNED_VERSION` unless `--ak version=…` names
  another, so every trial installs the same Pi (Harbor's own installs
  `@latest`);
- **the caps the other arms run under**, `max_budget_usd` and `max_turns`,
  taken as Claude Code's are and recorded, not enforced: Pi has neither a
  spend cap nor a turn cap, so a trial that passes them is flagged in its
  record (`limits`), never stopped;
- **install**: the harness sampler (`sampler.py`) uploaded beside it;
- **run**: the sampler started before Harbor's run (Pi's CLI, `pi` as
  `/proc/<pid>/comm` shows it, since it sets its process title, is the
  harness; what it runs is work) and stopped after it, in a `finally`, so
  Harbor's timeout stops it too;
- **populate_context_post_run**: after Harbor's own, the trial's ATIF
  trajectory (`agent/trajectory.json`, `pi_atif.py`, from Pi's session log),
  Harbor's three counters from the same log with the cache write in its
  input as the other arms count it, and the efficiency record
  (`agent/efficiency.json`, and `metadata["efficiency"]`; `efficiency.py`).

The sampler's interval is BENCH_SAMPLE_MS (250 by default), from the
environment of `harbor run`.
"""

from __future__ import annotations

import json
import os
import shlex
from pathlib import Path
from typing import override

from pydantic import Field

from harbor.agents.capabilities import AgentCapabilities
from harbor.agents.installed.pi import Pi, PiOptions
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

import efficiency as ef
import pi_atif
import sampler as smp

# The Pi every trial installs: @earendil-works/pi-coding-agent, its latest
# release when this arm was built (2026-10-05). It needs Node 22.19 or later;
# Harbor's install takes nvm's latest 22.
PINNED_VERSION = "1.0.4"
DIR = "/installed-agent/measure"
SAMPLER = f"{DIR}/sampler.py"
STATE = f"{DIR}/state"
ARM = ef.ARMS["pi"]


class MeasuredPiOptions(PiOptions):
    # Not flags: Pi has no such limits. The record holds the trial to them.
    max_budget_usd: float | None = Field(
        default=None, description="The other arms' spend cap, in dollars: recorded and flagged, not enforced.")
    max_turns: int | None = Field(
        default=None, description="The other arms' turn cap: recorded and flagged, not enforced.")


class MeasuredPi(Pi):
    capabilities = AgentCapabilities(**{**Pi.capabilities.model_dump(), "atif": True})
    options_model = MeasuredPiOptions
    options: MeasuredPiOptions

    def __init__(self, *args, version: str | None = None, **kwargs):
        super().__init__(*args, version=version or PINNED_VERSION, **kwargs)

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
            entries = ef.read_jsonl(ef.pi_session_files(self.logs_dir))
            if entries:
                traj = pi_atif.trajectory(entries, version=self.version() or "unknown",
                                          model_name=self.model_name)
                (self.logs_dir / "trajectory.json").write_text(json.dumps(traj, indent=2))
        except Exception:  # noqa: BLE001: a trajectory never fails the trial
            pass
        try:
            rec = ef.pi_record(self.logs_dir, self.options.max_budget_usd, self.options.max_turns)
            ef.write(self.logs_dir, rec)
            t = rec.get("tokens") or {}
            if rec.get("spend_from") in ("session_log", "stream"):
                # Harbor's Pi leaves the cache write out of its input; the
                # other arms' counters hold it, so these do too.
                context.n_input_tokens = t["input"] + t["cache_read"] + t["cache_write"]
                context.n_cache_tokens = t["cache_read"]
                context.n_output_tokens = t["output"]
                context.cost_usd = rec.get("cost_usd")
        except Exception as e:  # noqa: BLE001: a record never fails the trial
            rec = {"schema": ef.SCHEMA, "arm": "pi", "error": f"{type(e).__name__}: {e}"}
        context.metadata = ef.metadata(context.metadata, rec)
