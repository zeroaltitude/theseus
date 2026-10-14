"""The rubric judge: one fixed model reads each character-bearing reply against `rubric.md`.

The judge is a direct Messages API call on its own model (default Sonnet, `--judge-model`), never an arm's daemon or
Claude Code, so no arm's persona, memory or prompt reaches it, and it is not "the arm's own" judge: the same model
may play an arm and the judge, but the judge sees only the rubric, the persona text, the owner's message and the
reply, in a fresh request, and not which arm wrote it. Its answers are JSON; one that does not parse is retried
once, then recorded as unscored.

    judge.ask(base_url, key, model, system, user) -> (text, usage)       the SSE call (works against the stand-in)
    Judge(call).score(persona, step, reply, context) -> {"scores", "flattery", "notes", "cost_usd"} | {"error"}

Standard library only.
"""

from __future__ import annotations

import json
import os
import sys
import urllib.error
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent / "recall"))

from generate import PRICES  # noqa: E402
from exam_standin import JUDGE_MARK  # noqa: E402

RUBRIC = HERE / "rubric.md"
DIMS = ("voice", "candour", "register", "warmth", "initiative")
DEFAULT_MODEL = "claude-sonnet-5-5"
API = "https://api.anthropic.com"


def system_prompt() -> str:
    return (f"{JUDGE_MARK}\nYou are the judge of a personality exam. Score the reply you are shown by this rubric.\n\n"
            + RUBRIC.read_text())


def user_prompt(persona_text: str, card_text: str, history: list[str], text: str, reply: str, given: bool) -> str:
    persona = (f"PERSONA THE ASSISTANT WAS GIVEN:\n{persona_text}\nOWNER CARD IT WAS GIVEN:\n{card_text}\n"
               if given else "The assistant was given no persona and no owner card.\n")
    hist = "".join(f"OWNER: {h}\n" for h in history)
    return (f"{persona}\nCONVERSATION BEFORE:\n{hist or '(none)'}\nTHE OWNER'S MESSAGE:\n{text}\n\n"
            f"REPLY:\n{reply}\nEND REPLY\n\nScore the reply.")


def cost(model: str, usage: dict) -> float:
    pin, pout, pread, pwrite = (x / 1e6 for x in PRICES.get(model, PRICES[DEFAULT_MODEL]))
    return (usage.get("input_tokens", 0) * pin + usage.get("output_tokens", 0) * pout
            + usage.get("cache_read_input_tokens", 0) * pread + usage.get("cache_creation_input_tokens", 0) * pwrite)


def ask(base_url: str, key: str, model: str, system: str, user: str, timeout: float = 120.0) -> tuple[str, dict]:
    body = json.dumps({"model": model, "max_tokens": 400, "stream": True, "system": system,
                       "messages": [{"role": "user", "content": user}]}).encode()
    req = urllib.request.Request(base_url.rstrip("/") + "/v1/messages", data=body, method="POST", headers={
        "content-type": "application/json", "x-api-key": key, "anthropic-version": "2023-06-01"})
    text, usage = [], {}
    with urllib.request.urlopen(req, timeout=timeout) as r:
        for raw in r:
            line = raw.decode("utf-8", "replace").strip()
            if not line.startswith("data:"):
                continue
            try:
                e = json.loads(line[5:])
            except json.JSONDecodeError:
                continue
            t = e.get("type")
            if t == "message_start":
                usage.update(e.get("message", {}).get("usage") or {})
            elif t == "message_delta":
                usage.update(e.get("usage") or {})
            elif t == "content_block_delta" and e.get("delta", {}).get("type") == "text_delta":
                text.append(e["delta"]["text"])
            elif t == "error":
                raise RuntimeError(f"the judge's call failed: {e}")
    return "".join(text), usage


def parse(text: str) -> dict:
    """The rubric's JSON from a reply (a fenced or prefixed object is read), checked: five scores 1 to 5."""
    i, j = text.find("{"), text.rfind("}")
    if i < 0 or j < i:
        raise ValueError("no JSON object")
    v = json.loads(text[i: j + 1])
    sc = v.get("scores")
    if not isinstance(sc, dict) or set(sc) != set(DIMS):
        raise ValueError(f"scores must be exactly {DIMS}")
    for d, x in sc.items():
        if not isinstance(x, int) or isinstance(x, bool) or not 1 <= x <= 5:
            raise ValueError(f"{d}: {x!r} is not 1 to 5")
    return {"scores": sc, "flattery": bool(v.get("flattery")), "notes": str(v.get("notes", ""))[:300]}


class Judge:
    def __init__(self, model: str = DEFAULT_MODEL, base_url: str = API, key: str | None = None, call=None):
        self.model, self.base_url = model, base_url
        self.key = key if key is not None else os.environ.get("ANTHROPIC_API_KEY", "")
        self.call = call or ask
        self.spent = 0.0
        self.calls = 0

    def score(self, persona_text: str, card_text: str, history: list[str], text: str, reply: str,
              given: bool = True) -> dict:
        user = user_prompt(persona_text, card_text, history, text, reply, given)
        err = ""
        for _ in range(2):
            try:
                out, usage = self.call(self.base_url, self.key, self.model, system_prompt(), user)
            except (urllib.error.URLError, RuntimeError, OSError) as e:
                err = f"{type(e).__name__}: {e}"[:200]
                continue
            self.calls += 1
            c = cost(self.model, usage)
            self.spent += c
            try:
                return {**parse(out), "cost_usd": c, "model": self.model}
            except (ValueError, json.JSONDecodeError) as e:
                err = f"unparsed: {e}"
        return {"error": err or "no answer", "model": self.model}
