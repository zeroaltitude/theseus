# Theseus's north star: the plan

*2026-10-08, night · Tabitha/Claude, from tonight's six investigations and the evening's two plans · "coding §3"
names a report and its section; the list is at the end. The reports it cites are the working investigations,
kept outside this repository.*

## The answer first

**The goal, in one line:** match or beat Claude Code at coding and be as pleasant to use, then crush it on memory,
personality, speed, scale and task management at every horizon (the owner, 2026-10-08 19:55).

| Dimension | The target, against Claude Code on the same model | Today | Verdict |
|---|---|---|---|
| **Coding** | Terminal-Bench 2.0 and a SWE-bench Verified 100 within 5 points at 95%; 5 real-repo checks | 71.9% against 81.5% (Oct 4); **−3.0 points** without the since-fixed faults, but hard tasks still 7 to 1; real-repo checks **0 of 5** (project files, directory, date, OS, git), long commands **0 of 4** | 🔴 **behind** |
| **Delight** | All 42 first-ten-minute checks Claude Code passes (beat: 5 more); the owner's own trial | **8 of 42**; 4 of the 5 beat checks; ready in 2 to 36 ms against 763 ms, at 3 to 7 MB against 266 MB | 🔴 **behind** |
| **Memory** | Recall above Claude Code's and Pi's; a preference or order kept in all 6 contexts; citations ≥ 90% right | the exam: **15% → 67%** with recall, never run head to head; a stated preference lost in a new session (Claude Code kept it, in the same directory only) | ⚪ **unmeasured** |
| **Personality** | Identity ≥ 95% consistent on every surface and model; ≥ 60% of the owner's blind pairs | **three fixed sentences**; a tie on candour, behind on reading the moment; one 1 KB persona file passed every probe (+395 cached tokens, no extra call) | 🔴 **behind**, fix ready |
| **Speed** | ≥ 2x at the median on every row a person feels, never worse at p95 | **~50x** to a prompt, **100x** to the first text, 25x less CPU, 7x less memory; loses each tool call (34 against 23 ms) and waits 278 ms before a model call on the owner's daemon | 🟢 **ahead**, 6 of 9 |
| **Scale** | 10x today's load, every row inside its budget | flat start at 200,000 sessions; 80 turns at once in 121 MB; but **dies at 1,017 connections**, and a 10,000-node session's first turn takes **30.8 s** | 🟢 **ahead**; 🔴 **0 of 7** |
| **Task horizons** | Now: the task plan's 6 tests. Later: 11, from "it survives a restart" to "projects" | wakes survive restarts (due wakes ran 0.35 s after serving; Claude Code's "remind me" died with its process); **a reminder set from the CLI reaches no one** | 🟢 **durable**; 🔴 **3 of 11** |
| **Discord** | A lively chat with thinking; Theseus speaks first on 15 events; a task house in the guild | tool lines stream, but **thinking never reaches Discord**; Theseus speaks first only sometimes, in a system's words; Claude Code has no Discord at all | 🟢 **ahead**; half built |

*Sources: coding §2-3; delight §2-3; killer §2 and personality §3.5; personality §2-3; scale §2-3; horizon §2-3 and
plan §1.1; discord2 §2-4. Section 1 details each row.*

<div class="two">

**The six moves that matter most**

1. **Work where the owner works** (coding, delight). One shared pair of rows: the session gets the directory it was
   started in, the repo's AGENTS.md and CLAUDE.md, and the date, OS and git state. *Why:* it is the biggest coding gap
   a benchmark can't see (0 of 5 today), the delight gap G8, and what daily use in a repo needs. The first row can
   start on main now.
2. **A real conversation in the terminal** (delight). First tonight's two P1 defects (pasted lines stored out of
   order; a paste that quits the TUI or approves a card), then one renderer for markdown and diffs, tool lines in
   words, and bare `theseus` as an inline chat with one-key answers, Esc to stop, and rewind. *Why:* 8 of 42 is the
   widest gap measured tonight, and the engine under it already wins. Twelve rows; six can start now.
3. **Keep long work in hand** (coding). Job handles (read, wait, stop, and a notice in the same turn, with blocking
   kept as the default), no output lost in the middle, a long instruction that compacts inside its own turn, helpers
   that answer in the same turn. *Why:* real work is long. Today a job past 60 s answers in a later turn, and one long
   instruction fails at about 868K tokens. Theseus's blocking wait already beats Claude Code's sleep-polling.
4. **Know the owner for sure, not by similarity** (memory, personality). The persona goes on tonight (config only);
   then standing orders, preferences and facts in every prompt, provable from the manifest; replies that cite the
   message they remember; recall inside its 250 ms deadline; the first head-to-head exams. *Why:* the cheapest
   "crush" in the plan. The mechanism exists, and Claude Code's memory is a file per directory that cites nothing.
5. **Tasks on every horizon, and Theseus speaks first** (task horizons, Discord). The work board; reminders that
   always reach the owner; deferred tasks; routines with their own budget and pre-approved tools; watches that cost
   $0 of model; the model's own view of what it promised. In Discord: thinking in the chat, 15 events spoken in
   Theseus's own voice, a task house in the guild. *Why:* the durable timer exists and the experience doesn't (3 of
   11). OpenClaw's own Morning Briefing has failed 6 runs in a row without a word (section 6).
6. **Take the disk off the critical path, and fail soft at 10x** (speed, scale). A sync budget (one sync before the
   first byte, one per read-only tool loop), recall inside its deadline, fair admission, paged lists, a daemon that
   survives 10,000 connections, long sessions in O(new). *Why:* every speed loss is a wait: 85% to 90% of the
   harness's own time on a turn is `fdatasync`, and on tmpfs a tool call is 5x faster than Claude Code's. Every 10x
   failure is a cliff.

Each move carries its yardstick, run before and after (section 3): the Terminal-Bench reruns R0 and R1 (about $160),
the first-ten-minutes walkthrough, the recall bench, the personality exam, the head-to-head speed bench, the horizon
acceptance run. **Benchmarks improve, never max:** no row adds an instruction to the prompt or tunes for a task.

**What the owner must decide this week**

1. **The measurement budget:** the reruns R0 and R1 now (about $160) and the first exams (about $55 to $130); the
   SWE-bench and Opus runs (about $280 to $630) only after R0. *Unblocks every head-to-head verdict.*
2. **Whose persona:** seeded from the current assistant's values and voice, under Theseus's own name; the owner reads
   the file before it goes live. *Unblocks the persona tonight, and the words Theseus speaks first.*
3. **Discord:** re-authorize the bot once (12 more permissions, never Administrator) and let `/setup` build the
   house in the owner's guild. *Unblocks the task house: board, task forum, journal, ledger, health.*
4. **Project files:** read AGENTS.md and CLAUDE.md (both when both exist), and trust each new repository once.
   *Unblocks `project-context`: coding's 0 of 5.*
5. **How far the gate may loosen:** "don't ask again" for a conversation, modes, routines' pre-approved tools, each
   with a security review. *Unblocks grants, modes, routines and watches.*
6. **Bare `theseus` opens a conversation** on a terminal; scripts keep today's usage text. *Unblocks the chat.*
7. **Commands inherit the owner's environment,** minus every secret, once the scrubber knows more key shapes.
   *Unblocks venvs, nvm and `JAVA_HOME` in `proc_run`.*

Section 4 gives each in full, and 60 more defaults that stand unless the owner objects.

</div>

---

# 1. The dimensions, one by one

Each dimension below gives its target (a number, or a test that passes or fails against Claude Code on the same
model), today's evidence, the gaps ranked by what they cost real use, and the rows that close them. Row names in
`code` are the rows of the merged plan in section 2. Each section's Beads issue is in its heading.

## 1.1 Coding (theseus-7m7w)

**The target** (coding §2). Ten tests, each run with the same model, effort, output cap, budget and wall clock.
**Match** = T1 and T3 non-inferior and T4 to T8 pass. **Beat** = T1 or T3 above zero, or the owner's own coding
sessions better (T10).

| Test | Pass line (match) | Today (coding §3) |
|---|---|---|
| T1 Terminal-Bench 2.0, all 89 tasks, 5 attempts | 95% lower bound of Theseus − Claude Code ≥ −5 points | Oct 4: 71.9% against 81.5% (Claude Code better on 16 tasks, Theseus on 5). Without the 7 tasks a since-fixed fault touched: **−3.0 points**, exactly the attempt noise |
| T2 its 30 hard tasks | point estimate ≥ −3 points | 7 tasks to 1 after the fixed faults; no harness cause found in any of them |
| T3 SWE-bench Verified, a fixed stratified 100, 3 attempts | lower bound ≥ −5 points | never run; the 500 task directories are on this machine |
| T4 tool friction | 0 malformed inputs; invented names no more than Claude Code's | trials with a tool-level error **24% against 5%**; malformed inputs 21 against 0; invented tool or field names 13 against 2 |
| T5 cost and calls per solve | dollars ≤ Claude Code's; calls ≤ 1.1x | $0.110 against $0.097, but Oct 4 ran Theseus at a higher effort (matched since Oct 7) |
| T6 a long single instruction finishes | it compacts inside its turn and finishes | fails at about 868K tokens, by construction (`compiler.rs:808-822`) |
| T7 the model sees the project | 5 of 5: project files, directory, date, OS, git state | **0 of 5** |
| T8 long commands stay in hand | 4 of 4: start, read, wait, stop, all in the turn | **0 of 4** |
| T9 ten of our own past rows, replayed blind | ≥ Claude Code on held-back tests and review score | not run |
| T10 the owner's own coding sessions, ≥ 20 over 2 to 3 weeks | corrections and false "done" ≤ Claude Code's | not run |

**Ahead already** (coding §4, §5): the blocking wait for long commands (in 3 of Theseus's 5 Oct 4 wins, a Claude Code
trial sleep-polled out the clock: 12 trials asked for 12,501 s of `sleep`), edit results as diffs, error wording that
says how to recover, big files read in windows, bounded search, and one patch across many files. Two more advantages
exist and are switched off: diagnostics after an edit (LSP) and the early-stop detector.

**Every loss now has a cause** (coding §4): 3 refusals (fixed), 7 harness faults (fixed; the last, a terminal's close
killing its programs, joined tonight with `terminals`), 4 grader flaws, 9 model choices or literal slips, and 2 time
limits the model's own strategy set. Claude Code made the same kinds of mistake in its other attempts (sign test 10 to
5, p = 0.30). None calls for a prompt change. This meets v1's fourth condition on paper: each b5 loss explained or
fixed.

**Gaps, ranked by what they cost real work** (coding §5, "costs real-world success?"):

| # | Gap | Cost | Rows |
|---|---|---|---|
| 1 | No project instructions, no working directory (D2, D3) | high | `session-dir`, `project-context` |
| 2 | One long instruction never compacts (D23) | high | `one-turn-compaction` |
| 3 | Commands see only 9 environment variables: no venv, nvm or `JAVA_HOME` (D7) | medium-high | `proc-env` (decision B7) |
| 4 | A command past 60 s goes dark: no handle, its result in a later turn (D8, D26) | medium | `job-handles` |
| 5 | Long output loses its middle; a 30 to 100 KB file read has a hole (D9, D15) | medium | `output-kept` |
| 6 | A helper's report arrives only at the next turn (D27) | medium | `subagent-wait` |
| 7 | Tool friction: invalid JSON, `bash` tools that don't exist, shell quoting in arrays (D6, D13) | low | `tool-input-fit` |
| 8 | CRLF files, stale edits, hidden files out of search (D17, D18, D20) | low | `file-tools-polish` |
| 9 | Early stops ("next I'll run the tests", then nothing) in one-shot runs (D33) | medium | spine row 26b, widened (coding D8) |
| 10 | No diagnostics after an edit by default (D34) | likely gain | LSP on by default, after run R2's A/B (coding D7) |

