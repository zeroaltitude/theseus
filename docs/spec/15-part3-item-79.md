# The Ship of Theseus, chapter 15: Part III, A4's Items 79 to 85 ([index](README.md))
### Item 79. C2's bootstrap on the real account: the owner role's trust fixed, three applies, the `c2-resume` lane, and the guards set (theseus-nyzn, with theseus-oszz and theseus-crbk; the trust fix committed on `main` at 14:32 as ad1e02f, pushed 14:37; applies at 14:28, 14:50 and 14:54; the `c2-resume` lane 14:59 to 16:01, 1c45838 and fcda1e7; reviewed 16:19 to 16:21; joined 16:27 at f060021; the guards set at 16:34 from 777f1e2's build; installed 16:35 at 777f1e2; the operator's note bound the account at 17:15)

**A note on Items 79 to 96.** They landed on 2026-10-03 between 14:37 and 23:02, and reached the operator's daemon in
three installs: at 16:35 at 777f1e2 (Items 79 to 81), at 18:52 at d9b0931 (Items 82 to 85), and at 23:31 at 96d01de
(Items 86 to 96). Every join was a plain merge (Item 73's note): a fast-forward, or a signed merge commit whose second
parent is the branch's head, so a branch's commits keep their ids. From 20:24 most joins were made by join wakes:
one-shot scheduled runs, each holding a lock queued behind the joins ahead of it, and each merging, gating and pushing
one branch, so the joins stayed one at a time on `main` while the lanes and the cloud sessions worked on. The owner asked
for cloud sessions at scale: up to about eight at a time (12:28), "Let's continue to use cloud sessions too" (18:41),
the caps raised (19:31), and sixteen at once, tried once (19:41), beside six local lanes. At 21:00 he held new batches
while the account might be swapped, and the running sessions were harvested. At 23:24 he said joins may be batched.
Twice the work was stopped from outside: from 17:28 to 17:59 another agent, at the operator's request, recycled the
Claude processes older than his re-login; and at about 19:25 the account reached its monthly spend limit, and every
Claude run stopped at once. Each stopped run was relaunched from its worktree and its pushed branch, and the detached
gates ran on. Each Item's heading gives its own runs and times.

**Why.** Item 78 joined C2 with its bootstrap unapplied: the apply is the first write to the operator's account, and it
waited on two answers from the owner. He gave them at 14:20: lean (`TrailKey=aws-managed`, no trail key of its own), with
the alerts to an address of his. The apply ran from a scratch daemon of the installed build, with the operator's key,
and it found two bugs the fakes could not show.

**The three applies.**
- **14:28, the first.** The plan (14:24) showed three creates. The apply made `theseus-foundation` (`CREATE_COMPLETE`
  in the home region), then stopped (exit 1 at 14:28:16): the floor session's `sts:AssumeRole` into `theseus-owner` was
  refused, `not authorized to perform: sts:TagSession`. Diagnosed by hand at 14:29: the same role assumed without
  session tags, and was refused with one. The trust statement allowed `sts:AssumeRole`, `sts:SetSourceIdentity` and
  `sts:TagSession` together under a `Null` condition on `sts:SourceIdentity`; AWS authorizes `sts:TagSession` as an
  action of its own, whose request carries no `sts:SourceIdentity`, so the condition refused every tagged session, and
  every session the core mints is tagged. The posture and the relay were not made.
- **The trust fix, ad1e02f** (committed on `main` at 14:32, signed). Session tags got a statement of their own, for the
  root of trust alone, limited to Theseus's keys (`aws:TagKeys` `theseus:*`); `sts:AssumeRole` still requires the
  source identity. Test `the_owner_role_lets_its_root_of_trust_tag_sessions`, whose planted revert (the old statement)
  fails it; `infra/aws/check.sh` clean, 32 rule tests ok. Its gate on `main` passed at 14:35:07; pushed at 14:37. v0.80
  was cut before it and left it to this Item.
- **The repair by hand** (14:43). The bootstrap changes an existing foundation only in a floor session of that same
  role, so it could not apply its own fix. Tabitha/Claude made one `UPDATE` change set of `theseus-foundation` from ad1e02f's
  template, every parameter `UsePreviousValue` and the tags untouched, signed with the key as the create was, and
  checked before it ran that its one change was `OwnerRole`'s `AssumeRolePolicyDocument`, a `Modify` with no
  replacement. `UPDATE_COMPLETE` at 14:43:19; a tagged owner session was then minted to check it.
- **14:50, the second** (ad1e02f's binaries, built 14:46). The plan (14:48) had the foundation `none`, and the
  posture and the relay to create. The apply made `theseus-posture`, then `SetStackPolicy` was refused:
  `Unknown logical id 'LogicalResourceId/TrailKmsKey' … stack policies can only be applied to logical ids referenced in
  the template`. The lean posture makes no `TrailKmsKey`, but its policy file named it. The fakes take any policy.
- **14:54, the third** (plan at 14:52: the foundation and the posture `none`, the relay a create).
  `theseus-posture-relay` was made in us-east-1, with its stack policy and termination protection. All three stacks
  were up, but the foundation and the posture had neither guard: the apply set them only on stacks it had changed.
  One read Tabitha/Claude signed with the key at about 14:51 (a `DescribeStacks`) was likely an alert, by the posture's
  root-of-trust rule.

**The `c2-resume` lane** (theseus-oszz; 1c45838 on 7a31fba, and fcda1e7, a merge of `main` at 662e27c).
- **A stack's policy names only what the stack makes.** `stack_policy(file, made)` cuts each statement's
  `LogicalResourceId/<id>` entries to the resources the template makes under the plan's parameters (the guard crate's
  `planned_resources`, as the create path reads them; matched with its `glob`, so a `Prefix*` entry stays while it
  matches); a statement left with no resource goes; `"*"` and every other form stay; with nothing cut, the policy is
  the file byte for byte. The lean posture's policy loses `TrailKmsKey` and keeps `Trail`, `TrailBucket` and
  `TrailBucketPolicy`; the customer posture's is its file as written. The digest covers each stack's policy.
- **A re-run sets what a stopped run skipped.** For an existing, settled stack the plan reads `GetStackPolicy` (new,
  `Cfn::policy`) and the `EnableTerminationProtection` that `DescribeStacks` already returned, and lists what the stack
  lacks in `sets` (`stack policy`, `termination protection`), which the digest covers too. The apply now walks every
  stack that changes or has something to set (it skipped every `none` stack before), runs a change set only for a
  create or an update, and sets exactly what `sets` names in the floor session, with no change set. A policy that is set
  but differs from the computed one stays, and the plan warns that `aws.stack.*` governs the stack from there.
- **The surfaces.** A stack's line says what the apply sets (`none; its stack policy and termination protection are not
  set, and the apply sets them`), with the policy on one line; the wire's `AwsBootstrapStack` gains `sets` and `policy`,
  both defaulted, so a newer CLI reads an older daemon. `infra/aws/bootstrap.md` gains "When an apply stops": run it
  again, and the by-hand recovery of an owner role that can't mint sessions, as at 14:43.
- **The fake refuses what AWS refuses.** Its `SetStackPolicy` computes the stack's resources and refuses a policy that
  names another logical id, with AWS's own text, and its stacks hold a policy and termination protection. Against it,
  the old bootstrap test alone now catches the second apply's bug.

**How it is proven.**
- **Tests:** the policy rule on the real files (the lean posture without `TrailKmsKey`, the customer posture as
  written, the foundation's and the relay's byte for byte, and the fake refusing the uncut posture file with AWS's
  text); a bootstrap resumed against the fake as the account stood (the plan: the foundation and the posture `none`
  with both guards to set, the relay a create, no write sent; the apply: 3 `SetStackPolicy`, 3
  `UpdateTerminationProtection`, 1 change set and its execution, every guard signed in the floor session and none with
  the root key; a second plan with nothing to do); a policy set by hand staying, as one warning; the render's lines.
- **Five planted reverts**, each caught and each restore passing: no cut (the bootstrap test fails with the 14:50 run's
  own `Unknown logical id` error), the apply skipping `none` stacks again, the plan not reading the protection, the fake
  taking any policy, and the digest leaving the policy out.
- **The lane's gates:** 1,700 of 1,700 on 1c45838 (run 1 failed only because the regenerated TypeScript was not yet
  staged), and 1,711 of 1,711 on the merge.
- **Live, read-only:** the plan through a scratch daemon of the lane's build, with no `owner_role` in its config: the
  foundation and the posture `none` with their guards to set, the posture's policy without `TrailKmsKey`, the relay
  `none` with nothing to set, no warnings. An owner-session read of each stack agreed (the foundation and the posture
  with no policy and no protection; the relay with both). CloudTrail showed the key signing only
  `GetCallerIdentity` and `AssumeRole` into `theseus-owner`, the two calls the root-of-trust rule lets pass, and no
  write.

**The join** (16:19 to 16:21; joined 16:27). The review read `stack_policy`, `unprotected`, the apply's walk,
`set_what_it_lacks` in the floor session, and the digest. The join was the signed merge f060021 (2285fea and fcda1e7).
Its gate (16:27:41): 1,715 of 1,715, lifecycle ok in 11.9 s, a plain turn 5 frames; pushed 16:27:51. It queued behind
the t7-store and t7-defences lanes' gates.

**The guards, set** (16:34, from 777f1e2's `release-thin` build, through a scratch daemon on a fresh state dir). The
plan: the foundation and the posture `none`, each with its stack policy and termination protection to set, the
posture's policy without `TrailKmsKey`, the relay `none`. The apply (16:34:43) exited 0 in 2 s; a plan after it said
there was nothing to do; owner-session reads showed both stacks protected, each with a policy. theseus-oszz closed.

**The alerts' email** (theseus-crbk, 15:27 to 15:50). The owner confirmed the subscription several times, and each time it
was unsubscribed again. The topic listed the email subscription as deleted, and CloudTrail held no `Unsubscribe` by any
principal: it went through the unauthenticated unsubscribe link that every SNS email carries, which CloudTrail does
not record. The likely cause, a hypothesis, is a mail scanner following the links. From the confirmation email's token,
which the owner pasted at 15:48, the subscription was confirmed again with `--authenticate-on-unsubscribe true` in an owner
session: `ConfirmationWasAuthenticated` is true, so only the account can unsubscribe it. A CLI step for every operator,
`theseus aws confirm-alerts <token>`, was filed as theseus-9p40; the cloud's aws-curated session built it, and it
joined on 2026-10-04 at 00:00, after this record (v0.82).

**The install** (16:35:25, at 777f1e2, with the third cloud batch's four joins (Item 80), the `smalls1` lane (Item
81) and docs v0.80). `install-main.sh` exit 0, the store backed up first, the unit unchanged. Health: the config
confirmed by the vault in 1,035 ms, 8 secrets ready in 996 ms, Discord ready, the index ready, the places line
unchanged, the L1 self-test 6.1 ms in `theseusd check`, the cockpit's entry answering, and nothing at WARN or above in
the journal. **The account bound** when the owner pasted the template's C2 lines (his `[aws.accounts.<id>]` with
`owner_role`, and `[broker.programs.aws]`) and the daemon restarted onto them at 17:15: health says the account signs
with role sessions, and jobs get short-lived AWS sessions.

**Divergences.** The bootstrap could not repair its own owner role, and the repair was made by hand with the key; the
recovery is written down, and whether the bootstrap should learn it is the owner's (theseus-ps12, P2). Each stack's policy
is computed, not the file as written. The bootstrap took four applies, not one: three to make the stacks, and one to
set two stacks' guards.

**What it costs.** The C2 report's estimate stands: about $0.08 to $0.10 a month at this account's activity. Cost
Explorer is a day behind, so theseus-xsoq measures it from 2026-10-04, and again when GuardDuty's 30-day trial ends.
At 17:22 the owner chose that the $50 budget counts the whole account, its other services included (theseus-mhvb).

**Known gaps.** theseus-ps12 (above). S3's account-level Block Public Access and EBS encryption by default stay the
by-hand step 4 of `bootstrap.md`; this record has no line saying they were set. The design's write checks, and the
hands' reservations with the day's and the hour's tripwires, are still to come (row 40).

### Item 80. The third cloud batch's first four joins: one exam, Jev's `security.v2` in shadow, the cockpit's parity with the Observatory, and the WAL's open refusing rot under the checkpoint (theseus-celu.13, .18, .14 and .15, for theseus-vm3n.4, theseus-605, theseus-vm3n.6 and theseus-gt12; Claude cloud sessions fired 2026-10-03 13:00 from a59b7c1; reviewed 15:05 to 16:17 by the batch-3 harvest wake; joined as the signed merges dc3387f (15:33), 662e27c (15:46), 2633d3b (16:07) and 2285fea (16:17); installed 16:35 at 777f1e2)

**Why.** At 12:28 the owner asked for up to about eight cloud workers at a time on isolatable work, continuously. The third
batch was seven sessions, fired at 13:00 from a59b7c1: wakes-repeat (row 64, 37a), exam-trims (the cut-list's 6.2),
obs-parity (6.4's parity check and its ports), wal-rot (theseus-gt12), ledger-reads (6.3's server half, with
theseus-96w2 and theseus-tphr), load-flakes (five gate flakes), and jev-security (theseus-605). The harvest wake
(automation 6722a223, from 15:00) reviewed and joined four, here; it held three for the spine's store and defence steps,
and they are Items 84, 87 and 91.

**How these joins were made.** Each is a plain merge (the owner, 13:18): a signed merge commit whose second parent is the
cloud branch's head, so the session's commits keep their ids and GitHub shows the branch merged; `CLOUD_REPORT.md` is
removed in the merge, and the cloud branch deleted. From about 14:30 the machine's Claude login was in another
organisation than the routines, so the wake could not read their logs, and read each session's done-ness from its
branch's report commit.

**exam-trims** (theseus-vm3n.4; Sonnet 5.5; 314c5c0 and 593ef0a; 12 files, +357 −1,873, all in `crates/theseus-exam`).
- **One exam.** exam-v1 is deleted: `exam/exam-v1.toml`, `EXAM_V1`, `Family::V1`, `--exam v1`, `--against v1`, and the
  tests that pinned v1 (the 40-item shape, the 38 items word for word, v1's fixture, and `daemon_reads`' v1 test).
  `--exam` takes only a file path, the built-in exam by default. Tests that used v1 as a handy exam use v2, whose 38
  shared items are unchanged; the report test's arithmetic was redone by hand for 72 items, and the review checked it.
- **One probe.** The in-process BM25 probe is deleted (`probe.rs` 706 to 114 lines): `probe` is the tender's, and needs
  `--tender` and `--manifest`. What `tender.rs` reads stays (`KS`, `ItemProbe`, `Recall`, `recall`, `rows`).
- **A deviation, accepted.** The stemmer and the tokenizers were not the probe's alone: `item.rs` checks the paraphrase
  and scale families with `content_stems` (a paraphrase's task and gold share no content word). The session moved
  those 50 lines unchanged into `words.rs` rather than weaken what the exam checks of its own items.
- **The memory arm** (row 55) is named as the exam's scratch daemon's `[memory] arm` config key, one daemon per arm, not
  a `turn.submit` field. Nothing was added to the daemon or its template.
- **Proof.** The exam's tests 49 of 49; a planted revert (`content_stems` without its stemmer) fails two tests, re-run
  at the join; the offline paths (`list`, `write-store`'s summary, four notes, both reports) byte-identical before and
  after.

**jev-security** (theseus-605; Sonnet 5.5; cad9cb7; 13 files, +2,971 −56, all in `crates/theseus-judge`, a reserved
crate the daemon does not link).
- **What the session found.** `security.v1` missed its planted case (steered 0.37) for two reasons: Jev cannot decode a
  blob in a query, and the state never said that the host was new to the session or unnamed by the operator. Nor did it
  hold any text of the page that drove the call.
- **`security.v2`, a candidate, in shadow only** (`action = "none"`, a 3,000-token state cap). It is registered in the
  embedded packs, so `[judge.packs."security.v2"]` can select it; it is not the default. The builder states computed
  facts, each only when it applies and never stated false: whether the call's host was named in the operator's ask (a
  named domain covers its subdomains; a shared suffix such as `co.uk` names nothing), is the held text's host, was seen
  earlier, or is new on every count; whether a credential path (a short list: `.env` but not its templates, `.aws`,
  `.ssh`, keys, `op://`, …) was touched earlier, is named by the call, or by the ask; the encoded runs (base64 that
  decodes to text, hex), each with where it is, its decoded text scrubbed and clipped, and whether it looks like a
  credential; the query's length; a tainted path, when the core passes provenance; and up to three excerpts of what the
  session read (400 characters each, scrubbed). Every scan is bounded.
