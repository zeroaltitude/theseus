"""F10's twelve steps and five beat checks: their ids, weights and words, the
prompts both arms are given, and the scripted stand-in's answers for the
Theseus arm (theseus-qy2a).

The ids are the scorecard's and every later report's: never renumber them.
"""

# Each step: its id, its weight (H high, M medium), what it is, and its checks
# as (id, words). The words are the table in docs/benchmarks/f10/README.md.
STEPS = [
    ("S1", "H", "Open", [
        ("a", "one command, no arguments, opens a conversation in this directory"),
        ("b", "a prompt to type into appears in 2 s or less"),
        ("c", "the first screen names the model and the directory"),
        ("d", "it says in one line how to get help or how to use it"),
    ]),
    ("S2", "H", "Ask \"what does this project do?\"", [
        ("a", "the reply's markdown is rendered (headings, emphasis, code)"),
        ("b", "each tool call names what it touched (path or command)"),
        ("c", "something moves while it waits (spinner or elapsed time)"),
        ("d", "lines wrap at word boundaries at 80 columns"),
    ]),
    ("S3", "H", "Run the tests", [
        ("a", "the command is readable before approving"),
        ("b", "approving takes one key"),
        ("c", "what failed can be seen without leaving the conversation"),
        ("d", "no id has to be copied"),
    ]),
    ("S4", "H", "Fix, with a reviewed edit", [
        ("a", "the edit's diff shows before approving"),
        ("b", "it is coloured, with the changed words marked"),
        ("c", "the diff shows in the conversation afterwards"),
        ("d", "the file and the line numbers show"),
    ]),
    ("S5", "M", "Don't ask again", [
        ("a", "the prompt offers \"this command, for the rest of the session or project\""),
        ("b", "that grant covers the command's prefix, not the whole tool"),
        ("c", "one key switches to accepting file edits for the session"),
    ]),
    ("S6", "M", "A 90-second command", [
        ("a", "elapsed time shows while it runs"),
        ("b", "its output so far can be seen"),
        ("c", "you can keep talking while it runs"),
        ("d", "its end is announced and the agent goes on by itself"),
    ]),
    ("S7", "H", "Interrupt", [
        ("a", "one key stops the turn"),
        ("b", "the conversation stays open and asks what to do instead"),
        ("c", "work started earlier keeps running"),
    ]),
    ("S8", "H", "Paste a 60-line log with a question", [
        ("a", "paste and question are one message"),
        ("b", "the paste is folded so the prompt stays readable"),
        ("c", "nothing is lost or reordered"),
    ]),
    ("S9", "H", "Undo", [
        ("a", "the agent's edits since a chosen prompt can be reverted"),
        ("b", "the conversation can be taken back to that prompt"),
        ("c", "undoing the last edit takes 6 keys or fewer"),
    ]),
    ("S10", "M", "Context and cost", [
        ("a", "how full the context is, as a share of the window"),
        ("b", "what the session has cost"),
        ("c", "what fills the context, by part"),
    ]),
    ("S11", "H", "Leave and come back", [
        ("a", "leaving never stops work in flight"),
        ("b", "one short command reopens the last conversation in this directory"),
        ("c", "reopening shows the transcript"),
        ("d", "past conversations can be searched and picked"),
    ]),
    ("S12", "M", "Plan first", [
        ("a", "a read-only planning mode one key away"),
        ("b", "the plan shows for approval before any edit"),
        ("c", "progress through the plan's steps shows"),
    ]),
]

BEAT = ("Beat", "-", "Beat", [
    ("B1", "a ready prompt 100 ms or less after the command"),
    ("B2", "every turn's cost shows without asking"),
    ("B3", "a stopped command is verified gone (its processes)"),
    ("B4", "work in flight survives a restart of the agent's own process"),
    ("B5", "the client stays under 20 MB resident"),
])


def check_ids():
    """Every check's id in scorecard order: the 42 match checks, then B1 to B5."""
    ids = [f"{s}{c}" for s, _, _, checks in STEPS for c, _ in checks]
    return ids + [b for b, _ in BEAT[3]]


MATCH_CHECKS = 42
BEAT_CHECKS = 5

# What both arms are told, step by step.
PROMPTS = {
    "S2": "what does this project do?",
    "S3": "run the tests: python3 -m unittest -v test_tides",
    "S4": "fix the failing test",
    "S5": "run the tests again: python3 -m unittest -v test_tides",
    "S6": "run the slow survey",
    "S6.talk": "while that runs: which reading is the low water?",
    "S7.start": "start the survey again, in the background",
    "S7.long": "explain how tides are predicted, at length",
    "S8": "what do these log lines say went wrong?",
    "S9.edit": "add a comment line at the top of tides.py",
    "S11": "start the survey once more, in the background",
    "S12": "add a function for the mean height; plan it first",
}

