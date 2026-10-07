"""Aider as Harbor runs it, measured as the other arms are (theseus-qags).

    harbor run -a aider_agent:MeasuredAider \\
        -m anthropic/claude-sonnet-5-5 --ak max_budget_usd=2.0 --ak max_turns=200 …

with this directory on PYTHONPATH. Harbor's own `Aider` (`-a aider`: `aider
--yes --message=<the task>`, one message in scripting mode) with
`measured.MeasuredArm`: `--reasoning-effort medium`, the sampler around its
run, and the record from Aider's own output (`efficiency.aider_record`). It
adds:

- **the install, pinned**: Harbor's runs Aider's installer, which takes the
  latest release; this one installs uv and then `aider-chat==<PINNED_VERSION>`
  as a uv tool, the same layout (`~/.local/bin/aider`, `~/.local/bin/env`);
- **the model, by provider**: Harbor passes `--model=claude-sonnet-5-5`;
  Aider's LiteLLM needs the provider to route a model its table may not know,
  so the arm passes `--model=anthropic/claude-sonnet-5-5`;
- **its prices**: Aider 0.86.2's table predates Claude Sonnet 5.5, so the arm
  uploads a model-metadata file (`--model-metadata-file`) with its context
  window and its list prices (`efficiency.LIST_PRICES`): the dollars Aider
  prints are then the list price's;
- **its logs**: `--analytics-log` (local only; `--no-analytics` sends
  nothing) for each call's exact token counts, no update check, and
  `--no-check-model-accepts-settings`, so the effort is sent to the model
  rather than dropped for a model Aider does not know.

Aider has no spend cap and no turn cap: the caps are recorded, not enforced.
Aider does not drive a loop of tool calls as the other arms do: it answers
one message, with up to three reflections, and runs the shell commands it
suggests (`--yes-always`).
"""

from __future__ import annotations

import json
import re
import tempfile
from pathlib import Path

from harbor.agents.installed.aider import Aider

import efficiency as ef
from measured import DIR, MeasuredArm

# aider-chat's latest release on PyPI when this arm was built (2026-10-07).
PINNED_VERSION = "0.86.2"
METADATA = f"{DIR}/aider-models.json"
ANALYTICS = "/logs/agent/aider-analytics.jsonl"
RUN = re.compile(r"(?:^|[\s;])aider --yes ")
UV_INSTALL = "https://astral.sh/uv/install.sh"


def metadata(model: str) -> dict:
    """The model-metadata entry Aider reads (LiteLLM's keys): the context
    window and the list prices a token."""
    p = ef.LIST_PRICES[model.split("/")[-1]]
    return {model: {
        "max_input_tokens": 1000000, "max_output_tokens": 128000, "max_tokens": 128000,
        "input_cost_per_token": p["input"] / 1e6, "output_cost_per_token": p["output"] / 1e6,
        "cache_read_input_token_cost": p["cache_read"] / 1e6,
        "cache_creation_input_token_cost": p["cache_write"] / 1e6,
        "litellm_provider": model.split("/")[0], "mode": "chat",
        "supports_function_calling": True, "supports_prompt_caching": True, "supports_reasoning": True,
    }}


def install_script(version: str | None) -> str:
    spec = f"aider-chat=={version}" if version else "aider-chat"
    return (
        "set -euo pipefail; "
        f"curl -LsSf {UV_INSTALL} | sh && "
        'if [ -f "$HOME/.local/bin/env" ]; then . "$HOME/.local/bin/env"; fi && '
        f"uv tool install --force --python 3.12 {spec} && "
        "aider --version"
    )


class MeasuredAider(MeasuredArm, Aider):
    ARM = "aider"
    PINNED_VERSION = PINNED_VERSION
    EFFORT_OPTION = "reasoning_effort"

    async def install(self, environment):
        # Harbor's install with the version pinned, then the arm's own.
        await self.ensure_system_dependencies(environment, ("curl",))
        await self.exec_as_agent(environment, command=install_script(self._version))
        await self.measure_install(environment)

    async def after_install(self, environment):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "aider-models.json"
            path.write_text(json.dumps(metadata(self.model_name or "anthropic/claude-sonnet-5-5"), indent=2))
            await self.exec_as_root(environment, command=f"mkdir -p {DIR}")
            await environment.upload_file(path, METADATA)
        await self.exec_as_root(environment, command=f"chmod 755 {DIR} && chmod 644 {METADATA}")

    def rewrite(self, command, env):
        if not RUN.search(command):
            return command, env
        if self.model_name and "/" in self.model_name:
            bare = self.model_name.split("/", 1)[1]
            command = command.replace(f"--model={bare} ", f"--model={self.model_name} ")
        extra = (f"--model-metadata-file {METADATA} --analytics-log {ANALYTICS} --no-analytics "
                 "--no-check-update --no-show-release-notes --no-check-model-accepts-settings ")
        return command.replace("aider --yes ", "aider --yes " + extra, 1), env

    def record(self):
        return ef.aider_record(self.logs_dir)
