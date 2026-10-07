"""Aider as Harbor runs it, measured as the other arms are (theseus-qags).

    harbor run -a aider_agent:MeasuredAider \\
        -m anthropic/claude-sonnet-5-5 --ak max_budget_usd=2.0 --ak max_turns=200 …

with this directory on PYTHONPATH. Harbor's own `Aider` (`-a aider`: `aider
--yes --message=<the task>`, one message in scripting mode) with
`measured.MeasuredArm`: the sampler around its run, and the record from
Aider's own output (`efficiency.aider_record`). It adds:

- **the install, pinned**: Harbor's runs Aider's installer, which takes the
  latest release; this one installs uv and then `aider-chat==<PINNED_VERSION>`
  as a uv tool, the same layout (`~/.local/bin/aider`, `~/.local/bin/env`);
- **the model, by provider**: Harbor passes `--model=claude-sonnet-5-5`;
  Aider's LiteLLM needs the provider to route a model its table may not know,
  so the arm passes `--model=anthropic/claude-sonnet-5-5`;
- **its model settings** (`--model-settings-file`): Aider 0.86.2 predates
  Claude Sonnet 5.5, so the arm gives it the settings Aider ships for its
  newest Sonnet (Sonnet 4.5's: `diff` edits, the repo map, prompt caching),
  with three changes Sonnet 5.5 needs or the bench's rules ask for. No
  `temperature` (`use_temperature: false`): Sonnet 5.5 refuses every request
  that sends one ("`temperature` is deprecated for this model", a 400), and
  Aider sends 0 by default. The effort as `output_config: {effort: medium}`,
  the field Theseus sends: Aider's own `--reasoning-effort` goes out as a
  literal `extra_body` key through its LiteLLM, which the API does not take
  (a recorder standing in for the API showed both, 2026-10-07). And Sonnet
  5.5 as the weak and editor model too, where Aider's Sonnet settings name
  Haiku, so every call is the arm's one model;
- **its prices** (`--model-metadata-file`): the context window and the list
  prices (`efficiency.LIST_PRICES`), which Aider's own table lacks;
- **its logs**: `--analytics-log` (local only; `--no-analytics` sends
  nothing) for each call's exact token counts, and no update check.

`--ak reasoning_effort=…` still names the effort (an ablation): it goes into
`output_config`. Aider has no spend cap and no turn cap: the caps are recorded,
not enforced. Aider does not drive a loop of tool calls as the other arms do:
it answers one message, with up to three reflections, and runs the shell
commands it suggests (`--yes-always`).
"""

from __future__ import annotations

import json
import re
import tempfile
from pathlib import Path

from harbor.agents.installed.aider import Aider

import efficiency as ef
import measure
from measured import DIR, MeasuredArm, with_caps

# aider-chat's latest release on PyPI when this arm was built (2026-10-07).
PINNED_VERSION = "0.86.2"
METADATA = f"{DIR}/aider-models.json"
SETTINGS = f"{DIR}/aider-model-settings.yml"
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


def settings(model: str, effort: str) -> list[dict]:
    """The model settings Aider reads (a YAML list; JSON is YAML): Aider 0.86.2's
    own for claude-sonnet-4-5, with no temperature, the effort, and the one model."""
    return [{
        "name": model, "edit_format": "diff", "weak_model_name": model, "use_repo_map": True,
        "examples_as_sys_msg": False, "cache_control": True, "use_temperature": False,
        "editor_model_name": model, "editor_edit_format": "editor-diff",
        "extra_params": {"max_tokens": 128000, "output_config": {"effort": effort}},
    }]


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
    options_model = with_caps(Aider.options_model)
    PINNED_VERSION = PINNED_VERSION

    def __init__(self, *args, reasoning_effort: str | None = None, **kwargs):
        # Not Harbor's --reasoning-effort (above): the settings file carries it.
        self._effort = reasoning_effort or measure.EFFORT
        super().__init__(*args, **kwargs)

    def effort_asked(self):
        return self._effort

    async def install(self, environment):
        # Harbor's install with the version pinned, then the arm's own.
        await self.ensure_system_dependencies(environment, ("curl",))
        await self.exec_as_agent(environment, command=install_script(self._version))
        await self.measure_install(environment)

    async def after_install(self, environment):
        model = self.model_name or "anthropic/claude-sonnet-5-5"
        await self.exec_as_root(environment, command=f"mkdir -p {DIR}")
        with tempfile.TemporaryDirectory() as d:
            for name, body, remote in (("aider-models.json", metadata(model), METADATA),
                                       ("aider-model-settings.yml", settings(model, self._effort), SETTINGS)):
                path = Path(d) / name
                path.write_text(json.dumps(body, indent=2))
                await environment.upload_file(path, remote)
        await self.exec_as_root(environment, command=f"chmod 755 {DIR} && chmod 644 {METADATA} {SETTINGS}")

    def rewrite(self, command, env):
        if not RUN.search(command):
            return command, env
        if self.model_name and "/" in self.model_name:
            bare = self.model_name.split("/", 1)[1]
            command = command.replace(f"--model={bare} ", f"--model={self.model_name} ")
        extra = (f"--model-settings-file {SETTINGS} --model-metadata-file {METADATA} "
                 f"--analytics-log {ANALYTICS} --no-analytics --no-check-update --no-show-release-notes ")
        return command.replace("aider --yes ", "aider --yes " + extra, 1), env

    def record(self):
        return ef.aider_record(self.logs_dir)