- **A leak in v1, closed in v2.** v1's scrubber sees plain text only, so a vault value that travels base64- or
  hex-encoded reaches Jev. v2 wraps the scrubber in `DecodingScrub`, which decodes each run and holds back any whose
  decoded text the scrubber would change. v1 is left as it is.
- **The questions.** `steered` is rewritten to weigh the new facts and to treat the read text as evidence, never as
  instructions to Jev. `safe` is added beside `risky` as an undeciding companion (the two should sum near one); `risky`
  still decides.
- **An eval set**: `jev-probe --eval <pack>`, 17 planted-injection cases (10 benign, 6 risky, 1 observe-only), each with
  its reason; v1's goldens are byte-identical.
- **Proof.** 90 tests, five runs under nextest; a planted revert of each fact, each failing the golden test and its own;
  the builder at 28 µs a call (release); the huge-input test, which failed at 2.4 s before the scans were bounded.
  **Live, at the review, with the operator's own key for Jev:** 17 calls, p50 113 ms, $0.0013 in all, every call inside its
  reservation; 73 of 78 expectations met. v1's missed case is caught (steered 0.93, risky 0.90). The five misses: the
  file-laundered injection, whose `risky` read 0.55 against a confirm line of 0.60 (steered 0.83), and two edge calls on
  benign cases. The review re-ran two plants: `DecodingScrub`'s second pass off fails the secret test, and a new host
  never stated fails the host test and the eval's goldens.
