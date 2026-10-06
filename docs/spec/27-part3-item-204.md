# The Ship of Theseus, chapter 27: Part III, A4's Items 204 to 215 ([index](README.md))
### Item 204. The scrubber withholds a secret printed JSON-escaped: an output holding a backslash is decoded once as JSON decodes it, every board value is searched in the decoded text, and a match is withheld as whole escapes (theseus-ubp7; the ninth cloud batch's scrub-escaped session, fired 2026-10-05 20:06 from 4a449460, Opus 5.5; e915c133, 94ed9fc8 and c1394b36; reviewed 2026-10-05 23:01 to 2026-10-06 01:06 by local reviewer R26, stack B9-tools, the first of three; joined 2026-10-06 10:32 at 2b342594, a signed merge onto 21bf5454, by the tools-stack joiner, relaunched after the account's weekly limit stopped its first run at 01:52; installed 2026-10-06 13:03 at f589d9cb, install #7)

**Why.** The scrubber (§3.19's scrubbing, Item 44) matched each board value verbatim, in base64 and percent-encoded.
A tool that prints JSON holding a secret with a quote, a backslash, a control character or a non-ASCII character
shows the value JSON-escaped, and that form was not matched, so the value could reach a node, the WAL and the model.
Cloud aws-curated's planted revert (Item 97, C3.2) found it: `aws.call` printed a secret as pretty JSON. C3's handles
mask secrets before the scrubber, so AWS results were covered, but no other JSON-printing tool was.

**What landed** (theseus-core's `scrub/`; the merge 5 files, +394 −6, without the cloud files; no new package, no
config key, no protocol type, no store format change (23 stays 23)).
- **The escaped matcher** (e915c133). `scrub/escaped.rs` (new, 126 lines). On an output holding a backslash, the
  text is decoded once, left to right, as JSON decodes it: `\"`, `\\`, `\/`, `\b`, `\f`, `\n`, `\r`, `\t`, `\u` with
  four hex digits in either case, and a high surrogate followed by a low one as one character. Each escape keeps its
  place in the text and in the decoded text (a short list, one entry per escape, not a map per byte). Every board value
  is searched in the decoded text (`match_indices`), and a match is mapped back, a start inside an escape to the
  escape's start and an end to its end, so the span replaced is whole escapes and no half of one remains. A backslash
  that starts no escape (`\x`, `\u12`, a trailing one) and a lone surrogate are kept as they are, and the scan goes on
  at the next byte. One call in `scrub.rs`'s `encoded`, after the base64 and percent spans and into the same `splice`
  (overlapping spans keep the earliest start, the longest at a tie), so it runs before the shapes and counts in what
  `scrub` returns. theseus-core's AGENTS.md "Secrets" line names it.
- **Decode, not fixed forms.** The brief let the pass skip every value with no quote, backslash, control or non-ASCII
  character. The session refused that, rightly (R26): any character may be written as a `\u` escape (`Inv…`), Go
  escapes `<>&` and PHP `/`, so the pass searches every value and skips only outputs with no backslash, or with no
  valid escape. A fixed set of forms per value would miss every mix an encoder makes; decoding has one form.
- **Rust's debug form** (c1394b36). `{:?}` of a string escapes `"`, `\`, tab, CR and LF as JSON does, so it is caught
  for free; one test holds it. Its `\0` and `\u{…}` are not JSON's and pass.
- **Its cost** (94ed9fc8): `scrub_cost`, an ignored timing test of 64 KB outputs.
- The judge's own scrub (`ScrubWith`) and theseus-discord wrap the same `Scrubber`, and `toolrun::result_node_in`
  scrubs before the cap, so all gain the pass with no edit, and a cut cannot split a value inside one output.

**How it is proven.**
- **The session's tests** (`scrub/tests_escaped.rs`, 243 lines, a file of its own since `scrub.rs` would have passed
  1,000 lines): six values (a quote, a backslash, a tab, a newline, an accented letter, U+1F600) inside an object by
  `serde_json`'s compact and pretty printers, each equal to the same object with `[redacted:<name>]` in its place and
  parsing back; Python's `json.dumps` ASCII-only forms in both cases of hex, with surrogate pairs and needless `\u`
  escapes; `\/` and Go's `<>&`; every occurrence counted (two escaped and one verbatim count 3) with a Windows path, a
  lone surrogate and `\uzzzz` left byte for byte; and the debug form. `tests_outside_text`'s property test now plants
  the value JSON-escaped and feeds broken escapes (`\`, `\"`, `\u00`, `\ud83d`) to the no-panic test. The planted
  revert (the pass given no values) failed 5 of the escaped tests and the property test; the existing eight scrub tests
  passed. 58 of 58 scrub, redact and secret tests across the workspace.
- **The review** (R26, on main acf26214, store format 22): `escaped.rs` read against every escape JSON has, short and
  four-hex in either case, surrogate pairs, lone surrogates and broken escapes: no way found for half an escape or a
  decoded character to slip through. The build clean (the golden unchanged and passing); **59 of 59** scrub, redact and
  secret tests; **4 of 4 planted reverts caught**: the pass given no values (6 fail), a surrogate pair decoded one code
  point off, the debug form skipped, and R26's own, a match's start mapped past its escape, which leaves the value's
  first letter showing (`"\\u0049[redacted:accent]"`). The whole stack's workspace suite, merged with main c07bbe6a:
  2,885 of 2,885 (in review-b9tools).
- **Live** (R26, scratch daemons of the pair's build and main's, fresh state dirs, the stand-in model): a `file:`
  secret holding the invented `Inv"ent\ed-pröbe-9` (a quote, a backslash, an accented letter; mode 600, after a first
  try at 664 was refused, "it must not be group- or world-readable"). The report's three `json.dumps` prints (plain,
  `indent=2`, `ensure_ascii=False`) each came back `{"v": "[redacted:probe]"}` with no form of the value in the history,
  and no file of the state dir held the value, its stem or its escaped forms. On main all three showed it
  (`Inv\"ent\\ed-pröbe-9`), and main's WAL segment held the stem.
- **FAST.** Every tool output passes `scrub`. Against main on 64 KB outputs, in one hold, alternated: plain text +28 µs
  (+5 %, inside the spread), pretty JSON with no backslash −62 µs (noise), JSON with about 7,800 escapes **+90 µs
  (+16 %)** (the cloud's 4-core VM: +185 µs, +30 %). A scan for a backslash costs about 4 µs on 64 KB, so an output
  with none pays nothing measurable. The stack's turn A/B (main, the pair, the stack; four runs an arm in palindrome
  order): plain −1.4 ms, tool call +12.6 ms wall with equal minimums, inside each arm's spread; frames 5 and 9 on every
  arm.

**What the session and the review found.** The session's step 3 weighed other encodings (YAML's double quotes,
Python's `repr`, shell quoting, HTML entities, a form body's `+`, hex dumps) and recommended JSON escaped twice next.
R26 probed the open ones through the real `Scrubber` and live: **JSON escaped twice** (kubectl's
`last-applied-configuration`, CloudTrail and CloudWatch events, API Gateway bodies), **YAML's `\xF6`** and **Python's
`repr` of a value holding both quotes** pass (theseus-nlvx, P2), and **base64 wrapped by `\n` escapes inside a JSON
string** passes, the run ending at the backslash (theseus-cjyt, P2; GitHub's contents API prints a file's base64 that
way). Both are gaps main has too, and the branch closes the common one. An aside of the session's: `scrub` itself costs
0.7 ms on 64 KB of plain text and 1.7 ms on pretty JSON before this change (base64's run finder and `percent_spans`
walking every byte for every value look like where it goes; not profiled, no issue).

