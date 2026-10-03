# The Ship of Theseus, chapter 11: Part III, A4 (M3.6 Daily Driver) and Items 1 to 20 ([index](README.md))
## A4. M3.6 Daily Driver (theseus-5jl)

_P5d's items, each recorded when its review ends. Items 1 to 4 run before M3.5 (P5d, Order). The milestone closes when its prove passes: Eddie carries his daily Discord work on Theseus end to end._

### Item 1. Budgets in dollars (theseus-0sg, theseus-woy; 2026-09-29, 00:55–01:55; cb824c7, fdf4813, d6183d1)

**Why it moved up.** Eddie's Discord DM ended `budget_exhausted` at about $0.42 on 2026-09-29 at 00:05; its session's recorded cost is $0.4216.
- Its execution had opened under the old limit of 1,000,000 units, and a limit was fixed when an execution opened.
- After 15 turns and 28 tool calls it had spent 877,683 units, most of them cache reads counted at full weight. At Sonnet 5's prices a cache read costs $0.20 per million tokens and an output token $10, and units weighed them the same.
- The next call needed 172,068 units (Sonnet 5.5's 128 K output ceiling plus about 44 K of input), and 112,317 were left.
- The place rebound Eddie to a fresh session, which started with an empty conversation.

Eddie decided the design at 00:09 (§3.13, where his words are).

**What exists.**
- **The catalog in micro-dollars** (cb824c7).
  - A price in dollars per million tokens is micro-dollars per token. `cost_micros` prices a call's input, output, cache reads, and cache writes, each at its own price. `reserve_micros` prices the output cap at the output price, plus the input estimate at the input price. The arithmetic is integer, with one rounding up per call.
  - The template lists all 12 built-in models, uncommented, with their four prices. `Catalog::template_tables()` writes that text, and a test holds it equal to the built-in table.
  - At startup, one warning names every built-in model that has no table in the config.
- **The kernel** (fdf4813). A budget is micro-dollars: the limit, the spend, the reservations, and the amounts held unknown. The control reserve is gone.
  - A reservation that does not fit is refused with `OverBudget`, and it writes nothing. The turn then asks (`ask_budget`: a planned `budget.reset` action, one open at a time) and parks on the new `Wake::Budget`.
  - `reset_budget` is the only transition that lowers spend, and it is one frame. The question settles as succeeded, and the spend goes to $0. Reservations and held amounts stay, and `resets` goes up by one. A `budget.reset` row records who approved, the spend before, and the limit. The execution is queued, so the waiting call goes ahead.
  - A decline leaves it waiting, and new input asks again. Terminal stays terminal.
  - A versioned reader (schema 1) serves executions stored with unit budgets. Each gets the configured limit, its spend comes from its session's recorded `cost_usd`, and the unit figures are kept as `units_before`. Startup rewrites each one once, in one frame, with a `budget.migrated` row.
- **The simulator.** Five new random operations: ask, approve, decline, new input on a budget wait, and a cancel of one. Six new invariants:
  - nothing is reserved past the limit;
  - spend goes down only by an approved reset;
  - an execution has at most one open question;
  - no unit budget survives startup;
  - terminal stays terminal;
  - nothing new ends `budget_exhausted`.

  40 seeds × 300 steps held them, with 397 questions and 171 resets. The sweep found one real bug, fixed in the commit: a turn that ended its execution left its question open.
- **The core.** `[kernel] spend_limit_usd = 100.0`. `default_budget` and `control_reserve` load, are ignored, and warn once together. A model with no price anywhere is not called (class `unpriced`).
- **The narrative speaks dollars:** the limit when a session opens, each call's reservation, the budget wait ("reached its $100 limit; waiting for the operator to reset it"), and the reset ("Spend reset to $0 by X; continuing").
- **theseus-woy.** A `TurnError`'s text no longer embeds its cause, so a failure says it once.
- **Surfaces.**
  - The protocol, the CLI, and Discord count budgets in dollars. `theseus executions` and `theseus confirm` show budgets and questions.
  - Discord posts the question with Approve and Decline in the session's place, and only its listed users can press them.
  - The web UI (d6183d1) shows dollars, a reset count, an "at its limit" pill, and summaries for `budget.asked`, `budget.reset`, and `budget.migrated` in the Observatory. The transcript shows the question as a card: "Reset to $0 and continue" or "Keep waiting".

**How it is proven.**
- **Tests.** 167, up from 151; the gate reran on d6183d1 at 01:48. The new ones cover:
  - in the kernel: a call over the limit waits, and an approved reset continues; a declined question keeps waiting, and new input asks again; a reset approved before the turn parks still continues; cancelling a budget wait closes its question, and terminal stays terminal; an execution that ends closes its open question; executions stored with unit budgets serve in dollars;
  - in the core: a session at its limit asks, and an approved reset makes the waiting call; a declined reset keeps waiting, and the next message asks again; the frame budget still holds at 17;
  - in the catalog: a call's reservation and cost in micro-dollars match the prices, and the template equals the built-in table;
  - in the config: Eddie's vault config, with its unit budget, loads with one warning.
- **Live** (Tabitha, 01:51–01:53; a release build on a scratch daemon over a copy of Eddie's store; `spend_limit_usd = 0.002`, and the glm profile capped at 2,048 output tokens, so one reservation is about $0.0014):
  - Startup warned twice: once for the retired unit keys, and once for the 12 built-in models without a `[catalog]` table.
  - Eddie's old exhausted DM execution read in dollars and stayed `budget_exhausted`, at $0.4216, showing "before dollars: 877683 of 1000000 units".
  - Turn 1 (GLM, `fs.list`) ran 2 loops and 1 tool call, for $0.0008.
  - Turn 2 asked: "This session has spent $0.0008 of its $0.002 limit. Reset its spend to $0 and continue?" Its `budget.asked` row reads spent $0.000814, needed $0.001431, limit $0.002.
  - `theseus confirm` approved it. The `budget.reset` row reads by `sock#7`, spent before $0.000814, resets 1. The resumed turn ran `fs.list` and answered after 2 loops, for $0.0003.
  - Afterwards the session's lifetime cost was $0.0011, the sum of both turns, so the reset lowered nothing. The execution showed $0.0003 since the reset.
- **The run.** The subagent's run was aborted ("CLI run aborted") at about 01:42. It had pushed cb824c7 and fdf4813 and left the web half staged. Tabitha finished the step in review, from 01:48 to 01:55: she committed the web half (d6183d1), reran the gate, ran the live check, and wrote the report. Installed at 01:53.

**Follow-ups.**
- theseus-kks. A call whose reservation is larger than the whole limit never fits after a reset, so the question would come back after each approval. It cannot happen at $100 with today's models, whose largest reservation is about $3, but a tiny test limit reaches it. The question should say so and name the fixes: raise the limit, or lower `max_output_tokens`.
- An unpriced model is now refused. That is a hard stop where A3 had it run, and Eddie may prefer that it ask.
- `theseus executions` prints limits to two decimals, so a $0.002 test limit shows as "$0.00".
- Discord's question was not exercised live, because a second daemon must not bind the bot token while Eddie's runs. `render.rs` tests cover it.
- Eddie's note needs the new template: `narrative = true` at the top, `spend_limit_usd = 100.0` in place of the unit keys, and the twelve `[catalog]` tables.
- A session keeps the limit it opened with. A dollar-era execution stores its limit (`Budget::new` at open), and only a unit-era record takes the configured limit when it is read, so a changed `spend_limit_usd` reaches new sessions only. That includes a long-lived Discord place. Verified in the code at review, 02:50. It is held for Eddie whether an open session should follow the config.

**Divergence from Parts I and II.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| An unknown model runs with a startup warning and its cost unknown (A3; P5's catalog bullet) | A model priced neither in the config nor in the built-in table is refused as `unpriced` | Under a dollar limit, nothing may run unpriced | Held for Eddie: he may prefer that it ask (theseus-kks) |
| A reserved control and cleanup budget, so that reaching a ceiling never prevents a cancel (§3.13; P4) | Retired | Nothing at the limit ends work or refuses a cancel | Part I §3.13 amended |
| Reaching the limit ends the execution as `budget_exhausted` (§3.15; the M2 kernel) | The session waits and asks for a reset. Nothing new ends `budget_exhausted`, and old ones stay ended | Eddie's decision, 00:09 | Part I §1, §2, §3.13, and §3.15 amended |
| Money budgets land together with provider-safe caching (§4.5; theseus-ev1) | Money budgets landed first | Eddie's DM hit its unit limit in real use | Provider-safe caching stays scheduled (theseus-ev1); built in two parts, Item 16's cache lane and Item 29 |

### Item 2. The workspace: context files and roots (theseus-58a; 2026-09-29, 01:55–02:15; 76e7d35, a29e9f7)

**What exists.**
- `context_files` on a profile, with `[model].context_files` as the default, compiled into the system block with their digests as §4.4 describes.
  - Loading checks spelling only, and a relative path fails with the key named.
  - `ContextFiles` is the daemon's stat cache: one per runner, empty at startup, with reads capped at 64 KB + 1 byte.
- Records:
  - `Manifest.context_files` and the `context.compiled` row's `context_files` (both absent with no files, so stored manifests compare equal);
  - a `context.file_missing` row once per file per run;
  - the narrative's context line ends with ", N context files (M missing)".
- The template sets `context_files = []` at `[model]` with an example, shows the profile line commented, and shows `roots = ["/home/zeroaltitude/reports"]` commented under `[tools]`.
- The Observatory's Context view lists the current compilation's files with their digests.

**How it is proven.**
- **Tests.** 176, 9 of them new. They cover:
  - the rule in the system block and the digest in the manifest;
  - four turns giving exactly `[new_session, system_changed]`, and an unchanged file that is not read again;
  - a missing file warned once while the turn runs;
  - a cut at the cap;
  - a config without files keeping the same system bytes and manifest;
  - inheritance and validation, and the template;
  - the stat cache's hit, miss, and racy reread.

  The frame budget holds: a plain turn writes 17 frames or fewer.
- **Live**, on a scratch daemon over a copy of Eddie's store:
  - GLM ended an answer with "Theseus", which only a context file asked for, with no tool call;
  - after the file was edited to "Ithaca", the next turn recompiled once (`system_changed`) and ended with "Ithaca";
  - the turn after that appended.

  Eddie's vault config loads under the new binary with the same single warning as before.
- **Review** (02:26). The gate reran at 176 tests. Tabitha's own check on the release build, over a store copy, got a GLM answer that ended with "Ithaca", a rule only the context file stated, and the manifest's digest equals `sha256sum` of the file. Installed at 02:24.

**Divergence from Parts I and II.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| An edit recompiles "on the next loop" (the step's brief) | On the next turn's first loop. The system block is fixed for a turn | A recompile inside a tool loop strips the prefix's thinking between a tool call and its result | Kept |
| `context_files = []` on a profile in the template | Set at `[model]`. On the profile the line is commented | A live `[]` on a profile hides the `[model]` default | Kept |
| A stat of mtime and size per compile | Size, mtime, and inode per turn, plus the two-second racy reread | The inode catches an atomic replace, and the racy rule a same-size edit within one timestamp tick | Kept |
| With `~/.openclaw/workspace` and `~/reports` in `roots`, a spec session's reads and edits need no confirm (usage audit §6, item 2; P5d) | Not proven live in this step. Roots are unchanged A3 code, covered by the gate's root tests, and the template now shows the example | Eddie chooses his roots. It belongs to the exit test's config, which needs no code | Held for the exit test |

**Known gaps.**
- ~~An edit to a context file re-caches the whole session. The system block leads the cached prefix, so the next turn writes the session's entire context to the cache, and the prefix loses its thinking blocks. At the DM's measured 391 k tokens per call, one edit costs about $1 at Sonnet 5.5's cache-write price. If it bites, the files get their own later cache breakpoint, with the provider-safe caching work (theseus-ev1).~~ Closed by Item 29: the files are a block of their own, so an edit rewrites only it and what follows.
- The reads are synchronous file calls under the turn lock. A hung network mount would hang the turn, as it would any fs tool.
- Relative paths are refused at load.

**Open, held for Eddie.**
- Which files and roots go in his note.
- Whether a mid-turn edit should take effect within the turn.

### Item 3. Quiet notices (theseus-w4f; 2026-09-29, 02:28–02:38; b481a0b)

**What exists.**
- `[discord] notice_embeds`, false by default. With it off, a notified call posts no embed. Its line in the loop's tool message carries the notice and names the setting that made it one, for example `` ✅ `proc.run` cargo test · 🔔 notified (enforcement = notify) · 900 ms ``, or `([policy.tools] "proc.run" = notify)` for a per-tool rule. With it on, the embed behaves as before: a card, then its outcome.
- The reason needs no new plumbing: `tool.proposed` already carries the gate's decision and its setting.
- When a loop's calls overflow one message, the line that folds the oldest ones counts their notices: `-# … 4 earlier call(s) · 🔔 4 notified`.
- Unchanged: the `tool.notified` row, the `policy.notified` event, the web UI's notices, the CLI's `! notified:` line, and the narrative.

**How it is proven.**
- **Tests.** 180. They include:
  - with the setting off, a notified call rides on its tool line and posts no card;
  - with it on, the old card test still holds;
  - thirty notified `proc.run` calls in one loop render as one tool message, one create and 56 edits of 1,883 bytes, with no other message;
  - a core turn of thirty notified commands gives thirty `tool.notified` rows, each carrying the setting the renderer reads;
  - Eddie's `[discord]` shape loads unchanged, with the embeds off.
- **Live.** Discord was not exercised, because one bot token means one daemon, and Eddie's holds it. On a scratch daemon over a copy of his store, one notified `proc.run` wrote its `tool.notified` row with `setting = "enforcement = notify"`. Eddie's vault note loads under the new binary with only the known budget-units warning.
- **Review** (02:47). The gate reran at 180 tests.

**Divergence from Parts I and II.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| "A turn with 30 notified `proc.run` calls posts one tool message, edited in place, and no other message" (P5d, item 3) | One tool message per loop. Thirty calls in one loop post one message; calls spread over loops post one per loop, as before | The tool message is per loop by design (A3); the setting removes the per-call embeds | Kept |
| Every notice is visible in Discord | A loop that overflows one message shows its oldest calls only as a count on the fold line | Discord's 2,000-character limit | Kept; the ledger and the web UI keep every notice |

**Known gaps.** The parenthesis adds about 23 bytes a line (about 36 for a `[policy.tools]` rule), so a loop folds sooner. If it proves too long in use, the fallback is to name the setting once per message.

### Item 4. Attachments and images (theseus-9g2; 2026-09-29, 03:18–03:50; 7bcfd9a, 48fd2c8)

**Why.** Of Eddie's 332 DM messages in 30 days, 7 carried an attachment: 6 `text/plain` (Discord turns a long
paste into `message.txt`) and 1 PNG. The binding listed attachments by name and size and never read them, so
Theseus silently missed every long paste.

**What exists.**
- **The input.** `turn.submit` takes `attachments` (serde default), and an empty `input` is accepted when there
  are any. The user node keeps each file:
  - text, capped at `[tools].max_read_bytes` on a character boundary and marked when cut;
  - an image, as a reference to `<store dir>/blobs/<sha256>`, stored once and never in a WAL frame;
  - or the reason it was not read.
- **Rendering.** Each file is a block before the typed text, under a header that names it and its sender. An
  image is an image block for a model whose catalog entry has vision, and the "not shown, this model has no
  vision" line for any other. A message without attachments renders exactly as before.
- **`fs.read`** returns an image the same way, inside its `tool_result`, through a `Tool::run_with_image` hook.
  Its description gained one clause.
- **The estimate** leaves out base64 data and adds each image's tiles, so a 1 MB 1920×1080 PNG reserves 2,691
  tokens on Sonnet 5.5, not about 333,000.
- **The catalog.** `glm-5.3` and `glm-5.2` are text-only (`2026-09-29.1`); `glm-5.3-flash` keeps vision.
- **Discord.** `on_message` plans each file and spawns its download. The place's submit task awaits the downloads
  before `turn.submit`, so the gateway loop never waits and order is kept. A failed download is listed, and the
  turn runs.
- **The CLI.** `theseus ask --attach <file>`, repeatable.
- **Restore** carries `blobs/`.

**How it is proven.**
- **Tests.** 204, 24 of them new (9 with the text commit, 15 with the image commit). They cover:
  - a 5,001-character `message.txt` in the scripted provider's request, under its header;
  - a text cut at the cap, and a file listed with its reason;
  - a failed download that still runs its turn;
  - an image as one image block, stored once, with no bytes in the node, rendered byte for byte the same across
    turns and a cold cache;
  - the "no vision" line;
  - the pixel-based estimate;
  - `fs.read` of a PNG;
  - a 6 MB image refused with its reason;
  - the Discord planner and entry function, with bytes and no network;
  - the sniffer, the blobs, and restore.

  The frame budget holds at 17, and Eddie's vault config loads unchanged.
- **Live** (03:47–03:50, debug build on a scratch daemon over a copy of Eddie's store; about $0.02 in all):
  - Haiku answered a fact that only a 5,000-character attached note held.
  - Haiku read "TEAL HERON 77" from a headless-Chrome PNG, and the one blob is named for the PNG's SHA-256.
  - GLM-5.3 quoted the "no vision" line.
  - `glm-5.3-flash` read the PNG as an attachment and through `fs.read`, so z.ai takes images in user content
    and in `tool_result`s.
  - A 20 MB archive was listed as not read, with the reason.

  Discord was not exercised, because one bot token means one daemon, and Eddie's holds it.
- **The runs.** The first run (02:52–03:01) was marked external by the provenance plugin after it used web tools and a probe of z.ai, so exec and write were refused. It stopped without changing anything and left its findings, and run 2 built from them with no web tools.
- **Review** (04:22). The gate reran at 204 tests. Tabitha's own check on the release build, over a store copy: `glm-5.3-flash` answered from a 4,878-character attached note and read "AMBER FALCON 58" from a headless-Chrome screenshot, the one blob is named its SHA-256, and `glm-5.3` got the "no vision" line. Installed at 04:22.

**Divergence from Parts I and II.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| "The binding downloads each attachment up to `[tools].max_read_bytes`" (P5d, item 4) | Text up to `max_read_bytes` (256 KB); images up to 5 MiB | 256 KB would refuse most screenshots; 5 MiB is the provider's safe limit on every route | Kept |
| "text becomes user content … marked external" | Its own block under a header naming the file and its sender | Theseus has no provenance labels yet (§3.9) | The header is the mark until labels exist |
| `fs.read` returns "text, image, PDF as typed nodes" (§3.24) | Text and images; a PDF is still reported as binary | PDFs were not in this item's audit numbers | Held |
| One `vision` flag, true for every GLM row (the catalog as it stood) | `glm-5.3` and `glm-5.2` false | OpenClaw's table marks them text-only; `glm-5.3` answered a PNG with empty text | Kept; a `[catalog]` table can override |

**Known gaps.**
- Blobs are never collected, and redaction (§5.6) does not reach them. At Eddie's rate of one image a month,
  this costs nothing yet.
- An image the provider rejects fails every later request of its session (theseus-0s4). The sniffer checks the header, the
  size, and the sides, but not the pixels. `/new` in Discord, or a new message and then `recompile fresh`,
  recovers.
- A restore from a bare segment directory that is not named `wal` carries no blobs, and its images show as
  missing.
- The web UI shows an image's header line, not the image.
- The first turn of every session after install recompiles once as `tools_changed`, with its thinking stripped.
  For Eddie's DM that is about $1 of cache writes, once.
- A coalesced Discord batch from several authors labels its files "from discord", not per author.

**Open, held for Eddie.**
- Whether PDFs should be read (as text, or as document blocks).
- Whether the "no vision" line should instead route the turn to a vision profile.

### Items 1 and 2, follow-ups: the limit follows the config, and context files by persona (theseus-3pj, theseus-c48; 2026-09-29, 22:10–22:53; 430d291, 68cb127)

**Why.** Eddie accepted both on 2026-09-29 at 09:21, and the second again at 09:39.
- A dollar-era execution kept the limit it opened with, so a changed `spend_limit_usd` reached only new
  sessions, and never the long-lived Discord place. Since F1b, a changed note restarts the daemon onto it,
  so "I changed the limit and restarted" had to mean what it says.
- Item 2's `context_files` hung on profiles. Eddie wants a system level that every session gets, plus a
  persona's files, with the persona chosen by default until Jev chooses one from an ontology.

**What exists.**
- **The limit follows the config** (§3.13).
  - `Kernel::follow_spend_limit`, with the same rule in startup's step 2 for a config that may act at
    once. The core calls it on the vault's word, before the gate opens.
  - One frame, with a `budget.limit_changed` row for each execution.
  - A raise withdraws the waiting question and queues it as the next turn's result, and the continuation
    treats that as "the waiting call proceeds". A lower limit asks at the next reservation.
  - `Budget.pinned` marks a limit the opener named. It is absent from every record the product writes.
  - A narrative line per session, `confirm.resolved` (withdrawn) to the session's clients, and an
    Observatory summary of the row.
- **Context files in two levels** (§4.4).
  - `[context] files`, `[personas.<name>] files`, and `[context] default_persona`.
  - `[model] context_files` and the profile field are removed. Eddie's note used neither, checked by key
    name.
  - The headers name the level, and the manifest and the row record each file's persona.
  - Health's `context`, a `theseus health` line, and an Observatory line with a level column.
  - The template documents `[context]`, and a commented `[personas.theseus]`.

**How it is proven.** 369 tests in the gate, against 356 before:
- a raise across a restart lets a waiting session continue;
- a lowered limit makes the next turn ask;
- under an unconfirmed copy, startup writes no limit;
- a declined wait proceeds on a raise, and a pinned or ended execution keeps its limit;
- end to end, a limit raised in the vault: the old copy, the restart onto the changed note, and the vault's
  word, after which the waiting call runs, with the row before `config.confirmed`;
- system then persona files, each labeled; a persona with no files; an edited persona file recompiling
  once; an unknown `default_persona` refused, naming the known ones; and the template.

`kernel-sim` holds its budget invariants with limits that change across restarts, a third of the starts
served from an unconfirmed copy. Over 40 seeds × 300 steps: 497 changes, 327 rewrites, and 15 waits let
proceed. Four throwaway breaks were each caught, by the kernel tests and by the simulator.

Live, over a copy of Eddie's store and his real note through a shim `op`:
- a GLM turn's manifest listed the scratch system file, then the persona draft;
- a lower limit made the session's next turn ask;
- a raise restarted the daemon onto the note, and the session went on without a reset.

**Reviewed** (Tabitha, 2026-09-29, 23:18 to 23:25).
- The gate rerun passed: 369 tests, and all four bench phases within budget.
- On the release build, over a fresh copy of Eddie's store, with a file config:
  - Health said `context: 1 file at the system level · persona theseus (1 file)`.
  - A GLM turn's compilation listed the system file, then the persona draft, with the report's digests
    (`3ede0c7e…`, `7e3c050e…`). The answer knew Eddie from the persona draft. It did not end with the
    system file's "Ithaca" this time, which is the model's instruction-following: the file was in the
    block.
  - A limit lowered from $1 to $0.50 across a restart gave five `budget.limit_changed` rows, $1.0 → $0.5,
    for the open executions.
- Installed at 23:24.
- **Taken at review:**
  - The stale Discord question after a raise (a card whose buttons outlive its withdrawn question) goes
    into DD6's outbox brief: on reconnect, edit the cards of questions that closed while the binding was
    away.
  - Personas without a default stay a warning.
  - A carved task budget stays `pinned` (DD7).

**Divergence from the brief and the issues.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| The brief: rewrite the limits once, after the vault confirms the config at startup | Also in startup's step 2, for a config that may act at once | A file, or a vault read before serving, has no confirmation; step 2 already reads every execution | Keep |
| "Make an open execution's limit the config's" | Unless the opener named its own (`pinned`) | The kernel tests and the simulator open executions with limits of their own, and DD7's carved budgets will | Keep |
| A raise wakes a session parked at its limit | Also one whose question was declined; and a raise still too small withdraws the question, and the call asks again in the new figures | A declined wait is still parked on its budget; a stale question would show the old limit | Keep |
| The 58a header `# Context file: <path>` | `# Context file (system): <path>` and `# Context file (persona <name>): <path>` | Each file labeled by its level | Keep |

**Known gaps.**
- A Discord question posted before a restart keeps its buttons after a raise withdraws it (to DD6). _(Closed
  2026-09-30 by theseus-q4v for every card posted since. A card posted before then has no post to settle it.)_
- A lower limit makes theseus-kks likelier: a limit below one call's reservation asks again after every reset.
- ~~A persona switch by Jev will rewrite the whole cached prefix, until the files get their own breakpoint
  (theseus-ev1).~~ Closed by Item 29: the files are a block of their own, after the header.

### Item 5. `http.fetch` and `web.search` (theseus-yd6; 2026-09-30, 01:10–01:46 and 02:14–02:36; acb16f4, 28ac9d3)

**Why.** In 30 days the DM made 59 `web_fetch`, 36 `web_search`, and 29 `curl` calls (the usage audit,
section 6). Eddie chose the Brave Search API on 2026-09-29.

**What exists.**
- **Async tools.**
  - `Backend::Async` and `Tool::run_async`: a call is `tokio::spawn` under the in-process deadline (120 s),
    and holds no core while it waits. A panic is the call's error, not the turn's.
  - It goes through the one in-process path: `tool.started` (`backend: async`), the bound broker, the
    result node, and the completion.
  - Both tools are class `Read`, so F3's `run_calls` polls the fetches of one response together.
- **`http.fetch`** (`theseus-core/src/web/`).
  - GET only. The client follows no redirect itself; the tool follows up to 5, judging each hop first.
  - `[tools.web] timeout_secs` (30) over the whole call, and `max_bytes` (2 MiB) over the body.
  - HTML becomes text: headings, paragraphs, list items, links as `text (url)`, and `pre` as it is.
    Script, style, noscript, svg, and the like are dropped, and entities are decoded. The converter is
    hand-written, about 510 lines, and reads tags one at a time, so broken or cut markup still gives
    its text.
  - A page of 16 KiB or more converts on the pool. The converter runs at about 210 MB/s, and a hop to
    the pool costs 24 µs, so a smaller page converts on the call's own task in under 80 µs.
  - Text, JSON, and XML come back as they are. Any other type is named with its size, and is not
    downloaded when its size is given.
  - Any response is a result, a 404 included. Only a call that got no response fails.
- **Private addresses** wait at the gate, or are refused at connect (§3.9). Both clients use
  `no_proxy()`. A test-only resolver (`net::Dns`, with a host map and addresses taken as public) lets
  tests reach 127.0.0.1; no config key reaches it.
- **`web.search`.**
  - The Brave Search API gives rank, title, URL, and snippet, with the markup stripped.
  - Its key is `brave_api_key`, granted with `grant_tool` and read through `ToolCtx::secret`, so each
    search is at least `notify`, and health counts its uses.
  - No key, a key held stricter than the call ran at, a 401, or a 429 is a result the model reads. Only
    Brave's error code comes through, never its free text.
- **External text.** A result node's `external { url }` has a serde default, so old records read
  unchanged. An error result is never marked, since its text is Theseus's own. A redirect whose
  `Location` is not a URL names the parse error, never the header (`28ac9d3`).
- **Surfaces.** The URL or the quoted query shows on the Discord tool line, the web UI, and the notices.
- **Config.** `[tools.web]` has defaults, so a note without it loads unchanged. The template has the
  Brave `[secrets]` line, the table, and B1's comment under `[broker]`.

**How it is proven.**
- **Tests.** 402 in the gate, 20 of them new: the converter; every kind of non-public address, in every
  spelling; the fetches against a local server (content, redirects, the caps, and the gate's five
  URLs); Brave's fixture and its refusals; two fetches of one response at once; a declined loopback
  fetch that never connects; and a search held to its key's posture.
- **The step's live check** (02:23–02:27): the Mutex page's `lock`, the three right URLs for `ignore`'s
  `WalkParallel`, `http://127.0.0.1:7433/` waiting and never starting once declined, and a name that
  `/etc/hosts` maps to 127.0.0.1 refused at connect.
- **The runs.** Run 1 (01:10–01:46) wrote the code. The provenance plugin then tainted its session: the
  Bash heredoc that wrote its report held the word "links" followed by a space, and a placeholder URL,
  and the plugin's exec rule for the `links` browser matched them (openclaw-provenance-fqu). Every later
  exec, edit, and write was refused. Run 2 continued from the tree, and wrote every file with the Write
  and Edit tools.

**Reviewed** (Tabitha, 2026-09-30, 02:40 to 02:47).
- The gate rerun passed: 402 tests, and all four bench phases within budget (cold start p95 46.5 ms).
- On the release build, over a fresh copy of Eddie's store, with `proc.run` and the write tools pinned
  to `approve`:
  - one GLM response made two fetches, of the `HashMap` and `Vec` pages (196 KB and 953 KB). They were
    dispatched 7 ms apart and succeeded at 262 and 311 ms, so they ran together. Both answers were
    right: `entry` returns the `Entry` enum, and `with_capacity` makes an empty vector with at least that
    capacity;
  - a search found tokio's `JoinSet` page on docs.rs, from 5 results, and health said `used once`;
  - `http://169.254.169.254/latest/meta-data/` waited ("169.254.169.254 is a link-local address, and a
    private address waits for approval"). Declined, it was never dispatched, and the model tried nothing
    else;
  - the Brave key's value was in none of the copy's state files, the log, or the CLI's output. The
    control matched.
- Eddie's unchanged note loads under the new binary.
- Installed at 02:46.
- **Taken at review:**
  - **Web text can now steer a session that acts at `notify`.** A page's text reaches the model, and in
    Eddie's config `proc.run` and the write tools run at `notify`. Until provenance labels (§3.9) and
    Jev exist, a deterministic rule closes the gap: once a session has read external text, a call that
    acts waits for approval, until the operator clears it. Filed as theseus-9bp, and added to the chain
    before Eddie's end-to-end test.
  - The runtime's "the full output is stored" is wrong for an in-process result, which stores nothing.
    It predates DD5. Filed as theseus-46v.

**Divergence from Parts I and II.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| "native toollets" (P5d, item 5) | Async tools on the runtime, not toollets on a core | They wait on the network: async for waiting, the pool for compute (F3) | Keep. §3.23's "a toollet computes on a core" still holds for toollets |
| `http.*` fetch, post (§3.23) | `fetch` only | The audit's calls were GETs | `post` when a need shows |
| External text is marked (§5.2) | `external { url }` on the result node | There are no provenance labels yet (§3.9). A field reads old records unchanged | The field is the mark until labels exist. _(Since 19a, Item 61, the result's label is untrusted, with the URL as its source; the field stays the hold's input until 20a. 20a was dropped and the labels removed, Items 74 and 76, so the field is the mark.)_ |
| "a loopback or private address waits for approval" (P5d) | It waits when the URL names it, and is refused at connect when a name resolves to it | The gate sees the URL, and only the resolver sees the answer | Keep |
| "text, image, PDF" (§3.24) | A PDF is named with its size, not read | Decision 10 | Held |

**Known gaps.**
- An approval can reach a private name only if it is `localhost`. A name like `printer.lan` looks public
  to the gate, and its private answer is refused at connect. Asking by address breaks TLS and virtual
  hosts. The fixes are to resolve at the gate and pin the answer, or an operator's list of private names.
- A page's text is cut to `[tools] result_max_chars` (30,000) from its head, with no offset to read
  further. §3.16's node reference with a range is the likely home.
- Pages are read as UTF-8 only, with no gzip or brotli.
- Not exercised live: the Discord and web UI tool lines, since Discord and the web UI stay off in a
  scratch daemon. The Discord summary is unit-tested.

### Item 6. Durable delivery (theseus-q4v; 2026-09-30, 02:50–04:19; f028083, 9dd4cef, 765111d)

**Why.** 64 DM turns ran past 5 minutes. A reply that ended while Discord was away was lost, since the
binding posted with a plain HTTP call from a live watcher of its session. For the same reason every
continuation waited at startup, up to 20 s, for the binding to watch its sessions (A3). That wait was on
FAST's start path.

**What exists.**
- **Posts** (`theseus-kernel/src/outbox.rs`, `theseus-core/src/outbox.rs`). The core writes an outbox
  post when something must reach Discord, whether or not the binding is there:
  - a turn's reply: its loops' text by node, and its footer;
  - a confirm card, with its question;
  - a card's settle at every close: an answer, a supersede, or a raise's withdrawal;
  - a failed turn, a job's refused answer, and a restart onto a changed config note;
  - the binding's own notes.

  A post is a kernel action of the record kind `OUTBOX`: planned and authorized, dispatched, and settled
  with its message ids. It is no execution's work.
- **Frames.** A reply rides in the frame that ends its turn, and a card in its question's, so a plain turn
  still writes 8 frames. Delivering a post writes 2 more, off the turn path.
- **The lanes** (`theseus-discord/src/courier.rs`). One per place, and one for the operator, each the only
  writer of its messages.
  - A lane sends its posts in order, then the live progress, which keeps only each message's latest state
    and is dropped while Discord is away.
  - A card is routed when it is delivered (2b's rule), and its completion keeps what its settle edits.
  - The lanes need only REST, and start before Discord answers anything.
- **Exactly once.** A create carries a nonce from its message's key, with `enforce_nonce`. A create that
  the nonce returns with an earlier send's content is edited to the post's state. Past a 120 s window, a
  create that may have landed says it may be a copy.
- **S1's stale card.** Every close writes its card's settle. A level-triggered pass, on connect and every
  heartbeat, catches the closes that no event said.
- **No startup wait.** `BindingBoard::expect`, `started`, and `wait` are gone, with the driver's wait. The
  driver's start is a startup phase.
- **Health and the Observatory** show each binding's outbox: pending, sent, refused, the oldest pending
  post's age, and the last error.
- **For tests and scratch daemons**, `[discord] rest_proxy` and `gateway_proxy` point a binding at local
  stand-ins. `theseus-sim fake-discord` is one for REST: it honors a nonce as Discord does, can be down,
  hang creates, or fail, and never records a header.

**How it is proven.**
- The gate at 765111d: 420 tests. Among them are 6 binding scenarios against the fake: away during a turn;
  three replies in order; a crash between send and settle; a card's settle after its create, to its id;
  live edits coalescing; and 2b's DM route.
- Three real-daemon tests: a `kill -9` between send and settle leaves one message; a continuation at
  restart, with Discord away, posts once Discord is back; and the REST goes to the fake, with no header
  kept.
- The lifecycle bench now runs the binding, with its token resolving after 1000 ms. The driver starts at
  p50 41 ms, before the token.
- The step's live check (04:02–04:18), on a copy of Eddie's store: a reply posted once after the fake came
  back, and one message after a `kill -9` (tries `hung, hung, deduped`).

**Reviewed** (Tabitha, 2026-09-30, 04:40 to 04:47).
- The gate rerun passed: 420 tests, and every bench phase within budget (cold start p95 38.3 ms; the driver
  started at p50 41.1 ms, before the binding's token resolved).
- On the release build, over a fresh copy of Eddie's store, with the binding on a fake REST (port 9472) and
  a gateway that never connected, and a scratch bindings file that binds a DM with a user who does not
  exist:
  - the bind notice went out through the outbox;
  - **away during a turn:** with the fake down, a GLM turn ended. Health said `1 pending … last error:
    parsing or receiving the response failed`, and the fake had no message. With the fake back at
    04:44:41, the reply was posted once, footer included, at 04:44:55;
  - **a kill between send and settle:** with the fake hanging creates, the reply's create reached it twice
    with one nonce (the stream's and the post's). I killed the daemon with `kill -9` at 04:45:39, brought
    the fake back, and restarted. The retry at 04:45:49 was deduped (`hung, hung, deduped`), and the
    message was edited once to its final form. The channel held the bind notice and the two replies, each
    once, and nothing was pending;
  - **nothing reached Discord.** The daemon's connections went to 127.0.0.1 (24), api.github.com (2, the
    startup token check), and api.z.ai (2, the turns). No Discord host name was in the log.
- Eddie's unchanged note loads under the new binary.
- Installed at 04:46.
- **Taken at review:**
  - The labeled possible copy after a long outage stays, rather than a lookup of recent messages first,
    which would need another permission and a call per retry.
  - A post for a place no longer in the bindings file stays pending forever. Filed as theseus-l3m: settle
    it as refused, "not bound here any more", when the binding starts without that place.

**Divergence from the brief and the issue.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| "Every post and edit the binding makes becomes an outbox action" (the issue's title) | Posts that must be seen are durable; live progress (streamed text, tool messages, typing, notice embeds) stays best-effort | Only a live message's latest state matters, and a replay is noise | Keep |
| The nonce "derived from the outbox action's id" | Derived from the message's key, which holds the post's or question's id, or the turn's for a reply | The stream's create of a part and the reply's create of it share a nonce, so a stream create whose answer was lost cannot become a copy | Keep |
| "Use the kernel's actions" | Kernel actions of their own record kind, `OUTBOX`, with their own transitions; the `Action` type, states, `Completion`, and ledger rows are the kernel's | An execution's machinery would queue a post's result on its execution, cancel a reply when its execution ends, and let the reconciler mark a post unknown | Keep |
| The card's route decided when the live renderer saw the question | Decided when the card is delivered, with a fresh check of who can view the channel | Routing needs REST, which is the lane's | Keep |
| — | A card whose question closed before it could be posted is posted, then settled at once | Discord keeps a record of what was asked while it was away | Keep |
| — | A turn that parks on a confirm: its card is written with the question, and its reply at the turn's end | A card must exist whenever its question does. After an outage the card is posted above the text that led to it | Revisit if it reads badly |
| — | The lifecycle bench runs the Discord binding | To show no driver wait, the bench's daemon needs a binding to wait for | Keep |

**Known gaps.**
- The index reads every outbox record once per process, at first use: O(posts ever). Compaction, or a
  settled-below watermark, when it matters.
- A lane's maps (key to message, sent contents, sealed keys) grow for the process's life.
- A card posted before DD6 has no post: only a press settles it.
- The gateway side cannot be faked, so no test sends a Discord message in; turns enter through the
  protocol.
- `kernel-sim` does not yet inject crashes around the outbox's transitions.
- A task's report (DD7) will be a post of its own kind, through `Outbox::post`. _(Done in item 7.)_

### Item 7. Task sessions (theseus-qn2, with theseus-xeo; 2026-09-30, 04:49–06:03; 795ed91, 625dde3, 5b44ff5, 9610b2e, 3b0b20d)

**Why.** 43% of all tool calls in the audit's 30 days ran in delegated sessions. Eddie's daily pattern is
"go do this long thing and tell me when it's done", while he keeps talking.

**What exists.**
- **`task.create { brief, budget_usd? }`** (`theseus-core/src/task.rs`), a harness tool
  (`Backend::Harness`, new): it needs the turn's kernel and store, so the runtime runs it in the calling
  turn's task. It opens the child in one kernel frame (`Kernel::open_task`, under `lock_two(parent,
  child)`) and returns at once. The child's ids come from the call's correlation id (`act_X` opens `exe_X`
  in `ses_X`), so a call run again after a crash finds its task instead of opening a second.
- **What the child inherits** (§3.2a's note): authority, persona and context files, postures, model, and
  where its approvals go. Its cards, its budget question, and its notices name it (`task a1b2c3:`).
- **The carve** (`theseus-kernel/src/tasks.rs`). `budget_usd`, or a quarter of what the parent has left,
  capped at all of it, reserved in the parent as `task:<child>`, with the child's limit pinned. Each
  settle of the child's cost moves it into the parent's spend and shrinks the carve, under both locks
  (`lock_family`). The child's end releases the carve, but for what it still has in flight. At its own
  limit the child asks, as any session does.
- **Depth one.** A task's `task.create` is refused, and the kernel refuses too (`TaskDepth`).
- **The report.** A task turn that would wait on input ends the task, and its last message is its report.
  The frame that ends it carries the report's outbox post (`report:<task>`, one message ever) and puts the
  child on the parent's `reports`. The parent's next turn writes one node per report into its own session
  (`origin: harness`, author `task:<short>`), in one frame that clears the list. Nothing starts a parent
  turn. A failed task reports `failed`, and a cancel reports `stopped`, in the cancel's own frame.
- **Seeing and stopping.** `task.list` and `task.cancel` in the protocol, `theseus tasks` and
  `theseus cancel <task>`, Discord's `/tasks` and `/cancel task:<id>`, and the web UI's session tree with
  each task's state and spend. A cancel terminates the task's jobs and walks each action's cancel, as
  `execution.cancel` does. A second cancel writes nothing.
- **Crash safety.** A task is an execution and a session, so startup requeues it, and the driver continues
  it. A job that outlives a `kill -9` is found still running, and its result finishes the task.
- **theseus-xeo.** `Store::update_session` holds a per-session lock from the read to the indexed write,
  for one frame. A turn writes only its own fields, and takes a pending recompile when it starts. So a
  `session.recompile` asked during a turn is kept for the next.
- **`lock_two`'s first callers:** `open_task`, and `lock_family` for every transition of a task, which
  takes the task and its parent together.
- **A kernel fix found on the way.** A late completion after a cancel that held its reservation as
  unknown books its real cost and releases the hold. Before, a cancelled task would have kept its carve.

**How it is proven.**
- The gate at 3b0b20d: 445 tests, 25 of them new:
  - 12 in the kernel: the carve, the cap, one task per call, depth one, the spend carried, the end, the
    cancel, a late completion after a cancel, a crash, and three `lock_two` races that lose an update with
    one lock (a throwaway probe, reverted);
  - 5 in the core, through the real driver;
  - 2 for theseus-xeo, the filed race and the lock;
  - 4 in Discord, and 2 against the real daemon (a `kill -9` while a task's job runs, and a cancel that
    kills a real job).
- The step's live check (05:53–06:00), on a copy of Eddie's store:
  - a GLM turn started a task that ran `scripts/gate.sh`, and a second question was answered meanwhile;
  - the report posted once, and the parent quoted it;
  - a `kill -9` during a second task's gate run: the task finished after the restart, in 3 turns, and
    reported once.
  - The first gate run inside a task found that the broker test's own approval, from inside a job, is
    refused by J1's guard, correctly. 3b0b20d makes that test skip its operator's part inside a job, as
    `job_approval.rs` does.
- **The run.** A clean WSL shutdown at 06:03:36 (a WSL update) ended the run while it wrote the report's
  last four sections. Its commits were all pushed, and Tabitha wrote those sections at review.

**Reviewed** (Tabitha, 2026-09-30, 08:28 to 08:40).
- **The first gate rerun failed one test**, `theseus-tools git::tests::diff_and_log_against_a_real_repository`,
  in 60 s: "gpg failed to sign the data". Its fixture builds a repository with the git CLI and inherited
  the operator's global `commit.gpgsign = true`, and the reboot had left the gpg-agent locked. This is not
  DD7's fault, and it predates DD7. Fixed at review in 94d184d: the fixture sets
  `GIT_CONFIG_GLOBAL=/dev/null` and `GIT_CONFIG_NOSYSTEM=1`, and passes in 49 ms with the agent still
  locked. The gate then passed: 445 tests, and every bench phase within budget.
- On the release build of 94d184d, over a fresh copy of Eddie's store, with the binding on a fake REST
  (port 9473) and a gateway that never connected:
  - **A task that reports.** A GLM turn in the bound session called `task.create` with a $0.50 budget and
    a brief to run `git log --oneline -3`, and returned in 22.5 s. The task ran one turn, spent $0.0007 of
    its $0.50, and completed. Its report was one message at the fake, `📋 Task 4bed87 finished`, with the
    three subjects;
  - **the parent's next turn quoted it** exactly, in one loop with no tool call, and the parent's session
    held exactly one report node, authored `task:4bed87`;
  - **a cancel.** A second task ran `sleep 120` in a job wrapper. `theseus cancel 164775` stopped it: the
    wrapper and the `sleep` were gone, and one message, `⏹️ Task 164775 stopped … cancelled by sock#13`,
    reached the fake. A second cancel asked 0 actions to stop and posted nothing;
  - **the parent's spend holds its tasks'.** The parent's own three turns cost $0.001444, and the tasks
    $0.000995, which makes $0.002439. The parent's execution said $0.002442 spent, and $0 reserved once
    both tasks had ended;
  - nothing reached Discord: the connections went to 127.0.0.1, api.z.ai, and api.github.com.
- Eddie's unchanged note loads under the new binary.
- Installed at 08:38.
- **Taken at review:**
  - A cancel's author on Discord reads as the connection's label (`sock#13`). DD8, which touches
    `/cancel` for wakes, makes it name the surface or the person.
  - Whether a finished task should start a parent turn is put to Eddie as a later `wake_parent` option
    (the report's section 13).

**Divergence from the brief and the issue.** The report's section 14 has the table:
- `task.create` is a harness tool, not a toollet;
- the report reaches the parent at its next turn;
- the default carve is a quarter;
- `/cancel` now takes a required task;
- the kernel fix for a late completion after a cancel;
- the broker test's skip inside a job;
- and, at review, the hermetic git fixture.

**Known gaps.**
- Discord's `/tasks` and `/cancel` were not run live, since the gateway is not faked. The binding's tests
  cover them.
- A task's live progress shows only in the web UI's session, not in the place.
- `/stop` does not stop a session's tasks, which `/cancel <task>` does.
- §3.2a's arrangement, `autonomous: true`, sub-tasks, and §3.5's task graph are not built.

### Item 8. `wake.at`: one-shot wakes into the current session (theseus-cff; 2026-09-30, 08:41–09:50; 84ab96d, 07c0bb8, ba49df2, 4c6b72c)

**Why.** 6 of the DM's 8 schedule adds in the audit's 30 days were one-shot wakes into the current
conversation: "check the build in 10 minutes".

**What exists.**
- **`wake.at { at | after, note }`** (`theseus-core/src/wake.rs`), a harness tool. `after` is a duration
  (`90s`, `10m`, `2h`, `1h30m`, `1d`), and `at` an RFC 3339 time with its offset, 1 s to 30 days ahead.
  It writes the wake onto the session's execution in one kernel frame (`Kernel::set_wake`), and returns at
  once with the wake's id, its time, and the line its turn will read. It is refused in a task, and at 5
  pending, with the five listed.
- **Pending wakes** (`theseus-kernel/src/wakes.rs`): a list on the execution beside its one `wake`
  (§3.15). A due wake fires when its execution is free; the driver's tick queues it within half a second,
  and the reconciler is the backstop. A busy execution is queued again by the frame that ends its turn. A
  due `Wake::DueAt` now sets `resume_pending`, so the driver takes it: before, nothing did.
- **The wake's turn.** Its catch-up takes the due wakes in one frame, each a node (`wake:<short>`,
  `⏰ wake (set 13:05): <note>`), with a `wake.fired` row (`late_ms`, `while_down`). The reply's outbox
  post carries the wake's line, which the binding shows above the reply.
- **Late, and never twice.** Startup writes nothing for a due wake: the driver's first tick queues it,
  once the vault has confirmed the config. A wake more than 5 s late says so, when it was due, and why.
- **Seeing and stopping.** `wake.list`, `wake.cancel`, and health's `wakes`; `theseus wakes` and
  `theseus cancel <id>`; Discord's `/wakes`, and `/cancel id:` for a task or a wake; and the Observatory's
  Wakes section, with a cancel. A cancel's author names the surface or the person (DD7's review).

**How it is proven.**
- The gate at 4c6b72c: 473 tests, 28 of them new: 9 in the kernel, under the virtual clock; 6 in the core
  through the real driver, and 8 unit tests of the tool's parsing and text; 2 against the real daemon
  with the fake Discord REST (a wake's reply posted once, and a `kill -9` with 9 s down, then a late wake,
  once); 2 in Discord; and 1 in the CLI. The frame budget test holds 8.
- The step's live check (09:26–09:45), on a copy of Eddie's store:
  - "Remind me in one minute to check the build" set a 60 s wake, and the daemon was restarted 11 s later.
    The wake's turn started 60.0 s after the wake was set, ran a real `cargo check`, and its reply posted
    once under its wake line;
  - a 30 s wake with the daemon down for 61 s ran after startup, `42 s late: the daemon was not running
    then`, once. That start queued it before serving, which ba49df2 fixed. On ba49df2's build, the same
    check wrote nothing before serving;
  - `theseus cancel` cleared a pending wake, `by the CLI`, and a task's cancel read `cancelled by the CLI`.
- The step found and fixed three things live: startup's write before serving, a doubled full stop in the
  tool's result, and a due time printed without its seconds.

**Reviewed** (Tabitha, 2026-09-30, 10:00 to 10:07).
- The gate rerun passed: 473 tests, and every bench phase within budget (cold start p95 37.9 ms). The
  step's own last gate had passed cold start at 56.7 ms against 57, on a busy machine.
- On the release build of 4c6b72c, over a fresh copy of Eddie's store, with the binding on a fake REST
  (port 9475):
  - a GLM turn in the bound session, "Remind me in 20 seconds to stretch", set a wake due 10:05:08;
  - **down across the due time.** I stopped the daemon at 10:05:00 and restarted it at 10:05:53. The
    wake's turn ran at once, and its node read `⏰ wake (set 10:04, due 10:05:08, 46 s late: the daemon was
    not running then): …`. Its `wake.fired` row had `late_ms` 45877 and `while_down` true, and its reply
    reached the fake once, under the wake line;
  - **startup wrote nothing for it before serving.** The restart's frames were the startup steps, then
    `server.started+server.serving`, then `driver.started`, and only then the driver's
    `execution+execution.queued`;
  - **listing and cancelling.** A two-hour wake showed in `theseus wakes`. `theseus cancel beeee4`
    cleared it, the list was empty, and the `wake.cancelled` row said `the CLI`;
  - nothing reached Discord: the connections went to 127.0.0.1, api.z.ai, and api.github.com.
- Eddie's unchanged note loads under the new binary.
- Installed at 10:06.
- **Taken at review:**
  - `/cancel`'s option stays `id`, since it names a task or a wake, and a wake keeps waiting while an
    approval or the budget question is open, since only the operator answers those.
  - A task still cannot set a wake, and a session may hold 5.
  - A clean shutdown just after a turn's end can stop the daemon between a reply's create and its settle.
    The next start resends it, deduped by its nonce. Filed as theseus-pfv: the lanes settle what they
    already sent, within the shutdown budget.

**Divergence from the brief and the issue.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| "sets the kernel's existing `Wake::DueAt` on the current session" (issue, P5d) | A list of pending wakes on the execution, beside its one `wake`; the due scan that fired `Wake::DueAt` fires them too | One field cannot hold input and a due time, and a session parked on its own job must still be woken | Keep |
| The reconciler runs due wakes every `heartbeat_secs` (the audit) | The driver's 500 ms tick fires a due wake; the reconciler is the backstop | A 60 s heartbeat would run "after 2 s" up to a minute late | Keep |
| — | A due `Wake::DueAt` now sets `resume_pending` | Before, the reconciler queued it and nothing took the turn | Keep |
| — | Startup's reconcile no longer fires due wakes (ba49df2) | It wrote two frames before serving, which FAST forbids | Keep |
| "`/cancel` clears a session's pending wakes … or `/cancel wake <id>`" | `/cancel id:<id>` names a task or a wake, and `/wakes` lists them | One verb for "stop that one" | Keep |
| — | A wake's reply goes to where it was set, after `/new` | The reminder should reach the place that asked for it | Keep |
| — | A task cannot set a wake | A task ends when it has nothing to wait on | Keep; a later option |
| "a small number of pending wakes" | 5 | The DM set 6 in 30 days | Keep |

**Known gaps.**
- Discord's `/wakes` and `/cancel` were not run live, since the gateway is not faked. The binding's tests
  cover them.
- `at` needs an RFC 3339 offset, and times print in the daemon's zone.
- The kernel-sim's random operations do not include wakes yet.
- A wake is not moved when its session is `/new`ed away. It runs in its own session, and its reply reaches
  the place.
- An older binary reads a store with pending wakes but ignores them, and would drop them the next time it
  wrote the execution. F4's versioned readers close this.

### Item 9. W1: a task can wake its parent, and `/stop` only halts (theseus-lji; 2026-09-30, 10:09–11:02; 2975183, 426e2b7)

**Why.** Eddie, 2026-09-30, answering DD7's two questions. First, a finished task should be able to
start the conversation's next turn, as an opt-in: "Yes!" Second, `/stop` should only halt the work and
keep the conversation, while `/new` alone starts fresh: "Yes!" His rule for controls is one command, one
effect.

**What exists.**
- **`task.create { brief, budget_usd?, wake_parent? }`.** With `wake_parent`, a task that finishes or
  fails asks for its parent's next turn. The frame that ends it puts it on the parent's `report_wakes`,
  with a `task.report_wake` row, and queues a free parent for the driver (`why: report`), as a due wake
  does. A busy parent keeps the ask until its turn ends. The turn's catch-up reads every report and
  clears both lists, and its reply posts where the parent posts, under `📋 task a1b2c3 reported`. A
  cancelled task wakes nothing. The start's notice says the task will wake the conversation, and
  `task.list` and `theseus tasks` show the option.
- **A soft stop** (`theseus-kernel/src/stops.rs`, `Kernel::stop_execution`), §3.15's Stopping.
- **Surfaces.** `execution.stop` (an acting method), `theseus stop <session>`, and Discord's `/stop`,
  which no longer rebinds. The bind notice, the help text, and the commands' descriptions name each
  control with its one effect. The place freezes the stopped turn's stream, and a declined card settles
  as `stopped`. `/new` is unchanged.

**How it is proven.**
- The gate at 426e2b7: 496 tests, 23 of them new: 11 in the kernel; 7 in the core (6 through the real
  driver, and the notice's unit test); 2 in Discord (a place's `/stop` and `/new` over a real core); 2
  against the real daemon with the fake Discord REST (a report's turn, and a stop that kills a real
  job); and 1 in the CLI. The frame budget test holds 8.
- The step's live check (10:54–10:58), on a copy of Eddie's store with the binding on a fake REST: a task
  with `wake_parent` ran `git log`, and the parent's turn started by itself 14 ms after the task's end
  frame; a task without it reported, and no turn followed in 45 s; `theseus stop` during a `sleep 45` job
  killed it, and the next message continued the same execution.
- Discord's `/stop` itself was not run live, since the gateway is not faked. The binding's test drives it.

**Reviewed** (Tabitha, 2026-09-30, 11:30 to 11:37).
- The gate rerun passed: 496 tests, and every bench phase within budget (cold start p95 42.2 ms).
- On the release build of 426e2b7, over a fresh copy of Eddie's store, with the binding on a fake REST
  (port 9477):
  - the bind notice names `/stop`'s new meaning: "halts what I am doing and keeps the conversation";
  - **a task that wakes its parent.** A GLM turn started task `8ec704`, with `wake_parent` and $0.30, to
    run `hostname`. Its report reached the fake. Its end wrote `task.report_wake` (`queued: true`) and the
    parent's `execution.queued` (`why: report`). The parent's turn then started by itself, read the
    report (`task.reports_read`, `woke`), and posted "The hostname is `zeroradeons`." under
    `-# 📋 task 8ec704 reported`;
  - **a stop.** A turn ran `sleep 40` in a job. `theseus stop` answered "1 action(s) told to stop … the
    conversation goes on", and the `sleep` was gone. The ledger had `execution.stopped` by the CLI, the
    turn's end with `stop_reason: stopped`, and `execution.waiting`, `why: stopped`. The fake got only the
    call's tool line, and no reply;
  - **the conversation went on.** The next question ran in the same session and the same execution as the
    first turn, and GLM answered from its history that the sleep "was stopped by the CLI";
  - nothing reached Discord: the connections went to 127.0.0.1, api.z.ai, and api.github.com.
- Eddie's unchanged note loads under the new binary.
- Installed at 11:35.
- **Taken at review:**
  - A stop keeps the session's pending wakes, as it keeps its tasks: one command, one effect.
  - The web UI's stop control is filed (theseus-nkt), and so is aborting a stopped turn's in-flight model
    stream (theseus-yey).
  - A call that a stop killed shows `❌ … cancelled` on its Discord tool line, which reads as a failure,
    though it was asked for. It goes into fix batch 2 on the roadmap: `⏹️ stopped`.

**Divergence from the brief and the issue.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| "`/stop` … cancels the session's current work, as today" | A new transition, `Kernel::stop_execution`, not `execution.cancel` | A cancel is terminal, so a session it ends cannot take a turn | Keep |
| "the running turn, its jobs, and the queued messages" | Also declines planned calls, approvals, and the budget question | An approval answered after a stop would resume the stopped work | Keep |
| — | The running turn's model call runs to its end; its answer is kept, but not acted on or posted | No abort exists for a provider stream today | Keep; theseus-yey |
| — (DD8: a cancel, `/stop` then, dropped the wakes) | A stop keeps the session's wakes | One command, one effect | Keep |
| "the frame that ends the task also queues a continuation turn" | For a free parent in that frame; for a busy one, in the frame that frees it | A turn cannot start over another, or over an open question | Keep |
| — | A failed task wakes its parent too | A failure is a result a chain should review | Keep |
| — | `theseus stop <session>` in the CLI | So the stop runs live, through the method Discord's `/stop` calls | Keep |
| — | A job a stop or a cancel killed ends the turn's in-turn wait at once | `job_settled` did not count `cancelled` before | Keep |

**Known gaps.**
- Discord's `/stop` was not run live, since the gateway is not faked.
- ~~A stopped turn's in-flight model call runs to its end, and its cost is paid (theseus-yey).~~ Built in Item 54: a stop cuts the stream.
- An older binary would drop `stopped`, `report_wakes`, and `wake_parent` on a rewrite. F4a closes this.
- The kernel-sim's random operations include neither stops nor report wakes yet.

### Item 10. T1: after a session reads external text, a call that acts waits (theseus-9bp; 2026-09-30, 11:39–12:27; 822f127, 41e8fa5)

**Why.** This came from DD5's review. Since DD5, a page's text reaches the model, and in Eddie's config
`proc.run` and the writers run at `notify`. So an injection in a page could steer the model into running
a command, with only a notice after the fact. OpenClaw's provenance plugin closes this path for OpenClaw
sessions. Theseus has no provenance labels yet, and Jev's `security.v1` is M5. T1 is the interim,
deterministic floor, and it landed before Eddie's end-to-end test.

**What exists** (§3.9, External text).
- **The hold** (`theseus-core/src/external.rs`): `SessionRecord.external` (since when, the tool, the
  URL, the node, and, when it came from another session, which one and how), with one
  `session.external_read` row. It is written in the frame that brings the text in, under the record's
  lock (`Store::with_session`, holding it until the frame is indexed; the lock order is always the
  session, then the execution). There are three such frames: a result's completion, a task's
  `open_task` (for a task that a holding session starts), and the parent's `take_reports` (for a report
  from a holding task). A turn never writes it (`take_turns_fields`).
- **The gate** (`external::gate`, after the broker's posture): a call whose class is not `Read` waits,
  with the reason, the allow list's calls included. A `Read` call keeps its posture and reads no
  record. A record that cannot be read fails closed.
- **Trusting it again**: `policy.trust` and `action.confirm { trust }`, judged by `judge_act`
  (`Act::Trust`), and ledgered as `session.trusted`.
- **Surfaces**: `theseus policy trust <session>`, `theseus confirm --trust`, and a hint on a waiting
  call's lines; health's `external_text` and the CLI's `external text:` line; Discord's third button,
  "Approve + trust session", and its settle; the web UI card's third button and marker, and the
  Observatory's External text section.
- **Config**: `[policy] external_text = "ask" | "notify"`, `ask` by default, in the template.

**How it is proven.**
- The gate at 41e8fa5 ran 509 tests, 13 of them new: 9 through the whole core, 2 unit tests of the rule,
  a config test, and a CLI test. The core tests cover the same turn and the next, trust, a read keeping
  its posture, a job's process refused, a restart, a clean session unchanged, approve with trust, a
  task, a report's turn, and a wake's turn. The frame budget test holds 8.
- The step's live check (12:19–12:22) ran on a copy of Eddie's store with his note, with Discord and the
  web UI off:
  - a GLM turn fetched the `Option` page (200, 248,029 bytes), and `proc.run echo hi` waited, with the
    reason word for word;
  - the hold rode in the result's frame (`…node+session.external_read+session`), and the run was declined;
  - `theseus policy trust` cleared it (`session.trusted`, by the CLI), and the next `proc.run echo hi` in
    that session was a notice and ran;
  - beyond the brief, a hold survived a restart, and `theseus confirm --trust` approved a waiting run and
    trusted the session in one answer.

**Reviewed** (Tabitha, 2026-09-30, 12:47 to 13:05).
- **The gate rerun.** The first rerun's lifecycle bench missed twice, right after the test run's
  writeback: cold start p95 was 62.2 ms, then 74.6 ms, against 57, and the store phase's p95 was about
  50 ms. On a quiet disk, the bench alone passed (cold start p95 41.6 ms). The full gate then passed with
  509 tests and every phase within budget (cold start p95 41.2 ms). T1 does not touch the start path.
- **Reading the code.**
  - Every path that takes a session's lock takes it before an execution's, and no kernel transition
    takes a session's.
  - `run_harness` (`task.create`, `wake.at`) and the error path complete without the hold, and neither
    result can be marked external.
  - `task::create` reads the parent's hold without the lock. The one race is a fetch that completes in
    the same response as the `task.create`, and that call was written before the page was seen, so a
    clean child is correct.
- **A live check on the release build of 41e8fa5,** over a fresh copy of Eddie's store, with his note.
  Discord and the web UI were off, and the Brave key's reference was added, because his note has none.
  - **A search gives the hold, and the allow list waits.** A GLM turn ran `web.search` (a notice), and
    then `ls`, which his allow list runs `open`. `ls` waited: "this session read external text
    (web.search …?q=tokio+JoinSet+documentation…, at 12:55), and a call that acts waits for approval
    after that (§3.9)". Health listed the session. The call was declined.
  - **A job's process cannot trust its own session.** GLM was told plainly that this was the operator's
    test. It asked `proc.run` to run the scratch CLI's `policy trust` on its own session. The call
    waited, and was approved without trust. The job exited 1: "trusting the session again from the CLI
    does not count: from a Theseus job's process (job act_…, pid 366807, theseus). It still holds external
    text". The refusal was ledgered as `approval.refused` (`act: policy.trust`, `from_job: true`), and the
    hold stayed.
  - **Approve and trust, then the allow list is back.** The next `ls` waited. `theseus confirm --trust`
    ran it and cleared the hold (`session.trusted`, `how: action.confirm`). The `ls` after that ran at
    `open`, from the allow list, with no notice.
  - Outbound connections went to api.github.com, api.z.ai, and the Brave search API. Nothing reached
    Discord.
- Eddie's unchanged note loads under the new binary.
- Installed at 12:57.
- **Taken at review:**
  - **A job the operator approves can open a clean session.** It can run `theseus ask` over the socket,
    and that session holds nothing. This needs an approved acting call in the holding session first, and
    the card shows the command. Filed as theseus-d64 (P3): J1's trace, applied at `session.open` and
    `turn.submit`, would pass the job session's hold on.
  - **Cosmetic** (theseus-qiy, P3, fix batch 2): a search's hold names the search API's address rather
    than the query; health gives the hold's time in UTC while the reason uses local time; and a trust
    through an approval names the connection (`sock#32`), not the surface.
  - **Eddie's open questions, with the defaults the chain keeps.**
    - Reads keep their posture, so a fetch's URL can still carry data out, and its notice names the URL.
    - `wake.at` waits in a holding session.
    - There is no Discord `/trust`: the card's button, the CLI, and the Observatory clear a hold.

**Divergence from the brief and the issue.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| "every call whose class is not `Read` waits … It is a tightening, so the stricter posture wins" | Applied after the whole order, the allow list included, as a granted secret's posture is | An allow-list prefix is the easiest way through for a page that steers the model | Keep |
| "A fetch, then `proc.run` in the same turn: it waits" | From the next model call on; a call in the fetch's own response keeps its posture | That call was written before the model saw the page (F3 gates a response's calls first) | Keep |
| — | `wake.at` and `task.create` wait too | Both act (a write, a run), and the rule covers every class but `Read` | Keep; Eddie's question 2 |
| Discord: "a button on the first such confirm, or a slash command" | A third button, **Approve + trust session**, on every confirm the rule raises | The operator learns of the rule on that card, and each later one offers the same | Keep; no `/trust` |
| The CLI: `theseus policy trust <session>` | That, and `theseus confirm --trust <id>` | The card's button, for symmetry | Keep |
| "a `session.external_read` ledger row" per session | One per hold: a session that is trusted and then reads again writes another | "A later external read taints the session again" | Keep |
| — | A trust through an approval names the approval's author (the connection's label) | Approvals record the connection's label | Keep; theseus-qiy |

**Known gaps.**
- A job's output that carries outside text (a download, `gh issue view`, a pulled README, and files it
  leaves that `fs.read` reads later) is not marked external, so it gives no hold (theseus-20f). _(Since 18c, an L1 job that connected out is marked external, Item 62; the rest, at L0, was 20a's, which Tier 1.1 dropped: a listed program's output is marked since Item 74, and text laundered through a file is Jev's, row 39.)_
- A job the operator approves can open a clean session through the socket (theseus-d64).
- A page can still send data out through a fetch's URL: a read keeps its posture, by design, and each
  fetch is a notice with its URL.
- Discord's third button was not pressed live, since the gateway is not faked. The binding's tests
  cover its id and its parse, and the core's test covers the answer.
- An older binary would drop a session's hold the next time it wrote the record. F4a closes this.
- The model is not told that its session holds external text. It learns only when a call waits.

### Item 11. T1b: `wake.at` keeps its posture after web text, a Discord `/trust`, and interactions that route as messages do (theseus-q4t, theseus-e89; 2026-09-30, 16:08–16:36; 9362474, 3aa72a8)

**Why.** Eddie's answers to T1's three questions (2026-09-30, 14:25 to 14:34): fetches in a holding
session keep their posture; `wake.at` is exempt; and a Discord command clears the conversation's hold,
named `/trust` ("I think it's fine, I am over-worrying"). theseus-e89 came from his question at 14:42,
whether a test channel on the same bot could isolate a scratch daemon instead of a second bot: an
interaction found its place by channel or else by the user's DM binding, so a command typed in an
unbound guild channel acted on his DM, and an unbound daemon answered every interaction.

**What exists.**
- **`wake.at` keeps its posture** (`external::exempt`: a `Read`, and `wake.at`). The gate returns its
  decision unchanged and reads no session record for it. The wake's turn is the session's own, so its
  acting calls wait. `task.create` still waits.
- **`/trust`** (`theseus-discord/src/runtime.rs`): registered with the other commands, and named in
  the bind notice. In a bound place it trusts the place's current session through `policy.trust` on the
  binding's own connection, with the presser's Discord ids, so `judge_act(Act::Trust)` and `[approval]`
  judge it as a card's press: a refusal is ledgered as `approval.refused` and the hold stays, and
  `session.trusted` names `discord:dm` or `discord:<channel>` as `via` and the user as `by` and `who`. A
  place whose session holds nothing says so, and nothing is written. A typed `/trust` works as the
  other controls do.
- **Interactions route as messages do** (`on_interaction`): a guild interaction by its channel alone,
  a DM's by its user's DM binding. An interaction in a place the daemon does not bind gets no answer at
  all, so a daemon on the same bot that binds it answers. A bound place still refuses a user it does
  not list. One bot token may now serve several daemons whose bindings name different places, each on
  a build with this fix.
- The template's `[policy] external_text` comment no longer lists `wake.at`, and its `[discord]`
  comment says two daemons may share a token with disjoint bindings.

**How it is proven.**
- The gate at 9362474 ran 525 tests, 2 of them new: `/trust` through `place_for_tests` (cleared, with
  `via discord:dm`; nothing to trust, the ledger unchanged; refused under an `[approval]` that does not
  list the user, the hold kept; typed), and the routing through `on_interaction` against the fake
  Discord REST API (no request at all for an unbound channel, from a user whose DM is bound; a DM's
  command reaches the DM's place; a listed user's `/trust` carries its ids; an unlisted user is
  refused). Five were extended: the rule's unit test, `a_wakes_turn_follows_the_rule`, the command
  list, the bind notice, and the parse. The lifecycle bench passed (cold start p95 28.1 ms).
- The step's live check (16:26–16:32), on a copy of Eddie's store with Discord and the web UI off: a GLM
  turn fetched the `Option` page and then set a two-minute wake, with no wait (`wake.at`'s notice has its
  own setting, `enforcement = notify`); `proc.run echo hi` waited with the hold's reason and was
  declined; the wake fired at 16:30:40, and its turn's `proc.run echo woke` waited with the same reason;
  `theseus policy trust` cleared the live hold (`session.trusted`, by the CLI).
- On real Discord, a scratch daemon on a fresh state dir bound only `#theseus-test`: the log registered
  7 commands, and the bind notice naming `/trust` posted there.
  Eddie's daemon was not running. A bot cannot press, so the presses wait for Eddie.
- Eddie's unchanged note loads under the new binary.

**Divergence from the brief and the issue.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| `/trust`'s description: "Trust this conversation again after it read web text: its calls that change things stop waiting for approval" | "… its calls that change things stop waiting" | 108 characters; Discord refuses a description over 100 | Keep |
| The name `/theseus-trust` (the issue) | `/trust` | Eddie, 14:34 | Keep |
| "Clears the hold … through `Core::trust_session`" | Through `policy.trust` on the binding's connection, which ends in `Core::trust_session` | The connection names the surface, so only the binding can name a Discord user; the same path as the CLI's | Keep |
| — | A typed `/trust` works too | The other controls work typed | Keep |
| — | "Nothing to trust" is read from `session.list` before the trust | Its own reply, and nothing written | Keep |
| — | The narrative's line names `/trust` | It lists the ways to trust again (it named no reminder) | Keep |
| — | An ignored interaction is not counted in the binding's `interactions` | As a message in a place that is not ours is not counted | Keep |
| "A scratch daemon over a copy of Eddie's store" for the live check | Part 2 (real Discord) on a fresh state dir | The operator lane can fall back to a copied session's place, his DM (theseus-c3e) | Keep; theseus-c3e |

**Known gaps.**
- ~~The operator lane falls back to a session's place even where the daemon binds nothing, so a
  Discord-enabled daemon must not run on a copy of another's store (theseus-c3e).~~ Built in Item 55.
- `/trust` and the routing were not pressed live, since a bot cannot press; the binding's tests drive
  them through `place_for_tests` and `on_interaction`.
- A card already waiting when `/trust` clears the hold keeps waiting; its Approve runs it.
- Registering commands is global to the app, so a scratch daemon's list replaces the installed
  build's until Eddie's daemon next starts.

**Reviewed** (Tabitha, 2026-09-30, 17:03 to 17:11).
- **The gate rerun.** The first rerun failed one test, `versions::a_start_at_once_after_a_stop_waits_for_the_store`,
  with "the connection closed" at a shutdown. That was theseus-ur0, the lost stop answer, under load 9 from
  the parallel lanes' builds. It passed 5 of 5 alone, and the second full rerun passed: 525 tests, with every
  phase within budget at load about 9 (cold start p95 38.9 ms, swap 84.0). theseus-ur0 is raised to P1 and
  folded into the next spine step, with theseus-kol.
- **Reading the code.** `on_interaction` now finds a guild interaction's place by its channel alone, and a
  DM's by its user's DM binding. It answers nothing in a place it does not bind, and still refuses an unlisted
  user in a place it does. Two new tests cover the routing and `/trust` under `[approval]`.
- **A live check on the release build of 3aa72a8**, over a copy of Eddie's store (Discord and the web UI off,
  glm live). A GLM turn fetched the `String` page, and then set a 30-minute wake: it was set at once, with no
  approval, while health listed the session as holding external text. The test wake was then cancelled.
- Eddie's unchanged note loads under the new binary.
- **Installed at 17:10** from 3aa72a8.
- **Taken at review:** theseus-ur0 (P1, the gate's flake under the lanes' load) goes with theseus-kol in fix
  batch 1's head. theseus-c3e (P3) keeps Discord-enabled scratch daemons on fresh stores.

### Item 12. Fix batch 1's head: a continuation keeps its profile, and a stop always answers (theseus-kol, theseus-ur0; 2026-09-30, 17:14–17:59; 13adef6, be71fdc)

**Why.** F4a's review found that a `-P glm` turn cut by a SIGKILL continued on the live profile (Sonnet),
that Anthropic refused GLM's replayed thinking block (400, "Invalid `signature` in `thinking` block"), and
that the retry ended `nothing_new`, so the job's result was never answered. T1b's review saw the lost stop
answer (theseus-ur0) fail the gate under the parallel lanes' load. The roadmap's re-cut made this the spine's
first step after T1b.

**What exists.**
- **ur0, a stop always answers** (§3.18). `shutdown`'s method only prepares (`Core::stopping`: the row and
  the checkpoint). The connection that asked queues the answer, asks its writer to flush everything queued
  before the ask, and wakes the serving loops once it has (`Core::wake_after_answer`, bounded at 1 s). The
  socket loop registers its stop waiter once, before the loop, so a stop that lands while it takes a
  connection is not missed.
