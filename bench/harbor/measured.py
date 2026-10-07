"""What every arm built on one of Harbor's own agents adds to it (theseus-qags).

Harbor 0.23 ships an installed agent for Codex CLI, Aider, OpenCode,
OpenHands and OpenClaw. Each arm here is a thin subclass of Harbor's, with
this mixin first in its bases, so the install, the command line, the options
and `name()` stay Harbor's and the results read as that harness's. As the
Claude Code and Pi arms do (`claude_code_agent.py`, `pi_agent.py`), it adds:

- **the version**: `PINNED_VERSION` unless `--ak version=…` names another;
- **the effort**: the harness's own effort option (`EFFORT_OPTION`) set to
  `measure.EFFORT` (medium) unless an `--ak` sets it (an ablation);
- **the caps**: `--ak max_budget_usd=… --ak max_turns=…`, the other arms'
  caps, taken by every arm. Where the harness has caps of its own
  (`enforced_caps`), they are passed to it; elsewhere they are recorded and
  the record flags a trial that passed them (`limits`), as Pi's are;
- **install**: Harbor's, then the version as the container reads it
  (`version.txt`) and the harness sampler (`sampler.py`);
- **run**: the sampler around Harbor's run, stopped in a `finally`; at
  Harbor's timeout the agent is stopped first (`measure.stop_agent`), since
  Harbor's Docker environment ends only its exec client (theseus-sgpx);
- **populate_context_post_run**: after Harbor's own, the trial's efficiency
  record (`agent/efficiency.json` and `metadata["efficiency"]`) from the
  arm's `record()`, with the effort asked for and the version read; and, where
  Harbor's own left the trial unpriced, its counters and dollars from the
  record.

`rewrite(command, env)` sees every command Harbor runs as the agent's user,
for an arm that must change its harness's command line (Aider's model and
logs, OpenHands' process name).

The sampler's interval is BENCH_SAMPLE_MS (250 by default), from the
environment of `harbor run`.
"""

from __future__ import annotations

import asyncio
import os
import shlex
from pathlib import Path
from typing import Any

import efficiency as ef
import measure
import sampler as smp

DIR = "/installed-agent/measure"
SAMPLER = f"{DIR}/sampler.py"
STATE = f"{DIR}/state"


