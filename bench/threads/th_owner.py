#!/usr/bin/env python3
"""threadbench's simulated owner (theseus-l7z1): the one policy every threads arm (A1, A1n, A1p, A2) follows when the
product asks something of its owner. It is the owner's side of the experiment, not a tuned helper: it is the same
for every arm, it is written before any run, and it reads only what a person at the keyboard knows of their own
message (its stream, its role, whether the stream has begun). It never reads a probe, an oracle, a fact, or a
message's label other than its stream.

- A new-thread offer is accepted on the first message of a stream (the owner knows when a new line of work
  starts) and declined on any later one. An offer made on a `home` message is declined and counted: it is the
  router over-splitting.
- A routing question is answered with the message's true stream: the option that is its thread, else `new` when
  the stream has just begun, else `home`.
- When a stream's first message ran at home and the product made no offer (no thread was live to ask about), the
  owner starts the stream's thread by hand, so its later messages have somewhere to go. Counted apart.

Standard library only.
"""

from __future__ import annotations

import re

HOME = "home"
OFFER = "Start a new thread"  # a `thread.asked` question that offers one (crates/theseus-core thread_route::offer_new)
ANSWER_LINE = re.compile(r"answer: theseus thread answer (\S+) (\S+)")  # the CLI's own line (thread/ask.rs `command`)
OPTION = re.compile(r"\[(\w+)\](?:ew|ome)? ?(.*?)(?= {3}\[|$)")  # `[1] pr round`, `[n]ew New thread «x»`, `[h]ome`


def parse_ask(question: str, options: str, answer: str) -> dict:
    """What `theseus ask` says on stderr when the router asks (thread/ask.rs `lines` and `command`): its id, the
    keys that answer it, each option's label by key, and whether it is a new-thread offer."""
    m = ANSWER_LINE.search(answer)
    if not m:
        raise ValueError(f"no answer command in {answer!r}")
    labels = {}
    for key, label in OPTION.findall(options):
        key = {"n": "new", "h": "home"}.get(key, key)
        labels[key] = label.strip()
    return {"id": m.group(1), "keys": m.group(2).split("|"), "labels": labels,
            "offer": question.lstrip("❓ ").startswith(OFFER)}


class Owner:
    """The policy, with its counts. `sent` is told each message after it went, so "first of its stream" is the
    owner's own memory of having begun it."""

    def __init__(self) -> None:
        self.begun: set[str] = set()
        self.title: dict[str, str] = {}  # a stream's thread's title, as the product shows it in a question
        self.over_split = 0  # offers made on a `home` message
        self.started_by_hand = 0  # threads the owner made because the product offered none
        self.answers: list[dict] = []

    def first_of_stream(self, stream: str) -> bool:
        return stream != HOME and stream not in self.begun

    def answer(self, stream: str, ask: dict) -> str:
        """The key that answers `ask` for a message of `stream`."""
        keys = ask["keys"]
        if ask["offer"]:
            if stream == HOME:
                self.over_split += 1
            choice = "new" if self.first_of_stream(stream) and "new" in keys else "home"
        else:
            title = self.title.get(stream)
            by_title = [k for k, label in ask["labels"].items() if title and label == title and k in keys]
            if stream == HOME:
                choice = "home"
            elif by_title:
                choice = by_title[0]
            else:
                choice = "new" if self.first_of_stream(stream) and "new" in keys else "home"
        self.answers.append({"ask": ask["id"], "stream": stream, "offer": ask["offer"], "choice": choice})
        return choice

    def sent(self, stream: str) -> None:
        if stream != HOME:
            self.begun.add(stream)

    def needs_thread(self, stream: str, first: bool, ran_in: str | None) -> bool:
        """The first message of a stream ran at home and no offer was made: the owner makes its thread."""
        return first and ran_in is None and stream != HOME and stream not in self.title