- **A, a continuation runs on its session's profile** (§3.15). `run_inner` records the turn's target from its
  start. An input that changes it writes it in the input's frame, and every other session write carries it.
  A vanished profile falls back to its provider and model under the live settings, then to the live
  profile.
- **B, thinking goes back only to its own provider** (§4.4). `render_messages` drops the thinking blocks of
  any assistant message another provider wrote. Every recompile strips the prefix's thinking, a model change
  included.
- **C, a failed continuation keeps its input** (§3.15, §4.6). `has_news` counts results after the model's
  last answer as news for every continuation, not only a task's. `Kernel::take_results_with` takes the queue
  and writes the late results' nodes in one frame. The same rule answers a late result that landed during a
  turn, which was answered `nothing_new` before.
- No record layout changed: no `kinds::SCHEMAS` bump, and T1b's build opens a store be71fdc wrote.

**How it is proven.**
- The gate at be71fdc ran 535 tests, 9 of them new:
  - `tests_continuations`, five in-process tests with two scripted providers: the 529 and the 400 retry, a
    failed turn's profile, a result landing during a turn, the queue's frame, and GLM's thinking never
    reaching Anthropic;
  - `theseusd/tests/continuations.rs`, three real-daemon tests: a late result's turn, a restart's
    continuation, and a wake's turn, each on the profile that started it;
  - ur0's unit test.

  Two were changed: B's request-builder test, and the model-change test. Each fix was switched off once and
  its test failed as the bug did. The frame budget still holds 5.
