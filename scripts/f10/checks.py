"""F10's checks: one function per check, each reading a run's captured screens
and its records and answering yes or no with the evidence line (theseus-qy2a).

A run is what `driver.py` leaves in its directory, or a fixture: `run.json`
(the arm, the size, the records each step kept) and `screens/<name>.ansi`, each
a `tmux capture-pane -e` of the pane, its colours kept. A check never asks the
arm which client it is: it reads what a person would see, and the few facts a
screen cannot hold (a process alive, a key count, a file's bytes) from the
records the driver measured. Where a rule had to be chosen, it is in the
check's docstring, and docs/benchmarks/f10/README.md repeats it.
"""

import json
import re
from pathlib import Path

import steps

ANSI = re.compile(r"\x1b\[[0-9;:?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[()][0-9A-Za-z]|\x1b[=>]")
SGR = re.compile(r"\x1b\[([0-9;:]*)m")
SHELLS = {"bash", "sh", "zsh", "dash", "fish"}


def plain(ansi):
    """A screen's text, its escapes removed."""
    return ANSI.sub("", ansi)


class Run:
    """One arm's run at one size: its records and its screens."""

    def __init__(self, meta, screens):
        self.meta = meta
        self.arm = meta["arm"]
        self.cols, self.rows = (int(x) for x in meta["size"].split("x"))
        self.records = meta.get("records", {})
        self.screens = screens

    @classmethod
    def load(cls, path):
        path = Path(path)
        meta = json.loads((path / "run.json").read_text())
        screens = {p.name[: -len(".ansi")]: p.read_text() for p in (path / "screens").glob("*.ansi")}
        return cls(meta, screens)

    def rec(self, step, key, default=None):
        return self.records.get(step, {}).get(key, default)

    def ansi(self, name):
        return self.screens.get(name, "")

    def text(self, name):
        return plain(self.ansi(name))

    def lines(self, name):
        return self.text(name).splitlines()

    def has(self, name):
        return name in self.screens


def line_with(text, pattern, flags=re.I):
    """The first line of `text` that `pattern` finds, stripped, or None."""
    rx = re.compile(pattern, flags)
    for line in text.splitlines():
        if rx.search(line):
            return line.strip()
    return None


def missing(name):
    return False, f"no screen {name} was captured"


def yes(evidence):
    return True, evidence


def no(evidence):
    return False, evidence


# S1 Open


def s1a(run):
    """The open command is the binary alone, and what it left in the pane is a
    conversation: a program still in front (not the shell) whose screen takes
    input."""
    cmd = run.rec("S1", "open_cmd", "")
    front = run.rec("S1", "foreground", "")
    if len(cmd.split()) != 1:
        return no(f"the open command has arguments: `{cmd}`")
    if not front or front in SHELLS:
        last = next((l.strip() for l in reversed(run.lines("S1.open")) if l.strip()), "")
        return no(f"`{cmd}` returned to the shell: {last!r}")
    return yes(f"`{cmd}` left {front} in front, taking input")


def s1b(run):
    """The ready time the driver measured for the open command: from its Enter
    until the pane showed a prompt to type into, at most 2,000 ms. A command
    that never opened a conversation has no ready time."""
    ms = run.rec("S1", "ready_ms")
    if ms is None:
        return no(f"`{run.rec('S1', 'open_cmd', '')}` never showed a prompt to type into")
    return (ms <= 2000), f"a prompt after {ms} ms (`{run.rec('S1', 'open_cmd', '')}`)"


def s1c(run):
    """The first screen holds the model's name (its words, so `claude-sonnet-5-5`
    and `Sonnet 5.5` are both found) and the project directory (its path, `~`
    for the home allowed, or its last part)."""
    if not run.has("S1.open"):
        return missing("S1.open")
    text = run.text("S1.open")
    model = run.meta.get("model", "")
    names = model_names(model)
    found_model = next((n for n in names if n.lower() in text.lower()), None)
    project = run.meta.get("project_dir", "")
    found_dir = project and (project in text or Path(project).name in text)
    if found_model and found_dir:
        return yes(line_with(text, re.escape(found_model)) or found_model)
    lacks = [w for w, f in (("the model", found_model), ("the directory", found_dir)) if not f]
    return no(f"the first screen lacks {' and '.join(lacks)}")


