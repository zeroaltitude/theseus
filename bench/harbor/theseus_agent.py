"""Theseus as a Harbor installed agent (theseus-n88g.3).

    harbor run -a theseus_agent:Theseus -m anthropic/claude-sonnet-5-5 \\
        -d terminal-bench@2.0 -i fix-git

with this directory on PYTHONPATH, THESEUS_BENCH_BIN_DIR naming a directory
that holds static `theseus` and `theseusd` (`bench/build.sh`), and the model's
key in ANTHROPIC_API_KEY. bench/README.md has the rest.

- **install** uploads the two static binaries to /installed-agent/bin.
  Nothing is downloaded in the container, so any image runs them, whatever
  its libc.
- **run** writes the bench profile (`bench/theseus-bench.toml`) with this
  trial's model, limits, and working directory, then runs one
  `theseus --spawn theseusd --json ask` (theseus-n88g.2): one turn, every
  loop until the model ends it, and a clean stop of its daemon. The key
  reaches the daemon only as ANTHROPIC_API_KEY in the exec's environment,
  which the profile names as `env:ANTHROPIC_API_KEY` (theseus-n88g.1).
  `ask`'s exit code says how the turn ended; one that the model did not end
  is raised as the matching error below, so Harbor's results say why, and
  the verifier still runs.
- **A timeout** (Harbor cancels `run`): the turn is stopped as `/stop` stops
  it (a SIGTERM to `theseus`), so it does not go on spending, or changing
  the task's files, while the verifier runs, and the run's own end then
  reads the session's history from the store.
- **populate_context_post_run** writes the trial's ATIF trajectory
  (`agent/trajectory.json`, which Harbor's viewer and usage totals read) and
  its tokens and dollars: from the turn's result, or, for a turn cut short,
  summed from the history's model calls, so a timed-out trial keeps its
  spend. Then its efficiency record (`agent/efficiency.json`, and
  `metadata["efficiency"]`; `efficiency.py`), with the harness sampler's
  CPU and memory (`sampler.py`, run around the turn at nice 19, every
  BENCH_SAMPLE_MS, 250 by default).

**The routed arm** (theseus-eo3h), "Theseus as shipped": `-a theseus_agent:TheseusRouted` (or
`-a theseus_agent:Theseus --ak routed=1`) writes `bench/theseus-bench-routed.toml` instead, which turns the judge on,
so Jev picks the model (and, by the profile, the effort) for each message from the owner's routing table. The Jev key
is read from `TYPESAFE_API_KEY` in the environment of `harbor run` and reaches the container only as that variable in
the exec's environment, for this arm alone (the profile names it `env:TYPESAFE_API_KEY`); the run fails before it
starts if it is unset. After the turn the run reads the ledger's `judge.call` and `route.decided` rows
(`theseus-judge.json`, `theseus-routes.json`), and the trial's record (`arm` `theseus-routed`) adds Jev's cost to its
dollars (`cost_usd`; the model's alone is `model_cost_usd`), and names the models and efforts the turns ran on
(`routing`).

Settings, from the environment of `harbor run`: THESEUS_BENCH_MAX_LOOPS
(200), THESEUS_BENCH_SPEND_LIMIT (2.0 dollars a trial), THESEUS_BENCH_PROC_SYNC
(900 s a command may keep the turn waiting), and THESEUS_BENCH_SYSTEM_FILE
(extra system text, for an A/B arm).
"""

from __future__ import annotations

import asyncio
import json
import os
import shlex
from pathlib import Path
from typing import Any, override

from harbor.agents.capabilities import AgentCapabilities
from harbor.agents.installed.base import (
    AgentSafetyRefusalError,
    BaseInstalledAgent,
    NonZeroAgentExitCodeError,
    with_prompt_template,
)
from harbor.agents.model_connection import ModelConnectionSpec
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

import efficiency as ef
import measure
import sampler as smp
import theseus_atif as atif
import theseus_bench as tb

BIN = "/installed-agent/bin"
STATE = "/installed-agent/state"
CONFIG = "/installed-agent/theseus.toml"
SAMPLER = f"{BIN}/sampler.py"


class TheseusTurnEnded(NonZeroAgentExitCodeError):
    """The turn returned without the model ending it: one of the classes
    below, by `theseus ask`'s exit code. The verifier still runs."""


class TheseusSpendLimitError(TheseusTurnEnded):
    """Exit 5: the trial's spend limit (THESEUS_BENCH_SPEND_LIMIT) was reached."""


class TheseusApprovalWaitError(TheseusTurnEnded):
    """Exit 6: a call waits for an approval, which no one gives in a trial."""


