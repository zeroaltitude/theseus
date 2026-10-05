"""Theseus's request estimate, mirrored for the recall bench's generator
(theseus-523y): a request's bytes by class, read at the model's rates, with
its framing, the ring's margin, and the request budget. Each constant names
its Rust source, and test_generate reads each one from that file, as it
reads `generate.PRICES` from the catalog.

- **The census** (theseus-core's provider.rs, `Census`): tool schemas, tool
  inputs and tool results are `json` bytes; the system's and the messages'
  text is `text`; each message (and each system block) costs
  `MESSAGE_TOKENS`, each content block `BLOCK_TOKENS`, each tool id (in its
  `tool_use` and again in its `tool_result`) `ID_TOKENS`.
- **The rates** (catalog.rs, `TokenRates`): bytes a token, by family.
- **The estimate** (compiler.rs, `estimate`): from a compilation's second
  call on, the provider's count of the last request is `counted` and only
  what was written since is estimated; a new session's first call, and each
  of the ring's candidates, is estimated whole. Its upper bound is `counted`
  + the estimate × (1 + `MARGIN_PERCENT`/100).
- **The ring** (compiler.rs): it runs when the upper bound passes the
  request budget (the window less the output cap and `HEADROOM`), keeps
  turns while the whole estimate is under `RING_TARGET` of it, and a turn
  whose newest exchange alone still passes the budget is an overage: the
  turn fails before any call.
- **A tool result** is capped at `RESULT_MAX_CHARS` (config.rs,
  `default_result_max_chars`), and `fs_read` numbers each line with
  `FS_READ_PREFIX` bytes (theseus-tools' fs.rs, `"{:>6}\\t"`).

Standard library only.
"""

from __future__ import annotations

import json
import math
from dataclasses import dataclass

# theseus-core/src/catalog.rs, `TokenRates`: (json, text) bytes a token.
RATES = {
    "CLAUDE": (2.4, 3.3),  # Claude since Opus 4.7: Sonnet 5 and 5.5, Opus 5 and 5.5, Fable
    "CLAUDE_OLD": (2.9, 4.0),  # Haiku 4.5
    "GLM": (3.7, 4.4),
}
# theseus-core/src/provider.rs
MESSAGE_TOKENS = 3
BLOCK_TOKENS = 1
ID_TOKENS = 15
OPAQUE_BYTES_PER_TOKEN = 4.0
DENSEST_BYTES_PER_TOKEN = 0.5
# theseus-core/src/compiler.rs: the estimate's margin, the budget's
# headroom (`request_budget`), and the share of the budget a ring keeps
# (`budget * 6 / 10`).
MARGIN_PERCENT = 40
HEADROOM = 4096
RING_TARGET = (6, 10)
# theseus-core/src/turn/compaction.rs: the most a compaction's summary may
# be. It is written only where it fits beside the ring's kept turns (their
# upper bound) within the budget; where it does not, the ring keeps the cut
# with no summary (`context.compacted`'s outcome `ring`).
SUMMARY_MAX_TOKENS = 4096
# theseus-core/src/config.rs, `default_result_max_chars`: the characters of
# a tool result the model sees.
RESULT_MAX_CHARS = 30_000
# theseus-tools/src/fs.rs: `fs_read` writes each line as `{:>6}\t{line}\n`.
FS_READ_PREFIX = 7


def rates_of(model: str) -> tuple[float, float]:
    """`TokenRates::of`: the built-in figures for a model, by its family."""
    m = model.split("/", 1)[-1]
    if m.startswith("claude-haiku-4"):
        return RATES["CLAUDE_OLD"]
    if m.startswith("glm-"):
        return RATES["GLM"]
    return RATES["CLAUDE"]


