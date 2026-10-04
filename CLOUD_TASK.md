<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate you touch, scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

**Setup** (about 15 minutes, once):
- First, in the foreground and alone, run `cargo --version` and wait for it: it installs the toolchain that
  rust-toolchain.toml names (about 30 seconds). Start no other cargo or rustup command until it is done: two at once
  collide in rustup's download directory. If it fails, run it again.
- `cargo install cargo-nextest --locked` (about 3.5 minutes) and `cargo install cargo-deny --locked`.
- `npm ci` in cockpit/.
- `cargo build --workspace --all-targets` (about 9 minutes cold).
- `cargo deny fetch`, so the gate's deny phase can run offline. If the fetch fails, skip that phase and say so in the report.
- Run long commands in the background and wait for their completion notice. Don't end your turn while work remains, unless a background command will wake you.
- Never delete anything outside the repository and /tmp (nothing under /root/.rustup or /root/.cargo). This
  environment refuses some commands, and three refusals in a row stop the session until a person looks: when one is
  refused, take another route instead of retrying it.

**Known on this VM, and not yours to fix** unless your task names them (other changes fix them):
- The sandbox's contract tests that need a non-root user can fail or skip here: the VM runs everything as root, and Linux exempts root from `RLIMIT_NPROC` (theseus-pv6i).
- `theseus-core tests_push::the_snapshot_and_the_events_agree_under_the_position_rule` failed once in 20 suites here (theseus-amr2).
- A reaping or job test whose `turn.submit` is answered `internal`, `No such file or directory (os error 2)`, under load (theseus-46ya: a spool read race).

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine. Any other failure is yours to explain.

**Other changes in flight.** About twenty-five other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