class TheseusTurnCutError(TheseusTurnEnded):
    """Exit 8: the loop cap, the output limit, or the context window ended
    the turn before the model did."""


class TheseusStoppedError(TheseusTurnEnded):
    """Exit 9, 130, or 143: the turn was stopped (a signal)."""


# Exit code → the error a trial is recorded with. 7, a refusal, is Harbor's own.
ERRORS: dict[int, type[NonZeroAgentExitCodeError]] = {
    5: TheseusSpendLimitError,
    6: TheseusApprovalWaitError,
    7: AgentSafetyRefusalError,
    8: TheseusTurnCutError,
    9: TheseusStoppedError,
    130: TheseusStoppedError,
    143: TheseusStoppedError,
}


class Theseus(BaseInstalledAgent):
    capabilities = AgentCapabilities(atif=True)
    MODEL_CONNECTION = ModelConnectionSpec(
        default_provider="anthropic",
        api_key_envs=("ANTHROPIC_API_KEY",),
        base_url_envs=("ANTHROPIC_BASE_URL",),
    )

    # The routed arm (theseus-eo3h): `TheseusRouted` sets it, `--ak routed=1` does too.
    ROUTED = False

    def __init__(self, *args: Any, routed: Any = None, **kwargs: Any):
        super().__init__(*args, **kwargs)
        if routed is not None:
            self.routed = str(routed).lower() in ("1", "true", "yes")
        else:
            self.routed = self.ROUTED

    @staticmethod
    @override
    def name() -> str:
        return "theseus"

    @override
    def get_version_command(self) -> str | None:
        return f"{BIN}/theseus --version"

    @override
    async def install(self, environment: BaseEnvironment) -> None:
        src = Path(os.environ.get("THESEUS_BENCH_BIN_DIR", "")).expanduser()
        for b in ("theseus", "theseusd"):
            if not (src / b).is_file():
                raise RuntimeError(
                    f"THESEUS_BENCH_BIN_DIR has no {b}: {src} (bench/build.sh builds them)"
                )
        await self.exec_as_root(environment, command=f"mkdir -p {BIN} {STATE}")
        for b in ("theseus", "theseusd"):
            await environment.upload_file(src / b, f"{BIN}/{b}")
        await environment.upload_file(Path(smp.__file__), SAMPLER)
        owner = ""
        if environment.default_user is not None:
            owner = f" && chown {shlex.quote(str(environment.default_user))} {STATE}"
        await self.exec_as_root(
            environment,
            command=f"chmod 755 {BIN}/theseus {BIN}/theseusd {SAMPLER} && chmod 700 {STATE}{owner}",
        )
        await measure.record_version(environment, self.get_version_command(),
                                     self.environment_logs_dir.as_posix())

    @override
    @with_prompt_template
    async def run(
        self, instruction: str, environment: BaseEnvironment, context: AgentContext
    ) -> None:
        provider, _, model = (self.model_name or "anthropic/claude-sonnet-5-5").rpartition("/")
        if provider not in ("", "anthropic"):
            raise ValueError(f"the bench profile runs Anthropic models, not {self.model_name}")
        workdir = (await environment.exec(command="pwd")).stdout.strip() or "/app"
        access = self.model_connection
        system_file = os.environ.get("THESEUS_BENCH_SYSTEM_FILE")
        values = tb.settings(
            model,
            workdir,
            max_loops=int(os.environ.get("THESEUS_BENCH_MAX_LOOPS", "200")),
            spend_limit_usd=float(os.environ.get("THESEUS_BENCH_SPEND_LIMIT", "2.0")),
            proc_sync_secs=int(os.environ.get("THESEUS_BENCH_PROC_SYNC", "900")),
            system=Path(system_file).read_text().strip() if system_file else None,
            api_base=access.configured_base_url,
            routed=self.routed,
        )
        jev_key = ""
        if self.routed:
            jev_key = os.environ.get(tb.JEV_KEY_ENV, "")
            if not jev_key:
                raise RuntimeError(f"the routed arm needs the Jev key in {tb.JEV_KEY_ENV}")
        config = tb.profile((tb.PROFILE_ROUTED if self.routed else tb.PROFILE).read_text(), values)
        await self._upload_config_text(
            environment, content=config, remote_path=CONFIG, filename="theseus.toml"
        )
        env = {
            "ANTHROPIC_API_KEY": access.api_key or "",
            "THESEUS_CONFIG": CONFIG,
            "THESEUS_STATE_DIR": STATE,
            "THESEUS_LOG": "info",
            "THESEUS_BENCH_INSTRUCTION": instruction,
        }
        if self.routed:
            # The Jev key reaches the container for this arm only, and only
            # as the daemon's environment: never in a file or the command.
            env[tb.JEV_KEY_ENV] = jev_key
        logs = self.environment_logs_dir.as_posix()
        try:
            sample_ms = int(os.environ.get("BENCH_SAMPLE_MS", str(smp.INTERVAL_MS)))
            await self.exec_as_agent(
                environment, command=tb.run_script(BIN, STATE, logs, SAMPLER, sample_ms, self.routed), env=env
            )
        except asyncio.CancelledError:
            # Harbor's timeout: stop the turn, and let the run's end read its
            # history, before the verifier starts.
            await environment.exec(command=tb.stop_script(STATE, logs), timeout_sec=60)
            raise
        except NonZeroAgentExitCodeError as e:
            code = await self._exit_code(environment, logs)
            error = ERRORS.get(code) if code is not None else None
            if error is None:
                raise
            raise error(f"theseus ask: {tb.ended(code)} (exit {code})\n{e}") from e

    async def _exit_code(self, environment: BaseEnvironment, logs: str) -> int | None:
        r = await environment.exec(command=f"cat {logs}/{tb.EXIT}")
        try:
            return int((r.stdout or "").strip())
        except ValueError:
            return None

    @override
    def populate_context_post_run(self, context: AgentContext) -> None:
        turn = _json(self.logs_dir / tb.TURN)
        history = _json(self.logs_dir / tb.HISTORY)
        try:
            code = int((self.logs_dir / tb.EXIT).read_text().strip())
        except (OSError, ValueError):
            code = None
        model = turn.get("model") if turn else None
        if history:
            trajectory = atif.trajectory(
                history,
                version=self.version() or "unknown",
                model_name=model or self.model_name,
                turn=turn,
            )
            (self.logs_dir / "trajectory.json").write_text(json.dumps(trajectory, indent=2))
        if turn:
            u = turn.get("usage") or {}
            read = u.get("cache_read_input_tokens") or 0
            written = u.get("cache_creation_input_tokens") or 0
            context.n_input_tokens = (u.get("input_tokens") or 0) + read + written
            context.n_cache_tokens = read
            context.n_output_tokens = u.get("output_tokens") or 0
            context.cost_usd = turn.get("cost_usd")
            source = "turn"
        elif history:
            # A turn cut short (a timeout) printed no result: its spend is
            # what its finished model calls cost, from the store.
            s = atif.spend(history)
            context.n_input_tokens = s["input_tokens"]
            context.n_cache_tokens = s["cache_tokens"]
            context.n_output_tokens = s["output_tokens"]
            context.cost_usd = s["cost_usd"]
            source = "history"
        else:
            source = None
        context.metadata = {
            "exit_code": code,
            "ended": tb.ended(code),
            "spend_from": source,
            **{
                k: (turn or {}).get(k)
                for k in ("stop_reason", "loops", "tool_calls", "elapsed_ms", "awaiting_confirm")
            },
            "model": model,
        }
        # The trial's efficiency record (theseus-7gir.12): tokens by class
        # and model, dollars, calls, and the harness's CPU and memory apart
        # from its work. A record that cannot be made never fails the trial.
        try:
            rec = ef.stamp(self.logs_dir, ef.theseus_record(self.logs_dir, tb.TURN, tb.HISTORY),
                           tb.profile_effort(path=tb.PROFILE_ROUTED if self.routed else None),
                           None, self.parse_version)
            if self.routed:
                efforts = ef.profile_efforts(tb.PROFILE_ROUTED.read_text())
                rec = ef.routed_record(rec, _json(self.logs_dir / tb.JUDGE),
                                       _json(self.logs_dir / tb.ROUTES), efforts)
                if rec["cost_usd"] is not None:
                    context.cost_usd = rec["cost_usd"]
            ef.write(self.logs_dir, rec)
        except Exception as e:  # noqa: BLE001
            rec = {"schema": ef.SCHEMA, "arm": ef.ROUTED_ARM if self.routed else "theseus", "error": f"{type(e).__name__}: {e}"}
        context.metadata = ef.metadata(context.metadata, rec)


class TheseusRouted(Theseus):
    """Theseus as shipped (theseus-eo3h): the judge on, Jev picking the model
    per message. See the module's text."""

    ROUTED = True

    @staticmethod
    @override
    def name() -> str:
        return "theseus-routed"


def _json(path: Path) -> dict[str, Any] | None:
    try:
        text = path.read_text().strip()
        return json.loads(text) if text else None
    except (OSError, json.JSONDecodeError):
        return None
