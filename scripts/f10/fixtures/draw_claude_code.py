"""Draws the Claude Code fixture (theseus-qy2a): screens written from the
check definitions, as Claude Code 2.1 draws them, passing all 42 checks and
failing the five beat checks, as the baseline of 2026-10-08 found it. Not a
recording: the maintainer's first local run of the arm replaces it, and the
tests then hold the checks to real screens.

    python3 scripts/f10/fixtures/draw_claude_code.py

writes scripts/f10/fixtures/claude-code-120x40/.
"""

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import steps  # noqa: E402

E = "\x1b["
RESET = E + "0m"


def bold(t):
    return f"{E}1m{t}{E}22m"


def italic(t):
    return f"{E}3m{t}{E}23m"


def fg(code, t):
    return f"{E}38;5;{code}m{t}{E}39m"


def bg(code, t):
    return f"{E}48;5;{code}m{t}{E}49m"


DOT = fg(2, "●")
PROJECT = "/tmp/f10-claude-code-120x40-x1y2z3/project"
BOX = "╭" + "─" * 118 + "╮"
BOX_END = "╰" + "─" * 118 + "╯"


def input_box(text="", mode=None):
    lines = [BOX, "│ > " + text.ljust(115) + "│", BOX_END]
    lines.append("  " + (mode or fg(244, "? for shortcuts")))
    return lines


def screen(*parts):
    lines = []
    for p in parts:
        lines += p if isinstance(p, list) else p.split("\n")
    return "\n".join(lines[:40]) + "\n"


WELCOME = [
    fg(209, "╭───────────────────────────────────────────────────╮"),
    fg(209, "│") + " " + fg(209, "✻") + " Welcome to " + bold("Claude Code") + "!" + " " * 25 + fg(209, "│"),
    fg(209, "│") + " " * 51 + fg(209, "│"),
    fg(209, "│") + "   " + italic("/help for help, /status for your current setup") + "  " + fg(209, "│"),
    fg(209, "│") + " " * 51 + fg(209, "│"),
    fg(209, "│") + "   cwd: " + PROJECT + fg(209, "│"),
    fg(209, "╰───────────────────────────────────────────────────╯"),
    "",
    "  " + fg(244, "Opus 5.5 · Claude Max"),
    "",
]

S2_REPLY = [
    "> " + steps.PROMPTS["S2"],
    "",
    DOT + " " + bold("Read") + "(README.md)",
    "  ⎿  Read 9 lines",
    "",
    DOT + " " + bold("Read") + "(tides.py)",
    "  ⎿  Read 56 lines",
    "",
    DOT + " This is " + bold("tides") + ", a small tide-table library for an invented harbour, Port Wenlow. It reads a table of",
    "  " + fg(75, "HH:MM height") + " lines, finds the high and low waters by comparing each reading with its neighbours, and",
    "  works out the tidal range between the highest and the lowest water of the day.",
    "",
    "  " + bold("Files"),
    "  - " + fg(75, "tides.py") + " the library: " + fg(75, "parse_table") + ", " + fg(75, "high_waters") + ", " + fg(75, "low_waters") + ", " + fg(75, "tidal_range"),
    "  - " + fg(75, "test_tides.py") + " its unit tests, and " + fg(75, "slow_survey.py") + ", a 90-second survey script",
    "",
]


def ask(title, body, options):
    lines = [BOX, "│ " + bold(title).ljust(125) + "│", "│" + " " * 118 + "│"]
    lines += ["│   " + b + "│" for b in body]
    lines += ["│" + " " * 118 + "│", "│ Do you want to proceed?" + " " * 94 + "│"]
    for i, o in enumerate(options):
        mark = fg(75, "❯") if i == 0 else " "
        lines.append(f"│ {mark} {i + 1}. {o}")
    lines.append(BOX_END)
    return lines


TEST_OUT = [
    DOT + " " + bold("Bash") + "(python3 -m unittest -v test_tides)",
    "  ⎿  Error: test_format_height (test_tides.TidesTest.test_format_height) ... ok",
    "     test_tidal_range (test_tides.TidesTest.test_tidal_range) ... FAIL",
    "",
    "     ======================================================================",
    "     " + fg(1, "FAIL: test_tidal_range (test_tides.TidesTest.test_tidal_range)"),
    "     AssertionError: -2.8 != 2.8",
    "     … +6 lines (ctrl+r to expand)",
    "",
    DOT + " One test fails: the range comes out negative.",
    "",
]

OLD = steps.BUG_OLD
NEW = steps.BUG_NEW