@dataclass
class Census:
    """A request's bytes by class, and its framing (provider.rs)."""

    json: int = 0
    text: int = 0
    opaque: int = 0
    messages: int = 0
    blocks: int = 0
    ids: int = 0

    def __add__(self, o: "Census") -> "Census":
        return Census(self.json + o.json, self.text + o.text, self.opaque + o.opaque,
                      self.messages + o.messages, self.blocks + o.blocks, self.ids + o.ids)

    def tokens(self, rates: tuple[float, float]) -> int:
        """`Census::tokens`: each class's bytes at its rate, rounded up, and
        the framing."""
        js, tx = rates

        def at(n: int, per: float) -> int:
            return math.ceil(n / max(per, DENSEST_BYTES_PER_TOKEN))

        return (at(self.json, js) + at(self.text, tx) + at(self.opaque, OPAQUE_BYTES_PER_TOKEN)
                + self.messages * MESSAGE_TOKENS + self.blocks * BLOCK_TOKENS + self.ids * ID_TOKENS)


def _blen(s: str) -> int:
    return len(s.encode())


def _json_len(v) -> int:
    return len(json.dumps(v, separators=(",", ":"), ensure_ascii=False).encode())


def census_of_block(b: dict) -> Census:
    c = Census(blocks=1)
    t = b.get("type")
    if t == "text":
        c.text += _blen(b.get("text", ""))
    elif t == "thinking":
        c.text += _blen(b.get("thinking", ""))
        c.opaque += _blen(b.get("signature", ""))
    elif t == "redacted_thinking":
        c.opaque += _blen(b.get("data", ""))
    elif t in ("image", "document"):
        pass
    elif t == "tool_use":
        c.ids += 1
        c.json += _blen(b.get("name", "")) + _json_len(b.get("input", {}))
    elif t == "tool_result":
        c.ids += 1
        content = b.get("content")
        if isinstance(content, str):
            c.json += _blen(content)
        elif isinstance(content, list):
            for x in content:
                c.blocks += 1
                if x.get("type") == "text":
                    c.json += _blen(x.get("text", ""))
                elif x.get("type") not in ("image", "document"):
                    c.json += _json_len(x)
        elif content is not None:
            c.json += _json_len(content)
    else:
        c.json += _json_len(b)
    return c


def census_of_messages(messages: list[dict]) -> Census:
    """`Census::of_messages`."""
    c = Census()
    for m in messages:
        c.messages += 1
        content = m.get("content")
        if isinstance(content, str):
            c.blocks += 1
            c.text += _blen(content)
        elif isinstance(content, list):
            for b in content:
                c = c + census_of_block(b)
    return c


def census_of_request(req: dict) -> Census:
    """`ProviderRequest::census`: the messages, each tool's schema as json,
    and each system block as a message of text."""
    c = census_of_messages(req.get("messages", []))
    for t in req.get("tools", []) or []:
        c.json += _json_len(t)
    system = req.get("system") or []
    if isinstance(system, str):
        system = [{"type": "text", "text": system}]
    for b in system:
        c.messages += 1
        c.text += _blen(b.get("text", ""))
    return c


def upper(counted: int, estimated: int) -> int:
    """`Estimate::upper`: the counted part, and the estimate with its margin."""
    return counted + estimated + math.ceil(estimated * MARGIN_PERCENT / 100)


def bound(estimated: int) -> int:
    """The upper bound of a request estimated whole (no counted part)."""
    return upper(0, estimated)


def ring_target(budget: int) -> int:
    """The estimate a ring keeps turns under."""
    return budget * RING_TARGET[0] // RING_TARGET[1]


# ---- a turn's messages, as the generator plans them


def user_text(text: str) -> Census:
    """A user message of text."""
    return Census(messages=1, blocks=1, text=_blen(text))


def call(name: str, args: dict) -> Census:
    """An assistant message with one tool call."""
    return Census(messages=1, blocks=1, ids=1, json=_blen(name) + _json_len(args))


def result(nbytes: int) -> Census:
    """A user message with one tool result of `nbytes`, capped as the model
    sees it."""
    return Census(messages=1, blocks=1, ids=1, json=min(nbytes, RESULT_MAX_CHARS))


def fs_read_bytes(content: str) -> int:
    """The bytes `fs_read` returns for a file: each line numbered."""
    return sum(FS_READ_PREFIX + _blen(line) + 1 for line in content.splitlines())