- Stress, release, under 8 busy loops: 0 of 201 stop answers lost on 13adef6, against 26 of 201 on T1b's
  build. The versions test passed 20 of 20 under the same load.
- The step's live check, on a copy of Eddie's store with the live profile at `default` (Sonnet):
  - a SIGKILLed `-P glm` job's restart and late result both ran on glm, and GLM answered "Exit code 0.";
  - Sonnet then answered in the same session, with GLM's thinking dropped;
  - with glm's profile and provider removed from the config, the cut tool loop (GLM's `thinking` and
    `tool_use`, the 400's own shape) continued on Sonnet and was accepted;
  - 120 stops, each followed at once by a start, lost no answer.

**Divergence from the brief and the issues.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| A model change keeps the prefix's thinking (§4.4) | Every recompile strips it, and another provider's is never sent | A signature is its own provider's to verify; GLM's drew a 400 | §4.4 amended |
| B: drop thinking from "another provider or model" | The per-message rule compares providers; the compilation catches a model change | A node records the served model, which can differ from the one asked for, and a tool loop's own thinking must go back | Keep |
| A continuation reuses the session's last target (M3) | It does, and the target is recorded from the turn's start, with a fallback when its profile is gone | A crash or a failure left the previous turn's target, or none | §3.15 amended |
| Results after the last answer start a turn only in a task (DD7) | In every continuation | Each continuation that finds them was woken for them | §3.15 amended |
| ur0: wake the loops once the answer is flushed | That, plus the socket loop's waiter registered before the loop | `notify_waiters` wakes only registered waiters | Keep |