def model_names(model):
    """The ways a screen may name `model`: its id, and its family and version
    in words (`claude-opus-5-5` is also `Opus 5.5`)."""
    names = [model] if model else []
    m = re.match(r"claude-([a-z]+)-(\d+)-(\d+)", model or "")
    if m:
        names.append(f"{m.group(1).capitalize()} {m.group(2)}.{m.group(3)}")
    return names


def s1d(run):
    """One line of the first screen says how to get help or how to use it:
    `--help`, `/help`, `? for help`, `help`, or `usage`."""
    if not run.has("S1.open"):
        return missing("S1.open")
    text = run.text("S1.open")
    for rx in (r"(--help|/help|\?\s*for\s*(help|shortcuts))", r"\busage\b", r"\bhelp\b"):
        line = line_with(text, rx)
        if line:
            return yes(line)
    return no("no line names help or usage")


# S2 Ask


def s2a(run):
    """Below the prompt, no markdown marker is left raw (a line starting `#`,
    a `**`, a backtick), and emphasis is drawn: some line carries bold,
    italic or underline. The stand-in's reply has all three; a real model's
    reply is read the same way."""
    name = "S2.reply"
    if not run.has(name):
        return missing(name)
    text = after_prompt(run.text(name), steps.PROMPTS["S2"])
    raw = line_with(text, r"(^\W{0,3}#{1,3} \w|\*\*\w|`\w)", re.M)
    if raw:
        return no(f"raw markdown: {raw!r}")
    ansi = after_prompt(run.ansi(name), steps.PROMPTS["S2"])
    styled = [plain(l).strip() for l in ansi.splitlines() if styled_line(l) and plain(l).strip()]
    if not styled:
        return no("no emphasis is drawn in the reply")
    return yes(styled[0])


def after_prompt(text, prompt):
    """`text` from the line after the one holding `prompt` (the reply's part
    of the screen); all of it when the prompt is not on it."""
    at = text.find(prompt)
    if at < 0:
        return text
    nl = text.find("\n", at)
    return text[nl + 1 :] if nl >= 0 else ""


def squash(text):
    """`text` with its line breaks and runs of spaces as one space, so a phrase
    wrapped across lines is found."""
    return re.sub(r"\s+", " ", text)


def styled_line(ansi_line):
    """Whether a line carries bold (1), italic (3) or underline (4)."""
    for m in SGR.finditer(ansi_line):
        parts = m.group(1).replace(":", ";").split(";")
        if any(p in ("1", "3", "4") for p in parts):
            return True
    return False


BARE_TOOL = r"^\W*(fs[._][a-z]+|proc[._]run|Read|Bash|Edit|Glob|Grep)\W*(\(\))?\W*$"


def s2b(run):
    """A project file's name is on the screen beside a tool call, and no tool
    call is shown by its name alone (a line that is only `fs.read`). The
    stand-in's reply names no file, so a file's name there is a call's."""
    name = "S2.reply"
    if not run.has(name):
        return missing(name)
    text = after_prompt(run.text(name), steps.PROMPTS["S2"])
    bare = line_with(text, BARE_TOOL, re.M)
    if bare:
        return no(f"a call shows only its tool: {bare!r}")
    named = line_with(text, r"(README\.md|tides\.py|test_tides\.py|slow_survey\.py)")
    return (named is not None), (named or "no call names what it touched")


