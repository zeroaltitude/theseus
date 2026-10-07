"""OpenHands as Harbor runs it, measured as the other arms are (theseus-qags).

    harbor run -a openhands_agent:MeasuredOpenHands \\
        -m anthropic/claude-sonnet-5-5 --ak max_budget_usd=2.0 --ak max_turns=200 …

with this directory on PYTHONPATH. Harbor's own `OpenHands` (`-a openhands`:
`openhands-ai` in its own venv, `python -m openhands.core.main --task=…` with
the local runtime) with `measured.MeasuredArm`: `openhands-ai` pinned at
`PINNED_VERSION`, `LLM_REASONING_EFFORT` medium, the sampler around its run,
and the record from Harbor's trajectory of its events
(`efficiency.openhands_record`). It adds:

- **the caps, enforced**: OpenHands has both, so `max_budget_usd` becomes its
  `MAX_BUDGET_PER_TASK` (it stops once the dollars it counts pass it, as
  Claude Code's `--max-budget-usd` does) and `max_turns` its
  `MAX_ITERATIONS` (an iteration is one model call);
- **its process's name**: OpenHands runs as `python`, which is also what
  much of a task's work runs as, so the sampler could not tell the harness
  from its work. The arm links the venv's interpreter as `openhands-py` and
  runs OpenHands through the link, so its `/proc/<pid>/comm` names it (and
  the action server it starts from `sys.executable`).
"""

from __future__ import annotations

from harbor.agents.installed.openhands import OpenHands

import efficiency as ef
from measured import MeasuredArm

# openhands-ai's latest release on PyPI when this arm was built (2026-10-07).
PINNED_VERSION = "1.11.0"
VENV = "/opt/openhands-venv/bin"
PYTHON = f"{VENV}/python -m openhands.core.main"
NAMED = f"{VENV}/openhands-py -m openhands.core.main"


class MeasuredOpenHands(MeasuredArm, OpenHands):
    ARM = "openhands"
    PINNED_VERSION = PINNED_VERSION
    EFFORT_OPTION = "reasoning_effort"

    def enforced_caps(self, max_budget_usd, max_turns):
        caps = {}
        if max_budget_usd is not None:
            caps["max_budget_per_task"] = str(max_budget_usd)
        if max_turns is not None:
            caps["max_iterations"] = max_turns
        return caps

    async def after_install(self, environment):
        await self.exec_as_root(environment, command=f"ln -sf {VENV}/python {VENV}/openhands-py")

    def rewrite(self, command, env):
        return command.replace(PYTHON, NAMED), env

    def record(self):
        return ef.openhands_record(self.logs_dir)
