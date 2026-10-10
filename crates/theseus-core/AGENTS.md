# theseus-core

The agent itself: config and secrets, sessions, the turn, the context compiler, tool calls and the gate, the protocol
server, the push, the outbox, and telemetry. Read by theseusd, theseus-discord, and theseus-sim.

Key modules: `turn.rs`, `compiler.rs`, `toolrun.rs`, `rpc/`, `config.rs`, `crash.rs`, `aws/`. Read by: theseusd, discord, sim.

## What's here

- **The turn**: `turn.rs` (the turn runner: one turn under a session's turn lock, its loops, its frames), `advancer.rs`
  (continue or end the turn), `harness.rs` (the harness loop, parked on its events and the heartbeat), and
  `rpc/driver.rs` (what that loop drives: the heartbeat's spool drain and reconcile, continuation turns, due wakes,
  and the cancel path). A turn's end and a late result's wake are one frame (`turn/end_step.rs`, theseus-6qwr): the
  stop read under the frame's lock, the wake nested so its failure takes back only itself. A failed turn's retry is
  the driver's (theseus-ljr), but not a `--stdio` daemon's: its run parks the turn on input for its one client
  (`turn/stdio_step.rs`, set by theseusd before serving, theseus-zqxv); `[model.retries]` can make a transient
  failure's call again inside its turn instead (`turn/retry_step.rs`, none by default; the bench profile's, since a
  headless run ends with its turn: theseus-7gir.21). Once the stop has begun (`Outbox::stopping`), the driver begins
  no continuation and a call about to be sent is settled failed unsent, class `stopping` (`ProviderError::Stopping`),
  with no in-turn retry, so the next start's driver retries it (`turn/stopping_step.rs`, theseus-jtrc); its run posts no
  notice and health and telemetry count no provider error for it (theseus-36re). A `--stdio` daemon started `--one-shot SECS`
  (`theseus --spawn ask`, theseus-mqxk) serves one run (`one_shot.rs`, on the outbox): each turn's result names what
  it left for later (`TurnSubmitResult.later`, `turn/later_step.rs`, the jobs read before the late results are taken
  and again as it parks), its turn's connection watches the session, and `wake.at` says a wake due past the bound
  will not fire here; no other daemon reads it. The ask's own turn fixes the run's end, and a wake fires in the run
  when it is due by the end the run had when it was set, less `FIRE_MARGIN_MS` for the driver's tick (`OneShot::fires`):
  the words and the result's `fires`, which the CLI follows, are one rule however long the turn ran. A refused request goes once to its model's fallback, the catalog's
  `refusal_fallback_model` (Sonnet 5.5's: Sonnet 5), and the rest of the turn runs there (`turn/fallback_step.rs`,
  theseus-7gir.18; `[model.retries] refusal`, on by default; the provider's own `refusal_fallbacks` wins where it rides
  the request, `compiler::server_fallbacks`). The compilation stays the profile model's: the requests name the fallback
  (`RequestSpec::fallback`), carry both models' thinking, and leave the refused answer out (`replaced_answers`, as a
  cut one); the session's target stays. A `provider.fallback` row, and the result's `fallback`, whose `line` every
  surface shows. Tests: `tests_fallback.rs`.