def s2c(run):
    """Two captures while the reply is awaited, a second apart, differ: a
    spinner or an elapsed time moved."""
    a, b = run.ansi("S2.wait1"), run.ansi("S2.wait2")
    if not a or not b:
        return missing("S2.wait1/S2.wait2")
    if plain(a) == plain(b):
        last = next((l.strip() for l in reversed(plain(b).splitlines()) if l.strip()), "")
        return no(f"nothing moved in a second of waiting; the screen ends {last!r}")
    moved = [l.strip() for l, m in zip(plain(b).splitlines(), plain(a).splitlines()) if l != m]
    return yes(moved[0] if moved else "the screen changed")


def s2d(run):
    """No word is cut at the line's end: a line filled to the last column
    that ends in a letter, with the next line starting with a letter in its
    first column, is the terminal's own wrap through a word. A renderer that
    wraps at word boundaries ends its lines at a space, or indents the next.
    Read at the run's width; the 80x24 run is the one the check names. A reply
    of a line or two that never reaches the edge proves nothing, so the reply
    must hold a line within 10 columns of the edge."""
    name = "S2.reply"
    if not run.has(name):
        return missing(name)
    lines = after_prompt(run.text(name), steps.PROMPTS["S2"]).splitlines()
    for a, b in zip(lines, lines[1:]):
        if len(a) >= run.cols and a[-1].isalnum() and b[:1].isalnum():
            return no(f"cut: {a[-24:]!r} / {b[:24]!r}")
    if not any(len(l.rstrip()) >= run.cols - 10 for l in lines):
        return no(f"no line of the reply reaches the {run.cols}-column edge")
    return yes(f"no word is cut at {run.cols} columns")


# S3 Run the tests


def s3a(run):
    """The question's screen shows the command as a person types it
    (`python3 -m unittest -v test_tides`), not as JSON."""
    name = "S3.ask"
    if not run.has(name):
        return missing(name)
    line = line_with(squash(run.text(name)), r"python3 -m unittest")
    if line:
        return yes(re.search(r".{0,40}python3 -m unittest.{0,30}", line).group(0).strip())
    json_line = line_with(run.text(name), r'"python3"')
    return no(f"the command shows only as JSON: {json_line!r}" if json_line else "the command is not on the question's screen")


def s3b(run):
    """The driver's keys from the question to the approval: one key, and the
    command then ran (its output or its end was seen)."""
    keys = run.rec("S3", "approve_keys")
    if not keys:
        return no("no key approves the question")
    ran = run.rec("S3", "ran", False)
    if len(keys) == 1 and ran:
        return yes(f"one key: {keys[0]}")
    return no(f"{len(keys)} keys: {' '.join(keys)}" + ("" if ran else "; the command did not run"))


def s3c(run):
    """After the run, the conversation's screen shows the tests' own failure
    output: unittest's `FAIL: test_tidal_range`, its `AssertionError`, or
    `-2.8 != 2.8`. A model's sentence about it does not count: F10 measures
    the client (the stand-in's reply names no failure)."""
    name = "S3.after"
    if not run.has(name):
        return missing(name)
    line = line_with(run.text(name), r"(FAIL: test_tidal_range|AssertionError|-2\.8 != 2\.8)")
    return (line is not None), (line or "the test's own failure output is not on the screen")


def s3d(run):
    """No step's keystrokes held an id copied from the screen (the driver
    counts the ids it had to type: a session's, a question's)."""
    typed = run.rec("S3", "ids_typed", 0)
    if typed:
        return no(f"{typed} id(s) typed: {run.rec('S3', 'id_example', '')}")
    return yes("no id typed")


# S4 Fix, with a reviewed edit

OLD_BIT = "lowest - highest"
NEW_BIT = "highest - lowest"


def diff_lines(text):
    """The removed and added lines of the bug's edit, as a screen shows them."""
    old = line_with(text, re.escape(OLD_BIT))
    new = line_with(text, re.escape(NEW_BIT))
    return old, new


def is_json(line):
    return bool(line) and ('"old_string"' in line or '"new_string"' in line or "\\n" in line)