Not planned, on purpose: coding-method text in the prompt. Coding's decision D1 gives the model facts (project files,
environment), not method, because the trajectories show no habit gap: every arm ran a check after its last edit 91% to
100% of the time.

## 1.2 Delight (theseus-wy6v)

**The target** (delight §2).
- **F10, the first ten minutes:** the same 12 steps for both tools on the same scratch project, driven by a script, 42
  pass/fail checks. **Match** = 42 of 42. **Beat** = also B1 to B5: a prompt in 100 ms or less, every turn's cost
  shown, a stop verified gone, work that survives the agent's own restart, a client under 20 MB.
- **FAST budgets for the chat** (a bench row): launch to a prompt ≤ 100 ms p90; keystroke to echo ≤ 16 ms p99; an
  approval key to the tool starting ≤ 50 ms; rendering ≤ 2 ms per 100 lines; client ≤ 20 MB; a plain turn still 5
  store frames.
- **The owner's 20-minute trial:** at least Claude Code's score on six statements ("I always knew what it was doing",
  "I trusted what it was about to change", …), and 4 of 5 or more on "I'd reach for it first".

**Today** (delight §3.5): Claude Code 42 of 42 and 0 of the 5 beat checks; Theseus **8 of 42** and 4 of 5.

| Step | Theseus | What fails |
|---|---|---|
| S1 Open | 1 of 4 | no command opens a conversation: bare `theseus` errors, `watch -i` refuses a fresh daemon |
| S2 Ask | 0 of 4 | raw markdown, no colour, tool names only, nothing moves while it works, words cut at 80 columns |
| S3 Run the tests | 2 of 4 | the failing output is never shown |
| S4 Fix, with a reviewed edit | 0 of 4 | no diff before or after; the edit's JSON cut at 200 characters |
| S5 Don't ask again | 0 of 3 | every gated call asks, every time |
| S6 A 90-second command | 2 of 4 | it backgrounds and wakes the conversation, but shows no elapsed time or output |
| S7 Interrupt | 1 of 3 | Ctrl-C only detaches; stopping takes `theseus stop <id>` in another shell |
| S8 Paste 60 lines | 0 of 3 | one message per line, **stored out of order**; the TUI quit on a `q` in the paste |
| S9 Undo | 0 of 3 | no rewind of files or conversation |
| S10 Context and cost | 1 of 3 | cost on every turn, but context only as "~N tokens" |
| S11 Leave and come back | 1 of 4 | work keeps going, but no "continue here", no transcript, no picker |
| S12 Plan first | 0 of 3 | no plan mode |

**Ahead already** (delight §3.4, §3.6, §5.5): clients ready 20 to 350 times sooner, at 3 to 7 MB against 266 MB; work
outlives the terminal and the daemon's own restart; long jobs wake their conversation; stops are verified by cgroup;
every turn shows its cost. Of the 15 high-weight items, Theseus is better on 1, worse on 5 and missing 9.

**Gaps, ranked** (delight §4):

| # | Gap | Rows |
|---|---|---|
| G1 | No conversation to open | `chat`, `chat-sessions` |
| G2 | A paste breaks the input: lines stored out of order (P1, theseus-klo2); a paste quits the TUI or approves a card (P1, theseus-8hcg) | `submit-order`, `tui-paste`, the chat's editor |
| G3 | Replies print as raw markdown; tools show only their names; results are never shown | `term-render`, `tool-words` |
| G4 | Edits are approved blind | `tool-words` (a diff on the question and after the edit) |
| G6 | No one-key stop | the chat's Esc |
| G7 | No undo | `rewind` |
| G8 | Sessions have no project | `session-dir`, `project-context` (shared with coding) |
| G5 | Every gated call asks, every time; no modes | `scoped-grants`, `modes` (decision B5) |
| G9 | Model, compaction and context live in other commands | `chat-commands` |
| G10 to G12 | A 104-line help page; memory off by default; files and images only by `--attach` | `chat`, `fresh-defaults`, `chat-commands` |
| G13 | No IDE integration | out of scope for now (delight decision 10) |

## 1.3 Memory (theseus-iacs)

**The target** (killer §4.1 to §4.4 and §5.3; personality §2 P3; scale row 9). Seven tests, each head to head where a
competitor can run it:

| # | Test | Pass line |
|---|---|---|
| M1 | The recall bench: Theseus, Claude Code and Pi on the same seeded history (built, never run) | Theseus's recall above both at every age; confident-wrong below both |
| M2 | Standing orders survive compaction: 30 orders in 20 sessions, 200 turns and 3 compactions each | presence 100% from the manifests, by construction; adherence at least Claude Code's and Codex's when given the same orders |
| M3 | A preference stated once holds in the session, after compaction, in a new session, on another surface, in another project, and a week later | 100% in all six |
| M4 | Provable memory: does the reply cite the right message? | ≥ 90% of the exam's passes cite the gold message; the citation check's precision 0.9 on the owner's labels |
| M5 | Fact recall: plant a wrong fact, let it spread, retract it | every exposure found in under 1 s on an owner-sized store; the retracted fact recalled 0 times after |
| M6 | Recall inside its deadline on the owner's daemon | the vector half answers within 250 ms in ≥ 90% of recalls |
| M7 | History comes along | this machine's 19,304 Claude Code session files (9.3 GB) imported, private and scrubbed; the private exam (theseus-7gir.9) answered with citations |

**Today:**
- **The exam** (killer §2.1, row 4): no recall 14.6%, keywords 45.8%, Theseus's recall **67.1%**, the right notes
  handed over 99.5%. So what is left to win is retrieval. Questions about time and about corrected values score 0% in
  every arm (killer §3.2).
- **The import:** 21,779 episodes of the owner's earlier history in 2 min 4 s, none rejected (killer §2.1).
- **Never measured against anyone,** and off on a fresh install: the public template ships recall and the judge off
  (killer §3.2).
- **A live probe** (personality §3.5): a preference stated in one session ("start with 'Aye.', two sentences at most")
  was gone in the next once the store held 8 other sessions; recall went to database notes. Claude Code wrote it to
  its auto-memory and kept it in the same directory, not in another.
- **On the owner's daemon,** recall waits out its full 250 ms deadline in 36 of the last 41 recalls: the vector
  query's ~350 ms embed never fits, so recall falls back to words alone (scale §3.4).

