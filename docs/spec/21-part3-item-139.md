# The Ship of Theseus, chapter 21: Part III, A4's Items 139 to 148 ([index](README.md))
### Item 139. `route.v1`: Jev picks the model for each message by its interaction mode, live at inbound, waited for at most 200 ms beside the first compile, with detours and switches; store format 15 (theseus-0j2.11; step 25e, the batch-5 manifest's row R1; the fifth cloud batch's route session, fired 2026-10-04 11:14 from 802f9135, Opus; a2063729 (the task) and 7cf6f816; reviewed 14:06 to 14:46 by the local reviewer R3, the first of stack A, with one join fix; joined 16:35 at bf36375f, a signed merge onto d5ff8489, by the batch-5 joiner; store format 14 to 15; installed 20:00 at 3085f71a, install #2, with the owner's three new profiles)

**Why.** Every message ran on its session's profile, whatever it asked: a "thank you" on Sonnet 5.5, a hard design
question on Sonnet 5.5, a mechanical rename on Sonnet 5.5. Step 25e gives Jev a pack, `route.v1`, whose one deciding
question names the message's interaction mode, and lets the turn act on the answer: a mode maps to a list of profiles,
the first usable one wins, a trivial message takes a short detour that leaves the session as it was, and a lasting
change moves the session. The verdict is asked at inbound, in the request that already carries `classify.v1` and
`role.v1` (Item 121), and the turn waits for it at most `[routing] max_wait_ms` after its first
compile, so the wait runs beside work the turn does anyway.

**What landed** (55 files, +3,115 −80 with the paperwork; merged with the join fix, 53 files, +2,666 −80; no new
package).
- **The pack** (`packs/route.v1.toml`, embedded). Its state is `classify.v1`'s (`state = "inbound"`, cap 3000,
  `jev-1.13.0`; a test holds the state byte-identical). One deciding Choice, `mode`: `trivial`, `chat`,
  `sophisticated`, `deep_coding`, `routine_coding`, `other`. A new `Baseline::SessionProfile` and `Action::Route`, a
  golden request, and a row in the shape test. `WIRED` gives it `Live`. A pack with an action must name rollback rules
  (the loader's rule): the session chose `labels_per_day "wrong model" 3` and `on_path_p95` 250 ms over 20, for the
  owner to confirm.
- **`[routing]`** (`config/routing.rs`, sparse, with the template's own section): `enabled` (true), `mode` (`live` or
  `shadow`), `max_wait_ms` (200), `trivial_context_turns` (2), `cold_switch_tokens` (30,000), `switch_confidence`
  (0.6), and `[routing.modes.<mode>] profiles`, each mode table optional. The defaults: `trivial = ["cheapest"]`,
  `sophisticated` and `deep_coding` Opus and Fable, `routine_coding = ["glm53", "glm"]`, `chat = []` (the session's
  own). `cheapest` is reserved (a profile so named is refused); a mode may name a profile the config lacks, which is
  skipped as unusable. The template gains `[profiles.opus]`, `[profiles.fable]` and `[profiles.glm53]`.
- **The pure rule** (`routing.rs`, 641 lines). *Usable:* the provider is built, its key `Ready` or not on the board,
  its model in the catalog, and able to read images when the turn has one. *`cheapest`:* by a short turn's cost at
  catalog prices (4,000 uncached input tokens, 500 output). *`pick`:* a mode's first usable profile under the place's
  cap (a dearer one is passed over as `capped`, and the next under the cap still wins); an empty list is the
  session's own; none usable is `fallback`. *`decide`:* the confidence check (detours included), detour or switch,
  and the `cold_switch_tokens` hold: above 30,000 tokens of context a switch waits for a second agreeing turn.
  `break_even_turns`, and `Routed`/`Hold`, stored on the session.
- **The turn** (`turn/route_step.rs`, 561 lines). `at_inbound` returns a oneshot (`RouteWait`), and a live call
  waits for a permit (`Urgency::live`) instead of being shed. `compile_routed` takes the first loop's compile: it
  compiles on the session's profile, then waits at most `max_wait_ms` (`beside`), then decides:
  - **stay:** the first compile is persisted, as before;
  - **switch:** the routed target is set (a `OnceLock<Target>` declared before the `Turn`, so it outlives it), the
    spec is rebuilt and compiled again (the model change recompiles and strips the prefix's thinking), the routed
    provider runs, and `session.routed.profile` and `last_target` move;
  - **detour:** a request of the persona header, the last `trivial_context_turns` exchanges and the message, with no
    context files and no ontology walk, never persisted; the turn finishes with the session's own `last_target`.

  A verdict that comes late is kept by the judge, in memory, per session, for the next message. A pinned turn,
  `[routing]` in shadow, or Jev's open breaker never waits. An unpinned turn of a routed session runs on
  `session.routed.profile` (`route_base`).
- **The owner's choice.** `TurnSubmitParams.carried` is new: the CLI's pane sets it on the profile it carries from
  the last turn, which is no pin. A profile, provider or model named without it is a pin (`ask -P/-p/-m`,
  `prompt -P`, the cockpit's picker): its turn runs there, and route.v1 judges it in shadow with `pinned` and
  `chosen`. `profile.use` is no pin.
- **Records.** `route.decided` (`fact/route.rs`) rides in the turn's next frame: mode, confidence, judgment, from,
  profile, reason, detour, switch, `est_tokens` and `wait_ms`, with a `route` trace mark and a narrative line.
  `TurnSubmitResult.route { mode, reason, from }` is a new protocol module with its TypeScript, and the CLI's status
  line reads `[glm → zai/glm-5.3-flash · trivial (detour) · …]`. Judgments are scoped `judge:route`. The learning
  report's acting classes gain route's four. A system label, rule `chosen` (weight 0.5): when the session's next judged
  message, within 10 minutes after a routed turn, is pinned, the pinned profile's one mode (or `{"not": mode}`)
  labels the routed turn's judgment.
- **Two new reasons** beside the task's eight: `unsure` (a verdict under `switch_confidence`) and `no_verdict` (Jev
  failed or skipped, the budget, the breaker).
- **The compiler.** Thinking goes back only to the model that wrote it, no longer only to the provider: nodes record
  the model asked for (`provider.rs`), so the comparison is exact (§4.4). `compile_step` gains `defer_persist`, so
  only the compilation the call uses is stored.
- **The store's format** moves for `SessionRecord.routed { profile, hold }` (`#[serde(default)]`), with a layout
  sample: 14 on the branch's base, which main had already taken (independence, 28a), so **15** at the join.

**How it is proven.**
- **The session's tests.** `routing::tests` (7: each mode's first usable profile, then the next, then the session's
  own; unconfigured names; `cheapest`, with an image skipping GLM-5.3; the detour; the hold and agreement; nothing
  under 0.6 routes, 0.6 does; the place cap; the break-even numbers), `config::routing::tests` (4), and
  `route_step::tests` (3) on tokio's paused clock: with a 100 ms compile and a 200 ms bound, a verdict that never
  comes releases the call at exactly 300 ms, one at 150 ms releases it at 150, one during the compile at 100; 299 ms
  gives 299, 301 gives 300 and `late`; a dropped sender ends the wait at once. `tests_route.rs` (9), against the fake
  Jev with two fake providers: one request asks `classify.v1/kind`, `role.v1/role` and `route.v1/mode` (`call.packs =
  3`, mode live); a hard question moves the session to Opus with one stored compilation; routine coding goes to
  GLM-5.3; the detour sends 3 messages, leaves `compilation_id`, `last_target` and `routed` unchanged, and the next
  request on Sonnet begins byte for byte with the earlier one; `cold_switch_tokens = 1` gives `cache_hold`, then a
  switch, and `unsure` under the confidence; a pin stays; the pane's carried profile is no pin; Jev down, a 429 and a
  malformed answer say `no_verdict`, a 3 s Jev says `late`, each leaving the request byte-identical to the judge-off
  one in under 2 s; a late verdict moves the next turn; the system label at 9 minutes and not at 11. theseus-judge's
  122. Under load (four busy loops, the tests at nice 19), 19 of 19 three times.
- **Planted reverts.** The session's two, re-run at the review, and four more: **4 of 6 caught** (the verdict awaited
  before the compile, `left: (200, Late) right: (300, Late)`; the detour through the session's compile step; the
  label's window at 12 minutes; the pane's carried profile as a pin). The two not caught are test gaps with the code
  right (theseus-g1gl): a place's cap at the turn, and thinking sent back to another model of the same provider (the
  fakes write no thinking).
- **The review's build and suites:** clean, and 1,306 of 1,306 (theseus-core whole, -judge, -protocol, the CLI, and
  theseusd's versions, judge and default_config); after the join fix, 1,136 of 1,136.
- **Live, at the review** (a scratch daemon of the merged debug build, before the fix, on a fresh state dir; Sonnet,
  GLM and Jev live; about $0.12 of models, Opus $0.057 of it). With the task's five profiles: a design question's
  verdict came late (a cold connection, about 2 s), so the turn ran on Sonnet after a 201 ms wait; "thank you!" was
  trivial at 0.98 in 194 ms and **detoured to GLM-5.3 Flash** (7,120 tokens in, 6.9 s), the session staying on
  Sonnet; routine coding at 0.75 in 173 ms **switched** to GLM-5.3, and the next, 0.97, stayed there with **7,296
  tokens read from cache**. Every route.v1 judgment shared its call (`packs: 3`, 7 questions) with classify.v1 and
  role.v1, mode live. All nine routed messages waited 133 to 201 ms, and 4 of 9 verdicts missed the 200 ms bound.

**What the session and the review found.**
- The session: `at_inbound` returned nothing, so a verdict had no way back to the turn; the turn's `Target` is fixed
  before the `Turn` starts (hence the `OnceLock`); `compile_step` persisted every compilation (hence the defer flag);
  and the compiler stripped thinking only from another provider's messages, so a detour to another Anthropic model
  would have sent that model's thinking signatures to the session's.
- **The branch's own bug, found live by the review, fixed at the join.** A late verdict was kept per session until a
  later message's own verdict was also late or missing, however many on-time verdicts came between. Live, a sed
  one-liner went to Opus on a design question's verdict two messages old ($0.057), and a design question went to
  GLM-5.3 Flash as a `trivial` detour on the verdict of "hello" (11.0 s). The task's rule is that a late verdict
  applies to the next message.