# The bug the project carries, and its fix: S4's edit.
BUG_OLD = "    return round(lowest - highest, 2)"
BUG_NEW = "    return round(highest - lowest, 2)"
BUG_LINE = 50  # tides.py's line that holds it

# S9's edit, the one undone: a comment above the module's docstring.
S9_OLD = '"""A small tide-table library'
S9_NEW = '# Port Wenlow\n"""A small tide-table library'

# The tests' command, and the slow survey's.
TEST_ARGV = ["python3", "-m", "unittest", "-v", "test_tides"]
SURVEY_ARGV = ["python3", "slow_survey.py"]

# S2's reply: markdown long enough to wrap at 120 columns and at 80, whose
# words name no file, so a file's name on the screen is a tool call's.
S2_REPLY = (
    "# Port Wenlow tides\n\n"
    "A **small library** for an invented harbour's tide table. It reads `HH:MM height` lines, finds "
    "the high and low waters between their neighbours, and works out the tidal range between the "
    "highest and the lowest water of the day, which the harbour board shows in metres.\n\n"
    "## Its checks\n\n"
    "- one *unit test* for each function, against a single invented day of readings\n"
    "- a slow survey of thirty stations that prints its progress as it goes"
)
S2_WORDS = "invented harbour"  # a phrase of the reply, found whatever the rendering

S6_TALK_REPLY = "The low water is the 12:00 reading, at 0.60 m."
S7_LONG_REPLY = "Tides are predicted by summing harmonic constituents."
S8_REPLY = "The log says the gauge at station 17 stopped answering, then the survey gave up."
S12_REPLY = "Plan: 1. add mean_height to tides.py 2. test it 3. run the tests."


def survey_log(lines=60):
    """S8's paste: a 60-line log, each line numbered so an order can be read
    back."""
    out = []
    for n in range(1, lines + 1):
        if n == 41:
            out.append(f"{n:02d} 12:{n:02d}:07 ERROR station 17: gauge did not answer (timeout 5 s)")
        elif n == 60:
            out.append(f"{n:02d} 12:{n:02d}:07 FATAL survey aborted after 3 failed stations")
        else:
            out.append(f"{n:02d} 12:{n:02d}:07 INFO station {n % 30 + 1}: read {1 + n % 7 * 0.3:.2f} m")
    return out


def stand_in_rules(project):
    """The Theseus arm's scripted model (`theseus-sim fake-model --rules`):
    each step's prompt asks for its calls and answers its `then`, so the arm
    costs nothing and every run sees the same model. `project` is the scratch
    copy's absolute path."""
    tides = f"{project}/tides.py"
    survey = {"argv": SURVEY_ARGV, "cwd": project, "timeout_secs": 600}
    return [
        # The survey's late result, when the arm hands it to the model as
        # text: the agent goes on by itself.
        {"when": "survey complete: 30 stations", "text": "The survey finished: all 30 stations read."},
        {"when": PROMPTS["S2"], "hold_ms": 2500, "calls": [
            {"name": "fs_read", "input": {"path": f"{project}/README.md"}},
            {"name": "fs_read", "input": {"path": tides}},
        ], "then": S2_REPLY},
        {"when": PROMPTS["S5"], "calls": [
            {"name": "proc_run", "input": {"argv": TEST_ARGV, "cwd": project}},
        ], "then": "The tests pass now."},
        {"when": PROMPTS["S3"], "calls": [
            {"name": "proc_run", "input": {"argv": TEST_ARGV, "cwd": project}},
        ], "then": "The tests ran."},
        {"when": PROMPTS["S4"], "calls": [
            {"name": "fs_edit", "input": {"path": tides, "old_string": BUG_OLD, "new_string": BUG_NEW}},
        ], "then": "Fixed: the range is the highest less the lowest."},
        {"when": PROMPTS["S6.talk"], "text": S6_TALK_REPLY},
        {"when": PROMPTS["S6"], "calls": [{"name": "proc_run", "input": survey}],
         "then": "The survey is under way."},
        {"when": PROMPTS["S7.start"], "calls": [{"name": "proc_run", "input": survey}],
         "then": "The survey is running in the background."},
        # S8's before S7's long one: an arm may send S7's stopped question
        # again with S8's message, and S8's must answer it.
        {"when": PROMPTS["S8"], "text": S8_REPLY},
        {"when": PROMPTS["S7.long"], "hold_ms": 30000, "text": S7_LONG_REPLY},
        {"when": PROMPTS["S9.edit"], "calls": [
            {"name": "fs_edit", "input": {"path": tides, "old_string": S9_OLD, "new_string": S9_NEW}},
        ], "then": "Added the comment."},
        {"when": PROMPTS["S11"], "calls": [{"name": "proc_run", "input": survey}],
         "then": "The survey is running in the background."},
        {"when": PROMPTS["S12"], "text": S12_REPLY},
    ]