def s4a(run):
    """The question's screen shows both the line removed and the line added,
    as lines (not inside the call's JSON)."""
    name = "S4.ask"
    if not run.has(name):
        return missing(name)
    old, new = diff_lines(run.text(name))
    if old and new and not is_json(old) and not is_json(new) and old != new:
        return yes(f"{old} → {new}")
    shown = old or new
    return no(f"the edit shows as its call: {shown[:80]!r}" if shown else "no diff before approving")


def s4b(run):
    """The diff's two lines are coloured (a foreground or background colour)
    and differently, and the changed words carry a style of their own: a
    line holds two or more colours."""
    name = "S4.ask"
    if not run.has(name):
        return missing(name)
    ansi = run.ansi(name).splitlines()
    old = next((l for l in ansi if OLD_BIT in plain(l)), None)
    new = next((l for l in ansi if NEW_BIT in plain(l)), None)
    if not old or not new:
        return no("no diff lines to colour")
    co, cn = colours(old), colours(new)
    if not co or not cn or co == cn:
        return no("the diff's lines are not coloured apart")
    if max(len(co), len(cn)) < 2:
        return no("the changed words are not marked within their lines")
    return yes(f"removed {sorted(co)}, added {sorted(cn)}")


def colours(ansi_line):
    """The colour parameters an SGR in the line sets (30-49, 90-107, and
    38;… or 48;…)."""
    found = set()
    for m in SGR.finditer(ansi_line):
        parts = m.group(1).replace(":", ";").split(";")
        i = 0
        while i < len(parts):
            p = parts[i]
            if p in ("38", "48"):
                found.add(";".join(parts[i : i + (3 if parts[i + 1 : i + 2] == ["5"] else 5)]))
                i += 3 if parts[i + 1 : i + 2] == ["5"] else 5
                continue
            if p.isdigit() and (30 <= int(p) <= 37 or 40 <= int(p) <= 47 or 90 <= int(p) <= 107):
                found.add(p)
            i += 1
    return found


def s4c(run):
    """After the edit, the conversation still shows the diff's two lines."""
    name = "S4.after"
    if not run.has(name):
        return missing(name)
    old, new = diff_lines(run.text(name))
    if old and new and not is_json(old) and not is_json(new):
        return yes(f"{old} → {new}")
    return no("the diff is not in the conversation after the edit")


def s4d(run):
    """The file's name and a line number of the change are on the question's
    or the after screen: `tides.py` and the bug's line (49), beside the diff
    or as `tides.py:49`."""
    for name in ("S4.ask", "S4.after"):
        text = run.text(name)
        if "tides.py" not in text:
            continue
        line = line_with(text, rf"(tides\.py:{steps.BUG_LINE}\b|^\s*{steps.BUG_LINE}\s*[-+ ]|\b{steps.BUG_LINE}\s+[-+]\s)", re.M)
        if line:
            return yes(line)
    return no("no line number beside the file's name")


# S5 Don't ask again


def s5a(run):
    """The question offers to stop asking for the rest of the session or the
    project: "don't ask again" or "for this session/project"."""
    name = "S5.ask"
    if not run.has(name):
        return missing(name)
    line = line_with(run.text(name), r"(don.t ask again|for (the rest of )?(this|the) (session|project|conversation))")
    return (line is not None), (line or "no offer to stop asking")


def s5b(run):
    """The offer names the command's prefix (`python3`), so it covers that
    command, not every command the tool runs."""
    name = "S5.ask"
    if not run.has(name):
        return missing(name)
    line = line_with(run.text(name), r"(don.t ask again|for (the rest of )?(this|the) (session|project|conversation))")
    if not line:
        return no("no offer to scope")
    return ("python3" in line), line


def s5c(run):
    """One key (pressed once) switches the mode, and the screen then says file
    edits are accepted."""
    keys = run.rec("S5", "mode_keys")
    if not keys:
        return no("the arm has no key that accepts edits")
    line = line_with(run.text("S5.mode"), r"accept(ing)? edits|auto-accept")
    if len(keys) == 1 and line:
        return yes(f"{keys[0]}: {line}")
    return no(f"{len(keys)} keys" if line else "the screen does not say edits are accepted")