These are reviewed, and merging into `main` one at a time tonight. Your clone may hold some of them already:
- the Jev wire-in (`[judge]`, `JudgeService`, the judgment sink, `loop.v1` in shadow, crates/theseus-judge's client);
- the ontology wired in (crates/theseus-ontology's records, its snapshot, the context compiler's guidance walk; `MANIFEST_FORMAT` goes to 7);
- MCP tools in turns (`McpBoard`, `[mcp.servers]`, the `Tool` trait's names as `&str`);
- `theseus judge prove` (a report generator in crates/theseus-judge);
- five gate flakes;
- the user unit's restart limits and install checks (crates/theseusd/src/install/, scripts/user-service.sh);
- the MCP server (`[mcp_server]`, `/mcp` on loopback, `Surface::Mcp`);
- AWS hands on Lambda and Fargate (the `hand` role, `aws.hands.run`, the SQS poller, infra/aws/'s hands files);
- the durability tender (WAL segments and blobs to S3, index rows to DynamoDB);
- a terminal toolset (`term.*`, a pty per session);
- the gate's bench build, features recheck and cockpit build (scripts/gate.sh).

These are other cloud sessions like you, each on its own branch:
- the judgment surfaces (trace marks, `judge` spans, `judge.list`/`get`, the cockpit's Judgment section);
- `security.v1` in shadow at the gate;
- `classify.v1` and `role.v1` at inbound;
- CONTINUE's signals and `continue.v1` in shadow;
- the arrangement on `task.create`;
- the memory `Recall` node, `derived_from` edges and the `BudgetReport`;
- the `+rerank` memory arm;
- the cockpit's Ontology view;
- `categorize.v1` in shadow and `tasks.parked` in health;
- bindings format 2 (many guilds, per-place ceilings);
- MCP prompts (`/prompt`, `theseus prompt`, the cockpit's picker);
- `extend.propose`;
- `theseus restore --from s3://…`;
- AWS hands' cancellation, TTL reaper, reservations and grid;
- the LSP board and tools (crates/theseus-lsp's consumers).

On the owner's machine:
- a benchmark run (bench/, and docs/benchmarks.md at its end).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field to a stored record or a new record kind, bump it by one from main's number as you cloned it, and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. Others bump it too: the maintainer renumbers at the merge.
- **Files near their line ceiling** (scripts/long-files.txt): crates/theseus-protocol/src/lib.rs, crates/theseus-core/src/turn.rs, crates/theseus-core/src/config.rs, crates/theseus-discord/src/runtime.rs, crates/theseus-kernel/src/kernel.rs, and crates/theseus-core/src/tests_m3.rs. Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).

**The gate, before every commit:** `THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

The gate's shape phase fails a file over its line ceiling in scripts/long-files.txt. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Opus 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261004-smalls-tools`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
- No new dependencies: Cargo.lock and the package-lock.json files must not gain a package. If the right design needs one, say so in the report instead.
- Use invented names in fixtures, tests, and commits (AGENTS.md, Item 16). Write nothing about the owner, his machine, his accounts, or anyone else.
- Don't edit the spec, docs/status.md, the README, or docs/design/. The maintainer writes those at review. Where a doc should change, say what and where in the report.
- Don't commit half a step. If time runs out mid-step, leave it out and report what you found.

**The report:** when you are done, or at your task's deadline (by the clock, waits included), whichever comes first, write CLOUD_REPORT.md at the repository root. Commit it as the branch's last commit, subject `cloud report (not for main)`, and push. The maintainer reads it from the branch and drops that commit at the merge. For each step it says:
- what you found;
- what you changed (commit hashes);
- how you proved it: the commands and their results, with test counts, the runs under load, and each planted revert and what failed;
- the live check the maintainer should run, as exact commands, and what each should show;
- what is left or uncertain, and any design choice the owner should hear about.

Then the gate's result, naming each failing test and why. Your final message is short (under 1,200 characters): the line `CLOUD REPORT COMPLETE`, then the branch, its head commit, one line per step, and the gate's result.

**Planted reverts:** to prove a test guards a behaviour, plant the bug, show the test fail, then restore the file and `touch` it, so cargo rebuilds it (a restored file with its old mtime keeps the planted build). Run `git status` after every restore.

**Load:** where your task asks for runs under load, use AGENTS.md's recipe. Priority, not count, makes the load: run the test with `nice -n 19`, and beside it four busy loops at nice 0, each `sh -c 'while :; do :; done' &`. Kill the loops by the pids you started, never by a name pattern.

---
## Your task: the review's small changes to steps 28b, L3, 16 and 40 (theseus-ext.12; theseus-mgw.10's policy)

Branch: `cloud/20261004-smalls-tools`. Every commit's subject carries `theseus-ext.12`, except the restore policy's,
which carries `theseus-mgw.10`. Deadline for the report: 4 hours after you start.

**Background.** The owner reviewed four merged steps and chose small changes: `categorize.v1` in shadow (28b),
a language server's errors on edits (L3), the restore from S3 (16), and the hands' hour (40 part 2). Do the parts in
this order, each in green commits of its own, and report what's left if time runs short. Part 4 waits on the owner's
last word: keep it in commits of its own that the merge can leave out.

**Read first:** the root AGENTS.md and those of crates/theseus-core, theseus-lsp and theseus-aws;
docs/design/m5-judgment.md §2.12; docs/design/aws-toolset.md §3.3, §3.7 and §5 (step 16); the newest Items in
docs/spec/16-part3-item-86.md.

**What the code says** (the code and AGENTS.md win: report each difference, and where the code can't settle a
question, build the clear part and report the question):
- `prepare_categorize` (judge/categorize.rs) reads the records after the session's mark (META
  `judge.categorize.<session>`), asks `due()`, and returns unjudged when no topic is declared. The mark is written
  only as a judgment is dispatched, so with no topic every exchange end reads the session from its start
  (theseus-gky0); and with under ten new human messages the input reads the whole session (`session_nodes`).
- The Choice is the candidate topics plus `new_topic` and `none`. Jev answers only Choice, Score and Noul, so it
  can't name a topic: the operator's accept does (`theseus ontology accept <jdg> --topic <name> --desc …`).
- A Jev call reserves (theseus-judge's price.rs) its request at 3 bytes a token plus `CALL_OVERHEAD_TOKENS`, and per
  question `OUTPUT_PER_QUESTION` plus `OUTPUT_PER_OPTION` per option, at $0.042 a million tokens. Live, a 50-topic
  judgment billed 1,798 in and 477 out: 96 µ$ against 82 reserved (theseus-q0rn); a 2-topic one, 688 and 54: 32
  against 37. So the output allowance (552) covered the output, and the input estimate fell about 400 tokens short.
- `[lsp.servers.<name>] start_on_edit` is a plain bool, off unless set (`[lsp]` is off by default). Each preset
  (theseus-lsp's servers.rs) carries `initialization_options`.
- Errors that miss an edit's wait ride the session's next edit or `lsp.*` result (lsp/edits.rs, "Diagnostics that
  arrived since an earlier edit"), so an `lsp.diagnostics` for the same file repeats what it just listed.
- The restore session's policy (aws/durable/read.rs) grants `s3:ListBucket` under `StringLike` on `s3:prefix`. A
  `GetObject` carries no `s3:prefix`, so S3 should answer a missing key 403, not 404 (not yet seen live): that's
  `ReadError::Other`, and one missing blob or tail fails the whole restore. The tests' fake answers 404 either way.
- The hour's meter (aws/hands/watch.rs) sums at each poller pass what the account's AWS actions dispatched this clock
  hour reserve or spent (only hands reserve today). Past `hourly_alert_usd` it alerts once that hour; nothing refuses.
  toolrun/hands.rs admits a hands group.

**What to build:**
1. **categorize.v1 discovers topics (theseus-ext.12, with theseus-gky0 and theseus-q0rn).**
   - On an empty ontology the point still judges: the Choice offers `new_topic` and `none`, the mark moves, and a
     `new_topic` answer is a proposal as now. Edit no pack file (a change to one is a new version); update
     categorize.rs's doc comment.
   - gky0: with no topic declared, the decision reads only the records after the mark, and the input takes only the
     human messages since it, never `session_nodes`.
   - q0rn: make a Choice's reservation cover its options (say, input tokens per option), so both measured calls
     reserve at least their cost. Pin the figures in a test, and list the pinned reservations elsewhere that move.
2. **Language servers (theseus-ext.12).**
   - `start_on_edit` on by default for ty, tsgo and rust-analyzer, off where the config sets it false (the field
     becomes optional). The other presets stay off; the template's [lsp] comment says so.
   - rust-analyzer's preset gets `initialization_options` `{"cargo": {"targetDir": true}}`: its checks build in
     `target/rust-analyzer`, never taking cargo's lock on the agent's build directory (it used 4.2 GB on this
     repository; `idle_stop_mins` frees it). If its `workspace/configuration` answer could undo that, it says so too.
   - An `lsp.diagnostics` result leaves out of "Diagnostics that arrived…" every file it lists itself; other files'
     still ride. Pending errors stay in memory (the owner's call).
3. **The restore's policy (theseus-mgw.10).** Use `StringLikeIfExists` on `s3:prefix`, so a missing key is 404 and
   the restore says it's missing. A list with no prefix then passes too (key names in the owner's bucket, never
   contents): say so in the doc comment. The fake answers a missing key 404 only when the session's inline policy
   grants `s3:ListBucket` with no condition that needs `s3:prefix`, and 403 otherwise, as S3 does.
4. **Runaway-train mode (the owner's words: "let the budget notify be the authority unless 'runaway train' mode is
   triggered, which is, observationally spend is 10x over the limit").**
   - The budget lines stay alerts, and the alerts are the authority: `hourly_alert_usd` (1.0) and `daily_budget_usd`
     (the template's comment suggests 10).
   - `[aws.accounts.<id>] runaway_factor`: default 10.0, at least 2.
   - When the observed spend (what the hour's line already counts: reserved plus spent) reaches `runaway_factor` ×
     `hourly_alert_usd` within the hour, or `runaway_factor` × `daily_budget_usd` within the local day (when that is
     set), the account enters runaway mode.
   - In runaway mode, a new AWS action the meters count (a hands group today; anything later that reserves) is
     refused when its own reservation would add to the runaway figure. The refusal says the spend, the line, the
     factor, when the hour or day turns, and the keys that change it.
   - Never refused: a cancel, a list or status read, the reaper, and a running hand's settling. Running hands finish
     normally.
   - The meter must be current at dispatch: add what was dispatched since the last pass, or compute it then.
   - Entering runaway mode writes one row and one notice to the owner's surfaces, and health's hands line shows it.
     It ends when the hour or day turns. Raising a line or the factor takes a restart (the config is read at start).
   - The template: `runaway_factor = 10.0` beside `hourly_alert_usd`, with a comment in the owner's words.

**Proof, offline:**
- categorize: on an empty ontology a due exchange end dispatches a judgment with 0 candidates and moves the mark; a
  `new_topic` answer lists as a proposal, and accepting it with `--topic` and `--desc` makes the topic; after that,
  exchange ends read nothing before the mark (count the records read); a 52-option Choice built like the live one
  reserves at least 96 µ$, and the 4-option one at least 32 µ$.
- LSP: the three presets start on an edit, and `start_on_edit = false` stops it; rust-analyzer's `initialize` carries
  `cargo.targetDir` (the fake server records it); `lsp.diagnostics` right after a pending edit lists each error once.
- restore: with one blob's object deleted, the fake answers as S3 would; the restore says the blob is missing and
  restores the rest.
- runaway mode: spend at 9.9 × the hour's line runs a group; at 10 × it is refused with its words, one row and one
  notice; a running group finishes and settles; the next hour (a fake clock) lets it run; the day's line trips it the
  same way; a cancel in runaway mode still runs.
- Planted reverts, each naming the test it breaks: return early on an empty ontology again; drop rust-analyzer's
  options; put back `StringLike` (the restore test fails on AccessDenied); skip runaway mode's check.

**The live check is the maintainer's.** Write it in the report as exact commands and what each shows, on a scratch
daemon with Discord and the web off:
1. **categorize**, with `[judge] enabled = true`, `[secrets]` `jev_api_key` and a model key, no topic declared: ten
   messages on one subject in one CLI session; `theseus judge log` shows `categorize.v1` with 0 candidates answering
   `new_topic`, and `theseus ontology proposals` lists it; accept it with `--topic garden --desc …`. Add 49 topics
   and run another ten-message session: in `theseus ledger -k judge.call --json`, `reserve_micros` ≥ `cost_micros`.
2. **LSP**, with `[lsp] enabled = true` and a scratch cargo crate: ask for an `fs.edit` that breaks a function.
   rust-analyzer starts on the edit, its error is pending and then arrives, and `target/rust-analyzer` appears;
   meanwhile `cargo build` in the crate never prints "Blocking waiting for file lock on build directory".
   `lsp.diagnostics` right after lists the error once.
3. **The restore** (under a cent on the AWS account, after the owner's go): step 16's live check, with an image
   attached to one turn (`theseus ask --attach`) so a blob ships. Delete one object under the probe's `blobs/` with
   the owner's own credentials, then restore: it says that blob is missing and restores the rest, and CloudTrail
   shows that `GetObject` as NoSuchKey, not AccessDenied.
4. **Runaway mode** (a few cents, on an account set up for hands): with `hourly_alert_usd` and `runaway_factor` set
   so that one small group's reservation passes their product, `aws.hands.run` is refused with the words and its
   notice; raised, and after a restart, the group runs.

**Leave alone** (changes in flight near these files): the hands' network (40: config/aws.rs, the template's `[aws]`
lines, aws/hands/mod.rs, launch.rs; keep your key beside `hourly_alert_usd`); the memory pass (31a: judge/mod.rs,
theseus-judge's pack.rs and builders.rs); `route.v1` (25e), the ladder (26a), replay (25d) and security notices (24),
which add packs and judge code; rerank live (32d).
