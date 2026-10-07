"""OpenHands as Harbor runs it, measured as the other arms are (theseus-qags).

    harbor run -a openhands_agent:MeasuredOpenHands \\
        -m anthropic/claude-sonnet-5-5 --ak max_budget_usd=2.0 --ak max_turns=200 …

with this directory on PYTHONPATH (and `LLM_API_KEY` in Harbor's environment:
the SDK runner reads that name, not the provider's). Harbor's own `OpenHandsSDK`
(`-a openhands-sdk`: the OpenHands Software Agent SDK, `openhands-sdk` and
`openhands-tools` in their own venv, Harbor's `run_agent.py` running an
`Agent` in a `Conversation` in the task's container) with
`measured.MeasuredArm`.

**Why the SDK, not Harbor's `openhands`.** Harbor's `-a openhands` runs
`python -m openhands.core.main`, OpenHands' first-generation CLI. `openhands-ai`
1.x, its current line, ships the app server only, with no `openhands.core`, so
that agent cannot run a current OpenHands; the SDK is what OpenHands now
builds its agent on.

It adds:

- **the version**: `openhands-sdk` and `openhands-tools` pinned at
  `PINNED_VERSION`;
- **the effort**: `reasoning_effort` medium. The SDK 1.53 sends Sonnet 5.5
  `output_config: {effort: medium}` with adaptive thinking (its default would
  be high), as a recorder standing in for the API showed (2026-10-07);
- **the caps**: `max_turns` as its `max_iterations` (the SDK's
  `max_iteration_per_run`), enforced. The SDK has no spend cap (its
  `Metrics.max_budget_per_task` is a field nothing enforces), so
  `max_budget_usd` is recorded and flagged, not enforced;
- **its process's name**: the runner runs as `python`, which is also what
  much of a task's work runs as, so the arm links the venv's interpreter as
  `openhands-py` and runs through the link, so its `/proc/<pid>/comm` names it;
- **its calls**: the runner through `openhands_measure_run.py`, which writes
  each LLM's per-call metrics (`openhands-metrics.json`), since Harbor's
  trajectory keeps only totals without the cache write.
"""

from __future__ import annotations

from pathlib import Path

from harbor.agents.installed.openhands_sdk import OpenHandsSDK

import efficiency as ef
from measured import DIR, MeasuredArm

# openhands-sdk's (and openhands-tools') latest release on PyPI when this arm
# was built (2026-10-07).
PINNED_VERSION = "1.53.0"
VENV = "/opt/openhands-sdk-venv/bin"
WRAPPER = f"{DIR}/openhands_measure_run.py"
PYTHON = f"{VENV}/python /installed-agent/run_agent.py"
NAMED = f"{VENV}/openhands-py {WRAPPER}"


class MeasuredOpenHands(MeasuredArm, OpenHandsSDK):
    ARM = "openhands"
    PINNED_VERSION = PINNED_VERSION
    EFFORT_OPTION = "reasoning_effort"

    def enforced_caps(self, max_budget_usd, max_turns):
        return {} if max_turns is None else {"max_iterations": max_turns}

    def caps_enforced(self):
        # The turn cap only: the budget is recorded (above).
        return False

    async def after_install(self, environment):
        await self.exec_as_root(environment, command=f"mkdir -p {DIR} && ln -sf {VENV}/python {VENV}/openhands-py")
        await environment.upload_file(Path(__file__).with_name("openhands_measure_run.py"), WRAPPER)
        await self.exec_as_root(environment, command=f"chmod 755 {DIR} && chmod 644 {WRAPPER}")

    def rewrite(self, command, env):
        return command.replace(PYTHON, NAMED), env

    def record(self):
        return ef.openhands_record(self.logs_dir)