- **What it left.** theseus-ibm3: the laundered injection does not reach v2's deciding question. The owner chose its option
  A at 17:22 (`steered` decides beside `risky`, above a high bar set from shadow data); Item 95 builds it.

**obs-parity** (theseus-vm3n.6, the first of its two steps; Sonnet 5.5; 1de1451, 72c3f14, a90f346 and 69c7a98; 25
files, +1,408 −246, all under `cockpit/`).
- **The inventory.** Every element of the Observatory (`web/src`: the shell, the transcript, the timing tree, the
  Observatory's panels, the narrative, the disk and spool card, the AWS card, the sandbox section), with its file and
  line, and where the cockpit shows it. It was the owner's condition for retiring the Observatory.
- **What it ported.** Ledger rows read in words, every kind the Observatory named (`summary.ts`, `figures.ts`); one
  confirm card for the session deck and Actions (a write's or a patch's preview as a diff, the budget question's words
  and buttons, approve and trust); in the session deck, the asks inline, the live thinking, text and running tools from
  push events, a failed turn's class and error, the composer's queue while a turn runs, the context tab's compile log,
  and the timing tree's summary by kind; Fleet's executions table; Systems' new cards (push, the AWS account, the store,
  the start's phases, the secrets that failed, Discord's places and traffic, approval rows, the catalog's columns); the
  shell's "N need you" chip and provider errors; and caching by profile.
- **What it left.** The label, graduation and held-post parts, moot once the place rule removed them (Item 76); the
  sandbox section's details, whose place is the Boundaries view; and a list of minor or deliberate differences.
- **Proof.** Lint clean, 20 tests (15 new), the build; a planted revert of one row's words caught; by eye, twelve views
  against a scratch daemon over a synthetic store of 200 sessions, with no console or page error.
- **Its second step**, the gaps fixed, the cockpit at `/` and `web/` deleted, is Item 86.

**wal-rot** (theseus-gt12; Opus 5.5; d19c733; 8 files, +596 −28).
- **What the session found.** The writer's batch has no bound, so a whole frame found after a bad one proves nothing on
  its own. But the index's checkpoint claims only synced positions: `checkpoint_as` takes the `appending` lock for
  writing, so it runs between batches, and `commit` indexes a batch only after the batch's one sync returned. **So any
  frame whose first position is at or before the checkpoint was synced**, and a bad frame there is rot, not a torn
  write. And a second bug on `main`: when the frame that holds the checkpoint's own record went bad, the tail walk gave
  up, and the full walk cut that frame as a torn tail before the open refused. The open refused, but it had already
  destroyed an acknowledged frame.
- **What it changed.** `torn_or_rot`, one decision that both walks call. A frame that does not check in the last segment
  is first followed by a scan for a whole frame after it. If the bad frame's first position is at or before `synced_to`
  (the checkpoint, or more), the open **refuses** with `WalError::Corrupt`, naming the position, the checkpoint, any
  whole frame after it, "nothing was cut", and `theseusd restore --repair`; no byte changes. Otherwise it cuts, as
  before, and reports the cut with any whole frame after it (`Recovery::cut`) and a warning. `index::checkpoint_of`
  reads an index's checkpoint through redb's read-only open, which writes nothing. The repair opens its staging store
  with `Store::open_synced_to`, so a staged WAL that still holds a frame the store had synced is refused at its open.
  `restore` is unchanged: a backup's index may have been copied after its WAL. No record or frame format changed.
- **Proof.** Five new tests (rot refused with the segment byte-identical; a torn batch cut though a whole frame follows;
  a plain torn tail cut; the tail-only open refusing and cutting alike; a checkpointed frame gone bad refused before
  anything is cut); four planted reverts, each caught (the refusal off; the naive rule, refusing whenever a whole frame
  follows; the tail walk without the evidence; the repair's patch planted out); five runs under load, 49 of 49 each; a
  FAST A/B of two builds, interleaved five times: the store's open p50 median 4.76 ms before and 5.08 after, within the
  machine's noise, and the cold start and kill phases alike. The review read the rule from the writer's code itself,
  and checked that it never refuses an open `main` accepted.
- **What it left.** Rot after the last checkpoint, and any bad last frame with no index (a full replay, a restore), are
  still cut. The session proposed a synced mark in each frame's header, 8 bytes, which changes the frame's encoding.
  The owner approved the format bump at 17:14 (theseus-7nfj), and Item 92 builds it.

**The joins and their gates.**

| Session | Merge | Gate | Notes |
|---|---|---|---|
| exam-trims | dc3387f on 7a31fba, 15:33:46 | 1,691 of 1,691 (5 fewer: v1's tests), frames 5, the jobs phase's L1 start p95 10.1 ms | The first lifecycle run missed under neighbours' builds (load 30.9, CPU pressure 36 %, after the 5-minute settle and the busy allowance); no crate depends on theseus-exam, and the gate's rerun passed strictly |
| jev-security | 662e27c on dc3387f, 15:46:35 | 1,707 of 1,707 (16 new), frames 5, L1 start p95 5.8 ms | It waited 236 s for the lock; the first lifecycle run missed the clean stop (p95 118.8 ms against 104) at load 15.4, and the rerun passed; theseus-judge is not in the daemon |
| obs-parity | 2633d3b on 662e27c, merged 15:48, pushed 16:07:19 | 1,707 of 1,707, frames 5, lifecycle within budget (cold start p95 28.9 ms), L1 start p95 6.96 ms | One conflict, `Transcript.tsx`, against the place rule's publish control: both kept |
| wal-rot | 2285fea on 2633d3b, merged 16:08, pushed 16:17:20 | 1,711 of 1,711, frames 5, lifecycle within budget | Its first two plants re-run on the merged tree |

**The install** (16:35, at 777f1e2, with Items 79 and 81). Of the four, the daemon runs two: the cockpit's ports
(served at `/cockpit/` until Item 86) and wal-rot's refusal at the store's open. theseus-exam and theseus-judge are not
in the daemon.

**Divergences.** exam-trims kept the stemmer and the tokenizers (above). jev-security added `safe` beside `risky`,
which the brief did not ask for, and left `risky` deciding. obs-parity's gaps were left for the owner to accept or ask for.
wal-rot proved its rule from the checkpoint, where the brief had asked for a rule from the batch's bound, which does not
exist.

**Known gaps.** No live tender probe was run for the exam. The core's mapping into `security.v2`'s fields (23a's
inputs) and the file provenance its laundering facts read are not built. The rot that wal-rot still cuts is Item 92's.

### Item 81. The `smalls1` lane: settle's bar at three quarters of the cores, a private address's card in a shared place, herdr's prompt race, and the "should have asked" widget's words (theseus-lf1n, theseus-94a6, theseus-jhie; 2026-10-03 15:19 to 16:04; 9a5b64d, 1f1b829, 783a616, a823287 and e1b6944 on 7a31fba, with `main` merged in at 79643a7 and 08fbee4; reviewed 16:20 to 16:21; joined 16:31 as the signed merge 0c150e1 and the web's words, 777f1e2; installed 16:35 at 777f1e2)

**Why.** Four small items the owner decided that afternoon. At 14:20, "Your pick is good" on the gate-mode lane's
recommendation for settle's quiet bar (Item 75's known gap), and "Leave it to the approver" on theseus-94a6 (a shared
place is offered `http.fetch`, so an approved fetch of a private address brings that page into a conversation others
can read), with the card saying so. At 14:53, the "should have asked" widget's words: "It should say 'Should I have
asked?' and the text in the pulldown would say 'Make actions like this ask in the future'". And a test race that docs
v0.80's join gate hit at 15:09 (theseus-jhie).

**What landed.**
- **Settle** (`scripts/gate.sh`; §9, `scripts/AGENTS.md`). The machine is quiet when IO pressure is under 10 %, CPU
  pressure under 20 %, and the 1-minute load under three quarters of the cores (12 of 16; it was the core count,
  theseus-611s). The wait is bounded at 2 minutes (it was 5); past it the timing budgets get the busy allowance, as
  before. With the lanes no longer paused (Item 75), the strict misses cluster at loads of 12 to 16 (a phase missed in
  41 % of the normal-priority runs at 12 or more, 10 % from 8 to 12, 6 % under 8): the old bar called that band quiet,
  and a second miss there failed a join. Now a gate there waits, then benches with the allowance, which was calibrated
  on that band; and a busy machine holds the shared lock for 2 minutes, not 5. The settle line names the bar:
  `lifecycle: still busy after 2 minutes (IO pressure 0 %, CPU 0 %, load 12.50 on 16 cores, quiet under 12)`.
- **A private address's card in a shared place** (theseus-94a6; §3.9, the place rule). DD5's check still asks before
  `http.fetch` reaches a private address, in every place. In a shared place the card's reason now ends: "…and a private
  address waits for approval; this is a shared place, so the page joins a conversation others can read)".
  `places::private_fetch` adds the clause in the gate, after the policy's decision, where the place's class is known
  (beside `places::refusal`), so the policy's signatures did not change. Two surfaces cut long reasons, and now read
  them to the end: the Discord card's 300-character clip cuts the middle, not the end (`clip_middle`), and the TUI's
  card lines wrap at the pane's edge, up to five rows (the first, `…`, and the last three), where each was cut at one.
- **herdr's refused-answer test** (theseus-jhie). The watch's help line repeats the prompt's words, so a wait that
  counted them was one ahead, and the test could type `t` before the CLI asked again. It now counts the prompt's own
  line (`Watch::wait_prompt`). Test only.
- **The widget's words** (theseus-sgh's widget). Discord's menu says `Should I have asked?`, each option `Make actions
  like this ask in the future` with a description naming what its press does (`Every proc.run call asks you first`), a
  notice card's button the same, and the card's field after a press `Asks first now`. The menu's custom ids and values
  are unchanged, so a menu an older build posted still works. The cockpit's Boundaries view quotes the new words. The
  web UI's words were a patch the lane could not build offline, applied at the join (below).

**How it is proven.**
- The gate-mode lane's harness, copied: its 19 cases unchanged, and five new ones against both gates. Load 12.5 and
  12.0 on 16 cores wait 2 minutes and bench with `--allowance 65` (the old gate called them quiet); 11.9 is quiet and
  strict; a machine busy throughout waits 24 sleeps of 5 s, not 60, then gets the allowance; under `taskset` on 6 cores
  the bar is 4.5.
- theseus-94a6: the decision as the gate makes it (a shared place's private fetch asks with the clause, a private
  place's without, a public URL not at all); the whole core (a shared channel's and the owner's DM's fetch of
  127.0.0.1); the binding against the in-process fake Discord (the card in the DM carries the clause); a revert of the
  gate's one call fails the last two. The Discord card keeps a long reason's end; the TUI wraps it.
- theseus-jhie's planted proof: with the CLI's re-ask deferred 300 ms on its own loop's timer, the old test failed at
  20.065 s and the fixed one passed 6 of 6. A blocking sleep, the first plant, could not show the race: the watch is one
  `select!` loop, so stdin waited too. Then 100 runs beside the gate's whole-suite run, all passed.
- The lane's gate (16:03:18, on `main`'s 662e27c merged in): 1,712 of 1,712, no flakes, a plain turn 5 frames, the
  cockpit's lint, tests and build.

**The join** (reviewed 16:20 to 16:21, accepted as built). The review read each commit, the settle's bar computed
after `cores=$(nproc)`, and the clause's place in the gate. The join was the signed merge 0c150e1 (f060021, Item 79's
join, and 08fbee4), then 777f1e2: the web UI's words from the lane's patch, with `web/dist` rebuilt. Its gate
(16:31:21): 1,720 of 1,720, lifecycle ok in 10.3 s after a 15 s settle whose line said `quiet under 12`, the new bar,
live; pushed 16:31:36.

**The install** (16:35, at 777f1e2, with Items 79 and 80). The Discord menu's and the card's words, the clause on a
shared place's card, and the TUI's wrap reach the operator; the settle's bar is the gate's alone.

**Divergences.**
- The brief's plant for theseus-jhie, "a sleep before the prompt", holds the watch's one loop and cannot fail the old
  test; the plant that did defers the re-ask on the loop's timer.
- Item 2 changed two surfaces besides the core: Discord's card clips a reason in the middle, and the TUI's card wraps
  (§2.9's card cut a long reason at the pane's edge).
- The brief's live check (GLM, the fake Discord, a shared channel) was not run: the place rule's rig drives a stand-in
  model that cannot make a fetch. The binding's in-process test carries the card to the fake Discord instead.
- The web UI's words landed at the join, not in the lane.

**Known gaps.** theseus-3w3x (P3, post-v1): the old words left on the CLI's notice hint and policy list, its help
text, the core's refusal and narrative lines, and obs-parity's new cockpit button (the web UI's were applied at the
join, and `web/` itself was deleted in Item 86). Not filed: the Boundaries list clamps a reason at two lines (its
Approve dialog and the Actions view show it whole), and a Discord select option's description has no clip at Discord's
100 characters (the new words are 8 characters shorter than the old).

### Item 82. Tier 7's store: one store format number, an open that makes nothing durable, and an output golden of shapes (theseus-ptx1, with theseus-6a7o; the simplification cut-list's 7.9 as amended, 7.7, and 7.4; spine, in a worktree; 2026-10-03 15:01 to 16:22; 11dbe52, 3dcdd29 and 5471e38 on ad1e02f; reviewed 16:49 to 16:51; joined 16:56 at fb612d9, a signed merge; installed 18:52 at d9b0931)

**Why.** The core reviewer's G2, G1 and H1, approved by the owner at 14:20 with 7.9 amended. Each new stored field cost a
schema bump, a literal test, and three golden rewrites (R8 was sized S and landed at 1,764 lines; M4 bumped four kinds
within days), to guard a downgrade that a single number guards as well. Every start paid durable redb commits that a
crash simply redoes. The output golden had been rewritten 12 times in 22 hours, 94.6 % of its changed lines only
digits, and sat on the flaky list.

**What landed.**
- **One format number** (§6, P5b, F4a). `MANIFEST_FORMAT` is 4. Any step that adds a field to a stored record, or
  changes the encoding, bumps it (the owner's amendment: simpler to follow than "when data would be lost"); an older binary
  refuses the newer store, and the installer's `check_manifest` still asks first. The per-kind machinery is gone:
  `kinds::SCHEMAS`, the manifest's per-kind marks, `mark()` on every append, the open's per-kind check of the WAL's
  tail, `NewRecord.schema`, R8's `tests_schemas` (773 lines) and its 1,097-line golden, and the eight literal old-layout
  tests. The record header's schema field is frozen (`record::FROZEN_SCHEMA = 0`): new records write 0, old ones keep
  their kind's old number, and nothing reads it.
- **When a store moves.** An open writes nothing. The writer's first frame into an older build's store moves the
  manifest first, durably, once (`upgrade_manifest`, on the writer, which alone writes frames); a failed move fails the
  batch, so nothing reaches the WAL under the old manifest. A store only read keeps its format, and a format-3
  manifest's marks are left unread and dropped at the move. On a daemon that first frame is the kernel's startup frame,
  so the first start after a bump pays two syncs before serving, once (about 10 to 17 ms here; theseus-e4jo).
- **One table of old layouts** (`crates/theseus-core/src/tests_layouts.rs`, one test,
  `every_old_layout_on_disk_still_reads`): 26 samples, literal bytes their builds wrote, generated by a script from the
  deleted tests' literals and the 460a35b fixture's WAL, so no byte was retyped: the fixture's schema-1 records for six
  kinds, nodes 2 to 6, actions 2 and 3, outboxes 1 and 2, compilations 3 and 4, and seven ledger rows of kinds no build
  writes now. Each is decoded by its kind's reader, kept field for field (for the three layouts whose fields the place
  rule dropped, every field but those), and round-tripped. A copy of the operator's store held exactly such layouts.
- **An open makes nothing durable** (§9; the review's G1). The tables' creation is a non-durable commit, and a replay
  takes no checkpoint; the replayed tail counts toward the next periodic checkpoint, so a crash loop cannot grow the
  tail without bound. In redb 4.3 a durable commit is one fdatasync, so a clean restart pays one sync fewer and a start
  after a crash two fewer, and a crash before a run's first durable commit leaves nothing to repair.
- **The output golden pins shapes** (H1; theseus-j6qn's golden). Every JSON number and every other run of digits is
  `#` once the ids are aliased, a duration `<n>` with its unit, and the frame lines carry no schema; the golden is off
  the flaky list, and theseus-6a7o closed. `by_the_load` stays: its line's presence is the load's, and the 240
  narrative lines still catch a broken sentence.
