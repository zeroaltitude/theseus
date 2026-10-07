"""OpenClaw as Harbor runs it, measured as the other arms are (theseus-qags).

    harbor run -a openclaw_agent:MeasuredOpenClaw \\
        -m anthropic/claude-sonnet-5-5 --ak max_budget_usd=2.0 --ak max_turns=200 …

with this directory on PYTHONPATH. Harbor's own `OpenClaw` (`-a openclaw`:
`openclaw agent --local --json`, a released build installed from npm into the
task's container, nothing of OpenClaw changed) with `measured.MeasuredArm`:
OpenClaw pinned at `PINNED_VERSION`, its thinking level `medium`, its own CLI
timeout lifted (`TIMEOUT_S`) so the task's agent timeout bounds it as it
bounds every arm, the sampler around its run, and the record from Harbor's
trajectory of its session (`efficiency.openclaw_record`).

OpenClaw has no spend cap and no turn cap: the caps are recorded, not
enforced.
"""

from __future__ import annotations

from harbor.agents.installed.openclaw import OpenClaw

import efficiency as ef
from measured import MeasuredArm

# openclaw's latest release on npm when this arm was built (2026-10-07).
PINNED_VERSION = "2026.9.8"
# Longer than any Terminal-Bench 2.0 task's agent timeout (12000 s at most).
TIMEOUT_S = 14400


class MeasuredOpenClaw(MeasuredArm, OpenClaw):
    ARM = "openclaw"
    PINNED_VERSION = PINNED_VERSION
    EFFORT_OPTION = "thinking"

    def __init__(self, *args, timeout=None, **kwargs):
        super().__init__(*args, timeout=timeout or TIMEOUT_S, **kwargs)

    def record(self):
        return ef.openclaw_record(self.logs_dir)