# S6 A 90-second command

ELAPSED = re.compile(r"(\b\d+(\.\d+)?\s?s\b|\b\d+m\s?\d+s\b|\b\d+:\d\d\b|elapsed)")


def s6a(run):
    """While the survey runs, a time that grows: a line with a time (`12s`,
    `1m 3s`, `0:12`) that differs between two captures 3 s apart."""
    a, b = run.text("S6.run1"), run.text("S6.run2")
    if not a or not b:
        return missing("S6.run1/S6.run2")
    before = set(a.splitlines())
    for line in b.splitlines():
        if line not in before and ELAPSED.search(line) and not re.search(r"\d\d:\d\d:\d\d", line):
            return yes(line.strip())
    return no("no elapsed time moves while it runs")


def s6b(run):
    """The survey's own output so far (`survey: station N/30`) is on the
    screen while it runs."""
    for name in ("S6.run2", "S6.run1"):
        line = line_with(run.text(name), r"survey: station \d+/30")
        if line:
            return yes(line)
    return no("none of its output so far is shown")


def s6c(run):
    """A message sent while it runs is answered while it still runs: the
    answer is on the screen and the survey had not ended (records)."""
    name = "S6.talk"
    if not run.has(name):
        return missing(name)
    if run.rec("S6", "ended_before_talk", False):
        return no("the survey had ended before the answer came")
    line = line_with(squash(run.text(name)), r"12:00 reading")
    return (line is not None), ("answered while it ran: " + steps.S6_TALK_REPLY if line else "no answer while it ran")


def s6d(run):
    """Once it ends, the screen says so, unasked (the survey's last line, or
    its job's end), and the agent took a turn of its own after it (the
    session's record, read by the driver: an answer written after the end
    with no message from the person between)."""
    name = "S6.end"
    if not run.has(name):
        return missing(name)
    text = run.text(name)
    end = line_with(text, r"(survey complete|slow_survey.*(done|finished|completed|exited|ended)|(done|finished|completed|exited|ended).*slow_survey|exit(ed)? (code|status) ?0|\bexit 0\b)")
    went_on = run.rec("S6", "went_on", False)
    if end and went_on:
        return yes(end)
    return no("its end is not announced" if not end else "the agent did not go on by itself")


# S7 Interrupt


def s7a(run):
    """One key, pressed while the long turn runs, stops it: the screen says it
    was interrupted or stopped, and the held answer never came."""
    keys = run.rec("S7", "interrupt_keys") or []
    if not keys:
        return no("no key stops the turn")
    text = run.text("S7.after")
    stopped = line_with(text, r"(interrupted|stopped|cancelled|canceled)")
    if len(keys) == 1 and stopped and steps.S7_LONG_REPLY not in squash(text):
        return yes(f"{keys[0]}: {stopped}")
    if len(keys) != 1:
        return no(f"{len(keys)} keys: {' '.join(keys)}")
    return no(f"{keys[0]} did not stop the turn")


def s7b(run):
    """After the key, the conversation is still in front (not the shell) and
    takes input."""
    front = run.rec("S7", "foreground_after", "")
    if not front or front in SHELLS:
        last = next((l.strip() for l in reversed(run.lines("S7.after")) if l.strip()), "")
        return no(f"the key left the conversation for the shell: {last!r}")
    return yes(f"{front} still in front")


def s7c(run):
    """The survey started before the long turn is still running after the
    key (its process, counted by the driver)."""
    alive = run.rec("S7", "earlier_alive")
    if alive is None:
        return no("the earlier work was never started")
    return bool(alive), ("the earlier survey still runs" if alive else "the earlier survey was stopped too")


# S8 Paste


