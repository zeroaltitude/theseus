"""Harbor's OpenHands SDK runner, run unchanged, with its model calls kept (theseus-qags).

    openhands-py openhands_measure_run.py --instruction=… --logs-dir=… --trajectory-path=…

Harbor's runner (`/installed-agent/run_agent.py`) writes a trajectory whose
`final_metrics` hold only totals: no cache write, and no count of calls. This
wrapper notes every `openhands.sdk.LLM` that makes a call (the agent's, and a
condenser's if one runs) and, when the process exits, writes each one's
`Metrics` (a `token_usages` entry and a `costs` entry a call: prompt,
completion, cache read, cache write and reasoning tokens, and the dollars the
SDK's LiteLLM priced) to `openhands-metrics.json` in the logs directory
(`AGENT_LOGS_DIR`), for `efficiency.openhands_record`. Then it runs Harbor's
runner as `__main__`, with the same arguments.

It runs on the task's image in the SDK's venv: the standard library and the
SDK only. It never fails the run: a failure to note or to write is skipped.
"""

import atexit
import json
import os
import runpy
import sys

RUNNER = os.environ.get("HARBOR_OPENHANDS_RUNNER", "/installed-agent/run_agent.py")
OUT = "openhands-metrics.json"
SEEN = []


def note(llm):
    if all(x is not llm for x in SEEN):
        SEEN.append(llm)


def wrap(cls, name):
    original = getattr(cls, name, None)
    if original is None:
        return

    def call(self, *args, **kwargs):
        try:
            note(self)
        except Exception:  # noqa: BLE001
            pass
        return original(self, *args, **kwargs)

    setattr(cls, name, call)


def dump():
    out = []
    for llm in SEEN:
        try:
            out.append({"usage_id": getattr(llm, "usage_id", None), "model": getattr(llm, "model", None),
                        "metrics": llm.metrics.model_dump(mode="json")})
        except Exception as e:  # noqa: BLE001
            out.append({"error": f"{type(e).__name__}: {e}"})
    try:
        path = os.path.join(os.environ.get("AGENT_LOGS_DIR", "/logs/agent"), OUT)
        with open(path, "w") as f:
            json.dump({"llms": out}, f, indent=1)
    except Exception:  # noqa: BLE001
        pass


try:
    from openhands.sdk import LLM

    for method in ("completion", "responses"):
        wrap(LLM, method)
    atexit.register(dump)
except Exception:  # noqa: BLE001: the run goes on unmeasured
    pass

sys.argv = [RUNNER] + sys.argv[1:]
runpy.run_path(RUNNER, run_name="__main__")