- **Context**: `compiler.rs` (manifests, recompiles, the cache layout, the token estimate), `context_files.rs`, and
  `catalog.rs` (each model's window, prices, and caching). A model may have a second price tier, `LongPrompt`
  (Haiku 5.5's: 5x past a 100,000-token prompt, theseus-3okf): `terms` prices a usage at the tier its prompt
  (input, cache reads and writes) picks, `reserve_micros` and `reserve_parts` at the tier the input estimate picks;
  price a usage only through them, never by reading a row's base prices. A refusal fallback's window holds its
  model's (`every_refusal_fallbacks_window_holds_its_models_requests`).
  - **Files given to the model** (theseus-9g2, theseus-c9l6): `attach.rs` and `blobs.rs`. A message's files: text as
    text (`[tools] max_read_bytes`), an image as an image, and any other file kept whole in the store's blobs up to
    `[tools] max_attachment_bytes` (32 MiB), a `File`. A PDF is read once, when it arrives, in theseus-files' capped
    child (`theseus_files::convert`; the turn's `accept_files`, under `theseus_store::blocking`): its pages, its text
    by page (a JSON blob), and a part of its first pages for each page limit it passes (100, 600) or the request's
    byte budget (18 MiB). A `file.read` row each, with its span and `theseus.file.read.duration`. The compiler
    renders a PDF per model (`catalog`'s `pdf`): a `document` block (the whole file, or the largest part the request
    has room for, with a line saying what was left out), else its text by page (GLM, a PDF the provider refused, one
    the request has no room for). PDFs are budgeted in render order (`attach::Spend`), so an earlier one never
    renders differently because of a later one, and the cached prefix holds. `fs.read` and `http.fetch` return a
    PDF's pages (`theseus_tools::Media::Pdf`), kept the same way (`toolrun::keep_media`). Join 2: a Word, Excel,
    PowerPoint, OpenDocument, EPUB, RTF, or notebook file, and an archive's list, is read into sections when it
    arrives (`theseus_files::convert::doc`; a JSON `{"sections": …}` blob, `blobs::Section`), and every model reads
    its text, a vision model its notebook images too; a recording, a video, and any other file are named with how to
    read them. **`file_read.rs`** (`file.read`): the runtime runs it (`ToolRuntime::read_file`), finding the file
    among the session's nodes by name or digest: a PDF's other pages, a document's sections, an archive's member
    saved and read under `<cwd>/.theseus-files/<session's last 8>/`, a recording's transcript through Deepgram
    (`[voice]`'s key and settings; capped at `[tools] transcribe_max_minutes`; kept by digest in
    `blobs/derived/`; booked as `speech.transcribed` by `book_transcript`), a video's transcript and frames
    (ffmpeg), OCR (tesseract), or `save`. In a shared place its result is outside text. Tests: `attach.rs`'s, and
    theseusd's `tests/files.rs`, which run the real converter, a turn per model, and `file.read` with a stand-in
    Deepgram.
  - **CONTINUE's candidate signals** (M5 25b): `signals.rs`, read inside `compile()` from what it is given (the
    clock passed in as `CompileInput.signals`, never read there): a dormancy gap, the tail crossing its soft band,
    a task report or a wake arriving, and a provider cache miss, each against what was written since the model's
    last answer, so an input's signals fire once, at its turn's first compile. Thresholds are `[judge.signals]`;
    they are read whether or not the judge is on, and decide nothing: every one that fired rides on
    `context.compiled` (`signals`). A compile that appended and fired one asks `continue.v1` in shadow
    (`judge/compile.rs`: the dispatch is a spawn, and the turn's trace gets a zero-length `judge` mark naming the
    judgment's id, minted at the dispatch). Tests: `tests_continue.rs`.
  - **The system header** (`TurnRunner::system_blocks`): the persona, the assembly note (`turn::ASSEMBLY`: what the
    harness puts in a request, and that a recalled note is no part of the person's message; theseus-fpm2), the
    precedence line (`compiler::situation::PRECEDENCE`, 35a), the tools note, and the profile's text, all static.
    Harness facts every request needs go there, never into a session's messages.
  - **Situations** (M6 35a, theseus-3nk.1): `compiler/situation.rs`. What a compile is for: the step tells it from
    what it holds (`TurnRunner::situation_of`, `turn/situation_step.rs`: no compilation yet is a conversation's or a
    task's start; a session's first compile in this run, `RunCompiles` in memory, with nothing its turn brought is a
    resume; else a continuation; a detour its own), and `compile()` settles it (`settle`: a new compilation of its
    own triggers is a recompile with its trigger). It rides `Compilation.situation` (store format 22) and
    `context.compiled`. `admits` is the table of what each admits, from the code (lessons reserved for 35b): a
    detour admits its window's messages, replies and results, and a task's arrangement among them, never a recall
    or a summary (`compile_detour` leaves those out). `check`, after the compile, fails a piece not admitted, or a
    set that does not close (a result without its call, a call without its result), as `context_unadmitted`, with
    nothing sent and no retry; it enforces from the first day, with no shadow mode (the owner, 2026-10-05). An assembled
    `recall_id` whose node is not in the session is a section never written (its call never dispatched; nodes are
    never deleted): the render leaves it out, and so does the check (theseus-783a). The
    precedence line (`PRECEDENCE`) follows the persona and the assembly note in the header. Headers written from 35a
    on (§2.11's testimony): an item's names its origin and place (`TurnRunner::place_name`,
    `recall::render::item_header`) and ends `volatile: as of <date>, unverified` when its shown text holds one by the
    labeler's rule; a summary's names its positions and model; a synthesis's sources name their places too. A frozen
    header never changes. Tests: `tests_situation.rs`, `compiler::situation::tests`, `recall::render::tests`.
- **Places** (the place rule, theseus-nbsh; it replaced 19a's labels on nodes): `places.rs`. Every place a session
  speaks in is private or shared. Private: the CLI and the web UI (no place), a DM with an owner (`Config::owners_for`),
  and a guild channel the binding bound private (its own `private = true`, or, saying nothing, in a guild the bindings
  file trusts whole with its guild's `private = true`, theseus-rdqg); it gets everything. Shared: every other
  place; it gets the public tools (`places::public_tool`), the file tools only under `[places] public_paths`
  (canonical roots, so a link out of a tree is outside it), and only the context files marked `readers = "public"`
  (`context_files::withhold_shared`). A turn's class is `TurnRunner::class_of`: where its words go, which is its
  session's place (a task's is its parent's, `outbox.target`), or, for a session whose place moved on, where its wakes
  and reports answer (`outbox.wake_target`); shared when that cannot be read. It is fixed in `TurnCtx.class` once the
  turn has taken its wakes and reports. The catalog (`ToolRuntime::definitions_for`) and the tools note
  (`system_note_for`) are per class, and the gate refuses a shared place's other calls (`places::refusal`, answered
  `Not run: …`, reason `place: …`). The binding tells the core its places as it starts (`Core::bind_places`, in
  memory: an unnamed guild place is shared), and reads each private channel's viewers once (`private_place_viewed`,
  `place.viewed`), for health's `places:` line. In a trusted guild it reads none, and health names each private
  channel there `(in a trusted guild)` (`Core::trust_guilds`, `PlaceInfo.trusted_guild`). M6's recall draws in a
  shared place only on that place's own sessions (and the books, when built, will too). Tests: `tests_places.rs`,
  `places::tests`.
  - **Ceilings** (step 38a, theseus-ext.3; `ceiling.rs`): the bindings file may give a place a ceiling, which
    narrows what its class allows and never widens it. It is read with the class (`TurnRunner::view_of`, so a task
    has its parent's), and rides in `TurnCtx.ceiling`: its `tools` filter the catalog (`PlaceView::offered`) and the
    gate refuses the rest (`Ceiling::refusal`, reason `place: …`), its floor is applied after the policy's decision
    and before T1's hold (`Ceiling::floor`), its spend limit is the kernel's `place_limit` (the lower of it and the
    config's, pinned while a place caps it, told at each binding start), and its profile is a `turn.submit`'s when
    the turn names none (`Core::place_profile`). Each guild has its own trust word (`PlaceRule::trust_guilds`).
    Tests: `tests_ceilings.rs`, `ceiling::tests`, and the kernel's `tests_place_limit.rs`.
  - **Publish** (`rpc/publish.rs`; graduation's light form): `place.publish` puts one item (a node, a file the owner
    can read, or a message) into a bound place's session as the owner's message, with a `derived_from` edge for a
    node (`publish`), its `place.published` row, and a notice post in the place, in one frame under the place's
    execution lock, never while a turn holds it. Only the owner, from a private place: `judge_act(Act::Publish)`
    which asks `places::owner_in_private`. `theseus publish`, Discord's `/publish`.
- **The ontology** (row 26, step 21b; M4 §2.8; theseus-8kk.1): `ontology.rs` holds the snapshot (`Board`: built
  after serving by one `onto:` META prefix scan, `Core::warm_ontology`, or by its first reader; kept current on every
  write, which checks, writes its records and their rows in one frame, and swaps it), the given kinds (a bound
  place's `channel:`/`person:` category made at its first bind, `Core::bind_categories`; a session's given
  membership read from its place at compile, never stored), and the turn's `Walk`, fixed with the request's spec.
  `compiler::compile` renders the guidance after the context files: an append composes the memberships its
  manifest recorded, a recompile (or a ring) the current ones, so a membership change waits for the next recompile
  and a guidance edit in play is one `system_changed`. The manifest records `memberships` and `guidance`. A shared
  place composes only its own place's given memberships, and the place rule's admissions never read a membership.
  The methods are `rpc/ontology.rs` (`ontology.list`; the writes are `judge_act(Act::Ontology)`, the owner from a
  private place); the rows `fact/ontology.rs`. Tests: `tests_ontology.rs`.
- **Tool calls**: `toolrun.rs` (every call the model makes becomes a kernel action: the gate and the dispatch), with
  a job's call in `toolrun/job.rs` (its turn waits on the job's wake, `toolrun/waits.rs`, and takes the job's
  completion with its result in one frame; Tier 7.1; a command that never started says why, `toolrun/not_started.rs`,
  never "(no output)": theseus-f7tz), a `proc.run` batch's steps in `toolrun/steps.rs` (each a job
  under the call's one correlation id, the next launched once one exits 0, the turn's wait held across them so
  the drain never settles the call between two; theseus-7gir.3) and its gate in `toolrun/batch.rs` (`judge`: each
  step as the call alone, the strictest taken), the continuation in `toolrun/resume.rs`, and the results
  no call's own run writes (late ones, and a cancel's) in `toolrun/late.rs` (theseus-5gw9). A call's span is
  built in the turn that answers it (`turn/calls.rs`, theseus-8pei): `run_tools`'s calls in their loop, and the
  calls `resume` answered (`ResumeOutcome.ran`) and the late results `absorb` took (`LateCall`, a point with the
  job's `run_ms`) under the continuation's span; the tool metrics count each call once, at its answer. A result the
  cap cuts keeps its whole output (theseus-v73m; `toolrun/kept.rs`, `outputs.rs`): a job's (`proc.run`, a batch's
  step: its raw file scrubbed again in a streaming pass, to its 64 MiB, never the tail alone) and a `term.read`'s,
  0600 at `<state>/outputs/<session>/<call>.out`, named in the node's `full_ref` and in the cut line with the lines
  left out, counted in the whole output, and the `fs_read` that reads them; that read is judged as one inside the
  roots (`Outputs::own_read`, in `order.rs`, the session's own folder alone). The raw file is still deleted. The
  sweep (`Core::sweep_outputs`, with the spool's hourly sweep) deletes a retired session's copies, any past
  `[tools] outputs_keep_days`, and the oldest past `outputs_max_bytes`. `fs.read` has its own cap
  (`Tool::result_max_chars`, 100,000), cut as a contiguous head, never head and tail. Tests: `tests_kept.rs`. The gate's parts are
  `policy.rs` (postures and the floor), `external.rs` (the hold after external text), `broker.rs` (granted
  secrets), `approval.rs` (who answers, and from where), and `peer.rs` (the web UI's other-uid check at accept).
  A URL whose host is a private address waits for approval (`listed`) and only an approved fetch reaches it,
  unless `[policy] private_addresses = "open"` (theseus-7gir.20, the bench profile's), which lifts both
  (`web::net::PrivateAddresses`, `Web.private`). Plus the harness's own tools,
  `task.rs` and `wake.rs`, the web tools in `web/`, and AWS in `aws/`: the bound accounts, each key's check after
  serving (its calls fail closed until STS names the account), who signs (`session.rs`: the key until the config
  names `owner_role`, then work, job, floor, and tender sessions, and the key signs only STS), `aws.call` (reads,
  writes, runs; its plan reads `theseus-aws-guard`: a guardrail is the floor, IaC-only and stack writes are invalid
  input, deletions of what holds state wait), `aws.describe`, `aws.whoami`, `aws.s3.list`, the stack tools
  (`stack.rs`: plan, apply bound to the change set's digest, status, delete), `aws.cost` (`cost.rs`), the bootstrap
  (`bootstrap.rs`, `rpc/aws.rs`: a read-only plan, the apply on the operator's yes), and the tenders after serving
  (`tend.rs`: the budget's reconcile and line, GuardDuty's weekly usage), and the durability tender (`durable.rs`,
  step 15: with `durability = true`, the WAL's sealed segments, the open one's tails, and blobs to S3, and index rows
  to DynamoDB, in the session `theseus-durability`, from a cursor in `<state>/durability/` saved after every object,
  so a restart asks S3 instead of sending again). Its tests are `aws/tests.rs` (C1), `aws/tests_c2.rs` (a fake
  CloudFormation with state), `aws/tests_durable.rs` (a fake S3 and DynamoDB with state, binary bodies, and
  checksums, and a missing key answered 404 or 403 by the session's policy, GET and HEAD alike, judged as real S3
  judges it: the implied `s3:ListBucket` carries the key as `s3:prefix`), `aws/tests_list_prefix.rs` (`ListItsPrefix`
  under `StringLike`: each session lists only its own prefix, theseus-bfk9), `aws/tests_synced.rs`
  (only frames the writer synced ship: `Hooks::synced_to`, `Store::synced_to`, theseus-mgw.12), and
  `aws/tests_restore.rs`; `config/aws.rs` holds `[aws]`'s types and checks.
- **Hands** (step 40, theseus-mgw.6 and .11): `aws/hands/`. `aws.hands.run` (`tool.rs`, `launch.rs`: the request, the
  backend Lambda or Fargate as §3.3 chooses, the stacks' outputs read once per account, each launch and its tags) runs
  through `toolrun/hands.rs`, not `run_inproc`: its call answers `background`, and the group's aggregate is its late
  result (`job_result` hands it to `hands_result`). A group (`group.rs`) is the call's action, a META record
  `aws.hands.group.<group>`, and one kernel action per hand (tool `aws.hand`, its `resource` the group's key), all
  written in one frame before anything launches; a dispatch is a hand's claim. The poller (`poller.rs`,
  `Core::poll_hands_after_serving`) long-polls the completion queue only while a group or a hand is open, checks each
  envelope's HMAC (`envelope.rs`: HKDF of the account's secret and the correlation id, nothing stored), quarantines a
  failure (`completion.quarantined` with its `why`), and settles through `Kernel::accept_completion`. The `hand` role
  (`hand.rs`) is `theseusd hand`. Part 2 (theseus-mgw.11): a running hand stops by its backend (`cancel.rs`:
  Fargate's `StopTask`, verified `ecs` once `DescribeTasks` or ECS's own event shows STOPPED and booked at the time
  it ran; Lambda's `unsupported`, its timeout the bound, its late envelope booking its cost), when `until` is met and
  when a cancel or `/stop` reaches the group's call through `ToolRuntime::terminate_all`. Each hand reserves its
  worst case in the group's frame, and a group over the session's budget asks its question as a model call does
  (`Hands::take_over_budget`, read at the top of the turn's next loop). The heartbeat leaves hands to their own
  reconciler (`overdue.rs`: asked about with `DescribeTasks` before unknown; one the TTL reaper stopped fails with
  its reason). `quota.rs` caps a group's running hands by the account's quota, read once an hour, so a big group
  launches in waves. `watch.rs` is health's hands block, the hour's meter (`hourly_alert_usd`, alert only, its mark
  a META record), and the reaper's failures off the queue; `grid.rs` is `hands.list` and Discord's one line per
  group (a `hands` post under the group's key). Tests: `aws/hands/tests_hand.rs` (the role), `tests_hands.rs`
  (through the core, a fake AWS with a queue), `tests_part2.rs` (part 2 on the same fake), and theseusd's
  `tests/hands.rs` (a real daemon `kill -9`'d mid-group).
- **Language servers** (L2, theseus-n88g.8): `lsp/` over theseus-lsp's client. `Board` starts one server per
  (server, root) at the first call for a file of its language, never before (the root: the nearest marker inside the
  workspace roots; for Rust the nearest `Cargo.toml` with `[workspace]`, else the topmost), through
  `children::spawn(Kind::Owned)` in its own process group with the job environment, its stderr in a capped
  `<state>/lsp/<server>-<hash>.log`. It stops one idle for `[lsp] idle_stop_mins`, one whose request timed out, and
  all at the daemon's stop (SIGTERM, never waited for); a crash is `lsp.failed`; the next call starts it again. The
  tools (`lsp/tools.rs`) are async reads addressed by path, 1-based line, and symbol text; the rename is two calls
  (`lsp/rename.rs`, as the stack tools: `lsp.rename.plan` keeps the edit by digest, `lsp.rename` writes exactly
  it, its files the plan's resources). `lsp::gate`, after the call's own order in `toolrun`'s gate, judges a call
  that would start a server at `proc.run`'s posture for its argv (once per root in a daemon's life) and makes a
  rename's write outside the roots wait. A cancel or `/stop` aborts the call's task, so the client sends
  `$/cancelRequest`. Each request is an `lsp.request` span under its call (`Board::bind`, `spans`) and the metric
  `theseus.lsp.request.duration`; the facts are `fact/lsp.rs`; health's `lsp` block is `Board::health`. Never
  offered in a shared place (`places::offered`). Config: `config/lsp.rs`. Tests: `tests_lsp.rs` (the fake in
  process), and the daemon's `tests/lsp.rs` (the real fake binary, and a `kill -9`).
- **L1** (M4 17b): `sandbox.rs`: `[sandbox]`, a job's class (toward L1 alone: the default, `l1_argv`, the model's
  `sandbox: true`), L1's posture (notify), the view an L1 job gets, and health's block: the last L1 launch, read from
  its job's completion (`Sandbox::launched`), and why L1 refuses every job of a root daemon (theseus-pv6i).
  `theseusd check` runs the self-test on demand (`Sandbox::self_test`, over `toolrun::sandbox_for`'s view); nothing
  probes at the start, and an L1 job has no cgroup (theseus-gyin). Its facts are `fact/sandbox.rs`; its tests
  `tests_sandbox.rs`, and the daemon's `tests/sandbox.rs` with real L1 jobs. What a call's proposal binds about
  its job is `sandbox::Bound`: its class, and (18c) its egress list.
- **An L0 job's cgroup** (theseus-a5nv): `cgroup.rs` asks systemd once, 2 s after serving (`systemctl show -p
  Delegate`), whether the daemon's own cgroup is delegated, and readies it (`theseus_kernel::cgroup::ready`); health's
  `cgroup` startup phase holds the answer, and `run_job` hands it to each wrapper (`WrapperArgs.cgroup`), whose
  command is born in a cgroup of its own with `[tools] job_pids_max`. A result whose cap refused processes says so
  (`toolrun/job.rs`). The daemon's `tests/cgroup.rs` runs them in delegated scopes and units.
- **Grants in L1** (theseus-w5op; decided by the owner 2026-10-03, superseding theseus-7y9y): an L1 job takes its
  program's broker grants at its launch, exactly as an L0 job does. The gate's order (`toolrun/order.rs`) runs
  the L1 decision (`sandbox::unbrokered`) through `ToolRuntime::brokered` (decision 15: the stricter of the call's
  posture and the secret's, so any approval comes before the launch), and `sandbox::for_job` asks
  `Broker::for_job`. The value rides in the job's environment over
  the spec's pipe, the wrapper withholds it from the job's output (`redact`), and the result's head names what the
  job was given (`given GH_TOKEN`, names only, from the wrapper's `detail.sandbox.granted`). Tests:
  `tests_grants.rs`, and the daemon's `tests/sandbox.rs` with a real L1 job. 18d's run-time socket
  (`theseus-cred get`) is gone; a stored `cred.request` action and its ledger rows still read.
  An `aws` grant (18e) is the same: an L1 job gets its job session at launch, named by its correlation id,
  and it is the only AWS credential it holds (`~/.aws` is in `sandbox::CREDENTIALS`, hidden in every
  view whatever the approve list says; no daemon `AWS_*` variable passes `forbidden_env`; L1 has no route
  to the metadata service). It reaches AWS through its egress list, as any host: `[sandbox] egress`, or
  the hosts its call names. Tests: `aws/tests_l1.rs`, and the daemon's `tests/sandbox.rs`.
- **Egress** (M4 18c): `egress.rs`. `[sandbox] egress` and a call's `sandbox: { egress }`; the gate's step for
  hosts beyond the list (the call waits, and its approval reaches those hosts alone, since the list is in its
  proposal); and what a completion's `detail.egress` makes of a result: its `sandbox.egress` and
  `sandbox.egress_refused` rows (in the frame that writes the result), its lines, and DD5's `external` marker
  when the job reached a host beyond `[sandbox] egress`, so T1 holds the session (`via: egress`) and the node is
  untrusted, the owner's. A job that reached only listed hosts holds nothing (theseus-gyin). Its tests:
  `tests_egress.rs`, and the daemon's `tests/sandbox.rs` through a real proxy.
- **Cancellation** (M4 18a): `cancel.rs`, the one stop every path that stops running calls goes through
  (`ToolRuntime::terminate_all`: a cancel, a task's cancel, `/stop`, the disk's floor, a stop at a job's launch).
  A job's wrapper is asked to stop its tree; an async tool's task is aborted (`Stops::track`) and verified once its
  handle has finished; anything else is unsupported. Each verdict lands on its action, in health's `cancels` and
  the metric `theseus.cancel` (both from `Stops::count`, theseus-qdk5), and as a fact (`fact/cancel.rs`). An aborted call's result waits for the cancel's verdict (`after_abort`). Its test is
  `tests_cancel.rs`; the daemon's are `tests/job_wrapper.rs`, `tests/sandbox.rs` (L1), and `tests/tasks.rs`.
  A job's last steps wait for its stop's end (theseus-dwoj): `stop_backends` stops, then `Stopped::write_verdicts`
  writes each job's acknowledgement, verdict and fact's row in one frame, and a cancel's sweep of its unanswered calls
  joins that frame (`answer_after_cancel_with`), so a cancel of a running job is two frames, the cancel's and that one
  (theseusd's `tests/cancel_frames.rs`). A crash between them leaves the action `requested`, which every reader takes
  as `acknowledged`.
- **Terminals** (theseus-n88g.4): `term/`. `term.open`, `term.send`, `term.read`, `term.close`: a program on a pty
  (libc's `posix_openpt`; its child through `children::spawn`, `setsid`, the pty its controlling terminal), read as
  a screen by `term/vt.rs`, a small VT model whose module doc says what it leaves out. Async tools whose run needs
  its session, so `toolrun`'s async path runs a `term.*` call through `Terms::run`. `term.open` plans its argv and
  `term.send` its terminal's, so the gate judges both as that program's run; `term.read` and `term.close` are reads.
  A listed external program's screen (or a terminal sent keys naming one: `Listed::in_text`) is outside text. At
  most 4 a session whose programs run: an ended one frees its slot, its last screen readable until `term.close` or
  until a full session's open reclaims the oldest (theseus-ggqf). Closed at its execution's end (`turn.rs`), a cancel
  and a `/stop` (`rpc/driver.rs`), and the daemon's stop (`finish_stop`). `term.close`, a cancel and a `/stop` hang
  up, wait the grace, and kill what lingers, a process that left the tree but holds the pty included. At the
  session's end and the daemon's stop, under `[tools.term] keep_background` (default on, `config/term.rs`), the
  close ends only the program and the pty's foreground group (by group signals, so a fork mid-close is not spared)
  and leaves the background running, as `proc.run` does (`term/left.rs`: booked, in health's `terminals_left`, a
  `term.left` row, ended by a later cancel or `/stop` of the session's execution or its parent's,
  `Core::end_terminals_left`). `term.read`'s `until_idle` waits for the program back in front (`tcgetpgrp`) and
  waiting for input (`/proc/<pid>/syscall`: a read of fd 0, or a select or poll), looked at every 40 ms and never
  within 100 ms of a send. In memory only: the rows `term.opened`, `term.closed` and
  `term.left` (`fact/term.rs`) are the record. No broker grant reaches a terminal (`brokered`). Tests: `term/tests.rs` (goldens
  and real `sh`, `python3`, `cat`; a `/proc` scan for a test's own processes looks for a marker holding the run's
  pid, so another tree's run beside it is never taken for its own, theseus-d006, theseus-fps6), `term/tests_keep.rs`,
  `tests_idle.rs`, `tests_slots.rs` (theseus-ggqf; `term/bench.rs` its ignored measures), `tests_term.rs` (the
  gate, the hold, each close, a task's job left and a `/stop`'s or a cancel's end of it, the config reaching the
  terminals), and theseusd's `tests/headless.rs` and
  `tests/terminals.rs`.
- **The protocol server**: `rpc/` (`server.rs` routes each method by name; `methods.rs`; `confirms.rs`), with
  `bus.rs` and `outbound.rs` (one ordered, capped queue per connection; a test may lower the cap a new connection
  takes, `Push::backlog_cap`, so the lag prove overflows it with a few hundred events, theseus-0u6g).
- **Session states** (theseus-emqx): live, quiet or retired, derived when read (`theseus_protocol::sessions::derive`,
  `SessionRecord::state_at`; `session_info` gives every list its state, busy reading live whatever its age), never
  stored as a state: the record keeps only `retired` (superseded or by hand; empty is derived past `[sessions]
  empty_grace_minutes`), the `superseded_by`/`supersedes` links, `reopened_ms` and `title_was` (store format 25).
  `succession.rs`: a place's move (`Core::bind_place_to`: the place's record, both links and `session.superseded` in
  one frame, the old session's lock then the new one's), and the owner's `session.retire`/`session.reopen`
  (`judge_act(Act::Session)`, `rpc/sessions.rs`). `session.list` with no params is every session as before; `state`
  filters, and the answer names its windows. The re-title is `turn/title_step.rs` (in the turn's own session write,
  once, never a task's). Tests: `rpc/tests_session_states.rs`.
- **Surfaces of the record**: `push.rs` (the board), `outbox.rs`, `narrative.rs`, `ledger.rs`, `trace.rs`, and
  `telemetry/` (OTLP, metrics, spans).
- **Facts**: `fact/` (theseus-j6qn, Review 2's C2). One type per thing that happened, which says in one place what
  each channel gets: its ledger row (`KIND`, `row`), its notification (`METHOD`, `event`), its sentences
  (`narrate`), and its span (`span`). A site builds the fact and records it once (`TurnCtx::record`, a turn's
  `record`, or `Core::rec`/`session_rec` for the core's own); `FACTS` lists every one. A row's kind is a
  `theseus_protocol::LedgerKind`; `LedgerRow::new` takes nothing else (tests write an old or unknown name with
  `LedgerRow::named`).
- **Start and stop**: `config.rs`, `config_copy.rs`, `config_gate.rs`, `secrets.rs`, `startup.rs`, `restore.rs`
  (`restore`, and `repair`: a corrupt frame taken whole from a copy, theseus-15g), `sweep.rs`, `disk.rs`,
  `binary.rs` (whether jobs can write the daemon's own binary, read when health asks), and `crash.rs` (the panic
  hook's crash file beside the store, which the next start takes and health reports). The config template is
  `config/theseus.example.toml`, public, so it never carries a deployment's own values: a deployment's note holds
  only what differs from the defaults, and `config/sparse.rs` cuts a whole note to that (`theseusd config
  --sparse`, theseus-vwar). `config/lookup.rs` finds the config when nothing names one (theseus-5aqz):
  `~/.theseus/theseus.toml` if it exists, else `/etc/theseus/theseus.toml`, which `scripts/setup.sh` writes.
- **MCP servers** (M7 36b): `mcp/`. `McpBoard` tends each `[mcp.servers]` server (`config/mcp.rs`) after serving,
  never before: a stdio server through `children::spawn(Kind::Owned)` in its own process group (stderr to
  `<state>/mcp/<name>.log`, capped), with the job's environment and its `env` secrets; an HTTP one over reqwest.
  States, the crash backoff (1 s, 5 s, 30 s; a third crash in 10 minutes is `failed` until `mcp.restart`), and a
  stop that sends SIGTERM and never waits. Each server's last list is a META record, `mcp.tools.<server>`, read at
  the start (one key each), so a start offers it at once and a call waits for its own server alone. `McpTool`
  (`mcp/tool.rs`): canonical `mcp:<server>/<tool>` for the gate, wire `mcp__<server>__<tool>`, `Run` and
  `NonRepeatable` unless the server's `read` lists it (a server's hints never loosen), results outside text
  unless `external = false` (an error result too), `mcp_unavailable` and `outcome_unknown` in the failure's meta,
  which `toolrun`'s async path reads. `ToolRuntime::mcp` is the catalog the board fills: offered after the
  built-ins, in private places only, and a turn's request spec fixes its tools, so a `list_changed` applies at the
  next turn (`mcp.tools_changed`, and an operator notice). Facts in `fact/mcp.rs`; tests in `mcp/tests.rs` (an
  in-process fake through a `Connect` stand-in, the backoff on the paused clock) and theseusd's `tests/mcp.rs`.
  **Prompts** (36c, `mcp/prompts.rs`): each server's `prompts/list` is kept with a digest per definition and stored
  as META `mcp.prompts.<server>` (no format bump: a new key), listed again on `prompts/list_changed`; `mcp.prompt.list`
  reads it, and `mcp.list` carries it. `turn.submit { prompt }` resolves the prompt first (`McpBoard::resolve_prompt`:
  arguments checked against the definition, that server waited for alone, `prompts/get`), so a refusal leaves no
  session or node; a shared place's session is refused (`REFUSED`). The messages are user-role nodes of origin `mcp`
  (store format 8), author `prompt:<server>/<name>`, written with T1's hold (`mcp.prompt`, from `external = true`) in
  one frame (`turn/prompt_input.rs`). A use of a prompt whose definition differs from its last use (`mcp.prompt_used.<server>`)
  is `mcp.prompt_changed` and an operator notice (`mcp_prompt_changed`), once; the use goes ahead.
  A server with `sandbox = "l1"` (43a, `mcp/l1.rs`) is the daemon's own binary in its `mcp-sandbox` role
  (`theseus_kernel::mcp_l1`), spawned as any stdio server is, holding the server's init: the view a job gets
  (`Sandbox::job_view`), its own `egress` list (none: no network), and its pipes the board's own. It never falls
  back to L0: a root daemon's start fails with L1's refusal. `l0` stays a configured server's default. Its
  test is theseusd's `tests/mcp_l1.rs`, which as root checks the refusal alone.
- **Proposed extensions** (M7 43a, theseus-ext.5): `extend/`. `extend.propose`, a harness tool the turn awaits
  (`ToolRuntime::run_extend`, never on a core): `freeze.rs` copies the directory into
  `<state>/extensions/<name>/<digest>/`, read-only, the digest a SHA-256 over the sorted tree (paths, 755/644
  modes, bytes; a link is refused); the board starts the frozen copy as `ext-<name>` in L1 on trial
  (`mcp/trial.rs`, state `proposed`, never in `servers`, so `rebuild` never offers its tools), lists, runs each
  test, and stops it; the manifest is a META record, `extend.manifest.<name>.<digest>`; and the ack is a planned
  `extend.ack` action with its card, answered by `action.confirm` (`judge_act`'s place rule, then
  `extend/answer.rs`: an ack binds its confirm, a decline or an expiry declines it, neither wakes anything). Facts in
  `fact/extend.rs`; `extend.list` and health's `extensions` in `extend/list.rs`; tests in `extend/tests.rs` and
  theseusd's `tests/extend.rs`.
  **Loaded** (43b, theseus-ext.8; `extend/load.rs`, `mcp/ext.rs`): the ack's frame also writes the META record
  `extensions` (one key: name to digest, command, tools, capabilities, who acked and when, the proposing session, and
  its place and ceiling at the ack), `extend.loaded`, the server's stored list `mcp.tools.ext-<name>` (the trial's
  tools, so a new version never offers an old one's), and a replaced version's manifest (`replaced`). Then the board
  loads `ext-<name>` beside the configured servers (`McpBoard::load_extension`): the frozen copy in L1, the acked
  network only, no secret, results outside text only with a network (Q22); its tools are in the catalog at once and
  offered from the next turn's start (the turn's spec is fixed). Posture `notify` unless `[policy.mcp]
  "ext-<name>"` says otherwise (never looser than the enforcement), class `Run`; the load's ceiling's floor holds
  every call (`Floors`, in `toolrun`'s gate after the place's own). A start reads the record once
  (`Core::seed_extensions`, in `build`) and offers the stored lists; each starts after serving with the rest. The
  `ext-` prefix is refused for a configured server. `extension.revoke` (`extend/revoke.rs`, `judge_act(Act::Revoke)`,
  in the CLI's `OPERATORS`): one frame (the record without it, its manifest `revoked`, `extend.revoked`), then the
  board drops it and SIGTERMs its group; the frozen copy stays. Tests: `extend/tests_load.rs`, theseusd's
  `tests/extend.rs`.
- **The index tender's supervisor**: `tender.rs` (row 51): it starts `theseus-index` 2 s after serving
  (`START_AFTER`, so a start's aftermath stays quiet), restarts it with backoff, takes over the one an exec kept
  at once, and asks it for health and `index.query`, each call bounded (health asks only a tender that runs, and
  never past 100 ms). Its rows are facts (`fact/index.rs`). Its tests, `tests_tender.rs`, run on tokio's paused
  clock with a stand-in `Os`. Its gauges (`theseus.index.*`: lag, documents, RSS, and restarts by their rise) are
  sampled from health's block each metrics interval after serving, only while telemetry has an endpoint
  (`tender/sample.rs`, theseus-gfi4; tests `telemetry/tests_index.rs`).
- **The judge** (M5, steps 23a and 23b): `judge/` (`JudgeService`, built at the first judgment; `sink.rs`, the
  batched frames; `spend.rs`, the shadow day budget; `loop_end.rs`, loop.v1's input; `mark.rs`, a dispatch's mark).
  A judgment a turn dispatches is decided (mode and sample, pure) and its id minted before the turn's last frame,
  and marked in the trace there (a zero-length `judge` span of kind `mark`: pack, point, mode, judgment); the call is
  spawned after the frame with that id (`theseus_judge::Ask::id`). Every point that dispatches inside a turn marks
  the same way. The facts (`fact/judge.rs`) say their sentences, and `Telemetry::record_judgment` counts each
  judgment, once the sink's frame is written; nothing of a judgment rides in a turn's frames but its mark. The
  sink's frames are written only between turns (theseus-0j2.8), through the memory pass's writer handshake
  (`memory_pass::turns`, `JudgeService::write_between`), as consolidation's are: a row, its sentences and its
  metric wait while turns run (the pass's bounds end the wait), and a press finds it in `pending` meanwhile. A
  backlog keeps one clock from its pass's start until the queue is empty (`sink::Queue`, theseus-s1am), so past the
  quiet bound it drains in the next gaps; a clean stop writes the queue before its last checkpoint (`finish_stop`,
  `JudgeService::flush_sink`, theseus-ych4), and a SIGKILL loses it. Tests: `tests_sink_backlog.rs`,
  `tests_sink_flush.rs`. A frame's staged blobs are written before its wait, as one batch (`Blobs::put_many`), a
  stop's frames hold 512, categorize's mark rides in its row's frame, and a shadow judgment no turn waits on writes
  its budget block between turns (`reserve_between`; theseus-ehkp, theseus-xkbs; `tests_sink_blobs.rs`,
  `tests_sink_off_turn.rs`, `tests_sink_busy.rs`, `judge/tests_ahead.rs`, and categorize.rs's own, theseus-5o3d).
  A crash inside a batch leaves its temporary files, and the next start sweeps a dead writer's once it serves, on a
  thread of its own (`Blobs::sweep`, `Core::sweep_blobs_after_serving`; theseus-vipg; theseusd's `tests/spool_sweep.rs`).
  `theseus-sim bench turn --judge` measures where the judge's frames land.
  `judge.list` and `judge.get` are `rpc/judge.rs`; `judge.list` pages back from the newest `judge.call` row (its
  kind's tag, or its kind-and-session tag) and stops one match past its limit, so `matched` is a floor when `more`
  (theseus-wse2; `tests_judge_reads.rs`). Tests: `tests_judge.rs`, `tests_judge_surfaces.rs`,
  `telemetry/tests_judge.rs`.
  - **The judgments a turn waits on write nothing before their call** (theseus-otny): route.v1's inbound batch and
    a live rerank stage their state's blob (`stage_blob`; the sink writes it just before the first row naming it,
    `write_staged_blobs`) and write the shadow budget's block beside the call (`reserve_beside`, `Beside`), so their
    syncs (1.4 s each under a neighbour's IO, 2026-10-04: the 3.2 s late verdict) never delay a verdict. A person's
    message warms Jev's client as it arrives (`warm_on_message`, from `TurnRunner::run`: built, and two `HEAD`s of
    the judge's path, nothing billed, unless it answered within `theseus_judge::client::POOL_IDLE`, 180 s, under the
    edge's 200 to 400 s). From serving on (`Core::warm_judge`, theseusd's `after_serving`, never on the start
    path), `judge/warm.rs` opens two and keeps them: a keeper on tokio's timer sends the `HEAD`s again after each
    `theseus_judge::client::KEEP_WARM` (150 s) of Jev's silence, so a fresh daemon's first message pays no
    connection setup (theseus-ddbi; `tests_route_wait.rs`). A try that fails to connect, with nothing answered since, is `jev_unreachable`, and then
    no turn waits on route or rerank. The fake Jev answers a `HEAD` 405 and counts it as `warmups()`, apart from
    `connections()`, which stays the calls'.
  - **At the gate** (step 24, `gate.rs`): `security.v1` and `security.v3` in shadow at every call that acts, and at
    a fetch or a search in a holding session (Q12). `ToolRuntime::start` plans the call and sends its notice, then
    `judge_at_gate` makes the choice and the marks, nothing else; the gate's decision never waits on Jev or changes
    with it. A gate judgment's id is its pack and the call's correlation id hashed (`gate::judgment_id`), so the
    turn's trace marks it (under the call's span) and a "should have asked" press labels it (`judge.label`, in the
    press's frame) before its row is written. A notified call's score follows its notice as `judge.scored`.
    Tests: `tests_security.rs`.
  - **v3's live notices** (step 24's notices, theseus-0j2.13; the owner's decision of 2026-10-04): `notice.rs`.
    `WIRED` gives `security.v3` `live` while `[judge.packs."security.v3"] notices` is on (the default;
    `JudgeService::given`/`mode`), so its judgments are marked and recorded `live`. When v3, live, answers `risky`
    act-true for an open call the hold did not make wait (`notice::flagged`), the gate's task posts once the
    judgments land, never on the call's path: one frame with the post to the owner (`Outbox::stage_to_operator`,
    kind `jev_notice`, which the Discord courier posts to the owner's DM alone) and its `tool.notified` row
    (`by: judge`, keyed `notice_<judgment>`, scoped `judge:security`), then `judge.noticed` to the turn's clients.
    The brake is security.v1's `[[rollback]]` (`learn::check_all` over today's notices and the owner's `noise`
    labels on v3's judgments, at each notice and each such label, `JudgeService::after_label`): a fired rule
    writes one `judge.paused` row (`what: "notices"`), a `jev_paused` post and a META mark (`judge.notices`,
    health's read), and holds notices to the next local day. The first use in a run reads today's rows in
    `judge:security`: the pause by its key, and the day's `tool.notified` and `judge.label` rows by a page from the
    local midnight (`read_day`, theseus-b8e2), never the scope whole but while the index's shape is built. A label on a noticed judgment edits its post (`jev_labeled`), and `judge.label` takes a
    judgment whose `judge.call` row the sink has not written yet from its notice's row. Health's `notices`.
    Tests: `tests_notices.rs`.
  - **At an exchange's end** (step 28b, `categorize.rs`): at a private conversation's exchange end (never a shared
    place's, never a task's), `categorize.v1` judges in shadow when 10 human messages arrived since the session's
    last judgment, or the exchange began after 30 minutes' quiet; the decision, off the turn's path, reads the
    session's records after its mark (META `judge.categorize.<session>`, moved as a judgment is dispatched). It
    runs outside every turn, so it marks no trace. The service reaches the core by `Weak`
    (`JudgeService::attach`). Jev writes no membership: `rpc/proposals.rs` lists its proposals
    (`ontology.proposals`, one scan of `judge:categorize`) and takes the operator's answer
    (`ontology.proposal.accept`/`reject`, through `judge_act`): an accept's membership, its topic for
    `new_topic`, and its `judge.label` row (`fact::judge::ProposalLabel`) in one frame. Tests:
    `tests_categorize.rs`.
  - **People from text** (theseus-wy7y, `judge/people/`): `people.v1`, live (`WIRED`), at the same exchange end as
    `categorize.v1` (`live.rs`: private places only, never a task's, `categorize::due` over the records after its own
    mark, META `judge.people.<session>`, moved in the extraction's frame), and the owner's backfill
    (`backfill.rs`, `import.people { propose }`: a tag's sessions in id order, the machine's quiet between them,
    under a spend cap for the run, resumable from META `import.people.propose.<tag>`, a dry run pricing it with no
    call). A pass (`run.rs`): `[people] extract_profile` (`config/people.rs`) answers through one tool
    (`extract.rs`: name, handles, role line, evidence; scrubbed lines), a `people.extracted` row (cost, tokens,
    model; a day-ceiling spend kind); the owner, personas, agents, `[people] not_people` and a session's known names
    excluded before any call (`NotPeople`, built by `Core::not_people` in `house.rs`, theseus-0p1r: the place rule's
    owner handles, not only `[places] owner`; the held person they hold, by its name and each word, never a
    candidate, and a `match` to it (it stays an option, so Jev can name a form of his name the store cannot know)
    never listed; every imported episode's agent and each agent's `Name:` in its imported
    `IDENTITY.md`, kept per catalog version; the house's names, the profiles and models; a bot's or a UI's name;
    `fold` drops a leading "@"); one `people.v1` judgment a kept candidate (real, involved, which of the 50 nearest
    held people or new, whether its role line judges the person, whether it carries their pay, money, health,
    leave or HR matters: either, or unanswered, drops the line), the candidate in its context. Code
    decides as the proposals are read (`people::decide`, `[people] act`/`confirm`; a person the exclusions exclude
    is not listed nor bulk-accepted, counted in `ontology.proposals`' `hidden`): `rpc/proposals.rs` lists them
    beside the topics' (`OntologyProposal.person`), and an accept joins the held person, or declares the new one
    (unless an exact handle or name finds it by then). Tests: `judge/people/tests.rs`.
    **Live, gated by Jev** (theseus-u5n8, the owner's "combine, gated by Jev"; `seen.rs`, pack `people_seen.v1`,
    scope `judge:people_seen`): at each due point one Jev call first, over the exchange's lines and the held people
    it may involve (`seen::listed`: the session's own, then those its lines name; never the owner's or one
    `NotPeople` excludes; at most 20, two per-item Nouls of ten). A listed person's Noul at `[people] confirm` is a
    proposal of that person (named `<judgment>/<person>`, once a session, not again where the owner rejected it);
    only its `unlisted` Noul at `[people] gate` (0.6) runs the extraction and `people.v1`. Under it, or unanswered,
    no model call; the mark moves either way. The backfill has no gate. Tests: `judge/people/tests_seen.rs`.
  - **The learning ledger** (step 25c, `learning/`): labels (`labels.rs`: what a label says of a Noul, a Choice,
    a Score; the heaviest counts, then the newest), system labels derived by each run (`system.rs`, weight 0.5,
    keyed by judgment, question and rule so a second run writes none), the report per pack version and question
    from the `judge:<pack>` scopes alone, every number from `theseus_judge::learn` (`report.rs`), with its holdout
    frozen into it, and the tender (`tender.rs`: never within 10 minutes of a start, at `[judge] learning_hour`, a
    `learning` thread at nice 19, in `SCHED_IDLE`, and about 5% of a core, each next pack waiting while the
    machine is busy: theseus-tood). `judge.label` (`judge_act(Act::JudgeLabel)`) and
    `learning.report` are `rpc/learning.rs`; the run writes its labels, `judge.report` rows and the run's META mark
    in one frame, then `<state>/learning/<date>.json`. The mark keeps the last position the run read (`through`),
    and the next run's rules read no judgment whose windows closed before it (`system::Cut`, `open_ms`: its
    session's nodes, its call's action, the task briefs; theseus-cf5c), and log how many sessions they read. The
    mark's `cut_ms` is where the next run cuts: the run's clock, never past the newest judgment it read, so a clock
    that read ahead closes no open window (theseus-gf8j); a mark without it walks everything once.
    Tests: `tests_learning.rs`, `learning::*::tests`.
  - **Routing** (step 25e, theseus-0j2.11): the route pack (`judge::inbound::ROUTE_PACK`: `route.v3` since
    theseus-qe3v, live at once in place of `route.v2` and `route.v1`, which stay embedded) asks at the inbound point in a request of its own, beside
    the batch of `classify.v1` and `role.v1` (theseus-ddbi: one question answers sooner than the batch), live while
    `[judge]` is on (`[routing]`, `config/routing.rs`, lowers it). Its verdict comes back over a oneshot
    (`RouteWait`, an `Answered` with the time it came) the moment its request answers, and the call waits for a
    permit rather than being shed. The turn waits for it beside its first compile, at most
    `max_wait_ms` after it (`turn/route_step.rs`, `beside`; a late verdict applies to the next message alone, and a
    late detour's (`trivial`, `quick`) to none: `routing::carries`, theseus-6n5j), and not at all while Jev is known unreachable
    (`JudgeService::jev_unreachable`, reason `unreachable`, theseus-otny); `routing.rs` decides, purely, at the
    mode's own bar (`[routing.modes.<mode>] switch_confidence`, else trivial's 0.4, else the section's 0.6:
    `RoutingConfig::confidence_for`): the mode's first usable profile under a place's cap, a detour (the modes of
    `config::routing::DETOURS`, `trivial` and `quick`: that turn alone, compiled outside the session's compilation,
    which it never writes; never test a mode by name, ask `is_detour`), or a switch of the session's
    `routed` profile (stored: format 15), held above `cold_switch_tokens` until a second turn agrees. A turn whose
    profile the owner chose (`Target.chosen`; the pane's `carried` profile is none) is recorded in shadow. Only the
    compilation the call uses is persisted (`RouteState.defer_persist`), and only its `context.compiled` and
    `loop.started` recorded (`RouteState.deferred`, theseus-d13v); a detour's loop records `loop.started` alone,
    since its compilation is never stored (`tests_route_rows.rs`). The row is `route.decided`
    (`fact/route.rs`: `wait_ms`, `late` when the verdict missed the wait, `answered_ms` when the request came back
    after the turn's start, if it had; each live turn's wait is `theseus.route.wait` by `theseus.route.late`,
    `tests_route_late.rs`); thinking goes back only to the model that wrote it (`tests_thinking_writer.rs`), and a place's
    profile caps it (`tests_route_cap.rs`). `routed` holds only while route.v1
    acts live for the session (theseus-9yyr, `route_base`): routing off or in shadow, the ladder's rollback, the
    judge off, or Jev unreachable (`JudgeService::reachable`) clear it at the next turn, in the turn's own session
    write, and so does a `profile.use` after the session's last turn began (META `live_profile.switched_ms`
    against the turn id's time, `crate::id_ms`). A move keeps its base, `Routed.from` (the profile's name it was
    first moved from, format 21, theseus-0j2.17): once route.v1 stops acting the turn runs there, and a turn whose
    base is another (the live profile or a place's changed) clears the move (`same_base`). The pane's carried
    profile, when it is the routed one, stands for `from` (`turn_submit`), never a new base. Tests:
    `tests_route.rs`, `tests_route_base.rs`, `routing::tests`, `turn::route_step::tests`.
    **The effort** (route.v3, theseus-qe3v): `reply_effort` rides the route pack's one request beside `mode`;
    `routing/effort.rs` decides, purely: Jev's level over the profile's `effort` unless `effort_fixed`, clamped to
    `[routing] effort_bounds`, only for a model whose catalog row takes effort; below the question's confirm band,
    `unclear`, a pin, shadow, or a late (carried) verdict leave the profile's own. `apply_effort` sets it on the spec
    and the first compile's request (`compiler::output_config`), and takes that request's digest again, which the
    turn records and the overflow retry compares. `route.decided`'s `effort_*` fields and `TurnRoute`
    say which. Tests: `tests_route_effort.rs`, `routing::effort::tests`.
  - **The owner's corrections** (theseus-q31l; `correction/`, `turn/route_step/correct.rs`,
    `rpc/route_correct.rs`): words (`correction::words`, deterministic and narrow, read by `turn.submit`
    unless a job sent it, and taken at the inbound point only where the owner alone writes, the CLI, the web
    UI or an owner's DM, never a guild channel bound private and never a task's turn; after a bare verb a lone
    adjective is a remark, not a name: `named_plainly`), or `route.correct` (a reaction, a press, `theseus judge
    correct`: `judge_act(Act::RouteCorrect)`).
    Each writes the owner's label (`not:<mode>` on `mode`, only when the profile named is not one the
    verdict's mode leads to) and a `route.corrected` row (scoped `route.corrections`, its provenance by
    id) in one frame: the turn's own for words. The turn steers ahead of the verdict (`Steering`,
    `routing::steer_to`, `Reason::Correction`, `route.decided`'s `source`/`follows`) and waits for none.
    The layer (`correction::layer`) is in memory, bounded, rebuilt after serving (`warm_corrections`),
    and retires when another route pack version acts; the route pack is in `lineage::LEFT_ALONE`, so
    no nightly rewrite folds it today. Tests: `tests_route_correct.rs`, `correction::*::tests`.
  - **Replay, audit, and backfill** (step 25d, theseus-0j2.14; the owner's runs, each `judge_act(Act::JudgeRun)`
    and in the CLI's `OPERATORS`, each on a `learning` thread at nice 19, routed together by `rpc/judge_runs.rs`).
    `learning/replay.rs` (`Core::judge_replay`, public for the learning loop): a candidate the build does not wire
    over a report's frozen holdout, its train split, the incumbent's errors, or ids; stored states when builder,
    version and cap match, else `learning/rebuild.rs` (only `loop.v1`'s input: the turn's `turn.ended` row and its
    nodes through `loop_end::input`; `unrebuildable` says why for the rest); thresholds-only re-bands, no call. Its
    calls and run are scoped `judge.replay:<pack id>`, which the report never reads; `[judge] replay_limit_usd`.
    `learning/audit.rs`: a profile's answers as audit labels (weight 0.5), capped by `audit_limit_usd`.
    `learning/backfill.rs`: under `backfill_consent` only; one judgment per event (keyed by it), `event_at_ms`, which
    `read_scope` takes as the judgment's time. Tests: `tests_replay.rs`, `tests_audit.rs`, `tests_backfill.rs`.
  - **The prove** (L3, roadmap row 50, theseus-0j2.18): `learning/prove.rs` builds `theseus_judge::prove`'s records,
    one per `task.ended`, from rows the ledger holds (`build` is pure; `Core::prove_input` reads pages by kind and
    session), every outcome from the labels as `labels::resolve` settles them, never a rule derived again;
    `classify_quality` is classification's half. `judge.prove` (`rpc/judge_prove.rs`, a read: its window reads the
    `pack.mode` rows from their scope, since the ladder's first load writes) runs the generator over them; `theseus
    judge prove` prints its Markdown byte for byte as `theseus-judge prove` does. Tests: `tests_prove.rs`.
  - **The ladder** (step 26a, `judge/ladder/`): each pack version's mode as `pack.mode` rows scoped per pack id
    (`pack:<id>`, a few rows), read once after serving (`Core::warm_ladder`, `JudgeService::read_ladder`), or by an
    RPC (`pack.*`, `judge.label`) or the nightly check, then kept (`Ladder`; with no row, `WIRED`'s line). No point
    reads it (theseus-289c): before the warm read, `Ladder::given` answers `Ladder::unread`, the wired line under
    the config with a pack that would act in shadow (a row may have rolled it back; never act on an unread ladder),
    `placed` the root, health's lines say so (`… shadow (until the ladder is read; wired live)`), and route_base
    keeps a session's move for the read ladder. The warm read writes the adoptions in one frame, a quiet stretch
    after serving and between turns (`memory_pass::turns`). A new local day starts empty, reading nothing: every
    event since midnight landed through `land`, and the notices' brake reloads the ladder. Core rigs whose turns are
    judged do the warm read at build (`tests_judge::warm`: `tests_judge`'s rigs, `tests_route`, `tests_rerank`,
    `tests_notices`, `tests_security`); `tests_ladder_unread.rs` holds the rule. Every point asks
    `JudgeService::mode_for(pack, session)` (or `ask_mode`, which records `pack_arm` in the judgment's context): the
    ladder's rung under `JudgeConfig::mode_of`'s ceiling, a canary acting in its `learn::arm` and the control in
    shadow, `rolled_back` as shadow. A ceiling at shadow needs no read. Rollback (`rules.rs`): each event a rule
    counts is a `pack.event` row scoped `pack.event:<id>:<day>`, so a restart reads the day back; `learn::check_all`
    runs as each lands (`JudgeService::land`; `judge.label` lands a label's) and again after the nightly report; a
    rule from the adoption table (`adopt.rs`: `route.v1`, `rerank.v1`, `security.v3`, live before the ladder,
    adopted once as the owner's, and `route.v2` and `route.v3`, each adopted with its own reason, theseus-3okf and
    theseus-qe3v) is a day's brake (`until` the next local midnight, folded away after it), any other
    stands until a promotion. `rpc/packs.rs` is `pack.list`, `pack.promote` and `pack.rollback`
    (`judge_act(Act::Ladder)`; short of `promote::bar` the owner's row is `forced`, the system's refused); a
    `security.*` promotion is a card, its question planned on the ladder's own session (META `ladder.session`) and
    answered by `action.confirm` like an extension's ack (`answer_promotion`: the bind or decline and the row in one
    frame, nothing wakes). A promotion's `said` names a learned version standing ahead of the one moved, where the
    move judges nowhere or only in a canary's control arm, and its rollback (`rpc/packs_ahead.rs`, theseus-nwa5):
    said, never refused. Tests: `tests_ladder.rs`, `judge::ladder::tests`.
  - **The learning loop** (step 25f, theseus-0j2.12; design §2.17): `learning/propose.rs` (the run: nightly after
    the report, and `judge.learn`, the owner's act), with its pure parts in `theseus_judge::propose` (names from
    v101, the interleaved split below 200 labeled in the window, the text-only check, the threshold re-fit, the
    decision, the writer's prompt). Learned versions are `pack.version` rows scoped `judge.learn:<id>` beside the
    `judge.proposal` rows, read by `judge/lineage.rs` (`JudgeService::pack`, `root_of`, `placed`) by the warm read
    after serving, or by `placed_read` (the learning loop, `pack.list`), never by a point, their files
    `<state>/packs/` derived. Every point asks `placed(root, session)` for the version standing in its root's place,
    then `mode_for` that version (capped by the root's config line). A move is `Core::promote_learned` (26a's act,
    citing the proposal). The writer's output is capped (`WRITER_MAX_TOKENS`): the profile's own cap reserves past
    the day's limit. Tests: `tests_learn_loop.rs`, `theseus_judge::propose::tests`.
- **Recall** (M6 step 30a, in shadow): `recall.rs` (`Memory`: `[memory]`, the science, and who answers the index's
  query, the tender or a test's stand-in, `Memory::set_ask`; the manifest; `TurnRunner::place_of`, the place rule
  read as `class_of` reads it), `turn/recall_step.rs` (begun as the first loop's model call goes out, read once it
  answers, never past `[memory] recall_deadline_ms`: a stalled index costs a turn at most the deadline past its
  call), `fact/recall.rs` (the `recall.shadow` row, scoped `recall:<session>`, with references and the query's
  digest, never copies; the `recall` span; the narrative line), `rpc/memory.rs` (`memory.search`, which writes
  nothing, and `memory.recalls`), and `config/memory.rs`. In shadow the model's request is the one compiled without
  recall, and the row rides in the turn's next frame. The filters and pack are `theseus_memory::recall`. Tests:
  `tests_recall.rs`.
  - **Words in time** (theseus-w9qv): a query that asks for vectors goes out twice at once (`recall::race`): as
    asked, and for its word sources alone, which answer in a few ms while the query's embedding may take hundreds.
    The whole answer wins whenever it arrives within the deadline; past it (or when the vector search fails), the
    words' hits rank and pack as usual, the row's `outcome` is `words_only` (`recall::WORDS_ONLY`) with `why` and
    `skipped.vector`. Never past the deadline. Health's memory block counts the last 50 recalls by outcome and the
    last whole answer's time (`recall/outcomes.rs`, in memory since the start; `MemoryHealth.recalls`), and
    `theseus.recall.index_ms` times each recall's wait by outcome. Tests: `tests_recall_words.rs`.
    Since theseus-zo1y the vector source embeds the turn's new text alone (`RecallQuery.vector`, from `query_of`),
    cut by the tender at `[memory] recall_vector_tokens` (32) word pieces, while the words read the longer query; the
    two queries go on one connection, the words' first (`tender/pair.rs`, `IndexTender::query_two`), which closes
    once no one waits (the deadline passed, or the turn dropped its `Begun`, which aborts its task), so the tender
    stops embedding. Tests: `tests_recall_deadline.rs` (a stand-in socket that counts connections).
  - **In front of the model** (step 30b, theseus-6fn.2): `[memory] mode = "canary"` (a sticky share,
    `canary_fraction`, by a hash of session and `experiment`: `MemoryConfig::assign`, recorded once as a `memory.arm`
    row; the control runs `none` live with `baseline` in shadow) or `"live"`. The read finishes before the first
    loop's compile, under the same deadline (`recall_first`), and what it admits is a `Recall` node
    (`node::Body::Recall`: references, never copies) held in the turn (`Turn.recall`) and rendered by the compile as
    though written (`recall_view`, at the position after every node); the new compilation's prefix leaves it out
    (`recall_compiled`), so it renders in the tail as it will once written. It rides the provider call's plan frame
    with a `derived_from` edge to each source (`via = "recall"`, so `node.reach` counts the copy), and its
    `recall.ran` row the turn's next frame: no frame of its own. Its render (`recall/render.rs`) is §2.4's testimony,
    under a preamble that names the harness as its source and no part of the person's message (it renders in their
    user turn, after their words: theseus-fpm2), each item's frozen header and its source's text over the frozen byte
    range, read by position (cached: sources never change), so the next request begins with the previous one's bytes.
    Past `session_recall_cap_tokens` of notes in the tail, recall pauses (`paused`) until the next recompile.
    A trivial detour's request carries no recall (theseus-n7nc): while routing has the route to decide
    (`RouteState::deciding`) the row waits for it, and once it is known `recall_routed` drops a detour's node, its
    rides and its footer count, and the row says `detoured`. Tests: `tests_route.rs`.
    `memory.label` (`rpc/memory.rs`, `judge_act(Act::Label)`, refused by the CLI inside a job) writes a `memory.label`
    row scoped `memory`; `wrong` and `stale` drop a node as `labeled_wrong` from a set built after serving
    (`Core::warm_labels`, `recall/labels.rs`).
    Every compilation carries a `BudgetReport` (`Compiled.budget`, stored on a new `Compilation`; the ring's cut as a
    range, a recall's budget drops, an overage); the reply's `TurnSubmitResult.recalled` feeds Discord's
    `🧠 N recalled` footer. Tests: `tests_recall_node.rs`, `recall::render::tests`.
  - **The `+rerank` arm in shadow** (step 32c): `judge/rerank.rs`. After the shadow recall's pipeline
    (`recall_end`: `[memory] mode = "shadow"`, or a canary's control), the recall step's one call,
    `JudgeService::at_recall`, picks the candidates that passed every filter (`theseus_memory::rerank::eligible`,
    the place rule first, and the operator's labels: only they may reach Jev), marks the turn's trace (`judge`,
    kind `mark`, with the judgment's id), and spawns `rerank.v1` under its own 600 ms deadline (a live-urgency
    call): the reorder, the repack, and a `judge.call` row (scoped `judge:rerank`, context `purpose: "recall"`,
    and `rerank`: the recall's id, both orders' admitted keys, `changed`, `fallback`, the latency against the
    deadline, the cost). The day's limit sends nothing and writes no rerank row. Tests: `tests_rerank.rs` (the
    place test over generated stores reads what the fake Jev was sent).
  - **Rerank live, bounded** (step 32d, theseus-6fn.7): `WIRED` gives `rerank.v1` `Live`. The arms rule: memory's
    mode decides what reaches the model, rerank's whether Jev orders it. A recall in front of the model goes through
    one call, `TurnRunner::recall_reranked` (`turn/rerank_step.rs`; the mode is read before the candidates are
    cloned): live, `JudgeService::at_recall_live` dispatches as `at_recall` does and the turn waits at most
    `[memory] rerank_wait_ms` (200; 1 to 600) from the rerank's start, on a oneshot the turn closes when the wait
    ends (`try_recv` takes what was sent before, so `applied` and `late` never both hold); in time and answered,
    `Memory::refill` packs again in Jev's order. Rerank's breaker open, the day's budget paused, or Jev known
    unreachable (theseus-otny) is read before the dispatch and not waited on. A `judge` span of kind `wait`, the manifest's `rerank`, and the row's
    `live`/`applied`/`late` say what happened. Rerank has a breaker of its own (`rerank::BREAKER`,
    `JevJudge::with_breaker`): its outcomes move only it; the client, its permits and its shed count stay shared,
    and `breaker_status()` is the shared one's. Per-item answers (`helps.3`, `about` the note's key) take labels of
    their own (`learning/items.rs`: the check, the report's per-definition questions), and the owner's memory labels
    write rerank's system labels (`learning/rerank.rs`). Tests: `tests_rerank_live.rs` (one on tokio's paused clock,
    with Jev a channel: `JudgeService::rerank_with`), `tests_rerank_labels.rs`.
  - **FSRS-6 retention and the `+retention` arm** (step 32a's wire-in, theseus-6fn.11): `recall/retention.rs`. Each
    memory row is the `AccessEvent` it is (`event_of`, pure: `memory.labeled` a first sight by durability,
    `memory.used` a use by outcome or, not used, `Shown`, which is no review; `memory.label` by §2.7's table), at
    its row's time. The `Projection` folds them with `Fsrs6` (`fsrs6-default`) per node from every arm's rows (one
    projection, §5 question 9), each node's events kept by position, so a row brought twice counts once and one
    arriving out of order refolds the node: it always equals a rebuild. Built after serving only when read
    (`Core::warm_retention` under `[memory] arm = "+retention"`, the first `+retention` search, or a turn of the
    arm), by one walk of the three kinds through the ledger's index (`k:<kind>` tags; waits on tokio's timer while
    the index's shape is built), on the blocking pool; kept current by `Memory::retention_written`, which the memory
    pass's frames and `memory.label` call with their positions. The arms' seam is `Memory::science_for(arm)` (a
    match: an arm that ranks its own way adds its line) and `Scene.science`, which `manifest_ranked`, `refill` and
    the rerank's `Recalled` (with its `retention`) read; shadow, a canary's control and a search without an arm use
    `baseline`'s. Under a science that reads retention the manifest's `retention` says the projection's state
    (`ready`, or why it ranked without), and each admitted and dropped item its `RecallRetention`. `memory.search`
    takes `arm`; health's `memory` block and the gauge `theseus.memory.retention.nodes` give its size. Tests:
    `tests_retention.rs`, `recall::retention::tests`.
  - **The adjacency projection** (step 32b, theseus-6fn.12): `recall/adjacency.rs`, what spreading activation walks,
    folded from the record and never stored: neighbours by position in a session (a `Recall` node is no one's
    neighbour), a tool call and its result by `tool_use_id`, the EDGEs by kind and route (`mapped`: `derived_from`'s
    copying routes, `recall`'s at weight zero by construction, a route it does not know, such as 31b's `synthesis`,
    spreads nothing and is counted `unmapped`; `supersedes` 1.0 toward the newer node, 0.2 back), and each node's
    entities from its `memory.labeled` row, expanded at a spread from each entity's list, never stored per pair
    (`1/ln(1+df)`; an entity in more nodes than `cap` is not expanded: at the defaults, 1,095, past which one such
    edge cannot carry a seed over the threshold alone). A node the memory pass never labeled has no entities.
    `Projection::refresh` folds what was written since; the result does not depend on where a fold stopped. Tests:
    `tests_activation.rs`.
  - **The `+activation` arm** (step 32b): `recall/activation.rs`. `[memory] arm = "+activation"` (canary or live;
    shadow and a canary's control run `baseline`) asks the index for `baseline`'s sources, then, before the pipeline,
    spreads (`Memory::activated`, theseus-memory's `Activated` science, which is `baseline` but for `activate`): the
    turn's new node at 1.0 (`Begun.new_node`; its projection edges and the query's entities the hits matched, the
    tender asked nothing more) and the top 10 fused hits over the best one's score. The reached nodes are one more
    ranked source: a reached hit gains `weight / (60 + rank)` and `sources.activation`, and at most 20 of the
    strongest the index did not return (none the turn holds, none at or after `as_of`, none the index never indexes)
    join the candidates read from the store, before every filter (the place rule reads them as any candidate). The
    spread runs on the blocking pool for what is left of the index's deadline (`deadline`); the projection is built
    after serving (`Core::warm_activation`), and a turn that finds it unbuilt starts the build and goes on
    (`building`); a search builds it itself, unpaced (`Adjacent::paces` counts every build's paces), unless the warm
    build is running: then it answers `building` at once, never queued on the projection's lock behind the warm
    build's paces (theseus-e21m, theseus-6fn.14; `tests_activation_search.rs`). The seam other arms share: `MemoryArm`, `Memory::science_for(arm)`, the
    `Scene`'s `science` (read by `manifest_ranked`, `refill` and the rerank's `Recalled`), and `memory.search`'s
    `arm` (`theseus memory search --arm`). The manifest's `activation` says what it did and its share of what was
    admitted (each item's rank and score are its `sources.activation`); the `recall` span holds a `recall.activate`
    span, timed in `theseus.recall.activate_ms` by outcome, with its additions in `theseus.recall.activated`
    (`telemetry/tests_recall.rs`); health's `memory` block (`Core::memory_health`) names the projection's nodes,
    edges, entities and bytes. Tests: `tests_activation_arm.rs`.
  - **Consolidation and the `+synthesis` arm** (step 31b, theseus-6fn.10): `consolidate/`. Off every turn
    (`memory.consolidate`, routed with the owner's runs in `rpc/judge_runs.rs`, `judge_act(Act::JudgeRun)`; the
    nightly tender `consolidate/tender.rs` at `[memory] consolidate_hour`, never within 10 minutes of a start;
    the plan on a `learning` thread at nice 19, the calls and the frames' waits as tasks on the runtime, never a
    thread blocked on it: a stop mid-wait must not panic it, and the release profile aborts on a panic), the newest `recall.shadow`/`recall.ran` rows (by kind through the
    store's pages) become clusters (`theseus_memory::consolidate`), each written by `synth_profile` (`session`:
    the profile every source's session last used; disagreeing sources wait), checked deterministically and by
    `citation.v1` (`judge/citation.rs`, a point of its own, `WIRED` in shadow) on the answer's entry
    (`theseus_memory::consolidate::entry`, a leading heading set aside: the node keeps the entry,
    `synthesis.proposed` the answer whole, `synthesis.checked` the `heading`; a cluster whose one answer was rejected
    for its form, `rejected` with no `judgment`, is proposed once more on a later run, `FORM_TRIES`), and kept as a `Body::Synthesis`
    node (store format 18) in the harness session (META `memory.session`, never compiled) with `derived_from`
    edges (`via = "synthesis"`) and `synthesis.proposed`/`.checked`/`.scored` rows (`fact/synthesis.rs`, scope
    `memory`; the day's spend reads back from them). A cluster with external text is never synthesized. Each
    frame goes through the memory pass's writer handshake (`MemoryPass::writing`; `memory_pass/turns.rs` counts
    its writers). The arms' seam: `Memory::science_for(arm)` and `Scene.science`; every arm but `+synthesis`
    leaves the harness session out before the index's top k (`Memory::begin`'s `exclude_sessions`), and the
    pipeline drops a synthesis as `arm` or `unchecked` (`MemoryScience::synthesis`). Tests: `tests_consolidate.rs`.
- **The import** (theseus-0lrr.6, the soul migration): `import/`. An outside pipeline's episode files (JSON Lines,
  format 1: `import/episode.rs`, every field from the format's lists and the hash over Python's canonical JSON, both
  string forms, `py_float`) become **imported sessions**: a `SessionRecord` with `imported` (`ImportedFrom`: tag,
  episode, hash, source, agent, place and labels as recorded facts, triage, as-of; store format 23), its id the
  episode's (`ses_ep<64 hex>`, `session_id_of`), its record scoped `import:<tag>`. It has no execution, and
  `turn.submit` refuses it (`import::refusal`), so it never takes a turn or reaches a compile but as recall's
  testimony; `session.list` leaves it out unread: the whole list, `confirm.list` and `compilation.list` read
  `Store::live_sessions` (theseus-store's `latest_of_kind_except`: two reads of the key table either side of the
  `ses_ep` run, so no imported row is visited, theseus-26jo; one session's `compilation.list` reads its record
  alone), and a page and the learning tender's task-brief walk (`sessions_from`) step over the import's births by key in
  their walk (`newest_keys_where`, theseus-7087); health takes the tags' counts (one META record a
  tag) off the projection's keys, and its fallback reads every session, to count the owner's own, the imported and the
  erased apart (theseus-revl). Its place is private whatever place
  the episode names (`TurnRunner::place_of` reads the id), so a shared place never recalls it. Each message is a
  node of origin `import` (`Body::Imported`: source, unit, sha256, integrity; `created_at_ms` the message's own
  time), the summary a `Body::ImportedSummary` citing its messages' ids; outside integrity is external to the index,
  so recall keeps it out (`untrusted`) unless `[memory] include_external`, and its header names it outside text.
  `import.episodes` (`import/write.rs`) takes one batch of a file's lines in one frame (cut past 4,000 records or
  8 MiB), with the tags' counts (META `import.tag.<tag>`) and an `import.batch` row, on the blocking pool, one import
  or erase at a time; an episode imported before is skipped, the same id with another hash rejected, an erased one
  not imported again, and a line that does not read named by its number. `import.erase` tombstones a tag (each node
  written again under its id, origin and time with `Body::Erased`, the session's `erased` receipt, an
  `import.erased` row), then asks a running tender's `index.forget`. The tag's counts ride in every frame of a batch
  or an erase, and an erase's count of the tag's erased sessions is absolute, counted from the tag's records as it
  reads them: a reader lags a frame at most, a kill between frames leaves the counts whole, and the next erase counts
  a cut run's tombstones too (theseus-mce3). The follower drops a tombstoned node as it
  reads it, at a rebuild too; `session.history` and `node.list` show an imported node by its newest record
  (`import::shown`), so an erased one is its tombstone. The WAL's earlier frames still hold the payloads: §5.6's
  erasure in place is not built. `import.list` reads the tags' META records. The owner's acts (`judge_act(Act::Import)`; the CLI's
  `OPERATORS`). `theseus import openclaw|erase|list|topics`. Tests: `import/tests.rs`, theseus-index's `tests_import.rs`.
  **Its topics** (theseus-anh3): `import.topics` (`import/topics.rs`, the owner's act, never at a start) reads a tag's
  sessions' stored `labels.topic` (slash paths) and writes, through the ontology's `Board::write` a frame at a time
  (cut past 4,000 records, quiet waited for between, the stop looked for there): a topic for each label and each
  prefix (a part found by its name under its parent, so the operator's own topic is used; a new one's id the path's
  slug, `added_by` `import <tag>`), and each live imported session's topic list, origin `import` (the topic kind's
  seed row names it), at most the kind's 3: an ancestor of another label dropped first, then the pipeline's first
  three; the operator's own memberships kept and counted first; a list that holds the same topics from the same
  origins is not written again (idempotent), and an `import.topics` row per frame. `import.erase`'s second half
  (`topics::unassign`) empties the erased sessions' lists, then retires each topic an import made that nothing uses
  (`Category.retired_ms`: no child, membership, or guidance; the snapshot holds none for it, a load skips it), the
  deepest first. `ontology.list` counts each category's sessions (`members`) and leaves the memberships out on
  `memberships: false` (the cockpit's page and the CLI's tree). Tests: `import/tests_topics.rs`.
  **Its people** (theseus-wy7y): `import.people` (`import/people.rs`, the owner's act, `--dry-run` counts) reads a tag's
  live sessions' `person:<name>` authors (from their nodes) and each DM's other party (its place's id: digits are
  `discord:`, else `slack:`), one person each (a name that spoke alone in one party's DMs is that party), found by an
  exact handle first (`Ontology::person_by_handle`; a DM's person holds its `discord:<id>`), never by a display name
  across makers; an author or party the proposals' exclusions exclude (`Core::not_people`, theseus-0p1r) is never a
  person (counted in `excluded`); each session's person list, origin `import`, at most `PER_SESSION` (12). The erase empties every
  stored kind's lists (`Kind::stores`) and retires the import's people nothing uses. The person kind is given with a
  stored side (`theseus_ontology::person`): the operator and the import declare people and keep stored lists, never
  the transport; `Walk::of` takes a stored person the place also gives once; handles, merges and their undo are
  `rpc/people.rs` (`ontology.person.merge`, the `ontology.merged` row holds what moved; a DM's first bind whose id
  another person holds merges that one into it), with `ontology.proposal.accept_all`, the bulk yes. Tests:
  `import/tests_people.rs`, `tests_people_proposals.rs`, theseus-ontology's `tests_people.rs`.
- **The books, first cut** (theseus-civ0): `books/`, `rpc/books.rs`. `books.list` (each of the seven books and
  `unsorted` with its count and its episodes' first and last start) and `books.page` (a book's episodes newest first,
  by cursor, filtered by topic, source and place, its facets on a first page), read only, from the import's labels
  (`labels.book_hint`; none, or one not of the seven, is `unsorted`). The index keeps an imported session's book
  terms in the store's projection (`books::session_terms`, in `store::PROJECTION`, renamed `projection.core.2` for
  them): `bk:<book>` (the count), `bt:<book>␁<start hex>` (the order), `ft:`/`fs:`/`fp:` the same per topic, source
  and place, and `ct:`/`cs:`/`cp:` the facets; an erased session has none. So the list reads no record, and a page
  a range of terms (theseus-store's `terms_range`, `term_counts`, `terms_of`) and a session and a summary per
  episode shown; a filtered page checks its other filters by the key's own terms and stops past `SCAN_MAX` (5,000)
  looked at, with a cursor. Until the terms are whole (`build_terms`, after serving) both read every imported record
  (`books::Walked`) and say `indexed: false`. Asked on a surface that reads no private text (only the CLI and the web UI read it, as for
  `import.sessions`) or for a shared place (`session_id`), an episode's text (summary,
  topics, place, partner) is withheld and no filter or facet reads it, as recall keeps the import from a shared
  place. An episode's messages are `session.history`. Tests: `books/tests.rs` (the measure over 20,000 is ignored:
  `--ignored --nocapture`).
- **The daemon's resident memory** (theseus-9lxe): `resident.rs`. glibc keeps an arena per thread that allocated at
  once, and a blocking-pool thread that built something large (`import.sessions`' catalog, a full `ontology.list`, the
  books' terms rebuilt, an import's batch) leaves its freed pages held: over 21,779 imported sessions a daemon held
  202 MiB from the system with 60 in use. Every method's answer and each background build's end marks work
  (`Resident::mark`); after `QUIET` (10 s) with no other mark, and never later than `MAX_DEFER` (60 s) after the first
  (the cockpit reads `health` every 2 s while in sight), the tender trims (`malloc_trim(0)`) when the free heap passes
  `TRIM_FLOOR` (8 MiB). `import.sessions`' catalog (`import/catalog.rs`) is built a page of records at a time, under a lock of
  its own (health and the tender read its state on a runtime worker, never waiting for a build),
  holds each episode compactly (its repeated values interned; `Catalog::episode` makes a page's rows whole), and is dropped after `CATALOG_IDLE` (10 min) with no read; the next read builds it again (the Context page reads it
  every 30 s while open). The tender starts after serving (`tend_memory_after_serving`), waits on a `Notify` and
  tokio's timer, holds the core by `Weak`, and trims and drops on the blocking pool. Health's `resident` block: the
  resident set, the heap in use and held (`mallinfo2`, as the tender last read it: it walks every free chunk under each arena's lock, so health never calls it), the trims, and the caches by size. A musl build reads no heap
  and trims nothing. Tests: `import/tests_catalog.rs`, `resident::tests`.
- **The arrangement** (M5 step 27, theseus-vug.2): `arrangement.rs`. `task.create` needs an `arrangement` of quoted
  pieces (`{quote | node, role}`, `trust`, `supersedes`), resolved in the calling session's own transcript (exact,
  whitespace runs as one space, at least 20 characters, exactly one node; the reply holding the call and earlier
  `task.create` results are no source), refused without a standing `objective` or `design` piece, and held by the
  fidelity check (a brief under 200 characters, more than ten operator messages since the last task, one piece)
  unless `fidelity_ack`. The pieces go into the child as one `Body::Arrangement` node after the brief, in
  `open_task`'s frame, with a `derived_from` edge (`arrangement`) to each piece's node, and carry the text the
  compiler renders after the brief (a superseded piece by reference only). The session's `task.arrangement` names
  the node, which `task.list` reads; the rows are `task.arranged` and `task.arrangement_refused`
  (`fact/arrangement.rs`). A scripted `task.create` in a test needs an arrangement whose quote its parent's
  transcript holds once. Tests: `tests_arrangement.rs`, `arrangement::tests`.
- **Check tasks** (M5 step 28a, theseus-vug.3): `check.rs`. `task.create { check_of, profile? }` opens a task that
  checks another by its claim: `check_of` resolves only among the tasks the calling session started, and a task
  with no report is refused (`task.check_refused`). Its `Arrangement` node carries the checked task's standing
  `objective` and `acceptance` pieces (which stand for its own under 27's rule; the fidelity check does not apply),
  any pieces of its own, and the report as a `claim` (`Body::Arrangement.claim`, rendered by `check::render`), with
  a `derived_from` edge to the report (`VIA_CLAIM`). The exclusion is enforced on what the parent writes: an own
  piece that copies the report reads as the claim, one deriving from any other node of the checked session is
  refused. The overlap flag (`OVERLAP_WORDS`, 12, words as runs of letters and digits without case) compares the
  brief and own pieces with that session's nodes but its report, brief and arrangement. The basis is
  `TaskOf.check` (`theseus_protocol::TaskCheck`, store format 14), the row `task.check_opened`, and its line on
  `task.list`, the report's post and node. `profile` is a check's alone (`ToolRuntime::profiles`). Tests:
  `tests_check.rs`, `check::tests`.
- **The task graph** (M7 step 39a, theseus-ext.6): `task_graph/`. A task is a record of the store's `TASK` kind
  (`theseus_protocol::tasks::TaskRecord`, stored as shown; `tsk_…`, a task session's id sharing its session's tail).
  `task.create` without `brief` records a plan item (no arrangement needed); with it, 27's rules hold and the record
  rides `open_task`'s frame. `task.update`, `task.split`, `task.close` (`task_graph/tools.rs`) run in the harness under
  `Store::lock_task` (the order: a session's lock, a task's, an execution's), compare the `version` the call names
  (stale: refused with the record as it is now, `task.stale_refused`), and write the record in the frame that settles
  the call. Layer 1 (objective, acceptance, abandoning) plans with `Plan::authority`, so the gate asks at every posture
  as the floor does: its proposal is written on the record in the asking frame, the yes applies it, a no clears it in
  the answer's frame (`confirms.rs`). A task session's running state is read from its execution (`state_now`), and its
  report closes its record in the frame that ends it (`tools::Closing`, `turn.rs`). The view (`task_graph/view.rs`)
  is the last block of a request's last message, the conversation's breakpoint on the block before it; a test's
  stand-in that reads the last user text skips it (`view::is_view`, `view::HEAD`). Facts: `fact/task_graph.rs`.
  Tests: `tests_task_graph.rs`, `task_graph::tests`.
  - **Claim leases** (39b, theseus-ext.14; `task_graph/lease.rs`, store format 19): `task.claim { id, version }` sets
    `claim { by, session, until_ms }`, `[kernel] task_lease_minutes` ahead, and moves the version; the holder's
    edits and its claim again renew it without a version of their own, and a close ends it. A held task refuses
    another's claim before its version is compared (`blocked: claimed by session … until HH:MM`); another's edit is
    not held back. The due pass (`Core::free_expired_leases_if_due`, the driver's tick) frees a lapsed claim from
    `ToolRuntime::leases`, the claims kept in memory (built after serving by `Core::warm_leases`, noted after each
    edit's frame), never a scan a tick: the record and `task.lease_expired` in one frame, the version kept. A claim
    past its `until_ms` reads free before that (`claim_at`, `shown`). A change's board is its home's place
    (`task_graph::home`). Tests: `tests_task_claims.rs`.
  - **The layer-1 question's change** (39b): `ConfirmRequest.change` (`tools::change_of`: the task, its title, the
    field, before and after), worded once by `TaskChange::question` for the Discord card, `theseus confirm`, and
    the cockpit.
- **Parked tasks** (28b; theseus-vug): `parked.rs`, health's `tasks.parked`: each task that cannot progress by itself,
  with its blocker, from the open executions alone (a question counts as progress for 24 hours).
- **Limits that notify** (theseus-usei; the owner, 2026-10-07: for tasks and tokens, notify, not restrict):
  `turn/budget_step.rs`. With `[kernel] spend_limit_mode = "notify"` (the default) a provider call past the
  session's limit goes out (`Kernel::overdraws`, theseus-kernel's `overdraw.rs`), and the loop whose call took the
  spend past the limit, or a multiple of it (`crossed`), posts one `notice` to the session's place, a
  `budget.reached` row and a narrative line, all in the turn's next frame; `[profiles.*] max_loops_mode = "notify"`
  does the same at `max_loops` and each multiple (`loop.cap_reached`), and the turn goes on. `"ask"` and `"end"`
  are the old behaviour. Still asking: a pinned limit (a place's ceiling, an MCP client's), an AWS hands group, and
  the background day caps. Under notify a task's carve is what it asked for up to the parent's whole limit, and
  speech and transcripts are not refused for the limit. The budget question's words are `turn/budget_words.rs`.
  Tests: `tests_limits_notify.rs`, and the kernel's `tests_overdraw.rs`.
- **The day ceiling** (theseus-kp20; the kernel's `day_ceiling.rs` is the counter): `day_ceiling.rs` reads today's
  model spend back before serving (`read_today`: the ledger's pages by kind from local midnight, provider settles,
  booked reservations, speech, synthesis, the learning runs, the judge's day record), and says a refusal
  (`TurnRunner::day_refused`: the day's first writes one `spend.ceiling` row and, with a DM bound, one
  `spend_ceiling` post to the owner's DM, which the courier sends nowhere else; the start reads the row back). A
  turn's refused call fails it, class `daily_ceiling`, and leaves a harness note the next context reads
  (`turn/ceiling_step.rs`); the judge's day budget holds on it (`ShadowBudget::set_ceiling`), and consolidation, the
  audit, replay, backfill and the learning writer hold through `TurnRunner::day_hold`; compaction's summary is refused
  through the kernel and the ring runs; speech asks before its call. No mode: it stops. Tests: `tests_day_ceiling.rs`,
  and one each in `tests_compaction.rs` and `tests_consolidate.rs`.
- **The operator's two reads** (step 42a, theseus-ext.7): `rpc/budgets.rs` is `budget.list`, each open execution's
  money from its record (`Kernel::open_executions`), where its limit comes from (`config`, `place`, `carve`, or
  `pinned`), its session's lifetime cost, and its last reset from one small ledger page by kind and session tag,
  never a scan (`last_reset_unread` while the index's shape is built); tasks under their parents, and totals of the
  top rows, since a task's spend is its parent's too. Tests: `tests_budgets.rs`. `rpc/explain.rs` is
  `policy.explain`: for a session's place (or the CLI and every bound place), each tool's layers in the gate's
  order, built by running the gate's own order (`toolrun/order.rs`: `ToolRuntime::refusal`, then
  `ToolRuntime::order`, which hands its watcher the decision after each layer) on a call inside the roots; the
  call-dependent layers (the floor, the lists, the roots, a private address, AWS, L1, grants) are conditions with
  their entries. Never copy the order into it: change `order.rs`, and both follow. Tests: `tests_explain.rs` (every
  tool against the order on a real plan, and against whole turns' recorded gates).
- **`store.rs`** is the kernel's view of storage: `Store::for_turn`, a session's writes, and the turn's transcript.
- **Tiering** (M6 step 33, theseus-6fn.13): a turn decodes only what it renders.
  - `node_cache.rs`: the heat cache, `Arc<Node>` by WAL position, one per store and shared by every handle, so every
    reader of nodes (a turn's transcript, `session_nodes`, `get_node`, a recall's source by `Store::node_at`) decodes a
    node once. Bounded by `[memory] node_cache_mb` (64; 0 off) in record bytes; past it, `decay_sweep`'s hints go
    first, then the coldest by last touch and count. It is the store's read path: it serves with memory off. Health's
    `store.node_cache`, the metrics `theseus.node_cache.*`.
  - `stub.rs`: `Transcript` is `Vec<(u64, Stub)>`. A stub has its record's id, kind, origin, turn, and a summary's
    range's end from a peek that skips the payload; it derefs to its `Node`, decoded at the first touch (or taken from
    the cache). So **a walk over a whole transcript reads stub fields** (`n.kind`, `n.id`, `n.origin`, `n.turn_id`,
    `n.summary_last`, `compiler::renderable`, `compaction::is_summary`) **and touches `n.body` only for the nodes it
    needs**, else it decodes the session. A closure typed `|n: &Node|` over stubs derefs each one: type it `&Stub`.
    `context.compiled`'s `decoded` and `stubs` say what a compile read and what it left.

## Where the big things live

- **Kernel transitions and frames**: `crates/theseus-kernel/src/kernel.rs`, and `tx.rs`. Every mutating method writes
  one frame.
- **The store's WAL and index**: `crates/theseus-store/src/wal.rs`, `index.rs`, and `store.rs`.
- **The turn loop**: `crates/theseus-core/src/turn.rs`, with `advancer.rs`; the harness loop in `harness.rs`, and
  what it drives (continuations, the heartbeat) in `rpc/driver.rs`.
- **The compiler**: `crates/theseus-core/src/compiler.rs` and `context_files.rs`.
- **Tool calls**: `crates/theseus-core/src/toolrun.rs`; the gate's postures in `policy.rs`, `external.rs`, and
  `broker.rs`.
- **The RPC layer**: `crates/theseus-core/src/rpc/` (`server.rs`, `methods.rs`, `driver.rs`, `confirms.rs`).
- **Facts**: `crates/theseus-core/src/fact/`: every ledger row, notification, narrative line, and span of the turn,
  its tool calls, answers, and the driver, one type each, recorded once (theseus-j6qn). The ledger's kinds:
  `crates/theseus-protocol/src/ledger.rs`.
- **The push's board**: `crates/theseus-core/src/push.rs`, fed by `Kernel::observe`; `attention()` is in
  `crates/theseus-protocol/src/push.rs`. A view's `outstanding` counts tool calls only, never the model's own
  (`tool_calls`, as `park` counts them), and `same()` compares it; a queued view keeps its `why` until it leaves
  `queued`; `confirm.list` lists a question from its plan on (theseus-q5af).
- **The outbox**: `crates/theseus-kernel/src/outbox.rs`, `crates/theseus-core/src/outbox.rs`, and
  `crates/theseus-discord/src/courier.rs`.
- **The config**: `crates/theseus-core/src/config.rs`, and the template `crates/theseus-core/config/theseus.example.toml`
  (`theseusd example-config` prints it); `config/sparse.rs` cuts a note to what differs from the defaults.
- **L1**: `crates/theseus-core/src/sandbox.rs` (`[sandbox]`, the class, the last launch), and the wrapper's L1 path in
  `crates/theseus-kernel/src/job_l1.rs`.
- **The index tender**: the binary in `crates/theseus-index`, its supervisor in `crates/theseus-core/src/tender.rs`
  (started 2 s after serving, restarted with backoff, SIGTERM at a stop), and its child kind in
  `crates/theseus-kernel/src/children.rs`.

## Invariants

- **The frame budget.** A plain one-loop turn writes 5 frames, and each loop with one in-process tool adds 4.
  Observability rows ride in the turn's next frame; the session's write rides in `end_turn`'s
  (`Store::defer_session`). `tests_m3::a_plain_turn_stays_within_its_frame_budget` fails a sixth frame.
  Each turn counts its own (theseus-wz4y), as `frames` on its trace's root span: its admission's frames, read from
  this thread's count around calls with no `.await` (`theseus_store::frames_written_here`, since the store's writer
  thread writes them), its handle's (`Store::turn_frames`), and its last, which carries the trace. A frame written for
  the turn by another path is counted to it (`Store::count_frames`): a job's completion the spool's drain accepted.
  `tests_m3::frames_counted` and the turn bench (`theseus-sim bench turn`, against the WAL) hold it.
- **A turn has one exit after it begins** (R1). `run_inner`'s body is `turn_body`, and its error goes to `fault`,
  which closes the books as `fail` does (class `internal`). A new `?` in the body or in `finish` lands there; never
  return an error from a turn by another path.
- **Lock order**: a session's lock, then an execution's (`Store::with_session`, `update_session`). Both belong
  to their OS thread, so `SessionLock`, `SessionHold`, and the kernel's `ExecLock` are `!Send` (Review 2's R7):
  holding one across an `.await` in a spawned future is a compile error, and a build-time check beside each fails
  the build if one becomes `Send`.
- **Kernel writes made together are one `Kernel::frame`**, with the core's own records added by `stage`: an answer
  (the bind or the decline, an approval's trust, its row, and the wake) is one (theseus-jj9f). A transaction holds
  its executions' locks, which belong to an OS thread, so it never spans an `.await`; a session's lock goes around
  it, never inside.
- **Serve first.** Nothing new on the start path waits on the network or writes; it goes after serving. Secrets
  resolve behind the socket, and each consumer waits for its own.
- **Where work runs.** An in-process toollet computes on the CPU pool (`cpu.rs`, a permit per core). A tool that
  waits on the network is `Backend::Async`, a future on the runtime. A verb over Theseus's own state is
  `Backend::Harness`. Never block a runtime worker on a sleep or a blocking wait.
- **The gate never refuses** and never parses what a command does. The order is the floor, the approve lists, the
  allow list, then the posture; a granted secret's posture and the external-text hold apply after it, and the
  stricter wins. The whole order after the place's refusal is written once, in `toolrun/order.rs`, which the gate
  and `policy.explain` both run.
- **An L1 call runs at notify** (the owner's decision, 2026-10-02): none of the L0 order applies, since the view hides
  the floor, the approve list's paths, and the socket; the external-text hold still does, and so does the
  operator's own word about the tool: a `[policy.tools]` line or a tightening that asks makes it wait
  (theseus-jfs6), and so do hosts its call names beyond `[sandbox] egress` (18c), and a granted secret's posture,
  as at L0 (theseus-w5op). The inherited
  `[policy].enforcement` never does. Its proposal names its class, so a confirm binds it, and a confirmed call runs in the class its
  proposal names. Nothing falls back from L1 to L0.
- **Results tell the truth.** `toolrun::cap` cuts on line edges and says what it left out, with the tool's own way
  to get the rest (`Tool::rest`), or the kept file and its lines (`toolrun/kept.rs`). A listing names its scope.
- **A kept output is scrubbed, never raw.** What a job printed before the scrubber saw it stays in the spool only
  until its result is written; a copy anywhere else goes through the scrubber first.
- **Thinking goes back only to the provider that wrote it**, and every recompile strips the prefix's thinking.
- **A shared place never receives the owner's material** (the place rule). Its model is offered only the public
  tools, the gate refuses anything else, and its system block carries only public context files. A task takes its
  parent's class. Nothing flows from a private place to a shared one but what a person says there, and the owner's
  publish. A class is fixed for the turn, as the spec is, so a change applies at the next turn's first loop.
- **Nothing retractable goes in the shared header** (Appendix F), so sessions on a profile share one cache entry.
- **The config template is the contract.** Change a default there, not only in code. The loader rejects unknown
  keys, and `example_template_uncommented_still_parses` un-comments every line and parses it.
- **Approvals come from private places** (the place rule, theseus-zmgb). An answer, the undo of a tightening, a
  trust, and a publish count only from the CLI, the web UI, a DM with the owner, or a channel bound `private = true`,
  and only from the owner (`[places] owner`, else the person of each DM the bindings file binds):
  `places::owner_in_private`. A shared place's cards go to the owner's DM. `[approval]` is retired: a config that
  has it loads with a warning, and nothing reads it. A test's bare label answers as the CLI (`From<&str> for
  Answerer` is test-only).
- **A result that is outside text writes its session's hold in its own frame** (theseus-9bp): `external::with_hold`
  for a result written alone, and `external::under_hold` for a frame that may carry one (a late result, the cancel's
  sweep), which is built first without the session's lock and again under it only when it does. A job of a program
  `[policy] external_programs` lists is outside text too (`external::Listed`, read from the call's input by every
  `job_result`), and a session opened, or sent a turn, with `opened_from` a holding session takes its hold in the
  frame that writes it (`external::from_job`; every job's `THESEUS_SESSION`). Both are light guards under default
  trust, not boundaries: a job can strip its own environment (theseus-b5cl).
- **Secrets**: no value in a log, a row, a node, a result, or an error. `scrub.rs` scrubs tool output: each board
  value verbatim, in base64, percent-encoded, and with any of its characters JSON-escaped (`scrub/escaped.rs`:
  the text decoded once where it holds a backslash, a match mapped back to the escaped span; theseus-ubp7), twice
  for JSON inside a JSON string, YAML's and Python repr's escapes among them (a run of `\xNN` that is one character's
  UTF-8 read as that character; theseus-nlvx), and base64 in a decoded text where an escape joins it (`\n` every 60
  characters in a JSON string; theseus-cjyt), and
  the shapes of secrets never resolved here (token prefixes, AWS keys, private-key blocks, JWTs). The broker hands a value only to the program it is granted to, run by its
  own argv, and never to one the call could make it run (`broker::launches`: the call's own environment, gh's
  aliases and extensions, git's aliases, `-c`, and the programs its options and URLs name), in L1 as at L0.
- **The AWS keys and the providers' keys are harness-only** (theseus-gh7): Theseus's own tools read them from the
  board (`aws/`, the providers), and no job is handed one unless `[broker]` names it (`Broker::may_hand_out`). `broker::harness_only` is the line `theseusd check` and health print, naming them; the
  template test holds it (`the_templates_harness_only_keys`). A program granted `aws_account` gets a short-lived
  job session at launch (`Broker::for_job_of`, `aws::Account::job_session`), never the key, and none before the
  owner role exists. Containment is that, the operator's `[broker]` grants,
  and the egress list: credentials as stand-ins (TLS interception at the proxy) were dropped for v1 (the owner,
  2026-10-03). Do nothing heavier without his say.
- **One fact, recorded once.** A new ledger row, notification, or narrative sentence is a fact's projection in
  `fact/`, not a hand-written channel at its site. Recording writes no frame: its row rides in the turn's next frame
  (or a frame now, outside a turn), and a row that must ride in a frame the site builds is `Rec::row`, with the rest
  announced once that frame is written (`Rec::announce`).

## Tests

- In-process suites in `src/`: `tests_m3.rs` (turns through the whole core), `tests_continuations.rs`,
  `tests_failures.rs`, `tests_tasks.rs`, `tests_wakes.rs`, `tests_external.rs`, `tests_overflow.rs`,
  `tests_push.rs`, `tests_config.rs`, `tests_books.rs` (a fault at each exit after a paid loop still closes the
  books, R1), `tests_refused.rs` (a corrupt record skipped, counted, and the driver still continuing, R4), and
  `rpc/tests.rs`.
- `tests_registry.rs` is the reader rule's test. The gate runs it alone, before the suite. A `tool` marker is held
  to the install list, read as text (`scripts/build.sh`'s `shipped`, and `scripts/setup.sh`'s `SHIPPED`, its copy);
  a tool no install ships says why beside it (`run_from_tree`, theseus-exam's). A use of `EdgeKind`, `Event`, or
  `LedgerKind` counts only where the type named is its home's (`Home`): a path that resolves there
  (`graph::EdgeKind::X` after `use crate::graph;`), or a bare name in a file that imports the home's type by name
  and no other of that name; theseus-memory's own `EdgeKind` reads nothing of ours (theseus-g7qp).
- `tests_layouts.rs` holds the store's version rule (P5b; theseus-ptx1): one table of every old record layout on
  disk somewhere, each a literal its build wrote (the 460a35b fixture's records, and each layout since). Each must
  decode, keep every field but the ones it names, and survive a round trip. A step that adds a field to a stored
  record bumps the store's format and adds the layout it replaces there.
- `tests_output.rs` is the output golden (theseus-j6qn): scripted scenarios through whole cores, and every frame,
  notification, and narrative line they produce, against `tests/golden/core_output.txt`. It pins shapes, not
  numbers (theseus-ptx1): every number and digit run is `#` (an id's alias keeps its own), so a prompt, a token
  count, or a cost moves none of it; the money, telemetry, and frame-budget tests hold the numbers.
  `THESEUS_GOLDEN=write` rewrites it, for a change you mean, and the diff is the review. A narrative line whose
  presence the machine's load decides is left out there (`by_the_load`); a new one joins it.
- `tests_outside_text.rs`: property tests over every reader of outside text (the HTML reader, the wake's time
  parsers, the provider's SSE lines, and the scrubber, which also never lets a planted value through), with a fresh
  seed each run, so the gate keeps looking (Item 13). The release
  profile aborts on a panic, so one panic on outside text takes the daemon down. A new reader joins them.
- `FakeProvider::scripted` scripts a provider's answers. `Parts::for_tests` builds a core around it, and a second
  provider can be inserted into `parts.providers`.
- `Store::fail_turn_frame` (test-only) fails the first turn frame whose records match, once, as a full disk would.
- Debug builds assert, at every read, that a turn's kept transcript equals a fresh read of the store, by positions and
  record keys, decoding nothing. A node written past the turn's handle panics a debug daemon.
- `tests_tiering.rs`: the heat cache and stubs through whole turns (decodes fall, every request the same with the cache
  off, the debug check through a ring and a recompile, a recall source from before a floor); `node_cache::tests`
  holds eviction by heat under the bound (a property test).

## Traps

- Every child process the core starts (an `op`, a job's wrapper) goes through `theseus_kernel::children::spawn`, or
  the daemon's reaper can take its exit status (`ECHILD`).
- A static or a detached thread holds the core or the store by `Weak`. One that held an `Arc<Core>` kept the store
  open past a clean stop, and every start then paid redb's repair.
- A recompile inside a tool loop strips thinking between a call and its result, so context files change on the
  next turn's first loop, never mid-turn.
- **Every `tool_use` gets a result, whoever ends its execution.** A cancelled execution takes no more turns, so
  `answer_after_cancel` writes what its calls left unanswered (the cancel's handler, and the end of the turn that held
  the execution, whichever runs later; each call is answered once). A call that may have run is `unknown`, never
  "not run". A late completion after a cancel drops the node that rode with it (`accept_completion_with` writes
  `extra` only for a settle): the turn that holds the result writes it.
- **A reset never frees held money.** `Budget::available_after_reset` is what a reset leaves; a call over it cannot be
  answered by a question, so its question says so and the retry's refusal fails the turn (`over_limit`), never asks again.
- **The take frame keeps what its retry needs.** A turn that takes a wake or a report keeps where its reply goes in a
  META record written in the taking frame (`wake.target.<session>`); a retry frames its reply from the unanswered
  wake and report nodes. No in-memory field carries it.
- **A `/stop` cuts the model's stream** (theseus-yey): `Core::stop_execution` signals `TurnRunner::stops`, which only
  wakes `call_model`; the kernel's record of the stop decides. The cut call settles as failed at an estimate (the
  input its reservation assumed, the output from the characters streamed), never held unknown, and a `provider.cut`
  row says it is an estimate. A stop that lands after the stream ended keeps the answer and runs none of its calls.
- `cargo test -p theseus-core` builds a second copy of every dependency. Use
  `cargo nextest run --workspace -E 'package(theseus-core)'`.