def s8a(run):
    """The step sent one message, holding the question and the log (the
    session's own record of what was said, read back by the driver)."""
    messages = run.rec("S8", "messages", [])
    if len(messages) == 1 and steps.PROMPTS["S8"] in messages[0] and "FATAL survey aborted" in messages[0]:
        return yes("one message: question and log")
    return no(f"{len(messages)} messages" + (f", the first {messages[0][:50]!r}" if messages else ""))


def s8b(run):
    """Before sending, the input shows the paste folded (`[Pasted text +60
    lines]` or the like) and the question readable, not all 60 lines."""
    name = "S8.typed"
    if not run.has(name):
        return missing(name)
    text = run.text(name)
    fold = line_with(text, r"(past(ed|e)\b.{0,30}\d+\s*(more )?lines|\+\d+ lines|\d+ lines? (pasted|folded|hidden))")
    shown = len(re.findall(r"^\s*\d\d 12:\d\d:07", text, re.M))
    if fold and shown < 10:
        return yes(fold)
    return no(f"{shown} of the log's lines are shown unfolded" if shown else "no fold and no log on screen")


def s8c(run):
    """The message holds the log's 60 lines whole and in order, after the
    question or before it."""
    messages = run.rec("S8", "messages", [])
    log = steps.survey_log()
    joined = "\n".join(messages)
    at = [joined.find(l) for l in log]
    if -1 in at:
        lost = sum(1 for a in at if a == -1)
        return no(f"{lost} of 60 lines lost")
    if at != sorted(at):
        first = next(i for i in range(1, 60) if at[i] < at[i - 1])
        return no(f"reordered: line {first + 1} came before line {first}")
    return yes("all 60 lines, in order")


# S9 Undo


def s9a(run):
    """After the arm's undo, tides.py is back as it was before S9's edit,
    made by the prompt chosen (the driver compares its bytes)."""
    keys = run.rec("S9", "undo_keys")
    if not keys:
        return no("the arm has no undo")
    restored = run.rec("S9", "file_restored", False)
    return bool(restored), ("tides.py restored" if restored else "tides.py still holds the edit")


def s9b(run):
    """After the undo, the conversation is back at the chosen prompt (S9's
    edit's, the latest): it is offered again in the input, and its answer is
    gone from the screen."""
    if not run.rec("S9", "undo_keys"):
        return no("the arm has no undo")
    text = run.text("S9.after")
    prompt = steps.PROMPTS["S9.edit"]
    if prompt in text and "Added the comment" not in text and run.rec("S9", "answer_gone", False):
        return yes(line_with(text, re.escape(prompt)))
    return no("the conversation was not taken back")


def s9c(run):
    """Undoing the last edit took six keys or fewer, and it worked."""
    keys = run.rec("S9", "undo_keys")
    if not keys:
        return no("the arm has no undo")
    ok = len(keys) <= 6 and run.rec("S9", "file_restored", False)
    return ok, f"{len(keys)} keys: {' '.join(keys)}"


# S10 Context and cost


def s10a(run):
    """The context's fill as a share of the window: a percentage, or `N/M`
    tokens, on a line about context."""
    text = run.text("S10.screen")
    line = line_with(text, r"(context|tokens).{0,40}(\d+(\.\d+)?\s?%|\d+(\.\d+)?k?\s?/\s?\d+(\.\d+)?k)|(\d+(\.\d+)?\s?%).{0,30}context")
    if line:
        return yes(line)
    approx = line_with(text, r"~\s?[\d,.]+k?\s*tokens")
    return no(f"only {approx!r}" if approx else "no share of the window")


def s10b(run):
    """What the session has cost, in dollars, on the screen."""
    line = line_with(run.text("S10.screen"), r"\$\s?\d+\.\d{2,}")
    return (line is not None), (line or "no cost shown")


