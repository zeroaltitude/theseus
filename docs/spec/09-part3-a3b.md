# The Ship of Theseus, chapter 9: Part III, A3b, the build chain from the 2026-09-27 decisions ([index](README.md))
## A3b. The build chain from the 2026-09-27 decisions (theseus-5r9)

_P5c's items, in the order the owner approved on 2026-09-27, each recorded when its review ends. Step 1 is spec v0.37 (3877fe9). Steps 2a through 2a.3 were removed on 2026-09-28; they stay here as the record, and the reversal says what replaced them. The complexity cuts and Narration came after the reversal, on 2026-09-28 and 29, and close this section; M3.6's items are in A4._

### Step 2a. Consequences in the gate (theseus-770; 2026-09-27, 22:30–23:24; 03521f0, a218d8b, a3dcb64, 43faa91)

_Removed 2026-09-28 in dbd7567, except the `fs.patch` deletion fix; see the reversal below._

**What exists.**
- **Kinds.** Nine seed kinds (`theseus_tools::consequence::SEED_KINDS`), graded as §3.9 lists them, with `opaque` beside the three that need approval. `[consequences.kinds.<name>]` regrades a seed or adds a kind. Every key is optional (`irreversible`, `description`, `argv`), and the owner's pre-2a template loads unchanged (tested from a fixture copy).
- **Detection, layers 1 to 3.** A toollet declares consequences in its plan (`Plan.consequences`). Today every native toollet declares none:
  - the read-only toollets only read;
  - `fs.edit` keeps the replaced text in its arguments;
  - `fs.patch` now refuses a deletion that does not show every line it removes, so a patch always records what it took.

  For `proc.run`, `theseus_tools::shell` finds every command a call would start. It parses `bash`, `sh`, `dash`, `zsh`, and `ksh -c` strings (operators, pipes, subshells, `$(…)`, backticks, heredocs, redirections), sees through wrappers (`env`, `sudo`, `xargs`, `timeout`, `nohup`, `find -exec`, `ssh`, and more), and tracks every directory a command might run in. The rule table (`RULES_VERSION = 2026-09-27.1`, 33 rules) then names what each command does.

  Whatever the parser cannot see through is `opaque`: inline code, scripts, task runners, `eval`, remote commands, unknown git and gh subcommands, and a program word built from an expansion.
- **The regenerable rule.** It decides when a recursive delete is not `bulk_delete`:
  - inside a work tree, the target must be at or under a directory with a known build-output name (25 of them), which the repository's own ignore files ignore and under which `HEAD` tracks nothing;
  - outside any work tree, the target must be under `/tmp`;
  - a target the parser cannot resolve never qualifies.
- **Examples are the tests.** Every rule carries examples it must and must not match (162 and 125). Each example runs three ways (as shell text, under `bash -c`, and as a direct argv) in a fixture repository with ignored outputs, tracked work, and a force-added `build/`. A planted wrong example fails the test.
- **The irreversible column** (`ToolPolicy::decide`; the table is on `Enforcement`):
  - The floor is decided first, and nothing after it changes that answer.
  - An irreversible call waits for approval at every level.
  - A call that is irreversible and also against the policy gets the stricter treatment: refused under `strict` and `notify`, a marked wait under `ask` and `open`.
  - A needs-approval kind raises an allow-listed call: it waits under `strict` and `ask`, and runs with a notice naming the kind under `notify` and `open`.
- **The kinds appear wherever the decision does:**
  - `Decision` and `ConfirmRequest` (the new fields are optional, so old clients keep working);
  - the notice text ("irreversible: history_rewrite · needs approval: opaque");
  - the ledger rows `tool.confirm_requested`, `tool.notified`, and `tool.denied`;
  - the node's gate record, the tool span's attributes, and the counter `theseus.tool.consequences`;
  - Discord ("⛔ Irreversible. Approve?"), the web confirm card (an irreversible pill and a red border), and `theseus watch` and `theseus confirm`.
- **Replay.** `theseus policy replay [-n N]` (`policy.replay`, read-only) judges the newest N tool calls again with the current gate, and lists the ones it would treat or name differently. `theseus policy rules` (`policy.rules`) lists the kinds as graded, and every rule with its example counts.

**How it is proven.**
- **Tests.** 132; the gate passed at 43faa91, and again in review.
- **Seven scenarios** over the real store and kernel, with a local bare repository as `origin`:
  - Under `notify`, a force push parks, with `history_rewrite` named on the card, in the ledger row, and in the node. The remote does not move until the call is approved, and then the rewritten history lands.
  - A plain push runs with one notice.
  - `rm -rf somedir` parks, and `rm -rf target` runs.
  - A force push inside `bash -c` parks.
  - `open` still parks a force push.
  - `strict` changes nothing else.
- **A 28-spelling test:** quoting, wrappers, compound commands, substitutions, and `find -exec`; no spelling hides a force push.
- **Live, on a scratch daemon** (GLM 5.3 flash, `notify`):
  - The model's force push waited, and the bare remote stayed put.
  - A decline reached the model.
  - A feature-branch push and `rm -rf target` each ran with a notice.
- **In review, on a copy of the owner's store,** with the installed release binaries:
  - `policy rules` lists 9 kinds and 33 rules.
  - `policy replay -n 500` judged his 4 past tool calls and reported 2 changed. Both are his exit test's `bash -c 'sleep 45; echo done'`, which waited under the enforcement of the time and would now run with a notice under `notify`.
  - So replay compares against the whole current gate, settings included. The "should have asked" flow (2b) should hold the settings fixed and vary only the rule.

**Found in review** (2026-09-28, 00:05–00:14). A probe of the detector and the gate (not committed) confirmed that the plain spellings are caught, and these are not:
- **The floor**, a gap since M3b. It checks only the top-level program, and only the plan's resources.
  - Under both `notify` and `open`, `env op read …`, `bash -c 'op read …'`, and `bash -c 'theseusd config'` each run with an amber notice.
  - A floor path named as an argument or a redirection (`cat <store file>`, `cat < <store file>`) is judged only by the deny list, so under `open` it runs with a red notice.
  - The deny list is blind inside shell strings too (`bash -c 'sudo ls'`).
- **Detection.**
  - Brace expansion: `git push origin main --{force,}`, `rm -rf target/{,../src}`.
  - Words the parser cannot resolve: `F=--force; git push origin main $F`, `git push origin main -$(printf f)`, `bash -c 'git push origin main "$@"' _ -f`, `F=-rf; rm $F somedir`.
  - Unique-prefix long options, which git and GNU tools accept: `git push --mirro`, `git push --force-with-leas`, `git reset --har`, `rm --recursiv`.
  - A glob followed by `..`: `rm -rf target/*/../../src` resolves to `src`.

All of them are fixed in step 2a+ (theseus-xbg), before 2b.

**Divergence from Parts I and II.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| Detection rules are versioned data (§3.9) | Each row is a Rust matcher; its id, table version, and examples are data. The owner's `argv` prefixes are the data-only form | Shell semantics need code; the examples keep each matcher honest | Keep. A proposed rule (2b) lands as an owner prefix first |
| Owner-added kinds (§3.9) | They also take `argv` prefixes | A kind with no reader is inert (P0) | Part I §3.9 to say so |
| A needs-approval kind raises a call that needs confirmation | It also raises an allow-listed call | Allow lists match spelling; consequences are semantic | Held for the owner |
| Layer 2 lives in `proc.run`'s plan | It lives in the gate | The plan stays the toollet's own statement | Keep |
| `external_post` excludes Theseus's own places | Every loopback post counts | An over-approximation, until an exception names Theseus's own addresses | Held for the owner; with 2b or a later exception |
| An overwrite loses no work unnoticed | `fs.write` is not graded, and it keeps no before-image | It touches one file and is the everyday path; its class already stops it under `strict` and `ask` | Held for the owner: a before-image, or a grade when the file has uncommitted changes |
| P5c goes from 2a straight to 2b | Step 2a+ added between them | The holes above | Recorded here |
| The web confirm card is seen | Checked through the bundle and the RPC, not screenshotted | The OpenClaw browser tool refuses localhost | A headless-Chrome screenshot in 2a+ |