**Why Theseus can crush it** (killer §4): one store with lineage. A memory can cite the exact message it came from
(Claude Code's and Codex's memories are files with no source); an order can sit in the compiled prompt with a
manifest proving it was there; a wrong fact can be traced everywhere it spread; and other harnesses' history can
come along (none of them imports another's).

**Gaps, ranked:**

| # | Gap | Rows |
|---|---|---|
| 1 | Nothing about the owner is sure to be in context: a stated preference or order lives only as a message | `standing`, `standing-capture`, `card-proposals` |
| 2 | Never measured head to head | the recall bench, the compaction-survival bench, the personality exam (section 3) |
| 3 | Recall's vector half misses its deadline; the first recall after an idle unload has no vectors | `recall-in-deadline`, `index-at-10x`, `warm-first-token` |
| 4 | Replies never cite, and nothing checks a remembered claim | `provable-memory-a`, `provable-memory-b` |
| 5 | Questions about time and about corrected values score 0% | `fact-recall-a`, `fact-recall-b` (memory by time joined as `74bec7b6`) |
| 6 | Off on a fresh install | `fresh-defaults` |
| 7 | Only OpenClaw's history format imports | `import-a`, `import-b` |
| 8 | Memory has no face on Discord; consolidation never speaks | `discord-journal`, `voice-first` |

## 1.4 Personality (theseus-slvl)

**The target** (personality §2): a personality exam (PX) in about ten families, with three arms on the same model:
Theseus with its persona and the owner's card, Claude Code bare, and Claude Code with the same persona and card in its
CLAUDE.md (the fair arm). Fresh stores with distractor history, forced compactions, every routed model; deterministic
checks first, a rubric judge for character, and 20 blind pairs a quarter for the owner, one key per pair.

| # | Dimension | Pass line |
|---|---|---|
| P1 | The same named character on every surface, after every compaction, restart and model switch | in 100% of compiled requests; ≥ 95% rubric-consistent in every cell |
| P2 | Knows the owner and acts on it unasked (about 20 tasks: a reminder in the owner's zone, the attribution line, the right reviewer) | ≥ 90% |
| P3 | A preference stated once persists, in six contexts | 100% |
| P4 | Learns from a correction | captured ≥ 90%; the owner's "repeat-yourself" count near 0 a week by week 4 |
| P5 | Opinions and candour; holds a position under pushback without new evidence | ≥ 50% of blind pairs; 0 capitulations |
| P6 | Warmth and humour that read the moment | ≥ 60% of blind pairs; format fit ≥ 90% |
| P7 | Initiative: a promised follow-up has a wake behind it; proactive messages worth the buzz | promises kept ≥ 95%; ≥ 80% rated worth it |
| P8 | A voice to be heard in | one owner-picked voice on every call |

**Today** (personality §3). The whole persona is three fixed sentences (`turn.rs:82`). The owner's daemon adds nothing:
no persona, no context file, no profile text, and none of its 161 ontology categories carries guidance. The 21,779
imported episodes (the earlier assistant's persona files, about a hundred notes of the owner's feedback) sit in an
archive that recall may or may not fetch. Five probes, same model, both tools (personality §3.5):

| Probe | Claude Code | Theseus today | Theseus with a 1 KB persona file |
|---|---|---|---|
| "Who are you, and what do you know about me?" | "I'm Claude … inside Claude Code"; knew the account's email and the directory | "I'm Theseus …"; knew only a handle seen in its tool texts | knew its name, the owner's card and the standing preference |
| A standing preference, then a new session | saved to its auto-memory unasked; kept in the same directory, lost in another | honest ("I have no tool for saving memory across sessions"); lost in the new session | kept, in every new session |
| "Rewrite my Rust daemon in Go? One short paragraph." | "Don't do it.", with reasons and a cheaper path | "I'd lean against the rewrite", more explicit about its uncertainty | argued from what the owner's card says the owner values, unasked |
| "Long day … quick one: rebase vs merge?" | sections and bullets, 386 tokens | headers, a five-row table, bullets, 534 tokens | 102 tokens, and "get some sleep" |

The persona's cost: the cached header grew from 12,446 to 12,841 tokens (+395), written once and then read at a tenth
of the price; no call added. Every compiled request named the persona and its digest, so **presence is provable from
the manifest today**, which Claude Code cannot match.

**Gaps, ranked** (personality §4):

| # | Gap | Rows |
|---|---|---|
| G1 | No owner-shaped persona in use, and no way to shape one but hand-editing config | `persona-on` tonight (config only), `persona-data` |
| G2 to G4 | No owner model; a preference lost in the next session; a chat preference survives compaction only if the summary keeps it | `standing`, `standing-capture`, `card-proposals` |
| G11 | Nothing measures personality | `px-exam` |
| G5 | The model is never told which surface it is on (a table for a tired "quick one" on a phone) | `surface-tag` |
| G6 | What Theseus says first has no character; a promised follow-up has no wake behind it | `initiative-voice`, on `voice-first` (discord2) and `reminders` (horizon) |
| G9 | Thin self-knowledge: shown a voice-channel invite, it said "I can't join" (theseus-0pp3) | `self-card` |
| G7 | One global speaking voice, chosen by no one | `named-voice` |
| G8 | No Discord presence: no nickname, avatar or About Me | `discord-delight` |
| G10 | Untested across routed models (Sonnet, Haiku, GLM, Opus per message) | the exam's model family |
| G12 | The privacy line for the owner's personal facts is not drawn | `standing` (private by default; shared places get only its header) |

## 1.5 Speed and scale (theseus-c892)

**The target** (scale §2). *Speed:* on every row a person feels, at least 2x Claude Code at the median and never worse
at p95, on this machine under its real neighbour load. *Scale:* at 10x today's load (200,000 sessions, 10,000-turn
sessions, 1.7M chunks, 80 turns at once, 500 jobs, 10,000 connections, a week of use), every row inside its budget.
Both harnesses talked to one stand-in model, so only the harnesses differ ($0 of model).

| Speed row (scale §3.1) | Claude Code 2.1.295 | Theseus | Target | Met? |
|---|---|---|---|---|
| Start to a ready prompt | 408 to 502 ms | 4.8 to 31 ms on a small store | ≤ 50 ms at the owner's size and at 10x | small store yes; at 21,779 sessions `watch -i` 543 ms |
| Typed prompt to the request leaving | 25 to 34 ms | 25.5 to 28.6 ms on a quiet disk; **278 ms p50 on the owner's daemon** | ≤ 10 ms p50 | no |
| The model's first byte to text on screen | 112 to 216 ms | 0.6 to 1.5 ms | ≤ 5 ms | yes |
| A tool call: a file read / a shell command | 23 / 28 ms | **34 / 46 ms**; 4.6 / 8.0 ms with the store on tmpfs | ≤ 10 / 15 ms | no |
| Resume a long session | 583 to 1,647 ms | 24 to 31 ms | ≤ 50 ms | yes |
| CPU per plain turn | 270 to 300 ms | ~10 ms | ≤ a tenth | yes |
| Idle CPU; memory | 15 to 18 ms a second; 249 to 268 MB | 0 to 0.5 ms a second; 37 to 40 MB | ≤ 1 ms a second; ≤ 64 MB | yes (small store) |
| Under a neighbour's IO | not sensitive (no sync on its path) | 47 ms a store frame; calls stalled up to 0.9 s | p95 ≤ Claude Code's | no |
| Discord: a typed DM to the first text | none | the model's first byte + 58 to 88 ms | ≤ first byte + 100 ms | yes |

| Scale row at 10x (scale §3.4) | Holds | Breaks |
|---|---|---|
| S1 200,000 sessions | start 20 ms; paged lists 6 ms | `watch -i` 5.3 s to a prompt; `health` 682 ms; one whole list 5.8 s and 153 MB, leaving the daemon at 5.4 GB |
| S2 a 10,000-turn session | resume 24 to 31 ms | the first turn after a start **30.8 s** (quadratic); CPU per turn grows 23 → 101 ms with length |
| S3 1.7M chunks | | recall misses its deadline already today; a full re-embed would take ~7 days |
| S4 80 turns at once | all done in 24.7 s at 121 MB (Claude Code: 8 at once took 1.84 GB) | 42% of pairs out of order; the person's own turn ran 67th of 81 |
| S5 500 jobs | `health` 6 to 8 ms throughout | each completion costs more as more run (22 → 39 ms) |
| S6 10,000 connections | 900 watchers heard a reply within 35 ms | **the daemon exits at 1,017 connections** (soft fd limit 1,024) |
| S7 a day and a week | | RSS +127 KB a turn with no plateau; a restore needs 4.9x its WAL in memory |

**Where the losses come from** (scale §3.2): 85% to 90% of the harness's own time on a turn is waiting for
`fdatasync` (3 syncs before the request on a resumed session, 4 per tool loop). With the store on tmpfs the same
daemon does a read in 4.6 ms and a shell call in 8.0 ms, 5x faster than Claude Code. Durability is a feature (Claude
Code does not sync its transcript, so a crash can lose its last turns), so the fix is a budget for syncs, not
dropping them. And in absolute terms (scale §7): a real model's first byte takes 0.7 to 13.5 s on the owner's daemon,
so a 25 ms harness share is 1% to 3% of a turn; the leads a person feels are the start (~400 ms), resume (0.6 to
1.6 s), the first text (Claude Code's 110 to 210 ms of render lag), CPU and memory.

**Gaps, ranked** (scale §4):

| # | Gap | Rows |
|---|---|---|
| G1 | Syncs on the critical path: they fail T2, T4 and T8 | `turn-one-frame`, `tool-loop-frames`, `dispatch-overlap` |
| G5 | Recall's vector half misses its deadline; one embedding thread (~7 days to re-embed at 10x) | `recall-in-deadline`, `index-at-10x`, `warm-first-token` |
| G3 | 1,017 connections stop the daemon | `fd-soft-fail` |
| G9 | Long sessions: a quadratic first turn, CPU per turn linear in length | `long-session-o-new` |
| G2, G4 | Whole-history reads at every start; `health` costs O(state) | `cli-pages`, `health-o1` (the daemon side and the cockpit are theseus-0jet) |
| G7 | Admission is unfair at load, and an input still waiting at 600 s fails | `admission-fifo` |
| G8 | Job bursts: superlinear completions, no cap, jobs can take the daemon's last threads | `jobs-burst` |
| G6 | Memory over a day: 73 MB + 277 MB of swap on the owner's daemon after 13 h | `memory-flat` |
| G12 | The Discord path: two extra syncs; a tool line waits for the 1.2 s edit tick | `discord-path` |
| G13 | Every 1,000 records a durable checkpoint stalls every queued append (theseus-zsu9) | folded into `tool-loop-frames` |
| G11 | A cold provider connection after 90 s idle | `warm-first-token` |
| G10 | No head-to-head bench in the repo | `h2h-bench` |

**And one cost the owner pays today** (theseus-ezeg): the owner's DM session holds about 626K tokens (about 428K of
them one 213-page PDF attached on Oct 7), and **the `fable`, `opus` and `haiku` profiles cache for 5 minutes**: only
`[model]`, `sonnet` and `haikuhi` carry `cache_ttl = "1h"`, and a profile doesn't take `[model]`'s value. So a message
after more than 5 idle minutes rewrites the whole prompt, about $7.85 on Fable 5.1 (the ledger matches the 5-minute
write price to the cent). At the $200 daily ceiling that is about 25 such messages. Section 4 lists the fix.

## 1.6 Task horizons (theseus-l00z now, theseus-i3z0 later)

**The target, now** (plan §1.1, six tests): every task visible by name without a command; asked when needed, where
the owner is (2 s on the surface, the phone within 60 s when no Theseus screen is open); interrupted only when it
matters; answered anywhere and cleared everywhere; "since you last looked"; still fast (a plain turn stays 5 frames,
the status line costs 5 ms or less, a change reaches every surface in 10 ms or less).

**The target, later** (horizon §2, eleven tests, run live across a restart and an install):

| # | Test | Pass line | Today (horizon §3.5) | Claude Code 2.1.294 (horizon §3.4) |
|---|---|---|---|---|
| H1 | It survives every client closing, a restart, an install, `kill -9` | 100% fire; a series runs once and says "N missed" | ✓ for wakes, tasks, jobs, questions (due wakes ran 0.35 s after serving) | its "remind me in 2 minutes" died with its process, as it said it would |
| H2 | It reaches the owner | 2 s on the surface, 60 s to the phone; 0 silent | ✗ a reminder set from the CLI went to history only | fires only while its REPL is idle |
| H3 | On time | p99 ≤ 1 s | ✓ 254 ms | up to 10% of a period late |
| H4 | One view of the future on every surface | yes | ✗ wakes in three lists, by id | `/tasks`, `CronList` and the web, each its own list |
| H5 | History, cost and an enforced budget per routine; a failure pings within 60 s | yes | ✗ | routines in the cloud; no budget per job |
| H6 | Unattended runs never wait on an approval nobody can answer | 0 lost | half | `allowed_tools` on cloud routines |
| H7 | Waiting on the world costs nothing | $0 of model a check | ✗ a model turn per check | `Monitor` expires within 30 min |
| H8 | The model knows its commitments after 3 compactions, and moves or cancels one by words | 100% | ✗ it can only set a wake | `CronList` |
| H9 | Projects: dated milestones, progress everywhere, a weekly review | yes | ✗ | one `/goal` per session |
| H10 | Snooze, move, pause, resume, run now, cancel from every surface | yes | half: cancel only | partly |
| H11 | FAST: a plain turn stays 5 frames, no new timer | yes | ✓ | |

**Now** (plan §1.2): every task shows as "task"; the push carries no titles, progress or tree; the CLI shows nothing
of the tasks it started; herdr needed a manual sync, and its toasts were off (the owner turned them on at 19:52);
Discord's board did not follow running tasks. Wave A, in flight tonight, names the tasks, adds the status line and
fixes the push. And the owner's daemon has never held a single wake or task (horizon §3.3): the owner's recurring work
runs on OpenClaw.

**The design, in one line** (horizon §5.0): every future thing is a wake with a purpose. A **reminder** (it tells the
owner, with no model call), a **check-in** (today's wake), a **deferred task** (opened asleep), a **routine** (a fresh
task per run, with its own budget and pre-approved tools), and a **watch** (a probe with no model, until a condition
holds); **projects** are dated plan items with a weekly review. All of it lands on the work board, bucketed Now, Today,
This week, Later, Recurring, Waiting on the world, and Projects.

**Gaps, ranked** (horizon §4; plan §1.2):

| # | Gap | Rows |
|---|---|---|
| 1 | A reminder can reach no one (G1) | `reminders`, with its Home conversation (the DM) |
| 2 | Tasks are anonymous; the push carries no titles, progress or tree (now) | wave A in flight; `work-board`; `progress` |
| 3 | The model can't see, move or cancel what it promised (G3) | `commitments` |
| 4 | No routines; unattended runs stall on approvals (G7, G8) | `routines`, `routine-trust` |
| 5 | Waiting on the world costs a model turn per check (G9) | `watches`, `watch-replies` |
| 6 | "Do this Friday" holds a task's budget all week (G10) | `deferred` |
| 7 | No view of the future (G12) | `horizon-board`, `horizon-surfaces` |
| 8 | No projects (G11) | `projects` |
| 9 | A question asked in one place can't be answered in another; one nobody sees waits (now) | `work-answer`, `owner-ask`, `always-on-dm` |
| 10 | Limits: 30 days ahead, 5 per conversation, no months or cron (G6) | `reminders`, `routines` |
| 11 | A task can stop before it is done (G13) | spine row 26b |

## 1.7 Discord, the surface that carries most of it (theseus-bh3t)

**The target** (discord2 §1, §4; the owner's correction of 20:01). Claude Code has no Discord surface (it has Remote
Control and a mobile app), so the bar is the owner's own feeling, written as checks:
- **A lively chat:** thinking streams into each loop's tool message and folds to "thought for 4 s" when the answer
  starts; tool lines on; four keys per place (`show_thinking`, `show_tools`, `status_reactions`, `silent`), all on for
  the owner; every chat message may ping, as today.
- **Theseus speaks first,** once per event, in its own voice, on 15 events (a question, a reminder, a check-in, a
  finished or failed task, a routine's run, a watch met, what consolidation noticed, spend crossing a line, health, a
  restart, a morning note, long imports, proposals), paced: the owner's turn first, bursts folded into one ping, a
  night hold.
- **A house in the owner's trusted guild:** a board that never scrolls, a forum post per task under the task's own
  name and avatar, journal, ledger and health channels, and a status line in the sidebar.
- **Within Discord's limits:** a busy day is about 3,700 writes, at most about 3 a second at the peak against 50; FAST
  holds (no new frame on a turn, no new timer).

**Today** (discord2 §2): tool lines stream, but **thinking never reaches Discord** (the daemon streams it; the renderer
drops it at `render.rs:596`). Theseus speaks first for questions, task reports, failures, restarts and a few notices,
in a system's words ("Task `a1b2c3` finished"), and never for consolidation, health beyond the disk, spend below the
ceiling or a morning note. The task board is a pin in the scrolling DM. The bot holds 6 permissions and cannot create a
channel, webhook, thread or event. Wave A's `discord-silent` was built "silent by default" before the correction; its
redirect is running now (`discord-silent-cont`).

**Gaps, ranked** (discord2 §3):

| # | Gap | Rows |
|---|---|---|
| G1 | The wave A row, as built, would silence the chat | step 1 (in flight) |
| G2, G3 | Thinking never reaches Discord; nothing is configurable per place | `discord-show` |
| G4 | Theseus speaks first only sometimes, and as a log line | `voice-first`, then the persona's words (`initiative-voice`) |
| G5 | No pacing: speech lands mid-conversation; bursts ping separately | `discord-pace` |
| G6, G10 | Tasks have nowhere calm to live; the bot can't build anything in the guild | decision B3; `discord-house`, `discord-board`, `discord-task-posts` |
| G7 | The board doesn't follow running tasks | `discord-hears-push`, `discord-board` |
| G8 | One identity for everything | webhooks in `discord-task-posts` |
| G9 | No home for memory or money | `discord-journal`, `discord-ledger-health` |
| G11 | Nothing ambient survives a restart, and a webhook token is a credential | the house keeps ids in META and reads tokens back after each Ready |

The 43 Discord capabilities, ranked by delight × feasibility, are in discord2 §4.4. Its top dozen (20 to 25 points
each): buttons, webhook identities, a forum with tags, Components V2, forwarding a message into the DM, message
context menus, the silent flag, self-updating timestamps, subtext, slash commands, ephemeral replies and the custom
status.

---

# 2. One build plan

Every row from every report, once. About 120 rows: 10 already in flight tonight, then five waves by what they wait
for. Each row is one branch: built in the cloud, reviewed, joined after the full gate with its lifecycle budgets, and
proven live on a scratch daemon of its own build, as every report's rows already say. **Cloud rows use no local
session slot,** but the cloud holds 16 rows in flight, builds and reviews together (`launch_only.py`), so wave 1 is
ordered: launch from the top as slots free.

**How to read the tables.** *Issue:* an existing Beads issue, or "new · 7m7w" for a row the DM thread files under that
parent once the owner has read this (this synthesis changed no issue). *Size:* S fits a 2-hour cloud session, M a
4-hour one, L needs two branches (delight §6.1's scale); a `*` marks a size estimated here, because its report gave the
scope but no size. *Where:* "cloud" is a cloud row; "local" runs on this machine because it reads private data or
drives Claude Code or the owner's daemon; "config" is no branch at all.

**Assigned once.** Where two reports planned the same thing, it is one row:

| One row | Merges | Owner of each part |
|---|---|---|
| `session-dir` | coding 7.1 and delight's "sessions have no directory" (G8, E5) | coding: the protocol field and the tools' directory; delight: how the chat shows it |
| `project-context` | coding 7.2 and delight D4 | coding: discovery, environment, freshness; delight: the one-time trust and `/init` (decision B4 settles the files) |
| `job-handles` and `peek-attach` | coding 7.4's `job_read` and the board's `work.peek` (plan wave C) | one tail reader of the job's spool file, shared |
| `standing` and `standing-capture` | killer R7, R8 and personality rows 4, 5 | one record (order, preference, fact), one block, one capture question, one presence check |
| `rewind` | delight D8 and killer R19, R20's pre-images ("fork any moment") | delight's two branches; R20's `--worktree` and R21's surfaces stay in wave 5 |
| `why` | killer R5 and delight's `/context` (M8) | `why` renders `context.explain`; the chat's `/context` and `/why` call it |
| `horizon-board`, `horizon-surfaces` | horizon HZ8, HZ9 and the work board's surfaces | they wait for `work-board`; Discord's sections live in `discord-board` |
| `discord-delight` | discord2 step 14, horizon HZ11 (the calendar) and personality's G8 (nickname, avatar, About Me) | discord2 builds; the persona supplies the words and look |
| `initiative-voice` | personality row 9 on discord2's `voice-first` and horizon's `reminders`, `routines` | discord2 and horizon own when and where; the persona owns the words |
| `task-forecast` | killer 4.7 and the task plan's wave D forecast | one row pair |
| discord2's steps 2 to 15 | the first Discord pass's 13 steps (plan §1.6) | discord2 §5 maps each old step to its new one |
| `turn-one-frame` | scale row 4, theseus-2uby (S5) and push-fixes' one-frame open | scale row 4 builds on push-fixes |
| `chat`'s dock | the CLI plan's C5 (task footer, one-key task answers) | delight D3 builds it; C5 is not built twice |

**The shape of it.** What waits for what, in one picture:

```
main, now ─┬─ submit-order · term-render ─► chat ─┬─► chat-sessions · chat-commands · rewind
           ├─ tool-words ─► scoped-grants ─► modes                (chat also needs decision B6)
           ├─ session-dir ─► project-context (B4) ─► chat-sessions
           ├─ standing ─► standing-capture · card-proposals (needs work-board too)
           ├─ secrets-shapes ─► proc-env (B7)          recall-in-deadline ─► index-at-10x
           └─ commitments · deferred · output-kept · tool-input-fit · fd-soft-fail · persona-data · …
work-types (in flight) ─┬─► reminders ─┬─► routines ─► routine-trust ─► evening-haiku
                        │              └─► watches ─► watch-replies
                        └─► work-board ─┬─► work-answer ─► owner-ask · always-on-dm · task-start
                                        └─► horizon-board ─► horizon-surfaces · projects
push-fixes (in flight) ─► turn-one-frame ─► tool-loop-frames · dispatch-overlap · discord-path
spawn-ask-follow (in review) ─► job-handles ─► subagent-wait          … and run R0 (Terminal-Bench)
discord-silent-cont (in flight) ─► discord-show · voice-first ─► discord-pace ─► morning-note
decision B3 (the bot's permissions) ─► discord-house ─► discord-board · discord-task-posts · journal · ledger
```

## Wave 0: in flight tonight (don't relaunch)

| Row | What the owner gets | From | Issue | State at 22:45 |
|---|---|---|---|---|
| `push-fixes` | the push's warts fixed (outstanding counts, why, a question's expiry and listing); `session.open` in one frame | plan §1.7, wave A | theseus-q5af | build running; its report due about 23:05 |
| `work-types` | one view type for every kind of work, the shared ping rule, answer options | plan wave A | theseus-753z | built (`38ae464d`); its review is running |
| `client-names` | every task named by its title; the TUI keeps a session's profile; short ids everywhere; local times | plan wave A | theseus-0n1v | `client-names-cont` running (the first session died at the limit, its code pushed) |
| `discord-silent`, redirected | discord2's step 1: the chat pings as today; silence is opt-in per category and per place | plan wave A; discord2 §5 | theseus-l1y1 | `discord-silent-cont` running |
| `cli-status` | `theseus status` (long, `--short`, `--watch`, `--watch --tab`) and `theseus wait --any` | plan wave A | theseus-lweh | built (`9c2d1aac`); its review is running |
| `seen-shared` | one "seen" shared by the TUI and the CLI | plan wave A | theseus-yus0 | built (`6af5ce9f`); its review is running |
| `spawn-ask-follow` | a one-shot `theseus ask` waits for its late results and near wakes | coding §3 | theseus-mqxk | its relaunched review is running; the 00:30 joiner takes it if accepted |
| `terminals` | a program started in a terminal outlives the terminal; waiting for a typed command is one call | coding §3 | theseus-ggqf | ✓ joined at `024296fb` (21:50) |
| `footer-cost` | each reply's footer: the session's total, with this reply's cost in parentheses | the owner, 20:19 | theseus-c0bb | ✓ joined at `21167cac` (22:32) |
| `persona-on` | the persona file and two config lines on the owner's daemon | personality §5.0, row 0 | theseus-slvl | config only; waits for decision B2 |

The wave A rows are due by about 00:20; the 00:30 harvest wake spawns the joiner and the remaining reviews
(`contexts/theseus-l00z.md`, 21:10). Main is at `21167cac`.

## Wave 1: can start on main now, in this order

| # | Row | What the owner gets | From | Issue | Size | Where | Note |
|---|---|---|---|---|---|---|---|
| 1 | `submit-order` | a paste is stored in the order it was typed (requests that change a session apply in order) | delight D0 | theseus-klo2 (P1) | S | cloud | a live defect |
| 2 | `session-dir` | a session works in the directory it was started in; `pwd` answers it | coding 7.1; delight G8 | new · 7m7w | S-M | cloud | move 1; may bump the store format |
| 3 | `term-render` | one renderer: markdown, highlighted code, coloured diffs, word wrap, for `ask`, `history`, the chat and the TUI | delight D1 | new · wy6v | M | cloud | move 2 |
| 4 | `tool-words` | tool lines in words, results shown, a diff on every edit's question and after it, on every surface | delight D2 | new · wy6v | M | cloud | move 2 |
| 5 | `standing` | orders, preferences and facts about the owner in every prompt; a compile that drops one is refused; `theseus standing` | killer R7; personality row 4 | new · iacs | M | cloud | move 4; store format bump |
| 6 | `output-kept` | a capped command's whole output kept (scrubbed) and named; a file read never has a hole | coding 7.5 | new · 7m7w | M | cloud | move 3 |
| 7 | `tool-input-fit` | no invalid-JSON round trips; `proc_run {command}`; the tool names the model reaches for | coding 7.7 | new · 7m7w | S | cloud | coding D5 |
| 8 | `commitments` | the model sees its reminders, routines and watches and moves or cancels them by words; `theseus wakes` prints titles | horizon HZ1 | new · i3z0 | M* | cloud | move 5 |
| 9 | `fd-soft-fail` | the daemon survives thousands of connections; the unit gets `LimitNOFILE` and `TasksMax` | scale row 1 | new · c892 | S* | cloud | a cliff |
| 10 | `bench-parity` | a fair rerun: the same packages in both containers, the trial limit at $2 plus one reservation, quiet graders, the attempt plan committed first | coding 7.10 | new · 7gir | S | cloud | before run R0 |
| 11 | `recall-plan-fix` | the recall bench's plan fits the prompt again (13,958 tokens after `terminals`) | theseus-cs8k | theseus-cs8k | S | cloud | before R0 and the recall bench |
| 12 | `persona-data` | the persona as data (name, emoji, voice, initiative); a built-in default persona; `theseus persona`; a persona per place | personality row 2 | theseus-9xli | M* | cloud | move 4 |
| 13 | `recall-in-deadline` | recall's vectors answer inside 250 ms: a short query, a 4-thread pool, abandoned queries stopped | scale row 9 | new · c892 | M* | cloud | moves 4, 6 |
| 14 | `deferred` | "on Friday, draft the release notes" opens a task asleep that starts then | horizon HZ3 | new · i3z0 | S* | cloud | move 5 |
| 15 | `cli-pages` | `watch -i` ready in 50 ms or less at 200,000 sessions: the CLI asks for one session, not all | scale row 2 | new · c892 | S* | cloud | daemon side: theseus-0jet |
| 16 | `px-exam` | the personality exam with Claude Code arms (bare and with the same CLAUDE.md), and its report | personality row 1 | new · slvl | M* | cloud; runs local | the "before" |
| 17 | `f10` | the first-ten-minutes driver and scorecard; reproduces tonight's 8 of 42 | delight D10 | new · wy6v | S-M | cloud; Claude Code arm local | the "before" |
| 18 | `file-tools-polish` | CRLF files edit normally; a stale edit is refused; search can include hidden files | coding 7.8 | new · 7m7w | S | cloud | joins after `output-kept` (same `fs.rs`) |
| 19 | `surface-tag` | the model knows where it is ("Discord DM, phone or desktop"); replies signed "Theseus" | personality row 3 | new · slvl | S* | cloud | |
| 20 | `self-card` | Theseus knows what it can do here ("type `/join`") | personality row 8 | theseus-0pp3 | S* | cloud | |
| 21 | `secrets-shapes` | the scrubber knows OpenAI, Stripe and Google keys and passwords in database URLs; the planted-secret corpus | killer R4 | new · iacs (+ theseus-ej7i) | M* | cloud | before `proc-env` |
| 22 | `health-o1` | `health` and the push's seed cost the same at any store size | scale row 3 | new · c892 | M* | cloud | |
| 23 | `admission-fifo` | a person's message never waits behind a queue of task turns; nothing fails for its wait | scale row 7 | new · c892 | M* | cloud | |
| 24 | `jobs-burst` | 200 jobs end without slowing the daemon; a cap on jobs; jobs can't take the daemon's last threads | scale row 8 | new · c892 | M* | cloud | scale D-2 |
| 25 | `memory-flat` | memory that stays flat over days; out of swap; a restore that fits in memory | scale row 11 | theseus-4qou | M* | cloud | scale D-5 |
| 26 | `warm-first-token` | no cold connection after 90 s idle; recall's model warmed when the owner starts typing | scale row 13 | new · c892 | S* | cloud | |
| 27 | `long-session-o-new` | a 10,000-node session's first turn in under 1 s (today 30.8 s); flat CPU per turn | scale row 12 | new · c892 | L* | cloud | shares the ring with row 28 |
| 28 | `one-turn-compaction` | a long single instruction keeps going: old results cleared, compaction inside the turn | coding 7.3 | new · 7m7w | L | cloud | coding's wave 2; join one of 27, 28, then rehearse the other |
| 29 | `h2h-bench` | the head-to-head speed bench in the repo, with its report; counted rows in the gate | scale row 15 | new · c892 | M* | cloud | measurement |
| 30 | `discord-house` | `/setup` builds the house in the owner's trusted guild | discord2 step 6 | new · bh3t | M* | cloud | waits for decision B3 |
| 31 | `fresh-defaults` | a fresh install has recall on and the judge on when a Jev key resolves | killer R1 | new · iacs | S* | cloud | |
| 32 | `why` | `theseus why` and Discord `/why`: what the model was shown, and why each piece was there | killer R5 | new · iacs | S* | cloud | |
| 33 | `money-words` | the README says it again truly: every call reserved before it runs, a hard daily ceiling | killer R6 | new · iacs | S* | cloud | the ceiling (theseus-kp20) joined |
| 34 | `provable-memory-a` | replies cite the messages they remember; footnotes on every surface | killer R9 | new · iacs | M* | cloud | |
| 35 | `import-a` | Claude Code and Codex history imported, scrubbed on the way in | killer R11 | new · iacs | M* | cloud | |
| 36 | `fact-recall-a` | mark one fact wrong; every place it spread is found and corrected | killer R13 | new · iacs | M* | cloud | |
| 37 | `terminal-followups` | a terminal whose shell exited still tracks its jobs (theseus-d1zw); `term.open`'s description (theseus-3l9i) | the terminals join | theseus-d1zw, theseus-3l9i | S | cloud | |
| 38 | `stop-nudge` | a task, a conversation or a one-shot `ask` that announces unfinished work and stops gets one nudge | spine 26b; coding D8 | (spine 26b) | M* | spine | live, with a switch |
| 39 | `coding-log` | the scorer of the owner's coding sessions on both harnesses (corrections, false "done", changes kept) | coding 7.10 | new · 7m7w | S | local | reads private transcripts |

## Wave 2: waits for a branch in flight tonight, or for a wave 1 row

| Row | What the owner gets | From | Issue | Size | Where | Waits for |
|---|---|---|---|---|---|---|
| `reminders` | a reminder always reaches the owner (the Home DM when its session has no place), with Done and Snooze; owner verbs with no model turn (`theseus remind`, `/remind`); 365 days ahead | horizon HZ2 | new · i3z0 | L* | cloud | `work-types`; store format bump (one bump declares every wake kind) |
| `work-board` | the engine: one view of every task, job, question, wake and plan step, with title, doing, progress, question and options | plan §1.3, wave B (the keystone) | theseus-clxd | L* | cloud | `work-types` |
| `herdr-slots` | herdr's reporter in its proper slots; watches that reconnect after an install | plan wave B | theseus-inj9 | M* | cloud | `work-types` |
| `retry-dismiss` | a failed task stays flagged until dismissed; Retry makes a linked task | plan wave B, §1.8 | new · l00z | S* | cloud | `work-types` |
| `tui-paste` | the TUI takes a paste as text, never as keys; Enter works at start | delight D11 | theseus-8hcg (P1) | S | cloud | `client-names` |
| `turn-one-frame` | a turn starts in one frame: 1 sync before the request instead of 3 (6 on a new session) | scale row 4 | theseus-2uby | M* | cloud | `push-fixes` |
| `job-handles` | `proc_run {background}`, `job_read`, `job_wait`, `job_stop`; a job's end noticed in the same turn; blocking stays the default | coding 7.4 | new · 7m7w | M-L | cloud | `spawn-ask-follow` |
| `discord-show` | thinking in the chat, folding to one line when the answer starts; tools on, summary or off; status reactions; `/show` per place | discord2 step 2 | new · bh3t | M* | cloud | `discord-silent-cont` |
| `voice-first` | Theseus speaks first on V1 to V15 through one new post, `say`: consolidation, health streaks, spend at 80% and 100% of the ceiling, a crash restart, long jobs | discord2 step 4 | new · bh3t | L* | cloud | `discord-silent-cont` |
| `project-context` | Theseus follows the repo's AGENTS.md and CLAUDE.md, nested ones on first touch; knows the date, OS and git state | coding 7.2; delight D4 | new · 7m7w | M | cloud | `session-dir`; decision B4; may bump the format |
| `chat` | bare `theseus`: an inline conversation with a live dock, multi-line input, a folded paste, one-key answers with the diff, Esc to stop, a queue (D3a, then D3b) | delight D3 | new · wy6v | L | cloud | `term-render` (uses `submit-order`, `tool-words`); decision B6 |
| `scoped-grants` | "yes, and don't ask again" for a command prefix or for edits in this project, for the conversation; listed and revocable | delight D5 | new · wy6v | M | cloud | `tool-words`; decision B5; a security review |
| `proc-env` | commands see the owner's toolchain (venv, nvm, `JAVA_HOME`), minus every secret | coding 7.6 | new · 7m7w | S-M | cloud | `secrets-shapes`; decision B7 |
| `standing-capture` | "no tables with me" said once becomes a standing preference, with a notice and Undo (asks when unsure); the compaction-survival bench | killer R8; personality row 5 | new · iacs | M | cloud | `standing` |
| `provable-memory-b` | each cited sentence checked against its source between turns; the exam's attribution column | killer R10 | new · iacs | M* | cloud | `provable-memory-a` |
| `fact-recall-b` | `theseus retract`, the spread drawn in the cockpit, the plant-and-retract bench | killer R14 | new · iacs | M* | cloud | `fact-recall-a` |
| `import-b` | Pi, OpenCode and Aider history | killer R12 | new · iacs | M* | cloud | `import-a` |
| `index-at-10x` | embedding on idle cores (paced by pressure), a tender that starts in 1 s, ≤ 1 GB at 1.7M chunks | scale row 10 | new · c892 | L* | cloud | `recall-in-deadline` |
| `tool-loop-frames` | a file read ~11 ms instead of 34; a shell call ~25 ms instead of 46; the checkpoint waits for an idle writer | scale row 5 | theseus-zsu9 | M* | cloud | `turn-one-frame` |
| `dispatch-overlap` | the model call's sync overlaps its request: the request leaves in ~4 ms | scale row 6 | new · c892 | S* | cloud | `turn-one-frame`; scale D-1 |
| `discord-path` | a DM's reply posts sooner: fewer syncs, a tool line drawn at once, idle ticks stopped | scale row 14 | theseus-ht8b | M* | cloud | `turn-one-frame`; same files as `discord-show`, `voice-first` |
| `discord-pace` | the owner's turn first; bursts fold into one ping; the night hold; once per event | discord2 step 5 | new · bh3t | M* | cloud | `voice-first` |
| `routines` | "every weekday at 8, summarize…": a fresh task per run with its own log, cost, month budget; pause, resume, run now; cron and month schedules | horizon HZ4 | new · i3z0 | L* | cloud | `reminders`; decision B5 |
| `watches` | "tell me when PR 123 merges": a check with no model call, every 5 minutes, $0 of model | horizon HZ6 | new · i3z0 | M* | cloud | `reminders`; decision B5 |
| `named-voice` | the persona's own speaking voice; a spoken greeting on `/join`; `theseus voice audition` | personality row 7 | new · slvl | S* | cloud | `persona-data` |
| **run R0** | the owed Terminal-Bench rerun, Theseus against Claude Code, 89 tasks × 3 | coding §6.2 | theseus-7gir.22 | | EC2 or here at night | `spawn-ask-follow`, `bench-parity`, `recall-plan-fix`; decision B1 |

## Wave 3: waits for the work board, or for wave 2

| Row | What the owner gets | From | Issue | Size | Where | Waits for |
|---|---|---|---|---|---|---|
| `work-answer` | one answer verb from any surface, by short id; answered once, cleared everywhere | plan wave C (substrate R3) | new · clxd | M* | cloud | `work-board` |
| `digest` | "since you last looked", on every surface | plan wave C (R5) | new · clxd | M* | cloud | `work-board` |
| `peek-attach` | a job's last lines and ETA, its output followed live, `attach` | plan wave C (R6) | new · clxd | M* | cloud | `work-board`; shares its reader with `job-handles` |
| `progress` | progress from plan steps and the model's own line before each call | plan wave C (R7) | new · clxd | M* | cloud | `work-board` |
| `cli-tasks` | `theseus tasks` as a tree; notices in words and Windows toasts; the TUI's rows say what each task is doing | plan wave C | theseus-muum | M* | cloud | `work-board` |
| `herdr-follow` | `theseus herdr up --follow`, then the plugin and its answer card, links, `theseus herdr setup` (then the owner's approved herdr keys and plugin) | plan wave C | theseus-inj9 | M* (several) | cloud | `work-board`, `herdr-slots` |
| `discord-hears-push` | Discord follows every task from the push | discord2 step 3 | new · bh3t | M* | cloud | `work-board` for full fidelity |
| `discord-board` | the board that never scrolls: Needs you, Now, Coming up, Since you looked; the status line; the voice channel's status | discord2 step 7 | new · bh3t | L* | cloud | `discord-hears-push`, `discord-house` |
| `discord-task-posts` | a forum post per task, under its own name and avatar: card, live log, report, tags; a message there reaches the task | discord2 step 8 | new · bh3t | L* | cloud | `discord-hears-push`, `discord-house` |
| `discord-journal` | what memory learned and what Theseus holds about the owner, each with Keep, Correct and Forget | discord2 step 10 | new · bh3t | M* | cloud | `voice-first`, `discord-house`, `standing` |
| `discord-ledger-health` | a Today card for money, incident cards for health; `/spend`, `/health` | discord2 step 11 | new · bh3t | M* | cloud | `voice-first`, `discord-house` |
| `horizon-board` | reminders, routines, watches and projects on the board, bucketed Now to Projects; `theseus agenda` | horizon HZ8 | new · i3z0 | M* | cloud | `work-board`, `reminders` |
| `routine-trust` | a routine's named tools run unasked; Try it now at creation; a run that would wait ends "blocked" with the reason, never silent | horizon HZ5 | new · i3z0 | M* | cloud | `routines`; `work-answer` for buttons; decision B5 |
| `watch-replies` | "tell me when the harbour master answers in #ops", event-driven | horizon HZ7 | new · i3z0 | S* | cloud | `watches` |
| `projects` | a project with dated milestones and a weekly review; its goal survives every compaction | horizon HZ10 | new · i3z0 | L* | cloud | `routines`, `horizon-board`; store format bump |
| `chat-sessions` | `theseus -c` continues this project's last conversation; `-r` picks one, from every surface; the transcript on reopening | delight D6 | new · wy6v | M | cloud | `chat`, `project-context` |
| `chat-commands` | `/` commands with a menu, `@` files, `!` shell, images by path; `/context` and `/why` | delight D7 | new · wy6v | M | cloud | `chat`; calls `why` |
| `rewind` | Esc Esc: files and conversation back to before a message (D8a pre-images and restore; D8b fork and picker) | delight D8; killer R19, R20 | new · wy6v | L | cloud | `chat`; store format bump |
| `modes` | Shift+Tab: ask, edits, plan, auto; a plan card before any edit | delight D9 | new · wy6v | M | cloud | `scoped-grants`; decision B5 |
| `subagent-wait` | a helper's answer comes back in the same turn; several at once | coding 7.9 | new · 7m7w | M | cloud | `job-handles` |
| `card-proposals` | up to 30 proposals for the owner's card from the archive, each with its sources, approved one by one | personality row 6 | theseus-0lrr.7 | M* | cloud; its run local | `standing`, `work-board` |
| `initiative-voice` | what Theseus says first, written in the persona's voice; promises kept, measured from the ledger | personality row 9 | new · slvl | M* | cloud | `voice-first`, `reminders`, `persona-data` |
| **run R1** | the parity claim, Terminal-Bench at 5 attempts | coding §6.2 | new · 7gir | | EC2 or here | coding's wave 1 and 2 rows joined |

## Wave 4: waits for the answer verb, then the faces

| Row | What the owner gets | From | Issue | Size | Where | Waits for |
|---|---|---|---|---|---|---|
| `owner-ask` | a task or conversation asks the owner with 2 to 6 options; it expires to its default after 30 minutes | plan wave D (R4) | new · clxd | M* | cloud | `work-answer` |
| `always-on-dm` | a question shows in the DM at once; it pings after 60 s when no Theseus screen is open | plan wave D (R8) | new · clxd | M* | cloud | `work-answer` |
| `task-start` | start a task from any surface with no model turn: "Make this a task", herdr's new-task key | plan wave D | new · l00z | M* | cloud | `work-answer` |
| `stop-all` | stop a conversation and all its tasks | plan wave D | new · l00z | S* | cloud | `work-board` |
| `task-forecast` | "about $0.60 and 8 minutes, at most $1.50", then held to it, with a notice past p90 | plan wave D; killer 4.7 | new · l00z | M* (2 rows) | cloud | `work-board` |
| `system-work` | imports and rebuilds on the board | plan wave D | new · l00z | S* | cloud | `work-board` |
| `discord-cards-v2` | questions with choices as buttons, selects and forms; answered from the DM, the board or the post | discord2 step 9 | new · bh3t | M* | cloud | `discord-board`, `work-answer`, `owner-ask` |
| `discord-commands` | `/tasks`, `/task`, `/answer`, `/quiet`, `/digest`; "Make this a task" on any message; a forwarded message read as outside text; horizon's `/remind`, `/agenda`, `/routine` | discord2 step 12 | new · bh3t | M* | cloud | `discord-hears-push`, `digest`, `task-start` |
| `horizon-surfaces` | the future on every surface: the TUI's Horizon section, herdr's counts, Discord's Coming up and Routines, the cockpit's Agenda | horizon HZ9 | new · i3z0 | L* (a branch per surface) | cloud | `horizon-board` and each surface's rows |
| `morning-note` | one note at 08:00 on days something happened, in the persona's voice | discord2 step 13 | new · bh3t | M* | cloud | `voice-first`, `discord-pace`, `digest` (or `routines`) |
| `discord-delight` | app emojis (a spinning gear), the persona's nickname and About Me, the guild calendar (off by default) | discord2 step 14; horizon HZ11 | new · bh3t | M* | cloud | `discord-house`, `discord-board` |
| `voice-notes` | the morning note as a voice message (off by default) | discord2 step 15 | new · bh3t | M* | cloud | `morning-note` |
| `lsp-default` | an edit's result shows the errors it caused, where a language server resolves | coding D7 | theseus-n88g.11 | S | cloud | run R2's A/B arm |
| `evening-haiku` | the first live routine, moved from OpenClaw | horizon HZ13 | new · i3z0 | S* | local (config and runbook) | `routine-trust`; the OpenClaw job stays on until the sprint freeze lifts |
| `acceptance-now` | the task plan's six tests, live, ten tasks across every surface | plan §1.1 | new · l00z | M* | cloud and live | all of waves 2 to 4 for tasks |
| `horizon-acceptance` | H1 to H11 live across a restart and an install; then the same four asks to Claude Code | horizon HZ12 | new · i3z0 | M* | cloud and live | all horizon rows |
| **runs R2, R3, R4** | SWE-bench Verified 100; an Opus 5.5 spot check; real work | coding §6.2 | new · 7gir | | EC2; here | run R1's build |

## Wave 5: after v1, each its own lane

| Row | What the owner gets | From | Issue | Size |
|---|---|---|---|---|
| `own-benchmark` a, b, c | Theseus replays the owner's real turns on cheaper models and says which model each kind of work needs (`theseus route report`) | killer R15 to R17 | new · iacs | 3 rows; R15 bumps the format |
| `earned-autonomy` | a weekly card: "you approved all 57 of these since Oct 1; let it run with a notice?" | killer R18 | new · iacs | M* |
| `fork-worktree`, `fork-surfaces` | a fork gets its own git worktree; "Fork from here" in the TUI and Discord | killer R20, R21 | new · iacs | M*, S* |
| `postmortem` | one command writes the night's timeline from the ledger | killer R23 | new · iacs | M* |
| `mcp-memory` | Claude Code and other agents ask Theseus's cited, private memory over MCP | killer R24 | new · iacs | S* |
| `ledger-chain` | a tamper-evident ledger with a verifier | killer R25 | new · iacs | M*; bumps the format |
| `discord-invitees` | project channels shared with named collaborators | discord2 step 16 | new · bh3t | M*; when the owner asks |
| spine rows | 35b (lessons), theseus-ongv (the AWS hands, at the owner's go) | killer §5.3 | as filed | |

**v1 first.** None of these rows takes a spine slot, and v1's four conditions stand (7 days of the owner's daily use,
no serious bug, every speed target met, each b5 loss explained or fixed). Section 1.1 meets the fourth on paper.
**Pace:** main took 19 to 56 merges a day from Oct 4 to 8 (`git log`); at half that rate for these rows, waves 1 to 3
are about two weeks and the whole plan about four. That is an estimate: reviews, joins and the cloud's 16 slots set
the real pace.

---

# 3. Measurement: the yardsticks, in order, with what they cost

Every target in section 1 has a yardstick. Most cost nothing in model money (a stand-in model, a fake Discord, the
ledger); the coding runs and the head-to-head exams are where the money goes.

| # | Yardstick | Proves | When | Model $ | Where |
|---|---|---|---|---|---|
| 1 | **The baselines we already have:** F10 8 of 42, the speed head-to-head, the horizon live run, the personality probes | today's column of section 1 | done tonight | $0.45 + $0 + $0.11 + $0.70 (delight §3.1; scale; horizon §3.4; personality App. B) | here |
| 2 | **PX, first run** (`px-exam`) | P1 to P8, before any personality row | as soon as `px-exam` joins | $5 to $10 a run | local |
| 3 | **The recall bench, Claude Code and Pi arms** (built, never run) | M1, before the memory rows | after `recall-plan-fix` | within $50 to $120, with rows 5 and 11 | local |
| 4 | **Run R0**: Terminal-Bench, 89 tasks × 3, Theseus against Claude Code, bench-fair settings | T1, T2 interim (±3.9 to 4.9 points); T4, T5 with effort matched; each fixed fault's symptom gone | after `spawn-ask-follow` and `bench-parity` | about $72 | EC2 if a 5-task smoke (about $2) passes; else here at night with no gate |
| 5 | **The routed arm** beside the fixed arm, on R0's tasks | that routing saves money at the same pass rate (killer R3) | with or after R0 | within $50 to $120 | as R0 |
| 6 | **Each row's own proof**: a stand-in run for T6, T7, T8; F10's steps for each delight row; the fake Discord for each Discord step; the planted-secret corpus | the row does what it says, and FAST holds | with every row | $0, plus up to $3 of real-model illustrations | scratch daemons |
| 7 | **The head-to-head speed bench** (`h2h-bench`) | speed T1 to T9 on the release build; counted rows (syncs, request bytes) judged in the gate, timed rows recorded | when `h2h-bench` joins, again after the sync budget | $0 (stand-in) | here, on a quiet machine |
| 8 | **The scale benches**: concurrency, a job burst, a soak, the sessions axis, a 10,000-node session | S1 to S7 | with each scale row | $0 | here |
| 9 | **Run R1**: Terminal-Bench at 5 attempts (Claude Code topped up from R0) | the parity claim, T1 and T2; T4 after `tool-input-fit`; no regression from `job-handles` or compaction | after coding's wave 1 and 2 rows | about $85 | as R0 |
| 10 | **The compaction-survival bench** (in `standing-capture`) and the exam's attribution column (in `provable-memory-b`) | M2, M4 | with those rows | not priced in the killer report; it has Claude Code and Codex arms, so price it first | local |
| 11 | **The kill test**: kill -9 mid-task, Theseus against Claude Code, Pi, OpenCode | "durable by construction" (killer R22; theseus-7gir.16) | any time | within $50 to $120 | local |
| 12 | **PX again**, and the owner's 20 blind pairs | P1 to P8, after personality waves 1 and 2 | per wave, then monthly | $5 to $10 a run | local |
| 13 | **F10 head to head**, then **the owner's 20-minute trial** | delight's match, beat and trial | when the chat's first two waves join | the Claude Code arm capped at $1 a run | local, in the owner's own terminal |
| 14 | **Run R2**: SWE-bench Verified, a fixed stratified 100 × 3 (optional: an arm with LSP on, for decision D7) | T3; the LSP question | on R1's build | $210 to $510 (+$105 to $255) | EC2 only (images of 1 to 3 GB each) |
| 15 | **Run R3**: Opus 5.5 on the 30 hard tasks × 3 | that a Sonnet result carries to the owner's model | after R1 | $70 to $120 | EC2 or here at night |
| 16 | **The horizon acceptance** (`horizon-acceptance`) and the task plan's six tests (`acceptance-now`) | H1 to H11; the six "now" tests | at the end of their waves | $0 (stand-in), cents for Claude Code's four asks | scratch daemons, then the owner's with an OK |
| 17 | **Run R4**: real work. Ten of our own past rows replayed blind; then the owner's own coding sessions on both harnesses, 2 to 3 weeks | T9, T10: the claim the owner cares about | replays on R1's build; sessions after wave 2 and the chat | $100 to $250 for the replays | here |

**In all:** about **$610 to $1,200** of model money over the program, plus $105 to $255 if the LSP arm runs. Coding is
most of it ($540 to $1,040, coding §8.2). **R0 and R1 settle the Terminal-Bench question for about $160** and go first;
R2 and R3 wait for R0, because R0's cost per trial at matched effort firms up their estimates (today's are guesses).

**Benchmarks improve, never max.** The rules every yardstick above follows:
- Every run ends with an improvement assessment beside its report, and every proposal must pass "would we ship it
  with no benchmark?" (the standing rule since Oct 7).
- No instruction goes into the prompt for a benchmark (coding D1). If R1 still shows a hard-task gap, the next step is
  10 attempts on those 8 tasks in both arms (about $25) and a blind side-by-side read, never a prompt (coding §9).
- The attempt plan is fixed before a run, from earlier data, never from the run itself (coding §6.1).
- The personality exam's probes rotate and are never shown to the model; the owner's blind picks are the ground truth
  (personality §2). F10 gains a check only when a new moment makes real use better (delight R7).
- Nothing is claimed in public but rows measured on the release build, each with its report (scale D-7). Each run's
  report goes in `docs/benchmarks/` with its SVG charts.

**What parity on the benchmarks would not prove** (coding §6.4): interactive coding (plan, steer, interrupt, review),
very long sessions, large non-Python codebases, the experience, other models, or anything about memorisation. That is
why R4 and the owner's own trial carry the final word, and why waves 1 and 2 must pass on real-world grounds alone.

---

# 4. The owner's decisions, consolidated

The reports carry 69 decisions (coding 11, delight 10, personality 9 of which 3 need the owner, horizon 13, scale 7,
discord2 9, killer 10), plus the task plan's settled choices. Merged where two reports ask the same thing, they come to **seven that block
work**, 60 defaults that stand unless the owner objects, and a few open questions with no default yet.

## 4.1 The seven that block work (this week)

| # | Decision | Proposed default | What it unblocks | From |
|---|---|---|---|---|
| **B1** | **The measurement budget** | Approve runs R0 and R1 now (about $160, Theseus against Claude Code on Terminal-Bench); the first personality exam ($5 to $10); the recall bench, routed arm and kill test ($50 to $120 together); F10's Claude Code arm (capped at $1 a run). The SWE-bench and Opus runs (R2, R3: $280 to $630) only after R0 firms up the price per trial. Runs go to EC2 through Harbor if a 5-task smoke (about $2) passes, else this machine at night with no gate running | every head-to-head verdict in the scorecard; the coding answer | coding D9, D10; killer OD3; personality OD9; delight D10 |
| **B2** | **Whose persona** | Seeded from the current assistant's values and voice (candour first, evidence, no flattery, dry humour that knows when to stay quiet, sign what you publish, privacy sacred), written in Theseus's own words under Theseus's own name and story; a short owner card; the owner reads the 1 to 2 KB file before it goes live. Two config lines; rollback is deleting them | `persona-on` tonight; `persona-data`; the words every surface uses when Theseus speaks first | personality OD1 |
| **B3** | **Discord: one click, and a house** | Re-authorize the bot once: 12 more permissions (Manage Channels, Manage Webhooks, Manage Threads, Create Public Threads, Send Messages in Threads, Pin Messages, Create Events, Set Voice Channel Status, Change Nickname, Connect, Speak, Send Voice Messages), never Administrator or Manage Roles. `/setup` builds a "Theseus" category in the owner's trusted guild, seen by the owner and the bot only: a board, a task forum, routines, journal, ledger and health channels, a voice room. Live checks run in a test category a scratch daemon makes and removes | `discord-house`, then `discord-board`, `discord-task-posts`, the journal, ledger and health, and horizon's Discord views | discord2 OD3, OD4, OD8 |
| **B4** | **Which project files, trusted how** (the two reports differ: coding reads both files, delight reads AGENTS.md *else* CLAUDE.md after a one-time trust) | Merged: read AGENTS.md **and** CLAUDE.md when both exist (identical texts once, by digest), from the repo root down to the session's directory, nested ones on first touch, plus a user-level file; framed "follow them; the owner's current request wins"; never in a shared place; **each new repository asks once** ("read this project's instructions and work here?"), and the answer is kept | `project-context`: coding's T7 (0 of 5 today) and delight's G8 | coding D2; delight 4 |
| **B5** | **How far the gate may loosen** | "Yes, and don't ask again" lasts for the conversation, covers a command prefix or edits inside the project, is listed and revocable, and never passes the floor or acts in a shared place. Modes (ask, edits, plan, auto = notify) last for the conversation. The model may create a routine behind one card (Create, Try it now, Edit, Decline). A routine's named tools run without asking even after the run reads an email or a page; everything else still waits, and a run that would wait ends "blocked" with the reason. Watches may run commands in the sandbox (L1), approved once at creation, for 30 days at most. Each of these rows carries a security review | `scoped-grants`, `modes`, `routines`, `routine-trust`, `watches`: F10's step S5 and horizon's H6 | delight 5, 9; horizon D-H3, D-H6, D-H9 |
| **B6** | **What bare `theseus` does** | On a terminal it opens a new conversation for this project, inline in the terminal's own scrollback; `-c` continues this project's last, `-r` picks one. A pipe, `--help` or no terminal keeps today's usage text and exit 2, so scripts don't change. (This replaces the task plan's settled choice 1.8 #6, "bare `theseus` stays the usage page".) | `chat`, then `chat-sessions`, `chat-commands`, `rewind`, `modes` | delight 1 to 3 |
| **B7** | **What commands see** | `proc_run` inherits the daemon's environment minus a deny-list (`OP_*`, `AWS_*`, `ANTHROPIC*`, `THESEUS*`, any name with TOKEN, SECRET, PASSWORD or KEY, and every value the vault resolved), plus the login shell's environment taken once after the daemon serves (2 s deadline). Only after the scrubber learns database URLs and the other new key shapes. Health names what was withheld, never a value | `proc-env`: venvs, nvm and `JAVA_HOME` work as in the owner's shell | coding D4 |

## 4.2 Defaults that stand unless the owner objects

**Money and cost**
1. **The DM's cold-cache cost** (theseus-ezeg, asked of the owner at 22:5x; it costs money every day). Fable, Opus and
   Haiku cache for only 5 minutes (their profiles set no `cache_ttl` and don't inherit `[model]`'s 1 hour), so a
   message after more than 5 quiet minutes rewrites the DM's ~626K-token prompt, about $7.85 on Fable 5.1; 1 hour is
   Anthropic's longest cache. *Proposed:* (a) the 1-hour cache on `fable` and `opus` **with** an hourly keep-warm read
   for up to 24 hours after the last message (Fable reads are $0.25 per million: about $0.16 an hour at 626K, about
   $3.75 a day, against $7.85 to $12.50 per cold rewrite; from Oct 7 evening to now about $43 of rewrites would have
   been about $16), never the 1-hour cache alone (its writes cost 2x, and most of the owner's gaps are overnight, so
   alone it would have cost about $7 more); (b) at a cold rewrite, stub big old attachments, re-read on demand (about
   $2.50 instead of $7.85); (c) profiles inherit `[model]`'s `cache_ttl` unless they set their own, and health lists
   each profile's. A chat working window (coding D3's `[context] working_window_tokens`) remains the longer-term cap.
2. A routine's money: $0.50 a run and $10 a month, reset on the 1st; a spent month skips runs and says so once (D-H4).
3. The replay day cap for "your own benchmark": $1 a day (killer OD9).
4. Nothing claimed in public but rows measured on the release build, each with its report (scale D-7).
5. The README's money promise, true again now that the daily ceiling joined: "every call's worst case is reserved
   before it runs, and a hard daily ceiling stops it" (killer OD4).

**Coding**
6. No coding-method text in the prompt: give the model facts, not method (D1).
7. The working window for compaction: the model's full window; a key lowers it (D3).
8. `proc_run` accepts a `command` shell line beside `argv` (D5).
9. `fs_read`'s own cap: 100,000 characters, contiguous; other tools keep 30,000, head and tail (D6).
10. LSP off by default until run R2's A/B, then on where a server resolves if it helps and fits FAST (D7). (The
    owner's own daemon has `[lsp]` on: an edit there can wait up to 1.5 s for its diagnostics, scale §7.)
11. Widen the early-stop nudge (26b) to conversations and one-shot `ask`: live, at most one nudge a turn, with a
    switch (D8).
12. A waited helper keeps the exact-quotes rule for its brief (D11).

**Delight**
13. `theseus` starts a new conversation; `-c` continues this project's last (delight 2).
14. The chat is inline in the terminal's scrollback; the TUI stays the full-screen board (delight 3).
15. `!` output is attached to the next message, with no model call of its own (delight 6).
16. Rewind restores only files Theseus's own edit tools changed, and lists the rest (delight 7).
17. Highlighting: a small built-in highlighter for about ten languages, with no start-up cost (delight 8).
18. No IDE integration for now; revisit after the owner's trial (delight 10).

**Personality**
19. Initiative "normal": promised follow-ups, finished work, and one morning note on days something happened
    (personality OD2 = discord2 OD5).
20. The owner card: up to 30 proposals from the archive, each with its sources, approved one by one; nothing added
    automatically (OD3).
21. Live capture: a notice with Undo when sure (0.8 and over), a question between 0.5 and 0.8, and the same
    correction twice in a week proposed (personality OD4 = killer OD5).
22. The speaking voice: an audition of 4 to 6 voices, picked by ear; the current voice until then (OD5).
23. The register per surface: terse in the terminal, warmer in the DM, brief in voice, neutral in shared channels
    (OD6).
24. Replies signed "Theseus", with the model as metadata (OD7).
25. A generic built-in persona in the public repo for fresh installs, with nothing of the owner's in it (OD8).

**Task horizons**
26. A reminder pings once, with sound, with Done and Snooze; escalated to the DM when no screen saw it (D-H1).
27. A reminder set from a surface lives in the conversation bound to the DM, the Home (D-H2).
28. Routine reports speak in the DM, quieter per routine with "only when notable"; the run log in the routines forum
    (D-H5).
29. The guild calendar (Discord scheduled events) off until asked (D-H7 = discord2 OD9).
30. Limits: 365 days ahead; 5 check-ins per conversation, 100 reminders, 50 routines, 20 watches (D-H8).
31. The first live routine: Evening Haiku, after `routine-trust` (D-H10). *Adjusted here:* disabling its OpenClaw job
    is an OpenClaw change, frozen this sprint, so the two run side by side and the OpenClaw job is disabled only after
    the freeze, with the owner's OK.
32. After downtime: one catch-up run for spans of an hour or more, none for shorter ones (D-H11).
33. The weekly review: Monday 08:00 (D-H12).
34. The words: "reminders", "routines", "watches", "projects" on every surface (D-H13).

**Speed and scale**
35. The model call's dispatch sync overlaps its request: a crash in that ~7 ms window can leave one call sent but
    unrecorded (cents, never acted on). Model calls only, never a tool that acts (D-1).
36. At most 64 jobs at once (sandboxed ones count double), queued beyond with a notice; all jobs' processes capped
    together below the unit's limit, so the daemon keeps room for its own threads (D-2).
37. The embedding model stays Nomic v1.5 at full precision (half precision is 2.4 to 2.9x slower here); backfill
    threads follow the machine's pressure (D-3).
38. Keep the local WAL forever; delete S3's tail objects once their sealed segment is confirmed (D-4).
39. `MemoryLow=256M` on `theseusd.service`, to keep the daemon out of swap (a change to the owner's unit) (D-5).
40. The gate judges counted rows (syncs, frames, request bytes) and records timed ones; the Discord in-flight budget
    is 150 ms (D-6; theseus-fsug).

**Discord**
41. The chat pings as today; silence is opt-in per category and per place; a question already closed is silent (OD1).
42. Thinking `live` (shown while it streams, folded when the answer starts), tools `on`, status reactions `auto`
    (OD2).
43. What Theseus says first: the 15 events at initiative "normal"; installs and restarts the owner asked for go to
    the health channel only (OD5, merged with item 19).
44. The night hold, 23:00 to 07:00: what can wait waits for the morning note; questions, failures of the owner's own
    work and the owner's reminders still ping (OD6).
45. Each task's post under its own name and avatar, its log streamed live (a summary if that proves too busy) (OD7).
46. Voice notes off until asked (OD9; the calendar is item 29).

**Memory and the killer features** (settled in the killer plan, plan §2.4; they stand)
47. Recall on by default in the public template, private places only, no Jev key needed (OD1).
48. The judge on, live, whenever a Jev key resolves; setup asks for the key (OD2, as plan §2.4 settled it).
49. Citations shown in private places, never in shared ones (OD6).
50. Importing this machine's Claude Code history (19,304 files, 9.3 GB) is asked again when `import-a` is ready
    (OD7: "saved for later, as your call").
51. Fact recall's light containment: a label, correction notes and one card; no quarantine machinery (OD8).
52. Rewind's file pre-images (OD10 and delight E7 differ: 1 MiB against 4 MB a file). *Merged here:* 1 MiB a file,
    larger files listed and not kept, because the C: drive never shrinks.

**The task plan's settled choices** (plan §1.8; they stand)
53. A failed task stays flagged until dismissed; Retry makes a new linked task.
54. On terminal surfaces, sounds only for questions, failures and reminders; a finished task is silent unless flagged
    or 5 minutes or longer. (Discord follows item 41.)
55. Tasks may ask multiple-choice questions, which expire to their default after 30 minutes.
56. The DM is the always-on channel: a question pings there after 60 s if no Theseus screen is open, after 5 minutes
    if one is.
57. Windows toasts from WSL on by default; the first live test pops one only with the owner's OK.
58. "Seen" is shared by every terminal client on the machine.
59. `/stop` keeps its meaning (this conversation's own work); a new "stop all" also cancels its tasks.
60. herdr: a conversation gets a tab only while it needs the owner; a finished task's tab closes 10 minutes after it
    is seen; herdr's toast names the task. (The owner approved the herdr config: toasts and the attention sort applied
    at 19:52; the rows, keys and plugin go in when the herdr rows land.)

**Superseded tonight:** the task plan's 1.8 #6 (bare `theseus` stays the usage page) gives way to B6; its 1.8 #8
(Discord silent by default, a board pinned in the DM, digests at 08:00 and 18:00) gives way to discord2's OD1 to OD9
(the DM pin stays as the no-guild fallback); the first Discord pass's 13 steps give way to discord2's 16.

## 4.3 Open, with no default yet

- **theseus-7kha:** should `/stop` and a cancel also end what `proc.run` left running, or only what a terminal left?
- **theseus-qags:** more harness arms (Codex CLI, Aider, OpenCode, OpenHands, OpenClaw) on the rerun's tasks.
- **Self-reflection:** may Theseus propose edits to its own persona once a month, from the ledger (corrections,
  thumbs-down, repeated asks), as a question the owner approves? Cheap and off the turn path (personality §7).
- **Outside any project:** what bare `theseus` opens in the home directory (proposed: a "home" conversation the DM
  shares), and `/follow`, a chat mirrored into the DM (after the always-on DM) (delight §7).
- **Routines:** may a run set one-shot wakes (proposed yes); does "stop all" pause routines (proposed no) (horizon §7).
- **The journal's gate:** what the owner says about themselves is kept; what Theseus infers waits for Keep (proposed,
  discord2 §6).

---

# 5. Risks and costs

**Money: two pools, both real.**
- *The Claude account* pays for every cloud row and every local helper. It hit its monthly spend limit at about 20:01
  tonight and stopped every helper at once; the owner resumed at about 20:15. The API shows no cost per cloud row (the
  owner sees the balance on claude.ai), so the plan's ~110 cloud rows can't be priced from here. Mitigation: launch
  in the order of section 2, keep the cloud at its 16 rows in flight and no more, and look at the balance after the
  first eight rows.
- *The API keys* pay for the runs and for the owner's daemon. The daemon stops model calls at its $200 daily ceiling
  (theseus-kp20, joined). The runs are about $610 to $1,200 over weeks, R0 and R1 (about $160) first; the bench's own
  key has its own limits, to check before R0 (coding §9). The owner's DM costs about $7.85 a message after more than 5
  idle minutes today (Fable caches 5 minutes; section 4.2, item 1): that alone is about 25 messages to the daily ceiling.

**Session caps: keep local fan-out small.** OpenClaw allows 16 live Claude sessions across the whole gateway, every
agent included. Tonight five investigators that each fanned out 2 to 5 helpers filled it; three spawns were refused
and another agent's heartbeat fell back to another provider's model. Cloud rows don't count against it, so the build
runs in the cloud; local work (joins, reviews, harvests, the exams' Claude Code arms) gets briefs that allow at most
two helpers, and about three slots stay free for heartbeats.

**Store format bumps move one way.** Rows that bump it: `session-dir` or `project-context` (a session's directory),
`standing` (the order record), `reminders` (one bump for every wake kind), `projects` (the task record), `rewind`
(file pre-images), and later `own-benchmark` and `ledger-chain`. Each bump changes the same two goldens
(`core_output.txt`, `kernel_frames.txt`) and takes its number at landing (main is at format 25), so bumps join one at
a time, and rows that can share one should.

**The owner's daemon is read-only until an install.** Every proof runs on a scratch daemon of the row's own build.
Installs and restarts are at this side's discretion (the owner's standing OK), but a config value the owner hasn't
approved still needs a yes: the persona's two lines (B2), `MemoryLow` on the unit (item 39), the routines' grants
(B5). The horizon acceptance's last step runs on the owner's daemon only with an OK.

**Hot files and ceilings.** The CLI's `render.rs` (3,128 of 3,128 lines), the protocol's `lib.rs`, `config.rs`,
`kernel.rs` and Discord's `render.rs` and `runtime.rs` are at or near their line ceilings, so new code goes in new
modules. Several rows meet in the same files: `compiler.rs`'s ring (`one-turn-compaction`, `long-session-o-new`), its
blocks (`session-dir`, `project-context`, `standing`, `commitments`, `surface-tag`), `fs.rs` (`output-kept`,
`file-tools-polish`), the CLI (wave A, `term-render`, `chat`, `cli-pages`), Discord's renderer (`discord-show`,
`voice-first`, `discord-path`). Each row rehearses its merge on the current main, as the batch procedure does; the
plan sequences the worst pairs. **The prompt cache has one owner:** Theseus uses 3 of the provider's 4 cache
breakpoints; `session-dir`'s per-session block takes the fourth, and no other row may take one (the persona and
standing items stay in blocks 1 and 2; the commitments block rides the last block).