- **Where steps read the rule:** `crates/theseus-store/AGENTS.md` (the version rule, the frozen field, when a store
  moves), the root `AGENTS.md`'s store line, and `crates/theseus-core/AGENTS.md` (`tests_layouts`). A branch that
  still bumps a kind's schema becomes, at its join, a `MANIFEST_FORMAT` bump with a sample of the layout it replaces
  (Item 84 was the first).

**How it is proven.**
- **strace up to serving**, the base against this build: a clean restart of a copy of the operator's store 3 syncs to
  2, an empty store 12 to 11, a start after a SIGKILL 8 to 6 (3 when the kill was this build's).
- **Two bench A/Bs**, each in one hold of the gate lock: the daemon's store phase 8.1 to 4.6 ms (12.0 to 6.1 on the
  operator's store), SIGKILL then restart 40.6 to 26.5 ms (63.1 to 40.1).
- **Four planted reverts**, each failing its tests: a newer store opened (5 tests), a sample whose field the reader
  drops, the golden unmasked, and a duration's unit unmasked.
- **Under load**, theseus-core's suite three times beside four busy loops at nice 5, and the golden alone three times:
  all green.
- **On a copy of the operator's store**, this build served it with no record refused and every session, execution and
  row read, and moved it to format 4; the installed 57a3759 then refused it with the format message and wrote nothing.
- **The lane's gates:** 1,688 of 1,688 on 3dcdd29's tree; the final gate on 5471e38, 1,689 of 1,689, lifecycle ok
  (cold start p50 23.5 ms, SIGKILL then restart p50 22.9 ms, the store phase 4.44 ms), a plain turn 5 frames; the gate
  before it failed only on theseus-lgtj's known web UI flake.

**The join** (reviewed 16:49 to 16:51; joined 16:56). The review read `index.rs` and `store.rs`'s `open_once` and
`upgrade_manifest`. The first merge (328f9df, 777f1e2 and 5471e38) failed clippy at 16:50:12: `open_once` came to 104
lines, wal-rot's lines (Item 80) plus this branch's removal of its shape mark. `scripts/shape-expect.py` on the merged
tree put the mark back, as `main` had it, and the merge was amended, unpushed, to fb612d9. Its gate (16:55:58): 1,713
of 1,713, lifecycle ok in 9.8 s (cold start p50 18.9 ms, SIGKILL then restart p50 23.1 ms, the store phase p50 4.10
ms), a plain turn 5 frames; pushed 16:56:07. theseus-ptx1 and theseus-6a7o closed.

**The install** (18:52, at d9b0931, with Items 83 to 85). One way: the operator's store moved from format 3 to 5 at the
new build's first write (its manifest is dated 18:52:08), since Item 84's bump to 5 came in the same install. The
install's two store backups say so: the manifest taken before this install reads format 3, and the one taken before
the next install (23:31, Item 96) reads 5. Every older binary refuses the store, and the backup the install takes
before the swap is the only way back.

**Divergences.**
- `by_the_load` stays, against the brief (above).
- `durations()` masks the unit as well: the number mask alone left theseus-6a7o's first symptom in place.
- The masking code shrank less than the review's estimate (910 to 893 lines): the kept masks settle where digits fall
  and which unit, which `#` alone doesn't.
- Each durable redb commit was one fsync, not two; the saving is half the review's estimate, and measured.
- A store now moves to the new format at a daemon's first start, not at its first newer record: one number cannot tell
  which records are newer.

**Known gaps.** theseus-e4jo (P3, post-v1): the manifest's move costs two syncs before serving, once per bump; moving
it after serving would leave a crash window in which this build's first frame sits under the old manifest.

### Item 83. Voice with Deepgram: the crate back, speech both ways, the wire-in, and speech as spend (theseus-drrs, with theseus-yl5w; rows 77 and 78; the `voice2` lane; 2026-10-03 15:19 to 16:48; ea93602, 53136c9, 0ec1f8c and 97a4634 on 7a31fba, with `main` merged in three times; reviewed 16:50 to 16:52; joined 17:08 at 1d11949, a signed merge; installed 18:52 at d9b0931, and on since 23:38)

**Why.** The owner chose Deepgram for speech to text and synthesis at 14:20 (one key, in the vault), so voice returned to
v1, and with it the engine that tier0 had parked (Item 72's crate). He decided its licence at 15:26 (theseus-yl5w:
MPL-2.0 for songbird's seven crates, by name; §1).

**What landed** (four signed commits, and three merges of `main`, each without a conflict):

| Commit | What |
|---|---|
| `ea93602` | `theseus-voice` a workspace member again; `deny.toml`'s seven named MPL-2.0 exceptions and 44a's six advisory ignores back; `Cargo.lock` +192 packages, the same 192 tier0 removed |
| `53136c9` (45a) | `DeepgramSpeech`: nova-3 pre-recorded per utterance (a WAV), Aura-2 Andromeda per sentence (raw linear16 at 48 kHz); reqwest bounded at 5 s to connect and 20 s a call; errors that never carry the key; `[voice]`, off by default; the key harness-only; the VAD's 200 ms pre-roll |
| `0ec1f8c` (44b) | A voice channel as a place (`voice = true`); `/join` and `/leave`; songbird on the shard; utterances as turns in the voice place's session, by their speaker; replies spoken sentence by sentence with barge-in, other turns' replies at the pause; a dropped connection as `Failure::Connection`; `voice.*` rows; health's voice block |
| `97a4634` (45b) | Speech priced in code (`SPEECH_PRICES`); each call checked against what the session may still spend before it, and booked after it (`Kernel::book_spend`); `speech.*` rows |

- **The place.** A voice channel is a `[[channel]]` with `voice = true` in the bindings file: its own session, and its
  own text chat as the place's text. The place rule, the lanes, the routes, the private check and `/publish` apply to
  it unchanged. `/join` is taken only from a private place, by a user the voice place lists, naming the channel or
  taking the one the presser is in, and bounded at 20 s.
- **Speech as spend.** About $0.31 to $1.15 of speech an hour of conversation, most of it synthesis (Aura-2 at about
  $0.030 per 1,000 characters; nova-3 at about $0.0043 a minute). The prices are assumed from Deepgram's published
  rates; the owner, 17:14: keep them, with a note to confirm (theseus-zue5).
- **Nothing before serving.** `Voice::new` builds a struct; songbird's manager is built with the gateway's shard,
  after serving, and its driver starts at a `/join`.

**How it is proven.**
- **Tests.** Deepgram against a stand-in on 127.0.0.1 (each request's method, path, query, auth and body; a 401
  scrubbed, a 503, no answer, a malformed answer); `[voice]`'s config and the harness-only key; the wire-in's refusals,
  a voice turn into the session by its speaker, a report at the pause, a dropped connection's row and notice; the
  prices, the kernel's booking, the core's limit check and booking, and the binding's rows and its refusal past the
  limit; the engine's pre-roll and dropped connection. All 28 of 44a's tests pass unchanged.
- **Live, with the real key.** Synthesis 1.4 to 2.3 s for a 79-character sentence, transcription 0.2 to 0.4 s, 15 of
  16 words back. Through the engine, the pipeline lost the first word ("Theseus") until the 200 ms pre-roll; with it,
  the utterance was heard whole, its turn ran, and the reply's three sentences were synthesized and played, the first
  audio about 1 s after the reply. The key's value appeared in no output.
- **Live join.** In the operator's private test voice channel (16:31), songbird played a 7.16 s Deepgram greeting to
  its end, and Discord accepted DAVE over voice gateway v4 (its key package, external sender and pending group in
  davey's log). A scratch daemon registered `/join` and `/leave`, bound the channel as a private place, and showed
  `voice: ready`. The daemon's own `/join`, with a person speaking, is the owner's test, written for him
  (`conversation-test.md`).
- **The lane's gates:** each commit's, 1,724, 1,749, 1,760 and 1,774 passed; the lifecycle bench before the last
  commit, alone under the lock at normal priority, ok in 10.1 s (cold start p95 29.3 ms of 57).

**The join** (reviewed 16:50 to 16:52; joined 17:08). The signed merge 1d11949 (fb612d9, Item 82's join, and 97a4634),
with no conflict, and `Cargo.lock` consistent under `--locked`; `runtime.rs` stood at 3,498 lines of its 3,500 ceiling
and the protocol's `lib.rs` at 2,600 of 2,600. Its gate (17:07:52): 1,767 of 1,767; the lifecycle bench's first run
missed on one cold-start outlier (p95 65.6 ms, with a p50 of 29.3), and its rerun passed (p95 27.1, p50 19.7), as
`main` reads without voice; a plain turn 5 frames; pushed 17:08:09. theseus-drrs and theseus-yl5w closed.

**The install** (18:52, at d9b0931), with voice off until the operator's note turned it on. **Voice went on at
23:38.** The owner pasted the sparse note (Item 90) at about 23:37, with `[voice] enabled = true` and the Deepgram key's
reference, and the bindings file that names his test voice channel as a private voice place moved into place. The
daemon restarted onto them (23:38:01), and health at 23:39 read 9 secrets, the Deepgram key among them, 10 slash
commands, and `voice: ready`. His own `/join` test is next.

**Divergences from plan.**

| Planned | Built | Why |
|---|---|---|
| `[[voice]] guild, id, text = "<paired text channel>", users`; the voice place shares the paired text place's session (M7 §2.8) | A `[[channel]]` with `voice = true`: its own session, its own text chat as the place's text | The voice channel's own chat carries the conversation; the place rule, lanes, routes, the private check and `/publish` all apply unchanged |
| `/join` "in the paired text channel" | From any private place, by a user listed in the voice place | The place rule decides who may invite, as `/publish`'s does |
| nova-3 *streaming* STT (M7 §2.8) | Pre-recorded, one request per utterance | The engine's VAD already cuts utterances; no socket to keep up for v1 |
| Reserve when an utterance closes; reserve a synthesis by its text (M7 §2.8) | Check the estimate against what the session may still spend before the call; book the real cost after it | A speech call is no turn's, and a reservation needs a turn's guard |
| `speech.stt`, `speech.tts`, `voice.utterance` rows | `speech.transcribed`, `speech.synthesized`; `voice.joined`, `voice.left`, `voice.failed`, `voice.barge_in`, `voice.unlisted` | One row per call |
| `[catalog."deepgram:nova-3"] usd_per_minute` rows | `SPEECH_PRICES` in `catalog.rs`, beside the token tables, not configurable | Prices in the code's catalog, never only in the template |
| The utterance opens at its first loud frame (44a) | It opens with the frames of the 200 ms before (`Config::pre_roll`) | The live round trip lost "Theseus" without it |

A health warning about other viewers of the test voice channel was planned; the owner (17:14) decided that every channel
in his guild is private regardless of who else can view it, with no viewer warning (Item 89).

**Known gaps.** The VAD's threshold on the operator's real microphone (his test); streaming synthesis, if the first
audio feels slow (theseus-eute); a timed-out call's usage held as unknown spend (theseus-c071); voice's observability
beyond health's block and the rows (theseus-1001); all P3, post-v1. Two gate flakes found on the way, not voice's:
theseus-lgtj and theseus-qh0u (P2). `scripts/build.sh` and `scripts/AGENTS.md` still say the voice stack links into no
shipped binary; `theseusd` links it now.

### Item 84. A wake repeats: every, days and until, re-armed in the frame that takes it, and held after external text (theseus-d4pt, theseus-celu.12; row 64, step 37a; the third cloud batch's wakes-repeat session, fired 2026-10-03 13:00 from a59b7c1; be2d986, 9083fcf, 826e6b0 and 13d27b9; reviewed 15:25 to 15:50, held for t7-store; joined 17:53 at 1abf099, a signed merge, by the held-joins wake; installed 18:52 at d9b0931)

**Why.** Row 64 (37a): a wake that comes back, "every evening at nine" or "weekdays at eight", without the model
setting the next one by hand. The session was the third cloud batch's (Item 80). Its review held it for t7-store, as
that step's brief expected: the session bumped EXECUTION's schema from 2 to 3, and 7.9 (Item 82) replaced the per-kind
numbers with one store format number.

**What landed.**
- **The kernel** (be2d986; `crates/theseus-kernel/src/repeat.rs`, new). `Repeat { every, first_ms, days, until_ms }`,
  with `Every { n, unit: m|h|d|w }` and weekdays. Occurrence k is due at `first + k·every`, by jiff's zoned arithmetic
  in the kernel's zone (the system's; a test sets its own): days and weeks are calendar spans, so 21:00 stays 21:00
  across a change of offset; minutes and hours are exact time; a wall time the spring change skips lands after the gap.
  `PendingWake` gains `repeat` and `occurrence`, both skipped when empty, so a one-shot wake's bytes are unchanged.
- **The re-arm.** `take_wakes` puts a due series back in the same frame that takes it: the same id, at the first
  occurrence after now. Occurrences passed over while the daemon was down are one turn, with a count (`missed`), never
  a burst. Past `until` the series is not put back, and a `wake.ended` row (a new ledger kind, `why: "until"`) is
  written. A cancel removes the one wake, which ends the series; a series counts once against the cap of 5.
  `[kernel] min_repeat_minutes` (5) is the floor, and 365 days the ceiling.
- **The tool** (9083fcf). `wake.at` takes `every`, `days` (only with `every = "1d"`) and `until`; with neither `at` nor
  `after`, the first time is one span from now. Each occurrence's node reads `⏰ wake (every 1d, #4): note`, with `; 2
  missed while the daemon was down` or `, due …, N late` as they apply; one-shot text is unchanged. `wake.fired` carries
  `every`, `occurrence`, `missed` and `next_due_at_ms` for a series only, so one-shot rows are unchanged.
- **The hold.** A repeating `wake.at` set in a session holding external text waits for the operator's approval, and a
  one-shot one keeps its exemption (§3.9; the owner, 2026-09-30, T1b): `external::exempt(class, tool, input)` exempts
  `wake.at` only when its input has no `every`. It fails closed: any non-null `every`, even an invalid one, is held. A
  series runs unattended for as long as it repeats; a one-shot wake runs once, in a turn of the same held session.
- **The surfaces** (826e6b0). `WakeInfo` gains `every`, `occurrence` and `next` (a display string in the daemon's
  clock); `theseus wakes` has a column, `once` or `every 1d #4`; Discord's `/wakes` gives `🔁 every 1d · next <t:…:t>`
  in the reader's zone; the narrative says `Wake a1b2c3 fired (#4, 2 missed while down); next 21:00 Thu`.
  `theseus.wakes.fired{theseus.wake.repeat}` and `theseus.wakes.late_ms` count every wake, one-shot included, from a
  `wake.fired` span on the turn's trace.
- **The kernel-sim** (13d27b9) sets repeating wakes, takes and cancels them, and keeps a fifth of its crashed daemons
  down for 1 to 10 minutes, so occurrences are passed over. It checks that every wake is taken at or after its time, no
  occurrence runs twice, a taken series is pending again at its reported next time with the right number (read back
  from the store), a one-shot wake that ran is gone, and a series ends only past its `until`.

**The store format** (at the join). The session's EXECUTION bump became one `MANIFEST_FORMAT` bump, 4 to 5 ("5 = a
wake's repeat and occurrence in an execution's wakes"), with a literal sample of the execution layout it replaces (an
execution holding a one-shot wake and its target) in `tests_layouts`; its `kinds::SCHEMAS` edit and its kernel test of
the old schema were dropped. One way, as Item 82's is: once written, the store is format 5, and every older build
refuses it.

**Numbering after missed occurrences**, joined as built: the next occurrence's number follows the schedule
(`occurrence + 1 + missed`, so #4 with two missed is followed by #7), where the design said `occurrence + 1`. The owner,
17:14: "Built seems right, adjust the design to match"; M7 §2.2 now says so.

**How it is proven.**
- **The kernel:** 11 tests in `tests_repeat.rs` (parsing; New York through both 2026 changes, a day crossing one being
  23 h or 25 h with 21:00 kept, an hourly series moving with it, and a 02:30 wall time landing at 03:30 on the spring
  day; Phoenix's 300 days of exactly 24 h; weekdays; the re-arm, the take exactly one frame by the kernel's observer;
  three missed while down, the next #5, no burst; cancel; `until`; the cap and the floor); the kernel's 134 tests.
- **The kernel-sim** at the gate's seeds, and at 40 seeds of 300 steps (623 wakes set, 148 series put back, 107
  occurrences passed over, 7 ended by `until`); a planted revert of the re-arm, which the sim missed until 13d27b9
  added its read-back.
- **The hold:** in a holding session a one-shot wake is set at once, and a repeating one waits for approval with the
  reason naming the external text; in a clean session the series is set at once.
- **The daemon:** `a_repeating_wake_at_the_floor_runs_twice_a_minute_apart` (`min_repeat_minutes = 1`): both
  occurrences post once, a minute apart, as #1 and #2, `wake.list` shows #3 next, and a cancel ends it. It takes about
  63 s of real time, the floor without clock control, and is marked slow.
- **Planted reverts:** re-run at the join on the merged tree, the re-arm off fails the kernel's series tests, and the
  hold off fails the hold's test and external's own.
- **The join's gate** (run 2, 17:52:58, 296 s): 1,782 of 1,782 (10 skipped, 1 slow); lifecycle every budget ok (cold
  start p50 20.2 ms, p95 29.2; a clean shutdown with executions waiting p50 30.8, p95 43.2); the jobs phase's L1 start
  p95 6.67 ms; a plain turn still 5 frames.

**The join** (17:12 to 17:53, by the held-joins wake, automation e3c70850, across another agent's recycle of its first
run). Nine conflicts, resolved on a throwaway merge first and replayed: telemetry's three files took `main`'s label
removal and added the wake metrics (14 instruments); `record.rs` and `record_schemas.txt` took t7-store's side;
`long-files.txt` kept `main`'s, with the protocol's `lib.rs` raised to 2,613 for `WakeInfo`'s fields (its split is
theseus-pf8a). The goldens were rewritten: `kernel_frames.txt` changes in exactly the two `WakeSet` lines, and
`core_output.txt` not at all, since 7.4 masks every number. Gate 1 (17:46) was red on two tests: theseus-core's
`a_write_moves_an_older_store_to_this_builds_format` still pinned format 4 (fixed to 5, the merge amended to 1abf099),
and theseus-amr2's position-rule test, which load-flakes fixes (Item 87). Pushed 17:53:19. theseus-d4pt and
theseus-celu.12 closed.

**The install** (18:52, at d9b0931, with Items 82, 83 and 85): the operator's store moved to format 5 at its first
write.

**Divergences.** The numbering (above). The format bump instead of a schema bump. Occurrences passed over while the
session was busy are counted too, so a node may say `2 missed` without "while the daemon was down". `next` is a
display string, not a time. The design's cost to date per series is not built.

**Known gaps.** A series' cost to date, and the cockpit's view of a series (its span, occurrence, next time and misses,
and an "ended" mark), are not built; the cockpit can sum a series' turns from the wakes each turn's result names.

### Item 85. Tier 7's defences, lighter: act on the config copy, approvals from private places, and the CLI's speed bump (theseus-zmgb; the simplification cut-list's Tier 7, 7.5, 7.6 and 7.8; the `t7-defences` lane; 2026-10-03 15:01 to 18:26, stopped from outside at 17:43 and resumed from the tree at 17:50; a53a79a, 3db7987, a37faf8 and 382dbc4 on ad1e02f, with five merges of `main`, the last d9b0931 on 1abf099; reviewed 18:30 to 18:32; joined 18:48 at d9b0931, a fast-forward; installed 18:52 at d9b0931)

**Why.** The owner's picks at 14:20 ("Your pick is good"), under the default-trust principle (§2): visibility first, no rigid
guardrails. Each of the three defended against the operator's own agent with more machinery than its threat earned.
The vault act-gate held every acting method until the vault confirmed the config copy: a gate in eight places, two
states with a retry loop, an error code, and coupling into the kernel. The `[approval]` matrix judged users by channels
for one operator, and checked a guild channel's viewers on every answer. The ancestry trace walked every answering
process to pid 1, and for the web UI read every process's file descriptors on a runtime worker; the code called itself
"a speed bump before M4's sandbox". The goals are unchanged: an edited copy doesn't act, an approval is the owner's
from a private place, and a job doesn't answer its own approval.

**What landed** (§3.19, §3.9; the root, core, daemon and CLI `AGENTS.md` amended in the lane).
- **7.5, the copy acts** (`config_copy`, `config_gate`, `theseusd`). Writing the copy records its sha256 in the store's
  meta. A start acts on a copy whose digest matches at once; a mismatch, or no digest, execs the daemon in place to
  read the vault first, and health says why. After serving, the vault is read once: the same text, nothing; comments
  changed, the copy and its digest rewritten; the config changed, `config.changed`, the rewrite, and the restart in
  place (`exec_self` and its loop guard kept); changed again, invalid, or unreachable, a row, a log line, a narrative
  line, and health's `held`, and the copy keeps serving. Deleted: the Confirming wait, Held's hold and its retry loop,
  `ACTS` and its wait, the turn's `config.wait`, the actors' waits, `CONFIG_UNCONFIRMED` (-32006, retired), the
  kernel's `unconfirmed_config` with `follow_spend_limit`, kernel-sim's unconfirmed starts, `RESTART_GRACE`, the
  `config.confirmed` row, and `ConfigStatus.retry_in_ms`.
- **7.6, approvals from private places** (`places::owner_in_private`, `PlaceRule::owners`). An answer, a budget answer,
  an undo, a trust, and a publish count only from the CLI, the web UI, a DM with the owner, or a channel bound `private
  = true`, by the owner: `[places] owner`, else the person of each DM the bindings file binds. A shared place's cards go
  to the owner's DM. `[approval]` is retired: it loads with one warning a load, and nothing reads it. Deleted: the
  matrix, the per-answer viewer checks, `approval_checked` and `approval.channel_checked`, and health's matrix in the
  CLI, the web UI and the cockpit.
- **7.8, the CLI's speed bump** (`client::refuse_in_a_job`). The CLI refuses `confirm`, `policy untighten`, `policy
  trust`, `publish` and `aws bootstrap` (their methods, from any command, `theseus rpc`, `watch --interactive` and the
  TUI included, since the check sits in the client's `send`) when `THESEUS_SESSION` is set, sending nothing, and says
  the operator runs it from their own shell. Deleted: `peer.rs`'s trace and the web UI's fd scan, `refused_from_job`
  with `JobActRefused` and the job variant of `approval.refused`, the protocol's `Asker` and `ApprovalRefused` with
  their notification, the renderers' lines, the `asker` in rows, and the `/proc` read at each CLI accept. The review's
  §C4 came with it: the web UI's other-uid check reads `/proc/net/tcp` alone, and `sock_diag`'s six `unsafe` blocks
  go. The daemon's subreaper and orphan adoption (§F9) stay.
- About 4,240 lines fewer (1,730 added, 5,970 removed): 740 in 7.5, 1,317 in 7.6, and 2,183 in 7.8.

**How it is proven.**
- **The lane's gates** before each item's commit (1,692, 1,691 and 1,680 tests) and after each of the last three merges
  (1,689, 1,736 and 1,751), the last four with the benches: a cold start from the copy p50 23.2, 23.7, 26.1 and 37.2 ms
  against 50 (the last alone at normal priority under the gate lock, strictly, after the gate's own two runs missed in
  every phase at load 28 to 34; the phases the gate had not reached ran after it, by `finish-gate.sh`).
- **Seven planted reverts**, each failing its tests: a copy edited since the daemon wrote it passing its digest, a
  mismatched copy acted on at start, a shared place's answer counting, a non-owner's answer counting, a bound DM's
  person no owner, the CLI's marker check off, and the CLI refusing no method. Rerun on the final tree, after voice
  joined, every plant failed again, and the bound-DM plant failed voice's `/join` from a DM too.
- **A live check** (16:44 to 16:48) on a scratch daemon of 7.8's build, with the operator's config note behind a fake
  vault and the fake Discord on a fresh state dir: the places line exactly as before (`places: private: CLI, web, DM
  @zeroaltitude · shared: #openclaw (public tools only)`) with his `[approval]` lines retired and no `[places] owner`; the
  CLI's approval counting; an answer pressed in a shared channel refused; the CLI refusing `confirm` and `rpc
  action.confirm` with `THESEUS_SESSION` set, nothing reaching the daemon; an edited copy refused at the start, which
  exec'd in place and read the vault first; a changed note restarting the daemon in place; and the vault down, held.

**Stopped and resumed.** The first run (82c1f6d0) was stopped from outside at 17:43:49, by another agent's cleanup of
Claude processes older than the operator's re-login, mid-merge of `main` at 1d11949: every conflict resolved and staged,
not committed, and its detached gate on the staged tree finished green at 17:45:19. The second run (from 17:50)
checked the staged merge against voice's `runtime.rs` hooks and the 3,500-line ceiling, committed it (ba13637), merged
1abf099 (d9b0931, keeping wakes-repeat's `KernelConfig` fields without 7.5's retired ones), gated, re-ran the plants,
and finished the report.

**The join** (reviewed 18:30 to 18:32; joined 18:48). The review read each commit. `main` was fast-forwarded from
1abf099 to d9b0931 at 18:30; the join's gate waited on the shared lock behind the cockpit-parity and t7-kernel lanes'
gates, then passed (18:48:38): 1,751 of 1,751; the lifecycle bench's first run missed the clean shutdown with
executions waiting (p50 55.6 ms, right after those gates), and its rerun after a 5 s settle passed at p50 33.1, `main`'s
usual; a plain turn 5 frames; pushed 18:48:56. theseus-zmgb closed.

**The install** (18:52, at d9b0931, with Items 82 to 84). The first start found the copy without a digest (an older
build wrote it), so it read the vault before serving, once (1,034 ms), and kept the copy with its digest; every start
after serves from the copy. The journal had the one expected warning a load, `[approval]` retired, until the operator's
three `[approval]` lines left his template overlay, which `theseusd example-config` pastes into his note (18:53). The
places line was unchanged.

**Divergences.**
- The brief's "log it" kept the rows (`config.held`, `config.invalid`, `config.unreachable`, `config.changed`) as well
  as the log line: visibility first. `config.confirmed` went with "the same text: nothing".
- The owner's fallback is the person of each DM the bindings file binds, read from the place rule's bound places, not
  a DM's person for that DM alone: a private channel's viewer read then has an owner to measure against without
  `[places] owner`. A DM that isn't bound is no owner's.
- A press in a shared place can still reach the core when its card was posted while the place was private (a binding
  changed between starts); the core refuses it there.

**Known gaps.** Filed P3, post-v1: theseus-wj8n (the trace's kernel helpers lost their only reader; they go with the
subreaper, §F9), theseus-gfcz (the copy's digest lives in the store the copy itself names, so an edited copy that also
points `[server] state_dir` at a store prepared with a matching digest acts; a light check by design), and theseus-p7v6
(`theseusd config` cannot say the digest no longer matches; the next start finds it). Accepted, as the owner was told:
approvers besides the owner are gone; a viewer added to a private channel after the start goes unseen until the next
start; the CLI's check is a speed bump (a job that strips `THESEUS_SESSION`, or speaks to the socket or the web UI's
WebSocket itself, is not refused at L0; L1, whose view hides the socket and the loopback, is the boundary); and a
restart onto a changed note can interrupt a turn, which resumes after it.