**Known gaps.**
- Argv detection cannot see:
  - git config (`remote.*.push = +…`, `remote.*.mirror`);
  - `proc.run`'s environment (`GIT_CONFIG_*`);
  - escape hatches inside text tools (`awk`'s `system()`, `sed`'s `e`);
  - what runs behind `eval "$(…)"`, a heredoc fed to a shell, or `ssh`, beyond naming the call `opaque`.
- A floor check over argv, however careful, stops only the spellings it can read. Code the gate cannot parse is `opaque`, and under `notify` it runs with a notice. Only M4's boundary closes this for certain: L1 gives a job no ambient credentials and keeps the token file out of its reach.

### Step 2a+. Gate hardening from the 2a review (theseus-xbg; 2026-09-28, 00:24–01:28; 0d0b769, 0093ec4, e57c48c)

_Removed 2026-09-28 in dbd7567; see the reversal below._

**What exists.**
- **The floor and the deny list judge every command a call starts** (`ToolPolicy::floor_over_commands`, `deny_over_commands`), over what `shell::commands` returns:
  - each command's words, and the wrappers it was reached through (`Cmd.via`);
  - its path arguments and redirection targets (`Cmd.redirs`, including fd forms), resolved against every directory it might run in;
  - those directories themselves.

  `$HOME`, `${HOME}`, and `~` resolve to the daemon's HOME for path judgments. The floor still runs first and is refused at every level; the deny list follows the ladder.
- **A floor mention scan over code the gate cannot parse** (`floor_mention`). It covers unparsed commands, dynamic programs, inline code, `eval`, and remote commands, and it looks for:
  - a floor path, in its canonical, `~/`, `$HOME/`, or `${HOME}/` form, or a floor file's name;
  - `theseusd` as a word;
  - `op` followed by one of its subcommands.

  The refusal says it came from a mention, so a model can rephrase an innocent call.
- **Detection reads every word, not just the program:**
  - brace expansion inside shell strings, capped at 64 results (past the cap, the word is dynamic);
  - `$1`…`$9`, `$@`, and `$*` substituted for `bash -c 'script' arg0 args…` when the args are literal;
  - an unquoted unresolved word counts as any option, and a dynamic positional as a dangerous operand;
  - unique-prefix long options count as triggers;
  - a glob followed by `..` is never regenerable.

  Triggers and exemptions use separate helpers (`opt_detect` and `opt_exact`). An exemption never accepts a variable or an abbreviation.

**How it is proven.** 138 tests; the gate passed at each commit and again in review.
- **Unit tests.**
  - Seven floor spellings are refused with `floor: true` at all four levels.
  - `bash -c 'sudo ls'` follows the ladder.
  - Mentions inside `python3 -c` are refused.
  - The `$HOME` forms reach a HOME-based floor path.
- **Scenarios** over the real store and kernel:
  - Under `notify`, `env theseusd --version` is refused, and the model is told why.
  - Under `notify`, `bash -c 'git push origin main --{force,}'` parks as `history_rewrite`, and the bare remote does not move.
  - Under `open`, `cat` of a store file is refused as floor.
- **FAST.** In release, `ToolPolicy::decide` takes 510 µs on a 2.9 KB `bash -c` string of 30 commands, and 9.8 µs on a plain argv.
- **Live, on a scratch daemon** (GLM 5.3 flash, `notify`):
  - `bash -c 'theseusd --version'` and `bash -c 'cat <state>/store/MANIFEST.json'` are refused as floor.
  - A brace-spelled force push parks, the ledger row carries the expanded command, and the bare remote stays put through a decline.
- **The web confirm card has its first screenshot** (`shots/2a-plus-web-confirm.png`): the `IRREVERSIBLE` pill, the red border, the kind line, the exact command, and Approve and Decline.
- **In review:** installed at e57c48c. On a copy of the owner's store, replay is unchanged (2 of 4, the same two calls).

**Found in review** (2026-09-28, 01:32–01:37). A second probe (not committed) confirmed that the floor spellings from the first review are refused and that all eleven detection misses are now caught. These still get through:
- **The floor:**
  - a program word built at run time: `x=op; $x read op://…`, `$(echo op) read op://…`;
  - inline code in its natural form: `python3 -c 'subprocess.run(["op","read","op://…"])'`, `perl -e 'system("op", "read", …)'`. The scan wants `op read`, with a space;
  - a relative program path inside a string, which keeps its directory: `bash -c './target/release/theseusd config'`;
  - globs through a floor path: `cat <state>/sta*/store/wal`.
- **Detection:**
  - A quoted variable is treated as never an option. Quoting stops word splitting, not option parsing, so `x=-rf; rm "$x" somedir`, `m=--hard; git reset "$m"`, `f=--force; git push "$f"`, and `x=-delete; find . "$x"` all get through.
  - `"$@"` with two or more literal arguments is joined into one word, so `bash -c 'rm "$@"' _ -rf somedir` gets through.
- **The record:** the rule table's version is still `2026-09-27.1`, although detection changed. Replay and the ledger therefore cannot tell old judgments from new ones.

All of these go to step 2a++ (theseus-0tv).

A correction to the first review's record: its probe expected `history_rewrite` for `git reset --har`, but that rule's kind is `bulk_delete`. The miss in 2a was still real (the rule matched only the exact option), and 2a+ catches it.

**Divergence from Parts I and II.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| The floor is refused at every level (§3.9) | True now for the command forms the gate can read: wrappers, shell strings, arguments, redirections, and working directories. Step 2a++ closes what the second probe found | Before 2a+, the floor saw only the top-level program and the plan's resources (a gap since M3b) | Keep |
| The floor judges what a proposal became | It also scans unparsable code for a mention of a floor path or program | The gate cannot see inside inline code; a mention is all it has | Keep; M4's boundary is the real close |
| The parser over-approximates the program word | It over-approximates every word a rule reads | §3.9's "over-approximates on purpose", applied consistently | Keep. Accepted friction: inside `bash -c`, `git push origin "$BRANCH"` and `git reset $SHA` now wait. Held for the owner |
| P5c goes from 2a+ to 2b | Step 2a++ added between them | The second probe | Recorded here |

**Known gaps.**
- A recursive read through an ancestor of a floor path (`find ~ -exec cat {} +`, `grep -r x ~`) is judged by the deny list, not by the floor. `~/.theseus` and the token file are on the deny list by default. A floor rule for ancestors would be far too broad, because `~` is an ancestor of everything.
- Deliberate obfuscation in inline code (string building, base64) stays `opaque`.

Only M4's boundary closes these.

### Step 2a++. Gate hardening, round 2 (theseus-0tv; 2026-09-28, 01:42–02:29; 1a58cfb, 995f830)

_Removed 2026-09-28 in dbd7567; see the reversal below._

**What exists.**
- **The floor, round 2:**
  - A `$name` program word whose assignments in the script are all literal resolves to each candidate value (the union across branches). Any other dynamic program word is scanned by its own spelling for `op` or `theseusd`.
  - Inline code is refused when it names a floor program as a string literal (`"op"`, `'theseusd'`, `"/usr/bin/op"`), when it contains `op://` anywhere, or when `op` is followed by a subcommand across separators (so `["op","read"]` counts).
  - Parsed commands never get this scan: `grep -r "op://" crates` stays allowed.
  - A program word is matched by its file name, so `./target/release/theseusd` reaches the floor.
  - A glob argument or redirection target is matched against each floor and deny path, component by component (`*` and `?` also match a leading dot). A pattern shorter than the path names an ancestor and is left to the deny list.
- **Detection, round 2:**
  - Any dynamic word before `--` may be an option, because quoting stops word splitting, not option parsing.
  - `rm.recursive` keeps one guard: when `-r` is only possible, not certain, it needs a separate operand. So `for f in *.log; do rm "$f"; done` stays quiet.
  - `find` treats a dynamic word in its expression as a possible `-delete`.
  - `$@` and `$*` expand with bash's word semantics.
  - `RULES_VERSION` is `2026-09-28.1`, and `policy rules` prints it.