class MeasuredArm:
    """The mixin: list it before Harbor's agent class in the bases."""

    ARM: str = ""
    PINNED_VERSION: str | None = None
    # The keyword Harbor's agent takes the reasoning effort under, and the
    # value medium is in that harness's words.
    EFFORT_OPTION: str | None = None
    EFFORT_VALUE: str = measure.EFFORT

    def __init__(self, *args: Any, version: str | None = None, max_budget_usd: Any = None,
                 max_turns: Any = None, **kwargs: Any):
        self.caps = {
            "max_budget_usd": None if max_budget_usd in (None, "") else float(max_budget_usd),
            "max_turns": None if max_turns in (None, "") else int(max_turns),
        }
        if self.EFFORT_OPTION and kwargs.get(self.EFFORT_OPTION) is None:
            kwargs[self.EFFORT_OPTION] = self.EFFORT_VALUE
        for k, v in self.enforced_caps(**self.caps).items():
            kwargs.setdefault(k, v)
        super().__init__(*args, version=version or self.PINNED_VERSION, **kwargs)

    # ------------------------------------------------------------- hooks

    def enforced_caps(self, max_budget_usd: float | None, max_turns: int | None) -> dict[str, Any]:
        """The harness's own options that enforce the caps; none by default."""
        return {}

    def caps_enforced(self) -> bool:
        return bool(self.enforced_caps(**self.caps))

    def effort_asked(self) -> str | None:
        if not self.EFFORT_OPTION:
            return None
        return getattr(self.options, self.EFFORT_OPTION, None)

    def rewrite(self, command: str, env: dict[str, str] | None) -> tuple[str, dict[str, str] | None]:
        return command, env

    def record(self) -> dict[str, Any]:
        raise NotImplementedError

    async def after_install(self, environment: Any) -> None:
        pass

    # ------------------------------------------------------ Harbor's own

    async def exec_as_agent(self, environment: Any, command: str, env: dict[str, str] | None = None,
                            cwd: str | None = None, timeout_sec: int | None = None) -> Any:
        command, env = self.rewrite(command, env)
        return await super().exec_as_agent(environment, command, env=env, cwd=cwd,  # type: ignore[misc]
                                           timeout_sec=timeout_sec)

    async def install(self, environment: Any) -> None:
        await super().install(environment)  # type: ignore[misc]
        await self.measure_install(environment)

    async def measure_install(self, environment: Any) -> None:
        """What the arm adds once the harness is installed."""
        await self.after_install(environment)
        await measure.record_version(environment, self.get_version_command(),  # type: ignore[attr-defined]
                                     self.environment_logs_dir.as_posix())  # type: ignore[attr-defined]
        await self.exec_as_root(environment, command=f"mkdir -p {STATE}")  # type: ignore[attr-defined]
        await environment.upload_file(Path(smp.__file__), SAMPLER)
        owner = ""
        if environment.default_user is not None:
            owner = f" && chown {shlex.quote(str(environment.default_user))} {STATE}"
        await self.exec_as_root(environment, command=f"chmod 755 {DIR} {SAMPLER}{owner}")  # type: ignore[attr-defined]

    async def run(self, instruction: str, environment: Any, context: Any) -> None:
        # Harbor's own run renders the prompt template; this one must not.
        arm = ef.ARMS[self.ARM]
        logs = self.environment_logs_dir.as_posix()  # type: ignore[attr-defined]
        sample_ms = int(os.environ.get("BENCH_SAMPLE_MS", str(smp.INTERVAL_MS)))
        start = smp.start_script(SAMPLER, logs, STATE, arm["names"], arm["wrapper_args"], sample_ms)
        # As the agent's user, as Harbor runs the harness; never the trial's failure.
        await environment.exec(command=f"{start}; true", timeout_sec=30)
        try:
            await super().run(instruction, environment, context)  # type: ignore[misc]
        except asyncio.CancelledError:
            # Harbor's timeout: stop the harness and what it started before
            # the sampler's stop, and before the verifier starts.
            await measure.stop_agent(environment, arm["names"])
            raise
        finally:
            await environment.exec(command=smp.stop_script(logs, STATE), timeout_sec=30)

    def populate_context_post_run(self, context: Any) -> None:
        try:
            super().populate_context_post_run(context)  # type: ignore[misc]
        except Exception:  # noqa: BLE001: Harbor's converter never fails the trial either
            pass
        logs: Path = self.logs_dir  # type: ignore[attr-defined]
        try:
            rec = ef.stamp(logs, self.record(), self.effort_asked(), self.version(),  # type: ignore[attr-defined]
                           self.parse_version)  # type: ignore[attr-defined]
            rec["limits"] = ef.limits(rec, self.caps["max_budget_usd"], self.caps["max_turns"],
                                      self.caps_enforced())
            ef.write(logs, rec)
            t = rec.get("tokens") or {}
            if getattr(context, "cost_usd", None) is None and rec.get("cost_usd") is not None:
                # Harbor's own left the trial unpriced: its counters as the
                # other arms' hold them, the cache write in the input.
                context.n_input_tokens = t.get("input", 0) + t.get("cache_read", 0) + t.get("cache_write", 0)
                context.n_cache_tokens = t.get("cache_read", 0)
                context.n_output_tokens = t.get("output", 0)
                context.cost_usd = rec["cost_usd"]
        except Exception as e:  # noqa: BLE001: a record never fails the trial
            rec = {"schema": ef.SCHEMA, "arm": self.ARM, "error": f"{type(e).__name__}: {e}"}
        context.metadata = ef.metadata(getattr(context, "metadata", None), rec)