def diff(indent):
    n = steps.BUG_LINE
    old = indent + f"{n} " + bg(52, "-" + OLD.replace("lowest - highest", bg(88, "lowest - highest")))
    new = indent + f"{n} " + bg(22, "+" + NEW.replace("highest - lowest", bg(28, "highest - lowest")))
    return [
        indent + f"{n - 2}        highest = max(r.height_m for r in readings)",
        indent + f"{n - 1}        lowest = min(r.height_m for r in readings)",
        old,
        new,
    ]


def main():
    out = HERE / "claude-code-120x40"
    (out / "screens").mkdir(parents=True, exist_ok=True)
    s = {}
    s["S1.open"] = screen(WELCOME, input_box())
    s["S2.wait1"] = screen(WELCOME, "> " + steps.PROMPTS["S2"], "", fg(209, "✻") + " Pondering… " + fg(244, "(1s · esc to interrupt)"), "", input_box())
    s["S2.wait2"] = screen(WELCOME, "> " + steps.PROMPTS["S2"], "", fg(209, "✽") + " Pondering… " + fg(244, "(2s · ↓ 112 tokens · esc to interrupt)"), "", input_box())
    s["S2.reply"] = screen(S2_REPLY, input_box())
    s["S3.ask"] = screen(S2_REPLY[-4:], "> " + steps.PROMPTS["S3"], "", ask("Bash command", ["python3 -m unittest -v test_tides", fg(244, "Run the tide library's tests")], [
        "Yes", "Yes, and don't ask again for " + bold("python3 -m unittest") + " commands in " + PROJECT, "No, and tell Claude what to do differently (esc)"]))
    s["S3.after"] = screen("> " + steps.PROMPTS["S3"], "", TEST_OUT, input_box())
    s["S4.ask"] = screen("> " + steps.PROMPTS["S4"], "", ask("Edit file", ["tides.py", ""] + diff(""), [
        "Yes", "Yes, allow all edits during this session (shift+tab)", "No, and tell Claude what to do differently (esc)"]))
    s["S4.after"] = screen("> " + steps.PROMPTS["S4"], "", DOT + " " + bold("Update") + "(tides.py)",
                           "  ⎿  Updated " + bold("tides.py") + " with 1 addition and 1 removal", diff("     "), "",
                           DOT + " Fixed: the range is now the highest water less the lowest.", "", input_box())
    s["S5.ask"] = screen("> " + steps.PROMPTS["S5"], "", ask("Bash command", ["python3 -m unittest -v test_tides", fg(244, "Run the tests again")], [
        "Yes", "Yes, and don't ask again for " + bold("python3 -m unittest") + " commands in " + PROJECT, "No, and tell Claude what to do differently (esc)"]))
    s["S5.mode"] = screen(DOT + " All 5 tests pass.", "", input_box(mode=fg(141, "⏵⏵ accept edits on") + fg(244, " (shift+tab to cycle)")))
    run = [DOT + " " + bold("Bash") + "(python3 slow_survey.py)"]
    s["S6.run1"] = screen("> " + steps.PROMPTS["S6"], "", run, "  ⎿  survey: station 1/30 read", "     " + fg(244, "Running… (5s · timeout 10m)"), "", input_box())
    s["S6.run2"] = screen("> " + steps.PROMPTS["S6"], "", run, "  ⎿  survey: station 1/30 read", "     survey: station 2/30 read",
                          "     " + fg(244, "Running… (8s · timeout 10m)"), "", input_box())
    s["S6.talk"] = screen(run, "  ⎿  survey: station 5/30 read", "     " + fg(244, "Running in the background (↓ to manage)"), "",
                          "> " + steps.PROMPTS["S6.talk"], "", DOT + " The low water is the 12:00 reading, at 0.60 m.", "", input_box())
    s["S6.end"] = screen(run, "  ⎿  survey: station 30/30 read", "     survey complete: 30 stations", "",
                         DOT + " The survey has finished: all 30 stations were read without a fault.", "", input_box())
    s["S7.busy"] = screen("> " + steps.PROMPTS["S7.long"], "", fg(209, "✻") + " Musing… " + fg(244, "(3s · esc to interrupt)"), "", input_box())
    s["S7.after"] = screen("> " + steps.PROMPTS["S7.long"], "  ⎿  " + fg(1, "Interrupted by user"), "", input_box())
    s["S7.stop"] = screen("> stop the background survey", "", DOT + " " + bold("Kill Shell") + "(bash_2)", "  ⎿  Shell bash_2 killed", "", input_box())
    paste_box = [BOX, "│ > " + fg(244, "[Pasted text #1 +60 lines]") + " " + steps.PROMPTS["S8"], BOX_END]
    s["S8.typed"] = screen(DOT + " The survey has stopped.", "", paste_box)
    s["S8.after"] = screen("> [Pasted text #1 +60 lines] " + steps.PROMPTS["S8"], "", DOT + " Station 17's gauge stopped answering at line 41, and the survey gave up at line 60.", "", input_box())
    s["S9.menu"] = screen(BOX, "│ " + bold("Rewind"), "│ Restore the code and/or conversation to the point before…", "│", "│   " + steps.PROMPTS["S8"],
                          "│ " + fg(75, "❯ ") + steps.PROMPTS["S9.edit"], BOX_END)
    s["S9.after"] = screen(DOT + " Station 17's gauge stopped answering at line 41, and the survey gave up at line 60.", "",
                           [BOX, "│ > " + steps.PROMPTS["S9.edit"], BOX_END], "  " + fg(244, "Restored the code and the conversation"))
    s["S10.screen"] = screen("> /context", "  ⎿  " + bold("Context Usage"),
                             "     ⛁ ⛀ ⛁ ⛁   claude-opus-5-5 · 31k/200k tokens (16%)",
                             "     ⛁ System prompt: 3.1k tokens (1.6%)",
                             "     ⛁ System tools: 12.4k tokens (6.2%)",
                             "     ⛁ Messages: 15.5k tokens (7.8%)",
                             "     ⛶ Free space: 169k (84.5%)", "",
                             "> /cost", "  ⎿  Total cost:            $0.48", "     Total duration (API):  1m 52s", "", input_box())
    s["S11.left"] = screen(DOT + " The survey is running in the background.", "", "$ ")
    s["S11.reopen"] = screen(WELCOME, "> " + steps.PROMPTS["S8"], "", DOT + " Station 17's gauge stopped answering.", "",
                             "> " + steps.PROMPTS["S11"], "", DOT + " The survey is running in the background.", "", input_box())
    s["S11.picker"] = screen(bold("Resume Session"), "Search: log lines", "",
                             fg(75, "❯ ") + "what do these log lines say went wrong?  · 23 messages · 4 minutes ago")
    s["S12.mode"] = screen(input_box(mode=fg(37, "⏸ plan mode on") + fg(244, " (shift+tab to cycle)")))
    s["S12.plan"] = screen(DOT + " " + bold("Plan"), "  1. Add mean_height(readings) to tides.py", "  2. Test it in test_tides.py",
                           "  3. Run the tests", "", "Would you like to proceed?", fg(75, "❯ ") + "1. Yes, and auto-accept edits", "  2. Yes, and manually approve edits",
                           "  3. No, keep planning")
    s["S12.progress"] = screen(DOT + " " + bold("Update Todos"), "  ⎿  " + fg(2, "☒") + " Add mean_height to tides.py",
                               "     ☐ Test it in test_tides.py", "     ☐ Run the tests")
    for name, text in s.items():
        (out / "screens" / f"{name}.ansi").write_text(text)
    log = "\n".join(steps.survey_log())
    records = {
        "S1": {"open_cmd": "claude", "foreground": "node", "ready_ms": 763},
        "S3": {"approve_keys": ["Enter"], "ran": True, "ids_typed": 0},
        "S5": {"mode_keys": ["BTab"]},
        "S6": {"ended_before_talk": False, "went_on": True},
        "S7": {"interrupt_keys": ["Escape"], "foreground_after": "node", "earlier_alive": True, "stop_procs_left": 0},
        "S8": {"messages": [log + "\n" + steps.PROMPTS["S8"]]},
        "S9": {"undo_keys": ["Escape", "Escape", "Enter", "Enter"], "file_restored": True, "answer_gone": True},
        "S11": {"work_alive": True, "alive_after_restart": False, "restart_how": "claude's exit",
                "reopen_cmd": "claude -c", "reopen_foreground": "node", "picker_cmd": "claude --resume", "picker_searched": True},
        "S12": {"plan_keys": ["BTab", "BTab"], "unchanged_at_plan": True},
        "B5": {"client_rss_kb": 272384, "client": "node (pid 4242)"},
    }
    meta = {"arm": "claude-code", "size": "120x40", "project_dir": PROJECT, "model": "claude-opus-5-5",
            "version": "2.1.296 (Claude Code), drawn", "records": records}
    (out / "run.json").write_text(json.dumps(meta, indent=1) + "\n")


if __name__ == "__main__":
    main()