**How it is proven.** 141 tests; the gate passed at each commit and again in review.
- **Unit tests** refuse every floor spelling from the second probe, with `floor: true` at all four levels, and catch every detection case. Precision cases stay quiet: `grep -r "op://"`, `ls ~/*`, the `rm "$f"` loop, and `find -name "$p"`.
- **A scenario:** under `notify`, `python3 -c 'import subprocess; subprocess.run(["theseusd","--version"])'` is refused, and the model is told why.
- **FAST:** 497 µs on the 2.9 KB `bash -c` string, and 7.9 µs on a plain argv (release).
- **Live, on a scratch daemon:** `x=theseusd; $x --version` and the inline-code call are both refused as floor. `x=-rf; rm "$x" somedir` parks as `bulk_delete`, and the files survive a decline.
- **In review:** installed at 995f830. On a copy of the owner's store, `policy rules` prints `2026-09-28.1`, and replay is unchanged.

**Found in review** (2026-09-28, 02:49–02:53). A third probe found every floor and detection case from the first two probes handled, and no over-refusals among the allowed cases. These still get through:
- **The floor.** A variable used as the program after a wrapper or a re-parse: `x=op; command $x`, `exec "$x"`, `env $x`, `timeout 5 $x`, `eval "$x whoami"`, `bash -c "$x whoami"`. The literal-assignment resolution applies only to the first word of the outer command.
- **Detection:**
  - `set -- -rf somedir; rm "$@"`: `set --` redefines the positionals;
  - `rm "${a[@]}"`: a quoted array still expands to several words;
  - `x="-rf somedir"; rm $x`: an unquoted variable can carry both the option and the operand;
  - `rm -r$(printf f) somedir`: the literal `-r` prefix of a dynamic word is ignored;
  - `x=push; git $x -f`: a dynamic git subcommand is only `opaque`.

Most share one cause. So step 2a.3 (theseus-905) resolves every simple `$name` from the script's literal assignments, everywhere. That also makes honest scripts exact: `BRANCH=main; git push origin "$BRANCH"` resolves to a plain push, and stops waiting.

**Divergence from Parts I and II.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| The floor is refused at every level (§3.9) | True now for run-time program words, natural inline code, relative program paths, and globs. Step 2a.3 closes variables that reach the program through wrappers and re-parses | The second and third probes | Keep |
| Over-approximation reads every word (2a+) | A quoted dynamic word may be an option too, and `$@` splits as bash splits it | Quoting stops word splitting, not option parsing | Keep. Accepted friction: `git checkout "$branch"` and `git restore "$x"` wait, since a `-f` needs no operand. Held for the owner |
| P5c goes from 2a++ to 2b | Step 2a.3 is added, as the last argv-layer round | The third probe | Recorded here. After 2a.3, the remaining long tail goes to M4 |

**Known gaps.** The following stay `opaque`, and under `notify` they run with a notice:
- string building (`x=o; y=p; $x$y`);
- obfuscated inline code (base64, `chr()`, implicit concatenation);
- a script that is written and then run.

Honest calls are caught by their natural spellings. Against a model that is working to get around the gate, the argv layer is a speed bump, not a wall. The protection for sessions that read untrusted content is M4's exposure rule (one step stricter, so `notify` becomes `ask`) and its boundary. Until they land, such sessions should run under `ask`.

### Step 2a.3. Gate hardening, round 3, the last argv-layer round (theseus-905; 2026-09-28, 02:57–03:46; dc3ed99)

_Removed 2026-09-28 in dbd7567; see the reversal below._

**What exists.**
- **Every simple variable reference resolves from the script's literal assignments,** not only the program word. Resolution reaches:
  - words behind wrappers (`command`, `exec`, `env`, `timeout`, …);
  - the text `eval` runs, a nested `bash -c "…"`, and `$(…)`, backtick, and subshell bodies, which inherit the enclosing scope;
  - arrays (`a=(…)`, `"${a[@]}"` word by word);
  - `for` lists, and `set --` positionals (`shift` and function bodies make the positionals unknown);
  - embedded references (`$x$y`, `--$flag`), as a product of candidates capped at 64.

  The resolved value follows bash's rules. Unquoted, it is split; with a glob character, it gets the glob flag; if the script assigns `IFS`, it stays dynamic.
- **A dynamic word's literal option prefix is certain,** so `-r$(printf f)` counts as `-r`.
- **An unresolvable git subcommand** (`git $x -f`) is judged as each subcommand a git rule reads.
- **Notices and refusals show the resolved command and where each value came from:** ``rm -rf somedir (`$DIR` = `somedir`)``.
- `RULES_VERSION` is `2026-09-28.2`.

**How it is proven.**
- **The run did not finish.** OpenClaw's stuck-session watchdog aborted the subagent at 03:46:25 (openclaw-jygw). The gateway had stopped seeing its tool events at an `Edit` that started about 03:31, although the agent kept working: it made commit dc3ed99 at 03:37. So its report, live check, and FAST and friction numbers were never written. This record comes from the commit and from Tabitha/Claude's review.
- **Tests.** 147; the gate passed at dc3ed99 and again in review.
- **Review probe** (release, finished 08:25:46):
  - All 23 detection cases from all three rounds are caught. They include `set -- -rf somedir; rm "$@"`, `rm "${a[@]}"`, ``x="-rf somedir"; rm $x``, `rm -r$(printf f) somedir`, `git $x -f`, `y=$x` chains for options, function arguments, and `shift`.
  - All 8 precision cases stay quiet. Among them is the friction round 2 added, which is now removed: `BRANCH=main; git push origin "$BRANCH"`, `SHA=abc123; git reset $SHA`, `DIR=target; rm -rf "$DIR"`, and `branch=feature; git checkout "$branch"`.
  - A wait remains only when a variable comes from outside the script: `git checkout "$branch"` and `git push origin "$BRANCH"`, both unassigned.
  - The floor refuses 20 of 23 spellings. Among the refused are every earlier case, the wrapper and re-parse cases, `$x$y` built from literals, `$'op'`, `"o""p"`, `${x:-op}`, and a function calling `op`.
- **FAST** (release): 488 µs for a 1.7 KB script with 20 assignments and 30 references with substitutions, and 8.4 µs for a plain argv.
- **Live, in review** (release build of dc3ed99; scratch daemon; GLM 5.3 flash; `notify`):
  - `x=theseusd; command $x --version` is refused as floor, with the reason ``theseusd --version (`$x` = `theseusd`) is never run … This is final for this request; tell the operator rather than trying another way around it.``
  - `BRANCH=main; git push origin "$BRANCH"` runs with an ordinary notice (`tool.notified`, `approval_skipped`, no kinds), and the bare remote moves from e06a233 to a41ba3e.
  - `DIR=somedir; rm -rf "$DIR"` parks as `bulk_delete`, with the detail ``rm -rf somedir (`$DIR` = `somedir`)``. After a decline, the model says it won't route around the refusal, and the files remain.
- **Installed** at 08:32 from dc3ed99.

**Divergence from Parts I and II.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| The subagent writes the report and runs the live check | The run was aborted; Tabitha/Claude did the review probe, live check, and FAST numbers | openclaw-jygw (the gateway lost the tool stream; a single agent, no fan-out) | Recorded here; the evidence is on openclaw-jygw |
| Over-approximation on every word a rule reads (2a+, 2a++) | Literal values resolve exactly first; over-approximation is left for what stays unknown | Precision: honest scripts with variables no longer wait | Keep |
| P5c goes from 2a to 2b | 2a+, 2a++, and 2a.3 came between them | Three review probes | Recorded here. After 2a.3, the argv-layer long tail goes to Jev `security.v1` (M5, or earlier in shadow) and M4's boundary |

**Known gaps** (theseus-1eh, deferred; the owner, 2026-09-28: "I don't particularly mind the possibility that strange content could be hijacked in as e.g. base64 … it would have to be unwrapped then run"):
- The floor misses a program reached through a variable copied from another (`x=op; y=$x; $y`), and values set by `printf -v` or `read`.
- Obfuscated inline code and scripts that are written and then run stay `opaque`. Beyond the gate, the defenses are model judgment, the notices, Jev `security.v1` (tighten-only), and M4's boundary.
- Cosmetic: nested backticks in a resolved reason break markdown rendering.