**The join** (B9-tools' first). The stack's first joiner took its three locks a second apart at 01:29:28 to :30 and
was stopped at 01:52 by the account's weekly limit while it waited, with nothing merged; a 02:30 recovery died the
same way. The DM thread re-armed the locks at 08:05:42, and the relaunched joiner started at 08:08. Ahead of it landed
the bench stack (08:19), soul-import (09:11), voice-turns (09:37, a lock the DM thread placed at 09:05 at queue time
01:27:00), learning-fixes (09:58) and install #6 (a lock placed at 09:52 at queue time 01:28:30, done 10:19:31): **2 h
10 min in the queue.** Its dry runs on each new main were clean. The merge (10:19:50 onto 21bf5454): no conflict
(AGENTS.md auto-merged), no resolve.py, no join fix; the staged tree equalled R26's dry run on the same main; 5 files,
+394 −6; `MANIFEST_FORMAT` 23, main's. The warm (to 10:23:03; the test build 2 m 20 s, clippy clean); the scrub, redact
and secret tests, **61 of 61** (R26's 59 plus smalls' two publish tests now on main). The signed merge **2b342594**
(21bf5454 and 21fb40df), 10:25:16. Its gate (10:25:26 to 10:32:14, ok; 26 s of lock wait, the gate lock held shared by
a reviewer's busy-loop load run and another reviewer's test run): **2,938 of 2,938** (1 slow, 24 skipped;
learning-fixes' 2,933 plus this branch's 5), no hour crossed; lifecycle in every budget (cold start p50 21.9 / p95
30.3 ms; from the config copy 25.3 / 27.4; clean shutdown 33.7 / 48.4; a post in flight 76.0 / 96.0; SIGKILL then
restart 26.2 / 27.1; binary swap 46.2 / 52.6; restore 133.1 / 141.5; the daemon's own serving 19.98 / 26.10); L1
start p50 6.15 ms; turn frames 5 and 9, plain p50 75.6 ms, tool call 147.5 ms. Pushed 10:32:41, the branch deleted,
done line 10:32:49; theseus-ubp7 closed with the hash. The store stays at format 23.

**The install** (installed 2026-10-06 13:03 at f589d9cb, install #7). On Eddie's daemon a secret printed JSON-escaped (any of its characters: serde's and
Python's `json.dumps` forms, `\/`, `\u` escapes, Rust's `{:?}`) is withheld before the model and the WAL, where the
build before let it through. No config key; nothing to do. The two open encodings (theseus-nlvx, theseus-cjyt) stay
open on his daemon as before. Health after the restart (13:03:47): `theseusd check` exit 0, 9 secrets ready 1.02 s after the start, startup serving at 21.5 ms (at load 11 to 16), `cgroup: delegated`, the judge's live packs as before (`security.v3`, `route.v1`, `rerank.v1`), memory live on the `baseline` arm, voice ready, Discord ready, and no error or warning in the journal; no config change, the store at format 23, the S3 backup caught up again from 13:04:00, and no budget held.

**Divergences.** The brief's shortcut (skip values with no escapable character) was refused for the reason above.
Step 3 is a report and one test, as briefed, not new arms.

**Known gaps.** theseus-nlvx (P2: JSON escaped twice, YAML's `\xNN` and `\U`, Python repr's `\'`; R26: next, the
same decoder run again on outputs that still hold an escape) and theseus-cjyt (P2: base64 in an escaped string; the
fix reuses the decoded text the pass already builds), both since given to batch 10's scrub-encodings task. Left by
R26's advice: shell quoting, HTML entities (`http.fetch`'s reader already decodes them), hex dumps and `+` for a
space; a value split across two outputs (the verbatim pass's limit too). The scrub's own cost before this change is
worth a profile some day.

### Item 205. Gate tests: turns hold the gate's layers (L3's server start, an extension's load floor, a batch's floor step, a ceiling's MCP entry), the block-frame test waits for its `lsp.ready` row, and `policy explain` names L3 (theseus-x1jj, theseus-xx6w, theseus-sh9w, theseus-grms, theseus-cxqj and theseus-t2xr; the ninth cloud batch's gate-tests session, fired 2026-10-05 20:06 from 4a449460, Sonnet 5.5; 8316c22a, e8ad1513, 8906ae3e, 934a3ad7, f65b4e91 and 90f830b1; reviewed 2026-10-05 23:01 to 2026-10-06 01:06 by local reviewer R26, stack B9-tools, the second of three; joined 2026-10-06 10:43 at bd3eb769, a signed merge onto 2b342594, by the tools-stack joiner; installed 2026-10-06 13:03 at f589d9cb, install #7)

**Why.** Six gaps reviews had found, five of them a layer of the gate that a planted revert could drop with every
test passing:
- **theseus-x1jj** (R2, budgets-policy's review, Item 130): lsp-diagnostics' L3 (Item 127: an edit that starts its
  root's language server is judged at `proc.run`'s posture) had only a test of the function; no turn ran its line in
  `toolrun/order.rs`. **theseus-t2xr** (the same review): `policy.explain` had no row or condition for L3.
- **theseus-sh9w** (R2, extensions-load's review, Item 133): an extension's tools run no looser than the floor of the
  ceiling it was loaded under (`extend::load::Floors`), and no test ran that line through the gate.
- **theseus-grms** (R10, proc-steps' review, Item 175): the batch gate takes the strictest step by `(floor, posture)`;
  no test held the floor's half of the key.
- **theseus-cxqj** (R1, bindings-v2's review, Item 117): bindings-v2's join fix filtered a place's MCP tools by its
  ceiling, with no committed test.
- **theseus-xx6w:** `tests_lsp_edits::the_block_adds_no_frame` failed once under load ("with the block 10 frames,
  without 9"): it waited for 100 ms of a still store, a stillness wait standing in for an event.

**What landed** (theseus-core only; the merge 9 files, +487 −21, without the cloud files, one of them code; no new
package, config key, protocol type or store format change (23 stays 23); explain's new row and condition are values
in existing fields).
- **xx6w** (8316c22a). The record that landed inside the counted turn was the server's `lsp.ready` row, written by the
  board's own task (`lsp::readiness`) after `lsp.diagnostics` returns. The session could not reproduce it naturally
  (the old test passed 30 of 30 under the load recipe and 40 of 40 with eight loops) but did deterministically, with a
  130 ms sleep before that record: 3 of 3 failures with exactly the reported message. The test now waits
  (`until_ledgered`, real clock, 60 s bound) until the ledger holds an `lsp.ready` row, then counts; a failure prints
  the turn's row kinds. No `lsp.ready` row can come earlier: the warm-up and "without" turns start no server.
- **x1jj** (e8ad1513). `tests_gate_layers.rs` (new):
  `an_edit_that_starts_its_server_waits_in_a_turn_as_proc_run_would`. A private place's `fs.edit` leaves one card
  whose reason holds `starts fake on`, and nothing is spawned; in a shared place there is no card and the edit runs.
- **t2xr, the one code change** (8906ae3e, `rpc/explain.rs`, +74). The session found more than the brief said:
  explain's probe for any edit was the first workspace root, a directory with no extension, so `board.server_for`
  could never match and L3 never fired there; a row or condition alone would have been dead code. `starts_on_edit`
  lists the servers an edit by this tool can start in a private place (an `lsp` board, `[lsp] edit_diagnostics` on,
  `start_on_edit` servers; `lsp.rename`, whose family is `lsp`, excluded); `edit_probe` points an edit's probe at
  `<root>/explain-probe.<ext>`, the first such server's extension, inside the same root, so the roots, the approve
  lists and the floor read it as they read the root; an `lsp` row appears where L3 raised the posture, and an
  `lsp_start` condition names those servers and `proc.run`'s posture. The order itself is unchanged, written once and
  run by the gate and explain alike.
- **sh9w** (934a3ad7). `extend/tests_load/floor.rs`: an extension acked under `#pier`'s `posture_floor = approve` waits
  in `#den` (private, no ceiling, `[policy.mcp]` open), its reason naming `#pier`'s ceiling; declined, revoked, and
  loaded again from a no-ceiling place with different content, the same call runs with no second card.
- **grms** (f65b4e91). `tests_steps::a_floor_step_outranks_an_earlier_step_on_the_approve_list`: step 1 on
  `approve_argv`, step 2 `op whoami`, both approve; the batch's floor is true, its reason names step 2, and nothing ran.
- **cxqj** (90f830b1). `mcp/tests/ceilings.rs`: `#lab` (`tools = ["mcp:fake"]`) is offered the fake server's five tools
  alone, has the MCP note, and its echo runs; `#den` (`tools = ["web"]`) is offered `web_search` alone, has no MCP
  note, and its echo is refused in the brief's words, never reaching the server. The test binds places with
  `Core::bind_places`, as Discord would.

**How it is proven.**
- **The session:** each new test passed 5 of 5 under the load recipe, and xx6w's 30 of 30; the families 69 of 69;
  theseus-core whole, 1,284 passed. Its plants (L3's line out of `order.rs`, which also fails t2xr's test, since
  explain runs the gate's own order; the `Floors` line out; `stricter` by posture alone; the MCP filter and the note
  ignoring the ceiling; the block in a frame of its own; explain's new arm off) each failed its test.
- **The review** (R26, on scrub-escaped's review commit over main acf26214): the pair's build clean (the golden and
  protocol.gen unchanged); **84 of 84** in the families; **6 of 6 planted reverts caught**, each by its own new test
  (L3's line removed: "the edit waits: []", and explain "fs.edit — open"; `stricter` by posture: the reason names step
  1; the `Floors` line removed: "the call waits for approval: []"; the MCP filter given the place rule alone: `#den`
  offered the five MCP tools; the block in its own frame: "with the block 10 frames, without 9", now with the turn's
  rows printed; explain's L3 arm off: no `lsp` row).
- **Live** (R26, scratch daemons of the pair's build and main's, fresh state dirs, the stand-in model). L3 and explain:
  with `[lsp.servers.fake]` set to start on an edit and none running, `proc.run = "approve"` and `fs.edit = "open"`, an
  `fs_edit` of `work/a.fake` stopped on one card on both builds ("this call starts fake on …, a program, judged as
  proc.run: … approve"; L3 itself was already on main); `theseus policy explain --tool fs.edit` gave **approve** with
  an `lsp` row and an `lsp_start` condition (entries `["fake", "rust-analyzer", "ty", "tsgo"]`) on the branch, and
  **open** with neither on main. The batch floor: one card, floor true, "step 2 of 2 (`op whoami`): … (floor: `op` is
  Theseus's own binary or the 1Password CLI)", and the first step's file not created. The MCP ceilings were not run
  live: only the Discord binding binds places, and the live-check rules keep Discord off.
- **FAST.** Tests, and explain's probe and row, off the turn path.

**What the review found.** explain's `lsp_start` condition names every configured server that starts on an edit,
installed or not (the presets rust-analyzer, ty and tsgo start on an edit by default, so with `[lsp]` on they are
named; live, ty and tsgo were not installed), while the gate skips a server whose program is not on the job's PATH.
And (R26's own) the probe takes the **first** such server's extension: if that one is not installed and a later one
is, L3 does not fire on the probe, and explain says open where an edit of the other server's files would ask; the
condition still names the start. Recommended: filter `starts_on_edit` through the gate's own installed check, a small
follow-up.

**The join** (B9-tools' second). The lock `cloud-gate-tests-join` (01:29:29) cleared at scrub-escaped's done line; the
dry run on 2b342594 (10:33:50) was clean for gate-tests and approvals-batch. The merge (10:33:59 onto 2b342594): no
conflict (lib.rs auto-merged), no resolve.py, no join fix; the staged tree equalled the dry run's; 9 files, +487 −21
(the largest touched `mcp/tests.rs` 1,188 lines, `rpc/explain.rs` 717); `MANIFEST_FORMAT` 23; all 162 `mod` lines
kept. The warm (to 10:35:56; the test build 1 m 25 s, clippy clean); R26's families, **84 of 84**, the same 84 tests by
name. The signed merge **bd3eb769** (2b342594 and 857f6425), 10:36:59. Its gate (10:37:05 to 10:42:54, ok; 1 s of lock
wait): **2,943 of 2,943** (1 slow, 24 skipped; scrub-escaped's 2,938 plus 5), no hour crossed; lifecycle in every
budget (cold start p50 22.7 / p95 28.4 ms; from the config copy 21.7 / 27.6; clean shutdown 32.1 / 54.0; a post in
flight 75.0 / 77.7; SIGKILL then restart 24.6 / 35.5; binary swap 45.2 / 54.2; restore 132.4 / 151.4; serving 19.10 /
25.58); L1 start p50 6.03 ms; turn frames 5 and 9, plain p50 74.0 ms, tool call 156.2 ms. Pushed 10:43:07, the branch
deleted, done line 10:43:19; the six issues closed with the hash. The store stays at format 23.

**The install** (installed 2026-10-06 13:03 at f589d9cb, install #7). Tests, but for one visible change: `theseus policy explain` names L3, with an `lsp`
row where an edit's server start raised the posture and an `lsp_start` condition naming the servers (it also names
servers that are not installed). No config key; the gate itself is unchanged. The MCP ceilings' live check needs
bound places, so R26 left it for install time with the owner's bindings. After install #7's restart (13:03:47; its
report), `theseus policy explain --tool fs.edit` on Eddie's daemon carried the `lsp_start` condition naming
rust-analyzer, ty and tsgo at his `notify` enforcement, and `--tool lsp.diagnostics` listed the `lsp` layer: t2xr's
line, with R26's note standing (servers named whether or not they are installed). Health after the restart (13:03:47): `theseusd check` exit 0, 9 secrets ready 1.02 s after the start, startup serving at 21.5 ms (at load 11 to 16), `cgroup: delegated`, the judge's live packs as before (`security.v3`, `route.v1`, `rerank.v1`), memory live on the `baseline` arm, voice ready, Discord ready, and no error or warning in the journal; no config change, the store at format 23, the S3 backup caught up again from 13:04:00, and no budget held.

**Divergences.** gate-tests was briefed as tests only; t2xr changes what `policy explain` prints. explain's probe
moved from the root to a file named for a server's extension, beyond the brief, since the old probe could never meet
L3.

**Known gaps.** explain's installed check (R26's recommendation above; not filed as an issue). The MCP ceilings, live
with the owner's bindings. No product change for xx6w: the row is the board's, not the turn's.

### Item 206. Approvals in a batch: a declined call ends its batch's waits, and a call is found by its own response, so an approved call runs even when its id repeats an earlier answered call's (theseus-6i0 and theseus-w6uh; the ninth cloud batch's approvals-batch session, fired 2026-10-05 20:06 from 4a449460, Opus 5.5; b009fa0a and 995ad500; reviewed 2026-10-05 23:01 to 2026-10-06 01:06 by local reviewer R26, stack B9-tools, the last of three; joined 2026-10-06 10:54 at 25b0578f, a signed merge onto bd3eb769, by the tools-stack joiner; installed 2026-10-06 13:03 at f589d9cb, install #7)

**Why.**
- **theseus-6i0** (the dogfood pilot, theseus-14s, approvals 1 and 2). One model response held two `fs.write` calls
  outside the builder's roots. The first waited; when it was declined (unanswered for 30 minutes), a continuation
  with no loop settled it and parked at once on the second call's card. `run_calls` stops admitting at the first call
  that asks, and the continuation pushed the rest, all never planned, through `run_fresh` and `run_calls`, which parked
  on the next card: N waiting calls meant N cards in sequence, each waiting its full time, and the model heard the
  decline only after the last.
- **theseus-w6uh** (R1's live check of task-record, Item 125). An approved call never ran when its `tool_use` id
  repeated an earlier answered call's in the same session: the stand-in model then numbered each response's calls
  from `toolu_fake_0`, and resume keyed "answered" by the bare id across the session. A provider that numbers ids per
  response (some compatible proxies do) would meet the same silent skip.

**What landed** (theseus-core's `toolrun/`, `fact/`; the merge 9 files, +710 −134, without the cloud files; no new
package, config key, protocol type, `ResultStatus` or store format change (23 stays 23)).
- **A decline ends the batch's waits** (b009fa0a, 6i0). `run_calls` moved whole from `toolrun.rs` to a new
  `toolrun/calls.rs` (toolrun.rs 2,494 to 2,418 lines; only the call sites and one import changed there), now a
  wrapper of `run_batch(.., declined)`. With `declined` set, an `Admitted::Asks` call is not asked: `not_asked` writes
  its ToolCall node (the gate's record, `needs_confirm`, no correlation id, as an invalid input's has) and its result
  in one append, `Cancelled` with "Not run: an earlier call in this batch was declined." (meta `not_run`), and records
  a new narrative-only fact, `fact::tool::CallNotAsked`. The gating goes on, so calls that need no answer still run.
  No action is planned, so nothing waits in the kernel, no `tool.confirm_requested` row is written and no card is
  posted. `ToolRuntime::resume` sets `declined` once any call of the batch is answered `Done { Declined }`: an
  operator's decline, an expired card, or `run_confirmed`'s "confirmation is no longer valid" (a supersede too, though
  with input every never-planned call is already answered not run). The status is `Cancelled`, not `Declined`, since
  nobody declined that call, so no new `ResultStatus` and no format bump. Each not-asked call is one span under the
  continuation through `ran_batch`, counted once at its answer (Item 180's rule). Its result is stored at admission,
  before a later read's, but the model's request is in call order (the compiler orders results by the call).
- **A call is found by its own response** (995ad500, w6uh). The session wrote the tests first and found the issue's
  own case already passing on main as cloned (the stubs step reads only results after the last assistant message);
  the case main missed was a never-planned call after a waiting one whose id repeated an earlier response's answered
  call (answered from the old call's settled action, "The call settled but its output was lost in a restart", never
  run), and a cancel of that execution, which answered both new calls `Unknown` from the old actions. resume's map had
  kept the **last** ToolCall node with an id and late.rs's cancel lookup the **first**; both were wrong for a repeated
  id. `toolrun/resume.rs::calls_of(nodes, assistant)` maps by `tool_use_id` only the ToolCall nodes after the last
  response whose `assistant_node` is that response, and both use it. No stored field changes (every ToolCall node
  already carries `assistant_node`). Keying, not refusing a repeated id, since an id is unique within its response,
  which is all a turn needs.
- **The golden:** 37 lines of `core_output.txt` moved, all `context.compiled` (ledger, notify, compile span), each
  only gaining `"stubs":#` (checked pair by pair by the session and by R26): the continuation no longer decodes every
  ToolCall node of the session, so later compiles find stubs.
- **Not built:** one card for a whole batch, 6i0's other option. The report lists what it needs (a card listing every
  waiting call; a confirm that binds several actions all or none in one kernel frame; a per-call decline; the later
  calls planned at the first ask; expiry and supersede for the set; the floor and T1's hold still per call).

**How it is proven.**
- **The session's tests** (`tests_approvals.rs`, new, 446 lines): `a_declined_call_ends_its_batchs_waits` (one
  response of [a write to `guarded/a.txt`, a read inside the root, a write to `guarded/b.txt`], the first declined with
  "wrong place": one `tool.confirm_requested` row in all, no pending confirm, neither file; the next request holds
  three results in call order, the decline with its note, the note's content and the exact not-run line; statuses
  `Declined`, `Ok`, `Cancelled`; three spans under `continuation`); `an_approved_call_leaves_the_next_to_ask` (a second,
  distinct card); `an_approved_call_whose_id_an_earlier_response_used_runs`;
  `a_never_planned_call_whose_id_an_earlier_response_used_runs_anew`;
  `a_cancel_answers_the_last_responses_calls_whatever_their_ids`. Writes were made to wait through `[tools]
  approve_paths`, since a write outside the roots takes the tool's posture (theseus-ewi). Under load, 5 of 5 runs.
  Each step's planted revert failed as the bug does (the continuation parked on a second card, `left: ""`; the read
  answered from the old action, the cancel `Unknown`).
- **The review** (R26, on gate-tests' review commit over main acf26214): every path that could run a call twice or
  never read and none found (`not_asked` plans nothing, so a later continuation finds its node answered and not
  pending; `calls_of` applies the rule `unanswered` already applied to results; `Runs` and `Answered` calls are
  untouched). No client reads a stored ToolCall's gate record as a card (theseus-discord renders cards from
  `tool.confirm_requested` only). The stack's build clean, **the core golden as merged** passing; **211 of 211**
  related tests; **3 of 3 planted reverts caught** (the decline rule off; resume's map by bare id; late.rs's lookup
  taking the first). The whole stack merged with main c07bbe6a: 2,885 of 2,885, the golden's 37 moved lines holding on
  top of queue-frames' and smalls'.
- **Live** (R26, scratch daemons of the stack's build and main's, `approve_paths` a guarded directory). A decline with
  "wrong place": on the branch the confirm followed the resumed turn to the stand-in's `Done.`, no card was left, the
  history held the decline, the note's content and "Not run: an earlier call in this batch was declined.", and neither
  file existed; **on main a second card waited**, and the model had heard nothing. Repeated ids, with R26's own
  stand-in numbering each response's calls from `toolu_fake_0` (the sim's numbers them uniquely since 39b): after the
  note was rewritten to "a rising tide", an approved write and a repeated-id read: on the branch the read ran anew
  (its result the note's new line, "a rising tide"); on main it was answered "The call settled but its output was lost in a restart. Check the
  current state before relying on it."
- **FAST.** Only the continuation's path changes (`resume`, `run_batch`, a cancel's sweep), and `calls_of` reads only
  the nodes after the last response where the old map decoded every ToolCall node of the session. The stack's A/B is
  in Item 204 (no regression).

**What the session and the review found.** Two findings outside the branch, filed by R26: **theseus-r4hn (P2)**, the
session's step-1 gate had a 34th failure not on the known list, theseus-kernel's `children` sweep classing a tender
as an orphan under the gate's load (`Relearned { tenders: 0, orphans: 2 }` where 1 and 1 were expected; it passed
alone 3 of 3), reported as a finding, not a flake, since a tender taken for an orphan is one the sweep would reap;
and **theseus-1znq (P3)**, `judge/gate.rs`'s `last_calls` and `recent_reads` pair calls and results by bare id too
(shadow input to security.v1/v3 only, never a gate decision).

**The join** (B9-tools' last). The lock `cloud-approvals-batch-join` (01:29:30) cleared at gate-tests' done line; the
dry run on bd3eb769 (10:44:16) was clean. The merge (10:44:23 onto bd3eb769): no conflict (`fact/mod.rs`, lib.rs and
the core golden auto-merged), no resolve.py, no join fix; the staged tree equalled the dry run's; 9 files, +710 −134
(`toolrun.rs` 2,418, the new `tests_approvals.rs` 446 and `toolrun/calls.rs` 154 lines); `MANIFEST_FORMAT` 23; all 163
`mod` lines kept, the five the brief named present. The warm (to 10:46:17; the test build 1 m 17 s, clippy clean);
R26's suites with no `RUST_MIN_STACK`, **214 of 214** (R26's 211 plus three tests main gained since, from
history-pages and queue-frames), the core golden passing. The signed merge **25b0578f** (bd3eb769 and 016a3a0b),
10:47:30. Its gate (10:47:45 to 10:53:51, ok; started at minute 47 after reading the gate lock's one holder, a
reviewer's short test run, with a plan to stop it if it still waited at about 10:54:30; 17 s of lock wait): **2,948 of
2,948** (1 slow, 24 skipped; gate-tests' 2,943 plus 5), the suite 10:48:35 to 10:53:15, no hour crossed, the core
golden passing again; lifecycle in every budget (cold start p50 21.9 / p95 23.4 ms; from the config copy 21.9 / 29.5;
clean shutdown 32.0 / 49.3; a post in flight 76.0 / 80.7; SIGKILL then restart 26.2 / 35.8; binary swap 46.9 / 52.4;
restore 136.2 / 171.0; serving 17.34 / 26.66); L1 start p50 6.02 ms; turn frames 5 and 9, plain p50 76.5 ms, tool call
158.9 ms. Pushed 10:53:59, the branch deleted, done line 10:54:07; theseus-6i0 and theseus-w6uh closed with the hash.
Across the stack's three gates the end stood within 1.6 ms of its base on both turns, frames at budget throughout,
and cold start p50 back at 21.9 to 22.7 ms, so the chain log (11:02) lowered theseus-ccux, the cold-start creep of the
three gates before, to P3: most likely load. The store stays at format 23.

**The install** (installed 2026-10-06 13:03 at f589d9cb, install #7). On Eddie's daemon a decline reaches the model at once and ends its batch's waits: once
a call of a batch is declined (his no, an expired card, or a confirmation no longer valid), each later call that would
ask is answered "Not run: an earlier call in this batch was declined." with no card, so he sees no second card and the
model hears every result at once; calls that need no answer still run. An approved call runs even when its id repeats
an earlier answered call's. No config key. Health after the restart (13:03:47): `theseusd check` exit 0, 9 secrets ready 1.02 s after the start, startup serving at 21.5 ms (at load 11 to 16), `cgroup: delegated`, the judge's live packs as before (`security.v3`, `route.v1`, `rerank.v1`), memory live on the `baseline` arm, voice ready, Discord ready, and no error or warning in the journal; no config change, the store at format 23, the S3 backup caught up again from 13:04:00, and no budget held.

**Divergences.** The joiner's brief described the branch as "a batch's approvals are asked together"; the branch
built 6i0's first fix, a decline ending the later waits, which the issue allowed, so its close stands (the tools
joiner's finding 2; the chain log, 11:02, says the DM thread's brief had described it wrongly). The not-run status is `Cancelled`. Repeated ids are keyed, not refused.

**Known gaps.** R26's "For Eddie", each recommended: keep the broad trigger (any `Declined`, an expiry included: under
the narrow one, the operator's own no only, the next card would wait its full time again, the bug itself), and add a
test of an expiry or an invalid confirmation ending a batch, which none covers; one card for a whole batch, not now.
theseus-r4hn (P2) and theseus-1znq (P3), open. theseus-core's AGENTS.md "Tool calls" bullet (`toolrun/calls.rs`, the
decline rule, `resume::calls_of`, `tests_approvals.rs`), owed by the review, was not written at the join.

### Item 207. Discord live bindings: the bindings file is read while the daemon runs, so a place added or removed binds or unbinds with no restart, a lane's maps keep their newest keys, and the courier's disk notice is under test (theseus-ocwt, theseus-celu.37 and theseus-8phq; v1.1's lane discord2, its first two steps; the eighth cloud batch's discord-live session, fired 2026-10-05 13:22 from 60b43fb6, Opus 5.5, finished 17:17; 777b6088, b888621e, 244174ab, a3cb4d44, d95e39cc and 3b3e800c; reviewed 2026-10-06 01:22 to 01:52 and, relaunched after the account's weekly limit stopped it, 08:08 to 08:58, by local reviewer R23, stack D; joined 11:07 at 6496ce34, a signed merge onto 25b0578f, by the stack-D joiner; installed 2026-10-06 13:03 at f589d9cb, install #7)

**Why.**
- **theseus-ocwt.** The Discord binding read its bindings file only when it started, so a place removed from the
  file while the daemon ran stayed bound (its lane delivered, its session's posts went out) until the next start,
  when its unsettled posts were refused as "not bound here any more" (theseus-l3m). §3.16 said so: "a removed place is
  noticed at the next start".
- **theseus-celu.37,** Review 2's Part III Item 6, kept in roadmap-v1.1's Discord theme: a lane's maps grew for the
  process's life (`sent` and `sealed` in `courier.rs` were inserted into and never pruned).
- **theseus-8phq** (R13's planted revert 7 on health-words, Item 177): no test held the courier's disk arm, the notice
  health-words' disk watch posts where approvals go.

**What landed** (all in `crates/theseus-discord`; the merge 10 files, +1,250 −108, without the cloud files; no new
package, config key, protocol type, ledger kind or store format change (23 stays 23)).
- **What the session found first.** A place is more than its lane: its actor (whose mailbox sits in `Routes` by
  session, channel and DM user), its routes (`users`, `mention_only`, the DM list approvals read), its `PlaceBits`
  (guild and ceiling, immutable until now), its class and its guild's word in the core, and its row on the board,
  which nothing ever removed. A place's actor could never end (it holds its own sender, for `rebind`), so it needed an
  explicit message. And dropping a lane from the map alone would race the courier: `refuse_unbound`, called at every
  outbox change, refuses dispatched posts as well as pending ones, so a post mid-write would be refused under it.
- **The watch** (777b6088; new `runtime/live.rs`, 555 lines; runtime.rs 3,453 to 3,418 lines, under its 3,500
  ceiling). A 2 s timer of the binding's own (`live::PERIOD`, a constant, no config key) stats the file (mtime, size,
  inode); only when the stamp moved is it read and parsed, and only when the revision moved is anything done. Its
  first tick reads the file, so a change between the start's read and the watch is not missed; no inotify, since an
  editor's save by rename swaps the inode, which a stat sees. The per-place start code moved into `live.rs`, and the
  start and the live path both call it. A change tells the core first (`guilds::tell_core`: classes, guild words,
  warnings), replaces `PlaceBits` (now behind a mutex), then diffs places by key:
  - **removed:** its routes go at once (a message typed there starts nothing and gets no answer), its actor gets
    `PlaceMsg::Unbind` and ends (taking its board row with it), its session is unwatched, and its lane is **retired**
    (`Shared::retired`): it stays in `lanes` until it ends between posts, so `refuse_unbound` still counts it bound
    and never refuses the post it is writing, while `binds` leaves it out at once; the lane checks before each post
    and after each round (`Lane::retires`, under the lanes' lock), so a post in hand settles as sent, and then it
    refuses the rest;
  - **added:** as a start adds one: its lane (a lane still draining is kept and un-retired under the same lock),
    its session (a fresh one says the bind notice), routes and actor; a channel newly bound private outside a trusted
    guild has its viewers read once, as at a start;
  - **changed** (any field of its entry, or its class by its guild's word): **updated in place**, not stopped and
    started (its routes, `PlaceMsg::Rebound` to its actor for its label, users and spend limit, `LaneMsg::Label` to its
    lane), since a restarted actor would drop a running turn's state and a new lane would lose a streaming reply's
    message ids.
  - **A file that does not load changes nothing:** the board's detail says `the bindings file does not load, so
    revision <r> stays bound until it does: <why>` on one line (a3cb4d44 dropped TOML's caret drawing), and one
    `discord.error` row (`op: "bindings file"`) is written; the same failure seen again is said once (d95e39cc).
  - **What waits for the next start,** said on the board and in the log: the check that the bot is in each guild
    (a `[[guild]]` added binds its places at once, but the invite check runs only at connect) and the voice channels.
  - A re-added place keeps its stored session, so it posts no second bind notice, as at a restart.
- **A lane's maps keep their newest keys** (b888621e, celu.37). `sent`, `sealed` and also `msgs` (every message's
  ids, which the session found grew the same way) go through `Lane::touch` (or `seal`), stamped by a counter; past
  `KEYS_KEPT` = 256 keys a lane forgets the quarter named longest ago (64) from all three maps in one pass. The task
  board's key is exempt (its message is sought once a process). Recency, not pruning at the settle, because the lane
  cannot tell when a turn's stream has ended (the renderer lives in the place's actor) and a stream state can come
  after its post; the session called its guard "an argument, not a proof".
- **The disk notice's test** (244174ab, 8phq). `tests_outbox::each_disk_crossing_posts_one_note_in_the_dm_approvals_go_to`:
  for each crossing (low from ok, below the floor from low, low from below the floor, ok from low), one message in the
  owner's DM with `disk_note`'s words, settled under `note:<corr>` with that create's nonce.
- **A two-life test guard** (3b3e800c). Twice in 26 batch runs under load, a `tests_outbox` test with two lives on one
  store failed in the second life's `Store::open` ("Database already open"), a first-life worker still mid-poll in
  the gateway's connect after `shutdown_timeout(2 s)`; main's crate failed the same way once in 10 runs. Both tests now
  wait up to 30 s for the first life's core to drop (`until_dropped`).

**How it is proven.**
- **The session's tests** (`tests_live.rs`, through the fake Discord's REST and gateway, with no restart): a place
  removed live leaves health, its session's reply is refused (an `action.failed` row naming its outbox), a message typed
  there is no turn, and another place's reply goes; a place new to the store posts its bind notice and answers; the
  removed place put back answers in its old session with no second notice; a broken file leaves every place bound and
  answering, its reason on one line, a second save with the same fault no second row; a changed place keeps its session
  under its new label and its new users; a post held mid-write when its place leaves settles `Succeeded` and the next
  `Failed`, with one refusal row. `tests_bound.rs`: a lane driven through 128 notices and 256 replies holds 256 keys at
  most, the board kept, the oldest forgotten, the newest still sealed; a late stream state of a sealed key is dropped.
  180 of 180 theseus-discord and theseus-sim tests; 26 batch runs of three modules under load (nice 19 beside four
  busy loops). The session's plants (the watch never acting; the lane never retired; the lane dropped at once; the
  error not shortened; the once-per-failure guard removed; the bound removed; `sealed` pruned at its post; the disk arm
  renamed) each failed. It ran the live check on its own VM too, which found the multi-line detail and the doubled
  error row it then fixed.
- **The review** (R23, review commit dc518254 on main fc96e2da, format 22): the build clean (protocol.gen unchanged);
  **181 of 181** in theseus-discord and theseus-sim; **the whole workspace suite on the merged tree, 2,888 of 2,888**;
  the lock order holds (`lanes` before `retired` everywhere, the routes lock never with either). **2 of 5 planted
  reverts caught**: the report's two that matter most (the watch never acting; the lane dropped at once instead of
  retired); R23's three were not, each a test gap filed (`binds` naming a retired lane, theseus-88cp; a changed place
  stopped and started instead of updated in place, theseus-02bq; the task board's key forgotten by the bound,
  theseus-8u7m, P2, since `tests_bound` puts the board's id into `msgs` past `touch`).
- **Probes** (tests added for the run only): a lane at its bound, a background job's tool line, seven turns of 19
  loops (266 keys), then the job's late result: **two tool lines** where the control with the bound raised held one
  (theseus-6809, P2); and a voice channel added live, then an unrelated change: the board's "waits for the next start"
  note was gone though the voice channel still waited (theseus-btt4).
- **Live, 27 of 27** (R23, a scratch daemon of the merged build with the sim's Discord rig, a fresh state dir): bound
  in 1.8 s; an unchanged file at the first tick, a touch and a restart on the same file posted nothing and kept every
  session; a removal by an editor's save by rename left health in 1.4 s, a message typed there started no turn, its
  session's reply was refused and health's outbox line counted it; put back by an in-place append it was bound in 0.7 s
  in its old session with no second notice; a place never bound posted its notice in 1.7 s; a broken file kept every
  place, said why on one line, wrote one row for two saves, and its detail cleared 0.35 s after the mend; a half-written
  save (203 of 406 bytes, held 5 s) changed nothing and was said once; saves 1 s and 0.2 s apart gave one answer each
  time. **A hazard:** a save cut at a table boundary (198 of 406 bytes) is valid TOML naming fewer places, and the
  places past the tear left health for the hold and came back 0.8 s after it, their posts refused and messages
  unanswered meanwhile (theseus-sn2z).
- **3b3e800c's race** did not reproduce on the review's machine: 0 of 100 runs of the two tests without the wait (up
  to one busy loop per core, load 40 to 60) and none in three whole-module batches; the wait cost 0 ms every time. Two
  of main's tests failed in that extreme in-process run, test faults filed (theseus-o2tm, a fixed 300 ms sleep;
  theseus-sj0t, the card found as the first message naming `fs.write`, which the tool line names too). A real daemon
  cannot get "Database already open" from it; the mechanism, a task mid-poll when `shutdown_timeout` gives up keeping
  the core and store, is filed as theseus-9ggu.
- **FAST.** The binding's start is off the measured start path: the lifecycle bench's daemons answer health while the
  binding still waits for its token (51 of 51 starts, both arms), and the watch is spawned in `serve` after that,
  costing one stat, one read and one parse, then a stat every 2 s. R23's settled A/B (12 runs in one hold, A B B A
  order, after waiting 470 s for gate.sh's quiet bar): cold start +9.2 ms, config copy +6.0, SIGKILL restart +1.9,
  phases that run the same code in both arms (the daemon's own clock to serving, which no Discord code reaches, moved
  +4.7 ms with them: the A/B's noise); the phases where a binding serves, a clean stop with a job −3.4 ms and with a
  reply's post in flight −3.3: no cost. `bench turn --check`: frames 5 and 9.

**What the review found** (13 findings, all open at the join). Beyond those above: `refuse_unbound` copies the lanes'
keys and drops the lock before refusing, so a place removed and re-added within a tick or two can have a post the
re-added lane is sending settled as refused (theseus-yduk, with the fix: hold the lanes' lock across the refusal); a
`[[dm]]` put back live goes last in the DM list, so with two owners' DMs approvals move to the other's until a restart
(theseus-nz3q; one owner's DM, today's setup, is unaffected); a place added live whose session cannot open is never
retried, though the report said it would be at the next change (theseus-u6v6); and the renderer's own per-key maps grow
for the process's life (theseus-whb0, pre-existing). celu.37's guard holds in practice for a turn's own stream but
fails for cross-turn edits, which the renderer makes by design (`update_tool` edits a tool line in any of the last 8
turns): theseus-6809, with the fix to forget a turn's keys when the renderer drops the turn, which also bounds whb0.

**The join** (stack D, the branch alone). The lock `cloud-discord-live-join` was taken at 09:04:33 behind five locks,
and the DM thread placed voice-turns' ahead of it at 09:05; it waited on soul-import (09:11:50), voice-turns (09:37:26),
learning-fixes (09:58:51) and the tools stack (to 10:54:07), its polls run with `GIT_OPTIONAL_LOCKS=0`, and read clear
at 10:54:44. voice-turns had changed `crates/theseus-discord/src/runtime/voice.rs` (+7 −1), so R23's dry-run check,
which compared the merged crate with the review's by tree, would no longer match; the joiner's take accepted exactly
that case (the crate differing only in files main changed since the branch's base, none of them the branch's, each
main's blob), tested on a simulated main and on the real one with a negative control. R23's dry run on 25b0578f inside
the guarded take: clean, format 23, runtime.rs 3,418 of 3,500 and render.rs 3,001 of 3,001. The merge (10:54:53): clean,
no resolve.py, no join fix; the staged tree equalled the dry run's less the cloud files; 10 files, +1,250 −108, R23's
diff exactly. The warm (10:55:06 to 10:56:13, the test build 55.4 s, clippy clean), the check against everything joined
since the review, voice-turns' new `theseus_voice` variants among it; then theseus-discord, theseus-sim, theseus-protocol
and `tests_output`, **214 of 214** in 23.5 s (181, R23's count, plus 31 and 2), the branch's six new tests among them.
The signed merge **6496ce34** (25b0578f and 91cb2873), 10:57:26. The commit landed at minute 57, so the gate waited for
:01: 11:01:01 to 11:06:50, ok, no lock wait. **2,955 of 2,955** (1 slow, 24 skipped), no hour crossed; lifecycle in
every budget (cold start p50 24.5 / p95 44.0 ms; from the config copy 27.2 / 36.8; clean shutdown with a job 49.8 /
63.0; a post in flight 78.2 / 88.8; SIGKILL then restart 29.4 / 35.9; binary swap 78.8 / 149.3; restore 204.8 /
226.9); L1 start p50 6.17 ms; turn frames 5 and 9, plain p50 78.9 ms, tool call 153.1 ms. The stop, swap and restore
read high against the tools stack's gates, but phases with no Discord code rose as much (restore's own copy, open and
record; the daemon's store phase at each start, 7.18 / 30.98 ms against about 4.2 / 13.3; this disk's fdatasync probe
12.6 ms against 6.4), and R23's settled A/B had found the binding's phases no slower: the disk, not the branch, as the
judge pair's gate on 6496ce34 then confirmed (cold 22.6, swap 51.6, restore 143.4 ms; the chain log, 11:32). Pushed
11:07:08, the branch deleted, done line 11:07:17; theseus-ocwt, celu.37 and 8phq closed with the hash. The store stays
at format 23.

**The install** (installed 2026-10-06 13:03 at f589d9cb, install #7). Eddie's daemon re-reads its Discord bindings file every 2 s, so a place added or
removed binds or unbinds with no restart: a re-added place posts no second bind notice; a removed place's post in
flight settles as sent and its later posts are refused; a file that does not load changes nothing and health's line
says why; a new guild's invite check and the voice channels still wait for a restart. A lane keeps its newest 256 keys.
No config key. A save by rename, as an editor makes it, replaces the file in one step, so theseus-sn2z's torn read needs
a save written in place. After install #7's restart (13:03:47; its report), health's Discord line read ready with no
bindings-watch note, the healthy state: the watch speaks on that line only when the file does not load or a change
waits for a restart. Health after the restart (13:03:47): `theseusd check` exit 0, 9 secrets ready 1.02 s after the start, startup serving at 21.5 ms (at load 11 to 16), `cgroup: delegated`, the judge's live packs as before (`security.v3`, `route.v1`, `rerank.v1`), memory live on the `baseline` arm, voice ready, Discord ready, and no error or warning in the journal; no config change, the store at format 23, the S3 backup caught up again from 13:04:00, and no budget held.

**Divergences.** The watch is a 2 s stat of mtime, size and inode, not inotify or the heartbeat (roadmap-v1.1's row).
A changed place is updated in place. The bound is recency (newest 256), not pruning at the settle, and covers `msgs`
too. The session found and guarded a pre-existing two-life race in its tests; its daemon-side root is filed.

**Known gaps.** R23's "For Eddie", each recommended: keep the re-add's silence, the refused reply of a removed place
and the 2 s constant (theseus-sn2z's fix, acting on a stamp only once it has held a tick, costs one more period);
retry a failed live bind each tick and say so on the board (theseus-u6v6); keep the guild check and the voice channels
waiting for a restart, with the board saying so until then (theseus-btt4); take theseus-6809 (P2) next in the discord
lane. Open: theseus-6809 and 8u7m (P2); yduk, sn2z, nz3q, u6v6, btt4, whb0, 02bq, 88cp, 9ggu, o2tm and sj0t (P3), all
given to batch 10's discord tasks (discord-bound, discord-watch, discord-tests) at 12:37. discord2's third step, C5's
`BindingPort`, is left.

### Item 208. Judge tests: a reservation's sentences, the compile point's slow Jev and a rerank's links held by tests, and inbound and compile judgments record their workload class (theseus-02vo, theseus-bhn2, theseus-daiz and theseus-fi5n; the ninth cloud batch's judge-tests session, fired 2026-10-05 20:06 from 4a449460, Sonnet 5.5; f9c5bea5, 0e7716fc, e65d743a, 8b9681e0 and 72e41825; reviewed 2026-10-06 01:24 to 01:52 and, relaunched after the account's weekly limit stopped it, 08:07 to 10:02, by local reviewer R28, stack B9-judge; joined 11:20 at fb1133dc, a signed merge onto 6496ce34, the first of the accepted pair's two merges under one lock and one gate, by the B9-judge joiner; installed 2026-10-06 13:03 at f589d9cb, install #7)

**Why.** Four gaps the batch-5 reviews had left in the judge's points:
- **theseus-02vo** (R2, judgment-surfaces' review, Item 119): a planted revert that never said a reservation's facts
  (the pause, a booked block, the resume) passed all 20 judge tests.
- **theseus-bhn2** (R2, continue-shadow's review, Item 122): a plant that blocked the turn on continue.v1's judgment
  at the compile point passed.
- **theseus-daiz** (R3, rerank-live's review, Item 141): no test held the memory pass's links in a rerank's repack,
  rerank-live's join fix.
- **theseus-fi5n** (R2's stacked review of batch 5's judge points, Item 121): design §2.2 has every judgment record
  the turn's workload class, and 23b's surfaces read it from the row's `context.class`, but inbound (25a) and compile
  (25b) judgments recorded none, so the metrics counted them as `unknown`.

**What landed** (theseus-core; the merge 10 files, +568 −2, without the cloud files, two of them code, fi5n's; no
package, config key, protocol type or store format change (23 stays 23)).
- **02vo** (f9c5bea5, 72e41825). `judge/tests_reserve.rs` (`reserve` is private): the pause row and line once however
  many refusals; the resume row and line naming both days, once, and a fresh process says nothing; a META block
  reserved past its settled spend books its rest, with row and line, once; a breaker opened by the fake's `Down` says
  its line once; a shed report says its line. The session found what the brief did not say: a day's unsettled
  `in_flight` carries into the next day, so a resume test must settle the first reservation, as a real call does.
  "Only after its frame" could not be proved by order (the row and line are written in one call), so the tests check
  that they agree. The shed test failed 5 of 5 under load at first (the client's 1 s timeout freed the permit); the
  fifth commit gives its client 25 s.
- **bhn2** (0e7716fc). `tests_continue_slow.rs`: continue.v1 alone judging, Jev slow (30 s, the client's total 8 s);
  once the fake has seen the continue.v1 request, the turn's answer is awaited, and at that moment no call has settled
  (`calls_today == 0`, counted at settle) and no `judge:continue` row exists; the row then comes, failed `timeout`.
  Proved by order, not by duration.
- **daiz** (e65d743a). `tests_rerank_links.rs`: two notes on one subject, the newer linked `same_entity` to the older,
  Jev putting the older first, `recall_max_items = 1`: live (`Memory::refill`) and in shadow (`Recalled.links`), the
  repack admits the newer alone.
- **fi5n, the code** (8b9681e0). Inbound judgments record `context.class` from `inbound::class`: `task` for a task's
  message, and `unknown` on purpose otherwise, since the turn has not run yet (inbound runs only for a person's
  message, so wake and job-result turns never reach it; classify, role and route share the context). Compile judgments
  record `loop_end::class(task, loop_index)`: `task`, else `tools` after the first loop, else `reply` (`AtCompile`
  gains `task`, one line at its call). `theseus.judge.calls` counts them under those classes. rerank.v1's class was
  left out: its recall sits at a loop's compile, so compile's rule fits, but `Recalled` would need the turn's task flag
  and loop index, in soul-import's area then.

**How it is proven.**
- **The session's tests:** the nine new tests passed 5 of 5 at nice 19 beside four busy loops after the shed fix;
  68 judge, rerank, continue and telemetry tests. Its plants each failed (the reservation's `announce` removed: three
  tests, `left: []`; the breaker's and the shed's `announce`; the compile point's spawn made `block_in_place`: "the
  judgment had settled before the answer", the turn 8.3 s; `links: &[]` in either path; the class removed from
  compile's or inbound's context, `[Null]` against `reply` and `[Null, Null]` against `unknown`, which the metric
  alone would not catch, since an absent class already reads `unknown`).
- **The review** (R28, on main fc96e2da, the pair built together): the build clean (protocol.gen unchanged); **291 of
  291** in the judge families; **8 of 8 planted reverts caught**, each read by name since a Sonnet session wrote them,
  and two caught again on the whole stack with judge-turn-cost's sink; the whole workspace suite on the accepted pair
  with main a1bcbee2 and judge-reads' join fix, **2,906 of 2,906**, and on the full stack, 2,911 of 2,911.
- **Live** (R28, a scratch daemon of the stack's build, the stand-in model and the real Jev with its key by reference,
  capped): with dormancy 0, two messages in one session: classify.v1, role.v1 and route.v1 rows carried `class:
  "unknown"` on both turns and loop.v1's `"reply"` ($0.000273, 4 calls); continue.v1 did not judge in 60 s, so the
  compile point's class is held by the test and its plant only. At a $0.0001 day, health said paused and the
  narrative held the pause line once: "Shadow judging paused: today's $0.00 is spent. It resumes at local midnight."
  ($0.000056). R28's Jev spend in all, $0.000329 of the $0.25 cap.
- **FAST.** Two `&'static str` context fields at the inbound and compile points; nothing else on a turn's path.

**The join** (B9-judge, the accepted pair: one lock, two merges, one gate; judge-turn-cost, the stack's third branch,
was not accepted, its sink sent back as the judge-sink fix round, and not merged). The joiner, spawned at 10:33, took
`cloud-b9-judge-pair-join` at 10:39:10 behind gate-tests, approvals-batch and discord-live, and waited 28 min 20 s; its
polls read owner files and heads only, so it took no `git status` while the joiners ahead merged. R28's dry run on
6496ce34 inside the guarded take (11:07:30): judge-tests clean (tree 24a3e5e6), judge-reads clean onto it with
`joinfix.py`'s seven edits applied (tree 04e15e21), `MANIFEST_FORMAT` 23, computed from origin/main. The merge (11:07:31
onto 6496ce34): no conflict (lib.rs auto-merged), no resolve.py, no join fix; the staged tree was the dry run's first;
10 files, +568 −2, `judge/mod.rs` the largest at 916 lines. The warm (to 11:08:50, the test build 52.2 s, clippy clean);
**10 of 10** in 10.6 s (the branch's ten test functions; its report counted nine new tests with fi5n's among them). The
signed merge **fb1133dc** (6496ce34 and d6d89528), 11:09:35; judge-reads' merge followed (Item 209).
**The pair's one gate**, on d28bfc4b (11:13:48, minute 13, to 11:19:40, ok; no lock wait): **2,972 of 2,972** (1 slow,
24 skipped; discord-live's 2,955 plus the pair's 17: judge-tests' 10, judge-reads' 5 and the join fix's 2), the suite
11:14:17 to 11:19:04, no hour crossed; lifecycle in every budget (cold start p50 22.6 / p95 24.1 ms; from the config
copy 22.9 / 29.7; clean shutdown 34.5 / 53.1; SIGKILL then restart 30.4 / 33.1; binary swap 51.6 / 67.5; restore 143.4 /
159.3; serving 19.34 / 29.72), the unbudgeted "clean shutdown, a reply's post in flight" p50 76.0 with one outlier of
764.5 ms in 10 runs, the largest seen (earlier maxima under 393 ms); L1 start p50 6.66 ms; turn frames 5 and 9, plain
p50 81.5 ms, tool call 160.3 ms, inside the day's spread (tool call 147.5 to 164.6 ms). The cold start, swap and restore
back in range on discord-live's code confirmed that gate's high reads as the disk's. Pushed (origin/main d28bfc4b at
11:19:53), both branches deleted, done line 11:20:13; theseus-02vo, bhn2, daiz and fi5n closed with fb1133dc. The whole
one-gate stack took 12 min 43 s from the queue's clear to the done line. The store stays at format 23.

**The install** (installed 2026-10-06 13:03 at f589d9cb, install #7). Tests, plus one recorded field: on Eddie's daemon inbound judgment rows carry
`context.class` `unknown` (or `task` for a task's message) and compile rows the loop end's class, and an OTLP
collector's `theseus.judge.calls` counts them under those classes. No config key. Health after the restart (13:03:47): `theseusd check` exit 0, 9 secrets ready 1.02 s after the start, startup serving at 21.5 ms (at load 11 to 16), `cgroup: delegated`, the judge's live packs as before (`security.v3`, `route.v1`, `rerank.v1`), memory live on the `baseline` arm, voice ready, Discord ready, and no error or warning in the journal; no config change, the store at format 23, the S3 backup caught up again from 13:04:00, and no budget held.

**Divergences.** Inbound's class is `unknown` by design, not the turn's: the turn has not run when inbound judges.
"Only after its frame" is held by agreement, not order.

**Known gaps.** R28's "For Eddie", each recommended: write sub-cent amounts in the pause line with their digits (a
polish, not filed); rerank.v1's class, a P3 after soul-import (not filed); a task turn's `task` class stays held by the
unit test alone. The one 764.5 ms outlier of the unbudgeted post-in-flight phase was filed nowhere on one sample; the
next gate (B9-memtel's, Item 210) read that phase at 71.7 / 79.9 ms.

### Item 209. Judge reads: `judge.list` pages back from the newest judgment, the notices' brake reads today's rows, and the learning rules skip judgments whose windows closed; with R28's join fix, the CLI writes `judge.list`'s floor as `M+` and the task-brief walk steps over imported sessions (theseus-wse2, theseus-b8e2, theseus-e1ei and theseus-cf5c, with theseus-6jos; the ninth cloud batch's judge-reads session, fired 2026-10-05 20:06 from 4a449460, Opus 5.5; 80436561, e2ff85f9, a64f1ea1 and e02c30cb; reviewed 2026-10-06 01:24 to 01:52 and, relaunched after the account's weekly limit stopped it, 08:07 to 10:02, by local reviewer R28, stack B9-judge, with a join fix; joined 11:20 at d28bfc4b, a signed merge onto fb1133dc, the second of the accepted pair's two merges under one lock and one gate, by the B9-judge joiner; installed 2026-10-06 13:03 at f589d9cb, install #7)

**Why.** Reads that grew with history, and one window no test held:
- **theseus-wse2** (R2, judgment-surfaces' review, Item 119): `judge.list` read every record of each scope it listed
  from position 0 (with no pack, all the embedded packs' scopes), decoded each, and only then cut to `limit`; the
  cockpit's Judgment section polls it every 4 s.
- **theseus-b8e2** (security-notices' report, confirmed by R3, Item 142): the notices' brake's first read each run
  scanned all of `judge:security`.
- **theseus-cf5c** (the batch-5 harvest wake's review of learning-ledger, Item 129): each learning run re-derived system
  labels over every judgment in history, reading each judged session's whole node list for loop and classify.
- **theseus-e1ei** (the same review): no test held that a continuation more than 10 minutes after a turn takes no
  label; planting the window's removal passed.

**What landed** (theseus-core's `rpc/judge.rs`, `judge/notice.rs`, `learning/system.rs` and `rpc/learning.rs`, the
protocol's `JudgeListResult`, the cockpit's Judgment view; with the join fix, the CLI; the merge 16 files, +1,208 −90;
no package, config key or store format change (23 stays 23); the protocol change is additive).
- **`judge.list` newest first** (80436561, wse2). The index counts a kind and a scope without decoding, but no tag,
  pack or version, so `matched` cannot be exact without decoding. The read now pages back through `ledger_page` with
  `before` cursors over the `k:judge.call` tag (or `ks:judge.call\u{1}<session>` with a session), `since` as the page's
  `since_ms` with the row-time check kept, decodes only rows scoped to a listed scope, and stops once `limit` rows pass
  the filter and one more matches. While the index's shape is built after serving, the page answers `None` and the old
  scan runs. **The protocol:** `matched` is exact when the read reached the start of history, else a floor (the limit
  plus the one older match that proves more), and a new `JudgeListResult.more: bool` (serde default false, so an older
  daemon's exact answer reads as exact) says which; protocol.gen regenerated, and the cockpit prints "N of M+". The CLI
  was left alone in the branch, as briefed.
- **The brake's day from today's rows** (e2ff85f9, b8e2). `read_day(store, today, now_ms, paged)`: the pause by its
  key, and today's `tool.notified` and `judge.label` rows by a page over both kind tags from `local_midnight(now)`,
  newest first, decoding only rows scoped `judge:security`, each still counted by its own time through the same
  `count` rules; `brake_day` takes `now`, so the page's midnight is the brake's own clock. The dated position mark the
  issue suggested was not needed.
- **The continuation's window held** (a64f1ea1, e1ei): "go on" 10 min + 1 ms after the judged turn's last node takes no
  label; 9 min and exactly 10 min do (the rule is `<=`).
- **The rules read only what can still change** (e02c30cb, cf5c). The session found the security rule read each
  judgment's kernel action every run and the route rule walked its whole scope for each routed judgment (quadratic).
  The run's `learning.last_run` META value gains `through`, the store's last position taken before the scopes are read;
  the next run builds `system::Cut { through, at_ms }` and skips a judgment whose row is at or before `through` and
  whose time plus its window plus `MARGIN_MS` (1 h) is before the last run's time. Windows: loop 10 min (the
  continuation), or 24 h for a task's (false completion); classify 24 h; security `confirm_ttl` (900 s by default);
  route 10 min; rerank and others not cut. With no mark, or a mark without `through` (every mark written before this
  build), the run walks everything. The false-completion rule's task briefs page sessions newest first through the
  index's births, stopping at the first created before the earliest open task judgment less the margin. A log line,
  `learning: the rules read what can still change`, carries `sessions_read`, `actions_read`, `tasks_read`,
  `judgments_closed` and `first`.
- **The store format, decided: no bump** (R28). `through` is a key inside the untyped `learning.last_run` value, which
  every reader (main, install #5's build and this branch) decodes as `serde_json::Value`, taking `at_unix_ms` alone. The
  new build on an old store sees no `through` and walks everything once; a rollback ignores the key and its next mark
  drops it, so the newer build walks once more. Nothing is misread either way, and a bump would only block the rollback.
- **R28's join fix** (`judge-reads/joinfix.py`, run after the merge, seven edits). **Part 1:** the CLI's `theseus judge
  log` printed the floor as an exact count ("20 of 21 judgments" over about 1,900, seen live); it now writes `21+`, as
  the cockpit does, through `render::judge_log_footer`, with a unit test. **Part 2** (theseus-6jos), applied by itself
  where soul-import's `SessionRecord.imported` is on the tree, as it was since 09:11: soul-import writes each imported
  session born at its import with its episode's old time, and the task-brief walk ends at the first session older than
  its floor, so after an import the task sessions born before it were never read and a false completion took no label
  (R28's probe A showed the stop); the walk now skips imported sessions, with a test that imports a month-old episode
  through soul-import's own `import_batch` after two tasks are born.

**How it is proven.**
- **The session's tests** (`tests_judge_reads.rs`; `tests_learning`): the paged read gives the same judgments as the
  scan over 11 filters, by position, id and order, `matched` exact wherever the scan matched no more than the limit and
  `limit + 1` with `more` otherwise. Records decoded, scan then page, over 10,000 judgments in 30 days: the default
  **11,429 to 51**, a session 11,429 to 51, `-n 7` 11,429 to 8, a pack id 4,572 to 51, a version 4,572 to 101, every
  filter together 2,286 to 21; but `--pack loop.v2`, matching nothing, 2,286 to 2,000 (every loop row: the tags carry
  no pack). The brake's day equals the scan's (4 notices, 2 noise labels, today's pause, with near misses 1 ms before
  midnight, a policy's notice, a system's label and a judge's notice in another scope not counted); read as of tomorrow
  it decodes 0 records. Over 200 old judged sessions and one open judgment, the learning runs read **201, then 1, then
  0** sessions, with 21 labels, none twice. Each plant failed (the filter's version clause; the brake's page from
  yesterday's midnight: 361 decoded; the window removed: the late judgment labelled, which the existing loop test did
  not catch; the cut removed: 201 against 1). 172 of 172 in the judge and learning suites; each new test 3 runs under
  load (the 10,000-judgment test about 70 s there).
- **The review** (R28, on judge-tests' review commit over main fc96e2da, then main a1bcbee2 and 79be3213 merged in):
  the builds clean (protocol.gen as the branch regenerated it); 291 of 291 in the families; **4 of 4 planted reverts
  caught**, and the join fix's own (the skip removed: "task sessions read 0"); the whole workspace suite on the accepted
  pair with main a1bcbee2 and the join fix, **2,906 of 2,906**, and on main 79be3213 with both parts, 2,924 of 2,925,
  the one failure `term::tests::a_close_leaves_no_child_behind` ("a child outlived its terminal"), in code nothing here
  touches, which passed alone 3 of 3 and was filed as a finding, since a negative assertion that fails once may mean
  the leak happened (theseus-d006, P2). R28 tried to break the cut: a crash between `through` and the labels cannot
  happen (one frame), and two runs in a minute, a clock step back and a long turn hold; **a run whose clock reads ahead
  closes open windows** (probe B: a run at now + 5 h, then "go on" inside the window, then a run at now + 20 min: 0
  sessions read, no label; theseus-gf8j, P3, rare); classify's 24 h window loses only a turn still running a day after
  its message.
- **Live** (R28, scratch daemons of the pair's frozen build on main's sink, the stand-in model and R28's stand-in Jev,
  every pack as wired, turns back to back in three sessions for 180 s: 895 turns, 1,882 judgments): `judge log -n 20`
  0.01 s, ending "(20 of 21 judgments in …)" without the join fix; `--json -n 500` 0.24 s, `matched` 501, `more` true;
  `--pack loop.v2` 0.13 s, the whole-tag walk. `judge report` twice: `first=true`, then `first=false` (`sessions_read`
  6 each, `judgments_closed` 0, every judgment minutes old), 895 system labels, then 0 new. With the join fix the footer
  read "(20 of 21+ judgments in …)".
- **FAST.** The decode counts are asserted in the tests. A cost moved only to the rare pack filter that matches nothing
  and to the scan while the index's shape is built after serving. The gate's turn bench on the accepted pair: frames 5
  and 9, with no time claimed (a slow moment of the disk, fdatasync p50 6.9 ms).

**The join** (B9-judge's second, under the pair's lock; the queue, the dry run and the one gate are told in Item
208). The merge (`merge2.sh`, 11:09:40 onto fb1133dc, after checking HEAD was judge-tests' merge with
the dry run's tree and origin/main unmoved): no conflict (theseus-core's AGENTS.md and lib.rs auto-merged), no
resolve.py; `joinfix.py` applied all seven edits (part 2 by itself, soul-import's field being on main); `diff --cached
--check` one line, a trailing space in the ts-rs output `JudgeListResult.ts`, as 278 of 354 protocol.gen files have;
**the staged tree equalled the dry run's final tree**; 16 files, +1,208 −90 (render.rs 3,099 of its 3,100 ceiling);
`MANIFEST_FORMAT` 23, no bump. The warm (to 11:12:30, the test build 1 m 43 s, clippy clean); **95 of 95** in 9.7 s:
R28's families, 56 (R28's 54 plus learning-fixes' two newer prove tests), the join fix's two tests, theseus-protocol's
31 (its test regenerated protocol.gen identical to the merge's: never hand-merged), `tests_output`'s 2 and
`tests_judge_surfaces`' 5. The signed merge **d28bfc4b** (fb1133dc and e3bbfbec), 11:13:40, its message naming the join
fix, the no-bump decision and the additive protocol change. The pair's gate on it: 2,972 of 2,972, ok (Item
208). Pushed (origin/main d28bfc4b at 11:19:53), the branch deleted, done line 11:20:13;
theseus-wse2, b8e2, e1ei, cf5c and 6jos closed with d28bfc4b. The store stays at format 23.

**The install** (installed 2026-10-06 13:03 at f589d9cb, install #7). On Eddie's daemon `judge.list` pages back from the newest call row: the cockpit prints
"N of M+" and `theseus judge log` writes the floor as `M+`. The notices' brake reads today's rows. The first learning
run after the install finds no `through`, walks everything once as before and writes it; later runs read only
judgments whose windows are still open. The false-completion rule's task-brief walk steps over imported sessions, so
sessions `theseus import` writes will not end it. No config key; an older client reads `more` as absent. After install
#7's restart (13:03:47; its report), `theseus judge log -n 3` on Eddie's daemon ended "(3 of 4+ judgments in …)": the
floor, as the join fix writes it. Health after the restart (13:03:47): `theseusd check` exit 0, 9 secrets ready 1.02 s after the start, startup serving at 21.5 ms (at load 11 to 16), `cgroup: delegated`, the judge's live packs as before (`security.v3`, `route.v1`, `rerank.v1`), memory live on the `baseline` arm, voice ready, Discord ready, and no error or warning in the journal; no config change, the store at format 23, the S3 backup caught up again from 13:04:00, and no budget held.

**Divergences.** `matched` became a floor when `more`, not an exact count. The brake needed no dated position mark.
The CLI's floor and the imported-session skip came in at the join, by R28's join fix, not in the branch.

**Known gaps.** R28's "For Eddie", each recommended: keep cf5c's 1 h margin and classify's 24 h window; no bump, with
a sentence for the root AGENTS.md's version rule (a key inside an untyped META value that every reader reads as
optional, and whose absence means "start over", needs no bump), not written at the join; the rare pack filter that
walks a whole tag waits for a store change (a `judge.call` row tagged by its scope; the cockpit never filters by
version). theseus-gf8j (P3) and theseus-d006 (P2), open. A future-stamped row (its own time later than its frame's)
would be dropped by a page where the scan kept it; the writers stamp before they commit, so none should exist.
theseus-core's AGENTS.md learning paragraph does not say the walk steps over imported sessions (the joiner's note).
judge-turn-cost, the stack's third branch, waits for its fix round (judge-sink).

### Item 210. Memory tests: a search during the paced warm adjacency build answers `building` at once, a search's own build is counted unpaced, and the session summary's call is reserved on the estimate's upper bound (theseus-e21m, theseus-6fn.14 with its duplicate theseus-zv4x, theseus-x875, theseus-6fn.9 and theseus-6fn.8; the ninth cloud batch's memory-tests session, fired 2026-10-05 20:06 from 4a449460, Opus 5.5; e752f83f, 1a5f95e1, 455a890f, fee24fcb and 81119b2c; reviewed 2026-10-06 09:05 to 10:40 by local reviewer R30, stack B9-memtel; joined 11:32 at a8f4b27c, a signed merge onto d28bfc4b, the first of the stack's two merges under one lock and one gate, by the B9-memtel joiner; installed 2026-10-06 13:03 at f589d9cb, install #7)

**Why.** Five follow-ups from M6's memory reviews:
- **theseus-e21m:** no test held memory-checks' choice (Item 187, theseus-3edq) that a search's own
  build of the adjacency projection is unpaced; a planted paced build passed every activation test.
- **theseus-6fn.14** (R17's review of memory-checks, and theseus-1o8i's follow-up): a search under `+activation` that
  ran while the warm build was pacing (the machine busy) waited on the projection's lock and answered `deadline` at
  its recall deadline. theseus-zv4x was its duplicate.
- **theseus-x875** (recall-node's review, Item 112): `recall/render.rs`'s `source()` reads a recalled source by its
  position, so the next request begins with the previous one's bytes even after a later rewrite, but the test ran both
  turns in one Core and its source cache masked a planted read by id.
- **theseus-6fn.9** (30c's GLM live check, Item 131): no test showed `summary_profile = "session"` following a
  one-message model override.
- **theseus-6fn.8** (the same live check): the summary's reservation was priced on the input estimate's `tokens`;
  GLM counted 25 % over it, and reserved held settled only because the summary used 655 of its 2,000 output tokens.

**What landed** (theseus-core's `recall/activation.rs`, `turn/compaction.rs`, the test-only fake in `provider.rs`; the
merge 10 files, +451 −26, without the cloud files; no package, config key, protocol type or store format change (23
stays 23)). Nothing in `turn.rs`, `recall.rs`, the memory pass or soul-import's `hit_of`.
- **A search's own build never paces** (e752f83f, e21m). `PACES` was thread-local and the search's build runs on the
  blocking pool, so the session chose a count: `Adjacent` gains `paces: AtomicU64`, added to before each pace waits (so
  a pace still waiting is seen), and `Adjacent::paces()`; each build's "is built" log line names its own `paces`
  beside `waited_ms`.
- **A search during the warm build answers `building` at once** (1a5f95e1, 6fn.14, a behaviour change). In `Ask::run`
  a search (`build: true`) that finds the projection unbuilt checks `Adjacent::building()` first; if the warm build is
  running it returns at once with the reason `WARM`, "the adjacency projection's warm build is running, paced by the
  machine's pressure; a search does not wait for it", which `activated` maps to outcome `building`. `building()` over
  `try_lock`: only `warm` sets `building`, before it spawns the build that takes the lock, so a turn's refresh or
  another search's unpaced build, which holds the lock only as long as a fold, is still waited for. theseus-core's
  AGENTS.md (`+activation`) and the module docs say the rule. **The residual race** (the report's): a `warm()` begun
  between a search's check and its lock can still queue that search to its deadline, main's behaviour; closing it
  would mean folding outside the lock and two builds at once.
- **x875** (455a890f, a test). The recall-node test's second turn runs on a second Core over the same store and model,
  as after a restart (the session chose a fresh Core over a test hook in `recall.rs`, which soul-import was editing).
- **6fn.9** (fee24fcb, a test; the code was right). `summary_profile_session_follows_a_one_message_override`: a probe
  rig finds the turn that rings, then that turn runs with an override to glm-5.3-flash: one summary call goes to glm,
  the `Summary` node and the `context.compacted` row name glm, and the next turn, with no override, goes back to
  claude-sonnet-5-5.
- **The summary reserved on the estimate's upper bound** (81119b2c, 6fn.8, a money change). `plan_summary` reserves
  `price.reserve_micros(max_tokens, est.upper)`. The test-only fake gains `Scripted::BilledBy`, a bill computed from the
  request it answers. On glm-5.3-flash, billing 25 % over the estimate and the whole 4,096 output tokens: settled 2,591
  µ$, reserved 2,656 on `upper`, where main's `tokens` reserved 2,482.
- The session left the turn's own reservation alone (turn.rs reserves on `est_tokens`) and recommended `upper` there
  too: on Sonnet 5.5 at the default 128k output cap, +$0.008 (0.6 %) on a 70k append turn and +$0.08 (about 5 %) on a
  100k cold one.

**How it is proven.**
- **The session's tests:** `a_searchs_own_build_never_paces` (a search over kestrel's store plus `PAGE + 1` nodes
  builds the projection with `paces() == 0`, where a warm build of the same store counts 1);
  `a_search_during_the_warm_build_answers_building_at_once` (in a user and mount namespace with `/proc/pressure` faked
  at IO 55 %, the shared runner `tests_activation_pace::namespaced`: once the warm build is in its first pace, a search
  must answer `building` within 1 s, half memory.search's smallest deadline; measured 1.8 ms unloaded, 52 to 90 ms
  under load); and the three above. Each plant failed (a paced search build: `left: 1`; the check removed: `deadline`
  after 5.0 s; `source()` by id: the bytes differ, where main's test passed the same plant; the summary from the
  session's own profile: no call to glm; `est.tokens` again: settled 2,591 past 2,482). Under load, five runs each, all
  passed; main's own `a_clean_stop_ends_the_warm_builds_waits` failed 5 of 5 there (the build ending 5.0 to 5.3 s after
  it began, against a 4.5 s bound), and 3 of 3 with main's files checked out: main's flake (theseus-9o2o), its bound
  counting the fold's own time after the stop.
- **The review** (R30, review commit f6971724 on main 79be3213, format 23; telemetry-tests stacked on it): the stack's
  build clean (the core golden, clippy, protocol 31 of 31, shape); the stack's families **124 of 124**; **the whole
  workspace suite on the stack, 2,916 of 2,916**; the namespaced tests run, not skip ("answered building in
  3.535363ms"). **All 7 planted reverts caught**: the report's five and two of R30's (the counter counting every page's
  pace, paced or not; `activated` mapping only "building", not `WARM`: outcome `unavailable`). R30 checked 6fn.14
  against every caller: only `memory.search` builds, and a turn never reaches the check; a warm build starts only
  through `warm`'s swap, so a second warm does nothing; a failed build clears `building`, so the next search builds the
  projection itself; a stop ends the paces. Noted, not filed: a panic in the fold would leave `building` set for the
  process's life.
- **Live** (R30, scratch daemons of main 79be3213 and the stack, fresh state dirs, `/proc/pressure` faked at IO 55 %
  inside `unshare -rm`, a synthetic store of 5,000 sessions, 10,000 nodes): a search during the warm build answered
  **`building` in 0.01 s**, where **main answered `deadline` at 2.01 s**; the warm build then ended (`took_ms=20147
  waited_ms=20000 paces=2`). With `arm = "baseline"` and no warm build, a search's own build: `ran` in 0.32 s,
  `waited_ms=0 paces=0`. With R30's stand-in billing each body's bytes / 4 and a summary its whole `max_tokens`: on a
  one-message `-p zai -m glm-5.3-flash` turn the summary went to glm on both builds, and the session's next turn was
  back on sonnet; **main reserved 1,435 µ$ and settled 1,483, past its reservation; the stack reserved 1,608** for the
  same 1,483 (the stand-in billed 11 % over the estimate).
- **FAST.** The `paces` add runs only in a paced build's pace; the `building()` check is one atomic load, only on a
  search that finds the projection unbuilt; the compaction change is one argument, only when a summary is planned. The
  A/B (main and the stack, one hold, palindrome order, load about 22): frames 5 and 9 on both arms, plain −38.1 ms and
  tool call −6.0 ms, noise in the stack's favour; the lifecycle differences followed the CPU PSI (both arms missed
  budgets under that load), and none of the stack's code runs before serving.

**What the review found.** theseus-ps9i (P3, for Eddie): a turn's own call is still reserved on the estimate's
`tokens`, where the summary's now takes `upper`; on Sonnet 5.5, +0.6 % on a 70k append turn and +5.4 % on a 100k cold
one at the default 128k cap, +2.7 % and +22 % at a 16k cap. A reservation is released at settlement, so the only cost
is that a turn near the day's limit is refused a little sooner. The DM thread settled it for Eddie unless he objects:
yes, as a batch-10 task (the chain log, 11:03).

**The join** (B9-memtel: one lock, two plain merges, one gate). The joiner, spawned at 11:02, took
`cloud-b9-memtel-join` at 11:07:27, three seconds before the judge pair's joincheck cleared, which read it as behind;
its polls read the owner file ahead, not joincheck, and it waited 12 min 46 s. R30's dry run of the stack alone on
d28bfc4b inside the guarded take (11:20:19, format 23 read from origin/main): both steps clean, trees 04840558 and
1d2265cd. The merge (11:20:19 onto d28bfc4b): no conflict (AGENTS.md, lib.rs and `recall/activation.rs`, beside
soul-import's `hit_of`, auto-merged), no resolve.py, no join fix; **the staged tree the dry run's first, byte for
byte**; 10 files, +451 −26, `provider.rs` the largest at 1,590 lines. The warm (11:20:28 to 11:22:02, the test build
1 m 06 s, clippy clean); **44 of 44** in 15.7 s (R30's memory filter plus `tests_output`), the branch's five new or
edited tests by name, theseus-9o2o's flake passing (2.50 s), the core's golden passing. The signed merge
**a8f4b27c** (d28bfc4b and 7d3bd109), 11:23:01; telemetry-tests' merge followed (Item 211).
**The stack's one gate**, on 2bf9e7d3 (11:25:29, minute 25, to 11:31:45, ok; no lock wait): **2,980 of 2,980** (1
slow, 24 skipped; the judge pair's 2,972 plus the stack's eight new tests), the suite 11:25:58 to 11:30:47, no hour
crossed. The first lifecycle run **missed one budget** on one outlier: the clean shutdown with executions waiting and a
job running, p50 39.3 ms but p95 343.5 ms (its max, one run of ten) against 100 + 4 ms; the gate's own rule ran the
bench once more and it passed (cold start p50 23.1 / p95 29.6 ms; from the config copy 22.8 / 24.0; clean shutdown 33.1 /
50.8; a post in flight 71.7 / 79.9; SIGKILL then restart 26.8 / 35.3; binary swap 46.7 / 49.5; restore 138.8 / 146.8;
serving 18.47 / 26.17). The joiner read it as noise: the five mains before gave that phase's p95 at 48.4 to 63.0 ms,
the stack's code is not on that path, and the load was 4.37; nothing filed, both rows kept in the bench history. L1
start p50 6.12 ms; turn frames 5 and 9, plain p50 75.7 ms, tool call 156.9 ms. Pushed (origin/main 2bf9e7d3 at
11:32:29), both branches deleted, done line 11:32:42; theseus-e21m, 6fn.14, zv4x (as its duplicate), x875, 6fn.9 and
6fn.8 closed with a8f4b27c. The stack took 12 minutes from the lock's clear to green on a quiet machine. The store
stays at format 23.

**The install** (installed 2026-10-06 13:03 at f589d9cb, install #7). On Eddie's daemon a search during the paced warm build answers `building` at once,
which matters only once `[memory] arm = "+activation"` is named; his config stays `baseline`. A compaction's summary
reserves its input on the estimate's upper bound (in R30's live check about 4,060 tokens instead of 2,900: 1,608 µ$
reserved instead of 1,435, against 1,483 settled), released at settlement. No config key. Health after the restart (13:03:47): `theseusd check` exit 0, 9 secrets ready 1.02 s after the start, startup serving at 21.5 ms (at load 11 to 16), `cgroup: delegated`, the judge's live packs as before (`security.v3`, `route.v1`, `rerank.v1`), memory live on the `baseline` arm, voice ready, Discord ready, and no error or warning in the journal; no config change, the store at format 23, the S3 backup caught up again from 13:04:00, and no budget held.

**Divergences.** e21m is held by a count, `Adjacent::paces`, not a namespace. 6fn.14 checks `building()`, not
`try_lock`. x875's restart is a second Core over the same store, not a cleared cache.

**Known gaps.** R30's "For Eddie", each recommended: the turn's own reservation on `upper` (theseus-ps9i, given to
batch 10's budget-upper task with xd6l); 6fn.14's residual race, leave it (microseconds wide, only at a warm build's
start, bounded by main's 2 s); `Adjacent::paces` in health, no (a lifetime total; each build's line logs its own).
theseus-9o2o, main's warm-build stop flake, open, given to the same task. A panic in the warm fold would leave searches answering `building`
until a restart (noted). theseus-core's AGENTS.md has a new 151-character line, longer than the file's wrap.

### Item 211. Telemetry tests: the daemon's own telemetry path is held by tests (the stops', the judge's and memory's feeds through `install_telemetry`), and the index gauges by the tender's latest answer (theseus-qqhd and theseus-fk0g; the ninth cloud batch's telemetry-tests session, launched once telemetry3 had joined, fired 2026-10-05 22:22 from acf26214, Sonnet 5.5; ae6cb542, 261877fa and 7a9575e8; reviewed 2026-10-06 09:05 to 10:40 by local reviewer R30, stack B9-memtel; joined 11:32 at 2bf9e7d3, a signed merge onto a8f4b27c, the second of the stack's two merges under one lock and one gate, by the B9-memtel joiner; installed 2026-10-06 13:03 at f589d9cb, install #7)

**Why.** Two planted reverts that telemetry3's tests did not catch, found by local reviewer R18 (Item 190):
- **theseus-qqhd:** no test drove the daemon's path (`install_telemetry`, then `build_telemetry` after serving) for
  `theseus.cancel`; a planted removal of the stops' `export_to` there passed every test.
- **theseus-fk0g:** no test moved an index gauge between samples; a gauge frozen at its first answer passed.

**What landed** (tests only: no production line; the merge 6 files, +290 −22, without the cloud files; no package,
config key, protocol type or store format change (23 stays 23); `telemetry/tests.rs`, at 2,491 of its 2,500 limit,
untouched).
- **The cancel on the daemon's path** (ae6cb542). `telemetry/tests_cancel.rs::a_daemons_exporter_counts_the_cancels_health_counts`:
  parts with no pipeline and the config naming the receiver, so `install_telemetry` builds the exporter after serving;
  a background `proc_run` cancelled; the `theseus.cancel` points equal health's cancels, with no labels pinned.
- **The judge's and memory's feeds** (261877fa). `telemetry/tests_daemon_path.rs`: one judged turn with a fake Jev
  counts `theseus.judge.calls` 1 for {loop.v1, shadow, act, reply}; with `+retention` live and the projection built
  (2 nodes) before `install_telemetry`, `theseus.memory.retention.nodes` reads 2, and a third node through
  `retention_written` makes it 3.
- **A gauge moves between samples** (7a9575e8, fk0g). The tender's stand-in answers what a shared status holds; a
  sample, a changed answer, `health(STATUS_DEADLINE)` asked once, then a second sample with the new numbers; restarts
  stay 0.

**How it is proven.** Each of the session's plants failed with the message its report quotes (the stops', the
judge's and memory's `export_to` removed from `build_telemetry`; `Metrics::index` setting a gauge only while it is 0);
the new tests 5 of 5 under load. **The review** (R30, stacked on memory-tests' review commit): the stack's families 124
of 124 and the whole workspace suite 2,916 of 2,916 (Item 210); **all 6 planted reverts caught**,
the report's four and two of R30's: `retention_written` no longer measuring (the gauge's growth, which the report had
checked only passing) and `export_to` keeping the handle but measuring nothing. **Live** (R30, a scratch daemon of the
stack's build with theseus-index beside it and a loopback OTLP sink): a confirmed background `sleep 60` cancelled in
0.19 s; health said "cancels since the start: l0 1 verified" and the sink's next post held `theseus.cancel {backend l0,
state verified} 1`, equal; `index status` went from 5 to 7 documents after a turn with new words, and the next post was
the first to hold `theseus.index.documents` 7. "No bug found" holds in the code and live.

**The join** (B9-memtel's second, under the stack's lock; the queue, the dry run and the one gate are told in Item
210). The merge (11:23:07 onto a8f4b27c, after checking the lock, origin/main and HEAD): no
conflict, no resolve.py, no join fix; the staged tree the dry run's stack tree, byte for byte; 6 files, +290 −22. The
warm (to 11:24:20, the test build 49.2 s, clippy clean); R30's telemetry filter, **84 of 84** in 5.9 s, the four new
tests by name; with memory-tests' run, exactly R30's build of the whole queue less its 17 judge tests. The signed
merge **2bf9e7d3** (a8f4b27c and 36b8be96), 11:25:23; its gate, 2,980 of 2,980, ok. Pushed 11:32:29, the branch
deleted, done line 11:32:42; theseus-qqhd and fk0g closed with 2bf9e7d3. The store stays at format 23.

**The install** (installed 2026-10-06 13:03 at f589d9cb, install #7). Tests only: nothing changes on Eddie's daemon. Health after the restart (13:03:47): `theseusd check` exit 0, 9 secrets ready 1.02 s after the start, startup serving at 21.5 ms (at load 11 to 16), `cgroup: delegated`, the judge's live packs as before (`security.v3`, `route.v1`, `rerank.v1`), memory live on the `baseline` arm, voice ready, Discord ready, and no error or warning in the journal; no config change, the store at format 23, the S3 backup caught up again from 13:04:00, and no budget held.

**Known gaps.** theseus-xd6l (P3): `Core::build`'s pipeline branch hands the judge and the stops their telemetry but
not memory, so a core built with a pipeline (every test core, theseus-discord's runtime tests) never records the
retention gauge; one line, costing nothing, the daemon unaffected (its parts bring no pipeline); given to batch 10's
budget-upper task. No docs owed (R30).

### Item 212. Core waits: the core's golden waits for a wake its turn cannot outrun and masks a local time's offset sign, a batch's cancel lands while its four reads are held, and a failing Jev's turn is proved to wait for nothing (theseus-23wh, theseus-ig6n, theseus-t2yb and theseus-vbju; the ninth cloud batch's core-waits session, fired 2026-10-05 20:06 from 4a449460, Opus 5.5; 4b9611df, a866d9f7, 5d81b235 and c7df97df; reviewed 2026-10-06 09:03 to 11:04 by local reviewer R29, stack B9-core, the first of three; joined 11:59 at 2e592804, a signed merge onto 2bf9e7d3, the first of the stack's three merges under one lock and one gate, by the B9-core joiner; installed 2026-10-06 13:03 at f589d9cb, install #7)

**Why.** Four of theseus-core's tests failed under load or in another zone, each at a cause the session found:
- **theseus-23wh:** the core's output golden failed under CPU starvation at its 30 s wait for the wake, on main too.
- **theseus-ig6n:** the golden depended on the machine's time zone: a `wake.at` preview carries a negative UTC
  offset (masked to `-#:#`), and a machine in UTC prints `+#:#`, so every UTC cloud VM failed it.
- **theseus-t2yb:** `tests_m3::parallel::a_cancel_during_a_batch_leaves_no_call_dispatched` timed out "never
  dispatched" under load (seen in mcp-server's join gate, Item 110).
- **theseus-vbju** (R10, push-once's review, Item 172): `tests_judge::a_failing_jev_is_recorded_by_its_class_and_changes_no_turn`
  overran its 3 s turn bound under suite load.

**What landed** (tests only, all in theseus-core: `tests_output.rs` and its golden, `tests_m3.rs`, `tests_judge.rs`;
the merge 4 files, +206 −31, without the cloud files; no kernel, wake, judge or fake change; no package, config key,
protocol type or store format change (23 stays 23)).
- **The golden's wake** (4b9611df, 23wh). The session first made the wait's timeout say what it saw, and under the
  load recipe main failed 4 of 5, each "no wake due in 30 s: state Queued, wake None, pending [… (−30923 ms)]": the
  wake was due about 0.9 s before the wait began. The turn that set a 1 s wake took about 1.9 s after its `wake.at`,
  so the kernel's `end_turn` found the free execution's own wake already due and queued it itself (`why: "wake"`)
  instead of parking it `Waiting`, a transcript of another shape, which no poll length could help. The scenario's wake
  is now `WAKE_SECS` = 20 s out (ten times the measured tail; its input still masks to `"after":"#s"`, so the golden
  does not move), and `wake_due` first asserts the turn parked its execution with the wake pending, naming what it saw,
  then sleeps toward the due time by the kernel's clock (at most 1 s a sleep) until `due_now`, under a 60 s guard. The
  golden takes about 20 s longer.
- **The offset's sign** (a866d9f7, ig6n). `Mask::text` ends with `offsets`, which writes the sign of a `HH:MM:SS
  ±HH:MM` offset (`wake::Local::full`'s shape) as `±`, with a unit test (west, on and east of UTC alike; dates, `a -
  b` and a bare `12:30 -07:00` left alone). The golden's two `wake.at` lines go `-#:#` to `±#:#`, nothing else. A mask,
  not a pinned `TZ`: setting the variable while other tests read the local time would be a data race.
- **The held batch** (5d81b235, t2yb). `Timing` gains a hold (`hold(on)`, `wait_held`, capped at 90 s, past the test's
  60 s guard, so a test that never releases cannot hang the pool); the test runs with `cpu_cores = Some(4)`, holds the
  reads, waits for all four `Dispatched`, cancels, then releases, so each late completion comes after the cancel by
  order. The 600 ms delays go; every assertion stays.
- **The failing Jev** (c7df97df, vbju). Down fails at once, so the bound had measured only a fresh rig's first turn.
  `Slow` now sleeps 300 s against `total_secs` 30, and right after the slow mode's turn returns, health's
  `(calls_today, failed_today)` must be `(0, 0)`: a judgment books its call as it ends, so a turn that waited would
  return with `(1, 1)`. The other modes keep only their class and unchanged-turn checks.

**How it is proven.**
- **The session** (4-core VM): main failed the golden 4 of 5 under the recipe, t2yb 13 of 20 and vbju 10 of 10 under
  two busy loops per core; after, the golden 30 clean passes under the recipe, t2yb 30 of 30 under two loops a core,
  vbju 10 of 10; the golden passed under `TZ=UTC`, `America/Phoenix` and `Asia/Tokyo`, where main's failed under UTC.
  Plants: `set_wake` an hour late (fails at the guard, naming the wake +3,559,869 ms out); `Mask::text` without
  `offsets` (fails in all three zones); the kernel's `cancel` skipping one outstanding call (left 3, right 4);
  `at_loop_end` judging in place (left (1, 1), right (0, 0)). One more finding under load: `tests_m3`'s
  `a_calls_time_is_its_own_run_not_its_wait_for_the_turn` read 84 ms against its 20 ms bound once (theseus-b38m).
- **The review** (R29, review commit 48045126 on main a1bcbee2, the stack built on 872d2598): fmt, the test build,
  clippy and shape clean; **the whole workspace suite on the stack, 2,899 of 2,899** (in it the golden 26.0 s, vbju's
  test 41.8 s, t2yb's 1.0 s); the golden unloaded in Phoenix, UTC and Tokyo (26.0, 24.2, 25.5 s); three plants caught
  (the late wake, 64.7 s; the judgment in place, 33.5 s; the mask without `offsets`), t2yb's left to the report's own
  run. Beside 16 nice-0 busy loops: t2yb 0 of 10 failed, vbju 0 of 2, the golden 0 of 2 in Phoenix (76.7, 60.6 s) and
  0 of 2 in UTC (71.5, 83.3 s), far from nextest's 120 s kill.
- **FAST.** Tests only. The golden and turn-stack's golden-conversation test each take about 20 s more (26 s each in
  the suite), neither the suite's long pole (the repeating wake's 63.5 s).

**The join** (B9-core: one lock, three plain merges, one gate). The joiner, spawned at 11:32, took
`cloud-b9-core-join` at 11:34:46 to hold its place while it wrote its scripts; nothing was open ahead (B9-memtel's
done line 11:32:42), so its joincheck cleared at once at 11:37:46. Its dry run of the three alone on 2bf9e7d3
(judge-turn-cost not in the base, so route-tests' `resolve.py` was not run): clean at each step, format 23, the
golden's two wake lines `±#:#`, the flaky list empty. The merge (11:37:48 onto 2bf9e7d3): no conflict (`tests_m3.rs`,
`tests_output.rs` and the golden auto-merged), no resolve.py, no join fix; the staged tree the dry run's first, byte
for byte; 4 files, +206 −31 (`tests_m3.rs` 7,833 of its 8,050 ceiling). The warm (to 11:38:49, the test build 33.3 s,
clippy clean); **21 of 21** in Phoenix (`tests_output`, `tests_m3`'s parallel module, `tests_judge`, turn-stack's
golden conversation) and the golden and the mask's test **2 of 2** in UTC. The signed merge **2e592804** (2bf9e7d3
and 28d8d89f), 11:40:57; route-tests' and daemon-proofs' merges followed (Items 213 and
214). **The stack's one gate**, on d279767f (11:51:48, minute 51, to 11:58:50, ok; 18 s of lock
wait behind two other trees' review steps): **2,989 of 2,989** (1 slow, 24 skipped; B9-memtel's 2,980 plus the stack's
9), the suite 11:53:01 to 11:57:56, no hour crossed, and with the flaky list now empty nothing needed a retry; the
golden 24.8 s, turn-stack's conversation 24.1 s, vbju's 40.2 s. Lifecycle in every budget, the first run (cold start
p50 22.7 / p95 29.5 ms; from the config copy 23.7 / 27.5; clean shutdown 33.8 / 52.6; SIGKILL then restart 26.8 /
28.1; binary swap 49.0 / 51.2; serving 19.76 / 26.82), with **the new `cancel` row at 101.2 / 110.4 ms against its 250
ms budget** (Item 214) and the unbudgeted restore at 233.1 / 271.8 ms, its input grown by the cancel
row's sessions (theseus-ma8r); L1 start p50 6.25 ms; turn frames 5 and 9, plain p50 76.8 ms, tool call 158.7 ms (its
p95 491.5 ms the max, one slow run of the burst; not filed). The gate ran 45 s longer than the one before, about 15 s
of it the cancel row. Pushed (origin/main d279767f at 11:59:12), the three branches deleted, done line 11:59:31; the
twelve issues closed, theseus-23wh, ig6n, t2yb and vbju with 2e592804. The stack ran end to end in 21 min 44 s. The
store stays at format 23.

**The install** (installed 2026-10-06 13:03 at f589d9cb, install #7). Tests only: nothing changes on Eddie's daemon. Health after the restart (13:03:47): `theseusd check` exit 0, 9 secrets ready 1.02 s after the start, startup serving at 21.5 ms (at load 11 to 16), `cgroup: delegated`, the judge's live packs as before (`security.v3`, `route.v1`, `rerank.v1`), memory live on the `baseline` arm, voice ready, Discord ready, and no error or warning in the journal; no config change, the store at format 23, the S3 backup caught up again from 13:04:00, and no budget held.

**Divergences.** The golden's wait is a 20 s wake and a sleep by the kernel's clock, not a faster poll: the failure
was a transcript of another shape, not a slow poll. The zone is masked, not pinned.

**Known gaps.** R29's "For Eddie", each recommended and adopted by the DM thread (11:33): keep the golden's 20 s wake
(not 10); the lasting fix is a clock seam in `Parts`, so the wait becomes a step (theseus-agve, P3). The
`TZ=America/Phoenix` pin lives in no script of the repository, only in the briefs, the cloud preambles' gate line and
the joiners' scripts, and may leave them now. A same-named test in `tests_inbound` may carry the same 3 s bound (not
read). theseus-b38m (P3), the read-time bound.

### Item 213. Route tests: route.v1's tests no longer race the clock, a second switch and a switch back are held by tests, a routed turn's trace root and metrics name the model it ran on, and a switched turn's stored compilation keeps its recall drops (theseus-biy3, theseus-zvpl, theseus-490i and theseus-3urn; the ninth cloud batch's route-tests session, fired 2026-10-05 20:06 from 4a449460, Opus 5.5; 69b4b801, 26be5358, 676a0b4e and 4ab3132e; reviewed 2026-10-06 09:03 to 11:04 by local reviewer R29, stack B9-core, the second of three; joined 11:59 at 857ab09a, a signed merge onto 2e592804, the second of the stack's three merges under one lock and one gate, by the B9-core joiner; installed 2026-10-06 13:03 at f589d9cb, install #7)

**Why.**
- **theseus-biy3:** route.v1's tests expected verdicts within the route wait that came `late` under CPU starvation,
  on main too. On the session's VM, the four route test files (33 tests) failed 16 times in 5 loaded runs: the first
  compile ran past 300 ms, so the late tests' `Slow(500 ms)` verdict landed inside the 200 ms wait.
- **theseus-zvpl** (route-gaps' review, Item 181, a plant not caught): no test held a second switch's base
  (`route_to` keeps the first with `get_or_insert`) or a switch back to the base, which ends the move.
- **theseus-490i:** a routed turn's trace root kept the model the turn started on, and `Metrics::turn` reads
  `gen_ai.request.model` from it, so `theseus.turns` named the routed profile beside the base model.
- **theseus-3urn:** a switched turn's stored compilation lacked the recall's drops: `recall_compiled` took them at the
  first compile, which the switch discards.

**What landed** (theseus-core's `turn/route_step.rs`, `turn/recall_step.rs`, `trace.rs`, a test-only hook in
`judge/inbound.rs`, the tests; theseus-judge's test fake; the merge 9 files, +443 −27, without the cloud files; no
package, config key, protocol type or store format change (23 stays 23)).
- **Load-proof route tests** (69b4b801, biy3). The rig (`rig_on`, over a new `rig_built` that takes a `Parts`
  closure) sets `[routing] max_wait_ms = 5_000` before each test's tweak: a verdict ends the wait as it lands, so a
  quick one costs nothing. Lateness comes from the order of events, not a delay: theseus-judge's fake gains
  `FakeMode::Held` at the enum's end, each call's answer waiting for the next `FakeJev::release()` after it arrived
  (a counter the fake's std serving thread polls every 5 ms), then answering as `Up`. The late tests release the
  verdict after the turn that missed it and wait for it to land through a test-only `JudgeService::has_late`,
  replacing fixed 800 ms sleeps. The failing-Jev test's 2 s wall bound goes: under the 5 s wait, `no_verdict` itself
  says the failure ended the wait.
- **Two switches and a switch back** (26be5358, zvpl; tests): a session moved to Opus by `sophisticated`, then to
  glm53 by `routine_coding`, keeps `routed {glm53, from sonnet}` and runs on glm53 with no verdict; after a restart
  with another live profile `routed` clears; with routing off it runs on Sonnet, the first base, not Opus; a pane on
  `-P glm53` moved to Opus and switched back has no `routed`.
- **The routed model in the trace and the metrics** (676a0b4e, 490i, a code change). `Trace::set_root` merges attrs
  into the root under open spans; `route_to`, the one place a turn moves for a switch or a detour, sets the root's
  `profile`, `provider` and `model` to the routed target's. So `theseus.turns`, `theseus.turn.duration_ms`, the token,
  cost and tool-call points carry the routed model, and the exported root span agrees with its provider call span; a
  fallback's answering model stays the provider span's `served_model`. New `tests_route_model.rs`: two turns through
  `turn.submit` with telemetry's real pipeline, each root and the flushed `theseus.turns` points naming
  opus/anthropic/claude-opus-5-5 and glm/zai/glm-5.3-flash.
- **The recall drops** (4ab3132e, 3urn, a code change). While `t.route.defer_persist` holds (the first compile, which
  a switch may discard), `recall_compiled` clones the drops, else takes them; `keep_first` clears them once the first
  compile is the one used, so a later loop's compilation does not add them twice.

**How it is proven.**
- **The session:** after biy3, the same 33 tests 10 loaded runs, 0 failures; every route test (51: `tests_route*`,
  `routing::`, `turn::route_step::tests`) 10 loaded runs, 51 of 51 each. Plants: `from.insert` for `get_or_insert`
  (exactly zvpl's three fail); `set_root` removed (the root names sonnet); the first compile taking the drops
  (`dropped` 0 against 1).
- **The review** (R29, review commit 88a19271 over core-waits'): the stack's whole suite 2,899 of 2,899 (Item
  212). Plants on the dry run's tree (the joined state with judge-turn-cost's warm read in the rig):
  exactly the nine expected tests failed, and nothing else: `set_root` removed; `from.insert`; the first compile taking
  the drops; and R29's own, `Held`'s snapshot one release back, which answers the wrong call (the three late tests).
  **Not caught:** R29's plant removing `keep_first`'s clear passed every route and recall test, since the rig's turns
  are one loop each (theseus-y9p4).
- **Live** (R29, scratch daemons of the stack's build, a stand-in Jev scripted per request, route.v1 alone on, a
  loopback OTLP sink): a hard question judged `sophisticated` ran on opus and a "thanks!" judged `trivial` detoured to
  glm; the two exported root spans and the last metrics body's `theseus.turns` points named exactly those two. One
  session moved to opus, then switched to glm53, then kept by `chat`; restarted with routing off, its next message ran
  on sonnet, the first base. 3urn was not run live (recall needs the index tender's model files); its test and plant
  stand for it.
- **FAST.** `route_to` adds one root-attribute merge per routed turn; nothing on an unrouted turn.

**What the review found.** theseus-udzb (P3): a failed routed turn is still counted under its base model, since
`count_failed_turn` gets the request's pre-route target; since 490i the failed turn's trace root names the routed
model, so the fix reads it there. theseus-y9p4 (P3): the two-loop test for `keep_first`'s clear, with 3urn's
compaction case (which needs a catalog override giving the routed profile a window of a few thousand tokens). And one
text conflict with judge-turn-cost, not yet accepted: both edit `tests_route.rs`'s rig; R29's `route-tests/resolve.py`
keeps route-tests' `parts(&mut p);` before the build and judge-turn-cost's warm read of the ladder after it, in either
join order.

**The join** (B9-core's second, under the stack's lock; the queue, the dry run and the one gate are told in Item
212). The merge (11:41:42 onto 2e592804, after checking HEAD, origin/main and the reviewed head):
`judge/inbound.rs` and lib.rs auto-merged, no conflict; **route-tests' `resolve.py` was not run**, judge-turn-cost
not being on main (the DM thread's note, 10:35); no join fix; the staged tree the dry run's second; 9 files, +443 −27;
the rig with its `parts` and its 5 s wait and no warm read; `FakeMode` ending in `Held`. The warm (to 11:44:33, the
test build 1 m 53 s, clippy clean); **52 of 52** in 17.7 s (the 51 route tests, R29's count, and `set_root`'s unit
test). The signed merge **857ab09a** (2e592804 and 583f0747), 11:45:30. Its gate, the stack's, 2,989 of 2,989, ok.
Pushed 11:59:12, the branch deleted, done line 11:59:31; theseus-biy3, zvpl, 490i and 3urn closed with 857ab09a. The
store stays at format 23.

**The install** (installed 2026-10-06 13:03 at f589d9cb, install #7). On Eddie's daemon a routed turn's trace root, and with it every point of the turn's
metrics, names the profile, provider and model it ran on, for a switch or a detour (route.v1 is live there); an OTLP
collector's `theseus.turns` series by model moves accordingly. A switched turn's stored compilation keeps its recall
drops. No config key. Health after the restart (13:03:47): `theseusd check` exit 0, 9 secrets ready 1.02 s after the start, startup serving at 21.5 ms (at load 11 to 16), `cgroup: delegated`, the judge's live packs as before (`security.v3`, `route.v1`, `rerank.v1`), memory live on the `baseline` arm, voice ready, Discord ready, and no error or warning in the journal; no config change, the store at format 23, the S3 backup caught up again from 13:04:00, and no budget held.

**Divergences.** Lateness is made by a held fake, released by the test, not by a delay. A failed routed turn's count
was left to theseus-udzb, outside route's files.

**Known gaps.** R29's "For Eddie", adopted (12:31): theseus-udzb with the next telemetry row; theseus-y9p4 before
anything else touches `recall_compiled`; both given to batch 10's core-gaps task. When judge-turn-cost joins, run
`route-tests/resolve.py` at whichever of the two joins second. theseus-core's AGENTS.md Routing bullet (490i's root,
3urn's drops, biy3's rig wait and `Held`) and its test list, owed by the review, were not written at the join.

### Item 214. Daemon proofs: a `--stdio` daemon's stop drops the runtime, so its store closes before the process ends; the page tests write unsynced; the flaky list is empty; the lifecycle bench times a cancel's round trip; and an MCP kill test only the L1 role's watch can pass (theseus-xbtr, theseus-hohs, theseus-nh1k and theseus-grxh; the ninth cloud batch's daemon-proofs session, fired 2026-10-05 20:06 from 4a449460, Opus 5.5; 04d051d6, c9bfa313, 378c1fc5, a0cd63e2 and feb02b16; reviewed 2026-10-06 09:03 to 11:04 by local reviewer R29, stack B9-core, the last of three; joined 11:59 at d279767f, a signed merge onto 857ab09a, the third of the stack's three merges under one lock and one gate, by the B9-core joiner; installed 2026-10-06 13:03 at f589d9cb, install #7)

**Why.**
- **theseus-xbtr:** `versions::a_stdio_daemon_stops_cleanly_on_a_sigterm_or_a_sigint` failed once in a lane's gate on
  2026-10-03 under load: after a SIGINT stop, the next start replayed 10 records at `last_position` 20. It sat on the
  flaky list.
- **theseus-hohs** (R1c, compaction-roots' review, Item 131): `tests_pages::a_filtered_page_equals_the_scans_answer`
  took 21 s alone on the owner's machine and hit nextest's 120 s kill under load.
- **theseus-nh1k** (R18, timing-flakes' review, Item 189): since cs71's fix nothing bounded a cancel's
  round trip under 5 s; a planted 2.5 s sleep passed.
- **theseus-grxh** (R1, extend-propose's review, Item 118): no test proved the L1 role's watch of the daemon's pidfd,
  since the fake MCP server exits at its stdin's end by itself.

**What landed** (theseusd, theseus-sim, theseus-store's tests, theseus-sim's fake MCP server, the nextest config;
the merge 11 files, +332 −72, without the cloud files; no package, config key, protocol type or store format change
(23 stays 23)).
- **The `--stdio` stop, at its cause** (04d051d6, xbtr). The session could not make it fail naturally (30 of 30 under
  the recipe, 30 of 30 beside 16 busy loops) and read the cause from the numbers: 10 replayed at position 20 is the
  second run's every record, so the stop's checkpoint itself was lost, not one late row. `main` dropped the socket
  daemon's runtime (`drop(rt)`, which waits for every blocking task) but ended the stdio daemon's with
  `shutdown_timeout(500 ms)`, since tokio's stdin reads on the blocking pool and only the client's end of the pipe
  ends that read (theseus-p7q). The stop's last checkpoint is a non-durable commit that redb's close makes durable as
  the store drops, so any blocking task still holding the core 500 ms after the stop (on a loaded machine: the
  outbox's warm read, a warm build, consolidation, the learning pass) kept the store open as the process ended, and
  the next start replayed the whole run and repaired the index. Now `stdio.rs` (new, 96 lines) copies the pipes with
  two plain threads to and from one end of a `UnixStream::pair()`, and the core serves the other end as an ordinary
  tokio stream; the stdio arm ends with `drop(rt)` as the socket daemon's does, then waits up to 500 ms for the stdout
  thread (`stdio::flush`). The stdin thread stays blocked in its read holding nothing of the core; the stdout thread
  writes each read whole and, at its end or a write error, shuts the pair's read side, so a client gone from stdout
  fails the core's next write as the closed pipe did. A debug build's plant, `THESEUS_TEST_HOLD_CORE_MS`, holds the
  core on the blocking pool from the stop's start. A signal stop no longer waits out the 500 ms: the stdio stop test
  takes 0.12 s alone, not 1.1 s. theseusd's AGENTS.md has a trap line for the relay and the plant.
- **The page tests** (c9bfa313, hohs). The VM's `fdatasync` cost about 27 µs, so the 21 s was about 14 ms × 1,511
  syncs; and with the sync off a starved run still took 34 s, each frame a handoff to the store's writer thread. So
  both: `fsync: false` for the module's stores (nothing there asserts the WAL's sync), and the page test's same rows
  appended in frames of 25 (and at batch 700, so the checkpoint stays where it was), checked by a digest of every walked
  row, the last position (3,200), the row count (2,985) and the checkpoint's position (1,480), all unchanged, with all
  400 queries kept. The page test went from 1,511 syncs to 11 and from 41 to 49 s starved to 10.7 to 11.1.
- **The flaky list is empty** (378c1fc5). xbtr's entry goes with its fix; 81kk's override was left behind after its
  fix at its cause (cloud gate-flakes, joined at dd94bc6b on 10-04, Item 104), and goes too.
- **A cancel's round trip** (a0cd63e2, nh1k). In the lifecycle bench, not `bench jobs`, which runs no daemon: a `cancel`
  phase in the default phases (so the gate runs it), each run starting a real `proc.run` job through a turn
  (`Rig::start_job`) and timing `execution.cancel` from request to answer; the answer must cancel one action with one
  verdict, `termination_verified`, `killed` > 0 and `survivors` 0, and health must show nothing dispatched, else the
  bench fails. Its budget, 250 ms with no margin of its own, is Theseus's own, not §9's. On the cloud VM: quiet p50
  about 41 ms, p95 43; beside 16 busy loops p50 74, p95 95.
- **The L1 role's watch** (feb02b16, grxh). `theseus-sim fake-mcp --outlive-stdin` waits forever after its input
  ends, so only a signal ends it; `mcp_l1::a_server_that_outlives_its_input_ends_with_the_daemons_kill_9` runs it in
  L1, kills the daemon with `-9`, and requires the role, the init and the server gone within 10 s, which only the role's
  pidfd watch of the daemon can do.

**How it is proven.**
- **The session:** the new `a_stdio_daemons_stop_waits_for_a_task_that_holds_the_core` (the plant at 1,500 ms, then
  SIGINT: exit 0, the next start replays and repairs nothing, the stop at least 1.5 s); its plant, the old end, fails
  with the incident's shape (`replayed_into_index` 10, `index_repaired` true). The two stop tests and 81kk's clean-stop
  test, 30 iterations under the recipe: 30 of 30. All of `tests_pages` 5 times beside 8 busy loops: 5 of 5. The 2.5 s
  sleep after `terminate_all`: p95 2,545.6 ms, "LIFECYCLE BUDGET MISSED". The `DaemonGone` lines removed from the
  kernel's `mcp_l1.rs`: the new test fails at 10.2 s, "outlived the daemon's kill -9", as an ordinary user (as root the
  L1 tests return at once, the refusal path).
- **The review** (R29, review commit 872d2598 over route-tests'): the stack's whole suite **2,899 of 2,899** with the
  flaky list empty (Item 212); the mcp_l1 tests run as an ordinary user, so L1 really ran. Plants: the
  old stdio end (left (10, true), right (0, false)); the `DaemonGone` lines removed (10.3 s); the 2.5 s sleep (p50
  2,627, p95 2,720 ms, missed). R29 read the relay against every edge: a client that closes stdout first, a write error
  mid-reply, the stdin thread blocked at exit, the 500 ms flush wait, back-pressure. **Live** (scratch daemons of the
  stack's build): stdout closed first, the daemon served on until stdin closed, then stopped clean (exit 0 in 0.03 s,
  the next start replaying 0); 400 requests written unread, then 400 replies, 2,080,700 bytes, no line torn; SIGTERM
  with stdin held open and stdout unread, exit 0 in 32 ms. Beside 16 busy loops the stdio stop passed 20 of 20, the
  clean stop 20 of 20 and the new hold test 5 of 5.
- **The cancel row on the owner's machine** (R29, 16 cores, WSL2 ext4): CPU load barely moves it; the disk does.
  Quiet (the morning's one quiet window, on the dry run's build): p50 74 to 77 ms, p95 82 to 105; beside busy loops
  with the disk quiet p50 about 107; beside other builds' IO p50 127 to 181, with one strict miss of 250 ms in 7 runs
  (271.8, one 470 ms outlier). Two costs, read in the code: a ~40 ms floor, the stop's poll (`Stopping::poll` looks at
  about 0, 10, 30 and 70 ms, and the wrapper's `tree::stop` sleeps 10 ms before its next look), and on this disk the
  cancel's five synced frames (`fdatasync` about 9 ms quiet, 23 ms beside builds). A probe polling every 2 and 1 ms took
  the quiet p50 from about 75 to 44 ms.
- **FAST.** Nothing on the socket daemon's start, stop or turn path: its Done arm was already `drop(rt)`, and the relay
  and the plant run in the stdio arm alone; frames 5 and 9. The gate grows by about 15 s for the cancel row.

**What the review found.** theseus-yg1y (P2): a `--stdio` daemon answers the `shutdown` method and keeps serving
(live: answered in 8 ms, still serving 5 s later), since only the socket loop awaits `core.shutdown`; theseusd's
AGENTS.md invariant "every clean stop is one path: the `shutdown` method, SIGINT, SIGTERM" is false for `--stdio`
until it lands. theseus-jo7f (P3): `Exit::Exec`, a restart onto a changed note, still ends with `shutdown_timeout`, so
xbtr's store-close hazard remains for a restart in both modes, and for `--stdio` the relay's last bytes can be cut at
the exec. theseus-dwoj (P3, FAST): fold the cancel's frames and make both waits event-driven, then bring the budget near
50 ms.

**The join** (B9-core's third, under the stack's lock; the queue, the dry run and the one gate are told in Item
212). The merge (11:45:34 onto 857ab09a): theseus-sim's and theseusd's `main.rs` and `versions.rs`
auto-merged, no conflict, no resolve.py, no join fix; `git diff --cached --check` stopped the joiner's script on one
line, `.config/nextest.toml`'s new blank line at its end, which is the branch's own reviewed text (the flaky list's
last overrides removed), so it was kept and the remaining steps run by hand; the staged tree the dry run's stack tree;
11 files, +332 −72; no override left in nextest.toml; the cancel row's budget 250 ms (`"cancel" => Some(250.0)`). The
warm (to 11:48:10, the test build 55.3 s, clippy clean); theseusd's stops, versions and mcp_l1, theseus-sim and the
page tests as the ordinary user: **71 of 72**, the one red main's
`stops::a_stop_of_three_jobs_that_ignore_sigterm_takes_one_grace_and_holds_no_worker` (a job's SIGTERM file existing
but empty, at nice 10 beside two other trees' nice-0 builds), on no path of this branch's; alone right after it passed,
and with the flaky list empty a gate red on it would be real, so it was filed (theseus-y0lm, P2). The signed merge
**d279767f** (857ab09a and e3e34372), 11:51:36, its message naming the kept blank line and the stop test's rerun. The
stack's gate on it: 2,989 of 2,989, ok, the stop test passing there (5.8 s), and **the cancel row p50 101.2 / p95 110.4
ms against its 250 ms budget**, between R29's quiet and loaded readings. Pushed 11:59:12, the branch deleted, done line
11:59:31; theseus-xbtr, hohs, nh1k and grxh closed with d279767f. The store stays at format 23.

**The install** (installed 2026-10-06 13:03 at f589d9cb, install #7). Eddie's daemon serves the socket, whose stop already dropped the runtime, so the
`--stdio` fix changes nothing there; a `theseus --spawn` client's `--stdio` daemon now closes its store before it
ends. The lifecycle bench's `cancel` row is the gate's, not the daemon's. No config key. Health after the restart (13:03:47): `theseusd check` exit 0, 9 secrets ready 1.02 s after the start, startup serving at 21.5 ms (at load 11 to 16), `cgroup: delegated`, the judge's live packs as before (`security.v3`, `route.v1`, `rerank.v1`), memory live on the `baseline` arm, voice ready, Discord ready, and no error or warning in the journal; no config change, the store at format 23, the S3 backup caught up again from 13:04:00, and no budget held.

**Divergences.** The cancel row joined at 250 ms with no margin, as the branch had it, not R29's 250 with a 50 ms
margin (the DM thread's call, 11:33 and 12:31: until theseus-dwoj brings it near 50). xbtr was fixed at its cause
without a natural reproduction, from the incident's numbers and a forced interleaving.

**Known gaps.** theseus-yg1y (P2) and theseus-jo7f (P3), both given to batch 10's daemon-stops task; theseus-dwoj (P3,
FAST), batch 10's cancel-fast task; theseus-y0lm (P2: main's stop test under load; an atomic write and a telling
message proposed) and theseus-ma8r (P3: the bench's restore row now restores the cancel row's sessions, its unbudgeted
p50 138.8 to 233.1 ms), filed at the join. Owed by the reviews and not written at the join: `scripts/gate.sh`'s comment
above `lifecycle()` and `lifecycle.rs`'s "six phases" (nine now, `cancel` among them), theseusd's AGENTS.md stop
invariant, and the cloud preamble's flaky-list line, which reads "empty" now.

### Item 215. Voice echo: in a voice call an answer that repeats its question's words and a "yes" on a question's last word are turns again (an echo must be a near-whole, in-order copy, a speaker is echo-prone only after two echoes, and "mhmm", "gotcha" and "oh okay" are backchannels) (theseus-3ug0 and theseus-1cz8; R27's two findings on voice-turns; the tenth cloud batch's voice-echo session, launched at voice-turns' join and fired 2026-10-06 09:53 from f33f0eca, Opus 5.5, finished 10:35; 117aee96, 8aa74a8e and b72fc3ea; reviewed 11:34 to 12:14 by local reviewer R31; joined 12:44 at f589d9cb, a signed merge onto d279767f, by the batch-10 voice-echo joiner; installed 2026-10-06 13:03 at f589d9cb, install #7)

**Why.** Local reviewer R27, reviewing voice-turns (Item 202), found two P2s in its rules for what
is heard over Theseus's speech, each proven by a probe through the engine's seam:
- **theseus-3ug0:** an answer that repeats its question's words was heard as an echo, so it made no turn, and its
  speaker lost the 300 ms stop for the rest of the call. `is_echo` took 60 % of the distinct words, so "Yes, deploy it
  now." (3 of its 4 distinct words) and "The daily view." (3 of 3) were echoes.
- **theseus-1cz8:** a "yes" that answers a closing question was no turn when it overlapped the question's end (an
  utterance's overlap was fixed at its first speech frame, so a "Yes." begun 200 ms before the question ended stayed
  over speech and was heard as a backchannel), or when another reply was queued (`what_is_over` gave speech for any
  queue front, even a reply still synthesizing that had never played). And common backchannels ("mhmm" as the
  provider spells it, "gotcha", "oh okay") were missing from the list, so they cut a reply.

**What landed** (`crates/theseus-voice` only: `src/heard.rs` 251 to 331 lines, `src/engine.rs` 1,247 to 1,271,
`tests/turns.rs`, the crate's AGENTS.md; the merge 4 files, +436 −48, without the cloud files; no store format change
(23 stays 23), no protocol type, config key or package).
- **An echo is a near-whole, in-order copy; echo-prone takes two** (117aee96, 3ug0). `is_echo` takes the longest run of
  the utterance's words found contiguously and in order in **one** candidate sentence (`longest_run`, a word-level
  longest common substring, O(n·m) per sentence over at most a few sentences); the run must be at least `ECHO_RUN` = 3
  words and at least 80 % of the utterance's words. One or two words are never an echo. The engine's `echo_prone` set
  becomes `echoes`, a count per speaker, and a speaker's stop is off only from `ECHO_PRONE` = 2 echoes. One test of
  voice-turns' asserted the replaced rule (echo-prone after one echo); it is rewritten for two, with a true copy as its
  second echo.
- **The tail** (8aa74a8e, 1cz8). In `what_is_over`, a queue front that has not begun (`opens`, no hold) gives the
  tail's overlap: `Tail` when a sentence ended in the last 1.2 s, else none, as with an empty queue (its `over` is
  still `Saying` that item). `Opening` gains `last`: the utterance began over the queue's only sentence, which had
  begun; when such an utterance closes and the queue is empty, its overlap becomes `Tail`. A held front stays queued,
  so an utterance that stopped the question (300 ms or more over it) is still over speech. In the tail only echo is
  checked, so a "yes" there answers.
- **The backchannels** (b72fc3ea, 1cz8). The list gains "mm hmm", "mhmm", "mmhm", "mmhmm", "mmm", "uhhuh", "uh hum"
  and "gotcha"; "oh" counts before a listed word ("oh okay", "oh yeah", "oh I see"), the 3-word cap still holding; "oh"
  alone or last is no backchannel.
- The crate's AGENTS.md gives the echo, tail and backchannel rules in a paragraph.

**How it is proven.**
- **The session's tests** (`tests/turns.rs`, 14 tests to 21 by R31's count, seven new and one rewritten; heard.rs's
  unit tests rewritten for the run rule): an
  either/or answer over its question is a turn and cuts it, with no replay; an answer with its question's words 0.5 s
  after it is a turn; the played sentence heard back whole is still an echo, with a resume; after one echo a speaker's
  "Hang on, wait a second." over the replay still stops it at 300 ms and cuts it at the transcript; after two, the stop
  is off; a "Yes." 200 ms over a closing question's end is a turn at 4.2 s, the question not stopped; a "Yeah." after a
  closing question while another speaker's reply is queued and synthesizing is a turn, and that reply is superseded
  (`Cut { Superseded }`, voice-turns' existing rule that a listed speaker's words supersede a waiting reply); "mhmm" and
  "oh okay" over a long reply each stop it and resume it as `Backchannel`, five clips playing, no turn. Each of the
  session's five plants failed its tests (the 60 % rule back; `ECHO_PRONE` 1; a front that has not begun treated as
  speech; the close-time reclassification off; "mhmm" removed). theseus-voice and theseus-discord, 192 of 192; under
  load, theseus-voice's 70 of 70 three times.
- **The review** (R31, review commit 355281aa on main 2bf9e7d3; clean on d279767f too): fmt, the test build, clippy,
  theseus-protocol's 31 and shape clean; theseus-voice and theseus-discord **199 of 199** (voice 70, discord 129);
  **the whole workspace suite, 2,988 of 2,988**; theseus-voice under load 70 of 70 three times (its turn tests run on
  tokio's paused clock, so load cannot move their times). **R27's probes inverted:** the four `finding_*` tests of 3ug0
  and 1cz8 now fail and all five `holds_*` pass. **11 planted reverts, 10 caught**: the report's five and six of R31's,
  two of them FAST claims (the hold dropping the held audio, so a resume would synthesize again: the sound test's
  synthesis count fails; the stop one 20 ms frame late: 14 tests fail); the one missed, the echo count shared by all
  speakers, is caught by R31's probe C1 (noted on theseus-e6mj). **11 probes** of R31's, each asserting what the branch
  does, with an A/B against voice-turns' rules and two fix sketches. The public-repository scrub is clean; R27's probe,
  which names a real person, was in the tree only while it ran and was never committed.
- **FAST.** The rules still run once per transcript, off the audio path. The stop still comes on the same tick (the
  same speech frames against the same threshold, the same `Out::Stop`); the one change there is a count's compare where
  a set lookup was. A resume still replays the held audio with no new synthesis (the branch does not touch that path).
  The new matching is O(n·m) over bounded inputs: n at most a 30 s utterance's words (about 150), the candidates at most
  about 31 s of played speech; a few thousand word comparisons in a typical call, about 10^5 at the outside, under a
  millisecond. Nothing runs before serving or on the core's turn path.

**What the review found.** **theseus-j2ut (P2):** the brief's rule judges each sentence on its own with a 3-word floor,
so three real echoes are now heard as words, where voice-turns made them echoes (the A/B with its rule planted back):
an echo-prone speaker's echo of a whole multi-sentence reply, which arrives as one utterance (its run within any one
sentence a fraction of it), becomes a turn of Theseus's own reply (E1); a first echo, cut to one or two words by its
own 300 ms stop, cuts the reply and is a turn (E2), and since most first echoes are that short, a speaker on
loudspeakers rarely reaches the two verdicts that would make them echo-prone; and an echo across a sentence boundary
is words (E3). It bites only where echo cancellation fails (loud laptop speakers, a speakerphone); Discord's clients
cancel most echo. The fix, half of it proven: measure the run also over the candidate sentences joined in the order
they played (one line; it makes E1 and E3 echoes and moves no turns.rs test), and for E2 the report's position rule (a
run from the head of the sentence playing when the utterance began, begun within about 0.5 s of its start, is an echo
from 2 words). **theseus-q4pc (P3):** a "yes" on a closing question's last word is still no turn when another
speaker's reply is queued behind the question, 1cz8's two cases at once; a two-edit fix is proven (`last` from the
item's own index, and the close's flip when the front has not begun; 79 of 81 others pass). The report's own residual:
a 3-word answer that is exactly a run of its question ("The monthly view." for "Do you want the daily or the monthly
view?") is still an echo, though now one verdict, so its speaker keeps the stop.

**The join** (batch 10, the branch alone). Nothing was queued ahead: at 12:32 main = origin/main = d279767f. The
joiner's dry run before the lock (12:34:51) gave the tree R31's own 12:01 dry run on d279767f gave. The guarded take
(12:35:10) took `cloud-voice-echo-join`, found the queue clear, and ran the dry run on d279767f (merge-tree clean,
`MANIFEST_FORMAT` main 23, branch 23, merged 23, no file changed on both sides since the base f33f0eca, theseus-voice
and theseus-discord identical to R31's review commit); the merge: no conflict, no resolve.py, no join fix; the staged
tree the dry run's less the cloud files; 4 files, +436 −48. The warm (12:35:29 to 12:35:58, the test build 21.77 s,
clippy clean); theseus-voice, theseus-discord, theseus-protocol and `tests_output`, **233 of 233** in 24.4 s (voice 70
and discord 129, R31's 199; protocol 31, protocol.gen unchanged; `tests_output` 3). The signed merge **f589d9cb**
(d279767f and f5eae083), 12:37:06. Its gate (12:37:19, minute 37, to 12:43:30, ok; no lock wait): **2,997 of 2,997**
(1 slow, 24 skipped; the B9-core gate's 2,989 plus the branch's 8), the suite about 12:37:48 to 12:42:40, no hour
crossed, no known load flake red; lifecycle in every budget (cold start p50 21.6 / p95 22.4 ms; from the config copy
22.4 / 22.8; clean shutdown 31.3 / 49.1; a post in flight 71.6 / 78.9; SIGKILL then restart 26.1 / 33.0; binary swap
46.2 / 53.9; restore 231.2 / 254.4; a cancel's round trip 98.2 / 105.2 against 250; serving 17.29 / 26.59); L1 start
p50 6.11 ms; turn frames 5 and 9, plain p50 71.7 ms (its p95 240.2 ms, one run of ten), tool call 163.8 ms. Pushed
12:43:37, the branch deleted, done line 12:44:12; theseus-3ug0 and 1cz8 closed with the hash. The joiner noted a lone
slow turn-bench run two gates in a row, once on each kind of turn; checked against the bench history, such runs (a p95
more than twice its p50) came in 17 of 191 gates since 10-02, not new, and theseus-w7dk (P3, FAST) asks each run's wall
time and slowest frame to be printed (the chain log, 12:51). 12 min from the take to the done line. The store stays at
format 23.

**The install** (installed 2026-10-06 13:03 at f589d9cb, install #7). In a voice call on Eddie's daemon, an either/or answer, an answer that reuses its
question's words, and a "yes" on a question's last word are turns again; one false echo verdict no longer costs a
speaker their stop; "mhmm", "gotcha" and "oh okay" no longer cut a reply. To watch live (R31): a "yeah" said while a
reply synthesizes or waits for the floor now supersedes it (as one said while the model prepares already did); an
"okay" over the acknowledgment chime is a turn; a short "yes" in a reply's last ~0.94 s is a turn, question or not. On
loudspeakers without working echo cancellation, expect theseus-j2ut. No config key. Health after the restart (13:03:47): `theseusd check` exit 0, 9 secrets ready 1.02 s after the start, startup serving at 21.5 ms (at load 11 to 16), `cgroup: delegated`, the judge's live packs as before (`security.v3`, `route.v1`, `rerank.v1`), memory live on the `baseline` arm, voice ready, Discord ready, and no error or warning in the journal; no config change, the store at format 23, the S3 backup caught up again from 13:04:00, and no budget held.

**Divergences.** The echo rule is the brief's (a run in one sentence, 3 words, 80 %); its known cost is theseus-j2ut.
The session did not choose among its three proposals for the 3-word residual. A row's `over` for a reply queued but not
begun stays `Saying { sentence: 0 }`, though its overlap is now the tail (left to stay out of voice-heard's area;
voice-heard's note reads only `Preparing`, so no note is wrong).

**Known gaps.** R31's "For Eddie", each recommended and adopted by the DM thread (12:33): keep 80 %, measured also over
the sentences joined in play order; keep 3 words as the floor for a run found anywhere, with the 2-word head-run rule,
not 4 words; keep two verdicts (per speaker and per call); keep all the added backchannels (a hummed "mm-mm", a no, is a
backchannel through the old list's `mm`, worth treating as words later). theseus-j2ut (P2: fix it before any
loudspeaker call), theseus-q4pc (P3) and the per-speaker echo count's test (on theseus-e6mj) are folded into the
not-yet-launched voice-holds task, which waits for voice-heard's join. voice-heard, which shares engine.rs in separate
hunks, joins second, and its joiner runs theseus-voice's and theseus-discord's suites too.
