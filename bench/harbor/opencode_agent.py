"""OpenCode as Harbor runs it, measured as the other arms are (theseus-qags).

    harbor run -a opencode_agent:MeasuredOpenCode \\
        -m anthropic/claude-sonnet-5-5 --ak max_budget_usd=2.0 --ak max_turns=200 …

with this directory on PYTHONPATH. Harbor's own `OpenCode` (`-a opencode`)
with `measured.MeasuredArm`: OpenCode pinned at `PINNED_VERSION`, its model
variant `medium` (OpenCode 1.18.35's catalog gives Claude Sonnet 5.5 the
variants low to max, each adaptive thinking at that effort), the sampler
around its run, and the record from its own stream (`efficiency.opencode_record`:
each `step_finish` a call, with its tokens and the dollars OpenCode priced).

**Its catalog is pinned too.** OpenCode refreshes its model catalog (prices,
variants) from models.dev unless `OPENCODE_DISABLE_MODELS_FETCH` is set, so
the arm sets it, and `OPENCODE_DISABLE_AUTOUPDATE`: the catalog is the one
the pinned release ships. OpenCode has no spend cap and no turn cap: the caps
are recorded, not enforced.
"""

from __future__ import annotations

import re

from harbor.agents.installed.opencode import OpenCode

import efficiency as ef
from measured import MeasuredArm

# opencode-ai's latest release when this arm was built (2026-10-07).
PINNED_VERSION = "1.18.35"
RUN = re.compile(r"(?:^|[\s;])opencode --model=\S+ run\b")
RUN_ENV = {"OPENCODE_DISABLE_MODELS_FETCH": "1", "OPENCODE_DISABLE_AUTOUPDATE": "1"}


class MeasuredOpenCode(MeasuredArm, OpenCode):
    ARM = "opencode"
    PINNED_VERSION = PINNED_VERSION
    EFFORT_OPTION = "variant"

    def rewrite(self, command, env):
        if RUN.search(command):
            env = {**RUN_ENV, **(env or {})}
        return command, env

    def record(self):
        return ef.opencode_record(self.logs_dir)