### The reversal (theseus-8az, 2026-09-28)

**What had been built.** Steps 2a through 2a.3 (theseus-770, theseus-xbg, theseus-0tv, theseus-905) ran from 22:30 on 2026-09-27 to their install at 08:32 on 2026-09-28. They taught the gate to name what a call would do to the world:
- nine consequence kinds under one `irreversible` property, and the irreversible column on the `strict | ask | notify | open` ladder;
- a shell parser that found every command a `proc.run` call would start, and a versioned rule table over argv, with examples as its tests;
- `opaque` for whatever the parser could not read, and a floor that also scanned code it could not parse;
- `theseus policy replay` and `theseus policy rules`.

Each review probe found spellings that still got through, and each round of hardening closed them and made the parser larger. By 2a.3 the two detection files, `shell.rs` (2,499 lines) and `consequence.rs` (2,568), were the largest in the codebase, and `policy.rs` had grown from 667 lines to 1,978.

**Why it was removed.** The owner reversed the direction on the morning of 2026-09-28. Detection written by hand cannot be finished: "attempts to find embedded commands by hand -- that can **never** work" (emphasis his). The operator should be "asked, sure, but a hard no, almost never", because "Theseus should notify over block -- the operator should /know/ when something bad is going to happen", and because "we were responsible for running theseus on a default-safe environment." He added a general rule the same morning: "we should *ruthlessly remove complexity*" (emphasis his). The order he approved was a read-only scan of the vault's item titles and categories first, to anchor what "risky" means (its findings stay out of this document), then the teardown, then the replacement.