def s10c(run):
    """What fills the context, by part: three or more of the parts (system
    prompt, tools, messages, memory, free space) named with an amount."""
    text = run.text("S10.screen")
    parts = [p for p in (r"system", r"tools?", r"messages?", r"memory", r"free", r"mcp", r"agents?")
             if re.search(rf"{p}.{{0,30}}\d", text, re.I)]
    return (len(parts) >= 3), (f"parts: {', '.join(parts)}" if parts else "no parts named")


# S11 Leave and come back


def s11a(run):
    """The survey started before leaving still runs after the client left
    (its process, counted by the driver)."""
    alive = run.rec("S11", "work_alive")
    if alive is None:
        return no("no work was in flight")
    return bool(alive), ("the survey still runs after leaving" if alive else "leaving stopped the survey")


def s11b(run):
    """A short command (at most two words) reopened this directory's last
    conversation: it is in front and shows one of its messages."""
    cmd = run.rec("S11", "reopen_cmd")
    if not cmd:
        return no("no command reopens the last conversation here")
    if len(cmd.split()) > 2:
        return no(f"`{cmd}` is not short")
    front = run.rec("S11", "reopen_foreground", "")
    seen = any(p in squash(run.text("S11.reopen")) for p in (steps.PROMPTS["S11"], steps.PROMPTS["S8"]))
    if front and front not in SHELLS and seen:
        return yes(f"`{cmd}`")
    return no(f"`{cmd}` did not reopen it")


def s11c(run):
    """The reopened screen shows the transcript: two or more of the earlier
    prompts."""
    if not run.rec("S11", "reopen_cmd"):
        return no("nothing reopened")
    text = squash(run.text("S11.reopen"))
    seen = [k for k in ("S6", "S8", "S11", "S7.long") if steps.PROMPTS[k] in text]
    return (len(seen) >= 2), (f"shows {', '.join(seen)}" if seen else "no transcript")


def s11d(run):
    """A picker of past conversations, searched: after typing a word of S8's
    question, it lists that conversation."""
    cmd = run.rec("S11", "picker_cmd")
    if not cmd:
        return no("no picker of past conversations")
    text = run.text("S11.picker")
    line = line_with(text, r"(these log lines|survey once more|plan it first)")
    return (line is not None and bool(run.rec("S11", "picker_searched", False))), (line or f"`{cmd}` listed nothing found")


# S12 Plan first


def s12a(run):
    """One key (pressed once or more, the same key) enters a planning mode the
    screen names."""
    keys = run.rec("S12", "plan_keys")
    if not keys:
        return no("the arm has no planning mode")
    line = line_with(run.text("S12.mode"), r"plan mode|planning")
    if line and len(set(keys)) == 1:
        return yes(f"{keys[0]}: {line}")
    return no("the screen does not show a planning mode")


def s12b(run):
    """The plan is shown for approval (a question about it) before any file
    changes (the driver compares tides.py's bytes)."""
    if not run.rec("S12", "plan_keys"):
        return no("the arm has no planning mode")
    line = line_with(run.text("S12.plan"), r"(proceed|approve|accept).{0,30}plan|ready to code|would you like to")
    unchanged = run.rec("S12", "unchanged_at_plan", False)
    if line and unchanged:
        return yes(line)
    return no("no plan for approval" if not line else "a file changed before the plan's approval")


def s12c(run):
    """Progress through the plan: a checklist of two or more steps, at least
    one marked done or in progress."""
    if not run.rec("S12", "plan_keys"):
        return no("the arm has no planning mode")
    text = run.text("S12.progress")
    items = re.findall(r"^\s*(?:[⎿└│]\s*)?([☐☒✓✔◼■□▣]|\[[ xX~]\])\s+\S", text, re.M)
    done = [i for i in items if i in ("☒", "✓", "✔", "◼", "■", "▣", "[x]", "[X]", "[~]")]
    if len(items) >= 2 and done:
        return yes(f"{len(items)} steps, {len(done)} done or under way")
    return no("no progress through the plan's steps")


# Beat