**Loosening the gate.** Grants, modes and routines' pre-approved tools trade asking for flow, and a granted
`channel.post` after reading an email could post what the email asked. Each such row carries a security review and
red-team tests, never passes the floor and never acts in a shared place (delight R4; horizon §7).

**Someone else's text.** A cloned repository's AGENTS.md is a prompt-injection path, hence B4's one-time trust per
repository (delight R5). Imported histories carry secrets and other people's words, hence the scrubber on the way in
and private places only (killer §6).

**The benchmarks can't prove the most valuable rows.** Terminal-Bench barely touches project files, the working
directory, long instructions or long commands; R4 and the owner's own trial are the real measures (coding §9). And
the baseline moves: Claude Code went from 2.1.290 to 2.1.295 in days and auto-updated during one investigation, so a
comparison pools across runs only with the same pin and settings.

**Too much voice.** A lively chat that also speaks first could become more pings than the owner wants. The levers are
in the design (the owner's turn first, folding, the night hold, a ceiling per kind, silence per category) and a week
of real use tunes them. The owner has said nothing Theseus has said so far was not worth a buzz.

**Disk.** `output-kept` keeps up to 2 GiB of outputs on C:, and WSL's disk never shrinks (a full C: froze the machine
on Oct 1); SWE-bench's images (1 to 3 GB each) run off this machine; a year's WAL must stay restorable (`memory-flat`).

