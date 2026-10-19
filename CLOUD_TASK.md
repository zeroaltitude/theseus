<!-- CLOUD_TASK.md: your whole task. It came with your branch as its first commit, "cloud task (not for main)". Leave this file in place: the maintainer drops it at the merge, as he drops CLOUD_REPORT.md. Your commits go on top of it, on this branch. -->

You are a cloud build session for Theseus, a Rust agent harness: this repository, a Cargo workspace under crates/, with the cockpit (its web app) under cockpit/. The repository is public. A maintainer (an AI agent working with the repository's owner) reviews your branch, runs the full gate on the owner's machine, runs any live check that needs the owner's keys, and merges it. You can't reach the owner, his machine, or any issue tracker, so everything you need is in this prompt and in the repository.

**Read first:** the root AGENTS.md (the principles, the workflow, the commit style, the store's version rule, the reader rule), the AGENTS.md of every crate or directory you touch (cockpit/ has its own), scripts/AGENTS.md, and .config/nextest.toml. AGENTS.md's "This machine" section describes the owner's machine, not this one. This one is a 4-core VM with 15 GB of RAM and no swap. You run as root, there is no sccache, and nothing else runs here: no operator daemon and no other agents. Use only the tools you need for the code (Bash, Read, Write, Edit, Glob, Grep); call no connector or MCP tool.

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
- About 33 L1 tests fail here: theseus-sandbox's contract tests, its bench's `spawn_100`, and theseusd's sandbox tests. The VM runs as root, and L1 refuses a root daemon's job that has no job cgroup (theseus-pv6i).
- theseus-core's `tests_output::the_cores_output_matches_its_golden` fails under this VM's UTC clock, because two wake lines carry the offset's sign (theseus-ig6n). The gate line below sets `TZ=America/Phoenix` for it; set the same when you run the suite yourself, and commit only the golden lines your change moves.
- Under load, `theseus-store tests_pages::a_filtered_page_equals_the_scans_answer` can pass nextest's 120 s kill (theseus-hohs), and `theseus-core term::tests::python3s_repl_computes_on_the_screen` can fail (theseus-1n2y). Neither is on the flaky list: rerun it alone, and name it in the report.

Timing tests also fail here more often than on the owner's 16-core machine. A test on .config/nextest.toml's flaky list that passes on a retry is fine. Any other failure is yours to explain.

**Other changes in flight.** About fifteen other changes are being built against `main` or merged into it while you work. Stay out of their areas unless your task needs it. If it does, keep the edit small and say so in the report.

These are reviewed, and merging into `main` one at a time tonight. Your clone may hold some of them already:
- route.v1 on the turn path (`[routing]`, detours and switches; store format 15);
- the live rerank (rerank's own breaker, a bounded live wait, per-item grading);
- security.v3's live notices and their brake;
- replay, audit and backfill of judgments;
- the ladder (`pack.mode`, `mode_for`, `ask_mode`, `pack_arm`, `theseus packs`, the cockpit's Ladder panel);
- the tools smalls (categorize on an empty ontology, the LSP's `start_on_edit`, AWS hands' runaway mode);
- the tasks smalls (layer 1 for the owner's tasks only, `task.change_expired`, place warnings, a check's restricted view; store format 16).

These are other cloud sessions like you, each on its own branch:
- consolidation, `Synthesis` nodes and the `+synthesis` arm;
- FSRS-6 retention and the `+retention` arm;
- activation's adjacency and the `+activation` arm;
- tiering: stubs and the bounded heat cache;
- situations, the precedence line and testimony;
- claim leases, the task board, `/tasks` and the cockpit's task graph;
- the cockpit's Budgets, Ledger and Policy tabs;
- the learning loop, if it launches beside them.

On the owner's machine:
- gliding with the place rule (`channel.post` and `channel.read`; places.rs, rpc/publish.rs, approval.rs, outbox.rs);
- the cockpit's Ship view (a gentle roll);
- the Linux lanes: spawn without fork and a light job cgroup (the job launch path), one sync per job completion, background work that yields, LSP watching, then socket activation;
- a benchmark run (bench/).

**Shared files, and how to stay mergeable.** Several of those changes add to the same files, so:
- **The store's format.** The store has one format number, `MANIFEST_FORMAT` (AGENTS.md, "The store's version rule"). If your task adds a field or a variant to a stored record (a new NODE body is one), or a new record kind, bump it by one from main's number as you cloned it (main is at 15 since route, and 16 after the tasks smalls), and add the literal sample of the old layout to theseus-core's tests_layouts.rs, as the rule says. A body variant alone owes no old-layout sample. Others bump it too: the maintainer renumbers at the merge.
- **Files at or near their line ceiling** (scripts/long-files.txt): at it, crates/theseus-protocol/src/lib.rs (2,696) and crates/theseus-core/src/compiler.rs (2,547); near it, crates/theseus-discord/src/runtime.rs, crates/theseus-discord/src/render.rs, crates/theseus/src/render.rs, crates/theseus-kernel/src/kernel.rs, and crates/theseus-core/src/turn.rs. Other changes add to them as well. Put new code in new modules: protocol types in a module file of their own, config types under crates/theseus-core/src/config/, compiler code in crates/theseus-core/src/compiler/, turn logic in a module beside turn.rs, tests in a tests_<subject>.rs of their own. In the long files, add only the field, the call, or the `mod` line.
- **The protocol's TypeScript** is in cockpit/src/protocol.gen/, written from the Rust types by theseus-protocol's test (AGENTS.md says how). Regenerate it if you add or change protocol types; the maintainer regenerates it again at the merge. theseus-protocol's ts.rs type-list test sits at clippy's 100-line limit: add a new type to an existing line, never a new line.
- **The config.** The owner's config holds only what differs from the defaults, so every new key needs a default. If you add a config section, add it to `theseusd example-config`'s template the way the existing sections are, with its template tests (AGENTS.md).

**The gate, before every commit:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` (it takes its lock itself; leave `THESEUS_GATE_LOCK` unset). When its suite phase fails only on the cases above, run the phases after it yourself (`machine_checks` in scripts/gate.sh), and count the commit green if they pass. Chain commands with `&&`, never `;`. The speed budgets are measured on the owner's machine: leave them alone.

The gate's shape phase fails a file over its line ceiling in scripts/long-files.txt. If you grow a file past its ceiling, split it, or raise the ceiling in the same commit and say why.

**Commits:**
- One commit per green sub-step, on the branch named below, created from `main` as you cloned it. Don't merge or rebase onto a newer main; the maintainer merges.
- The subject is `area: what changed (<issue id>)`, as in `git log`. The body says what changed and why, in plain words. End each message with the trailer `Co-Authored-By: Claude (cloud session, Sonnet 5.5) <noreply@anthropic.com>`.
- You can't sign here: commit without `-S`. The maintainer's merge commit is signed.
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261019-gw-core`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: gw-core, Google Workspace as a native capability, part 1 of 6: the `theseus-google` crate, the `[google]` config, the one-time consent and the minted refresh token, the startup check, the RPC and CLI verbs, the private marker, the External helper, the facts and `google.call` (theseus-batvg.1)

Branch: `cloud/20261019-gw-core`. Every commit's subject carries `theseus-batvg.1`. Deadline for the report: 6 hours
after you start.

**Why.** The owner wants Google Workspace (Gmail, Drive, Sheets, Slides, and now Calendar and Docs) as a native Theseus
capability, on the same terms as AWS: the harness holds the credential and makes every call itself, with typed tools,
postures, ledger rows and the hold; the model never sees a token, and no external CLI or MCP server sits between the
harness and Google. It is v1 patch1's primary task. This row builds every shared piece, so the four service rows (mail,
drive, sheets and slides, calendar) can run in parallel after your join. **Source:** the design's §1, §2, §3, §4 and
§6.1, copied below. Read them whole before you start.

**Risk: medium.** New code in new modules, behind config (nothing runs without `[google]`). The parts that can hurt:
the token's handling (never on argv, never logged), the minting write into a secret store, and the data rule's mark.

**Prerequisite:** `main` (`bbc65530` or later).

## The owner's decisions since the design (they win over the design where they differ)

1. **One account, the owner's personal Gmail,** on his own Cloud project's Desktop OAuth client, External, In
   production and unverified (the design's §2.3, row 2). Your code knows none of this: the account is
   `[google] account`, and the fixtures use invented addresses.
2. **The client id and secret are one secret, two labelled lines.** The owner's client sits in one 1Password item
   whose notes field reads `client_id: <id>` on one line and `client_secret: <secret>` on the other. So `[google]
   credentials` is `{ client = "google_client", refresh_token = "google_refresh_token" }` (the `[secrets]` names; these
   are the defaults), not the design's separate `client_id` and `client_secret` names. Parse the `client` secret's
   value: the two labelled lines in either order, `:` or `=`, whitespace and surrounding quotes trimmed, other lines
   ignored; **also** accept the JSON Google's console downloads (`{"installed": {"client_id", "client_secret", ...}}`).
   A value that yields neither fails `google.check` with "the client secret `<name>` holds no client_id line" (or
   client_secret), never echoing the value. One right name: no `client_id`/`client_secret` keys.
3. **Where the refresh token lives (the design's Q1): option A.** Theseus has its own 1Password service account: read
   on the owner's main vault (the client), read and write on a separate vault that holds only the refresh token. The
   daemon keeps **one** token file (`--op-token-file`), and the maintainer points it at Theseus's own account at the
   install, so your minting step runs `op` through the **same** client `secrets.rs` already uses (its `op_bin` and its
   token), with no second token or config key. The design's §2.2 store kinds stay: `op://` is minted with `op item
   create` (no item yet) or `op item edit` (one concealed field) with the item JSON on **stdin**, then read back with
   `op read` and compared by sha256 before success; `file:` is an atomic 0600 write; `env:` is refused with the
   design's sentence; a refused write holds the token in memory only and says so.
4. **Every mail send asks the owner, deterministically** (the owner, verbatim: "approve for send, deterministic").
   This overrides the design's §3.2 "approve as a posture line" and §5's unattended routine sends. A send is
   **authority**: its plan sets `Plan.authority` (`theseus-protocol/src/gate.rs`, the field 39a's layer 1 uses), so
   `Policy` asks at every posture and `posture_only` is false: no `[policy.tools]` line, earned grant, routine grant,
   standing rule, `/trust` or Jev judgment loosens it. In this row that is `google.call`'s classifier: every send
   method (`messages.send`, `drafts.send`, and `messages.import`/`messages.insert`) is authority, as the design's
   §3.2 already lists. G2 does the same for `gmail.send`.
5. **Calendar and Docs are in** (the design's Q2): `services` defaults to all six, and the scope map is the design's
   §2.4 table as written.
6. **`docs/privacy.md` must be true when the owner signs in** (its URL is on the consent screen). On `main` it says
   Theseus has no Google sign-in of its own and keeps no Google token. Rewrite exactly the paragraphs this row makes
   false: what Google data Theseus reads and why (the services `[google] services` lists, by the owner's own consent
   through `theseus google auth`), where the refresh token is kept (the store the owner's `[secrets]` entry names: a
   secret store or a 0600 file; the access token in memory only), how to revoke (`theseus google revoke`, or Google's
   permissions page, then the token is gone from the store), and the China-origin paragraph (every Google result now
   marks the session private, so none of it reaches a model of restricted origin). Keep its plain, short style; no
   names. A public doc: the maintainer scrubs it at the join.

## Your row
| # | Slug | Beads | Scope | Files | Size |
|---|---|---|---|---|---|
| G1 | `gw-core` | theseus-batvg.1 | the design's §6.1 row 1, with the decisions above | `crates/theseus-google/**` (new), `theseus-core/src/config/google.rs` (new), `config.rs` (the `mod`, the field, `validate_google`), `src/google/{mod,auth,call,external,private,send_cap}.rs` (new), `fact/google.rs` (new), `tools_defer/family.rs`, `toolrun.rs` (one call), `broker.rs`, `data_gate.rs` and the route's data step (one condition each), `rpc/google.rs` (new), `theseusd/src/main.rs` (the phase), `theseus/src/{main,cmd}.rs` (the verb) with the CLI's work in a module of its own, `theseus-protocol/src/google.rs` (new), the template, `docs/privacy.md` | L |

## What exists (on your base)
- The AWS precedent for everything here: `theseus-aws` (its `Credentials` with a masked `Debug`, `retry.rs`'s
  backoff and `Retry::NonRepeatable`), `theseus-core/src/aws/` (tools, `external.rs`'s `s3://` External,
  `stack.rs`'s plan bound to a digest), `config/aws.rs` (`AwsCredentialNames`: names of `[secrets]` entries, never
  values), `validate_aws()` in `config.rs`, `rpc/aws.rs` (a CLI-surface-only verb), the `aws.check` startup phase in
  `theseusd/src/main.rs`, and `fact::tool::AwsCalled`.
- `secrets.rs`: `SecretBoard` (`op://`, `env:`, `file:` refs resolved at start; `hold` puts a value on the live board),
  the `op` client (`op_bin`, the service-account token, `op_error`). A fake `op` for tests: point `op_bin` at a script.
- `broker.rs`'s `may_hand_out`: the crown jewels a sandboxed job never gets.
- `scrub/shapes.rs` already knows Google token shapes (`ya29.`, `1//`); `Scrubber::from_board` redacts board values.
- `theseus_tools::External` and the external hold (`external.rs`, `SessionRecord.external`, `[policy] external_text`).
- `routing/data.rs` (`DataAnswer::private_by_name`) and `data_gate.rs` (the provider floor, theseus-oejh3).
- `tools_defer/family.rs` (`FAMILIES`, `of_wire`, `hideable`), `tests_census.rs`, `first_request.rs`.

## What to build (commit in this order, so a short session still leaves a useful prefix)
1. **The crate and the config.** `crates/theseus-google`: the OAuth client (refresh single-flight about 5 minutes
   before expiry; the access token `Zeroizing` with a masked `Debug`), the REST client on the workspace `reqwest`, the
   retry and backoff of the design's §4.3, the error map (`invalid_grant` → needs-auth; 403 `SERVICE_DISABLED` /
   `accessNotConfigured` → "enable the <name> API in project <number>", with Google's activation link; Advanced
   Protection and `admin_policy_enforced` named), the method classifier (§3.2, with decision 4), and the **fake**
   (shared contract 6). `[google]` in `config/google.rs` with `deny_unknown_fields`, `validate_google`, the template
   section (shared contract 7), the service → scope map with every tool's accepted scopes (§2.4; shared contract 2),
   and the client-secret parser (decision 2).
2. **The consent and the minting** (§2.2 with decisions 2 and 3): `google.auth.begin` (PKCE S256, a `state`, a
   one-shot loopback listener on `127.0.0.1:0` with a 10-minute timeout, the consent URL with `access_type=offline`,
   `prompt=consent`, `include_granted_scopes=true`, `login_hint`, exactly the requested scopes), the redirect page,
   `google.auth.finish` (the pasted address bar for `--manual`, or after 90 s with no redirect), the code redeemed with
   the verifier, the granted scopes checked per service (a partial grant turns off only the tools missing a scope), the
   account checked against the `id_token`'s `email` (a mismatch stores nothing), the store write by kind, the live
   hold, and the `google.authorized` row (account, scopes, store kind; never the token).
3. **The daemon and the CLI.** The `google.check` startup phase beside `aws.check` (refresh, then the cheapest call
   among the granted services to confirm the account; tools fail closed until it passes), the health state (`ok` |
   `needs_auth` | `off`), `invalid_grant` → `needs_auth` with **one** Notice to the owner ("Google access lapsed: run
   `theseus google auth`") and a `google.auth_lapsed` row, `google.revoke` (revoke at Google, clear the store, drop
   it from the board, `google.revoked`), `google.status` (account, services, granted scopes, last refresh, state, the
   tools on). The RPC methods refuse every surface but the CLI, as `rpc/aws.rs` does. CLI: `theseus google auth
   [--manual] | status | revoke`; the paste is read from the terminal, never an argument.
4. **The shared tool pieces.** `google::finish` (the `private` marker on every result, and External with the
   owner-authored rule: `gmail:thread/<id>`, `drive:<id>`, `calendar:<id>` urls), the data rule's two conditions
   (shared contract 10), the crown-jewel names (the client and refresh-token secret names), `google.called` and the
   auth facts in `fact/google.rs` (service, method, resource id, class, status, attempts, quota units, recipient
   **count** for a send; never content, subjects, addresses or tokens), the daily send counter (`max_sends_per_day`,
   default 25, 0 = no sends; counted per local day, sends by `google.call` included), the `mail`, `calendar` and
   `docs` families for every tool name, `google::tools` and its one registration call (shared contract 3), and
   **`google.call`** (any REST method of the granted services; GET is Read, other verbs Write, the authority methods
   of §3.2 plus decision 4, unknown methods Write; External always; deferred).
5. **`docs/privacy.md`** (decision 6).

## Tests (each with a planted revert; the first is the key test)
- **`a_refresh_token_for_another_account_is_refused_and_not_stored`** (the fake's `id_token` names another address:
  nothing is written to the store, nothing held). Revert: skip the account check.
- `pkce_state_mismatch_is_refused`, `a_pasted_redirect_url_finishes_auth`.
- `the_refresh_token_never_reaches_op_argv` (a fake `op` records its argv and its stdin: the token is in stdin only).
  Revert: pass the field as an argument.
- `a_minted_token_is_read_back_before_success`, `a_refused_op_write_holds_the_token_in_memory_and_says_so`,
  `env_ref_refresh_token_is_refused`.
- `the_client_secret_parses_from_two_labelled_lines_in_either_order` (and from the console's JSON; a value with
  neither fails with the sentence and never the value). Revert: require `client_id:` first.
- `requested_scopes_are_exactly_the_declared_set`, `partial_grant_turns_off_only_the_tools_missing_a_scope`,
  `a_disabled_api_names_the_api_to_enable`.
- `invalid_grant_sets_needs_auth_and_one_notice` (two lapsed refreshes, one Notice). Revert: notice on every lapse.
- `refresh_is_single_flight` (ten concurrent calls, one token request to the fake).
- **`a_send_through_google_call_asks_at_every_posture_and_no_grant_loosens_it`** (posture `open` for `google.call`,
  an earned grant and a routine grant that would cover it, a trusted session: the plan still waits for approval).
  Revert: classify `drafts.send` as Write.
- `google_call_classifies_delete_permissions_and_acl_as_authority`, `the_daily_send_cap_counts_google_call_sends`.
- **`a_google_result_makes_the_session_private_and_the_floor_refuses_a_restricted_model`** (a fake Google read in a
  session, then a request of that session to a profile of restricted origin is refused before a byte is sent, and the
  route's data answer is `private`). Revert: drop the mark.
- `owner_authored_unshared_content_is_not_external`, `google_secrets_never_reach_l1`,
  `config_rejects_unknown_google_keys`, `a_token_shaped_value_never_reaches_a_fact_or_a_log`.
- `no_google_section_registers_nothing_and_reads_nothing` (no tool, no phase, no secret read, a plain turn's request
  bytes equal to `main`'s).

## The design's own words: §1. Goal and shape

**Goal.** Make Google Workspace a native capability of the harness, on the same terms as AWS. The harness holds the
credential and makes every call itself, with typed tools, postures, ledger rows and the hold. The model never sees a
token, and no external CLI sits between the harness and Google (no `gog`, no MCP server). This follows AGENTS.md's
NATIVE FIRST rule ("a new capability is a native toollet unless a written reason says it can't be").

**Shape.**
- A new crate `crates/theseus-google`, the counterpart of `theseus-aws`. It holds the OAuth client (token refresh,
  PKCE, the loopback consent listener), a thin REST client on the workspace `reqwest 0.12` (rustls, already used by
  six crates), retry and backoff, error mapping and a method classifier. It has no Google SDK dependency. The
  endpoints are plain JSON REST: `gmail/v1`, `drive/v3`, `sheets/v4`, `slides/v1`, `calendar/v3`, and
  `oauth2.googleapis.com/token`.
- The tools live in `crates/theseus-core/src/google/`, mirroring `src/aws/`. They are built by
  `Google::from_config(&cfg.google, secrets)` and registered next to the AWS tools in `toolrun.rs` (~:2202).
- **One configured account** (`[google] account`). A second account later is a rename to `[google.accounts.<email>]`,
  which the right-over-compatible rule allows. Building the map now buys nothing.
- **Services:** Gmail, Drive, Sheets and Slides, read and write, as asked.

**Calendar and Docs in the first cut: I recommend yes to both** (owner question Q2). Their scopes are already on the
owner's consent screen, so saying yes costs no Cloud Console change. The evidence:
- **Calendar is the most-used Google service by far.** The owner's usage history with another tool has 1,128 calls of `gog calendar
  events primary --from …` and 802 calls of the unread-mail search, against 12 `drive recent`, 3 `drive get` and 3
  `docs export`. No Sheets use, no sends and no event creation were found. The real use today is the hourly and
  morning briefing (unread mail plus today's events, on both accounts).
- Calendar costs two tools and one sensitive (not restricted) scope, `calendar`, which is already declared. It is
  what makes the morning note good (§5).
- **Docs costs almost nothing.** Reading a Doc is a Drive export to `text/markdown`. Writing one is a Drive upload of
  markdown converted to a Google Doc. Both ride on the `drive` scope and the Drive tools. Fine-grained in-place Docs
  editing (`documents.batchUpdate` with index arithmetic) has no typed tool. It goes through the escape hatch
  (`google.call`) on the `documents` scope.


## The design's own words: §2. Config and auth, first-class like the AWS key

### 2.1 The config

The model is `[aws.accounts.<id>]` plus `AwsCredentialNames` (`crates/theseus-core/src/config/aws.rs:239`): the
section holds **names** of `[secrets]` entries, never values. Each `[secrets]` value is an `op://…`, `env:` or
`file:` SecretRef, resolved at start by `SecretBoard` (`secrets.rs`; `theseusd/src/main.rs:453`).

The owner's live config would read as below. `<item>` is the 1Password item name he will give. The field names
`client_id` and `client_secret` are a request: please name the item's two fields exactly that. A downloaded
`client_secret_….json` attached as a file would need a different reference.

```toml
[google]
account = "<owner>@gmail.com"            # the personal Gmail; the startup check refuses a token for any other
services = ["gmail", "drive", "sheets", "slides", "docs", "calendar"]   # scopes derive from this (2.4)
max_sends_per_day = 25                   # sends (gmail.send) per local day, a runaway brake; 0 = no sends
# credentials = { client_id = "google_client_id", client_secret = "google_client_secret",
#                 refresh_token = "google_refresh_token" }   # [secrets] names; these are the defaults
# endpoint = "http://127.0.0.1:7499"     # loopback stand-in for tests only (dev_origin_ok, as [aws] endpoint)

[secrets]
google_client_id     = "op://<vault>/<client item>/<field>"
google_client_secret = "op://<vault>/<client item>/<field>"
google_refresh_token = "op://<theseus vault>/<token item>/refresh_token"  # minted by `theseus google auth` (2.2)
```

`docs` and `calendar` are dropped from `services` if the owner says no to Q2. Their scopes are then not requested,
although they stay declared on the consent screen. Declaring a scope without requesting it costs nothing.

- **Type:** `GoogleConfig` in `crates/theseus-core/src/config/google.rs` with `#[serde(deny_unknown_fields)]`.
  - Declare `mod google;` beside `mod aws;` (`config.rs:15`).
  - Add `pub google: GoogleConfig` to `struct Config` (:93), with `#[serde(default, skip_serializing_if =
    "GoogleConfig::is_empty")]`.
  - Add `validate_google()` next to `validate_aws()` (:1732). It checks that `account` is an email, that each
    credential name exists under `[secrets]`, that `services` holds known names without duplicates, and that
    `endpoint` passes `dev_origin_ok`.
  - Document it in `theseus.example.toml` beside the AWS section (:526).
- **`services`, not raw `scopes`.** The brief asked for scopes in the config. I chose a service list mapped to scopes
  in code (§2.4), for two reasons. Scope URLs are long and easy to get subtly wrong (`drive` vs `drive.file` vs
  `drive.readonly`), and the tools need to know which services exist anyway. The map requests exactly the scopes the
  owner declared, so the consent screen and the request can't drift apart. `theseus google status` prints the scopes
  actually granted, so nothing is hidden.
- **Harness-only secrets.** Add the three Google names to the crown jewels that `broker.may_hand_out` refuses to L1
. A sandboxed job never gets a Google token.
- **Redaction is already there.** `scrub/shapes.rs` already catches Google token shapes (`ya29.`, `1//`), and
  `Scrubber::from_board` redacts every board value from tool output.

### 2.2 `theseus google auth`: the one-time consent

`theseus aws bootstrap` (`crates/theseus/src/main.rs:1031`, `cmd.rs:776`) is the precedent: a CLI verb that goes over
JSON-RPC to the daemon, and the daemon refuses any surface but `Surface::Cli` (`rpc/aws.rs`). The Google verbs work
the same way.

```
theseus google auth [--manual]     # consent, store the refresh token, hold it live (no restart)
theseus google status              # account, services, granted scopes, last refresh, state (ok | needs_auth)
theseus google revoke              # revoke at Google, clear the stored token, state -> needs_auth
```

**The flow** (an OAuth 2.0 Desktop client with a loopback redirect and PKCE S256, per RFC 8252):
1. **`google.auth.begin`** (RPC, CLI surface only). The daemon:
   - draws a PKCE verifier and a `state`;
   - binds `127.0.0.1:0` (an ephemeral port, loopback only, one-shot, 10-minute timeout);
   - returns the consent URL, with `access_type=offline`, `prompt=consent` (so Google always issues a refresh token,
     even on a re-auth), `include_granted_scopes=true`, `login_hint=<account>` and the scopes from §2.4.

   The CLI prints the URL. The URL holds no secret (client id, PKCE challenge, state), so the maintainer can run the
   command and hand the owner the URL in the DM.
2. **The owner consents in the browser.** Google redirects to `http://127.0.0.1:<port>/?code=…&state=…`. The daemon
   checks `state` and answers with a small "done, close this tab" page.
3. **WSL fallback, built in from day one.** The browser runs on Windows and the daemon in WSL. The NAT-mode localhost
   forwarding usually carries the redirect, but mirrored loopback broke both ways on 2026-10-04.
   - So `--manual`, which also kicks in automatically when no redirect arrives within 90 s, prompts: "paste the
     address bar of the page that failed to load".
   - The CLI sends the pasted URL to `google.auth.finish`, which parses out the code and checks `state`.
   - The paste goes into the terminal, never into chat (gog's rule). The code is single-use, expires in minutes and
     is useless without the daemon's PKCE verifier, but it stays out of transcripts anyway.
   - **Open the URL on the Windows desktop, not on a phone.** On a phone, `127.0.0.1` is the phone itself, so the
     redirect can't reach the daemon, and the paste path needs the terminal. WSL runs in `networkingMode=mirrored`
     today , so a desktop browser should reach the listener directly.
4. **The daemon redeems the code** at `oauth2.googleapis.com/token` with the verifier and the client secret. Then it:
   - **Checks the granted scopes** in the response's `scope` field. Google's granular consent lets the user untick
     boxes. A missing scope is reported by name ("Drive was not granted: the drive tools stay off"), and the services
     that were granted still work.
   - **Checks the account** with the `openid email` scopes. The `id_token`'s `email` comes straight off the TLS token
     endpoint, so no signature check is needed; a mismatch with `[google] account` refuses and stores nothing.
   - **Stores the refresh token where `[secrets] google_refresh_token` points (the minting step):**
     - `op://` → the daemon runs `op` with its own service-account token. If the item doesn't exist yet, it runs
       `op item create`; otherwise `op item edit` sets one concealed field. The item JSON goes in on stdin (`op item
       edit` takes piped input), so the token is never on argv, where `/proc` would show it, and never in a temp file.
       Then the daemon reads the field back with `op read` and compares sha256 before it reports success.
     - `file:` → an atomic write at mode 0600.
     - `env:` → refused ("an env ref can't be written; point it at op:// or file:").
     - A refused write (no `write_items` on the vault) fails with that sentence, and the token is held only in
       memory: Google works until the next restart, and the CLI says so.
   - **Holds the token on the live board** (`SecretBoard::hold`), so the tools work at once without a restart.
   - **Writes a `google.authorized` ledger row:** account, scopes, store kind. Never the token.

**Where the refresh token can live: the write-access problem.** The daemon reads 1Password through the service
account in `a token file`, which the owner's other agents share. That account has `read_items` on
the owner's main vault and `write_items` on nothing. Three ways out:

| | Setup | Write blast radius | Code | Verdict |
|---|---|---|---|---|
| **A. Theseus gets its own service account** | The owner creates a vault `Theseus-Google`, then a service account `theseus` with read on the owner's main vault and read+write on `Theseus-Google`. He saves its token as an item in the owner's main vault; I fetch it to `a token file of its own` (0600) and point `theseusd.service --op-token-file` at it (standing install permission). | One vault holding one item | None beyond the minting step: the daemon still has one token, and `SecretBoard` reads everything as today | **Recommended.** It also splits Theseus's 1Password identity from OpenClaw's, so either can be revoked alone. |
| B. Write on the owner's main vault | A new service account with read+write on the owner's main vault. The refresh token sits next to the client. | Every item in the vault, including the GPG passphrase and every other credential | None | Not recommended: a token that can overwrite every secret the agents use. |
| C. `file:` | None: `google_refresh_token = "file:~/.config/theseus/google-refresh-token"`, 0600 | One file | None | Fine as a fallback. The exposure is the same tier as the service-account token file beside it, which unlocks the whole vault. The loss is that a WSL rebuild loses the token (re-auth takes a minute) and 1Password doesn't show it. |

As far as I know, 1Password fixes a service account's vaults and permissions when it is created, so A and B each
mean a new account rather than an edit to the existing one. Check that at 1Password.com before relying on it. Until
the owner picks, gw-core builds all three store kinds, and the minting step behaves as described above whichever is
configured. This is new Q1 in §7.

**Token life in the daemon.**
- The access token is in memory only, `Zeroizing`, with a masked `Debug`, as `theseus-aws`'s `Credentials`.
- It is refreshed single-flight about 5 minutes before its ~1 h expiry.
- A startup phase `google.check` mirrors `aws.check` (`theseusd/src/main.rs:1176`). It refreshes the token and calls
  `gmail users.getProfile` (or the cheapest call among the granted services) to confirm the account. Tools fail closed
  until it passes.
- **When the grant lapses** (`invalid_grant`), the state becomes `needs_auth`:
  - health shows it;
  - one Notice goes to the owner's DM ("Google access lapsed: run `theseus google auth`");
  - each tool fails with that same sentence;
  - a `google.auth_lapsed` row is written.

### 2.3 OAuth app setup: what actually breaks

**The owner's setup matches what this section recommends for a personal account,** row 2 of the table below. The
table and the Workspace notes stay as the record of why, and for a second (work) account later.

**Two setup steps that are easy to miss tonight:**
- **Enable each API in the new project** (APIs & Services → Library): Gmail API, Google Drive API, Google Sheets
  API, Google Slides API, Google Docs API and Google Calendar API. Consent can succeed while every call then fails
  with 403 `SERVICE_DISABLED` / `accessNotConfigured`. `google.check` maps that error to "enable the <name> API in
  project <number>", with the activation link Google puts in the error.
- **The scopes go on the Data Access page** (Google Auth Platform → Data Access) as declared. The client requests
  exactly that set (§2.4).

**Gotchas for this account and app type:**
- **The warning screen.** Consent shows "Google hasn't verified this app" → Advanced → "Go to <app> (unsafe)". That
  happens once per consent, and again on each re-auth.
- **The 100-user cap** applies to unverified apps with sensitive or restricted scopes. It doesn't matter for one user.
- **Changing the account password** revokes refresh tokens that carry Gmail scopes. That shows up as `needs_auth`
  (§2.2).
- **Six months unused** expires a refresh token. The daily `google.check` refresh keeps it alive.
- **Advanced Protection Program** blocks unverified apps. If the personal account is enrolled, `google auth` names
  that error, and this whole route needs a different plan.
- **Granular consent:** the owner can untick any box. A partial grant is handled per service (§2.2, step 4).

**The app type is Desktop** ("Desktop app" in Cloud Console → Credentials):
- Loopback redirects on any port need no registration.
- PKCE is supported.
- Google itself treats a Desktop client secret as non-confidential, but it goes in the vault anyway.
- The old copy-paste out-of-band flow (`urn:ietf:wg:oauth:2.0:oob`) is gone. Loopback with a manual paste is the
  replacement.

**A dedicated Theseus client, not gog's** (the owner's own project gives this). Google keeps at most 100 live refresh
tokens per client per account and silently kills the oldest. A separate client also means revoking Theseus never logs
gog out, and the reverse. gog's personal client (`its own credentials file`) stays untouched.

**Publishing status, the trap gog users hit.**

| Status | Who | Refresh token | Consent screen | Verdict |
|---|---|---|---|---|
| External, **Testing** | listed test users only | **expires after 7 days** for any scope beyond name/email/profile | "unverified" banner | **never use it.** Every week Theseus would lose Google until someone re-consents. |
| External, **In production, unverified** | any Google account (100-user lifetime cap for sensitive and restricted scopes) | no time limit | "Google hasn't verified this app" → Advanced → "Go to … (unsafe)", once | **personal gmail.com**: the only good option. Don't submit for verification: restricted scopes would need a paid security assessment (CASA), which a one-user app doesn't need. |
| **Internal** | users of the Workspace org that owns the Cloud project | no time limit | clean, no warning | **Workspace account**: best by far. No verification, no expiry, no warning. Only possible when the Cloud project belongs to that org. |

**For a Workspace account later:** Admin console → Security → API controls can block "unconfigured" apps from
restricted Gmail and Drive scopes. An Internal app may need "Trust internal, domain-owned apps" ticked, or its client
id marked Trusted. `google auth` maps that error (`admin_policy_enforced`) to that sentence.

### 2.4 Scopes: the owner's declared set, mapped per service

The code requests exactly the scopes the owner declared, one service at a time. `openid` and `userinfo.email` are
always requested.

| `services` entry | Scope requested | Class | Tools it turns on | Notes |
|---|---|---|---|---|
| (always) | `openid`, `userinfo.email (full URL form)` | basic | — | The account check. `userinfo.email` is the long form of `email`, and the `id_token` carries `email` with either. |
| `gmail` | `gmail.modify` | restricted | gmail.\* | Every Gmail scope that can read mail is restricted. `gmail.modify` lacks only **permanent delete** (trash still works, recoverable for 30 days) and IMAP/SMTP, which is what I want. |
| `drive` | `drive` | restricted | drive.\*, and Docs read and write as markdown | `drive.file` would see only files Theseus created, so no search over existing files. |
| `sheets` | `spreadsheets` | sensitive | sheets.\* | The Sheets API also accepts `drive`. Requesting `spreadsheets` as well means a consent that unticks Drive still leaves the Sheets tools working on a sheet by id. |
| `slides` | `presentations` | sensitive | slides.write, and Slides text in drive.read | As for Sheets. |
| `docs` | `documents` | sensitive | `google.call` on `docs.documents.*` (in-place edits) | Read and write as markdown ride on `drive`. |
| `calendar` | `calendar` | sensitive | calendar.\* | **Wider than the tools need.** Full `calendar` also manages calendars and their sharing (ACLs). The typed tools never touch those, and §3.2 makes `calendar.acl.*` and `calendars.insert/delete/clear` authority in `google.call`. If the owner prefers least privilege, he can swap `calendar` for `calendar.events` on the Data Access page; that is a one-line change in the map. |

**Partial grants degrade per tool, not per service:** each tool lists the scopes it accepts. For example
`sheets.read` runs on `spreadsheets` or `drive`, and `drive.find` only on `drive`. `theseus google status` shows which
tools are on.

**Why request exactly the declared set:** I haven't confirmed whether Google refuses an undeclared scope for an
unverified production app or only warns. Requesting the declared set means consent never finds out.


## The design's own words: §3. The tool surface

**Principles:**
- The fewest tools that cover the real jobs, typed where the harness adds value: MIME building, HTML to text,
  markdown to Doc conversion, threading headers, External marking, a send bound to a digest.
- One generic escape hatch, `google.call`, for the long tail, as `aws.call` is for AWS. The tool-tree measurement
   found Opus 5.5 writes `aws.call` from its own knowledge and never used the
  typed AWS helpers. Google's REST is just as well known, so the typed set stays small.
- **Every Google tool is deferred.** A deferred declaration costs a constant ~93 tokens total however many there are,
  so the first-request budgets (8,000 cli/task, 15,000 DM; `first_request.rs`) are untouched. `[tools.defer]
  load_share = 0.10` promotes a tool to always-loaded only once the ledger shows real use. `gmail.search` and
  `calendar.events` in the DM are the likely first.
- **Families** (`tools_defer/family.rs` `FAMILIES`, `of_wire()`):
  - `mail` (gmail.\*), `calendar` (calendar.\*), and `docs` (drive.\*, sheets.\*, slides.\*, google.call);
  - without an entry, the tools would land in `other`, which `hideable()` never hides.
- **Schema size:** each tool's description plus schema is aimed at ≤ 220 tokens, held by `tests_census.rs` rows. All
  15 loaded together come to about 3k tokens, which is exactly why they are deferred.
- **Names** follow the dotted internal / underscored wire convention (`wire_name`, `theseus-tools/src/lib.rs:434`).
  The template's routine-grant example `gog.gmail.search` becomes `gmail.search` (right over compatible).

### 3.1 The tools

The postures below are template defaults in `[policy.tools]`. The template must list every tool
(`example_template_lists_every_tool_under_policy_tools`). "inherit" means the tool takes `[policy] enforcement`, which
is `notify` on the owner's machine. **Ext** means the result is marked External (§4.2). **Authority** means
`Plan.authority` forces approve whatever the config says.

| # | Tool | Does | Class | Posture | Ext |
|---|---|---|---|---|---|
| 1 | `gmail.search` | `{q, max≤50=20, page?}`. Gmail query syntax. One row per thread: id, from, subject, date, snippet, labels, unread, count. | Read | inherit | yes, unless every sender is the account |
| 2 | `gmail.read` | `{id, save_attachments?: dir}`. A thread or a message: headers, plain-text bodies (HTML → text, quoted tails folded), attachments listed (name, type, size). Saving them writes under the workspace for the file and PDF tools. | Read | inherit | yes, unless every message is from the account |
| 3 | `gmail.draft` | `{to[], cc?, bcc?, subject, body, reply_to?: message id, draft_id?}`. Creates or replaces a draft. `reply_to` sets threadId, In-Reply-To, References and a `Re:` subject. Plain text by default. | Write | notify | — |
| 4 | `gmail.send` | `{draft_id}`. Sends **an existing draft only**. The plan reads the draft and binds the call to its content digest, so a draft changed after the card was shown is refused (the `aws.stack.apply` pattern, `aws/stack.rs:58`). Counts against `max_sends_per_day`. | Write | **approve** (a posture, see Q3) | — |
| 5 | `gmail.label` | `{ids[], add?[], remove?[], trash?: bool}`. Archive (remove INBOX), mark read (remove UNREAD), star, user labels; `trash` moves to Trash (recoverable). | Write | notify; `trash` → **authority** | — |
| 6 | `calendar.events` | `{from?, to?, q?, calendar?="primary", max?}`. Events with attendees and their responses, conference link, organizer. | Read | inherit | yes, unless the organizer is the account |
| 7 | `calendar.put` | `{event_id?, summary, start, end, attendees?, location?, description?, notify_attendees?: bool=false, cancel?: bool}`. Create, update or cancel. | Write | notify; **authority** when invitations or cancellations go out (`notify_attendees` with attendees) | — |
| 8 | `drive.find` | `{q?, text?, name?, folder?, type?: doc\|sheet\|slides\|pdf\|folder, recent?: bool, max?}`. Typed fields compile to Drive `q`. `text` is `fullText contains` (the Drive API does have full-text search; gog doesn't). Rows: id, name, type, owner, modified, shared, link. | Read | inherit | yes, if any row isn't the account's own unshared file |
| 9 | `drive.read` | `{id, range?, save_to?}`. A Doc → markdown; a Sheet → its tabs plus the first 200 rows as a table (or `range`); Slides → per-slide text with object ids and layouts; PDFs and binaries → `save_to` (required past 1 MB). | Read | inherit | yes, unless owned by the account **and** `shared == false` |
| 10 | `drive.write` | `{id?, name?, folder?, content? \| from_path?, as?: doc\|sheet\|slides\|raw, move_to?, rename?}`. Create, or replace a file's content (Drive keeps revisions), or move or rename it. Markdown → Doc and CSV → Sheet go through Drive's import conversion. | Write | notify | — |
| 11 | `drive.share` | `{id, email?, role?: reader\|commenter\|writer, anyone_with_link?: bool, remove?: bool, notify?: bool}`, and `trash?: bool` for files. Every permission change and every trash. | Write | **authority** | — |
| 12 | `sheets.read` | `{id, ranges[]}`. Values, formatted or raw, plus tab metadata. | Read | inherit | as drive.read |
| 13 | `sheets.write` | `{id?, title?, range, values[][], mode: update\|append\|clear, input?: user\|raw}`. No `id` creates a spreadsheet. `user` = USER_ENTERED, so formulas evaluate. | Write | notify | — |
| 14 | `slides.write` | `{id?, title?, slides?: [{layout?, title?, body?, notes?, replace?: object id}], replace_text?: {find: replace}, requests?: [...]}`. No `id` creates a deck. Simple ops for the common case; `requests` passes raw `batchUpdate` through for anything else. | Write | notify | — |
| 15 | `google.call` | `{method: "drive.files.list" \| "gmail.users.settings.filters.list" …, params?, body?}`. Any REST method of the granted services. Classified by a method table (§3.2). | per method | per method | yes, always |

Not built:
- Contacts (no use found);
- Gmail settings: forwarding and filters need `gmail.settings.*` scopes, which aren't granted, and auto-forwarding is
  the classic exfiltration move;
- permanent delete (scope);
- Pub/Sub push (§5).

### 3.2 Postures and the `google.call` classifier

**Reading:** the template leaves reads to inherit `enforcement` (notify on the owner's machine), as the brief asked.
`[policy.tools]` can open them per tool, as the live config opens `aws.describe` and `fs.read`.

**Drafting, labelling, writing files, sheets and slides:** notify. Nobody else sees a draft until it is sent, and
Drive keeps revisions, so all of these can be undone.

**Sending mail:** approve, as a posture line (`"gmail.send" = "approve"`), not authority, so the owner can loosen it.
That matters for the killer use (§5). Earned grants and routine grants (`policy/earned.rs`,
`routine/grant.rs`) can then let one named routine send unattended; a routine grant can loosen only a posture-only
wait. This is owner question Q3.

**The approval card for a send** (Discord `render/preview.rs`, cockpit, `theseus confirm`) shows:
- from, to, cc and bcc, with every recipient **not already on the thread and never mailed before** flagged "new";
- the subject;
- the first ~15 lines of the body;
- the attachment names.

**Sharing, permission changes, trash, and calendar invitations or cancellations:** approve by authority. These are
outward or reach other people, so no config line loosens them, as AWS deletions of stateful resources wait for the
operator today.

**`google.call` classes:**
- GET → Read.
- Other HTTP verbs → Write.
- These methods → authority: `*.delete`, `*.trash`, `*.untrash`, `permissions.*` (create, update, delete), `*.send`
  (`messages.send`, `drafts.send`), `messages.import` and `messages.insert`, `calendar.acl.*`,
  `calendars.insert`/`delete`/`clear` (the full `calendar` scope allows them), and `events.*` with
  `sendUpdates != none`.
- Unknown methods → Write, and they still need a granted scope at Google.

`google.call` counts sends against the daily cap too, so the escape hatch can't route around the brake.


## The design's own words: §4. Safety

The default-trust principle sets the bar: light, cheap bolstering, nothing extraordinary. Everything below reuses an
existing mechanism.

### 4.1 Data gate: all Workspace content is private

**The rule** (the owner): work, private, personal or confidential information never goes to China-origin
models. Mail, calendars and Drive content are all of those.

**The mechanism.** Every Google tool result carries an explicit `private` marker on its `ToolOutput`.
- In theseus-a1wvq's design, a tool result ends the open run unless `data::screen` passes it. The screen checks for
  secret shapes and requires every path to sit under `open_paths`.
- **A Gmail result has no paths, so it could pass that screen.** The marker closes that hole: `data::screen` fails any
  result carrying `private`, and so does `content_class` (a1wvq.7) when it lands.
- Fail closed: a Google result in an open session ends the open run, and every China-origin profile is skipped from
  then on.

**Ordering:**
- If cn-tool-screen (a1wvq.9) has joined before G1, G1 adds the one `private` check there.
- If it hasn't, G1 adds the marker field and the.9 row picks it up. Its brief gains one test,
  `a_google_result_is_never_open`.

**Background work and Jev.** Compaction, consolidation and the morning note already fall back from restricted
targets under a1wvq.10. Jev's route.v6 `data` question would usually say "private" for mail anyway. It is not the
guard: the marker is.

### 4.2 Inbound mail and shared docs are external untrusted text (prompt-injection vector #1)

**Anyone can email the owner, and anyone can share a doc with them.** So Google reads return
`theseus_tools::External { url }` (`theseus-tools/src/lib.rs:117`). The url is a stable `gmail:thread/<id>`,
`drive:<id>` or `calendar:<id>` marker, as `aws/external.rs` does with `s3://`. The existing hold then does the rest:
- `SessionRecord.external` is set, and a `session.external_read` row is written;
- after that, every non-Read call asks (`[policy] external_text = "ask"`) until the owner trusts the session (`/trust`
  or "Approve + trust session");
- the hold spreads to tasks the session starts.

**Two light refinements**, both in keeping with default trust:
- **Owner-authored content is not external.** A thread where every message is from the account, an event the account
  organizes, or a file the account owns that is not shared: none of these is stranger text. One metadata check per
  result: `From` headers, `organizer.self`, `owners[].me && !shared`. This removes most of the friction on the
  owner's own docs.
- **`gmail.draft` is exempt from the hold, as `wake.at` is.** A draft is invisible to everyone until `gmail.send`,
  which keeps its own approve posture and shows the draft on the card. Without the exemption, "read this thread and
  draft a reply" costs two cards (draft, then send) where one carries all the risk. Labels are not exempt: an
  injected mail must not be able to archive or mark read the mail it wants hidden.

**Residual risk, named and not solved here.** A held session can still call Read-class tools, and `http.fetch` is
Read. An injected mail could steer a fetch to `attacker.example/?q=<private text>`. This exposure exists today for
web text; Google makes it richer. Follow-up: in a session that holds both private Google content and the external
hold, `http.fetch` to a host outside `[tools] egress`/allowlists asks. That is a one-predicate change in
`external::gate`, filed with this work as a separate small row and **not** in the first cut unless the owner wants it.

### 4.3 Rate limits and quotas

- **Gmail** gives 250 quota units per user per second. `messages.get` costs 5, `threads.get` 10, `messages.list` 5,
  `drafts.create` 10, `messages.send`/`drafts.send` 100. An agent loop won't reach that in practice.
- **Gmail's own sending limits** are about 500 recipients a day on gmail.com and 2,000 on Workspace. Theseus's own
  brake, `max_sends_per_day = 25`, sits far under them and is the one that matters.
- **Sheets** allows about 60 reads and 60 writes per minute per user. That is the one a loop can hit.
- **Drive** allows about 12,000 requests per minute per user.
- **Slides** allows about 60 writes per minute per user.
- Recheck all of these against Google's quota pages at build time.

**Handling:**
- Retry 429, `rateLimitExceeded`, `userRateLimitExceeded` and 5xx with full-jitter backoff, from 1 s up to 20 s,
  three attempts, honouring `Retry-After`. This is `theseus-aws/src/retry.rs`'s shape.
- **`gmail.send`, `drafts.send` and every authority method are non-repeatable**
  (`Retry::NonRepeatable`). A send that may have gone out is reported `outcome_unknown` and never retried.
- There is no client-side rate limiter, as AWS has none: the cap plus backoff is enough.
- The calls cost no money. `action.planned` reserves USD 0.

### 4.4 Audit rows

- **`google.called`** (`fact/google.rs`, a new `LedgerKind`, emitted like `fact::tool::AwsCalled` at
  `toolrun.rs:1842`): service, method, resource id (thread, message, file or event id), class, status, attempts,
  quota units and recipient **count** for sends. It never holds content, subjects, addresses or tokens.
- **Auth rows:** `google.authorized`, `google.auth_lapsed` and `google.revoked`.
- `tests_registry::every_ledger_kind_is_written` keeps them honest.
- The digest and the morning note read these by kind (§5).

### 4.5 Revocation

- `theseus google revoke` posts the token to `oauth2.googleapis.com/revoke`, clears the store (the `op` field
  emptied, or the file deleted) and drops it from the board. State → `needs_auth`.
- **Revoking at Google's side** (myaccount.google.com → Security → Third-party connections) surfaces at the next
  refresh as `invalid_grant` → `needs_auth`, with one Notice.
- **Rotating the client secret** in Cloud Console means re-running `theseus google auth`.


## The design's own words: §6.1 Rows

Nothing is launched. These rows follow the batch5 conventions:
- **Branch:** `cloud/<date>-<slug>`.
- **Model:** Sonnet 5.5 (Sonnet codes; memory `theseus-model-roles-sonnet-codes`).
- **Gate:** the lane gate, `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`.
- **Key test:** the first one listed for each row, proven against a planted revert.
- **Brief:** each row's brief carries the no-names rule and links this design.

| Order | Slug | Scope | Main files | Key test first, then the others | Size | Prereq |
|---|---|---|---|---|---|---|
| 1 | `gw-core` | Includes all the shared pieces, so rows 2-4 run in parallel after it: the `theseus-google` crate (token refresh single-flight, PKCE + loopback listener + paste parser, REST client, retry and backoff, error map, method classifier); `[google]` config + validation + template; crown-jewel names; startup `google.check` + health state; RPC `google.auth.begin/finish`, `google.revoke`, `google.status` (CLI surface only); CLI `theseus google auth\|status\|revoke`; the service → scope map (§2.4) with per-tool scope sets; the API-disabled error mapping; token minting (`op item create/edit` from stdin with a read-back, `file:` 0600, `env:` refused, an in-memory hold when the write is refused); the `private` marker + `data::screen` check (or hand-off to a1wvq.9); the External helper with the owner-authored rule; `google.called` and auth facts; the `docs` family; `google.call`. | `crates/theseus-google/**`, `theseus-core/src/config/google.rs`, `config.rs`, `src/google/{mod,auth,call}.rs`, `fact/google.rs`, `tools_defer/family.rs`, `theseusd/src/main.rs`, `theseus/src/{main,cmd}.rs`, `theseus.example.toml` | **`a_refresh_token_for_another_account_is_refused_and_not_stored`**, `pkce_state_mismatch_is_refused`, `a_pasted_redirect_url_finishes_auth`, `the_refresh_token_never_reaches_op_argv` (a fake `op` records its argv and stdin), `a_minted_token_is_read_back_before_success`, `a_refused_op_write_holds_the_token_in_memory_and_says_so`, `requested_scopes_are_exactly_the_declared_set`, `a_disabled_api_names_the_api_to_enable`, `partial_grant_turns_off_only_the_tools_missing_a_scope`, `invalid_grant_sets_needs_auth_and_one_notice`, `refresh_is_single_flight`, `google_call_classifies_send_delete_permissions_as_authority`, `a_google_result_is_never_open`, `owner_authored_unshared_content_is_not_external`, `env_ref_refresh_token_is_refused`, `google_secrets_never_reach_l1`, `config_rejects_unknown_google_keys` | L, 4 h | main |
| 2 | `gw-mail` | gmail.search, read, draft, send, label. MIME build (RFC 2822 with RFC 2047 headers, threading), HTML → text with quoted-tail folding, attachments to the workspace, send bound to the draft digest, daily cap, the draft hold exemption, the `mail` family, template policy lines, census rows. | `src/google/mail.rs`, `src/google/mime.rs`, `external.rs` (exemption), `theseus.example.toml`, `tests_census.rs` | **`a_send_is_refused_when_the_draft_changed_after_the_plan`**, `send_is_non_repeatable_and_reports_outcome_unknown`, `the_daily_send_cap_counts_google_call_sends`, `a_reply_draft_threads_with_in_reply_to_and_references`, `a_stranger_thread_holds_the_session`, `a_draft_runs_under_the_hold_and_a_label_asks`, `html_mail_reads_as_text`, `send_card_flags_new_recipients` | L, 4 h | gw-core join |
| 3 | `gw-drive` | drive.find, read, write, share. Docs as markdown export and import, Slides text read, Sheet preview, binary save, `fullText` search, the trash and share authority. | `src/google/drive.rs` | **`share_and_trash_always_ask_whatever_the_posture`**, `markdown_round_trips_through_a_doc`, `a_shared_owned_file_is_external`, `find_compiles_typed_fields_to_drive_q`, `a_large_binary_needs_save_to` | M, 3 h | gw-core join |
| 4 | `gw-sheets-slides` | sheets.read, sheets.write (update, append, clear, create) and slides.write (simple ops, raw `requests`, create), plus the Sheets-quota backoff. | `src/google/{sheets,slides}.rs` | **`sheets_429_backs_off_and_succeeds`**, `user_entered_formulas_evaluate`, `append_inserts_rows`, `a_deck_is_created_with_two_slides`, `raw_requests_pass_through` | M, 3 h | gw-core join |
| 5 | `gw-calendar` | calendar.events and calendar.put, with invitations and cancellations under authority; the `calendar` family. **Only if the owner says yes to Q2.** | `src/google/calendar.rs` | **`inviting_attendees_asks_whatever_the_posture`**, `an_event_the_account_organizes_is_not_external`, `calendar_acl_and_calendar_delete_are_authority`, `an_all_day_event_reads_in_local_time` | S, 2 h | gw-core join |
| 6 | `gw-surfaces` | `Check::Mail` (history polling, trusted-sender Task, deadline follow-up), the morning-note lines (events, unread, drafts waiting), the digest kinds, the "waiting on a reply" board reason, and the send card preview in Discord and the cockpit. | `src/watch/{spec,mail}.rs`, `initiative/{morning,digest}.rs`, `rpc/work_board.rs`, `theseus-discord/src/render/preview.rs`, cockpit card | **`a_strangers_mail_never_starts_a_turn`**, `a_trusted_senders_mail_starts_a_held_task`, `a_reply_deadline_fires_the_follow_up`, `history_poll_resumes_from_the_last_history_id`, `morning_note_lists_todays_events`, `digest_counts_sent_mail` | M, 3 h | gw-mail join (+ gw-calendar join if built) |

Wall-clock if launched: row 1 (4 h) → rows 2-5 in parallel (4 h) → row 6 (3 h). That is about 11 h plus joins.

**The fake.** Every row tests against a loopback fake Google (`[google] endpoint`): token endpoint, Gmail, Drive,
Sheets, Slides and Calendar stubs, built in row 1 under `crates/theseus-google/src/fake.rs`. That is the same idea as
the AWS loopback stand-in. No row touches a real account.

## Where main is

The preamble above may describe an older `main` (its store format and its list of changes in flight are from an earlier
batch). Where the preamble and this task disagree about `main`, this task is right: `main` is at `bbc65530` (2026-10-18
21:32 MST, the v1 cut's last join fix) or later, with **store format 45**. The Google Workspace design quoted below was
written against `c7ad21f7`, so every `file:line` in it is from then; on your base a line may have moved: search by name.

Line ceilings at `bbc65530` (`scripts/long-files.txt`, length / ceiling): `crates/theseus-protocol/src/lib.rs` 2,706 /
2,733, `crates/theseus-discord/src/runtime.rs` 3,497 / 3,500 (full: never add a line there), `crates/theseus-core/src/turn.rs`
3,384 / 3,525, `crates/theseus/src/render.rs` 3,108 / 3,128, `crates/theseus-discord/src/render.rs` 2,963 / 3,009,
`crates/theseus-core/src/tests_m3.rs` 7,973 / 8,050, `crates/theseus-kernel/src/kernel.rs` 2,993 / 3,030. Files near
2,500 lines that are not listed yet: `crates/theseus-core/src/toolrun.rs` 2,489 (one registration call there, no more:
a file that passes 2,500 needs a ceiling entry, which this batch may not add). Never raise a ceiling in this batch. Put
new code in new modules; in a long file add only the `mod` line, the field or the call.

## v1 patch1: the night's cloud rows

Theseus's v1 is cut. v1 patch1 is the first fix release after it. The owner's words (Discord DM, 2026-10-19 00:21,
verbatim excerpt): "Yes to all. Looks wonderful. ... Please feel free to aggressively use cloud lanes ... to get this
work across the finish line quickly." "Yes to all" answered, among others: the Calendar build step is in, and the speed
regressions are fixed overnight with the benchmark measurement in the morning on a quiet host. Earlier (2026-10-18
12:18): "Make sure we set up Google workspace as a primary task for v1, patch1."

| Row | Slug | Beads | Prerequisite | Area |
|---|---|---|---|---|
| G1 | `gw-core` | theseus-batvg.1 | main | `crates/theseus-google/` (new), `theseus-core/src/google/{mod,auth,call,external,private,send_cap}.rs` (new), `config/google.rs` (new), `fact/google.rs` (new), `tools_defer/family.rs`, `toolrun.rs` (one call), `broker.rs` (the crown jewels), `data_gate.rs` and the route's data step (one predicate each), `rpc/google.rs` (new), `theseusd/src/main.rs` (the `google.check` phase), `theseus/src/{main,cmd}.rs` + a `google` CLI module, `theseus-protocol/src/google.rs` (new), the template, `docs/privacy.md` |
| G2 | `gw-mail` | theseus-batvg.2 | G1 joined | `src/google/{mail,mime}.rs` (new), `external.rs` (the draft exemption), its fake stubs, its template lines, census rows |
| G3 | `gw-drive` | theseus-batvg.3 | G1 joined | `src/google/drive.rs` (new), its fake stubs, its template lines |
| G4 | `gw-sheets-slides` | theseus-batvg.4 | G1 joined | `src/google/{sheets,slides}.rs` (new), its fake stubs, its template lines |
| G5 | `gw-calendar` | theseus-batvg.5 | G1 joined | `src/google/calendar.rs` (new), its fake stubs, its template lines |
| G6 | `gw-surfaces` | theseus-batvg.6 | G2 and G5 joined | `src/watch/{spec,mail}.rs`, `initiative/{morning,digest}.rs`, `rpc/work_board.rs`, `theseus-discord/src/render/preview.rs`, the cockpit's approval card |
| S1 | `speed-first-request` | theseus-nsp9l.1 | main | `config.rs` (`context_paths`), `census.rs`, `first_request.rs`, `tests_first_request.rs`, `fixtures/owner_shape.toml` |
| S2 | `speed-send-path` | theseus-nsp9l.2 | main | the turn's path before the request leaves (found by profiling; see the row) |
| S3 | `speed-idle-size` | theseus-nsp9l.3 | main | what an idle daemon keeps resident, and what grew `theseusd` (found by measuring; see the row) |

G2 to G5 run in parallel after G1's join; G6 after G2's and G5's. S1 to S3 run beside them. Joins are plain merges, one
at a time.

**Other work in flight on the owner's machine tonight** (local lanes; stay out of their areas): the data-routing lane
(an allowlist of model providers by origin for private data, failing closed; then routing hard tasks; then Jev's
warnings on prompts that draw out personal details: `routing/data.rs`, `data_gate.rs`, `catalog.rs`'s origin, the
route's data step, `config/routing_data.rs`, `config/privacy.rs`); a Discord test hang and the installer's timer PATH;
the import sync of the owner's decisions; the README; a voice fix.

**Contracts between the rows** (so each join is mechanical):
1. **Stay in your area.** If the change you want sits in another row's area, leave it and name it in the report. Do
   not refactor code you do not need, and do not reformat files you only read.
2. **G1 owns every shared Google piece.** The crate `theseus-google` (OAuth client, token refresh, PKCE, the loopback
   listener and the paste parser, the REST client, retry and backoff, the error map, the method classifier), the
   `Google` handle in `theseus-core/src/google/mod.rs`, the scope map with **every** tool's accepted scopes (the
   tools G2 to G5 build are named in it now: `gmail.search`, `gmail.read`, `gmail.draft`, `gmail.send`, `gmail.label`,
   `calendar.events`, `calendar.put`, `drive.find`, `drive.read`, `drive.write`, `drive.share`, `sheets.read`,
   `sheets.write`, `slides.write`, `google.call`), the families (`mail`, `calendar`, `docs`) for all those names, the
   `google.called` fact and the auth facts, the daily send counter, the External and owner-authored helper, and the
   `private` marker. G2 to G5 **use** them and never redefine them; if one lacks what a row needs, the row adds the
   smallest public accessor in G1's module and says so in its report.
3. **One way in for tools.** G1 registers Google's tools with **one** call in `toolrun.rs` (beside the AWS tools),
   which takes the list from `google::tools(&Google)` in `google/mod.rs`. A later row adds its tools by adding **one
   line** for its own module to that list (keep the list in the order mail, drive, sheets, slides, calendar, with each
   row's line separated from the next by G1's marker comments, so two rows' lines never touch). No later row edits
   `toolrun.rs`.
4. **Every Google call goes through G1's `Google::call`** (or a typed wrapper of it), which writes the `google.called`
   row, retries what may be retried, refuses a repeat of what may not (`Retry::NonRepeatable`), and maps errors. No row
   writes its own fact or its own HTTP client.
5. **Every Google tool result goes through G1's `google::finish`**, which sets the `private` marker and decides External
   with the owner-authored rule from the metadata the row hands it (`From` headers, `organizer.self`, `owners[].me` and
   `shared`). A row never builds a Google `ToolOutput` any other way.
6. **The fake.** G1 builds the loopback fake Google in `crates/theseus-google/src/fake.rs` (the token endpoint, the
   userinfo/`id_token` account, a route table, a request log, and a switch for 429s, 5xx, `invalid_grant` and
   `SERVICE_DISABLED`). Each later row puts its service's stubs in its own file, `fake/<service>.rs`, with **one** `mod`
   line and **one** route-table line in `fake.rs`. No test touches a real Google account.
7. **The template** (`crates/theseus-core/config/theseus.example.toml`): G1 writes the `[google]` section and, under
   `[policy.tools]`, the `google.call` line plus four marker comments, each on its own line with an unchanged line
   between them: `# gw-mail: the gmail.* lines`, `# gw-drive: the drive.* lines`, `# gw-sheets-slides: the sheets.* and
   slides.* lines`, `# gw-calendar: the calendar.* lines`. Each later row replaces only its own marker with its lines.
   If the template test `example_template_lists_every_tool_under_policy_tools` refuses a marker or a line for a tool
   that is not registered yet, follow what it allows and say so.
8. **The protocol:** G1 puts its wire types (the auth begin/finish, revoke and status methods and results) in
   `crates/theseus-protocol/src/google.rs` and adds **one** `pub mod` line to `lib.rs`; G6 adds its types to that
   module, no new line in `lib.rs`. Regenerate `cockpit/src/protocol.gen/` when you change protocol types.
9. **The store format:** G1 bumps it once, 45 → 46, for everything it stores (a session's private-source mark if it is
   a field, the new ledger kinds), with the old layout's literal sample in `tests_layouts.rs`. G2 to G5 store nothing
   new. G6 bumps by one from its base for `Check::Mail`. S1 to S3 store nothing new; if one must, it says why. The
   joiner renumbers.
10. **The data rule:** a Google result reaches the data rule only through G1's mark: once a session has read any Google
    result, the session's data answer is `private` by code, as a sensitive name makes it today
    (`routing::data::DataAnswer::private_by_name`, theseus-oejh3), and the provider floor (`data_gate.rs`) refuses a
    request of that session to a model of restricted origin. G1 adds one condition to each of those two places and no
    origin logic of its own (the data-routing lane is changing what "restricted" means tonight: call its predicate,
    never a model or provider name).
11. **Sends always ask, deterministically** (the owner's decision, which overrides the design): see G2 and G1.
12. **Leave `crates/theseus-discord/` and the cockpit alone** except G6's send card preview. Never add a line to
    `theseus-discord/src/runtime.rs`.

## Out of scope for every row

Any call to a real Google account (the live check is the maintainer's, with the owner, after the joins). The owner's
config, his 1Password items and his daemon. Pub/Sub push, Contacts, Gmail settings (forwarding, filters), permanent
delete, a second account. The `http.fetch` egress predicate for sessions holding private Google content and the
external hold (the design's §4.2 residual risk: a follow-up row). The benchmarks themselves: speed is measured in the
morning on a quiet host, never by a row. `docs/status.md`, the README, `docs/design/` and the spec (the maintainer
writes those; say what should change). If your work shows one of these is needed, write it in the report and stop
there.

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant. Every line you add should earn its place. Prefer one
  right name over a compatibility alias: when you rename, fix every caller (scripts, bench, cockpit, docs in code) and
  make an old config key fail the load with "X was renamed to Y". Stored data stays readable.
- **Nothing without config.** With no `[google]` section the Google code does nothing: no tool is registered, no
  startup phase runs, no file or secret is read, and a plain turn is byte-for-byte what it is on `main` (a test shows
  it). Speed rows change no behaviour a person can see beyond speed and size, unless the row says so.
- No new always-loaded model text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens;
  `tests_first_request.rs` must pass on the defaults and on the owner's shape). Every Google tool is deferred. FAST:
  nothing new on a plain turn (a plain turn stays 5 frames), and nothing on the start path grows with history.
- Every behaviour change gets a test and a planted revert (break it, see the named test fail, restore and `touch`, then
  `git status`); list each plant in the report with the test that failed. A pure move needs no plant: say so.
- **Keel.** Do not delete, ignore or weaken a test, and do not raise a budget, cap or ceiling. Before your last commit
  run `python3 scripts/keel-guard.py --base origin/main --head HEAD --quiet` and paste its output in the report. It
  must be empty unless this task names keel findings.
- No new dependency: Cargo.lock and the package-lock.json files gain no third-party package (a new workspace crate's
  own entry is fine). The workspace already has what this batch needs: `reqwest` 0.12 (rustls), `serde_json`, `sha2`,
  `base64`, `rand`, `url`, `percent-encoding`, `chrono`, `zeroize`, `axum` and `tokio`. HTML to text is
  `theseus-core/src/web/html.rs`'s `to_text`. A store format change only as the shared contracts say (bump by one
  from 45 and add the old layout's literal sample to tests_layouts.rs; the joiner renumbers).
- **Secrets and names.** A token, a client secret or an authorization code is held in memory only (`Zeroizing`, a
  masked `Debug`), never on a command line (`/proc` shows argv), never in a log, a fact, an error message, a report or
  a test's output; a test plants a secret-shaped value and shows it never reaches what is stored or printed. Run
  anything that leaves (a fact, a report, a push) through `core.tools.scrubber`. No person's name anywhere in the repo,
  in commit messages or in the report: write "the owner". Fixture accounts are invented (`owner@example.test`,
  `collaborator@example.test`); never a real address, vault or item name.
- Read the AGENTS.md/CLAUDE.md of every crate you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed,
  what you could not run in the cloud (L1 needs a non-root daemon with a job cgroup: about 33 L1 tests fail on this VM,
  so say which of your tests you could run and give the maintainer the exact commands for the rest).