def b1(run):
    """The ready time of the arm's conversation (the open command's, else the
    command the driver opened it with), at most 100 ms."""
    ms = run.rec("S1", "ready_ms")
    cmd = run.rec("S1", "open_cmd", "")
    if ms is None:
        ms, cmd = run.rec("S3", "ready_ms"), run.rec("S3", "open_cmd", "")
    if ms is None:
        return no("no ready time")
    return (ms <= 100), f"a prompt after {ms} ms (`{cmd}`)"


def b2(run):
    """Every turn's screen (S2's, S3's, S4's) shows a dollar amount, unasked."""
    names = ["S2.reply", "S3.after", "S4.after"]
    lacking = [n for n in names if not re.search(r"\$\s?\d+\.\d{2,}", run.text(n))]
    if lacking:
        return no(f"no cost on {', '.join(lacking)}")
    return yes(line_with(run.text("S4.after"), r"\$\s?\d+\.\d{2,}"))


def b3(run):
    """The arm's stop of the survey left none of its processes, and the
    conversation says the stop was verified: on the stop's screen, or on the
    next step's (an arm may show the verdict when its next turn takes it)."""
    left = run.rec("S7", "stop_procs_left")
    if left is None:
        return no("the arm was never asked to stop a command")
    rx = r"(verified|confirmed gone|no process(es)? (left|remain))"
    line = line_with(run.text("S7.stop"), rx) or line_with(run.text("S8.after"), rx)
    if left == 0 and line:
        return yes(line)
    return no(f"{left} process(es) left" if left else "gone, but the screen does not say it was verified")


def b4(run):
    """The survey in flight when the agent's own process restarted ran on:
    alive after the restart (the driver)."""
    alive = run.rec("S11", "alive_after_restart")
    if alive is None:
        return no("no restart was tried")
    how = run.rec("S11", "restart_how", "a restart")
    return bool(alive), (f"the survey ran on through {how}" if alive else f"{how} stopped the survey")


def b5(run):
    """The client's largest resident set the driver sampled, under 20 MB (its
    process tree in the pane, not a daemon behind it)."""
    kb = run.rec("B5", "client_rss_kb")
    if not kb:
        return no("the client's memory was not sampled")
    return (kb < 20 * 1024), f"{kb / 1024:.1f} MB ({run.rec('B5', 'client', '')})"


CHECKS = {
    "S1a": s1a, "S1b": s1b, "S1c": s1c, "S1d": s1d,
    "S2a": s2a, "S2b": s2b, "S2c": s2c, "S2d": s2d,
    "S3a": s3a, "S3b": s3b, "S3c": s3c, "S3d": s3d,
    "S4a": s4a, "S4b": s4b, "S4c": s4c, "S4d": s4d,
    "S5a": s5a, "S5b": s5b, "S5c": s5c,
    "S6a": s6a, "S6b": s6b, "S6c": s6c, "S6d": s6d,
    "S7a": s7a, "S7b": s7b, "S7c": s7c,
    "S8a": s8a, "S8b": s8b, "S8c": s8c,
    "S9a": s9a, "S9b": s9b, "S9c": s9c,
    "S10a": s10a, "S10b": s10b, "S10c": s10c,
    "S11a": s11a, "S11b": s11b, "S11c": s11c, "S11d": s11d,
    "S12a": s12a, "S12b": s12b, "S12c": s12c,
    "B1": b1, "B2": b2, "B3": b3, "B4": b4, "B5": b5,
}


def judge(run):
    """Every check on `run`: {id: (answer, evidence)}, in scorecard order. A
    check that raises answers no, with the error as its evidence."""
    out = {}
    for cid in steps.check_ids():
        try:
            ok, evidence = CHECKS[cid](run)
        except Exception as e:  # a check's bug is a no with its reason, never a crash of the scorecard
            ok, evidence = False, f"check error: {type(e).__name__}: {e}"
        out[cid] = (bool(ok), " ".join(str(evidence).split())[:160])
    return out
