"""Codex CLI as Harbor runs it, measured as the other arms are (theseus-qags).

    harbor run -a codex_agent:MeasuredCodex \\
        -m openai/gpt-6.1-sol --ak max_budget_usd=2.0 --ak max_turns=200 …

with this directory on PYTHONPATH. Harbor's own `Codex` (`-a codex`) with
`measured.MeasuredArm`: Codex pinned at `PINNED_VERSION`, its
`model_reasoning_effort` medium, the sampler around its run, and the record
from Harbor's trajectory of its rollout (`efficiency.codex_record`).

**A native-model arm.** Codex 0.161 speaks only OpenAI's Responses API
(`wire_api = "chat"` is gone: "is no longer supported"), and Anthropic's API
answers the Chat Completions form but not Responses (404), so Codex cannot
reach Claude Sonnet 5.5 without a translating proxy between them. It runs its
own default model, GPT-6.1-Sol, on OpenAI's API (`OPENAI_API_KEY`), and its
points are drawn as run under conditions the other arms did not share.

Codex has no spend cap and no turn cap: the caps are recorded, not enforced.
"""

from __future__ import annotations

from harbor.agents.installed.codex import Codex

import efficiency as ef
from measured import MeasuredArm

# @openai/codex's latest release when this arm was built (2026-10-07).
PINNED_VERSION = "0.161.0"
# What `codex exec` runs when no model is named (its catalog's first).
NATIVE_MODEL = "openai/gpt-6.1-sol"


class MeasuredCodex(MeasuredArm, Codex):
    ARM = "codex"
    PINNED_VERSION = PINNED_VERSION
    EFFORT_OPTION = "reasoning_effort"

    def record(self):
        return ef.codex_record(self.logs_dir)