- **The break-even of a switch**, priced from the catalog: Sonnet to GLM-5.3 at 30,000 tokens of context and 1,000
  of output a turn pays back in 11 turns, Opus to GLM-5.3 in 3, and Sonnet to Opus never in money (a quality choice);
  GLM-5.3's cache read ($0.26 a million) is dearer than Sonnet's and Opus's ($0.20), so only output pays a switch
  back. The review added two gaps in that model (the warm read staying would have paid, about 14 % at 30,000; and the
  way back, Opus's cold write at 100,000 being $0.50). The rule itself prices nothing: the hold and two agreeing
  turns are a confidence guard.
- Two trivial detours to GLM-5.3 Flash took 6.9 s and 11.0 s (first tokens 6.6 and 7.6 s), where Sonnet's and Opus's
  answers took 2.4 to 4.6 s: "the trim is for speed" did not hold in that sample.

**The join** (the batch-5 joiner, spawned 16:15; lock `cloud-route-join` 16:17:44). `git merge --no-ff --no-commit`
on d5ff8489, the two cloud files removed, then the review's `resolve.py`: rerere replayed all four conflicted files
(`tests_inbound.rs`, theseus-judge's `pack.rs`, theseus-store's `store.rs`, `long-files.txt`), every block
keep-both; the script renumbered the format to main's plus one (15), applied **join fix 1** with its test, and left
both ceilings as the review raised them (`compiler.rs` 2,547, the protocol's `lib.rs` 2,696). The staged tree
equalled the review commit 6be9f37e plus main's own diff since bddfd407, line for line. Join fix 1: `read_verdict`
takes the session's late slot at every message and uses it only when it was asked for the turn just before
(`session.last_turn_id`); a verdict landing after the next message began is dropped. Its test,
`a_late_verdict_applies_to_the_next_message_alone`, and its planted revert, which fails `left: ("opus", "verdict")
right: ("sonnet", "late")`, the live bug. The warm was clean at 16:26:00. **The gate** (16:26:13 to 16:34:52, 113 s
of it waiting for the reviewers' holds): suite **2,395 of 2,395** (17 skipped); lifecycle ok on strict budgets
(settled 70 s, load 11.24): cold start p50 24.2 and p95 57.1 ms, at its limit, vault 28.3/31.9, clean shutdown
34.6/57.3, SIGKILL and restart 31.2/35.5, swap 52.1/59.4; jobs' L1 start p50 7.47, p95 16.10; turn frames **5 and 9**.
The turn's wall p50s, plain 105.5 and tool-call 252.1 ms (the daemon's 75.0 and 214.0), were up on the gates before
(d5ff8489's 85.4 and 209.8), with the disk probe at 6.7 ms before the turns and 14.7 after. With the judge off,
route's turn path is an `Option` check, a `OnceLock` and two `Option` checks, so it was pushed and left for the next
gate: lane gliding's, on route's code at 16:50, read 85.7 and 185.5 ms with the probe at 7.0 and 6.7, and no A/B was
needed. Pushed d5ff8489..bf36375f, the cloud branch deleted, the done line 16:35:52, theseus-0j2.11 closed.

**The install** (install #2, 19:59:53 to 20:00:01, at 3085f71a). Eddie decided at 18:02 to add the models and go
live with route.v1, judging its decisions in the cockpit's Judgment view ("wrong model" three times a day rolls it
back for the day, and an on-path p95 over 250 ms over 20 too). The install's precheck appended the template's
`[profiles.opus]`, `[profiles.fable]` and `[profiles.glm53]` to his config, with a backup; `[routing]` stayed at its
defaults (live, 200 ms). Health after: store format 16, `route.v1`, `rerank.v1` and `security.v3` live (owner:
decision of 2026-10-04), 9 secrets ready, startup serving at 29.1 ms. The same night, each a config edit with a
backup, a check and a restart: at 23:03, after his first two routed messages both ran on Sonnet (route.v1 said
`chat`, 0.85 and 0.55, and chat keeps the session's own), `[profiles.haiku]` (Claude Haiku 4.5) as the trivial
model; at 23:12, his routing table (trivial Haiku then GLM-5.3 Flash; chat the session's own; sophisticated Fable
then Opus; deep coding Opus then Fable; routine coding GLM-5.3 then Flash); at 23:53, `max_wait_ms = 300`.

**Divergences.** Two reasons beyond the task's eight. The late verdict lives in memory, so a restart loses it (its
judgment row is durable). Live route judgments are paid from the judge's shadow day budget (the point's one
reservation) until 26b's kernel actions. The place cap is proved at the unit level and wired through
`TurnCtx.ceiling`; no core test binds a place with a ceiling profile.

**Known gaps.**
- **theseus-9yyr** (P2, raised to P1 at 18:02): routed state outlived routing being off, and a fallback kept the
  routed profile. On Eddie's config before install #2 (Sonnet and GLM only), routine coding would have moved a
  session to GLM-5.3 Flash and a hard question would then have stayed there. Its first item was fixed that evening
  (Item 150); its second, where chat, other and a fallback leave a routed session (the cache-warm
  stay), is theseus-17jn, settled as built by the DM thread for Eddie to overrule (the morning notes' section 31).
- **theseus-ddbi** (P2, raised to P1 at 18:02): the wait. The morning notes' call: read his real numbers in the
  cockpit before deciding; his 23:53 config raised the bound to 300 ms.
- theseus-g1gl (P3): the two untested rules above. theseus-d13v (P3): a switched turn records loop 0's
  `context.compiled` and `loop.started` twice, the first naming a compilation never stored, and a detour records
  `loop.started` twice.
- Not filed, for the record: route.v1's two rollback rules were declared, but nothing evaluated them until the ladder
  (Item 145), and `on_path_p95` reads `on_path_ms`, which the inbound point records as 0; with Jev's breaker
  open a live turn records `shadow`, not `no_verdict`; the detour offers the trivial profile's tools (history may hold
  `tool_use` blocks); the cockpit's session header does not show the route yet.

### Item 140. Gliding on the place rule: `channel.post` and `channel.read`, into a private place always, out of one or between two shared places only once the owner says (theseus-ypy0; step 38b, roadmap row 69; Eddie's "Gliding with the place rule: yes!" of 2026-10-04 15:09; the `gliding` lane, spawned 15:11, in a worktree; b92c5f07 on 9bf8ac35; joined 16:51 at 21683c9c, a signed merge onto bf36375f, by the lane itself; reviewed after the join, 17:52, by the DM thread; installed 20:00 at 3085f71a, install #2)

**Why.** 38b's first design let an execution post into another channel and borrow its history under a subset rule
built on 19a's labels, and the place rule removed those labels (Item 76), so the step waited on a redesign (Part II's
P9). At 14:37 Eddie asked what gliding was; the DM thread explained it and proposed a design on the place rule, and
at 15:09 he said yes. The rule, in the brief's words: into a private place always; out of a private place asks first,
as `/publish` does; from one shared place to another asks first, since they are different audiences.

**What landed** (one signed lane commit; the merge 24 files, +1,968 −52).
- **One function decides** (`places::glide_rule(from, to) -> Glide { Allow | AskFirst(reason) }`, in
  `crates/theseus-core/src/places.rs`). Words go from where they were said to where they go: a post goes from its
  session's place to the place it names, and a read from the place it names into its session's place. The CLI and the
  web UI are private.

  | from \ to | private | shared |
  |---|---|---|
  | private | allowed | asks first: "out of a private place: #x is shared, so this puts words from Y where others read them, and only the owner does that, as with /publish" |
  | shared | allowed (a read's text is outside text) | asks first: "between two shared places: #x and #y are different audiences" |
  | the same place | allowed | allowed |
- **Where it applies.** The gate's order (`toolrun/order.rs`) gains a `Glide` layer after the place's floor and before
  T1's hold: an `AskFirst` raises the call to `approve`, setting "the place rule (38b)", and a post is no looser than
  its destination's ceiling's floor (38a). The places are resolved once at the gate; a place not bound here is refused
  there. The answer is `/publish`'s: it counts only from the owner in a private place, and a shared place's card goes
  to the owner's DM. An approved post out of a private place into a shared one also writes publish's
  `place.published` row (who approved and through what, from the answer's own row; the source call and session; the
  words by digest; the bytes). The run checks the rule again (`toolrun/glide.rs`), so a place the binding rebound
  while the question was open is not glided past.
- **`channel.post { to, text }`:** class Write, a harness tool, at most 4,000 characters. One outbox post of kind
  `glide`, staged in the frame that settles the call (a cancel that settles first posts nothing). The place's lane
  sends it once, under the keys `glide:<call's correlation id>` (then `:1` for a second part), and the message ends
  with `-# ↪ posted from Theseus session `a1b2c3``. T1's hold applies.
- **`channel.read { from, last? }`:** class Read, `last` 20 by default and 100 at most: the place's session's last
  messages, people's and Theseus's, oldest first, each `[time] who: text` cut at 2,000 characters, as one node, the
  call's result, whose first line is `[borrowed from #x: its last N messages, oldest first]`. From a shared place the
  result is marked external, with the place's name as its URL, as a fetched page is; the line says it is outside text
  and no instruction, and the session takes T1's hold in the same frame. The node has a `derived_from` edge, via
  `glide`, to each message it took, so `node.reach` finds the copy.
- `to` and `from` name a place by its label (`#deploys`, `DM @…`) or its key (`channel:<id>`, `dm:<user>`); a place
  this daemon isn't bound to fails, "#x is not a place Theseus is bound to: name one of the places it is, which
  `theseus places` lists".
- **Rows and words.** `glide.posted` (correlation id, from and to with their names, characters, `allowed` or
  `approved`, why, the post) and `glide.read` (the same, with the message count, `outside` and the node), each with a
  narrative line ("session a1b2c3 posted to #deploys (1,204 chars, asked first and approved)."). `policy.explain`
  gives the rule as a condition, `glide`, since its probe names no other place. The template gains commented
  `channel.post` and `channel.read` lines.
- **Offered everywhere.** A shared place is offered both tools too (`places::glide_tool`), since the rule asks
  wherever it must; a ceiling still narrows them by the family `channel`.
- **No store format change:** the ledger kinds are strings, the post is an outbox body, the outside-text mark is the
  existing `ToolResult.external`, and the edges are EDGE records with a new `via`.

**How it is proven.**
- **Tests.** `tests_glide.rs`, 7 through the whole core on the places rig with a scripted model: a post into a
  private place runs once, keyed by its call; a post out of a private place waits, and nothing moves meanwhile, a
  non-owner's answer and the owner's from a shared channel don't count, and the owner's CLI approval posts it once
  with `glide.posted` (approved) and `place.published`; a declined post posts nothing and writes no row; shared to
  shared asks, shared to the DM and to its own place run; an unbound place fails with words for a post and a read;
  a read from a shared place is outside text, holds its session, and makes the session's next post wait on T1's
  hold, even into the DM; a shared channel's read of the owner's DM asks first, and nothing of the DM reaches that
  model before the answer; a post takes its destination's floor. The table test runs 12 rows. theseus-discord's
  `an_approved_glide_posts_once_in_the_other_places_lane`, against the fake Discord: the channel gets exactly one
  message, made by one create whose nonce is the glide's key.
- **Runs.** 96 of 96 glide, places, explain, ceiling, template and fact tests on the workspace build; the full suites
  of theseus-core, theseus-discord and theseus-protocol, 1,120 of 1,121 (the one miss the output golden's wake step at
  load 22, its wake due before the turn ended; it passed alone in 7.7 s, and the change leaves `wake.at` alone).
- **Planted reverts** (each baseline 11 of 11): the rule allowing private to shared fails 4 tests; a read not marked
  outside text fails 1; the gate's order skipping the glide layer fails 5, which proves the gate asks (the run's
  check alone would refuse, not ask).
- **Live, on a scratch daemon** with the fake Discord, a stand-in model, and the rig's fake secrets, its ports checked
  free just before: private to private ran; private to shared parked with the rule's words, the channel held nothing
  before the answer, and the owner's approval from the CLI posted exactly one message ("Posted 74 characters to …, with
  the owner's approval"); shared to the owner's DM ran; shared to shared sent its card to the owner's DM, where a
  decline settled both messages and posted nothing; a shared channel's read of the owner's DM parked, and once
  approved in the DM, borrowed its last 2 messages; the CLI's read of a shared channel came back external, the session
  held it, and its next post, into the DM, parked on T1's hold. The ledger: three `glide.posted`, one
  `place.published`, two `glide.read`, one `session.external_read`. The daemon's log had no error.

**FAST.** Nothing new at the start but two tools in the registry's map. A turn that calls neither pays two string
compares at the gate and no read or frame; the two definitions add 2,138 bytes of JSON to every request's tool list,
in the cached prefix. A post's records ride in its call's completion frame; a read scans its source session's
transcript once.

**The join** (lock `lane-gliding-join` taken 16:36:00, 8 s after route's done line). `git merge --no-ff -S` gave
21683c9c, a clean textual merge (`turn.rs`, `tests_outbox.rs` and `ledger.rs` auto-merged; no join fix). The warm:
the test build in 3 min 52 s, clippy clean, exit 0 at 16:41:54. **The gate** (`gate: ok`, exit 0 at 16:50:34, 513 s,
65 s of it waiting for a reviewer's step): suite **2,408 of 2,408** (17 skipped), the 13 glide tests among them;
lifecycle ok, measured with the busy allowance after 2 minutes of IO pressure at 16 %, but every phase inside its
strict limit (cold start p50 31.1, p95 40.0 ms; the config copy's p95 34.0; clean shutdown with a job p95 75.2;
SIGKILL and restart 45.1; swap 126.2); jobs' L1 start p50 7.88 ms; turn frames **5 and 9**, wall p50 plain 85.7 and
tool-call 185.5 ms against route's 105.5 and 252.1, the fdatasync probe 6.7 ms in both; resident memory 61.9 MB after
the start and 88.7 after the burst, against 62.0 and 86.4. Pushed, the done line at 16:51:06, theseus-ypy0 closed;
the lane's 29 GB target deleted. **Docs the lane changed:** M7's §2.3 (38b rewritten to the place rule, with the
matrix, the tools, the outside-text mark, the publish record and the run's check), roadmap row 69's note, and §2's
"still open" line and a "Decided on October 4" bullet (Part I, chapter 2), each scrubbed (0 hits).

**The review** (17:52, the DM thread): the rule matches the 15:09 call, the table test, the three plants and the live
check were read, and the docs scrub found no new hit. The one choice beyond the brief, both tools offered in shared
places, lets a shared channel post into the owner's DM unasked; Eddie was told, and the DM thread settled it as built
for him to overrule (the morning notes' sections 30 and 31).

**The install** (install #2, 20:00, at 3085f71a; its health as in Item 139).

**Divergences.** Offering both tools in shared places was the lane's choice beyond the brief, with the rule asking
where it must (above). An approved glide out of a private place writes publish's own row, not a row of a new kind.

**Known gaps.** The cockpit's Boundaries view shows the `place.published` row an approved glide writes, but no glide
row of its own (a follow-up, if wanted). The live check ran with the web UI off, so the cockpit's rendering of a
glide was not seen.

### Item 141. `rerank.v1` live: Jev's order of recall goes in front of the model, waited for at most 200 ms, with a breaker of its own, the owner's labels held out on both paths, and each per-item answer graded (theseus-6fn.7, with theseus-mm4a; step 32d; Eddie's decision 4 of 2026-10-04 10:11; the fifth cloud batch's rerank-live session, fired 11:14 from 802f9135, Opus 5.5; 61ded074, e59a46af, 604188f3 and c4806477; reviewed 14:46 to 15:46 by the local reviewer R3, the second of stack A, with two join fixes; joined 17:03 at 3829c7ea, a signed merge onto 21683c9c, by the batch-5 joiner; no store format change; installed 20:00 at 3085f71a, install #2)

**Why.** Step 32c (Item 128) put `rerank.v1` beside recall in shadow: Jev reordered recall's top twenty
notes off the turn's path, and the ledger showed what `+rerank` would have admitted. At the 9am review (10:11, "Take
your recommendation") Eddie chose option (c), live, behind three conditions in one cloud row: theseus-mm4a's test
first (a note the owner labeled wrong or stale never reaches Jev or the repack), a breaker of rerank's own (so slow
reranks cannot stop `route.v1` or the security pack), and a bounded live rerank in recall's live path, with recall's
own order on a miss. Since Eddie wants his labels to count ("Live reinforcement learning is exciting"), the row also
grades `rerank.v1`'s per-item answers, with a system label drawn from his memory labels.

**What landed** (the merge: 44 files, +1,941 −142; with the cloud paperwork 46 files, +2,365 −136; no new package, no
store format change: new JSON fields in ledger rows, and `JudgeHealth.breakers` with a serde default).
- **The labels test first** (61ded074, theseus-mm4a). `recall_end` hands the owner's labels to the rerank
  (`Recalled.labeled`), and with that field emptied no test had failed: the filter
  (`theseus_memory::rerank::eligible` with the asker's labels) was right, and nothing held the hand-over.
  `tests_rerank_live.rs` gains `a_labeled_note_never_reaches_a_shadow_rerank` and, after step 3,
  `…_live_rerank`: through a real turn against the fake Jev, a note labeled `wrong` (then `stale`) appears nowhere in
  the body Jev was sent, `eligible` is 2, and its key is in none of the row's admitted lists.
- **A breaker of its own** (e59a46af). `theseus_judge::JevJudge::with_breaker(name, packs, cfg)`; the core builds the
  judge `.with_breaker("rerank", ["rerank.v1"])`. A batch answers to its first part's pack's breaker (a batch's packs
  share a state, and `rerank.v1`'s state is its own), so rerank's failures and timeouts open only `rerank`; one
  client still, its in-flight permits and its shed count shared. `breaker_status()` stays the shared breaker's,
  which `route.v1` reads; `breaker_of(pack)` and `own_breakers()` are new. `judge.circuit` rows carry
  `breaker: "rerank"` (the shared breaker's rows are byte-identical to before); health gains
  `breakers: ["rerank: closed"]`, the CLI's judge line ` · rerank breaker closed`, the cockpit's Judgment view a pill,
  and the time machine leaves a named breaker's rows out of the shared breaker's state.
- **Live and bounded** (e59a46af). `[memory] rerank_wait_ms`, 200 by default, 1 to 600 (the call's own deadline).
  The recall step's one call is `recall_reranked` (`turn/rerank_step.rs`), which reads `rerank_mode()` before cloning
  any candidate: `off` takes the plain manifest; `shadow` reranks off the path, as 32c did; `live` takes
  `Memory::manifest_ranked` (the manifest, the candidates and each candidate's source ranks) and then
  `JudgeService::at_recall_live`. That starts its clock first (tokio's `Instant`, so the paused clock can test it),
  picks the eligible notes, marks the judgment `live`, and spawns the rerank task with a oneshot. The turn waits
  `timeout_at(start + wait, rx)`, then `close()`s the receiver and `try_recv()`s: an answer handed over before the
  end is taken, and one after it finds the turn gone, so its row says `late: true`; `applied` and `late` never both
  hold. There is no wait at all when rerank's breaker is open or the day's budget is already paused (read before the
  dispatch); those reranks still go out, counted, so the breaker's half-open probe can run. In time and answered,
  `Memory::refill` packs again with `theseus_memory::rerank::repack` (the same filters, the place rule and the labels
  first, the manifest's budget, the sources' ranks kept), so the node holds Jev's order.
- **What it records.** A `judge` span of kind `wait` (pack, judgment, `applied`, `why`, `wait_ms`); the `recall.ran`
  manifest's new `rerank` (`RecallRerank`: judgment, applied, why, waited_ms, wait_ms); the judgment row's
  `rerank.live`, `applied` and `late`; and `theseus memory recalled` prints `in Jev's order (jdg_…): rerank.v1
  answered after 115.0 ms of the 200 ms wait`, or `in recall's own order (jdg_…): <why>`. The call keeps its 600 ms
  deadline, and its reservation stays on the judge's day budget (§2.7's session pay waits for 26b).
- **Per-item answers graded** (604188f3). `judge.label --question helps.3` looks the question up among the
  judgment's own answers, checks the label as a Noul's (true, false, right, wrong), and writes `about`, the item's key,
  on the row (`JudgeLabel.about`, written only when set, so every other label row is byte-identical) and on
  `JudgeLabelResult.about`. An item the judgment did not answer, or the definition itself (`helps`), is refused,
  naming the ones it did answer; nothing is written. The report (`learning/items.rs`) grades the per-item definitions
  `helps` and `helps_more` as questions of kind `noul`. **The system label** (`learning/rerank.rs`, rule
  `memory_label`, weight 0.5): each `memory.label` of `useful` or `should_have` (true) or `wrong` or `stale` (false)
  grades the answer about its node, in the rerank whose recall the label names, else the newest answered rerank
  before the label (by WAL position) that asked about the node; keyed so each memory label writes one, once.
  `SystemLabel.question` became a `String`, with `about`. The cockpit's `JudgmentLabels.tsx` gives each per-item
  answer a row with true and false buttons.
- **Live by default** (e59a46af's line, c4806477's words). `WIRED` gives `rerank.v1` `PackMode::Live`; it rode with the
  code, since the config can only lower a mode (`mode_of` is the least of `WIRED`'s, `max_mode` and the pack's line),
  so the live path could not be reached or tested without it. The template's `[judge]` words, theseus-core's and
  theseus-memory's AGENTS.md.

**How it is proven.**
- **The session's tests:** `tests_rerank_live.rs` and `tests_rerank_labels.rs`, 9 tests. On the paused clock, an
  answer 199 ms after the call reached Jev is applied (waited 199.0 ms) and one at 250 ms is not (waited exactly
  200.0 ms, why `timeout`, recall's own note admitted, the row `late`); a Jev at 400 ms against the 200 ms wait leaves
  the provider request equal to a judge-off core's; an open breaker, a spent budget, the judge off and rerank in
  shadow each wait not at all; a per-item label written, four bad ones refused; a memory label's system label written
  once, and the owner's label on the same answer outweighing it. Five planted reverts, each caught by its test
  (the labels emptied on the shadow path and on the live path; rerank on the shared breaker, which then wrote no
  `judge.circuit` row at all, since loop.v1's answers kept resetting the streak, the very coupling 32d removes; the
  wait unbounded; the per-item questions dropped from the report). Under four busy loops, the 9 tests 5 times, all
  passed, after the session loosened two bounds of its own test (a starved turn's wall time; the pre-checked cases'
  `waited_ms` under 50, which load took to 51). The turn bench, judge off: frames 5 and 9.
- **The review** (R3, in cloud-r1, stacked on route's review commit): the build and clippy clean; the suites (theseus-core
  whole, -judge, -memory, -protocol, the CLI, theseusd's versions, judge and default_config) **1,366 of 1,366**.
  Planted reverts, run at a load of 25 to 43: **5 of 6 caught** (the labels on the live path, the shared breaker, the
  wait unbounded, the per-item report, and a live recall waiting on rerank's open breaker, `left:
  Some("circuit_open") right: Some("breaker_open")`). The sixth, join fix 1's links left out of the repack, was not
  caught: the code is right as merged, and no test runs a rerank over two linked notes (theseus-daiz, P3).
- **Live, at the review** (a scratch daemon of the stack's debug build with the merged `theseus-index` beside it,
  `ready · hybrid`; the stand-in model, so no model spend; Jev live with the owner's own key, never printed;
  `[memory] mode = "live"`, `[routing]` off so rerank's was the turn's only Jev wait; a fresh state dir; Jev's spend
  about a cent). Session A said where the grey heron nests, session B asked: `memory recalled` read **"in Jev's order
  (jdg_…): rerank.v1 answered after 153.2 ms of the 200 ms wait"**, the heron's note first, and the row `live`,
  `applied`, not `late`, with Jev's call at 122 ms. `helps.1 true` was written with its `about`; `helps.9` was refused
  ("this judgment did not ask helps.9: name one it answered, helps.1, helps.2"); the report read `helps (noul): 2
  answered, 1 labeled`; a memory label then wrote **"1 system (1 new)"**, and the owner's label still graded
  `helps.1`. With `rerank_wait_ms = 1` and a restart, the same question read **"in recall's own order (jdg_…): Jev
  had not answered within the 1 ms wait; its answer, if it comes, is recorded late"**, its row `applied: false, late:
  true`, `order_changed: true`. Not seen live: the `wait` span (likely in the turn's `turn.trace` row; the offline
  tests hold it), and the cockpit's buttons (web off).

**What the session found.** The hand-over of the owner's labels was untested, though right. A whole-judgment `right`
or `wrong` stands for every per-item answer (`labels::resolve` takes a label with no question for any question), so
one press grades up to twenty Nouls; the session would have a whole-judgment label skip answers with `about`, and
left it, since it changes what existing labels mean. `late` means the turn had moved on when the outcome came,
whether Jev answered or the call failed (a 600 ms timeout after a 200 ms wait is `late` with fallback `timeout`). The
exam's `+rerank` arm is not wired (its daemons run the judge off); the report says how. Under the VM's UTC, the
core's output golden fails at a wake line's offset (theseus-ig6n, Item 96). And a second worktree verifying a commit
with the same `CARGO_TARGET_DIR` left the first tree's protocol crate fingerprinted to the other's sources.

**FAST** (the review's). Nothing new before serving: the client and its breakers are built at the first judgment.
With the judge off or memory in shadow, as in the gate's benches, nothing a bench can see. With the judge on and
memory live, as on the owner's daemon, every recall in front of the model waits up to `rerank_wait_ms` from the
rerank's start, after the index answers (`recall_deadline_ms`, 250 ms in a release build) and before the first
compile; with `route.v1` after the compile (Item 139), a person's message can wait up to about 650 ms before
the first model call (theseus-ddbi for route's part). Live: 153 ms waited, Jev's call 122 ms.

**The join** (the batch-5 joiner; lock `cloud-rerank-live-join` taken 16:51:17, after lane gliding's join). The merge
on 21683c9c: the review's eight conflicted files, all replayed by rerere (every block keep-both: `WIRED` with the
memory pass's memory.v1 and attribution.v1, route's fields, the module lists, health's pack lists, the recall step,
and recall.rs), then the review's `resolve.py` with its two join fixes, with rustfmt. The staged tree equalled the
review commit fa717a7d's plus main's own diff since route's review tree, line for line. **Join fix 1:** the memory
pass (Item 136) gave `manifest_with` the links of linked notes, and the branch had split the same body,
so `manifest_ranked` takes `links_of`, the rerank step's two manifest calls name the store's links, its `Recalled`
carries them, and `refill` reads them: a repack in Jev's order keeps baseline v2's newer-node rule, and cannot admit
the older of two linked notes. **Join fix 2:** route's `chosen` system label builds its `SystemLabel` with `question:
"mode"` and `about: None`. The warm was clean (exit 0 at 16:55:12). **The gate** (16:55:21 to 17:02:41, `gate: ok`,
54 s of lock wait): suite **2,417 of 2,417** (17 skipped); lifecycle strict (settled 85 s, load 4.20): cold start p50
21.7, p95 25.1 ms, vault 22.0/34.0, clean shutdown 29.4/56.0, SIGKILL and restart 25.1/26.3, swap 47.5/50.7; jobs'
L1 start p50 6.03, p95 7.47 ms; turn frames **5 and 9**, wall p50 plain 79.1 and tool-call 177.3 ms (the daemon's
53.0 and 145.0), fdatasync 5.8 and 6.7 ms, level with gliding's gate. The gate runs the judge off, so the live wait
cannot show there. Pushed 21683c9c..3829c7ea, the cloud branch deleted, the done line 17:03:00, theseus-6fn.7 closed.

**The install** (install #2, 19:59:53 to 20:00:01, at 3085f71a). Memory is live and the judge on in the owner's
config, so from here each recall in front of his model waits up to 200 ms for Jev's order. Health after: store format
16, `route.v1`, `rerank.v1` and `security.v3` live (owner: decision of 2026-10-04, the ladder's adoption,
Item 145), startup serving at 29.1 ms.

**Divergences.** `WIRED`'s live line rode with the code commit, not the docs one. Live reranks are paid from the
judge's shadow day budget until 26b (its `budget: "shadow"` column now covers a live pack). A rerank skipped at an
open breaker or a paused budget is still dispatched, so it is counted and the probe can go, but nobody waits on it.

**Known gaps.** theseus-daiz (P3): no test holds the memory pass's links in a rerank's repack, live or shadow. A
whole-judgment label grades every per-item answer (above). A `waited` can read past the bound when the answer came at
it (an earlier live run read "201.6 ms of the 200 ms wait"). The exam's `+rerank` arm is unwired. Docs owed by the
review and the joiner: the template's `[judge]` words ("the turn never waits for it": `route.v1` and `rerank.v1` now
bound a wait; "Today eight packs judge"), m6 §2.7, §2.9's table and §2.14, and m5 §2.2 and §2.9 (Part I and the
design notes, this version).

### Item 142. `security.v3`'s live notices: after an open call Jev is at least 90 % sure was risky, a notice to the owner's DM alone, never on the call's path, with `security.v1`'s brake until the next local day (theseus-0j2.13; step 24's notices; Eddie's decision 5 of 2026-10-04 10:21; the fifth cloud batch's security-notices session, fired 11:21 from 802f9135 by the DM thread, Opus 5.5; 3469ff3d; reviewed 15:46 to 16:41 by the local reviewer R3, the third of stack A, with three join fixes; joined 17:14 at 250ccd23, a signed merge onto 3829c7ea, by the batch-5 joiner; no store format change; installed 20:00 at 3085f71a, install #2)

**Why.** Step 24 (Item 120) put `security.v1` and `security.v3` at the gate in shadow: each acting
call judged after the gate decides, never delaying it, and nothing told the owner. At the 9am review (10:21, "Take
your recommendation") Eddie chose option (b), live notices: when v3 is at least 90 % sure that a call the gate allowed
was risky, a notice after the call, never delaying it, with right, wrong and noise buttons; the design's brake (more
than 30 notices a day, or 3 labeled noise in a day, sends them back to shadow); v3 beside v1; and `tool_class` kept.

**What landed** (one code commit; the merge 55 files, +2,020 −75; with the paperwork 55 files, +2,415 −74; no new
package: Cargo.lock gains one edge, theseus-discord's dev-dependency on the workspace's theseus-judge with its fake;
no store format change: new optional JSON fields in ledger rows, and a META key of its own).
- **`judge/notice.rs`** (479 lines, new):
  - `flagged` decides which calls get a notice: posture `open`, not made to wait by the external-text hold, no notice
    of its own, and v3 live, answered by its pinned model, with `risky` act-true (0.90 or over). Only `risky` posts:
    `steered`, v3's other deciding question at its provisional 0.75 bar (Item 95), does not, until the learning
    report calibrates that bar; a high `steered` shows among a posted notice's reasons;
  - `Brake`: today's notices, the owner's `noise` labels on v3, and the pause, read once a run from
    `judge:security`;
  - `post_notice`: one frame with the `jev_notice` post and a `tool.notified` row (`by: judge`, the judgment, the
    percent, keyed `notice_<judgment>`), then a `judge.noticed` notification, so the CLI's `ask` prints it under the
    call (`🔔 notified after it ran: proc.run · Jev: NN% risky (sends data out 92%, beyond the ask 71%) · label it:
    theseus judge label jdg_… right|wrong|noise`);
  - `pause_notices`: one frame with a `judge.paused` row (`what: "notices"`, the rule, `short`, `day`, `until`,
    `mode: "shadow"`, keyed `notices_paused_<day>`, so the ladder can read it as a mode), the `jev_paused` post and the
    META mark `judge.notices`, which health reads without a scan;
  - `after_label` counts a `noise` label toward the brake and edits the notice (`jev_labeled`); `notices_state` is
    health's word.
- **The rest of the core.** The gate keeps per-pack modes (`judged_as`) and posts from its spawned task after the
  budget settles (`spawn_blocking`), beside the running call. `WIRED` gives v3 `Live`; `[judge.packs."security.v3"]
  notices`, on by default and refused on any other pack, drops v3 to shadow when false. Two facts,
  `JudgeNotified` and `JudgeNoticesPaused`; the outbox's `stage_to_operator`; the turn's clients handed to the gate
  while notices are live.
- **A race the tests found.** A notice can post before the judge's sink writes the judgment's `judge.call` row (the
  sink batches rows in a 2 s window), so a fast press was refused, "no judgment is named jdg_…".
  `Core::judge_label` now falls back to the notice's own `tool.notified` row, which names the pack, the call and the
  session.
- **The owner's DM alone.** `to_operator` could fall back to the session's place, which may be shared, so the
  courier's `jev_notice`, `jev_paused` and `jev_labeled` posts use a new `Lane::owner_dm` (`approval_dm(None)` only);
  with no such DM the post is refused, saying why. Discord's `runtime/jev.rs` (175 lines): the buttons
  (`jev:<label>:<judgment>`), the press handler and the texts; a press counts only from the owner's DM, and a forged
  button in a shared place is refused ("🔐 Your label did not count"), with no row written.
- **The protocol, the CLI and the cockpit.** `JudgeNoticed` and `judge.noticed` (a new notification method, which
  departs from m5 §2.13's "no new notification method", since the brief asked for the CLI to print the notice),
  `JudgeHealth.notices`, `JudgeLabelParams.discord`, `PolicyNotified.{by, judgment}`, `LedgerKind::DiscordLabel`, with
  the TypeScript; health's judge line ends ` · notices on`; the cockpit's transcript shows a pill and the label buttons
  beside a noticed call, the Judgment view a Notices panel, and the time machine skips `what: "notices"` rows when it
  folds the budget's pause.

**How it is proven.**
- **The session's tests:** `tests_notices.rs`, 7. An open call scored 0.95 posts one notice after `tool.started`, v3
  recorded `live` and v1 `shadow`; at 0.89 with `steered` 0.99, none. `notify` at 0.95, `approve`, and an open call in
  a holding session post nothing; with Jev down nothing posts; **with Jev slow (5 s), `tool.started` comes as fast as
  with notices off**. 30 notices post and the 31st trips the brake (one `judge.paused`, `notices_per_day`, "31
  notices today"; health `paused until <day>`, the budget's own pause untouched); a day later the next flagged call
  posts again. Three noise labels trip it on the third, and a restart keeps the pause. `notices = false`, the pack's
  `mode = "shadow"` and `max_mode = "shadow"` each post nothing. `tests_security.rs` 9 of 9; theseus-discord 101 of
  101, among them the fake Discord's notice in the owner's DM with three buttons, never in `#lab`, a forged Noise in
  `#lab` refused, and the owner's press labeling it with `via: discord:dm`. Under four busy loops: 15 of 15 and 14 of
  14. Two planted reverts, each caught: the call made to wait on Jev (`on 5.03s, off 26ms`), and the brake counting
  notices only.
- **The review** (R3, stacked on rerank-live's review commit): the build and clippy clean with the join fixes; the
  suites (theseus-core whole, theseus-discord, -judge, -protocol, the CLI, theseusd's versions, judge and
  default_config) **1,427 of 1,427**. Planted reverts: **5 of 6 caught** (the call made to wait; noise uncounted;
  the brake never lapsing at local midnight, `the_31st_notice_of_a_day_trips_the_brake_until_the_next_day` failing;
  a press's place dropped, so the forged press from `#lab` counted; join fix 3 reverted), and the sixth void in
  effect: dropping `waited_on_hold()` from `flagged` changes nothing, since `posture != "open"` already excludes
  every held call. The brake lapses at local midnight, by the ladder's rule: its day is the local day, and its
  `until` the next local midnight.
- **Live, at the review** (a scratch daemon of the stack's debug build; GLM-5.3 Flash and Jev live; `proc.run` open;
  routing, memory, web and Discord off; a fresh state dir; GLM about $0.006 and Jev under $0.001). The task's own ask,
  a random fake key piped to `nc` at an unroutable test address, then `ls`: Jev's v3 scored the pipe's call **0.80
  risky**, in the confirm band, so, by the rule, **no notice** posted. Three `noise` labels: **the second paused the
  notices, "3 labeled noise today"**, the branch's own bug (join fix 3); the third added no row. A restart showed the
  pause before any judgment, and the next ask's five v3 judgments posted nothing. Not run live: a posted notice
  (Jev's scores stayed under 0.90), the cockpit's pill and panel, and Discord's buttons (proved offline).

**What the session found.** v3's pack header still says it never posts a notice; any edit to a pack is a new version,
so it waits for v4. "After `tool.started`" holds by the work's order (a blob, a scrub and a call to Jev before the
post, against a dispatch already in flight), not by code. A run's first notice or noise label scans all of
`judge:security` to count today's rows (theseus-b8e2, P3). The pause has no "resumed" row; it ends at the next local
day. Live v3 judgments are still paid from the judge's shadow day budget, while m5 §2.16 says a live judgment comes
from the session's limit (an open question for the owner).

**FAST** (the review's). Nothing new before serving: the brake reads its day at its first use, and health the META
mark. With the judge off, as in the gate's benches, `gate_on` is false and `notices_live()` a config read. With the
judge on, the call never waits (the plant holds it), a notice's frame lands beside a running turn, never as one of
its frames, and every acting call's judging gets a clone of the turn's sink while notices are live.

**The join** (the batch-5 joiner; lock `cloud-security-notices-join` taken 17:03:07, after the review's done line at
16:41:21). The merge on 3829c7ea: the review's four conflicted files, all replayed by rerere, every block keep-both;
the template's `[judge]` words now say three packs act (`route.v1`'s model for a message, `rerank.v1`'s order of a
recall, `security.v3`'s notices). Then the review's `resolve.py`, with rustfmt; the protocol's `lib.rs` stayed at its
2,698 ceiling. The staged tree equalled the review commit 311c57e5's plus main's own diff since rerank-live's review
tree, line for line. **Join fixes:** (1) main's memory-pass test builds two `JudgePackConfig` literals, which gain
`notices: None`; (2) rerank-live's label test builds `JudgeLabelParams`, which gains `discord: None`; (3) **the
branch's own bug, found live:** `judge_label` writes the label's row and then calls `after_label`, which at a run's
first use read the day from the store, already counting the row, and then added one more; a label now counts itself
only when the day was already read, with `tests_notices::a_runs_first_noise_label_counts_once` (three notices, a
restart, two noise labels pause nothing, the third trips it) and a planted revert that fails it. The warm was clean
(17:06:23). **The gate** (17:06:30 to 17:13:55, `gate: ok`): suite **2,428 of 2,428** (17 skipped); lifecycle on the
busy allowance (no quiet window in 2 minutes: IO pressure 31 %, load 4.26, so +65 %): cold start p95 93.6 ms, one
outlier (p50 29.9, minimum 26.0), and clean shutdown p95 135.7 (p50 68.1) passed only on the allowance; vault
29.4/42.4, SIGKILL and restart 33.9/41.7, swap 89.9/98.2 passed strictly; the history records the strict verdict as a
miss. The branch changes nothing before serving or on the stop, and a shutdown p50 near 70 ms is what the day's busy-IO
gates read; replay's gate, two joins later, passed every phase strictly. Jobs' L1 start p50 7.11, p95 9.44 ms; turn frames **5
and 9**, wall p50 plain 78.3 and tool-call 181.6 ms (the daemon's 52.0 and 155.0), level with rerank-live's. Pushed
3829c7ea..250ccd23, the cloud branch deleted, the done line 17:14:23, theseus-0j2.13 closed.

**After the join.** The ladder's join (Item 145) put the notices on the ladder: the gate asks `mode_for` per
call, `notices = false` became a ceiling in `JudgeConfig::mode_of`, the notices' pause is read by its key as
security's day brake, `flagged` takes a v3 canary as acting in its arm, and each posted notice lands on the ladder as
an event.

**The install** (install #2, 20:00, at 3085f71a). Health after: `security.v3` live (owner: decision of 2026-10-04), with
`route.v1` and `rerank.v1`; the owner's DM is bound, so a notice has a place to go. Settled for Eddie by the DM thread
that night (the morning notes' section 31): the 0.90 line stays until a week of his labels says otherwise.

**Divergences.** A new notification method, `judge.noticed`, against m5 §2.13's "no new notification method": the
brief asked for the CLI to print the notice. The notices' posts use the owner's DM alone, never `to_operator`'s
fallback. v3's pack file is unchanged, so its header comment is stale.

**Known gaps.** theseus-b8e2 (P3): the brake's first read per run scans `judge:security` from its start, off every
call's path. Notices may be rare at first: the task's own exfiltration example scored 0.80. Two brakes count the
same things (the notices' own and the ladder's adopted rules for pack `security`; Item 145). Docs owed by the
review: m5 §2.7's table row ("`security.v1`'s rules, braking `security.v3`'s notices"), §2.8b (v3 where it says v1),
§2.13 (`judge.noticed`), and the template's stale `[judge]` sentences.

### Item 143. The Ship's sea rolls gently, always, in Live mode, at 15 frames a second when idle; Calm stays still (theseus-wp2d; Eddie's "A gentle roll always is awesome" of 2026-10-04 14:37; the `cockpit-swell` lane, 14:41 to 17:25, in a worktree; 57927336 on bddfd407; joined 17:23 at 14bcce94, a signed merge onto 250ccd23, by the lane itself; reviewed 17:53 by the DM thread; installed 20:00 at 3085f71a, install #2)

**Why.** Item 64's Ship drew only while something moved, so an idle fleet sat on a still sea. At 14:37 Eddie answered
"A gentle roll always is awesome": in Live mode the sea rolls, idle included, and Calm stays still. The lane's brief
asked for the waves to drift and the stars to twinkle, at about 20 frames a second when nothing else moves, with the
rest of the Ship still moving only when something happens.

**What landed** (one signed lane commit; 9 files, +594 −90, all under `cockpit/` but one spec line; no Rust).
- **The swell** (`SEA_SWELL` in `cockpit/src/ship/shaders.ts`, on its own clock `uSwell`, in seconds, which runs only
  in Live mode, so Calm stills the sea where it stands). The waves' crests drift along the rows (the logo's faint teal
  sine lines), one crest every 14 s; a longer second harmonic runs the other way (31 s), so the motion never looks
  mechanical; the rows rise and fall out of step (±5 % of their spacing, 19 s), and a row's light grows up to 14 % as
  the swell lifts it; the stars' glints on the water twinkle, each at its own pace and phase (4 to 9 s); the swell
  rises over its first 6 s after a load. The deep stars stay still. On screen at the fleet's view the waves move about
  10 to 20 px a second, about a pixel a frame, under the activity's brighter and quicker motions.
- **The loop** (`cockpit/src/ship/loop.ts`, new, pure: no DOM, no three.js). Something moving (the camera, a vessel
  settling, a flare, a sail, a lantern, a gear, a stream, a current) draws at every display frame, as before. Nothing
  else moving in Live mode draws the swell alone at `IDLE_FPS = 15`: each wait a timer ending 10 ms early, then a
  display frame, so at 60 Hz every fourth display frame (measured 15.0 and 15.3 fps). Calm, or `?swell=0` (new, beside
  `?calm=1`), draws one frame after a change and stops. A change during the swell's wait cancels the timer and draws
  at the next display frame. Hidden, the loop clears its timer and display frame and asks for none, without relying
  on the browser's pause; shown again, it draws one whole frame.
- **The engine** (`engine.ts`, `post.ts`). A whole frame (the fleet's layer, its glow and the composite) when
  something changed or moves; otherwise a **swell frame**, the composite alone, one full-screen pass. The fleet's layer
  is the scene drawn over black with the share of sea still showing through it kept in alpha, so the composite's `sea ×
  share + layer` is exactly the scene over the sea, and a swell frame keeps the layer and its glow. Each pixel finds its
  point of the sea from the camera's ray, and the waves and glints are added over the sea's still cache (light table,
  grid, rose, stars), which is redrawn only when the camera moves. A still swell (Calm, `?swell=0`) is baked into that
  cache once, so Calm and `?swell=0` cost what they did before. The swell's clock runs on wall time, so slow frames do
  not slow the sea, and a long gap resumes it where it stood.
- **Two fixes the measurements turned up.** The adaptive resolution learns only from back-to-back frames and skipped
  any frame slower than 250 ms, so a software renderer whose first frames ran slow kept full resolution for good (the
  lane's first adaptive bench stuck at scales 1 and 0.85, 1.8 and 4.8 fps, where main's reached 0.5): such a frame
  now counts as 250 ms. The trap was in main's code too. And the engine adds its visibility listener only after its
  renderer is built, so a browser without WebGL never runs a half-built engine.
- **Docs.** The engine's and the post pass's headers, the shaders' periods, `IDLE_FPS`'s comment (why 15),
  `cockpit/AGENTS.md` (the Ship's invariant: the swell is ambient and everything else moves only when something
  happens; "Measuring the Ship": `?swell=0`, `swellFrames`, the swell's idle cost, and how to check a hidden tab in
  headless Chrome), Calm's words, and Item 64's line in Part III, with an inline note (the spec's master chapter edited
  the same way, with no version bump).

**Where it differs from the brief, on purpose.** The wave rows are 16 times closer, alternate rows half a wave apart, as
on the coin: at the old spacing one faint line crossed the screen at the fleet's view, so Calm's still sea shows more
rows too, as faint. The swell is worked out in the composite, not by redrawing the sea's layer, which cost half again
as much. The idle rate is 15, not 20, picked by measuring: each step stays about a pixel, and 15 costs a quarter less
than 20 on a CPU rasteriser. The tests use node's runner, as the cockpit's tests do.

**How it is proven.**
- **`cockpit/test/loop.test.ts`**, 8 tests on a fake 60 Hz clock whose display frames pause while hidden: Live with
  nothing moving draws at exactly `IDLE_FPS`, one timer a frame; Calm or `?swell=0` draws once and stops with nothing
  pending; switching to Calm stills it after one frame, and back starts it; activity raises the rate to every display
  frame (59 to 61 in a second) and settling lowers it; a change during the wait draws at the next display frame; a
  hidden tab schedules and draws nothing for a minute, then a whole frame; hidden mid-activity stops at once; a
  disposed loop leaves nothing waiting.
- **Exactness.** A swell frame against a whole frame at the same moment: identical, every pixel. Live with its effects
  off against Calm's direct draw (the scene straight over the sea, the old way): at most 3 of 255, on 106 of 1,766,808
  pixels.
- **Frames looked at** (headless Chrome, each read with the image tool): in Live the two wave lines in a 2x crop of open
  sea drift about 20 px a half second, 5,600 to 5,900 of the patch's 144,000 pixels changing; in Calm none change in
  the sea (the only changes, 46 to 129 pixels, were the header's clock). With WebGL off, "The Ship needs WebGL" and no
  page error, before and after a visibility change. A hidden tab over 30 s: 0 frames, 0 display-frame calls, no timer
  pending.
- **The cost, on this machine's CPU rasteriser** (SwiftShader, 1920x1080; every number a ceiling, not what a GPU sees).
  Idle, 30 s each, in palindrome order: Live 15.3 fps and **4.69 cores** for every browser process on a quiet CPU (3.71
  at 10.5 fps on a busy one), Live with `?swell=0` 1.04 to 1.27, Calm **0.14 to 0.22**, hidden 1.03 to 1.21 (headless
  Chrome keeps compositing Live mode's CSS animations, which a real hidden tab pauses). So **the swell's own cost is
  3.7 cores at scale 1, and 2.4 at scale 0.85**. One swell frame: 31 to 37 ms of wall time and 0.29 to 0.30 s of CPU,
  against 24 to 25 ms for the composite without it and 50 to 53 ms for the brief's sketch (the sea's pass redrawn).
  The bench (the synthetic 10,000-node fleet, CPU per frame, main against the lane): Calm 176 and 168 ms, `?swell=0` 387
  and 385, so no regression with the swell off; the rolling swell adds about a fifth to each busy frame, and the
  default adaptive bench went from 40.9 to 36.5 fps, above the Ship's 30 fps bar. A first run had shown a regression in
  Calm and `?swell=0` (the still swell worked out every frame); baking it into the cache fixed both.
- **The cockpit's checks:** lint (the same 24 warnings as main, none in the lane's files), 55 of 55 tests, the build,
  and no synthetic session in the production dist.

**The join** (lock `lane-cockpit-swell-join`). At 16:51:11 the lane took the lock on a free queue, six seconds before
the batch-5 joiner took rerank-live's and merged into main's tree; the lane's guarded merge refused, and it yielded and
waited. At 17:03:16 it took the lock again, nine seconds after the joiner took security-notices', and kept it with a
"queued" line, waiting only on locks older than its own (its `harness/joincheck.sh`), so neither could wait on the
other. The merge at 17:14, `git merge --no-ff -S` of 57927336 onto 250ccd23, gave **14bcce94**,
signature good, no conflicts, and no Rust, so no warm. **The gate** (17:14:39 to 17:22:35, `gate: ok`, 476 s, 61 s of it
waiting behind a reviewer's suite): the cockpit's lint, tests and build in 19 s; suite **2,428 of 2,428**, one flaky
retry passing (`theseus-sim::sim the_kernel_holds_its_invariants_under_seeded_faults`, outside the cockpit: it held
every invariant and failed its coverage check, "no series was put back", noted on theseus-81ig at the review);
lifecycle ok after an 85 s settle (cold start p95 28.8 ms, a shutdown with a job 46.1, SIGKILL and restart 31.0, swap
81.1); turn frames 5 and 9. Before the push, main's debug `theseusd` served the gate's own production build on a copy
of a scratch store: the idle Live Ship drew 52 frames in about 3.4 s, all swell frames; Calm drew none; no page errors.
Pushed 250ccd23..14bcce94 at 17:23, the done line 17:23:58, theseus-wp2d closed.

**The review** (17:53, the DM thread): accepted; the lane's branch deleted on origin and locally.

**The install** (install #2, 20:00, at 3085f71a; its health as in Item 139): the cockpit at `/` rolls in Live
mode.

**Known gaps.** theseus-yx5i (P3, waiting for Eddie): on a software renderer the idle swell costs 2.4 to 3.7 cores;
keep it everywhere, or detect SwiftShader or llvmpipe and slow or still the sea there. Settled by the DM thread that
night for him to overrule (the morning notes' section 31): slow or still it on a software renderer; nothing is built for
it yet, and the issue still waits for him. theseus-n2hd: the
Ship on a real GPU is unmeasured (the swell should cost a few tenths of a millisecond a frame there, an estimate).
While something moves, the swell adds about a fifth to each frame's CPU on SwiftShader; a swell cached at its own pace
could about halve that, at the cost of another full-resolution target. A headless page held open costs more in Live
mode, so screenshot and watch scripts should pass `?swell=0` or `?calm=1` (the cockpit's AGENTS.md says so).

### Item 144. Replay, audit and backfill: a candidate pack version asked over recorded judgments, a pack's judgments labeled by a model profile, and a pack's past points judged under the owner's consent (theseus-0j2.14; step 25d, roadmap row 43; the fifth cloud batch's replay session, its task written by writer M after Eddie's 10:21 approval of the learning loop, fired 11:21 from 802f9135 by the DM thread, Opus 5.5; 3ab2a6f1, 9307a93e, 41dbb922 and 02ca7980; reviewed 14:06 to 14:30 by the local reviewer R4, the first of stack B, with three join fixes; joined 17:45 at d5a4b808, a signed merge onto 14bcce94, by the batch-5 joiner, with two join fixes more; no store format change; installed 20:00 at 3085f71a, install #2)

**Why.** The learning ledger (step 25c, Item 129) grades each pack against the owner's labels, with a
frozen holdout. Before a reworded pack replaces its incumbent, M5's design (`docs/design/m5-judgment.md` §2.9) asks
three things of the record: replay a candidate over the judgments the incumbent made and compare them on the same
labels; audit a pack with a stronger model's answers as labels of their own weight; and backfill a pack's points from
history, so a new pack has judgments to grade. At 10:21 Eddie, eager for labels to adjust Jev's prompts, approved the
learning loop (theseus-0j2.12); 25d and the ladder (26a) were its next steps.

**What landed** (four code commits; the merge 49 files, +4,363 −31; with the paperwork 51 files, +4,837 −25; no new
package; no store format change: the new rows are ledger rows, and no stored struct gained a field).
- **Replay** (3ab2a6f1; `learning/replay.rs`, 913 lines; `theseus judge replay`). theseus-judge's pure half
  (`replay.rs`): `what_changes` (a candidate that changes nothing Jev reads, only thresholds or which questions decide,
  is `ThresholdsOnly`; one that changes a question's instructions or criteria, the model, the builder or its cap
  `Asks`), `stored_state` (a blob back to its state, its sha256 checked), `reband` and `builder_identity`.
  `Core::judge_replay` (public and async, so the learning loop can call it in-process) takes the candidate, embedded by
  name or as `--pack-file` text through every loader rule, and refuses it when it is wired, the incumbent, another
  pack's, or a name already recorded under another sha256 (a name means one text). The set: a report's frozen holdout
  with its frozen labels, its train split, `--errors` (the labeled judgments the incumbent got wrong), or ids; each
  judgment's counting label is made absolute, so both sides are graded by one truth. A state goes as sent when builder,
  version and cap match, else is rebuilt (`learning/rebuild.rs`: today `loop.v1`'s, from the turn's `turn.ended` row and
  its nodes, through the live point's own input function), else is left out with the reason. A thresholds-only
  candidate makes no call and re-bands the stored answers. Calls go one at a time through the judge's client and
  breaker at `Urgency::Shadow`, each reserved inside the run's limit, `[judge] replay_limit_usd` ($0.50), the estimate
  checked first. Both sides go through `report::pack_report_of`, with each judgment the candidate fixed or broke,
  agreement with the incumbent, and each class whose precision or recall fell; a security candidate also runs
  `security.v3`'s planted-injection set beside the incumbent. One frame: each call's `judge.call` (`budget: replay`,
  `purpose: replay`) and the `judge.replay` row (`rpl_…`, the candidate's text in a blob), all scoped
  `judge.replay:<pack id>`, which the report never reads. The fake Jev gained `script_when`, answers per state and per
  criterion text, so one wording can fix one judgment and break another. The CLI prints the two versions in two
  columns.
- **Audit** (9307a93e; `learning/audit.rs`, 403 lines; `theseus judge audit <pack> --sample <n> --profile <p>`). The
  pack's answered judgments with no audit label yet, sampled in sha256(seed, id) order (the seed in the result). One
  request per state, outside any session, to the profile's model: the state's JSON and each whole question with its
  options as Jev reads them, answered as one JSON object; an answer outside its options is counted and dropped. Each
  request is reserved at catalog prices in memory against the run's cap, `[judge] audit_limit_usd` ($5.00), and the run
  stops before a reservation would pass it, saying so. Each answer is a `judge.label` with `source: audit`, weight 0.5,
  `rule` the run's id, keyed once per judgment, question and run, in one frame with the `judge.audit` row.
- **Backfill** (41dbb922; `learning/backfill.rs`, 299 lines; `theseus judge backfill <pack> --since <date>`). Refused
  without `[judge] backfill_consent = true` (false by default; a line of the owner's note, which agents can't write),
  and for a pack whose input can't be rebuilt, with the reason. The events: `turn.ended` rows since the local day's
  midnight that ended with no tool calls, in the pack's shadow sample. Each state is rebuilt at the event's time and
  judged once in shadow, keyed by its event (pack, session, turn), skipped when a live judgment of that turn exists,
  with the live point's context plus `purpose: backfill`, the run and `event_at_ms`, which the holdout's split now reads
  as the judgment's time (it had read the row's write time, so last week's event judged today would have landed after
  any frozen window). Each run records the consent's digest.
- **The owner's runs.** `Act::JudgeRun { method, what }`, judged by `judge_act` as every approval-like act: the owner,
  from a private place; a refusal is an `approval.refused` row with a sentence. The three methods are in the CLI's
  `OPERATORS`, so a job's shell refuses them before anything is sent. Each run is a `learning` thread at nice 19,
  holding the core weakly until it starts; the RPC awaits its result on a oneshot. One dispatch arm serves the three
  (`rpc/judge_runs.rs`), keeping `dispatch` inside clippy's 100 lines. Protocol types with their TypeScript; three ledger
  kinds; theseus-core's AGENTS.md (02ca7980).

**How it is proven.**
- **The session's tests:** `tests_replay` (5), `tests_audit` (4), `tests_backfill` (4) and theseus-judge's `replay::tests`
  (3). Among them: `loop.v2` (one option's meaning reworded) over a frozen holdout of four seeded `loop.v1` judgments,
  three labeled, against a fake scripted per state: 4 called, labels `frozen`, one fixed and one broken, every request
  carrying the new criterion, the calls in `judge.replay:loop` and none in `judge:loop`, and a later report counting
  `loop.v1` 4 and no `loop.v2`; a thresholds-only candidate making no call; a state rebuilt from a real turn, and
  `security.v2` over a `security.v1` judgment left out with its reason while the planted set runs; an audit of 3 of 5
  writing 6 labels and dropping 6, the next run drawing the 2 left; three live `loop.v1` judgments' blobs equal to
  backfill's rebuilt states byte for byte; a backfill judging each event once under consent. Under four busy loops,
  13 of 13 three times.
- **The review** (R4, in cloud-r2, on bddfd407): the build and clippy clean, theseus-protocol's tests leaving the
  generated types unchanged; **412 of 412** tests; **7 of 7 planted reverts caught**, the session's four (replay rows
  written into `judge:<pack id>`, `left: 0, right: 4`; the consent check dropped; the audit past its cap, `left: 5, right:
  1`; backfill keyed by run) and three new (a thresholds-only candidate read as one that asks; a backfilled judgment's
  time read from its row; a replay whose `judge_act` refusal is ignored).
- **Live, at the review** (a scratch daemon of the merged debug build, a fresh state dir, GLM-5.3 Flash and Jev live;
  about $0.0015 in all). Four asks made four `loop.v1` judgments, three labeled, and a report. **The audit** printed
  `4 of 4 eligible judgments sampled, 4 asked, 0 failed; 24 audit labels, 0 answers dropped · $0.0012 (limit $5.00)`;
  the same command again printed `0 of 0 eligible` and wrote nothing. **The replay** of a reworded `loop.v2` by ids:
  `4 called (4 stored states, 0 rebuilt)`, `$0.0002 of an estimated $0.0003 (limit $0.50)`, both columns, `agreement
  with the incumbent: 100% · fixed 0 · broken 0` (the reworded meaning moved one question's bands, 25/75 to 0/100);
  over the holdout, 0 judgments (every judgment was today's), and a second report still counted no `loop.v2`. **The
  backfill** without consent: "a backfill sends your recorded history to Jev, so it runs only under your consent:
  `backfill_consent = true` under [judge] in your config note, which agents can't write. Nothing was sent." Inside a
  job's shell, the replay was refused before anything was sent.

**What the session and the review found.** Only `loop.v1`'s input can be rebuilt: security's needs the gate's decision
and hold, inbound's the live tasks and roles, CONTINUE's the compile's tail, categorize's the ontology as it stood,
rerank's the candidates; and packs whose questions draw on dynamic items can't be replayed from a stored state, since
the record keeps the state's JSON, not the builder's items. Recording each point's builder input in its judgment's
context would open both. The live `loop.v1` point reads the session's nodes when its task runs, so a message that lands
first becomes the judged ask (it should read up to the turn's end). **The review's finding, theseus-b7rw (P3):** a run
holds each call's cost in memory and writes every row at its end, so a stop mid-run leaves up to $0.50 of Jev calls (or
$5.00 of an audit's model calls) spent with nothing in the record; the report had said a crash loses at most one
request's cost. A replay by ids counts a judgment with only audit labels as labeled.

**FAST.** Nothing at the start, the stop or a turn: the runs build their client on first use, inside the run; the
holdout's `event_at_ms` read is the learning tender's, nightly, after serving.

**The join** (the batch-5 joiner; lock `cloud-replay-join` taken 17:24:07, after lane cockpit-swell's join). The merge
on 14bcce94 conflicted in nine files: rerere replayed confirms.rs, `fact/answer.rs` and the CLI's `client.rs` from the
review, and the review's `resolve.py` (as R4 extended it at 17:02 for a main past stack A) resolved the core's
AGENTS.md, ts.rs, `learning/mod.rs`, `learning/report.rs`, the generated protocol index and `long-files.txt` (the
protocol's `lib.rs` to 2,702, with the reason). Then the review's join fixes: (1) theseus-judge's `builder_identity` and
(2) the core's `unrebuildable` gain the memory pass's `Memory` and `Attribution` builders, which are left out of a
replay with the reason and refused by a backfill; (3) the protocol's type-list test folded back to 100 lines. Two more
the warm found, the joiner's, both semantic conflicts with rerank-live and neither changing behaviour: (4) the audit's
label gains `about: None` (an audit asks whole questions only, so its labels never name an item); (5)
`learning/report.rs`'s three reads of `pack.as_deref()` read `pack` (clippy). The staged tree differed from the review
commit c3e9c85f plus main's gains only in the keep-both regions, and equalled the joiner's dry run of the same merge.
The warm was clean (17:34:49), and the replay, audit, backfill, learning and per-item label tests passed, 30 of 30.
**The gate** (17:36:38 to 17:44:43, `gate: ok`): suite **2,444 of 2,444** (17 skipped); lifecycle with no quiet
window (IO pressure 17 %, load 5.19), but every phase strict: cold start p50 22.0, p95 36.0 ms, vault 22.6/36.9, clean
shutdown 36.6/64.2, SIGKILL and restart 27.8/37.0, swap 50.1/54.0; jobs' L1 start p50 6.24, p95 7.04 ms; turn frames **5
and 9**, wall p50 plain 72.2 and tool-call 176.6 ms (the daemon's 49.0 and 147.0). Pushed 14bcce94..d5a4b808, the
cloud branch deleted, the done line 17:45:51, theseus-0j2.14 closed. The learning loop's cloud row (25f) waited on this
join and the ladder's.

**The install** (install #2, 20:00, at 3085f71a; its health as in Item 139). The three runs are the owner's
commands; `backfill_consent` stays false until he writes it.

**Divergences.** The runs' spend is a per-run cap in memory, never the shadow day budget: the session read "a replay
never pauses shadow judging" as "it does not draw on the day budget", where §2.6 says the judge's own budget pays. The
audit's reservation is held in memory, not as a kernel action, since an audit has no session to plan it under. The
consent's digest is sha256 of the running config's JSON, not the owner's note's own bytes.

**Known gaps.** theseus-b7rw (P3, above). Only `loop.v1` rebuilds. The morning notes' section 31 lists, among the small
defaults the DM thread settled for Eddie to overrule, "replay and audit spend counts in the judge's day budget"; as
built, the runs keep their own caps and their cost is only in their rows. Docs owed by the review: m5 §2.9's "As built
(25d)" paragraph (the report gives it whole), §2.15's `[judge]` lines, §2.6's table (the runs' per-run caps), and the
join's: the memory pass's two builders are unrebuildable; an audit's labels are whole-question.

### Item 145. The ladder: each pack's mode kept in the store, with arms, a promotion bar, rollback by rules and the live packs' adoption, and `route.v1`, `rerank.v1` and `security.v3`'s notices put on it at the join (theseus-0j2.15, with theseus-9j7x; step 26a, roadmap row 44; the fifth cloud batch's ladder session, its task written by writer M, fired 11:21 from 802f9135 by the DM thread, Opus 5.5; 05604b8d, f4680bfd, 06e67869 and 7d199fee; reviewed 14:31 to 15:21 by the local reviewer R4, the second of stack B, with one join fix and, after route's join, a second; joined 18:26 at d56f3bc0, a signed merge onto d5a4b808, by the batch-5 joiner, with the ladder integration; the join reviewed 19:22 by the DM thread; no store format change; installed 20:00 at 3085f71a, install #2)

**Why.** Until this step a pack's mode was its `WIRED` line lowered by the config (`mode_of`): every point asked
`cfg.mode_of(pack, Shadow)`, so promoting a pack meant a new build, and nothing could roll one back when it misbehaved.
M5's design (`docs/design/m5-judgment.md` §2.5 and §2.7) keeps the mode in the store: `pack.mode` rows, a canary's
arms, a promotion with a bar the learning report sets, rollback rules, and the owner's word. Step 26a builds that
ladder; the learning loop (25f) promotes on it. The same afternoon three packs went live by their own switches
(route.v1, Item 139; rerank.v1, Item 141; security.v3's notices, Item 142), so the
review asked that this join put them on the ladder too (theseus-9j7x).

**What landed** (four code commits; the merge with the integration 61 files, +4,182 −145; with the paperwork 48 files,
+4,083 −58 before it; no new package; no store format change: `pack.mode` and `pack.event` are ledger rows).
- **The mode in the store** (f4680bfd, `crates/theseus-core/src/judge/ladder/`). `pack.mode` rows, scoped
  `pack:<id>` (a version, its mode, from, share, who, by, via, why, the report and its holdout's bounds, forced, the
  numbers, the rule and its words, `until`, the card's question, declined), and `pack.event` rows. `Rung` is the four
  modes and `rolled_back`, which acts as shadow and which the config can never name. `fold` lays a version's rows over
  its wired line: a declined row changes nothing, and a brake whose `until` passed leaves what it stood on. The
  ladder is read once and kept; `Core::warm_ladder` reads it after serving, on the blocking pool, judge on only, and
  health never loads it itself.
- **One answer for every point.** `JudgeService::mode_for(pack, session)`; `ask_mode` gives a point the judgment's mode
  and writes its arm. Every judged point asks it: the loop end, the gate, inbound, compile, categorize and rerank.
  `mode_of` is unchanged and stays a ceiling: the config lowers the ladder's mode and never raises it, and a ceiling at
  shadow or below answers without reading the ladder.
- **Arms** (`given_of`). A canary in its arm (`learn::arm(session, pack, share)`) acts as live, its judgment's mode
  `canary`; the control is shadow, arm `control`; live and shadow are arm `all`. Nothing is stored on the session. The
  arm goes into a judgment's context as `pack_arm`, since rerank's context already uses `arm` for memory's arm.
- **Promotion** (`ladder/promote.rs`; `pack.promote`, `pack.rollback` and `pack.list` in one dispatch arm; `theseus
  packs [list|promote <pack> --canary <share> | --live [--report <id>]|rollback <pack>]`; both moves in `OPERATORS` and
  through `judge_act` as `Act::Ladder`). The bar: a `judge.report` row of the version (cited, or the latest of the last
  14 days, by key), whose holdout passes `learn::sufficient` (200 labels per deciding question, 30 per acting class),
  in which the version beats its baseline, read as every acting class's labeled precision above 0.50 (today only
  `loop.v1`'s `work_state: progressing`). The system is refused short of the bar, with the numbers (`loop.v1 to live
  refused: short of the bar: work_state: labeled 37 of 200`); the owner may force, and the row says `forced` with the
  same numbers.
- **A security pack's promotion is a card**, the owner's or the system's. A question is a planned action on an
  execution, and a promotion has none, so the card is planned on the ladder's own session ("the ladder: promotions
  waiting on the owner", META `ladder.session`): one short turn that parks again, listed by `confirm.list`, expiring as
  any question does; it never runs a model turn. Approval writes the row in the answer's frame, and a decline or an
  expiry a `declined` row with no mode. 7d199fee: an answered card is read back at once (`reload`, not `forget`), which a
  real daemon's test found.
- **Rollback** (`ladder/rules.rs`). Every event a rule counts lands through `JudgeService::land(pack, event)` as a
  `pack.event` row scoped `pack.event:<id>:<day>`, so a restart reads the day back; then every acting version of that
  pack id runs `learn::check_all` on today's events after its latest row (a promotion starts the count again). A pack in
  shadow is never rolled back. A label lands as an event in words (`noise`, `useful`, `wrong role`); the nightly run
  rechecks every pack after the report. A rollback is a `pack.mode` row (`rolled_back`, `who: system`, `via: ladder`,
  the rule and its words), a narrative sentence and health's line; the owner's rollback stands until a promotion.
- **Adoption** (05604b8d, `ladder/adopt.rs`). theseus-judge's `learn.rs` gains `PinsPerDay` and `OpensPerDay`, with
  events `Pinned` and `BreakerOpened`. `ADOPTED` names route.v1 (3 pins a day), rerank.v1 (2 breaker openings a day)
  and security.v3 (more than 30 notices, or 3 labeled noise, a day), read beside each pack file's own rules; a rollback
  by one of them is a day's brake, until the next local midnight. A pack is adopted only when the build wires it live,
  so its first read writes one row, `live (owner: decision of 2026-10-04)`, once.
- **Surfaces.** Health's judge line per pack: unchanged for a pack with no row; with one, `route.v1: live (owner:
  decision of 2026-10-04)`, `loop.v1: canary 1.0 (owner: forced by the owner)`, `security.v3: rolled back until 00:00
  (notices_per_day)`, or under a lower ceiling `loop.v1: shadow (the config's ceiling; on the ladder: canary 1.0 (…))`.
  `theseus packs` lists each version's mode, why, rules and last five rows. The cockpit's Judgment section gains a
  Ladder panel (`PackLadder.tsx`) with promote and roll-back buttons, off while the time machine is set (06e67869).

**How it is proven.**
- **The session's tests:** theseus-judge's adopted rules firing on a day's pins and openings and not on a near miss;
  the ladder's unit tests (rows folded, a brake lapsing, arms sticky and monotone, the next midnight, the bar's
  numbers); `tests_ladder.rs`, 11, whole cores on their own stores (the config lowers and never raises; short of the bar
  the system is refused and the owner forces; a security promotion is the owner's card, a shared place's answer
  refused; a rule's trigger rolls a pack back, and a pack in shadow is not; adoption once after serving; each adopted
  rule fires on its scripted events and not one short; a restart keeps the day's count; a brake lapses at midnight on
  tokio's paused clock; `max_mode = "shadow"` caps every pack after a restart; a canary judgment records its arm);
  theseusd's real-daemon ladder test over the socket, with a restart under `max_mode = "shadow"`; the CLI refusing both
  moves inside a job. Five planted reverts, each caught. Under four busy loops, 38 of 38 five times. The turn bench, 5
  and 9 frames; a lifecycle run ok.
- **The review** (R4, in cloud-r2, merged on replay's review commit): the build, clippy and the protocol's tests clean;
  the cockpit's lint, 50 of 50 tests and build; **422 of 422** in the shared judge and learning set and **118 of 118**
  in the ladder's own and its neighbours', at a load of 33; **8 of 8 planted reverts caught**, the session's four (the
  config raising a mode; a security promotion without its card; the adoption written at every start; the day's count
  reset at a restart) and four new (an automatic promotion short of the bar let through; a pack in shadow rolled back;
  a day's brake that never lapses; a promotion whose `judge_act` refusal is ignored).
- **Live, at the review** (a scratch daemon of the merged debug build, a fresh state dir, GLM-5.3 Flash and Jev live;
  about $0.004). Nothing adopted on that main (nothing wired live yet). `packs promote loop.v1 --canary 1.0`: "loop.v1 is
  canary 1.0, forced short of the bar (no learning report of loop.v1 in the last 14 days)"; the next turn's `loop.v1`
  judgment `canary`, `pack_arm: canary`; a rollback by hand narrated ("It records in shadow and acts on nothing.").
  `packs promote security.v1 --live` gave a card and no row; inside a job's shell it was refused; approved, `security.v1:
  live (owner: forced by the owner (approved))`; a second card declined left health unchanged. security.v3 promoted the
  same way, three `proc.run echo hi` calls judged live, and three `noise` labels: after the third, `security.v3: rolled
  back until 00:00 (labels_per_day)`, the row `who: system`, `via: ladder`, `until` the next local midnight, and
  **security.v1 rolled back too**, since events count by pack id. After a stop and a start under `max_mode = "shadow"`:
  no new adoption row, both rollbacks still holding from the day's events, and a promoted pack shown as `shadow (the
  config's ceiling; on the ladder: canary 1.0 (…))`.

**What the review found.** The memory pass (Item 136, joined after the branch's base) was a judged point
the ladder never saw: its two packs asked `mode_of`, so a promotion, a rollback or an arm would never reach them. Join
fix 1 moves them onto `pack_on` and `ask_mode` (seen live, `memory.v1`'s judgments carrying `pack_arm: all`; no test
fails without it). Filed: **theseus-9j7x** (P2: the live packs must take their mode from the ladder and land their
events, whichever joins second); **theseus-289c** (P3: the ladder's first read, and the day's rollover, can run on a
runtime worker inside a turn, and once adoptions exist that first read writes rows there); **theseus-0rba** (P3:
`theseus confirm` words a ladder card's approval as an extension's ack and its decline as a budget question's).

**The join and the integration** (the batch-5 joiner; lock `cloud-ladder-join` taken 17:46:04, theseus-9j7x claimed).
The merge on d5a4b808 conflicted in sixteen files; rerere replayed seven from the review. Then, in order: the joiner's
`ladder_resolve.py` for the six files where stack A meets the ladder (the gate takes each pack's mode from `mode_for`
for the call's session, the ask recording the arm; rerank's `at_recall` asks `rerank_mode(session)`; health's lines
are the ladder's `pack_lines`; the notices' `after_label` runs before the ladder's `land_label`; the cockpit's Judgment
view keeps both panels); R4's `resolve.py` and `joinfix.py` (the memory pass's fix, and route.v1 onto the ladder:
inbound's other packs and `route_mode` ask `mode_for` with the session); the joiner's integration; its build fixes; its
tests. A dry run of the same sequence on `git merge-tree d5a4b808 ladder` gave the same staged tree, 54 files of 54.
**The integration** (the brief's four items):
- **Adoption** needed no code: this build wires the three live, so a store's first ladder read writes their three rows
  once.
- **Every acting decision from the ladder.** rerank.v1's `rerank_mode(session)` is `mode_for(RERANK_PACK,
  session).mode`; security-notices' own `given` and `mode` are gone, and the gate asks `mode_for` per call,
  `notices_live` per session; **the notices' switch moved into `JudgeConfig::mode_of`** (`notices = false` caps
  security.v3 at shadow), so the ladder's answer, health and the gate read one ceiling; route.v1's ask at inbound
  records the ladder's arm and mode where its turn gave it live, and shadow for a pin or `[routing]` in shadow.
  `flagged` takes v3's judgment live, or canary in its canary arm, since a v3 canary would otherwise show live in
  health and never post.
- **The events, one `land` each.** route.v1's `Pinned`: after each routed turn's decision (route acting: live, Jev's
  breaker closed) the session's routed profile is kept, in memory as route's late verdict is, and a message the owner
  pins to another profile within 10 minutes lands once per routed turn, off the turn's path (`spawn_blocking`, since
  `land` writes a frame). rerank.v1's `BreakerOpened`: in the sink, after the frame with its `judge.circuit` row, for an
  opening of rerank's own breaker, not a probe's reopening. security.v3's `Notice`: in `post_notice`, after the notice's
  frame.
- **The notices' brake by key.** `rules::brakes_today` reads `store.ledger_by_key(&notice::paused_key(day))`, never a
  scan of `judge:security`, and `pause_notices` has the ladder read itself again at once, so the brake shows the moment
  it is written. Both brakes lapse at local midnight.

The build's join fixes: inbound.rs's and rerank.rs's imports (`Mode`, `PackMode`, which route's and rerank-live's code
still named); security-notices' `discord: None` in the ladder's `noise` test; rerank's breaker count moved into a helper
(`count_opened`), since inline it took the sink's `write` to clippy's cognitive complexity 26 of 25. **New tests:** the
three live packs adopted once after serving, with health's words and none at a restart; the notices' brake read by its
key and lapsing at midnight (a same-shaped row under no key is not the brake); a ladder rollback of route, of rerank and
of v3 each stopping it acting, with health saying so; a pin after a routed turn landing on the ladder; and event
assertions in rerank's five-timeouts test and the notice test. Tests that listed health's packs moved to the adoption's
words. Before the gate, on the final tree: the judge, learning, route, rerank, notices, security, inbound, continue,
categorize, memory-pass, recall, replay, config, template, registry, golden and frame-budget suites **259 of 259**;
theseusd's judge, versions and default_config **15 of 15**; theseus-protocol 29, the CLI 142, theseus-judge 126,
theseus-discord 105; the cockpit's lint, tests and `tsc -b` clean; the warm clean (18:03:27). **Planted reverts, 7 of 7
caught**: rerank's mode back on the config; the brake back on a scan (`left: (Live, None, None)`, `right: (RolledBack,
Some("notices"), Some(…))`); the pin never told; the breaker's opening not counted; a posted notice not counted; the
ladder's `land_label` before the notices' `after_label` (the ladder rolls v3 back on the third label, and the notices'
brake, asked after, finds v3 no longer live and writes no pause: `left: 0`, `right: 1`); the gate's packs on the config's
mode. **The commit** d56f3bc0, signed, parents d5a4b808 and 63622d41. **The gate** (18:14:49 to 18:25:47, `gate: ok`,
278 s of it waiting behind lane linux-jobs' workspace test run): suite **2,471 of 2,471** (17 skipped); lifecycle run 1
missed one budget on one outlier (the vault start's p95 59.1 ms against 57.1; p50 28.8, minimum 25.2), and the gate's own
rerun passed every phase strictly (cold start 22.3/32.6 ms, vault 21.8/23.4, clean shutdown 29.6/42.6, SIGKILL and
restart 25.7/25.9, swap 47.1/57.5); the ladder adds nothing before serving. Jobs' L1 start p50 6.25, p95 7.07 ms; turn
frames **5 and 9**, wall p50 plain 74.9 and tool-call 175.9 ms (the daemon's 49.0 and 148.0); with the judge off in the
bench, `mode_for` answers off at once and reads no ladder. Pushed d5a4b808..d56f3bc0, the cloud branch deleted, the done
line 18:26:14, theseus-0j2.15 and theseus-9j7x closed; a note on theseus-289c that the adoptions it foresaw now exist.

**The DM thread's review of the join** (19:22): accepted; the six integration tests (adoption once, the notices' brake by
key, a pin landing, a rollback stopping route, rerank and v3) 6 of 6 on main. Three integration items went to Eddie:
security has two brakes on the same counts (the notices' own, which fires first, and the ladder's adopted rules for pack
id `security`, which act only for a security version without notices); a v3 canary now posts in its arm; route's pin
counts against the profile the routed turn ran on, a trivial detour's included. Settled by the DM thread that night for
him to overrule (the morning notes' section 31): ladder rollbacks stay per pack id, and the integration items as built.

**The install** (install #2, 20:00, at 3085f71a). The store's first ladder read on the owner's daemon adopted the three:
health after, `route.v1`, `rerank.v1` and `security.v3` live (owner: decision of 2026-10-04); store format 16; startup
serving at 29.1 ms. Since 18:02 Eddie judges route.v1's decisions in the cockpit's Judgment view: its adopted rule
(three pins a day) is a day's brake, read beside its pack file's own ("wrong model" three times a day; an on-path p95
over 250 ms over 20).

**Divergences.** Steps 1 to 5 share one commit (the ladder is one module whose parts call each other). `pack_arm`, not
`arm`. "Beats its baseline" is read as the acting classes' precision above one half; a sharper comparison needs the
baseline's own outcomes on the same holdout (26b). A pack is adopted only when wired live. The rollback notice is a
narrative line, as `judge.paused` is, with no Discord post.

**Known gaps.** Discord buttons for a security card are not built: the ladder's session has no place, so the card shows
in `theseus confirm` and the cockpit only. A label's `pack.event` row is a frame after the label's, so a crash between
them loses that one count. `CanaryEvent::Label` carries no judgment, so three labels on one judgment count three. Events
count by pack id (a v3 label rolls back v1 too, and makes a v1 rollback a day's brake). Memory's canary is untouched
(its hash differs from `learn::arm`'s, so moving it would move a running experiment's sessions between arms).
`theseus packs` under a lower `max_mode` shows the ladder's mode, with the cap only in its header. theseus-289c and
theseus-0rba (P3, above). Docs owed: m5 §2.5's `pack.mode` row (until, question, declined, numbers, `pack.event`), §2.7
(the adoption, the brakes, where a security card's question lives, every acting point asking the ladder, the notices'
switch as a ceiling), §2.13 (`theseus packs`, the Ladder panel).

### Item 146. The review's small changes to tasks and places: layer 1 guards only the owner's tasks, a place with a bad ceiling fails alone with its warnings, and a check sees the checked task by title and state; store format 16 (theseus-ext.10, theseus-ext.11 and theseus-w8ys; Eddie's decisions 6 and 7 of 2026-10-04 10:38 and 10:45, and the DM thread's w8ys call; the fifth cloud batch's smalls-tasks session, fired 11:55 from e27405a2 by the DM thread, Opus 5.5; a333d4bb, a07ca47e and 7eef7961; reviewed 15:52 to 17:20 by the local reviewer R4, the last of stack B, with no join fix; joined 19:07 at 0c9edcc2, a signed merge onto d56f3bc0, by the batch-5 joiner, with one comment fixed; store format 15 to 16; installed 20:00 at 3085f71a, install #2)

**Why.** Three reviews of the morning's joins left a small change each, and Eddie took the recommendations at the 9am
review. **Decision 6** (39a, task-record, Item 125; 10:38): layer 1, a task's objective and acceptance and
abandoning it, asks only for tasks the owner created or whose objective and acceptance came from his brief; the model's
own plan items change freely, still versioned and visible; and an expired proposal is cleared as a decline is.
**Decision 7** (38a, bindings-v2, Item 117; 10:45, "Take all of these excellent recommendations"): an
unknown profile fails only its own place, with the reason in health and at the start; unknown tool families show in
health; and a place whose spend limit is below one call's worst case on its profile is warned of. **theseus-w8ys** (28a,
independence, Item 132; asked without a recommendation, so chosen by the DM thread at 10:45): in a check's
view, the checked task and its subtasks show title and state only, no notes and no evidence.

**What landed** (three code commits; the merge 36 files, +1,253 −142; with the paperwork 38 files, +1,637 −142; no new
package).
- **Layer 1 guards only the owner's tasks** (a333d4bb, theseus-ext.10). `authority_of` had decided from the input
  alone, so every `task.update` of objective or acceptance and every abandoning `task.close` asked, plan items
  included. `TaskOrigin.by_model` (theseus-protocol, skipped when false) is set for a plan item and for each
  `task.split` child, and false for a task session `task.create` opens with a brief; absent reads as the owner's.
  `task_graph::tools::authority_for` drops the plan's authority before the gate when the named task is the model's
  (three lines in toolrun.rs); a task that cannot be read keeps it, the safe side. `lock_for_call` and `proposed`
  follow it, so no proposal is ever written on a plan item, whose layer 1 applies at once as `task.updated` or
  `task.closed`, version plus one. The task view marks the owner's tasks (", the operator's objective") and its head,
  with the tools' descriptions, says only a marked task's change waits. **An expired change** (`expire_question`,
  about 15 lines) takes the task's lock and clears the proposal in the expiry's own frame, with a new
  `task.change_expired` row and its narrative line ("the change to task … expired unanswered; it stays as it was");
  the expiry still writes 2 frames on its thread.
- **The store's format** moved for `TaskOrigin.by_model`, with a format-14 plan item written by hand in the old
  layout as a layout sample (`TASK_BEFORE_MODEL_MARK`), read byte for byte: 15 on the branch's base, which route had
  taken on main (Item 139), so **16** at the join.
- **A bad place fails alone** (a07ca47e, theseus-ext.11). theseus-discord's `runtime/guilds.rs`:
  `unbind_unknown_profiles` takes each place whose ceiling names an unknown profile out of the bindings it serves (no
  route, no session, no answer, like a channel the file does not name), and `tell_core` binds the rest; the whole
  binding no longer fails. The core's `place_warnings.rs` (new) makes each place's warnings: `unbound`,
  `unknown_family` (moved out of `bind_places`), and `limit_below_call`, priced with `reserve_micros(effective_max_tokens,
  0)` on the ceiling's profile, else the live one, worded "#pier's $1.00 limit is below one call's $1.28 on sonnet
  (before its input)"; an unpriced model gets no figure. Each is a WARN in the log, a `place.warned` row with a
  narrative line, once a start, and health's `places.warnings`, which the CLI prints under `places:` and in `theseus
  places`.
- **A check sees the checked task by title and state** (7eef7961, theseus-w8ys). `task_graph/view.rs`'s
  `render_for(tasks, session, check)` (every other view byte-identical) and `restricted_ids` (the checked task's subtree
  and the excluded sessions' records, never the check's own task): their lines are bare, `id "title" [state]`.
  `TaskViewSummary.restricted` counts them, absent when 0; `attach` reads the session's `TaskOf.check` itself.

**How it is proven.**
- **The session's tests:** a plan item's and a split child's layer 1 applying with no question at the template's
  posture, and abandoning applying at once; an old record (the layout sample) read as the owner's, its change waiting,
  and its expiry clearing the proposal at exactly `expires_at_ms`; the existing layer-one test moved onto a task
  session opened with a brief; one place's unknown profile leaving only it unbound against the REST stand-in, with
  health's three warnings and three rows; the limit warning at $1 and not at $2; a check's view, pure and through a
  whole core (every request of the check's has the maker's line bare, and its `context.compiled` rows carry
  `tasks.restricted`). Four planted reverts, each caught. Under four busy loops, 8 of 8 five times.
- **The review** (R4, in cloud-r2, on smalls-tools' review commit): no conflict on the stack; the build, clippy, the
  protocol's tests and the cockpit clean; **476 of 476** tests (theseus-discord, theseus-protocol and theseus-store
  whole among them, and theseusd's versions at formats 15 and 16); **7 of 7 planted reverts caught**, the session's four
  (brief tasks given the plan item's mark; the expiry's clear skipped; one unknown profile unbinding every place; the
  checked task rendered whole) and three new (the restricted lines not counted; a limit below one call not warned; the
  harness's word skipped, so a plan item waits again). The turn bench on the whole stack: plain 5 frames, tool-call 9.
- **Live, at the review** (about $0.01 of GLM). A plan item's `task.update` raised **no card** and moved it to v2; a
  briefed task's acceptance change **waited on a card**, "layer 1: changing task tsk_… 's acceptance"; unanswered for
  a minute (`confirm_ttl_secs = 60`), the proposal was gone, still v1, with one `task.change_expired` row and the
  narrative's "expired unanswered" line. On the Discord stand-ins with no keys: the binding `ready`, three warnings
  ("#lab is not bound: its ceiling names profile "nosuch", which the config does not have …", the unknown family, and
  **"#pier's $1.00 limit is below one call's $1.28 on sonnet (before its input)"**); the DM answered, `#lab` was
  silent, and `#pier` asked its budget question for exactly that $1.28. A check of a finished task, with `parent` set
  to it: every `context.compiled` row of the check's session carried `tasks.restricted: 1`.

**What the session found.** A task session's scope is its own subtree plus its parent's line, so the checked task
appears in a check's view only when the check sits under it (`parent`); with no `parent` the check never saw the checked
task at all. The rule is built over the whole graph, so it holds wherever such a line shows.

**FAST.** Nothing at the start or the stop; the binding's start, after serving, also computes the warnings. On a turn,
every tool call's gate gains one `if plan.authority.is_some()`, and only a layer-1 change reads the task's record; the
compile's view computes `restricted_ids` only in a check's session.

**The join** (the batch-5 joiner; lock `cloud-smalls-tasks-join` taken 18:47:31; it did not wait on smalls-tools,
whose gate had failed at 18:43, Item 147). The merge on d56f3bc0 conflicted in two files: theseus-store's
store.rs and toolrun.rs, where gliding's glide check sat where this branch's `authority_for` goes (Item 140).
The review's `resolve.py` kept both (the harness's word on layer 1, then the glide's check) and **renumbered the
format to 16**, route's 15 plus one: the constant, the core's store pin, theseusd's versions test (writes 16, refuses
17, reads 2 to 16), and the layout sample's labels. **Join fix 1** (the joiner's, a comment the renumber missed):
tests_layouts.rs's "its layout before 15" now says 16. The warm was clean (18:51:07). Before the gate: the branch's and
its neighbours' suites 156 of 156, theseus-discord 106 of 106, theseus-store 60 of 60, theseusd's versions and tasks 14
of 14. **The gate** (19:01:40 to 19:07:31, `gate: ok`): suite **2,479 of 2,479** (17 skipped), the kernel tree test that
had failed smalls-tools' gate among them; lifecycle strict: cold start p50 21.2, p95 22.1 ms, vault 22.0/31.1, clean
shutdown 33.7/50.1, SIGKILL and restart 25.5/26.7, swap 53.8/59.1; jobs' L1 start p50 5.67, p95 6.71 ms; turn frames
**5 and 9**, wall p50 plain 80.1 and tool-call 176.5 ms (the daemon's 52.0 and 148.0). Pushed d56f3bc0..0c9edcc2, the
cloud branch deleted, the done line 19:07:43, theseus-ext.10, theseus-ext.11 and theseus-w8ys closed.

**The install** (install #2, 20:00, at 3085f71a). The owner's store moved from 14 to 16 at its first write (route's 15
and this 16 in one move), after the install's backup; health after, store format 16.

**Divergences.** A plan item made with an arrangement of the owner's own words is still the model's (the brief's line,
kept); only a task session opened with a brief is his. An unknown profile is now a warning, not a failed binding: a typo
leaves its place silent, failing closed for that place, and the rest answering.

**Known gaps.** Whether a check should go under its checked task by default is open (28a's and 39a's design question;
today it is the model's `parent`). The cockpit does not show `places.warnings` yet. Docs owed: theseus-core's AGENTS.md
"task graph" (layer 1 is the owner's tasks only; `by_model`, `authority_for`, `task.change_expired`, the format), "Check
tasks" (the restricted view, `tasks.restricted`) and "Ceilings", and theseus-discord's "Guilds and ceilings"
(`tell_core`, `place_warnings.rs`, `place.warned`, `places.warnings`).

### Item 147. The review's small changes to judging, language servers, the restore and the hands: `categorize.v1` on an empty ontology, a Choice's options reserved, three language servers starting on an edit, the restore's list under `StringLikeIfExists`, and AWS runaway-train mode (theseus-ext.12, with theseus-mgw.10's policy, theseus-gky0 and theseus-q0rn; Eddie's decisions 8 and 9 of 2026-10-04 11:00 and 11:26; the fifth cloud batch's smalls-tools session, fired 11:55 from e27405a2 by the DM thread, Opus 5.5; e7232b22, b3967836, ac433e56 and 5a569972; reviewed 14:56 to 16:26 by the local reviewer R4, the third of stack B, with no join fix; a first join's gate red at 18:43 on a kernel test it does not touch, theseus-g11i; re-gated and joined 19:43 at 3085f71a, a signed merge onto 0c9edcc2, by the re-gate joiner; reviewed 19:53 by the DM thread; no store format change; installed 20:00 at 3085f71a, install #2, with `start_on_edit = false` for rust-analyzer)

**Why.** The morning's reviews left four small changes outside the task layer, and Eddie took them at the 9am review.
**Decision 8** (11:00, "Take your recommendations. Add rust to the list of autostarted language servers"): 28b's
categorize point (Item 126) proposes topics from an empty ontology, reading only what is new
(theseus-gky0) and reserving for its options (theseus-q0rn); `start_on_edit` on for ty, TypeScript 7 (tsgo) and
rust-analyzer, rust-analyzer with a target dir of its own so its checks never take the agent's build lock; and the
point from L3's review (Item 127) that `lsp.diagnostics` right after an edit should not repeat the
edit's block. The restore of step 16 (Item 123) needed its list policy loosened so a missing object reads as
missing (theseus-mgw.10). And **decision 9** (11:26), on the hands' spend: "let the budget notify be the authority unless
'runaway train' mode is triggered, which is, observationally spend is 10x over the limit": the hourly line and the daily
budget stay alerts, and a `runaway_factor` (10) puts Theseus in runaway-train mode at ten times a line.

**What landed** (four code commits, the runaway mode in a commit of its own as asked; the merge 31 files, +1,201 −66;
with the paperwork 33 files, +1,591 −66; no new package; no store format change: new META keys, a ledger row, and a
health type).
- **`categorize.v1` on an empty ontology** (e7232b22). `prepare_categorize` had returned nothing when no topic was
  declared, so its mark never moved and every exchange end reread the session from its start (gky0). The return is
  gone: the Choice is then `new_topic` and `none` (the pack already offered both, so no pack change), and the mark
  moves. With no topic declared, the input is the human messages after the mark alone, never `session_nodes`; with
  topics and under ten messages since the mark, it still reaches back for the last ten.
- **A Choice reserves its options** (q0rn). theseus-judge's `price.rs` adds `INPUT_PER_OPTION`, 10 input tokens per
  Choice option, to every Jev reservation (`reserve_tokens`); `reserve_micros(bytes, output)` keeps its meaning. Score
  levels do not follow, for want of live evidence.
- **Language servers** (b3967836). `[lsp.servers.<name>] start_on_edit` is now optional: unset is on for
  `lsp::START_ON_EDIT` (rust-analyzer, ty, tsgo) and off for the other presets and the owner's own servers; `false`
  stops it. rust-analyzer's preset carries `cargo.targetDir: true` both in its initialization options and in its
  settings, since rust-analyzer replaces the first with what `workspace/configuration` answers; an owner's own settings
  for it must keep the line. `lsp.diagnostics` puts the files it lists in its result's `meta.files`, and a finished
  pending wait for one of them is taken without being said again.
- **The restore's policy** (ac433e56, theseus-mgw.10). `ListItsPrefix` uses `StringLikeIfExists` on `s3:prefix`, so S3
  answers a missing object's `GetObject` 404 rather than 403; a list with no prefix now passes, which shows key names,
  never an object's contents outside the prefix. The tests' fake now gives each assumed role its own key, keeps its
  session policy, and answers 404 or 403 by that policy.
- **Runaway-train mode** (5a569972, `aws/hands/runaway.rs`, 345 lines). `[aws.accounts.<id>] runaway_factor`, 10.0 by
  default and at least 2, checked at load. The figure is the hour's meter (reserved by running hands, cost of settled
  ones), per clock hour and per local day. At an `aws.hands.run` group's admission, after the session's budget check,
  `Sink::admit` refuses the group when the figure plus its own worst case reaches factor × the line
  (`hourly_alert_usd`, or `daily_budget_usd`); that refusal **enters runaway mode** until the period turns: one frame
  with META `aws.runaway.<account>` and an `aws.runaway` row, then one notice where approvals go. While it holds, every
  new reserving action is refused, the words naming the account, the figure, the factor, the line's key and dollars,
  when it ends, and the keys to raise. The poller's pass also enters it when the figure alone is at a line, and fills
  health (`AwsHandsStatus.runaway`, `runaway_until_unix_ms`; the CLI's `aws: <id> RUNAWAY: …`). Never refused: a cancel
  or `/stop`, a list or status read, the reaper, settling, and an admitted group's later waves. The template gains the
  line, with the owner's words, and its `daily_budget_usd` example is now 10.

**How it is proven.**
- **The session's tests:** categorize on an empty ontology dispatching with `candidates: 0`, offering `new_topic` and
  `none`, and reading only records after the mark; a 52-option and a 4-option Choice now reserving **104** and **39**
  µ$, at least their billed 96 and 32 (the old formula reserved 82 and 37, as the live calls had); the three presets
  starting on an `.rs`, `.py` and `.ts` edit and `false` stopping one, rust-analyzer's fake seeing `{"cargo":
  {"targetDir": true}}`; `lsp.diagnostics` after a pending edit listing its error once; the restore saying a missing
  blob is missing and restoring the rest; runaway mode at ten times the hour's line (a two-hand group at 9.9 × runs, the
  next at 19.8 × is refused with its words, one row and one notice, a third refused with no second row, the running
  group settling, the next hour admitting), at exactly 10 × refusing with a `/stop` still running, and the day's line
  the same way; `runaway_factor = 1.5` failing validation. Planted reverts, each caught. Under four busy loops, 30 of
  30 three times.
- **The review** (R4, in cloud-r2, on ladder's review commit): one conflict (config.rs's `aws` re-exports); the build,
  clippy, the protocol's tests and the cockpit clean; **628 of 628** tests at a load of 20, theseus-judge whole among
  them, so every pinned reservation held under the per-option input; **8 of 8 planted reverts caught**, the session's
  four (the empty-ontology return; rust-analyzer's options dropped, `left Null`; `StringLike` back on the list; the
  admission check skipped) and four new (a Choice reserving no input; the day's line never checked; the factor's floor
  lowered to 1; a diagnostics result's own files riding its attach again).
- **Live, at the review** (a scratch daemon of the merged debug build, GLM-5.3 Flash and Jev live, `[lsp] enabled =
  true`; about $0.01). Ten messages about a vegetable garden on an empty ontology: `topic=new_topic 0.92 act`,
  `candidates: 0`, 34 µ$ reserved against 25 billed; accepted as the ontology's first topic; then 50 topics and ten
  messages on sailing: `candidates: 50`, **128 µ$ reserved against 111 billed**. rust-analyzer **started on the edit with
  no server line in the config** (ready in 12.9 s, 622.3 MB), its checks built in the crate's `target/rust-analyzer`,
  and a `cargo build` beside it printed no "Blocking waiting for file lock". **But rust-analyzer reported nothing**,
  for a trait error or an unknown name, while its own CLI saw E0277; main as installed did the same, its checks
  building in the crate's own target, and its `lsp.diagnostics` result carried the file twice, where the branch's
  carried it once. Not run: the restore's and runaway's live checks (AWS and money; the owner's, after the join).

**What the review found.** **theseus-c6hv** (P2): rust-analyzer's errors do not reach the harness, on main too; it is
L3's, not this branch's, and what makes L3 worth having for Rust (fixed later by lane lsp-checks, Item 153).
Runaway mode has two readings for the owner: the check is prospective (a group whose own worst case reaches the product
is refused at $0 spent), and that refusal latches the mode for the rest of the period (dropping the latch is a two-line
change). Two groups admitted at the same instant can both pass (best effort, as the meter is). The durability tender's
session has no `s3:ListBucket`, so real S3 answers its `HeadObject` of a missing key 403, which the fake cannot show.

**FAST.** Nothing at the start or the stop. categorize's decision runs after the turn's last frame, as before, and
reads less. The price change is arithmetic. Runaway's `admit` runs only at a hands group's admission: a META scan of
the groups in a 13-hour look-back and kernel reads per hand. With `[lsp]` on, the first edit of a Rust, Python or
TypeScript file in a root starts its server, and the edit waits up to `edit_wait_ms` (1.5 s) for its errors;
rust-analyzer took 6 to 13 s to be ready at 620 MB on a one-file crate (4.2 GB on this repository; `idle_stop_mins`
frees it).

**The join: a red gate, a finding, and a re-gate.** The batch-5 joiner took `cloud-smalls-tools-join` at 18:26:27, after
the ladder's join; rerere replayed config.rs, and the review's `resolve.py` (extended at 17:13 for a main past stack
A) resolved `judge/categorize.rs` and theseus-judge's `judge.rs` (rerank-live's `lock` kept, `part` made pub(crate)).
The warm was clean and the branch's suites passed, 378 of 378. **The gate was red** (18:37:30 to 18:43:25): 2,477 of
2,478, one failing, `theseus-kernel::tree the_deadline_stops_the_whole_tree_too`: "a /proc scan found 300.1802 still
running", a deadline's stop that left a job's two sleepers alive past its completion. The branch changes no line of
theseus-kernel, and the test passed 10 of 10 alone on main at load 2.3; but a negative assertion that fails once is a
finding, not a flake to retry away, so it was filed as **theseus-g11i** (P2), main was reset, and the signed merge was
parked locally. smalls-tasks joined in between (Item 146). **The re-gate** (a joiner of its own, 19:22 to
19:47): the lock's earlier "done: NOT joined" line was reworded so no reader would take the re-taken lock as finished;
the take, the checks and the merge ran in one guarded call (`take-and-merge.sh`, with the joincheck); on 0c9edcc2, rerere
replayed all three conflicts from the first take, and the staged tree equalled the first take's merge carried onto
main, file for file (tree 77728b44); no join fix. The warm clean (19:36:09); the suites before the gate 535 of 535
(theseus-core 378, theseus-judge 127, theseus-lsp 30). **The gate** (19:38:05 to 19:43:11, `gate: ok`, quiet, so
strict budgets): suite **2,486 of 2,486** (17 skipped), the kernel tree test passing at a load near 7, a fresh gate,
not a retry, so theseus-g11i stayed open; lifecycle strict: cold start p50 23.8, p95 26.1 ms, the config copy 23.9/29.8,
clean shutdown with a job 34.1/43.9, SIGKILL and restart 26.5/27.9, swap 54.2/61.0; jobs' L1 start p50 5.97, p95 6.61
ms; turn frames **5 and 9**, wall p50 plain 81.1 and tool-call 180.8 ms (the daemon's 52.0 and 152.0), level with the
gate before. Pushed 0c9edcc2..3085f71a, the cloud branch deleted, the done line 19:43:51, theseus-ext.12 closed, the
park branch deleted. All seven of batch 5's branches were on main. theseus-g11i was fixed later by the kernel fixes
(Item 173).

**The install** (install #2, 19:59:53 to 20:00:01, at 3085f71a, this join's own commit). The precheck added
`[lsp.servers.rust-analyzer] start_on_edit = false` to the owner's config, with a backup, until theseus-c6hv is fixed:
ty and tsgo start on an edit, rust-analyzer does not. `runaway_factor` reaches the owner's daemon unset, so 10. Health
after: store format 16, the three live packs, 9 secrets ready, startup serving at 29.1 ms. Eddie at 18:07, on the brake:
"defense in depth -- both methods": a worst-case refusal trips runaway mode, and actual spend trips it too, as built.

**Divergences.** Runaway mode's prospective check and its latch (above), decided as built at 18:07. Score levels keep
their old reservation. The template's `daily_budget_usd` example became 10.

**Known gaps.** The restore's live check (under a cent: a blob shipped, one object deleted, the restore saying it is
missing, CloudTrail showing NoSuchKey and not AccessDenied, and the tender's `HeadObject` answer noted) and runaway's (a
few cents) wait for the owner's go. The cockpit's Systems cards do not show runaway mode yet. Docs owed: theseus-core's
AGENTS.md (categorize on an empty ontology, the hands' `runaway.rs`, `start_on_edit`'s defaults), theseus-lsp's
(`cargo.targetDir` in both places), m5 §2.12 (categorize, the per-option input), aws-toolset §3.7 (runaway mode) and
§3.5 / step 16 (the `IfExists` list).

### Item 148. B5: the first full Terminal-Bench 2.0 run, Claude Code 81.5 % and Theseus 71.9 % and 73.6 % on the same model, for $71.60, and the loss analysis behind the fixes (theseus-n88g.5, with theseus-7gir.1; Eddie's go of 2026-10-03 22:58; the `b5` lane, from 079f1dbf, its first agent aborted at 23:24:31 and relaunched at 23:51; the run 2026-10-03 23:21:20 to 2026-10-04 18:07:32, paused by hand 13:37 to 14:09; the docs commit 9c686904; the launch reviewed 00:31, the loss analysis 19:53 and the run 20:13, by the DM thread; joined 20:18 at 4cef410c, a signed merge onto 3085f71a, by the card-2 joiner, under one gate with card 2; docs only, in the tree of install #3, 21:22 at 645769d2)

**Why.** Item 93 built the benchmark plumbing: the Harbor adapter in the repo, a headless run that says how it ended,
and the bench profile. Its last line recorded B5 as under way: Eddie had approved the full Terminal-Bench run at 22:58
on 2026-10-03. B5 measures Theseus against Claude Code on the same model, over the whole of Terminal-Bench 2.0, and
puts a second Theseus arm beside it with one paragraph of batching advice in its system text, the question theseus-n88g.6
waited on.

**What ran.**
- **The set:** `terminal-bench@2.0`, all 89 tasks in their own containers, through Harbor 0.23.0, 2 attempts per task and
  configuration: 534 trials. One model for every arm, `anthropic/claude-sonnet-5-5`.
- **The arms:** **A**, Theseus with the bench profile as committed (`-a theseus_agent:Theseus`, no vault, every tool
  open, L0), from static `release-thin` binaries built at 079f1dbf; **B**, the same with one paragraph of extra system
  text (`THESEUS_BENCH_SYSTEM_FILE`); **C**, Claude Code 2.1.288 through Harbor's own adapter, its budget and turn caps
  set to Theseus's (`max_budget_usd=2.0`, `max_turns=200`).
- **The limits per trial:** $2.00, 200 model calls, and each task's own agent timeout, unchanged; the agent setup
  timeout tripled for all three arms, for Claude Code's install of Node and its CLI.
- **The driver** (`driver.py`, the user unit `theseus-b5.service`): at most 4 trials at once (two jobs of `-k 2 -n 2`),
  task by task with the longest agent timeout first and C first within a task; before each task the disk guard (35 GB
  free on C:), the spend check (a $600 pause line), an image snapshot and one pull; after it, the task's image removed by
  name and nothing pruned; a watchdog per job; an unfinished job moved aside and run again, its spend still counted;
  rate-limited trials run once more at the end as jobs of their own (none were needed). The operator's model key went
  only into each `harbor run`'s environment, never printed or written: a count over every file the run wrote found its
  value 0 times (9,657 files at the end).
- **The smoke test** (2026-10-03 23:15 to 23:18): `fix-git` through the driver, all three arms solved it, $0.1389, each
  trial's ATIF trajectory valid against Harbor's model.

**How it ran.** The driver started at 23:21:20. Three minutes later the lane's first agent was aborted with the DM
thread's run (23:24:31), before its launch report; the driver and the hourly harvest wake survived it. The relaunched
agent (23:51) re-tested every path of the harvest check on scratch copies, with every outside command faked, and found
one bug in its last edit: the done path marked Eddie as told before printing anything, so a missing or hung `bd` would
have left every later wake silent and Eddie never told. Fixed (the note's call bounded at 20 s, the mark written only
once the text is whole), 33 of 33 scenarios passed, and the first live wake (00:24) ran the fixed script. The run was
paused by hand from 13:37:53 (a `PAUSE` file at 13:32:52) to 14:09:56, around the afternoon's install, and finished at
18:07:32 with no driver error, no job failed twice, and 32 task images removed by name. Its finish wrote the summary,
filled and scrubbed `docs/benchmarks.md`, counted the key again, and committed 9c686904, signed, on `lane/b5`.

**The results.**

| | A. Theseus plain | B. Theseus + batching paragraph | C. Claude Code |
|---|---|---|---|
| Solved, attempt 1 / attempt 2 | 62/89 / 66/89 | 65/89 / 66/89 | 73/89 / 72/89 |
| **Mean pass rate** | **71.9 %** | **73.6 %** | **81.5 %** |
| Solved in both / in either | 59 / 69 | 60 / 71 | 68 / 77 |
| Dollars, total / per task | $24.72 / $0.278 | $24.23 / $0.272 | $22.65 / $0.255 |
| Model calls per trial | 8.4 | 8.3 | 8.0 |
| Agent time per trial, mean / median | 4.1 / 1.0 min | 4.4 / 1.1 min | 4.1 / 0.7 min |
| Input read from the cache | 87.5 % | 88.2 % | 92.9 % |
| Output tokens | 1,137,664 | 1,123,878 | 879,056 |
| Trials ending in a timeout / turn cut / spend limit / error / refusal / approval wait | 6 / 4 / 2 / 2 / 6 / 1 | 5 / 2 / 1 / 2 / 6 / 0 | 8 / 0 / 0 / 4 / 0 / 0 |

- **Spend:** $71.60 in all, every trial included, against the estimate of $180 to $540 and the $600 pause line.
- **B against A** (n88g.6's question): +1.7 points (+1.5 tasks), within A's own spread between its attempts (4 tasks);
  on the 118 pairs both solved, B used 11 % fewer model calls and 8 % fewer dollars. The report read that as the signal
  n88g.6 waited for, and changed no default.
- **C against A:** on the 122 pairs both solved, C spent 13 % fewer dollars and 15 % fewer model calls. Nine tasks C
  solved in some attempt and A never did; one the other way.

**The loss analysis** (theseus-7gir.1, a read-only lane over the run's files; reviewed 19:53). Of the gap of about 9.6
points, six tasks are strong losses (C solved both attempts, A neither), and five of the six have a mechanical cause,
none of them the prompt:
- **Three tasks to a provider refusal** with no fallback (`category: cyber`): Claude Code's Sonnet 5.5 refused the same
  three tasks, then fell back to Sonnet 5 and solved them; Theseus's catalog turns the server-side fallback on only for
  two other models, so for Sonnet 5.5 a refusal ended the turn (exit 7). Arm B refused on the same three, so the system
  text is not the lever (theseus-7gir.18).
- **Two tasks to the bench profile's 32,000-token output cap**, a quarter of the model's and Claude Code's 128,000: four
  of A's trials ended `max_tokens` (theseus-7gir.19).
- **One trial to an approval the open profile should never ask for:** a fetch of the task's own `localhost` page hit the
  private-address rule before the tool's posture was read, and with no operator the turn ended waiting (theseus-7gir.20).
- **One trial to a transient first-byte timeout** the one-shot `ask` parked instead of retrying inline
  (theseus-7gir.21).
- The sixth strong loss is the model writing different code (pytorch-model-cli). Timeouts were the task's own limit,
  and C had the most; the spend-limit trials were on a task every arm failed. The cache-read gap is the leaner prefix's
  (Theseus's first call is a fresh cache write), not waste; most of A's extra output is the four cut turns' 128,000
  tokens.

**What it led to.** Before the run ended, B6 (theseus-n88g.6, the batching paragraph as the default) was closed as
superseded by Eddie's no-prompt-tricks rule (16:10); the run's B-against-A evidence went onto theseus-7gir.3, the
`proc.run` steps array, the non-prompt route to the same saving. Fixes .19 to .21 ran in lane route-bench-fixes
(Item 150). Eddie's v1 rule at 22:09 ("A") counts each B5 loss, explained or fixed, among v1's
conditions; at 22:32 he chose the refusal fallback ("A": a refused request retried once on Sonnet 5, as Claude Code
does, the reply saying so and the log recording it; no prompt change), built in lane refusal-fallback
(Item 154). The held-out rerun (theseus-7gir.22) adds B5's three refusal tasks and matches Claude
Code's actual-spend budget rather than counting a call's worst case (settled by the DM thread that night, the morning
notes' section 31).

**The join** (the card-2 joiner; lock `lane-linux-jobs-card2-join` taken 20:06:44 in one guarded call with the first
merge). `git merge --no-ff` of `lane/b5` onto 3085f71a was clean: docs/benchmarks.md alone, +140 −3, its table's first
three rows, the run's setup, the summary table and the per-task table, replacing "No full run yet". The scrub found the
same one hit (the URL family) in main's file and the merged one, and none in the added lines. The signed merge
**4cef410c**, then card 2's merge on it (Item 149), under one gate (20:12:37 to 20:17:29, `gate: ok`, strict):
suite **2,486 of 2,486**, one listed flake passing on its retry (theseus-81ig's coverage count); cold start p50 23.1, p95
27.1 ms; clean shutdown with a job 33.1/50.8; swap 50.6/68.0; jobs' L1 start p50 5.95, p95 6.61 ms; turn frames 5 and 9.
Pushed 3085f71a..3c85ecee at 20:18:39; the lane's worktree removed and its branch deleted; theseus-n88g.5 closed.

**The install.** Docs only; install #3 (21:22, at 645769d2) carried the tree, and nothing in the daemon changed.

**Divergences.** The run took about 18 h 46 min of wall clock, the paused half hour included, against the launch
report's forecast of 9.5 to 12 hours (its report's "4.0 h" counts only from the resume at 14:09:56). The spend was
under half the low estimate.

**Known gaps.** The run shared the machine with the day's gates, lanes and reviewers, which the analysis names for the
time-bound tasks (neighbour IO); a run off the contended box is planned. The held-out rerun (theseus-7gir.22) is owed
before the fixes count. The working directory in Theseus's cached header differs per task, so the prefix never warms
across trials (noted, not filed).

