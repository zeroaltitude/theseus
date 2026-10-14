"""The fake Discord DM surface: `theseus-sim fake-discord` (a stand-in for Discord's REST and gateway) beside a scratch
daemon whose `[discord]` points at it, the owner's DM bound, and the owner's id in `[places] owner` (a DM with an
owner is a private place). A step on this surface is a message typed in the DM through the fake's control route
(`POST /_fake/say`) and the bot's reply read back from `GET /_fake/messages`: the text a person would see in
Discord, after the daemon's own rendering of it (the reply's pieces; a footer line is Discord's, and is part of what
the checks read).

The DM is its own place, so its session is one the plan did not open: the exam's Discord steps all run in that one
session, in order. They are the same prompts as the CLI's, so the checks compare the surfaces.

Standard library only.
"""

from __future__ import annotations

import http.client
import json
import re
import socket
import subprocess
import time
from pathlib import Path

OWNER_ID = 900_000_000_000_000_101
GUILD_ID = 900_000_000_000_000_001
BOT_ID = "1553557000000000001"
STABLE_SECS = 1.5
POLL = 0.1
TOKEN_ENV = "EXAM_DISCORD_TOKEN"


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def bindings_toml() -> str:
    return (f'guild_id = "{GUILD_ID}"\n[[dm]]\nuser = "{OWNER_ID}"\nname = "owner"\n')


def config_patch(cfg: dict, rest: str, gateway: str) -> dict:
    """The daemon's config with Discord on the stand-in: the bot token an `env:` secret, the DM bound, the owner's id
    an owner of private places, and fast edits (the fake is not rate limited)."""
    cfg.setdefault("secrets", {})["discord_bot_token"] = f"env:{TOKEN_ENV}"
    cfg["discord"] = {"enabled": True, "token_secret": "discord_bot_token", "bindings_file": "bindings.toml",
                      "rest_proxy": rest, "gateway_proxy": f"ws://{gateway}", "edit_interval_ms": 250}
    cfg.setdefault("places", {})["owner"] = [f"discord:{OWNER_ID}"]
    return cfg


def control(addr: str, method: str, route: str, body: dict | None = None, timeout: float = 30.0):
    host, _, port = addr.partition(":")
    c = http.client.HTTPConnection(host, int(port), timeout=timeout)
    try:
        c.request(method, f"/_fake/{route}", body=json.dumps(body) if body is not None else None)
        r = c.getresponse()
        raw = r.read()
        return r.status, (json.loads(raw) if raw else None)
    finally:
        c.close()


FOOTER = re.compile(r"^-# .*$", re.M)
THIS_REPLY = re.compile(r"\$([0-9]+(?:\.[0-9]+)?) this reply")


def split_footer(text: str) -> tuple[str, str, float | None]:
    """A Discord reply as the model wrote it, the footer line the daemon adds (`-# profile · model · loops · $ · s`,
    small text), and the dollars the footer gives for this reply. The checks read the first: the footer is the
    harness's, not the persona's."""
    foot = FOOTER.findall(text)
    body = FOOTER.sub("", text).rstrip()
    m = THIS_REPLY.search(" ".join(foot))
    return body, "\n".join(foot), float(m.group(1)) if m else None


class FakeDiscord:
    """The stand-in process: REST on `rest`, its gateway on `gateway`, a guild for the owner."""

    def __init__(self, sim: Path, workdir: Path):
        self.sim = Path(sim)
        self.dir = workdir
        self.rest = f"127.0.0.1:{free_port()}"
        self.gateway = f"127.0.0.1:{free_port()}"
        self.proc: subprocess.Popen | None = None

    def start(self, log: Path) -> None:
        guild = self.dir / "guild.json"
        guild.write_text(json.dumps({
            "id": GUILD_ID, "name": "exam-guild", "owner": OWNER_ID, "everyone": 68608, "roles": [],
            "members": [{"id": OWNER_ID, "name": "owner", "roles": [], "bot": False},
                        {"id": int(BOT_ID), "name": "Theseus (fake)", "roles": [], "bot": True}],
            "channels": []}))
        self.log = log.open("w")
        self.proc = subprocess.Popen(
            [str(self.sim), "fake-discord", "--addr", self.rest, "--gateway", self.gateway, "--guild", str(guild)],
            stdin=subprocess.DEVNULL, stdout=self.log, stderr=subprocess.STDOUT)
        t0 = time.monotonic()
        while True:
            try:
                control(self.rest, "GET", "messages", timeout=2)
                return
            except OSError:
                if self.proc.poll() is not None:
                    raise RuntimeError("fake-discord exited before serving: see its log")
                if time.monotonic() - t0 > 30:
                    raise RuntimeError("fake-discord did not serve in time")
                time.sleep(0.05)

    def stop(self) -> None:
        if self.proc is not None:
            self.proc.terminate()
            try:
                self.proc.wait(10)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()
            self.proc = None
            self.log.close()

    def messages(self) -> list[dict]:
        return control(self.rest, "GET", "messages")[1] or []

    def say(self, text: str, wait: float = 60.0) -> str:
        """Type `text` in the owner's DM; the id of the message. The gateway takes a moment to hold the daemon."""
        t0 = time.monotonic()
        while True:
            status, v = control(self.rest, "POST", "say", {"channel": None, "user": OWNER_ID, "name": "owner",
                                                           "content": text})
            if status == 200:
                return v["id"]
            if time.monotonic() - t0 > wait:
                raise RuntimeError(f"the fake would not take a message: {status} {v}")
            time.sleep(0.2)

    def answer(self, typed_id: str, timeout: float) -> str | None:
        """The bot's reply to a typed message: its parts (the messages that answer it) in order, joined, once they
        have not changed for STABLE_SECS. None at the timeout."""
        t0 = time.monotonic()
        last, since = None, time.monotonic()
        while time.monotonic() - t0 < timeout:
            mine = sorted((m for m in self.messages() if m.get("reply_to") == typed_id and m.get("author") == BOT_ID),
                          key=lambda m: int(m["id"]))
            snap = [(m["id"], m["content"], m.get("edits")) for m in mine]
            if snap != last:
                last, since = snap, time.monotonic()
            elif snap and time.monotonic() - since >= STABLE_SECS:
                return "\n".join(m["content"] for m in mine)
            time.sleep(POLL)
        return "\n".join(c for _, c, _ in last) if last else None