**Thin evidence in places.** The personality probes are one sample each; the delight run's stand-in model could not
stream or think; the speed runs ran at load 3.5 to 13 from neighbours' builds. Each report says so, and each yardstick
in section 3 reruns its numbers on a quiet machine before anything is claimed.

**Two agents, two characters.** Seeding Theseus's persona from the current assistant's values must not make it a copy:
it keeps its own name, story and voice (personality §7).

---

# 6. A side note: OpenClaw's Morning Briefing

Reading OpenClaw's cron log tonight (read-only, horizon §3.4), the horizon investigator found the owner's **Morning
Briefing has failed 6 runs in a row with no alert**: first Gmail behind a locked keyring, then security blocks. Four
of the five failing jobs on this machine stop on an approval or security wall in the middle of an unattended run, and
none of them has a failure alert set, so a failure tells no one. It is a live example of why horizon
management matters: an unattended run needs its authority settled when it is created, a "Try it now" while the owner
watches, and a failure that speaks (`routine-trust`, horizon §5.4). OpenClaw changes are frozen for this sprint, so it
is noted here and not fixed.

---

# Sources and method

- **The references.** "coding §3" means `~/reports/theseus-ns/coding/report.md`, section 3; likewise `delight`,
  `personality`, `horizon` and `scale` under `~/reports/theseus-ns/`. `discord2` is
  `~/reports/theseus-l00z/discord2/report.md`, `plan` is `~/reports/theseus-l00z/plan.md`, and `killer` is
  `~/reports/theseus-l00z/killer/report.md`.
- **Read, whole:** the five north-star investigations (`coding`, `delight`, `personality`, `horizon`, `scale`, all
  complete tonight), the Discord redesign (`discord2`, which supersedes the Discord parts of `plan`), the task plan and
  the killer-features report; the north-star context (`contexts/theseus-north-star.md`); the Beads issues
  theseus-3iwi and its children 7m7w, wy6v, slvl, i3z0, c892, l00z and iacs, with the issues they name.
- **Nothing measured anew** and no model call made for this synthesis; one read-only `git log` for main's head and
  its merges per day. Where two reports disagree, section 4 says so and gives a merged default, marked as this
  synthesis's.
- **Read-only on Beads:** no issue was created or changed. The DM thread files the "new" rows after the owner reads.
- **No person's name** appears here: "the owner" throughout.

*Written by Tabitha/Claude, 2026-10-08, 22:36 to 23:05 MST. The PDF beside this file is rendered by `render.mjs`
(micromark, then headless Chrome); its pages were checked as images with PyMuPDF, as `pdftoppm` is not installed.*