**What replaced it.** The commits, each behind the gate, signed, and pushed:
- **The teardown, dbd7567** (reviewed and installed 10:38). `shell.rs`, `consequence.rs`, their scenario tests, and a template fixture were deleted: 5,911 lines. Every other file the chain had touched went back to 3877fe9 byte for byte, except the `fs.patch` fix that refuses a deletion which does not show every line it removes. Rust went from 35,151 lines to 27,489, tests from 147 to 114, and `policy.rs` from 1,978 lines back to 667, before the ladder was added.
- **The ladder, 09bad8b and 78b7fd9** (reviewed and installed 11:31). One posture that every tool and MCP inherits, with `[policy.tools]` and `[policy.mcp]` overrides. The template lists every tool, and a test binds the list to the registry. The floor asks at every posture and sees path arguments. The posture shows in `theseus tools`, in history, and on every surface.
- **No deny anywhere, 2f4a285, 3c9d0c3, and a046f53** (the owner's decisions at 11:36; reviewed and installed 13:26).
  - `deny` left the postures and the kernel's band, so the gate cannot express a refusal.
  - The deny lists became approve lists, and a path outside the roots waits.
  - The floor keeps the token file whether it came from the flag or the environment.
  - `op` and `theseusd` left the default approve list, since the floor covers them.
  - A declined call reads "not run".
  - The config is parsed once, unless an old key name appears.
- **The stored vocabulary, e49a363** (the owner's approval at 16:00; reviewed and installed 16:21). A decline is stored as `declined`, the ledger kind is `action.declined`, and the resolution reads "declined by". Rows stored under the old names still decode and render. **Rolling back past e49a363 is unsafe once a decline has been stored this way**, because an older binary has no `declined` value. Upgrades from here are forward only.
- **The catalog, 274facd** (theseus-px4; not part of the gate, landed the same evening; installed). Claude Sonnet 5.5 joined the catalog, version 2026-09-28.1, and became the default model: the built-in default, and the template's `[model]` and `[profiles.sonnet]`.

After all of it, Rust is 28,523 lines, `policy.rs` is 899, and there are 135 tests. `decide` parses no shell. It takes about 4.5 µs per call in a debug build; under 2a.3 it took 8.4 µs on a plain argv and 488 µs on a 1.7 KB script, in release.

**How it is proven.**
- The gate passed at every commit and again in each review: 114 tests after the teardown, 126 after the ladder, 133 after no deny, and 135 after the vocabulary.
- Live, on scratch daemons:
  - a `proc.run` under `notify` ran with a notice, and `"proc.run" = "approve"` made the same call wait;
  - under `open`, the floor made three calls wait, each marked as the floor: a read of the file named as the token file (a decoy), a listing of the store, and `theseusd --version`;
  - a read outside the roots waited, and ran once approved;
  - `sudo -n true` waited on the approve list, and its decline read "not run".
- The owner's vault config loaded under the teardown, ladder, and no-deny binaries, in read-only checks. Until its old key names are renamed, every start logs one warning per old key.
- On a copy of the owner's store, rows written by the chain builds decode under the restored types. An old "denied" decline renders as "not run", and `theseus ledger --kind action.declined` returns the old row.

**Divergence from Parts I and II.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| IRREVERSIBLE WAITS (§2); the consequence kinds, rule table, `opaque`, replay, and "should have asked" flow (§3.9; P5c item 1, steps 2a to 2a.3) | Built, hardened three times, then removed in dbd7567. Only the `fs.patch` deletion fix was kept | Detection of hidden commands can never be complete. Each review found spellings that got through, and the cost was the two largest files in the tree | Part I §2 and §3.9 marked superseded; §3.9 rewritten to the gate as built |
| theseus-8az as first stated: one Jev risk classifier (0–100) per call, feeding the ladder | Not built. The gate is a posture ladder over the finite tool and MCP lists | The owner's direction at the ladder step: the tool and MCP lists are the surface the operator controls | Jev stays M5 design, stricter only (§3.9, Jev). When is held for the owner |
| The ladder `strict \| ask \| notify \| open` (§3.9, v0.37) | Postures `open \| notify \| approve`, inherited by every tool and MCP, with `[policy.tools]` and `[policy.mcp]` overrides. The first cut (09bad8b) also had `deny`, and 2f4a285 removed it | Notify over block: the operator is asked, and almost never told no | Kept |
| The floor refused at every level (§3.9) | The floor waits for approval at every posture and says it is the floor. It sees path arguments, and it keeps the token file however the daemon was given it | The owner's floor decision (09:30): never a hard block, never a silent notice | Kept |
| `deny_argv`, `deny_paths`, and a path outside the roots refused (A3) | `approve_argv` and `approve_paths`: a match waits, and so does a path outside the roots. The old key names load with a startup warning | No deny anywhere | Keep the aliases until the vault config is renamed |
| The class modes `read`, `write`, `run` (A3) | Retired: the old template's values load with a warning and are ignored; any other value fails to load | The owner's live config still had them | Remove when his config is updated |
| A decline stored as `denied` (A3) | Stored as `declined` (`action.declined`, "declined by"); rows stored under the old names still decode and render | "deny" nowhere, for consistency (the owner, 16:00) | Forward only: rolling back past e49a363 is unsafe once a decline is stored |
| A request the owner is not permitted to make is blocked at the gate with a clear message (P5, the M3 prove) | It waits for approval with its reason, and a decline means it never runs | No deny anywhere | Recorded here; P5 is left as written |
| An owner override for any refusal, and the floor's typed ceremony (§1, §3.9; step 2c, theseus-qc4) | Nothing in the gate refuses, and the owner can approve whatever waits | Follows from no deny | The floor ceremony is moot. theseus-qc4 is held for the owner |
| The exposure rule, one level stricter on the old ladder (§3.9, M4) | Restated on the three postures: `open` behaves as `notify`, and `notify` as `approve` | The ladder changed | Part I §3.9 updated; built in M4 |
| The consequence boundary under L1 (§7, P6) | There is no consequence left for it to name | The kinds are gone | Held for the owner, with M4 |

**Open, held for the owner.**
- theseus-qc4 (the owner override): close it, or keep it for a later layer that refuses.
- Appendix F's M4 item "content refused by label class wherever it leaves" is a refusal. Under notify over block it may become a wait.
- §7's boundary: should a credential request, or a recognized request shape, notify or wait?
- The Jev risk classifier: when, and whether.
- Under `approve`, the allow list still runs `ls` and `pwd` inside the roots (the ladder step's Q2, unchanged).
- A hook may still block a call (§3.17; P5c item 2). A hook is not the gate, so it was left as designed. _(Resolved 2026-09-28: the hook system was deleted; see the complexity cuts, below.)_

### The complexity cuts (theseus-hco; 2026-09-28, 19:16–23:53; 95ee929, bb3ce56, b242258, 11d2f43, 8a2b503, 54b0918)

**Why.** The owner set the rule on the morning of 2026-09-28: "ruthlessly remove complexity … If we reach a supremely exquisitely simple codebase to reach 80% security, that's so much better than a complex codebase that reaches more at the price of brittle patterns, brittle code." With the gate tower gone (the reversal, above), a read-only review looked for what else the tree carried that it did not need.

**The review** (19:16–19:48, at 8a1e41d; nothing changed). 21 ranked findings in 28,523 lines of Rust: 22,803 production and 5,720 test. Four things dominated:
- speed traps that grow with history: `session.list` re-decoded every action once per session, 2.87 s at 400 sessions on a debug build, polled every 2.5 s by the Observatory, and startup decoded every node in the store to seed a counter;
- 27 fsyncs for one plain turn, 10 of them `hook.site` rows;
- machinery with no production caller: a second kernel session model, the kernel's `Policy` trait wired through an adapter, and a simulator that ran in no test and no gate;
- giant functions: `TurnRunner::run_inner` at 785 lines, the CLI's `run()` at 652, and the RPC `dispatch` at 496.

In all, it proposed cutting about 2,600–3,200 production lines and 39 of 325 external crates, and bringing a plain turn from 27 fsyncs to about 8. The owner approved its five do-first cuts at 20:38: "do all of your suggestions, including 2, and add that feature". At the same time he asked for Narration: "Let's remove the hooks. But let's add a new feature: Narration." Findings 3 and 8 went to M3.5 (P5b), and findings 10 to 16 and 18 to 21 are batch C (theseus-0g4).

**What each cut did.** Each commit went through the gate, signed and pushed, and each was reviewed and installed.
1. **Findings 1a and 7: the quadratic and the start-path scan** (95ee929; batch A). `session.list` scans the open actions once, not once per session. Startup no longer decodes every node to seed the tool-call counter, and the GitHub token check runs after the socket binds.
   - `session.list` at 400 sessions went from 3,130 ms to 45 ms, and it now grows linearly (debug build, tmpfs, fake provider).
   - Cold start to the first `health` went from 1,300–1,397 ms to 1,096–1,181 ms on an empty store, and from 1,521–1,655 ms to 1,281–1,331 ms on a 2,000-session store. About 1 s of what remains is secrets through `op` (P5b).
   - `tool.list` now counts calls since the daemon started.
2. **Findings 5 and 9: the rest of the gate teardown** (bb3ce56; batch A). The kernel keeps only `Proposal`, `digest_proposal`, and a new `digest_json`. The `Policy` trait, `run_gate`, `GateTrace`, `GatePolicy`, and `Mode` are gone: the toollet plans a call, then `ToolPolicy::decide` decides it. One serializer replaces the two canonical-JSON copies, and golden-digest tests prove every digest byte-identical. Production lines fell by 248, and `gate.rs` went from 151 lines to 36.
3. **Findings 6 and 17: dead kernel API, and the simulator in the gate** (b242258; batch A, finished in review). The kernel's second session model (`Session`, `open_session`, `promote`, `session_tail`) and `KernelError::PolicyDenied` are gone. The crash test and the kernel simulation run in the gate, the simulation on the product's own `open_execution` path. The crash test's `--tear` defaults off (theseus-4x6). On a copy of the owner's store, the first `health` took 1,132 ms and `session.list` 3 ms.
4. **Finding 2: the hook system** (11d2f43). Deleted: `hooks.rs` (25 events, 4 kinds, the registry, and the outcomes), its 11 call sites in the turn and three in the RPC server, `hooks.*`, `theseus hooks`, and the `hook_spans` mode.
   - A plain one-loop turn went from 27 frames and fsyncs to 17, and a two-loop turn with one `fs.read` from 45 to 29.
   - The median of 20 plain turns went from 211–224 ms to 134–142 ms (debug build, before and after interleaved).
   - Production Rust went from 22,177 lines to 21,618, 559 fewer, and tests from 140 to 138.
   - A new test fails the gate if a plain turn writes more than 17 frames.
   - The owner's config loads, with one warning for the retired `[telemetry] hook_spans`. Old `hook.site` rows and hook spans still decode and render, and `hooks.list` answers "method not found".
5. **Finding 4: the lost cost, then the split of `run_inner`** (8a2b503, 54b0918).
   - **The failing test came first.** A turn fails in its second loop, once on a provider error and once on the budget. On the old code it found 18 lost facts. After a provider failure, the session and health lost the finished loop's cost ($0.30 in the test) and its tool call. After a budget failure, the session recorded nothing. Neither exit put cost or tool calls in `turn.failed` or in the error.
   - **The fix** (8a2b503). One function closes the books on success and on both failure exits. `turn.failed` gains `cost_usd` and `tool_calls` beside its `usage_so_far`, and the error's data gains `usage`, `cost_usd`, and `tool_calls`. A turn that fails on the budget now counts in the session's turns, as a provider failure always did.
   - **The split** (54b0918). `run_inner` became 11 named steps, with one `fail()` for both failure exits. No function in `turn.rs` is over 113 lines, where one was 676, and clippy at the review's strict thresholds finds no cognitive-complexity or nesting warning in the file.
   - **Behaviour is identical.** An output-diff probe over 8 scenarios dumped every frame, ledger row, notification, session total, result, and trace, on the fix and on the split. Each dump was 37,791 lines, and after masking ids, digests, and times the diff was empty.

**How it is proven.**
- The gate passed at every commit, and again in each review: 136 tests at 95ee929, 138 at bb3ce56, 140 at b242258, 138 at 11d2f43, and 139 at 8a2b503 and at 54b0918.
- Live checks on scratch daemons:
  - batch A, on a copy of the owner's store, with a real Sonnet 5.5 turn;
  - cut 2, with rows the installed daemon wrote read by the new binaries, and again on a copy of the owner's store;
  - cut 5, on a copy of the owner's store: a GLM `fs.list` turn ran 2 loops and 1 tool for $0.0008, and the session's books matched the ledger.

  The owner's vault config loaded under each new binary.
- Installed: batch A at 21:54, cut 2 at 23:04, and cut 5 at 23:52.
- Two runs since v0.42 were aborted by OpenClaw's stuck-session watchdog (openclaw-jygw) and finished in review. One is batch A, at 21:25:56 during its item 3, with 12 files edited and nothing committed. The other is the budget step (A4, item 1).

**Divergence from Parts I and II.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| M0's hook registry: every event typed, handlers registered in code and over the protocol, and the run-hooks path at each site (P2; §3.17 as it stood). P0 rule 3's hook half: every hook event lands with a reader, and the registry test enumerates them | Deleted in 11d2f43, with `hooks.*`, `theseus hooks`, and the `hook_spans` mode | No handler could do anything: `Blocked` and `Claimed` were never built, and 10 of the 25 events were never dispatched. A plain turn still paid 10 of its 27 frames for it. The owner, 20:38: "Let's remove the hooks." | Part I §3.17 rewritten, and its other mentions amended. Part II is left as written, including P6's hook points and M5's hooks bullet (P7). Rule 3's edge and label half stands (theseus-wjy) |
| Wire the seven unwired sites, make three honor their results, and prove a blocking handler per gating point (P5c item 2; A3's hook-site row; theseus-0dp) | Moot: no sites remain | Follows from the deletion | theseus-0dp closed as superseded |
| One small hook point to replace the system (theseus-bdn) | Deferred | "Add that feature" meant Narration; it was first read as the hook point and corrected at 23:44. The hook point was Tabitha/Claude's suggestion in the review, to bring one back "when a real handler exists", and none does | Deferred until a real handler exists (§3.17) |
| Cut 5 saves about 130 lines (the review) | `turn.rs` grew by 113 lines, from 1,134 to 1,247 | The estimate assumed one payload per fact for the trace, the ledger, and the notification. Those payloads differ field by field, and the cut had to keep behaviour identical. Each of the 11 steps also pays a signature and a doc comment | Kept for its shape |
| `tool.list` keeps durable per-tool counts, seeded at startup (A3) | Counts since the daemon started, so the shell-fallback ratio (§3.23) covers the current run | The seed decoded every node in the store on the start path (finding 7) | Lifetime counts can return cheaply with an index (finding 1b, M3.5) |

**Known gaps.**
- A fault after a paid loop still skips the books. A raw error inside the loop, such as a cancel between a provider settle and a tool's plan, is not a classified failure, so the session misses the loops that already ran.
- OpenTelemetry's `record_failure` takes no usage or cost, so the OTel counters miss a failed turn's finished loops. The owner's config has telemetry off.
- The failure path ignores the result of its session write (`let _ = put_session`).
- The frame-budget test asserts at most 17 frames. It is tightened to the new count when batching lands (P5b).
- The crash test's `--tear` stays off until theseus-4x6 bounds it.

#### Batch C, part 1: the low-risk deletions (theseus-0g4; 2026-09-29, 04:22–05:11; cf258c7, ea06ff8, b407dd3, f8a68c2, 605979e)

**Why.** The review's remaining findings, queued before new features so they land on simpler
code. Batch C runs in three parts: this one takes the five low-risk deletions (findings 21, 15,
16, 20, and 14), part 2 takes findings 10, 13, and 18, and findings 11, 12, and 19 wait until after
the Daily Driver (M3.6).

**What each cut did.** One gated, signed commit each, in this order.
1. **Finding 21: dead code and lint cleanups** (cf258c7). Eight items no code called went, with
   `kinds::CHECKPOINT` (255) and the protocol's `BLOCKED` (-32001) kept as reserved numbers; two
   test-only constructors are `#[cfg(test)]`; 24 of the review's 25 lint sites were fixed (the 25th
   is `Core::new`, finding 10's); and a workspace `[lints]` table holds `unused_async`,
   `unused_self`, and `redundant_clone` from now on.
2. **Finding 15: one index engine** (ea06ff8). fjall, the `Index` trait, the engine parameter, the
   simulator's `--engine` flags, and the format-1 move-aside are gone, and the manifest is read
   once. `Engine` has one variant, so the manifest and `[server] store_engine` still name it and
   refuse anything else. The owner's store is format 2, redb, so it needed no migration.
3. **Finding 16: config shims** (b407dd3). The theseus-8az renames (`deny_paths`, `deny_argv`), the
   retired policy class keys, `telemetry.hook_spans`, and the `max_tokens` alias fail to load as
   unknown keys; the retired `[kernel] default_budget` and `control_reserve` still load with their
   one warning, because the owner's note sets them. `effort`, `thinking_display`, and a provider's
   `kind` are serde enums, and the implicit profile and provider are resolved once at load.
4. **Finding 20: policy leftovers** (f8a68c2). The allow list's flag blacklist and its near-miss
   text are gone. An allow entry is a prefix, documented, and still runs only when every path
   argument is inside the roots. `[policy.mcp]` and its overrides stay (the owner, 2026-09-28).
5. **Finding 14: OpenTelemetry behind `otel`** (605979e). The exporter builds only with the cargo
   feature, off by default; `[telemetry]` parses the same, and a set endpoint warns once. The gate
   lints the feature (about 1.5 s).

| measure | before (48fd2c8) | after (605979e) |
|---|---:|---:|
| crates in `theseusd`'s default build | 331 | 292 |
| external crates, workspace | 325 | 286 |
| `theseusd` release binary | 24,281,704 B | 19,021,104 B |
| clean release build, 8 jobs | 253 s | 218 s |
| production lines, total / compiled by default | 25,815 / 25,815 | 25,474 / 25,013 |
| gate tests | 204 | 198 |

**How it is proven.** The gate passed at every commit (204, 204, 202, 202, 202, and 198 tests).
The owner's vault note loaded with the same single warning under the binaries after findings 16, 20,
and 14 (`theseusd check` on its `op://` reference), and a copy of it with Discord and the web UI
off did under those after 21 and 15; after 16, `theseusd config` printed the same bytes. On copies of the owner's store (only `store/`; Discord and the web UI
off; never port 7433), the new binaries opened it, served it, and printed byte-identical
`theseus sessions` and `theseus history`. The final check ran a GLM `fs.list` turn: 2 loops, 1
tool call, $0.0013, with its `turn.trace` row in the ledger.

**Divergence from Parts I and II.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| Two `Store` implementations, `redb` and `fjall`, behind a trait, fjall selectable by config (M1; §1, §6) | redb only; the trait and fjall are gone | M1 chose redb, and a store never switches engines | §1 and §6 revised (drafts in the run's report) |
| OpenTelemetry on by default, always compiled in, a config change to enable (§3.20; theseus-vng) | Compiled only with the `otel` feature, off by default | 19 crates, aws-lc among them, for an export no deployment enabled | §3.20 revised (draft) |
| Old config spellings load with a warning (theseus-8az, theseus-hco) | They fail to load as unknown keys, except the two the owner's note still sets | No deployment uses them | The owner's note loads unchanged |
| The allow list never covers nine git and curl flags (theseus-8az, 09bad8b) | Removed; the roots rule stays | It only second-guessed entries the operator added | Allow entries documented as prefixes |

- **Review** (05:22). The gate reran at 198 tests. Tabitha/Claude's own check on the release build, over a store copy, listed the owner's 5 sessions and ran a GLM `fs.glob` turn (2 loops, 1 tool call, $0.0005), with no error in the log. The owner's `allow_argv` is `[["ls"], ["pwd"]]`, so dropping the flag blacklist changes nothing for his config. Installed at 05:22.

**Known gaps.**
- ~~The otel exporter's tests do not run in the gate, which only lints the feature.~~
- ~~The default test build still compiles `opentelemetry` and `opentelemetry_sdk`, a dev-dependency
  cargo cannot gate on a feature.~~
- Both moot since Step O1 (A3c), which removed the feature and the SDK: the exporter is in every build. Found
  by the v1.1 roadmap (Item 45).
- `Core::new` still takes the resolved secrets by value, and the functions over six arguments stay
  for part 2 (findings 10 and 18) and after the Daily Driver (finding 19).

#### Batch C, part 2: the structural cuts (theseus-0g4; 2026-09-29, 05:23–06:21; 750ec12, 6e76a5f, 69851ad)

**Why.** These three findings shape the code the next Daily Driver items touch: durable delivery
goes through the tool runtime and `Core`, and task sessions go through `Core` and the kernel.
The owner's rule is "ruthlessly remove complexity", and behaviour stays identical.

**What each cut did.** One gated, signed commit each, in this order.
1. **Finding 13: pending confirms, one way** (750ec12).
   - Before, "what waits for the operator" was computed four ways: a node scan, an open-action
     scan, the CLI's fan-out, and the transcript re-read that `confirm_action` and `resume` each
     did to find the proposal.
   - Now `Action::awaits_confirm` and `Kernel::pending_confirms` are the one derivation. The
     history, the session list, a new `confirm.list` (so `theseus confirm` is one request), the
     web UI, and resume all use it.
   - An action that waits for the operator keeps its `Proposal`. That is a tool call the policy
     stopped (`plan_confirm_with`) or a budget question. Other actions keep none, so the
     transcript holds the arguments once.
   - An action stored before this decodes without a proposal and still waits. Its proposal comes
     from its node's gate record, in one place (`confirm_proposal`).
   - The simulator checks that a question carries its proposal.
2. **Finding 18: `toolrun.rs`** (6e76a5f).
   - A `ResultNode` value replaces the 11-argument `result_node` and its `#[allow]`.
   - `process` is a list of named gate steps, with one `plan_call` for both postures.
   - `execute` splits into `run_inproc` and `run_job`.
   - `resume` places each unanswered call (`Pending`), then acts in one flat match: 261 lines
     became 61.
3. **Finding 10: `rpc.rs`** (69851ad).
   - `Core`'s six jobs have a file each under `rpc/`.
   - `dispatch` is a 49-line table of named methods, where it was 462 lines. `route` parses,
     runs, and serializes, and `?` is `INTERNAL`.
   - One constructor path: `Core::new(cfg, secrets, store)` still takes the secrets by value,
     so the daemon keeps no copy, and builds `Parts` for `Core::build`.
   - `BindingBoard` counts starting bindings with a watch channel instead of a sleep loop.
   - `health` reads the sessions once, the `turns` counter is gone, and `Config::profile` owns
     the unknown-profile message.

| measure | before (605979e) | after (69851ad) |
|---|---:|---:|
| production lines (not cutting at a test-module declaration) | 25,487 | 25,930 |
| longest function in the rewritten files | 457 (`dispatch`) | 141 (`Core::build`) |
| functions over 60 lines there, and their total length | 9, 1,583 | 10, 848 |
| functions over 6 arguments there | 5 (2 behind `#[allow]`) | 2 (the kernel's planning pair) |
| gate tests | 198 | 201 |
| frames for a plain turn | 17 | 17 |

**How it is proven.**
- The gate passed at every commit.
- An output-diff probe dumped every WAL record, frame count, notification, narrative line, and
  result for 26 scenarios. These include confirm approved, declined, superseded, answered after a
  restart, pending across the upgrade, and a budget question approved and declined. Its masked
  diff was empty between runs, empty across findings 18 and 10, and across finding 13 showed only
  the proposal on the 56 records of actions that waited, and the new method.
- On copies of the owner's store (only `store/`; Discord and the web UI off; never port 7433), the
  old and new binaries printed byte-identical sessions, histories, and confirm lists.
- A GLM turn's `fs.write` outside the roots waited, was listed by `theseus confirm`, was
  approved, and resumed and wrote the file.
- **Review** (06:48–06:51). The gate reran at 201 tests. Tabitha/Claude's own check on the release build, over a store copy, listed the owner's 5 sessions and an empty `confirm.list`; a GLM `fs.write` outside the roots waited, `theseus confirm` listed it, the approval resumed the turn, and the file held the text. Installed at 06:51.

**Divergence from Parts I and II, and from the review.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| An action's arguments are a node and are not duplicated on the action (§3.16; Part II M2) | An action that waits for the operator also keeps its proposal | The confirm binds it and `authorize` re-checks it; every approve path went back to the transcript for it | Recorded here; the Part II text stays as the plan |
| The review: store the proposal on every action at plan time | Only on actions that wait | Each action record is rewritten at every transition, and `session.list` (polled every 2.5 s) decodes every action ever stored | Recorded here |
| Pending confirms derived from planned actions and their nodes (Part III M3) | From `Kernel::pending_confirms`; the node is read only for a question's text | One derivation for every reader | Recorded here |
| The review's estimate: about −340 production lines for the three findings | +443 | Named functions and types with their docs, per-file docs, and every payload kept identical | Accepted; the structure, not the count, was the target |


**Known gaps.**
- `Kernel::cancel_execution` leaves a planned tool call planned (theseus-w98), though its comment says planned
  actions are cancelled. Such a call still counts in the history and the session list.
- A crash between a call's plan and its authorization leaves an action that still "awaits a
  confirm" that was never asked.
- `open_session` and `execution_for` still differ in their narrative and ledger rows.
- The open-action scan still decodes every action (finding 1's index).
- `run_job` (109 lines), `Core::build` (141), and `turn_submit` (88) are still long.

### Narration (theseus-5fy; 2026-09-28, 23:53, to 2026-09-29, 00:55; e3ba8d6, 810aa6d, 12a4805)

_Neither a P5c item nor a plan item. It is a request of the owner's (2026-09-28, 20:38; his words are in §3.14), recorded as an addition to M0.6's visibility (P2b). It took the hook point's slot in the queue._

**What exists.**
- **Config.** A top-level `narrative = true`, the first field of the config, so `theseusd config` prints it before any table. Absent or false means off. Written under a table, it belongs to that table, and the config fails to load (`unknown field narrative`). The template sets it to true, just before `[model]`.
- **The narrator** (`theseus-core/src/narrative.rs`) holds a sequence counter, a 500-line tail in memory, the subscribers, and the sessions met since start (for "resumed"). Every emission is a macro that tests the flag first, so with narration off no argument is evaluated.
- **Eight parts:** session, turn, loop, context, model, tool, approval, and job. Each line carries `seq`, `at_unix_ms`, `part`, `session_id`, `turn_id` when there is a turn, and `text`. The report (`~/reports/theseus-5fy/narration.md`) lists every narrated point with its file and line. Startup, the Discord binding, profile switches, and `session.recompile` requests are not narrated; the recompile a request causes is.
- **What a sentence may carry.** A tool's subject is its first path, relative to the working directory. For `proc.run` it is the program's basename, plus `argv[1]` when that looks like a subcommand (a lowercase word of at most 24 characters), and never the rest of the argv. The gate's "why" is the rule from the gate's reason, with the argv replaced by "the command". Both pass through the secret scrubber. Input shows only as a character count, and a result as lines, bytes, and an exit code.
- **Protocol.** `narrative.watch` (the tail, then `narrative.line` notifications), `narrative.unwatch`, and `narrative` in `health`. With narration off, both requests answer `DISABLED` (-32004), and the message says how to turn it on. `seq` restarts at 1 with the daemon, and a client merges the tail and the live lines on it.
- **Web.** When `health` says `narrative`, the side pane has two tabs, the Observatory and The Narrative (`web/src/Narrative.tsx`). Otherwise the pane is the Observatory exactly as before, and nothing calls `narrative.watch`. A line's session link shows the short id, and its tooltip the title and the full ids (12a4805, from the live check).
- Since theseus-0sg the narrative speaks dollars, and since theseus-58a its context line counts context files (A4).

**How it is proven.**
- **Tests.** The failing tests came first: against the skeleton, 5 of the 13 failed. All pass now, and the workspace ran 151 tests, with the gate green before each commit. They cover:
  - off, which makes no line and refuses a watch;
  - a two-loop tool turn narrated part by part in exact order, with no input or file text in any line;
  - a failed turn that narrates its class and what its finished loops spent;
  - a late subscriber that gets the bounded tail, then every line;
  - the frame budget (17) with narration on and a watcher;
  - the config key;
  - an approval and a background job, with no line carrying the command's script.
- **FAST** (release, interleaved over five rounds). The median of medians of a plain turn was 118.9 ms on the parent, 116.5 ms off, 117.2 ms on, and 117.3 ms on with a watcher, all within noise. A line costs 1.43 µs with a watcher, 0.44 µs on without one, and nothing off: about 13 µs per plain turn with the tab open.
- **Live** (2026-09-29, 00:41–00:46, a scratch daemon over a copy of the owner's store).
  - On: a GLM turn that read a file showed its 17 lines live in the tab, in order, and a tab opened afterwards got the same 17 from the tail.
  - Off: health said `narrative: false`, both requests answered `DISABLED`, the same turn ran normally, and the page had no tab.

  The screenshots are in `~/reports/theseus-5fy/`.
- **Review** (00:55). The gate reran at 151 tests. An independent check on the release build, over a store copy, got 17 narrative lines from a GLM glob turn through `narrative.watch`. Installed at 00:54.

**Divergence from Parts I and II.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| M0.6's windows onto a turn: the trace, the ledger, and telemetry (P2b) | A fourth, the narrative, written for a person watching live and never stored | The owner's request, 2026-09-28, 20:38 | Part I §3.14 records it |
| "a new pane called 'the narrative' … a new tab in the web UI" (the owner) | A tab in the side pane, beside the Observatory | The transcript stays in view beside it | Kept |
| The first build named each line's session by its title | The link shows the short id, and the title is in its tooltip | A title is the first words of a prompt | Kept (12a4805) |

**Known gaps.**
- ~~Subscriber queues are unbounded, as for every notification. A stalled tab grows its queue by about nine lines per turn.~~ Bounded since Item 33: a connection's queue holds at most 4,096 messages, then drops and counts until it drains, and says so with `events.lost`. Found by the v1.1 roadmap (Item 45).
- Two sentences print a kernel error's text. They are the only free text in the narrative that is not a fixed template.
- A `proc.run` subject may include `argv[1]`, so a secret that is a plain lowercase word, and is not in the vault, could show there.
- The owner's daemon stays off until his vault note has `narrative = true` at its very top, before `[model]`, and the daemon restarts.

### Step 2b, part 1. Approval from trusted channels and trusted users (theseus-sgh; 2026-09-29, 06:52–07:25; 8fe368f, 795356f)

**What exists.**
- `[approval]` with `trusted_users` (`discord:<user id>`) and `channels` (`cli`, `web`, `discord:dm`, `discord:<channel id>`). It is optional; the template has it commented, with the owner's DM as the example. A bad entry fails to load and names the forms. There are two warnings, both only when the section exists: an empty `channels`, and Discord listed with nobody trusted.
- One judgment, in `Core::confirm_action`, before the budget branch, so tool calls and the budget question pass the same rule. A refusal is error `REFUSED` (-32005) with `{who, via, why}`, an `approval.refused` row, and a narrative line; the action and its execution do not move. `action.confirm_answered` rows gain `via`.
- A typed surface per connection: `serve_connection` takes a `Client {label, surface}`, and the socket, `--stdio`, the web bridge, and the binding each name theirs. `action.confirm` gains `discord: {user_id, channel_id, guild_id}`, taken only from the binding's connection.
- Discord:
  - The binding sends the ids with each button press.
  - A refused press keeps the card's buttons and gets an ephemeral reason.
  - Cards for an untrusted place go to a trusted DM (`Op::InDm`) with a one-line note in the place.
  - Listed guild channels are checked with twilight's `PermissionCalculator`. The Server Members intent is read from the application's flags, and health reports it as `bindings[].members_intent`.
- Health's `approval {configured, trusted_users, channels[]}`, a `theseus health` line, and the Observatory's Approval section and ledger summaries.

**How it is proven.**
- **Review** (07:48–07:52). The gate reran at 222 tests. Tabitha/Claude's own check on the release build over a store copy: with `channels = ["web"]` a CLI answer was refused with the reason and ledgered, and after a restart with `channels = ["cli"]` the same answer approved and the call ran. Installed at 07:52. The review found theseus-6qy: a `proc.run` job runs as the operator and can reach the CLI socket and the web UI, so with `cli` or `web` trusted it can answer its own session's approvals until M4's boundary; mitigations are held for the owner.
- **Tests.** 222. They include:
  - Without `[approval]`, the existing confirm, decline, supersede, and budget-reset tests pass unchanged, and every surface approves over the protocol.
  - With it, a trusted user in a trusted channel approves. An untrusted user, a trusted user through an unlisted surface (the CLI, the web UI, an unlisted guild channel), and a forged Discord claim are each refused with the reason, and the call keeps waiting. The budget question follows the same rule.
  - The renderer routes an untrusted place's card to the DM with a note.
  - A guild channel without the intent is not trusted.
  - The owner's config shape loads with no rule and no new warning.
  - The frame budget still holds (a plain turn is at most 17 frames).
- **Live**, on a scratch daemon over a copy of the owner's store, with Discord and the web UI off. The owner's note loads under this build with only the known budget-units warning. With `channels = ["web"]`, `theseus confirm` on a waiting `fs.read` of `/etc/hostname` was refused with the reason (exit 1, `approval.refused` ledgered), and the call kept waiting. After a restart with `channels = ["cli"]`, the same answer approved and the read ran. Discord was not exercised, because one bot token means one daemon.

**Divergence from Parts I and II.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| Theseus checks who can see a guild channel when it posts, and again when an answer arrives (§3.9) | Checked at startup, before each card, and at each answer, for listed guild channels only, and only when the portal has the Server Members intent on. The bot's gateway intents lack `GUILD_MEMBERS`, so today a listed guild channel is not trusted | The member list needs the privileged intent. Asking for it on the gateway while the portal has it off closes the gateway (4014), so the binding reads the portal setting from the application's flags and fetches members over HTTP | Kept. The owner decides whether to turn the intent on |
| Every trusted channel's members are all trusted users | `cli` and `web` need no trusted-user entry | Their member is the operator of this machine: the socket is 0600; the web UI is loopback-only, so any account on the machine can reach it, which makes listing it a choice | Proposed as the spec's reading (5a) |
| The approver must be a trusted user and hold the capability | A trusted user only | There is no capability model yet; every execution's principal is the operator | Open until M4 |
| The confirm goes to the person who issued the request (§1) | A card for an untrusted place goes to the DM of the place's latest author when trusted and bound, else to the first trusted DM | A Discord place knows message authors, not a principal | Kept |
| — | `channels` defaults to `["cli", "web"]` and `trusted_users` to none once the section exists | Safe defaults: the section alone takes Discord out until Discord is listed, and leaves the local surfaces as they are | Held for the owner (6) |

**Known gaps.**
- Discord has not run live; the first run is the owner's daemon on this build.
- The web UI still shows Approve and Decline when `web` is not trusted; pressing one shows the refusal on the card.
- Each check with the intent is at least three HTTP requests, which is fine for one guild.
- A thread under a listed channel has its own id, so it is not trusted unless that id is listed.

### Step 2b, part 2. Should have asked (theseus-sgh; 2026-09-29, 07:52–08:53; 814ec6d)

**What exists.** One press on a notice makes that tool ask first until it is undone (§3.9 "Should have asked"):
- `tighten.rs`: the tightenings are one `meta` record, `policy.tightenings`, written in the same frame as each `policy.tightened` or `policy.untightened` row, and read once at startup.
- The gate's `posture_now` takes the stricter of the config's posture and the tightening. A tightened call's reason names who tightened it and what the config says.
- `Core::judge_act` is the one judgment for every approval-like act: an answer, a press, and an undo. An undo needs a trusted answer; a press only needs a surface that can answer an approval.
- `policy.tighten` and `policy.untighten`, with notifications to every watching connection. The notice (`tool.notified`, `policy.notified`) now carries its call's correlation id, since it is written after the plan.
- Discord: one "Should have asked…" select menu on each loop's tool message, or a button per card with `notice_embeds`. The web UI has a button by each notice and an Undo in the Tools view. The CLI has `theseus policy tighten|untighten|list`, and the notice line ends with the exact tighten command.

**How it is proven.**
- **Tests.** 236, 14 of them new. They cover: a press makes the next call wait and an undo notifies again; a tightening never loosens and its undo stops at the config; it survives a restart; only a trusted answer undoes it; without `[approval]` every surface can press and undo; the Discord menu and button; and the CLI lines. The frame budget holds (a plain turn is at most 17 frames).
- **Live** (the step's own check, 08:26–08:29, and Tabitha/Claude's, 08:51–08:52, on the release build over a store copy): a notified `proc.run` offered the tighten command; the press made the next `proc.run` wait with the tightened reason; the tightening survived a restart; the undo put it back to `notify`. With `[approval] channels = ["discord:dm"]`, a CLI undo was refused with the reason and ledgered.
- **The run.** The subagent's run was aborted at 08:31, after its commit and live check. Tabitha/Claude finished the report, reran the gate, checked the release build, and installed it at 08:52.

**Divergence from Parts I and II.**

| Planned | Actual | Why | Disposition |
|---|---|---|---|
| The "should have asked" button turns a call into a labeled example and a *proposed rule* (theseus-770, the consequence gate) | It tightens the tool's posture to approve, stored and undoable; the call is kept as a labeled example | There are no rules left to propose since the reversal (2026-09-28); the posture is the one control surface | Kept |
| A button on every amber notice | One select menu per loop's tool message on Discord (quiet notices), a button per card with `notice_embeds`, and buttons in the web UI | Quiet notices put a loop's notices on one message | Kept |

**Known gaps.** Discord was not exercised live, because one bot token means one daemon. The allow list still runs its entries outright under a tightening, as it does under a config `approve`. An undo from a job through the CLI or the web UI counts wherever those channels are trusted (theseus-6qy).