**Known gaps.**
- A failure that recurs is retried without end, with a notice each time (theseus-ljr). It was already true
  for input turns, and a continuation with unread results now retries too.
- ~~A failed wake's or report's turn: the retry answers its node, but posts without the `⏰ wake` line and to
  the session's current place (theseus-4lx).~~ Built in Item 54.
- The fallback for a vanished profile takes the live profile's `max_output_tokens`, which could exceed the
  session model's ceiling. Left as is, since it needs both.

**Reviewed** (Tabitha, 2026-09-30, 18:09 to 18:20).
- **The gate rerun** at be71fdc, with the lanes paused: 535 tests, lifecycle OK. It passed first time.
- **Reading the code.**
  - ur0's writer drains everything queued before the flush ask, and the answer is queued first, on the same
    channel. A writer that is gone drops the ack, so a stop never hangs.
  - A's changed target rides in the input's frame, so a plain turn writes no extra frame.
  - B compares each node's recorded provider. `provider` is a required field of `assistant_message`, so an
    old session's thinking stays with its own provider.
  - C's frame keeps the queue and the node together.
- **A live check on the release build of be71fdc**, over a fresh copy of Eddie's store (Discord and the web
  UI off; the live profile `default`, Sonnet). An `ask -P glm` turn ran `sleep 20` through `proc.run`, and
  the daemon was SIGKILLed 3 s into the job and restarted.
  - The ledger shows three `turn.started` rows (the input, the restart's continuation, and the late
    result's turn), all on glm (`zai`, `glm-5.3-flash`), three `provider.call` rows to `zai`, and no
    `provider.error`.
  - GLM answered "Exit code 0 — reviewed."
  - Ten `theseus shutdown`s, each followed at once by a start, lost no answer and failed no start.
- Eddie's unchanged note loads under the new binary.
- **Installed at 18:18** from be71fdc.
- **Taken at review:**
  - theseus-ljr is raised to P1 and goes into fix batch 1's rest, since a recurring 400 would now post a
    notice every few minutes in a DM.
  - The hardening lane's H3 changed `absorb` too (deleting a job's raw output once absorbed), so its join
    must re-apply that delete after be71fdc's single frame.

### Item 13. Hardening H1 to H4, the first code lane's join (theseus-70f, theseus-s68, theseus-wz2, theseus-skc; 2026-09-30, lane 16:32–17:40, join 18:21–18:35; 40818db, a60f4e6, 44300da, 5b13509)

**Why.** Review 2 (theseus-zaz, 15:24) named four security findings with concrete repros:
- H1: any web page in Eddie's browser could drive the web UI, by DNS rebinding or by a WebSocket from
  another origin.
- H2: one fetched page with multibyte text after a raw element's end tag aborted the daemon.
- H3: the store and raw job output were world-readable.
- H4: `git.diff` read through a working-tree symlink to any file.

They were built as the re-cut's first code lane, in a worktree beside the spine (`lane/hardening`), and
joined here through a spine step.

**What exists.**
- **H1** (`theseusd/src/web.rs`, `theseus-core/src/webui.rs`).
  - An axum middleware refuses any request whose `Host` (or an absolute-form target's authority) does not
    name the UI at its real port: its bind address or `localhost`. A missing `Host` is refused too.
  - `/ws` refuses an upgrade whose `Origin` is not `http://` and an own host, and refuses a missing
    `Origin`.
  - Refusals are counted in health's `web` section. The ledger's `web.refused` row comes at once for the
    first refusal of a kind, then at most once a minute per kind, with the count, so a page can't grow the
    store.
  - There is no per-start token. A token served by the same page would reach exactly the clients that
    already pass both checks. The boundary against another local user is the socket owner's uid
    (theseus-3qf).
- **H2** (`html.rs`). `raw_until` compares the candidate end tag as bytes.
  - Property tests now cover every reader of outside text: the HTML reader, wake's time parsers, the SSE
    line reader, and Discord's `split_text`. That's about 18,500 cases a run, with a random seed, so the
    gate keeps looking.
  - They found three more bugs, all fixed:
    - `<ol start=4294967295>` overflowed the list count;
    - the SSE reader garbled a multibyte character split between two network chunks, in every model reply
      (now `SseLines`: a line is decoded whole);
    - `split_text` looped forever, allocating, when the budget was smaller than the next character.
- **H3** (`theseus_kernel::umask`, `theseusd` main, `toolrun.rs`, `fs.rs`).
  - `theseusd` sets umask 077 before creating anything.
  - The operator's own umask is kept and given back to a job's command (the wrapper's `--umask`, set in
    `pre_exec`) and to the file tools' new files and directories.
  - The state dir, store, and spool are created 0700, and tightened at start when they exist with group or
    other bits.
  - Raw job output is created 0600, and deleted once its result's node is written: in a turn
    (`answer_job`), and for a late result after the frame that takes it from the queue (`absorb`, merged at
    the join).
  - No client is given a spool path: `ResultNode.full_ref` is gone, `session.history` leaves an old node's
    out, and `ActionInfo.result_ref` drops it.
- **H4** (`theseus-tools` `git.rs`). `git.diff` reads the working tree as git does.
  - A symbolic link's content is its target's path, never what it points at.
  - A path under a linked directory, or a tree path that is not plain names (`..`, `.`, absolute), is not
    in the working tree.

**How it is proven.**
- **The lane.** Four signed commits, each gated on exactly its own tree (536, 538, 542, then 549 tests).
  - Every fix's test was proved against a revert of the fix (`revert.py`). For example, without H1 a rebound
    `Host` got `200 OK` and a foreign `Origin` got `101`. Without H4, the diff showed an outside file's
    secret.
  - The lane's live check, on a copy of Eddie's store: a WebSocket probe went 12 of 12 (foreign `Host` 403;
    foreign, null, https, or missing `Origin` 403; the UI's own page 101, and `health` over it).
    `web.refused` rows were at once and then a minute later, matching health's counts. The state dir and
    store went 0775/0755 to 0700 at start.
- **The join** (Tabitha):
  - `lane/hardening` rebased onto f6b68eb (docs v0.64). Commits 1 to 3 applied cleanly. Commit 4 (H3)
    conflicted in `toolrun.rs`'s `absorb`, which be71fdc had rewritten so a late result's node rides in the
    frame that takes it from the queue. Resolved by collecting each absorbed job's raw output path in
    `late_results`, and deleting it once that frame is written (`remove_raw_output`, shared with
    `answer_job`). H3's own test, "absorbed, then gone" after a background job's continuation, holds the
    merged path.
  - `Cargo.lock` resolved unchanged (`cargo metadata`; the lane only adds `proptest` as a dev-dependency).
  - The whole gate on the joined tree, the other lanes paused: 559 tests, lifecycle OK.

**Divergence from the brief and the review.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| H1: a per-start token for the web UI (review 2's option) | `Host` and `Origin` checks, no token | A token from the same server reaches exactly the clients that pass both checks; the real boundary is the peer's uid | Keep; theseus-3qf |
| H1: default `[web] enabled` to false in the interim (review 2) | Left on | With both checks in, the default can stay; Eddie's call if he wants it off | Keep |
| H3: tighten the store's directories | Directories tightened at start; files that already exist keep their bits | The 0700 directories leave other users no path to them; a store made since has none | Keep |
| H3: raw job output deleted or swept | Deleted once absorbed; output no result absorbs is not swept | The common path is clean; the rest needs a sweep by action state | theseus-2ij, built in Item 25 |
| — | `git.diff` also refuses tree paths that climb out (`..`, absolute) | Found while reviewing the fix: a fetched tree can hold one | Keep |
| — | Three more bugs found by the property tests, fixed | The tests were asked for; their finds came with them | Keep |

**Known gaps.**
- ~~The web UI's port is still open to other local users' processes (theseus-3qf: refuse a peer whose uid
  is not the daemon's).~~ Closed by the `secfix` lane (Item 22).
- ~~Raw output that no result absorbs is not swept: a cancelled job, a crash between the frame and the
  delete, and every file from before H3 (theseus-2ij).~~ Closed by fix batch 2 part 3 (Item 25).
- ~~`git.diff` and `git.log` open their repository with `gix::discover`, which climbs above the roots
  (theseus-bsc).~~ Closed by the `secfix` lane (Item 22).
- ~~The Vite dev server's `/ws` proxy is refused by H1, since it passes the dev page's headers (theseus-zab;
  the built app is unaffected).~~ Closed by the `secfix` lane and the cockpit (Items 22 and 23).
- Health's `web` section is in the JSON only; the CLI's text summary and the web UI don't show it yet. The
  cockpit shows it (Item 23); the CLI and the Observatory still don't (theseus-jxau).
- The panic policy (unwind, or abort under a supervisor) is still Eddie's call, from review 2.

**Reviewed** (Tabitha, 2026-09-30, from 17:52: the report; the join from 18:21).
- The lane's report was read in full. The fixes, their reverts, and the property tests' finds are as
  described.
- The join's conflict was resolved as above, and the whole gate passed on the joined tree.
- **A live check on the release build of 5b13509**, over a fresh copy of Eddie's store (Discord off; the web
  UI on at 7436; `proc_sync_secs = 2`):
  - At start, the state dir went 0775 to 0700 and the store 0755 to 0700, each with its `tightened` log
    line. The spool was made 0700.
  - The WebSocket probe went 12 of 12, with health counting `{"refused_host": 2, "refused_origin": 5}`.
  - An `ask -P glm` turn ran `sleep 6` through `proc.run`. The job outlived the turn's 2 s and came back
    as a late result (a `tool.late_result` row). Its continuation ran on glm and answered "Exit code 0 —
    joined." Afterwards the spool's `results/` was empty: the merged `absorb` deleted the raw output.
  - No `provider.error`.
- Eddie's unchanged note loads under the new binary.
- **`main` fast-forwarded** to 5b13509 and pushed. `lane/hardening` was force-pushed with a lease, since it
  was rebased.
- **Installed at 18:34** from 5b13509: the binaries the live check ran.
- When Eddie's daemon next starts on this build, it tightens `~/.theseus` and its store to 0700, once, with
  a log line each.

### Item 14. The dogfood pilot: Theseus builds theseus-kks itself (theseus-14s, theseus-kks; 2026-09-30, 21:21–22:51; d10294f)

**Why.** Eddie, 2026-09-30 15:17: "It would be fantastic to start developing theseus on theseus -- very on
brand". The re-cut made the pilot spine row 2: Theseus builds one small fix to itself, and Tabitha reviews it as
any step. At 19:37 Eddie decided its isolation: the builder runs as him, in the same general context, with the
same approvals and notifications as a normal agent. So the pilot has no second user and no sandbox.

**What exists.**
- **The pilot's harness**, which is not product code:
  - A builder daemon: the installed build (5b13509), with its own state dir (`~/.theseus-builder`), socket,
    and worktree (`lane/pilot`), bound only to `#theseus-test`, with Eddie's posture unchanged.
  - A `theseus-dev` persona: the agents' operating notes for the repo, plus a short "How you work here". 52 KB.
  - A profile `opus` on `claude-opus-5-5`, and `spend_limit_usd = 40`.
  - A runner (an OpenClaw subagent) set it up, sent the step brief as the first message of the channel's
    session, watched, and measured. It never touched the step's work.
- **theseus-kks, as the builder built it** (§3.13's new sub-bullet):
  - `turn::budget_question` is the one text, for the turn's `confirm.requested` and for every surface that
    renders the question again. It is unchanged, byte for byte, for an ordinary over-budget call.
  - `Kernel::ask_budget_for` keeps the call's profile, model, and output cap in the question's proposal as
    `args.call`. `ask_budget` delegates with `Null`, so the kernel's tests and kernel-sim are untouched.
    `budget.asked` gains `exceeds_limit`.
  - After an approved reset of a call over the whole limit, `catch_up` sets `retry_over_limit`. If the retry's
    call still does not fit, the turn fails with class `over_limit` (not transient) and a `budget.over_limit`
    row. It does not ask again. A call that fits clears the flag, so a later call over the limit asks as usual.
  - No record layout changed: `args` is the proposal's JSON, and `exceeds_limit` is a ledger field.

**How it is proven.**
- **The builder's three tests** in `tests_m3`:
  - a call over the whole limit asks once and names the remedies;
  - an approved reset of it does not ask again;
  - an ordinary over-budget call still asks, resets, and goes ahead.

  Its revert proof: the first two failed on the bug itself. The third guards the unchanged path, so it failed
  only on the new `exceeds_limit` field, as the builder's report says plainly.
- **The gate:** the builder's run passed (562 tests, lifecycle OK, deny OK) after it fixed its own clippy slip.
  The runner reran it, and the revert, in a throwaway worktree: green, and each test failed without the fix.
- **The builder's live check was not done.** Its two scratch-file writes outside its roots waited for
  approval in `#theseus-test`, and nobody answered: the cards ping no one (theseus-9j9). The runner declined
  each after 30 minutes. The builder treated each decline as final, and reported the check as not done
  rather than route around it through the shell.

**The pilot's numbers.**
- **Time:** 75 minutes from the first message to "Done.", of which the two unanswered approvals took an hour.
  About 14 minutes was work.
- **Cost:** $3.22, over 57 Opus 5.5 calls.
- **Help:** no operator intervention, and no correction.
- **Snags:** one tool error (`fs.patch`; it used `fs.edit` 19 times after that).
- **Channel noise:** 49 messages and 56 edits in the channel for one step.

**Divergence from the brief.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| The step's live check, by the builder | Not done by the builder; done at review | Its scratch writes outside its roots needed approval, and the cards notified nobody | theseus-9j9 (P1); theseus-2tw (a scratch root for a builder) |
| Approvals answered by Eddie in the channel | None answered; both declined after 30 minutes | `allowed_mentions` is empty on a card | theseus-9j9 |
| kks's behaviour after an approval: the brief left it to the builder | One retry, then `over_limit` | It keeps the kernel's invariant that reservations fit the limit, and an approval after a config change is useful | Keep |

**Known gaps.**
- ~~**A reset leaves amounts held unknown in place** (theseus-6g6, found by the builder). A call with
  `needed ≤ limit` but `needed > limit − held_unknown` comes back after each approval too.~~ Built in Item 54: the question says so, and an approval
  tries the call once.
- **The pilot's friction, each filed:**
  - theseus-9j9 (P1): a card pings nobody;
  - theseus-2tw: a builder has no scratch root;
  - theseus-6i0: a batch's approvals come one at a time;
  - ~~theseus-830: a card says it expires, but a request never does;~~ built in Item 55;
  - theseus-lqk: `max_loops` (40) parks a builder's step;
  - theseus-8wm: notices flood the channel;
  - ~~theseus-ewi: `proc.run` may write outside the roots under `notify` while `fs.write` asks (a decision for
    Eddie);~~ decided and built in Item 55;
  - theseus-inw: `fs.patch` rejects a hunk whose header counts are off.
- **The persona's nextest wording** ("`-j 4`") led to one failed command. The next pilot's persona says
  `--build-jobs 4 --test-threads 4`.

**Reviewed** (Tabitha, 2026-09-30, 23:15 to 23:26).
- **Reading the code.**
  - `retry_over_limit` comes only from a settled, approved budget question whose own proposal says
    `needed > limit`. A raise that withdraws the question is not an approval, so it doesn't set the flag.
  - The failing path runs before any provider call, so the turn has no settled call of its own, and no fault
    wake follows (the pattern of theseus-ljr). The builder's test holds the execution at `Waiting` on input.
  - A question asked before this build has no `args.call`, and renders the generic remedy.
- **The live check the builder could not run.** It used the release build of d10294f, on a fresh state dir
  under `/tmp`, with Eddie's note (Discord and the web UI off) and `spend_limit_usd = 0.002`.
  - The question read: "This session is waiting on the call to claude-sonnet-5-5, which alone reserves $1.29:
    more than its whole $0.002 limit, so resetting its spend to $0 cannot make it fit. Raise `[kernel]
    spend_limit_usd` above $1.29, or lower `max_output_tokens` under `[profiles.sonnet]` (now 128,000)." It
    cost $0 and 0 tokens, and made no provider call.
  - The ledger: `budget.asked` with `exceeds_limit: true`. Then, after `theseus confirm` approved it,
    `budget.reset`, one continuation turn, `budget.over_limit` (needed $1.2867), and `turn.failed`
    (`over_limit`, $0).
  - Nothing was waiting for confirmation. Twelve seconds later there were still two turns, and the execution
    was `waiting` with nothing queued: no loop.
- **The gate rerun** at d10294f on `main` (23:22 to 23:24, the lanes paused): 562 tests, lifecycle OK.
- Eddie's unchanged note loads under the new binary: 8 secrets resolved.
- **`main` fast-forwarded** to d10294f and pushed. **Installed at 23:24** from d10294f: the binaries the live
  check ran.
- **The verdict on more builders:** yes for the coding. Not yet for unattended steps with a live check, until
  theseus-9j9 and theseus-2tw land. The next spine step folds in 9j9, which is cheap and recovers the pilot's
  hour.

### Item 15. Fix batch 1, part 2: no endless retry, a card that reaches its answerer, and an image that can't poison a session (theseus-ljr, theseus-9j9, theseus-0s4; 2026-09-30 23:27 to 2026-10-01 02:31; 6b9e515, 02e7834, 276205b, c27c661, 8ed512f)

**Why.** The roadmap re-cut's row 3, split, with the pilot's theseus-9j9 folded in.
- A failure that would not pass was retried forever, with a notice each time (theseus-ljr, raised to P1
  at Item 12's review).
- The pilot's builder lost an hour to approval cards that pinged nobody: two waited 30 minutes each.
- The 9g2 review's first risk: an image the provider rejects went out again in every later request of its
  session (theseus-0s4).

**What exists.**
- **ljr, a failure's retries are bounded** (§3.15):
  - the session record keeps its run of failures (`failing`, session schema 3);
  - a transient class keeps the driver's backoff for as long as it lasts;
  - any other class, an internal fault included, gets one silent retry, then parks the execution on input;
  - a turn that settled nothing of its own (kks's `over_limit`, an unpriced model) parks at once;
  - the run posts one notice, plus one when it parks, and `turn.next` ledgers each decision.
- **9j9, a card names who can answer it** (the Discord binding):
  - in a guild channel a card starts with `<@id>` for the place's `users` (under `[approval]`, its trusted
    ones), and its `allowed_mentions.users` is exactly them;
  - every other message, and a card in or routed to a DM, mentions no one;
  - `discord.message.out` records the mentions Discord answered with.
- **0s4, an image the provider refuses is shown as its line** (attachments, §4.4):
  - a 400 that names an image marks it not shown in the session record (`not_shown`, by digest, session
    schema 4) and makes the call again with its line, once a turn;
  - Anthropic's own "Could not process image" names no block, and then the error means the images after
    the model's last answer;
  - every later request, recompile, and restart renders the line, for every copy;
  - when a copy sat before an answer, the retry strips the prefix's thinking (`image_not_shown`), which
    preserved thinking requires.
- Two schema bumps in one step: SESSION 2 to 3 (ljr), then to 4 (0s4). Each has its reader and test, per
  P5b. **A store this build has written a session record to is refused by 02e7834 and older**, and one
  6b9e515 wrote is refused by d10294f and older.

**How it is proven.**
- The gates: 572 tests at 6b9e515, 576 at 02e7834, 582 at 276205b, 583 at c27c661, and 584 at 8ed512f,
  each with the lifecycle bench OK. Each fix was switched off once, and its tests failed as the bug did. The frame
  budget still holds 5.
- The tests:
  - ljr: `tests_failures`, and `theseusd/tests/failures.rs` (the real driver: no third call 4 s after the
    park; the 529's backoff kept);
  - 9j9: `tests_outbox` with the fake Discord's notification model;
  - 0s4: five `tests_m3` turn tests and the parser's tests.
- The live checks, on release builds and fresh state dirs from Eddie's note:
  - ljr: a model the provider does not serve failed twice and parked, with no third attempt 5 min 41 s
    later, and the next message answered;
  - 9j9: a card in `#theseus-test` pinged Eddie once (Discord's answer: `mentions` = his id alone), and
    the footer, tool line, and reply mentioned no one;
  - 0s4, on Sonnet 5.5: Anthropic refused a PNG with a valid header and corrupt pixel data with a 400,
    "Could not process image", which names no block. The image was hidden, the call was made again, and
    the model said it could not see it. The next turn, and one after a restart, answered with no 400. On
    8ed512f, a session holding an image the model had answered over hid only the new one, and the model
    could still see the first.

**Divergence from the brief and the issues.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| ljr: a non-transient class retries once | And a turn that settled nothing of its own parks at once | A retry of a call that was never made changes nothing; kks's `over_limit` is that case | Keep |
| ljr: transient and non-transient provider classes | An internal fault is a lasting class (`internal`) | A recurring fault was retried forever too | Keep |
| ljr: one notice per run of failures | One, plus the park's when a run that backed off turns lasting | The park changes what the user must do | Keep |
| ljr: (not said) where the run is kept | On the session record, schema 3 | A restart must neither retry it again nor post twice | Keep |
| 9j9: show the card's mentions | `discord.message.out` records Discord's `mentions` | The read-back the live check needed | Keep |
| 0s4: mark the attachment not shown | Marked by blob digest, per session, so every copy hides | The same image sent again would fail the same way | Keep |
| 0s4: render the line and retry | Also strip the thinking over a changed history | Sonnet and Opus 5.5 bind a thinking block to every message before it | Keep |
| 0s4: a 400 that names an image, or its block's index | Also one that names neither: the images after the model's last answer | Anthropic's 400 for a corrupt PNG names no block, so a session holding another image stayed poisoned | Keep |
| 0s4: one commit | Three (276205b, c27c661, 8ed512f) | Run 3 found the strip blind to an earlier copy, and the live check found the 400 without a path | Keep |
| 0s4: check the two image limits against the claude-api reference | Not in its bundle (2.1.285 and 2.1.286) | The live vision docs were out of bounds | Unverified, in theseus-8gf |

**Known gaps.**
- A turn hides one refused image: a request with several bad images (a many-image limit) recovers one
  per turn, with ljr's park in between (theseus-8gf, P3).
- Theseus rebuilds `messages` from its nodes for every request, and has never run preserved thinking's
  three-step check. An account created on or after 2026-08-31 would see any rendering drift as a 400
  (theseus-3za, P2).
- An image's retry takes a loop index, so it can run one call past `max_loops` (40 in Eddie's note)
  (theseus-6hk, P3).

**Reviewed** (Tabitha, 2026-10-01, 02:32 to 02:53).
- **Reading the code.**
  - ljr: `Failing::after` decides from the run so far. A turn that settled nothing parks at once. A call the
    provider answered with an error counts as settled, so a 529 on a turn's first call still backs off. An
    input turn starts a new run, and the model's answer ends one. When the record cannot be written, the
    rule still answers, from the run as it was.
  - 9j9: only a card in a guild channel names anyone. `answerers` is the place's users, and under
    `[approval]` only its trusted ones; `allowed_mentions.users` is exactly them. Every other message
    sends `parse: []` and no `users`.
  - 0s4: a 400 that names no block falls back to the images after the model's last answer, and only when
    its text says "image". A lone image with nothing newer hides itself; older images with nothing newer
    hide nothing. The marks reach the store at once, one per digest, each with an `image.not_shown` row.
- **The live check**, on the release build of 8ed512f, over a copy of Eddie's store (Discord and the web UI
  off, its own socket and state dir):
  - d10294f and 8ed512f list the same five sessions, and each session's history is byte-identical under
    both.
  - One Sonnet 5.5 turn on 8ed512f ($0.0129) wrote a session record at schema 4, and a restart read it back.
  - d10294f then refused the copy: "holds session records (kind 1) at schema 4, and this build reads session
    records up to schema 2: install the newer theseusd".
  - `theseusd check` on Eddie's note: 8 secrets resolved, and the GitHub token is ok.
- **The gate rerun** at 8ed512f on `main` (02:43 to 02:44): 584 tests, lifecycle OK in 7.0 s.
- **Eddie's store was copied** to `~/.theseus-backups/store-pre-fb1b-20261001-024659` before the install. It
  is the only rollback.
- **Installed at 02:50** from 8ed512f, which the step had already pushed to `main`.
- **Part I** now says what the three fixes do: §3.15's bounded retries, a card's answerers in the Discord
  binding, and the refused image in attachments.
- **Builders.** With 9j9 in, a builder's card now reaches Eddie. theseus-2tw (a scratch root for a builder's
  live check) is what still stands between a builder and an unattended step, and the friction batch takes it.

### Item 16. The first merge batch: ten lanes on `main` ahead of their readers (theseus-zaz.18; 2026-10-01 02:58 to 03:18; 1fe9c0e to abf01c5)

**Why.** Eddie, 2026-10-01 00:00: "merge and dismiss branches once done". The lane recipe's rule 3 now merges a
lane as soon as it is reviewed. It no longer waits for the spine step that reads it.

**What landed.** One lane at a time, in this order. Each was rebased onto `main` and gated on `main`'s own tree.
Then `main` was fast-forwarded and pushed, and the lane's branch (origin and local), worktree, and target dir
were deleted. None of these crates is on `theseusd`'s path yet: each waits for its reader, the spine step named.

| Lane | What | Its reader | Landed | Gate |
|---|---|---|---|---|
| sandbox | `theseus-sandbox` (L1: namespaces, seccomp, the init; the egress proxy), and `theseus-tools`' net seam | 17b, 18c | 03:00, 1fe9c0e | 619 tests |
| ontology | `theseus-ontology` (kinds, seed rows, compose) | 21b | 03:02, 3718f49 | 666 |
| judge | `theseus-judge` (the Jev client, bands, batching, the breaker, the six packs, `learn.rs`) | 23a | 03:04, 2237494 | 740 |
| math | `theseus-memory` (FSRS-6 and spreading activation, pure) | 30a; 32a and 32b's wire-ins | 03:05, 8454626 | 771 |
| exam | `theseus-exam` (exam v1.1 and v2, the checks, the statistics) | 34b | 03:06, bcff18f | 820 |
| mcp | `theseus-mcp` (the client, a fake server, the server side) | 36b, 41b | 03:12, dc367b1 | 869 |
| cache | the byte-identical header test; the Observatory's cache figures | 13c | 03:13, c2a507d | 871 |
| index | `theseus-follow` (the WAL follower) and `theseus-index` (tantivy BM25 and entities, the tender) | row 51 (29b's wire-in) | 03:16, c4abfed | 915 |
| aws-infra | `infra/aws/` (four templates, stack policies, `check.sh`) | C2 (14b) | 03:17, 05801f6 | 915 |
| aws-guard | `theseus-aws-guard` (`guardrails.toml`, the evaluator, the scanner, the generated guards and SCPs) | C2 (14b) | 03:18, abf01c5 | 941 |

Every gate's lifecycle bench passed; two of them only on the gate's own rerun (below).

**What the joins changed.**
- **ontology:** the test fixture named the real company and its operator. They are invented now (Kestrel, Ada).
  The two guidance digests the text changed were recomputed outside the code, by Python's `hashlib`, which also
  reproduced the old pair from the old text. Its first gate failed on rustfmt alone, since the shorter names
  folded lines.
- **exam:** two asserts that keep a real repository's name out of the exams spelled the name. It is one constant
  now, spelled in parts. The exams and their digests are unchanged.
- **cache:** fb1b (Item 15) and this lane each changed the fake model server, one for arrival times and a queue
  of refusals, the other for each request's raw bytes. The merged server keeps all three. `web/dist` was rebuilt
  from the merged source, byte-identical to the lane's.
- **index:** `Cargo.lock` gained tantivy 0.26.2's 48 packages, at exactly the versions the lane tested. The store's
  one change, `read_frame` and three items made public, was read: read-only, and checked by the store's own
  `check_frame`.
- **aws-guard:** reviewed at 02:56. Its model tests had defaulted to an old CLI's models (2.9.13). They now find
  botocore beside the `aws` on PATH (dc93ae7): 523 operations, 77 paths, and the boundary's patterns, all against
  2.34.15. The live check, rerun read-only, simulated 125 actions with 0 mismatches. Access Analyzer accepted all
  7 documents; its one warning, allow-all's `iam:PassRole` in the boundary, is in a boundary's nature.
- **aws-infra:** `infra/aws/check.sh` passes on `main` (cfn-lint, the rules, 31 tests).

**What the gates found.**
- **A flaky test from Item 15.** mcp's first gate failed on
  `a_card_in_a_channel_mentions_its_answerers_and_nothing_else_mentions_anyone`, a 9j9 test. Run alone at
  dc367b1 it failed 4 runs in 8: it read the channel once the outbox drained, and the tool line can land after
  that. It now waits for every message it reads, and passes 20 runs of 20 (9bd55a6). Whether the tool line can
  land after the reply in a real channel is theseus-50p (P3).
- **Two bench misses, each passed on the gate's rerun.**
  - mcp: the cold start from the config copy, p95 62.7 ms against 57. One run in ten; the rerun's p95 was
    29.6 ms.
  - aws-guard: both cold starts.

  Each ran beside the aws-client lane's test run, which `tools/theseus-quiet.sh` does not pause: a lane's tests
  keep wall-clock deadlines that a pause would break. Gates now take a shared lock, `~/.cache/theseus-gate.lock`,
  which lane gates take too, so the two no longer overlap. The script also scans every second for lane builds
  that start mid-gate.

**The spikes.** The voice and embedding spikes' branches were deleted, their trees archived under `~/reports`.
The embedding spike's verdict (29a, theseus-zaz.17) is candle 0.11, at f32, on one thread. It goes to 29c, along
with what 30a needs to know: a query takes 85 to 90 ms to embed, so recall's 60 ms p95 can't hold if it waits on
the vectors.

**Left open.** `lane/aws-client` (P1's catalog is committed; P2, the client, is still running).

**Reviewed** (Tabitha, 2026-10-01, 02:56 to 03:24). The batch is this review: each lane was read when its report
landed (Items 12 to 14's dates), its join is above, and its gate log is in `~/reports/theseus-merge/`.

### Item 17. Fix batch 1, part 3: a first open a kill can't brick, a tear that respects durability, and a
cancel that cancels everything (theseus-0b8, theseus-4x6, theseus-w98; 2026-10-01 03:24 to 04:30; c79a903,
8f1f293, fe28405, 69c1e7e)

**Why.** The rest of fix batch 1, the roadmap re-cut's row 3.
- A SIGKILL inside the store's very first open left `index.redb` unopenable, and the fix was deleting it
  by hand (theseus-0b8).
- The crash test's `--tear` could damage durable bytes, so the gate ran it with tearing off
  (theseus-4x6).
- A cancel left a call waiting for approval planned, so it still counted as waiting (theseus-w98).

**What exists.**
- **0b8, an index that is not a database is moved aside and rebuilt** (§6):
  - redb's error kind (`Storage(Io)` of kind `InvalidData` or `UnexpectedEof`) tells it from a held file
    (`DatabaseAlreadyOpen`) and from a real database that fails another way (`Corrupted`);
  - the move happens under the file's lock, on the file the name still names, and keeps it as
    `index.redb.bad-<unix ms>`;
  - the index is built again from the WAL, and the open says so in a WARN, the startup store phase, and a
    `store.index_replaced` row;
  - the crash test's workaround is gone, so a kill lands inside the first open again.
- **4x6, a tear stays out of durable bytes** (§8). The bound is the longest length the worker reported
  durable, or an open read back after a kill. The worker's reports alone were not enough: an open
  checkpoints frames no worker reported, and a later tear could cut them. `--tear` defaults on again, and
  the gate tears.
- **w98, a cancel ends everything unsent** (§3.15):
  - every action planned or authorized and not dispatched settles cancelled in the cancel's frame, with
    its resolution;
  - each tool call's "Not run" result rides in the same frame, and the cards settle;
  - a turn that ends its execution does the same;
  - the simulator checks that no ended execution keeps one, and its turns now ask the operator.

**How it is proven.**
- 0b8:
  - three store tests: a partial header, random bytes, and zeros, each moved and rebuilt; a held index,
    even one not yet a database, refused and never moved; a real redb file with another failure left;
  - a second process holding the store, refused;
  - the crash test: seeds 3, 7, and 11, loops of 40 and of 30 × 8 restarts, and six runs at once with 32
    kills inside the first open;
  - live, on a copy of Eddie's store with a 37-byte partial header for its index:
    - the file is moved aside and the index rebuilt from 1,490 records;
    - `theseus sessions` and all five histories are byte-identical to the copy with its index intact;
    - a second daemon beside it refuses after 3 s, as before, and moves nothing.
- 4x6:
  - a unit test: no tear lands inside the bound, over 9 bounds × 200 seeds;
  - the loaded A/B: 4 of 6 runs pass with the reports alone, 6 of 6 with the bound;
  - live, the release build with `--tear`: seeds 7, 3, and 11, a loop of 50, and 25 × 8 restarts all pass,
    with 4 kills inside the first open between them.
- w98:
  - kernel, core, and Discord tests;
  - the simulator's invariant fails against the old cancel at seed 1, step 18, and holds over 16 seeds;
  - live: a GLM `proc.run` of `dd --version` waits for approval, and `theseus executions cancel` follows.
    The session list goes from 1 waiting to 0, `confirm` from the question to "nothing is waiting", and
    the history ends with "← proc.run cancelled · Not run: the execution was cancelled by the CLI.".
- The gate was green at each commit: 945, 946, and 950 tests, with the lifecycle bench within its budgets
  and a plain turn still 5 frames.

**Divergence from the brief and the issues.**
- 4x6's bound adds what an open read back to the worker's reports, as the loaded A/B found it must.
- w98 also covers a turn that ends its execution, not only a cancel: the new invariant needs it.
- w98 also refuses `authorize`, `dispatch`, and `authorize_and_dispatch` on a call a cancel settled as
  `NotRunnable`, so a running turn hears the cancel as it did before.
- w98 also gives the core a "Not run" result in the cancel's frame, which is what makes the history show
  the call cancelled.
- The crash between plan and authorization is filed as theseus-ni5, not fixed: the product can't
  produce it any more.

**Known gaps.**
- ~~theseus-0o8: a cancelled execution's dispatched calls, and a running turn's planned ones, get no
  result in the transcript.~~ Built in Item 54.
- ~~theseus-ni5: the never-asked planned call (the kernel API only).~~ Built in Item 54.
- theseus-2fs: the sandbox lane's flaky `clause_10` test.
- ~~theseus-2qt: a cancel, and a turn that ends its execution, scan every action in the store
  (`open_actions`), as a stop already did. That costs little on Eddie's store (62 actions), and grows
  with the store.~~ Built in Item 46.
- An execution an older build cancelled may still hold a planned call that counts as waiting. Eddie's
  store holds none.

**Reviewed** (Tabitha, 2026-10-01, 04:31 to 04:40).
- **Reading the code.**
  - `move_aside`: it takes the file's lock, and checks that the name still names the locked inode. A file of 320
    bytes or more that starts with redb's magic is a database and stays. Anything else is renamed, never
    deleted, and the directory is synced.
  - `end_unsent`: only this execution's planned or authorized actions, each with its resolution, its row, and
    its reservation released. Its scan of every action (theseus-2qt) runs only when an execution ends, never on
    a turn.
- **The gate rerun** at 69c1e7e (04:32 to 04:34; it first waited for the vectors lane's gate to release the
  shared lock): 950 tests, lifecycle OK in 7.0 s.
- **A second live check**, on the release build, over fresh copies of Eddie's store:
  - an empty `index.redb` is recreated by redb itself, with nothing moved and every history identical;
  - 100 zero bytes are moved aside and rebuilt, with every history identical;
  - the installed 8ed512f then serves the rebuilt copy, so rolling back still opens a store this build
    rebuilt.
- **What stays as it was.** A redb file that is a database but `Corrupted` (cut below its layout, bad commit
  slots) is still refused, and its recovery is still `theseusd restore` from the WAL directory. The lane's
  crash loops never made one: 32 kills inside a first open under load, each moved and rebuilt.
- **Eddie's store was copied** to `~/.theseus-backups/store-pre-fb1c-20261001-043807`, and the build **installed
  at 04:38** from 69c1e7e. No layout changed, so the copy is a precaution, not the only rollback.

### Item 18. Two more lanes on `main`: the AWS client, and `theseusd install`, proved as root (theseus-mgw.2, theseus-7hh; 2026-10-01, merged 04:41 and 04:43; f3b2eeb, fcf833c)

**Why.** The lane recipe's rule 3 (Item 16): a reviewed lane merges at once, ahead of its reader. Both waited
only for fix batch 1's last step (Item 17), which held `main`.

**What landed.**

| Lane | What | Its reader | Landed |
|---|---|---|---|
| aws-client | `theseus-aws-catalog` (every operation of 416 AWS services, from the CLI 2.34.15's botocore models: a 2.27 MB brotli blob, one service decoded on first use, in milliseconds) and `theseus-aws` (one caller for all six protocols: SigV4 through `aws-sigv4`, per-operation endpoints, retries by retry class, pagination, denials that name their enforcer) | C1 (14a) | 04:41, f3b2eeb; gate 1,022 tests |
| installer | `theseusd install`: a plan by default, `--apply`, `--check`, `--user`, and `--separate` with `--remove` and `--migrate-state` | 22b | 04:43, fcf833c; gate 1,053 tests |

**The AWS client's review** (2026-10-01, 03:30 to 03:45).
- **Its proof.** 34 requests and 28 answers generated by the CLI's own botocore, offline. AWS's SigV4 suite, 40 of
  40. A fake endpoint for retries, pages, and caps. A read-only live check on the Home account: 8 reads, plus 7
  reads of what doesn't exist, each failing as expected, across all six protocols. Rerun on the reviewed build, it
  matched.
- **A bug fixed at review** (0ac3b4e on the lane, f3b2eeb on `main`). The call's last attempt decided its error. So a repeatable write whose first
  attempt dropped after sending, then never reconnected (or was refused, or throttled), came back as never sent,
  and `may_have_run()` said no. That would let a model retry an EC2 launch under a new token and launch twice.
  An attempt that may have run now leaves the call's outcome unknown, whatever follows. The test fails without
  the fix.
- **A smaller one:** a deny that names `theseus-boundary`, the hands' boundary that carries the guards, is the
  guard's, not IAM's.
- **Filed:** theseus-fln (P3). The catalog generator should find the models beside the `aws` on PATH, and the
  guard's model tests could read the embedded catalog.

**The installer's review** (04:33 to 04:42). Eddie ruled out Docker for its proof, so the real `--separate` ran
on this machine, with sudo (his word, 2026-09-30 23:45), then was torn down (23:58).
- **Reading the code that runs as root.**
  - Every deletion is one planned file, an empty directory (never recursive), or a socket.
  - Ownership changes use `lchown`.
  - The account tools run by absolute path. `useradd` makes no home, and `userdel` runs without `-r`, so the
    state dir never goes with its user.
- **The run.** As root, the plan listed 12 actions, as the lane's report had them verbatim.
  - `--apply` made 12 changes, `--check` matched, and a second `--apply` found nothing to do.
  - The layout read back as §2.9 says, `systemd-analyze verify` passed the unit, and nothing was enabled or
    started.
  - `--remove --apply` made 12 changes: `userdel` had already taken the user's own group, so that step had
    nothing left. `--remove --check` matched.
  - A read-only snapshot of the machine was identical before and after: no user, no groups, no files, and no
    membership.
- **Findings for the chain.**
  - **The daemon stops cleanly only on SIGINT** (theseus-bv5, P2). A SIGTERM, which is systemd's stop, skips the
    clean path. The next spine step takes it. Until then, the units set `KillSignal=SIGINT`.
  - 22b's: the daemon's own toollets, run as `theseus`, can't read the operator's home. The separated socket stays
    0600. `op` isn't on the unit's PATH.

**Both merges** were gated on `main` (`~/reports/theseus-merge/<lane>-gate.log`), each with the lifecycle bench OK in
7.0 s, and both branches and worktrees are deleted. Only `lane/vectors` remains.

### Item 19. Fix batch 2, part 1: a clean stop on any signal, posts that settle before it, a restore that is durable, and a spool that never keeps a granted secret (theseus-bv5, theseus-pfv, theseus-ez3, theseus-l0d; 2026-10-01 04:44 to 05:57; a871640, 79a895c, 93a87f6, 6448534, 5914183)

**Why.** The roadmap re-cut's row 4, fix batch 2, split; the shutdown and durability half first.
- A SIGTERM, which is systemd's stop and `kill`'s default, killed the daemon outright: the socket stayed
  behind and the clean path never ran (theseus-bv5). The installer's units sent SIGINT instead.
- A clean stop about a second after a turn's end could cut a reply post between its write and its settle,
  so the next start sent it again (theseus-pfv).
- A restore said "restored" before its copies were durable: a power loss soon after could leave a short
  history (theseus-ez3).
- A program that prints its own granted secret, as `gh auth token` does, put it in the spool's raw output
  file, which the floor keeps (theseus-l0d).

**What exists.**
- **bv5, one clean stop** (§3.22): the serving loop takes SIGTERM as it takes SIGINT, each registered once.
  Both now do what a client's `shutdown` does before its answer: a `server.stopping` row, naming the
  signal, and a checkpoint (`Core::stopping_on`). SIGINT's path had skipped both, so its next open
  replayed the start's own rows.
- **pfv, posts that settle** (§3.16, §9):
  - a post is in flight from just before its dispatch until it settles or its delivery gives up
    (`outbox::Sending`); the stop's row marks the outbox stopping, and no post is dispatched after it;
  - `Core::finish_stop`, at the end of every clean stop, waits for the posts in flight until
    `[server] stop_grace_ms` (default 50) after the stop began, then checkpoints after them; a post still
    in flight stays dispatched, as before;
  - a checkpoint with nothing written since the last costs nothing;
  - the fake Discord can hold the answer to a write by its content (`hold_writes_containing`).
- **ez3, a durable restore** (§6): every sync goes through one small trait (`Durable`): each copied segment
  and blob before the open; the staging store's `wal/`, `blobs/`, and the staging store after the open;
  the state dir after moving an occupied store aside, and after the rename.
- **l0d, no granted value on disk** (§3.19): the wrapper gets each granted variable's name and its
  secret's (`--redact`, names only). With a grant, the command writes into a pipe, and a copy writes the
  spool file with each value replaced by `[redacted:<secret>]`, holding back across reads only a tail that
  could still become a value. A job without a grant writes its file itself, as before. A descendant that
  keeps the output open holds the report 200 ms at most. The completion counts what was withheld.

**How it is proven.**
- bv5: `versions::a_sigterm_or_a_sigint_stops_cleanly_and_the_next_start_replays_nothing`; live, a SIGTERM
  stopped a scratch daemon in about 42 ms, exit 0, socket gone, and the next start replayed nothing,
  against the baseline's exit 143, socket left, and 9 records replayed with a repaired index.
- pfv:
  - `outbox::a_clean_stop_lets_the_reply_in_flight_settle_before_it_exits` (with no grace it fails: the
    next start sends the post again) and `…_no_longer_than_its_grace`, plus core and store unit tests;
  - release A/B, quiet: no post in flight 27.3 ms p50 in both builds; a post never answered 63.5 ms p50,
    70.7 ms p95 (the grace's worst case), inside §9's 100 ms; the gate's clean shutdown 41.7 / 51.2 ms;
  - live on `#theseus-test`: with a 2 s grace the stop waited 236.6 ms and settled the post, and the next
    start sent nothing; with the default the post stayed dispatched and was sent again once.
- ez3: `restore::tests::a_restore_syncs_every_copy_and_directory_before_it_reports` (dropping one sync fails
  it); `strace` of the real binary: 6 `fsync`s against the baseline's 2; live, a restore of a copy of
  Eddie's WAL (1490 records) whose 5 sessions and their histories (105 nodes) equal the copy's, over the
  protocol. The syncs cost about 15 to 20 ms (release, quiet).
- l0d: `redact::tests` (every split of a value across two reads, every read size from 1 to 97 over a long
  run with two values, one the start of the other, against one pass over the whole); `job::tests` and two
  wrapper-process tests (a value split across writes, and a descendant printing it after the report);
  `broker::a_program_that_prints_its_granted_secret_leaves_it_nowhere` (the spool file while the job runs,
  the store, the log, and every surface). With nothing withheld all four job tests fail. Live, a scratch
  config granted Eddie's GitHub token to a stub that prints it split across two writes: 0 occurrences in
  the spool file while the job ran (its 85 bytes held the mark), and 0 in the WAL, the index, eight
  surfaces, the log, and the CLI's output after it, counted by value. The pipe costs a one-line job
  nothing measurable and a 32 MiB one about 40 ms; a job without a grant keeps its old path.
- The gate was green at each commit: 1054, 1059, 1060, and 1068 tests, with the lifecycle bench within
  its budgets.

**Divergence from the brief and the issues.**
- bv5: SIGINT's path, which the brief asked SIGTERM to copy, now also writes the stop's row and
  checkpoint; without them "the next open replays nothing" could not hold for either signal.
- pfv: the grace counts from the stop's start, not the serving loop's end, so it stays inside the
  budget; and no post is dispatched once the stop has begun (one planned then waits for the next start).
  The grace is a config key, so a test can lengthen it; the default is the issue's 50 ms.
- ez3: the staging store itself is synced too, once its open has written its manifest and index; the
  issue's crash-test idea cannot show a missing sync (a kill keeps the page cache), so the test records
  the syncs.
- l0d: the copy holds back only a tail that could still become a value, not always the longest value less
  one byte; a job without a grant keeps writing its file itself; values under 8 bytes are not withheld,
  as the scrubber does not scrub them.

**Known gaps.**
- ~~theseus-4xa (P2): the default 50 ms grace settles only a write near its end; real Discord takes 250 to
  400 ms; Eddie's call.~~ Eddie kept 50 ms (2026-10-01 14:38).
- ~~theseus-ndw: the lifecycle bench never has a post in flight, so the gate does not hold the grace.~~ Built in Item 46: the bench's `inflight` phase measures it, and its budget waits on Eddie (theseus-fsug).
- ~~theseus-p7q: `--stdio` has no signal arm.~~ Built in Item 54.
- theseus-rnx: the PTY path, when built, must withhold granted values the same way.
- ~~theseus-26r: rare 300 to 900 ms clean stops in debug, unquiet, not seen in the quiet release A/B.~~ Found in Item 46: the machine's writeback, in one of the stop's two waits on the disk.
- The installer's units keep `KillSignal=SIGINT`: no longer needed, harmless, right for an older binary.
- `theseusd restore` still needs 1Password access to start, though it reads no secret.

**Reviewed** (Tabitha, 2026-10-01, 07:31 to 07:36).
- **Reading the report against the code:** the signal arms registered once before the loop; `Core::stopping_on`
  and `finish_stop`; the grace counted from the stop's start; the restore's `Durable` trait and its order of
  syncs; the wrapper's pipe and `Redactor`, with jobs without a grant on their old path.
- **The gate rerun** at 5914183: 1,068 tests, lifecycle OK in 7.3 s.
- **A live check of the stop**, on the release build, over a copy of Eddie's store:
  - SIGTERM: exit 0 in 32 ms, socket gone, the next start replayed nothing and repaired nothing, and
    `server.stopping {"signal":"SIGTERM"}` was in the ledger;
  - SIGINT: the same, in 26 ms.
- **Eddie's store was copied** to `~/.theseus-backups/store-pre-fb2a-20261001-073547`, and the build **installed at
  07:35** from 5914183. No layout changed.
- **The grace's default** (theseus-4xa) is Eddie's call. Tabitha recommended keeping 50 ms and §9's budget, since a
  post the stop cuts off is sent again under its nonce and Discord returns the first one. Eddie kept 50 ms
  (2026-10-01 14:38: "50ms!").

### Item 20. Vectors, voice, and two small fixes on `main` (theseus-nz8, theseus-3xn, theseus-2fs, theseus-fln; 2026-10-01, merged 07:39 to 07:43; 648239e, 3a0a567, 128b3f6)

**Why.** The lane recipe's rule 3: each lane merged once reviewed. All three waited for fix batch 2's first step
(Item 19), which held `main`.

**What landed.**

| Lane | What | Its reader | Landed |
|---|---|---|---|
| vectors (29c) | Embeddings in `theseus-index` on candle 0.11 (f32, one thread): a hand-written WordPiece that matches Hugging Face's `tokenizers` id for id; weights hashed as they are read and pinned; vectors kept by text in a file per stamp, so a rebuilt index never re-embeds; the 256-d int8 flat scan and the 768-d re-score; rank fusion; `index.neighbours`, `index.embed`, `index.warm` | row 51 (the tender's wire-in); 31a | 07:39, 648239e |
| voice (44a) | `theseus-voice` on songbird 0.6.0: the `VoiceIo` seam with a WAV stand-in, the pipeline (utterances, coalescing, barge-in, sentence by sentence, the acknowledgment, reports at the pause), `Speech` and its stand-ins, the `join` example | 44b | 07:42, 3a0a567 |
| smallfix | theseus-2fs: the sandbox's clause 10 test waits for the sessions to differ; theseus-fln: the catalog generator finds its models from the `aws` on PATH, and the guard's tests read the embedded catalog | — | 07:43, 128b3f6 |

**The vectors lane's review** (05:01 to 05:10).
- **Spot checks.**
  - `RAYON_NUM_THREADS` and `CANDLE_NUM_THREADS` are set before any thread starts.
  - The pinned hashes match the fetch's manifest; the model's was also checked against Hugging Face's LFS hash.
  - Weights are refused before use when wrong.
- **Live, on a copy of Eddie's store:**
  - 536 MiB with the model loaded, 11 MiB once it unloads;
  - a load answers 0.53 s after `index.warm`;
  - a query embeds in p50 74 ms, p95 86 to 94 ms;
  - the scan over 100,000 chunks takes p95 7.0 ms.
- **The first quality evidence** (exam-v2's held-in items only).
  - Vectors find the paraphrase golds BM25 never reaches.
  - On the four hard families at k = 6: BM25 finds 1 of 16, vectors alone 7, and the equal-weight hybrid only 3.
    BM25's confident decoys outvote a vector-only find. Hence theseus-jz8 (P2): weighted fusion or a rerank, for
    30a and 32c, which also inherit a query embed past 30a's 60 ms target.
- **Filed:** theseus-64x (P3, a forgotten or redacted text's vector leaves the file promptly) and theseus-emc (P3,
  the exam probe's `--tender` mode).
- **The join** added one `deny.toml` ignore: RUSTSEC-2024-0436, `paste`. It is unmaintained, not vulnerable, and a
  build-time proc-macro that candle's gemm needs.

**The voice lane's review** (07:31 to 07:35).
- **The tests:** 28, on tokio's paused clock, so the timings are exact. An utterance closes at 1.700 s, a barge-in
  stops playback at 300 ms, and the acknowledgment comes at 2 s.
- **Size:** voice adds 5.0 MB, so `theseusd` would be about 22.4 MB of §9's 60 MB.
- **The SHA-3 question, checked at review against the downloaded sources:**
  - hpke-rs 0.6.1 calls SHAKE only in its X-Wing and ML-KEM key derivation (`kem.rs:141-162`);
  - openmls's RustCrypto provider maps only the DH KEMs (`provider.rs:54-65`; X-Wing is `unimplemented!`);
  - DAVE's one protocol version maps to `MLS_128_DHKEMP256_AES128GCM_SHA256_P256` (`davey session.rs:34-41`).

  So the three libcrux advisories are unreachable, a conclusion.
- **The join** added six `deny.toml` ignores, each with its reason: derivative, instant, the three libcrux ones,
  and ringbuf, whose ring holds only `u8`, for inputs voice never makes.
- **What 44b inherits:**
  - songbird plays only Opus as configured, so the lane added symphonia's PCM;
  - songbird receives no audio unless its manager is the crate's `manager()`.
- **The live join waits** for Eddie's private test voice channel.

**The smallfix lane's review.**
- 2fs: under 32 busy loops, the old test failed 27 of 50 runs; the fixed one passed 50 of 50, and 200 of 200
  beside 48. With `setsid` denied in the job's seccomp, it still fails at its deadline.
- fln: with no arguments, the generator rewrites `aws-catalog.bin` byte for byte. The guard's model tests pass on
  the embedded catalog with no AWS CLI at all, so they now run in CI.

**Gates.** Each merge was gated on `main` under the shared lock (`~/reports/theseus-merge/<lane>-gate.log`):
vectors 1,090 tests (lifecycle OK in 7.8 s), voice 1,118 (7.4 s), smallfix 1,118 (7.1 s), each passing on its first
run. No lane branch remains.

