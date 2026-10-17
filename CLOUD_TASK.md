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
- Commit and push each sub-step as soon as it is proved; don't hold commits for the end. Push only your branch, `git push origin HEAD:refs/heads/cloud/20261017-cn-catalog`, at every green point, so nothing is lost if the session ends. Never push to main, never force-push, never delete a branch, and open no pull request.
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
## Your task: cn-catalog, model origin in the catalog (endpoint-aware), DeepSeek's rows, GLM and Sonnet price fixes (theseus-a1wvq.1)

Branch: `cloud/20261017-cn-catalog`. Every commit's subject carries `theseus-a1wvq.1`. Deadline for the report: 4 hours after you start.

**The owner's words (Discord, 2026-10-17 09:26, verbatim; two names redacted, because the public repo never carries them):** "DeepSeek is also anthropic messages API compatible. Please add the best flash (as of today, V4.1-Flash-1017) and non flash models to our supported and automatically selected models list. Also let's update jevs selection input. [The owner's company] and any private or personal or confidential information never goes to Chinese models. General questions and [an open project of the owner's], open source, openclaw, Theseus, etc can go to China. In addition, we need to carefully evaluate the glm and DeepSeek and give a more nuanced coding guidance that picks them where appropriate." Decisions taken since (the owner's questions Q1-Q4 of design §8): standing orders stay in the open view; no DeepSeek key exists yet, so nothing may need one (its live probe waits for the key); Q2 and Q3 are moot after the update below. Origin is judged by provider endpoint as well as by model. **The owner's update (2026-10-17 09:57, verbatim):** "I think that Chinese models in dms can be evaluated by content rather than by the nature of the channel. My understanding is also that they do have coding skills that are worth applying at their cost -- I'd always want to do final review and fixes with a more competent model." So clearance is per request (a China-origin request carries only the open run and the open view; the DM included), China-origin models only draft code, and a Claude model always reviews and fixes the draft before anything is reported or applied. The sticky session gate and `session.data_clear` are gone, and so are open places, open threads, `theseus --open` and the cockpit toggle.

**Prerequisite:** `main` (`c6f62abf` or later; store format 45). Wave 1: builds now.

The preamble above may describe an older `main` (its store format and its list of changes in flight are from an earlier batch). Where the preamble and this task disagree about `main`, this task is right: `main` is at `c6f62abf` or later, with **store format 45**, route.v5 acting, and GLM wired as provider `zai`. Every `file:line` below is at `c6f62abf`; on your base a line may have moved a little, so search by name.

## Your row (from the plan's table)
| # | Slug | Beads | Wave | Scope | Files | Tests (planted revert) | Depends on | Model | Size |
|---|---|---|---|---|---|---|---|---|---|
| 1 | `cn-catalog` | `theseus-a1wvq.1`, child of `theseus-a1wvq` | 1 | origin on every catalog row and every profile (by endpoint and family too); DeepSeek's two rows; the GLM rows fixed; Sonnet 5.5's cache read; the template's DeepSeek provider and profiles; no gate yet | `catalog.rs`, `config.rs`, `compiler/effort.rs`, `compiler.rs` (only if `Implicit` needs it), `provider.rs` (a test), `config/theseus.example.toml`, `broker.rs` test, `fixtures/owner_shape.toml` | T1, T2, T3, T4, `a_china_endpoint_or_family_is_cn_whatever_the_model_id_says` (revert: drop the host rule), `a_provider_cannot_vouch_for_a_china_endpoint`, `glm_rows_think_implicitly_and_take_three_effort_levels`, `implicit_thinking_sends_no_thinking_field`, `sonnet_5_5_cache_reads_are_ten_cents`, `sse_keep_alive_comments_are_skipped` | `main` | Sonnet 5.5 | L |

## What to build
1. **`ModelOrigin`** exactly as design §2.1, on `CatalogEntry` and `CatalogRow` (`catalog.rs:64-120`, `:351-394`;
   `over` at `:428-494`). `fn claude` (`catalog.rs:503`) sets `Us`; `fn glm` (`catalog.rs:538-571`) and the new
   `fn deepseek` set `Cn`. Bump `BUILTIN_VERSION` (`catalog.rs:45`, today `"2026-10-07.1"`) to `"2026-10-17.1"`.
2. **Origin by endpoint and family as well as by row** (the owner's decision of 2026-10-17; research §4.4).
   `catalog::origin_of(api_base, vouched, model, entry)` returns, first match wins:
   1. `Cn` when the `api_base` host is a China-origin endpoint: `deepseek.com`, `z.ai`, `bigmodel.cn`, or any
      subdomain of them (`api.deepseek.com`, `api.z.ai`, `open.bigmodel.cn`), **whatever the model id says**.
      DeepSeek's Anthropic endpoint serves any `claude*` id with a DeepSeek model (research §1.3), so a profile
      `model = "claude-sonnet-5-5"` on `https://api.deepseek.com/anthropic` is `Cn`.
   2. `Cn` when the model id, lowercased, names a China-origin family on any host: it contains `deepseek`, or its last
      `/`-segment starts with `glm`, or it contains `z-ai` or `zai-org` (OpenRouter `deepseek/...`, `z-ai/glm-5.3`,
      Bedrock, Fireworks, self-hosted weights: research §4.3).
   3. `Cn` when the catalog row says `Cn`.
   4. On Anthropic's first party (`api.anthropic.com`) or a loopback host (`127.0.0.1`, `::1`, `localhost`: test rigs
      and local stand-ins): the catalog row's origin; no row is `Unknown`.
   5. Any other host: `vouched` (the provider's `origin`, below) when set, else `Unknown`. A catalog row's `us` never
      vouches for a host: a `claude-*` id on an unknown host is `Unknown`.
   `Unknown` is restricted. `ProviderConfig` (`config.rs:952-960`) gains `origin: Option<ModelOrigin>`, the
   operator's word for a host Theseus doesn't know (a company proxy to Anthropic, say). `Config::validate`
   (`config.rs:1343`) refuses a provider `origin` other than `cn` on a China-origin host, in words.
   `Config::profile_origin(profile, catalog)` resolves a profile's provider (`ProfileConfig.provider`,
   `config.rs:977`, default `"anthropic"` at `config.rs:1195`; providers from `all_providers`, `config.rs:1809`) and
   calls `origin_of`. Nothing reads origin for routing yet: cn-gate-floor does.
3. **Config rows** (design §2.2): a new model's row without `origin` is `unknown`; `validate` refuses a config row
   that loosens a built-in `cn` row's origin ("catalog.\"glm-5.3\".origin: a China-origin model's origin is not
   configurable"); tightening is accepted; `origin = "china"` fails the load.
4. **`ThinkingMode::Implicit`** (`catalog.rs:50`): "the model thinks without being asked; Theseus sends no `thinking`
   field". GLM always thinks (`thinking.type: "disabled"` fails, research §2.2) and DeepSeek thinks by default (its
   endpoint documents only `enabled`/`disabled`, research §1.2). `ThinkingMode::Always` would send
   `{"type": "adaptive"}` and possibly a beta (`compiler.rs:974-988`), which neither endpoint documents, so the wire
   for GLM and DeepSeek stays as it is today (`_ => None`). Check every `ThinkingMode::` use
   (`git grep -n "ThinkingMode::"`: `compiler.rs:975`, `turn/compaction.rs:800`, `:901`): thinking blocks do come
   back, so anything that reads or keeps them treats `Implicit` as thinking; nothing sends for it.
5. **DeepSeek's rows** (research §1.1-1.4, §7.4), `fn deepseek(...)` beside `fn glm`:

   | Field | `deepseek-flash` | `deepseek-v4-pro` |
   |---|---|---|
   | provider | `"deepseek"` | same |
   | window / max out | 1,000,000 / 384,000 (the docs say "1M" / "384K"; take the smaller reading) | same |
   | input / output / cache read / cache write (5m and 1h) per MTok | 0.30 / 1.20 / 0.006 / 0.30 | 1.32 / 3.96 / 0.044 / 1.32 |
   | caches | true (automatic; `cache_control` is ignored, harmless) | same |
   | thinking / effort | `Implicit` / true (three levels, item 6) | same |
   | vision / pdf / tool_search / refusal fallbacks | true / false / false / false | **false** / false / false / false |
   | source | "api-docs.deepseek.com pricing and Anthropic API pages, 2026-10-17; peak rates (off-peak is half); `deepseek-flash` is a floating alias, served by DeepSeek-V4.1-Flash-1017 since 2026-10-17" | "... served by DeepSeek-V4-Pro-0813" |

   Peak prices are the safe direction (the catalog has no time of day). `bytes_per_token`: no measurement exists, so
   `TokenRates::of` (`catalog.rs:203-211`) keeps Claude's densest rates for `deepseek-` (over-estimates: safe for the
   keel), with a comment naming the measurement owed.
6. **The GLM rows** (research §2.4): thinking `Implicit`; `effort: true`; flash and flashx `cache_write` 0.0 -> 0.15 and
   0.37 (= input; `catalog.rs:660-667`); source "docs.z.ai pricing and model pages, 2026-10-17". Z.ai documents three
   effort levels (low, high, max) and DeepSeek maps the rest itself (research §1.2), so for these rows the wire carries
   only those three: medium -> high, xhigh -> high (DeepSeek's documented mapping). Put that in `output_config`
   (`compiler/effort.rs:87-93`) driven by a catalog fact, not a provider-name match. GLM-5.3 and 5.2 stay text only.
7. **Sonnet 5.5's cache read**: 0.20 -> **0.10** (`catalog.rs:602-608`; `claude-sonnet-5` unchanged). Anthropic
   halved Sonnet 5.5's cache reads on Haiku 5.5's launch day (Anthropic's Haiku 5.5 page, read 2026-10-10: "this
   reduces the cost of Sonnet 5.5 on most agentic tasks by around 20%"). Source line: "Anthropic Models API and
   pricing page, 2026-09-28; cache reads $0.10 since Haiku 5.5's launch (Anthropic's Haiku 5.5 page, 2026-10-10)".
   Commit only the golden lines and test figures this moves.
8. **The template** (`crates/theseus-core/config/theseus.example.toml`): `[providers.deepseek]` with
   `api_base = "https://api.deepseek.com/anthropic"`, `api_key_secret = "deepseek_api_key"` and a comment that the
   operator adds the key first; the commented secret line; profiles `dsflash` (`deepseek-flash`, effort high, "drafts
   routine code; a Claude profile always reviews it"), `dsflashmax` (effort max, "a draft at max effort, for
   deep_coding's draft when the operator turns it on"; **not** a reviewer: under the owner's update a China-origin
   model never reviews) and `dspro` (`deepseek-v4-pro`, effort high, "catalogued; in no mode list"). The broker test
   asserting the template's providers (`broker.rs:776-787`) and `fixtures/owner_shape.toml` move with it. No
   `[routing]` change.
9. **SSE keep-alive**: DeepSeek streams `: keep-alive` comment lines while it waits (research §1.3). `parse_sse_line`
   (`provider.rs:1082-1088`) already ignores any line without `data:`; prove it with a test beside
   `assembles_text_and_usage_from_sse` (`provider.rs:1554`).
10. **No key exists.** The owner has not added a DeepSeek key, so no test may need one and the template's secret line
   stays commented. The maintainer probes the GLM change live (the z.ai key exists) before the join; the DeepSeek
   probe waits for the key.

**Key test (planted revert):** `a_china_endpoint_or_family_is_cn_whatever_the_model_id_says`: drop rule 2.1 and see
`claude-sonnet-5-5` on `https://api.deepseek.com/anthropic` read `Us`. Its cases: that one; `deepseek/deepseek-v4-flash`
on an unknown host; `z-ai/glm-5.3`; `glm-5.3-flash` on `api.z.ai`; `claude-sonnet-5-5` on `api.anthropic.com` (`Us`) and
on `127.0.0.1` (`Us`); `claude-sonnet-5-5` on `proxy.example.com` (`Unknown`) and the same vouched `us` (`Us`); a model
missing from the catalog on `api.anthropic.com` (`Unknown`).

## Shared contracts (all thirteen cn rows; keep the names exactly, so the joins dedupe cleanly)

The China-origin data gate is thirteen rows (Beads `theseus-a1wvq.1` to `.13`), joined to `main` in waves. Each row
owns the names in its line. A later row builds on them and never renames them. A row that needs a name owned by a row
not yet on your base stops and says so in the report, rather than inventing a second one.

**The rule since the owner's update (D1, D2):** clearance is **per request**. A request to a China-origin (or
unknown-origin) model carries only the **open run** (the newest stretch of the transcript in which every node is open,
ending at the newest person message, which was judged `open` at `act`) and the **open view** (no owner material). Its
`DataPass` covers that one request's digest. China-origin models act only as the `trivial` fallback behind Haiku and as
**coding drafts** in open tasks, and a Claude profile always reviews and fixes a draft before it is reported or
applied. There is no sticky session gate, no owner override, and no open place, open thread or `--open`.

| Name | Where | Owner row |
|---|---|---|
| `ModelOrigin { Us, Eu, Cn, Unknown }` (`#[default] Unknown`, serde snake_case, unknown words fail the load); `ModelOrigin::restricted(self) -> bool` (Cn or Unknown) | `crates/theseus-core/src/catalog.rs` | cn-catalog |
| `CatalogEntry.origin` (`#[serde(default)]`), `CatalogRow.origin: Option<ModelOrigin>`, `ThinkingMode::Implicit` | catalog.rs | cn-catalog |
| `catalog::origin_of(api_base: &str, vouched: Option<ModelOrigin>, model: &str, entry: Option<&CatalogEntry>) -> ModelOrigin` | catalog.rs | cn-catalog |
| `ProviderConfig.origin: Option<ModelOrigin>`; `Config::profile_origin(&self, profile: &str, catalog: &Catalog) -> ModelOrigin` | config.rs | cn-catalog |
| `[routing.data]` = `RoutingConfig.data: RoutingData` (`#[serde(default)]`, `deny_unknown_fields`) with `open_topics: Vec<String>`, `private_topics: Vec<String>` | `config/routing.rs` | cn-route-v6 |
| `RoutingData.enabled: bool` (default true), `RoutingData.fallback: Vec<String>` (default `["haiku", "sonnet"]`) | config/routing.rs | cn-gate-floor |
| `RoutingData.open_paths: Vec<String>` (default empty) | config/routing.rs | cn-open-view |
| `routing::DataAnswer { choice: String, confidence: f64, band: theseus_judge::band::Band }`; `Verdict.data: Option<Box<DataAnswer>>` | `routing.rs` | cn-route-v6 |
| `InboundInput.data_names: DataNames { open: Vec<String>, private: Vec<String> }` (skipped when empty) | `theseus-judge/src/builders.rs` | cn-route-v6 |
| `crate::data` (new `crates/theseus-core/src/data.rs`): `DataPass` (private fields and constructor; holds the digest of the **one** request it covers; `covers(&self, digest) -> bool`); `DataWhy` (snake_case words: `disabled`, `unchecked`, `private_view`, `not_in_run`, `unvetted`, `sealed`, `attachment`, `verdict_private`, `verdict_unclear`, `below_act`, `no_verdict`, `late`, `pack_not_live`, `tool_result`); `Clearance` (opaque: only `data.rs` builds a cleared one, bound to one request's digest; `Clearance::closed(DataWhy)`, `is_cleared()`, `why()`, `pass() -> Option<DataPass>`) | data.rs | cn-gate-floor |
| `ProviderRequest.data_pass: Option<DataPass>` (`#[serde(skip)]`); `ProviderError::DataGate { model }` (never retried, never sent); `Reason::DataGate` (`"data_gate"`); the `data_gate.blocked` row | provider.rs, routing.rs | cn-gate-floor |
| `Ask.cleared: bool` (an open request can be built for this ask), `Profile.restricted: bool`, `ModeProfiles.cn_profiles: Vec<String>`, `.cn_after: Option<String>`, `routing::effective(..)` | routing.rs, config/routing.rs | cn-routing-compose |
| `SessionRecord.data: Option<Box<DataState>>`; `DataState { opened: Option<Opened>, run: Option<OpenRun>, taint: Option<Taint> }`; `OpenRun { from: String, through: String }` (node ids); `Opened { how, at_ms }` (`how`: `task` or `draft`); `Taint { why: DataWhy, node: Option<String>, at_ms }` (bookkeeping only, never a gate); ledger kind `session.data` (`opened`, `tainted`) | session.rs, data.rs | cn-session-data |
| `TaskStartParams.open`, task.create's `open` | theseus-protocol `task_start.rs`, task.rs | cn-open-tasks |
| `PlaceView.open`, `PlaceView::content_class()`, recall's `Place::Open` | ceiling.rs, recall | cn-open-view |
| `data::admits(s: Option<&DataState>, c: &Check<'_>) -> Result<OpenRun, DataWhy>`, `data::Check`, `data::OpenRequest`, `data::clearance(run: &OpenRun, r: &OpenRequest) -> Clearance`, `turn/data_step.rs`, the detour's open compile | data.rs, turn/ | cn-open-request |
| `data::screen(..)` | data.rs | cn-tool-screen |
| `ModeProfiles.cn_draft: Option<String>`, `ModeProfiles.final_review: Vec<String>` (TOML key `final`); the draft task (`Opened.how = "draft"`) and its tree `<state>/drafts/<task>` on branch `draft/<task>`; the review as a check task (`check_of`); the `task.draft` row | config/routing.rs, new `draft.rs`, task.rs, check.rs | cn-draft-review |
| `TurnRoute.data: Option<TurnData { verdict, clearance, gate, origin }>`; `route.decided`'s `data`, `data_confidence`, `clearance`, `run_nodes`, `data_gate`, `provider`, `model`, `origin` | theseus-protocol `route.rs`, `fact/route.rs` | cn-observability |

The join order: cn-catalog + cn-route-v6 → cn-gate-floor + cn-routing-compose → cn-session-data → cn-open-tasks +
cn-open-view → cn-open-request → cn-tool-screen + cn-background-gate → cn-draft-review → cn-observability →
cn-config-guidance.

**Other rows in flight today** (the Discord tasks batch, branches `cloud/20261017-discord-*`, `-voice-tasks`,
`-task-offer-chip`): they own `crates/theseus-discord/src/runtime/{threads,start_task,follow,task_cmds}.rs`,
`dc/render/task_view.rs`, `dc/courier.rs`, `theseus-sim/src/fake_discord*`, core `turn/join_step.rs`, and
`classify.v3` (which adds an `EMBEDDED` pack line beside route.v6's). None of them bumps the store format: the gate's
cn-session-data takes 46. `crates/theseus-protocol/src/lib.rs` has 2 lines of room under its ceiling (2,731 of 2,733,
`scripts/long-files.txt`): put new wire types in their module files, and a new method constant on an existing line.

## This feature's rules (in addition to the rules at the end)
- **Fail closed, always.** Anything unknown, missing, late or unparsable is restricted or closed, never open. Never
  loosen a check, a threshold, a test or a ceiling to make a test pass.
- **No names.** The repository is public. Never write a person's name, the owner's employer's name, or the owner's
  private project names in code, tests, fixtures, docs or commit messages: say "the owner" and "the owner's company".
  Organisation and project names live only in the operator's config (`[routing.data] open_topics` /
  `private_topics`, which reach Jev as route.v6's `data_names`). Tests use made-up names such as "Acme" and
  "Widgetco".
- **The lane gate:** `TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` passes on your last commit (run the
  touched crates' tests with `cargo nextest run -p <crate>` first). Say in the report what it printed.
- **A planted revert for your key test** (named in your row): break the line the test guards, see that test fail with
  the expected message, restore. The report names the test, the line, the failure and the restore.
- **The token keel:** the first request stays <= 8,000 tokens for cli/task and <= 15,000 for the DM
  (`crates/theseus-core/src/first_request.rs:24`, `:26`; `tests_first_request.rs`). Jev's route pack is Jev's own
  request, not the turn's. A new field a model sees in a tool schema stays tiny (a description of 12 words at most),
  and the first-request tests pass unchanged.
- **The design's test names** (T1-T30, as the owner's update renames, narrows or retires them) are the names to
  use; your row lists the ones it builds.

## The owner's update (`design-delta-1017.md`, 2026-10-17 09:57; verbatim; it overrides the design)

### Design delta, 2026-10-17 10:0x: content, not channel; China models draft, Claude finishes

The owner's message, 2026-10-17 09:57 (verbatim): "I think that Chinese models in dms can be evaluated by content rather
than by the nature of the channel. My understanding is also that they do have coding skills that are worth applying at
their cost -- I'd always want to do final review and fixes with a more competent model."

This delta **overrides** design.md wherever the two disagree. Everything else in design.md stands:
- the catalog origin, judged by endpoint as well as by row;
- fail closed;
- the chokepoint `DataPass`;
- route.v6's `data` question;
- background calls obey the gate;
- observability.

#### D1. Judge the request, not the place or the whole session

- **Removed:** "the owner's DM session never reaches a China model" (design §0, §5 consequence, T18). Removed too: the
  requirement that a session be *opened* open before any China-origin model may see anything from it.
- **New rule:** a China-origin request may be built in **any** place, the DM included. It may contain only content
  judged open.
  - **Open-run history:** the history it carries is the run of most recent nodes up to the newest person message.
    Every person message in that run was judged `open` at ≥ act. The run ends at the first node that is not open: a
    private or unclear message, a seal, an attachment, an unscreened tool result, or a node written outside the
    blessed writers.
  - **Nothing older:** history before that point is not sent.
  - **Open view:** the prefix is the design's open view (§5): no owner context files, standing facts, preferences,
    people guidance or personal/private recall. Recall comes only from open-labelled sources.
  - **Standing orders stay in the open view** (Q1 default).
- **Clearance is per request.** The `DataPass` is minted for one compiled request whose every included node is
  open, and it covers that request's digest. The pass replaces session-level sticky clearance as the precondition.
- **Sticky session taint stays only as bookkeeping** for open sessions and tasks (R4a), not as a gate on the DM.
- **Consequence:** in the DM, a message judged open can be served by a China-origin model. That model sees only the
  open run and the open view. Private content earlier in the conversation is cut off, never sent.

#### D2. China models draft; a Claude model always does the final review and fixes

- **Where China-origin models act:** coding work only, as **drafts**: routine coding, test writing, mechanical
  refactors, and well-specified feature implementation from an agreed plan.
- **They never give the final answer on code.** Every China-origin coding draft is followed by a **mandatory final
  review-and-fix pass on a Claude profile** before the result is reported or accepted:
  - routine work: `sonnet`;
  - deep work: `sonnethi`, then `opus`.
  
  The reviewer reads the diff, runs the tests, fixes what is wrong, and says what it changed.
- **Mechanism: the task is the unit.** An eligible coding request becomes an **open task**:
  - **The brief.** It is built only from open content, and Jev's `data` question is asked about the brief itself.
    The task runs in the open view on the China-origin profile (`dsflash`).
  - **The report.** It goes to a **review step on Claude**, either a review turn in the parent or a follow-on review
    task in the private view. The owner sees the Claude-reviewed result with one line: "drafted by DeepSeek Flash,
    reviewed and fixed by Sonnet".
  - **Never applied unreviewed.** If the review step cannot run (no usable Claude profile, spend limit), the draft is
    not applied and the owner is asked.
- **Config:** `[routing.modes.<mode>] cn_draft = "dsflash"` plus `final = ["sonnet"]` (exact names are the writer's
  call), on routine_coding and deep_coding. deep_coding's `cn_draft` is **off by default**: unknown-cause debugging
  and design stay on Claude. The owner can turn it on.
- **Never on China-origin models:** sophisticated, review, and the conversational modes (chat, quick, trivial).
  - Haiku is cheaper for short turns.
  - **GLM-5.3 stays out of routing.** It costs more per coding task than Sonnet.
  - GLM-5.3 Flash stays only as the trivial fallback, under D1's per-request rule.

#### D3. Measure it

- Every draft and review records:
  - the draft's model and cost;
  - the reviewer's model and cost;
  - how much the reviewer changed (lines changed, tests fixed).
- The benchmark rerun (theseus-lgakz, then theseus-ji599) gets an arm: China-origin draft plus Claude review,
  against Claude alone, on the night set's routine coding tasks. Report solve rate, total cost and wall time.

#### Tests that change

| Test | Change |
|---|---|
| T18 | **Replaced** by `a_dm_open_message_reaches_dsflash_with_only_the_open_run`. The compiled request has no node from before the last private message and no owner material. |
| `a_private_message_in_the_dm_cuts_the_open_run` | **New.** The next open message's China-origin request starts after it. |
| `a_cn_coding_draft_is_never_reported_without_a_claude_review` | **New.** The review step's absence leaves the draft unapplied and asks the owner. |
| `draft_and_review_costs_are_recorded` | **New.** |
| T8 | **Narrowed** to open sessions' bookkeeping. |
| T17 | **Narrowed** to open sessions' bookkeeping. |

## The design (`design.md` of 2026-10-17, written against `c6f62abf`; excerpts verbatim; implement your row's part of it)
**Precedence:** "What to build" above is the latest decision; then the owner's update (the section above); then the design. Where a design excerpt disagrees with either (a session-level sticky gate, `session.data_clear`, the late grace, open places and threads, "the DM never reaches a China model", `cn_min_tokens`), the design excerpt is superseded.

### design.md §0 (lines 7-40)

## 0. The rule, and the design in one paragraph

The owner's rule (2026-10-17, verbatim): *"[the owner's company] and any private or personal or confidential information never goes
to Chinese models. General questions and [an open project of the owner's], open source, openclaw, Theseus, etc can go to China."* "Chinese
models" means models of Chinese **origin** (GLM from Zhipu/Z.ai, DeepSeek), whoever hosts them.

The design has four layers. Every one of them fails closed.

1. **The model's origin is a catalog fact.** Every model row carries `origin`. A model whose origin is `cn` or
   `unknown` is **restricted**.
2. **A restricted model only ever sees a cleared session.** Clearance is sticky and session-level. A session is
   cleared only when all of the following hold:
   - it was opened as an **open session**, so it is compiled under the **open view**, which carries no owner material
     by construction;
   - every person message in it was judged `open` by Jev at confidence ≥ `act`;
   - nothing unvetted entered its transcript;
   - it is not sealed;
   - its place is not marked private.
3. **Routing never picks a restricted profile for an uncleared session.** This covers every path: the mode lists,
   `cheapest`, the session's own base, a pin, a correction and a detour. When a restricted base is skipped, the turn
   moves to `[routing.data] fallback` and says `data_gate`.
4. **A deterministic backstop at the single HTTP chokepoint** (`Anthropic::stream_message_impl`). It refuses to send
   any request for a restricted model unless the request carries a `DataPass`. Only the clearance check can mint a
   `DataPass`. Every background call (summaries, consolidation, initiative, people, cards, the learning writer,
   replays, keep-warm) therefore obeys the same gate, including any call path nobody remembered.

Jev decides the content question. The structure (open view, place, seal, chain, chokepoint) makes sure Jev is never
the only guard, as `AGENTS.md` requires ("Jev is never the only guard on safety").

**Consequence for the owner's DM, stated plainly.** The DM's own session is compiled under the private view: USER-like
context files, standing facts and preferences, people guidance, and live recall of the owner's personal and work
history. Its content is never sent to a China-origin model. From the DM, China models are reachable only through an
**open thread or open task** whose context starts clean (a brief only), and only while every message in it stays
`open`.

### design.md §1.1 (lines 46-67)

### 1.1 Catalog and providers

- `CatalogEntry` (`crates/theseus-core/src/catalog.rs:64-120`) has a provider, window, prices, `caches`,
  `thinking`, `effort`, `vision`, `pdf`, `tool_search`, `source` and `bytes_per_token`. It has **no origin**.
- `fn glm` (`catalog.rs:538-571`) sets `provider: "zai"`, thinking `None`, no effort, `pdf: false` (Z.ai's
  Anthropic-compatible endpoint takes no document blocks) and no tool search. `Catalog::builtin`
  (`catalog.rs:574-679`) inserts glm-5.3 and glm-5.2 text only, and glm-5.3-flash and flashx. `BUILTIN_VERSION`
  (`catalog.rs:45`) names the table.
- Config rows: `CatalogRow` (`catalog.rs:351-394`) and `over` (`catalog.rs:428-494`). A new model must name provider,
  window, output cap and the four prices. `with_overrides` is at `catalog.rs:682-703`.
- Providers:
  - `ProviderConfig` (`crates/theseus-core/src/config.rs:952-960`) has `api_base`, `api_key_secret`, `kind`
    (only `anthropic_messages`, `config.rs:963-968`) and `timeouts`.
  - `rpc/mod.rs:386-401` builds one `Anthropic` client per provider. It sends `POST {api_base}/v1/messages`
    (`provider.rs:899-908`), and `Provider::name()` is always `"anthropic"` (`provider.rs:1036-1037`).
  - Everything provider-specific comes from the model's catalog row. Thinking is sent only for Always/Adaptive
    (`compiler.rs:974-988`). Effort is sent only when `entry.effort` (`compiler/effort.rs:87-91`). PDFs go as text
    when `pdf` is false (`compiler.rs:1045`, `attach.rs:421`). Breakpoints go only when `caches`
    (`compiler/layers.rs:134`). Thinking blocks from another provider or model are stripped
    (`compiler/render.rs:119-122`).
- **Adding DeepSeek needs no new client.** It needs a `[providers.deepseek]` table, a secret, profiles, and catalog
  rows. In non-test code, `"zai"` is matched only at `catalog.rs:547` and `:568`.

### design.md §2 (lines 182-220)

## 2. Requirement 1: origin in the catalog

### 2.1 The type

The type is added to `catalog.rs` beside `ThinkingMode`:

```rust
/// Where a model was made (theseus-a1wvq): the owner's data rule reads it.
/// A model of `cn` or `unknown` origin is restricted: it sees only a cleared
/// session's request (`crate::data`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModelOrigin { Us, Eu, Cn, #[default] Unknown }

impl ModelOrigin {
    pub fn restricted(self) -> bool { matches!(self, ModelOrigin::Cn | ModelOrigin::Unknown) }
}
```

- `CatalogEntry.origin: ModelOrigin` gets `#[serde(default)]`, which means `unknown`, which means restricted.
- `CatalogRow.origin: Option<ModelOrigin>`.
- It is named `ModelOrigin`, not `Origin`, because `crate::node::Origin` already exists.

### 2.2 The rules

1. **Built-in rows.** `fn claude` sets `Us`, and `fn glm` and the new `fn deepseek` set `Cn`. `BUILTIN_VERSION` is
   bumped.
2. **A config row for a new model** with no `origin` is `unknown`, so it is restricted. This holds whatever its
   provider or host, because a model routed through OpenRouter or a self-hosted endpoint is still of its maker's
   origin. **This is the answer to "unknown origin for a non-Anthropic model": fail closed.** A restricted model is
   usable only in a cleared session. The operator may write `origin = "us"` or `"eu"` for a model they know.
3. **A config row may not loosen a built-in restricted origin.** `Config::validate` refuses `origin` on a built-in
   `cn` row unless it is `cn` again: "catalog.\"glm-5.3\".origin: a China-origin model's origin is not configurable".
   A typo or a copied table can therefore never open the gate. Tightening (`us` → `cn` or `unknown`) is allowed.
4. **The enum denies unknown variants**, so `origin = "china"` fails the load.
5. **Restriction is a function of the model, not the provider**: `restricted(entry: Option<&CatalogEntry>) -> bool`
   is `entry.is_none_or(|e| e.origin.restricted())`. A model missing from the catalog is restricted. It is already
   unusable for routing (`unpriced`, `route_step.rs:282-305`), and the chokepoint (§5.4) blocks it on any host
   except Anthropic's first party, where the request cannot reach a China-origin model.

### design.md §6.1 (lines 566-605)

### 6.1 Provider and profiles (template and live config)

```toml
[providers.deepseek]
api_base = "<DeepSeek's Anthropic-compatible base, from research.md>"
api_key_secret = "deepseek_api_key"
kind = "anthropic_messages"

[secrets]
deepseek_api_key = "op://<vault>/<item>/credential"   # the owner adds the vault item first

[profiles.dsflash]          # DeepSeek's best flash model (the owner named V4.1-Flash-1017)
provider = "deepseek"
model = "<flash id>"

[profiles.ds]               # DeepSeek's best non-flash model
provider = "deepseek"
model = "<id>"
```

The catalog's `fn deepseek(...)` (beside `fn glm`, `catalog.rs:538`) has these values:

| Field | Value |
|---|---|
| `provider` | `"deepseek"` |
| `origin` | `Cn` |
| window, output cap, the four prices | `<price>` from research |
| `cache_write_1h_per_mtok` | `cache_write`, unless research shows TTL-split writes |
| `caches` | per research |
| `thinking` | per research (`None` unless the endpoint documents `thinking`) |
| `effort` | `false` unless documented |
| `pdf` | `false` (no document blocks unless documented) |
| `vision` | per research |
| `tool_search` | `false` |
| `refusal_fallbacks` | `false` |
| `source` | "DeepSeek API docs and pricing page, <date>" |
| `bytes_per_token` | a measured `TokenRates::DEEPSEEK` once probed; until then `TokenRates::default()` and a `TokenRates::of` branch on `"deepseek-"` (`catalog.rs:203-211`) |

The template gains the provider, secret and profiles. The broker test that asserts the template's provider keys
exactly (`broker.rs:776-787`) and `fixtures/owner_shape.toml` move with it.

### design.md §8 (lines 735-744)

## 8. What the owner should confirm (defaults chosen, fail closed)

The design proceeds on each default. A "no" changes one row, not the design.

| | Question | Default |
|---|---|---|
| Q1 | Standing **orders** are kept in the open view, as a shared place keeps them (`standing.rs:278-313`). Facts and preferences are withheld. | Keep. Withholding orders too is a one-line change in R5. |
| Q2 | The owner's `session.data_clear` override (§3.7) | Include (R7a). |
| Q3 | May a shared place be bound `data = "open"` (a public OSS channel)? | Yes, only by the bindings file's explicit word. |
| Q4 | The DeepSeek key | Blocking for R2's live probe only. The vault has items for z.ai and openrouter only. |

### design.md §9 (lines 748-837)

## 9. Requirement 7: tests

Unit tests are pure (`data.rs`, `routing.rs`, `catalog.rs`). Core tests use the fake providers in `tests_route.rs`.
The daemon test runs on a scratch daemon.

**Catalog and config**

- **T1** `every_builtin_row_names_its_origin_and_glm_and_deepseek_are_cn` (`catalog.rs` tests).
- **T2** `a_config_model_without_origin_is_unknown_and_restricted`.
- **T3** `a_config_table_cannot_loosen_a_builtin_cn_origin`: validate error. Tightening is accepted.
- **T4** `deepseek_rows_are_priced_as_research_says`: prices, window, `pdf=false`, `thinking`, `effort` (`<price>`).
- **T5** `routing_data_parses_and_fallback_must_be_unrestricted`; `cn_profiles_parse_per_mode`; the template still
  parses (`example_template_uncommented_still_parses`).
- **T6** `background_profiles_refuse_a_restricted_profile`, one case per key in §6.4.

**Clearance (pure, `data.rs`)**

- **T7** `fail_closed_matrix`, one assertion per `DataWhy`: no state; taint; place private; chain broken; sealed;
  seal pending; seal yes at confirm; attachment; private; unclear; below act; no verdict; pack not live. Only
  open + Act + all structure passes.
- **T8** `taint_is_sticky`: after a private message, a run of open messages never clears; only `data_clear` does.
- **T9** `late_grace_applies_once`: a late open clears the previous message; a late private taints; no late verdict
  taints with `no_verdict`.
- **T10** `an_unknown_writer_breaks_the_chain`: a node written outside the blessed writers (a merge, a channel.read,
  a join steer) leaves the next turn `unvetted` and tainted.
- **T11** `turn_write_back_never_erases_a_stored_taint` (`take_turns_fields`).

**Routing**

- **T12** `a_restricted_profile_runs_only_when_cleared`: pick, cheapest, dear guard, step-down.
- **T13** `cn_profiles_come_first_only_when_cleared_and_obey_the_cap`.
- **T14** `a_restricted_base_moves_to_the_fallback_with_data_gate`: no verdict, late, unsure, shadow, pinned `-P glm`,
  a correction to glm, a hold to glm.
- **T15** `no_usable_fallback_fails_the_turn_and_sends_nothing`: the fake provider counts zero requests.
- **T16** `a_detour_to_a_cn_profile_needs_clearance`.

**Turn (core, fake providers)**

- **T17** `an_open_session_reaches_glm_and_a_private_message_moves_it_back`: two turns, `route.decided` rows, a
  `session.data` taint row.
- **T18** `the_dm_session_never_reaches_a_cn_profile`: a private-view session with every message judged open.
- **T19** `a_private_tool_result_stops_a_cleared_turn`: a scrubber hit, a path outside `open_paths`. The result node
  is written, the next loop is not sent, and the session is tainted.
- **T20** `task_inheritance`:
  - a cleared parent's task is cleared;
  - a tainted parent's task is tainted;
  - an open task from a private parent carries no arrangement and needs its brief judged open;
  - a tainted task's report taints its parent.
- **T21** `fork_copies_data_state_through_the_fork_point`.
- **T22** `open_view_request_carries_no_owner_material`:
  - a private context file, a standing fact and a personal imported episode are all absent from the compiled
    request;
  - a public-labelled episode is recallable;
  - the first request is ≤ 8k tokens (cli/task) and ≤ 15k (thread).
- **T23** `summaries_obey_the_gate`:
  - `session` on a cleared cn turn carries a pass;
  - a named glm `summary_profile` in an uncleared session goes to fallback;
  - consolidation with one uncleared source goes to fallback.

**Chokepoint**

- **T24** `the_provider_refuses_a_restricted_model_without_a_pass`: a fake HTTP server records zero requests; the
  error class is `DataGate`; a `data_gate.blocked` row is written.
- **T25** `data_pass_never_changes_the_request_digest`.

**Pack**

- **T26** `route_v6_is_v5_word_for_word_and_a_data_question` (beside `route_v5_is_v4_…`, `pack.rs:1589`). It also
  covers the description row (`pack.rs:1298-1307`) and the rules row (`pack.rs:1379-1386`).
- **T27** The golden `route.v6.request.json` via `UPDATE_GOLDEN=1` (`builders.rs:1116-1132`). `inbound.state.json` is
  unchanged with empty names; a names fixture backs the v6 golden.
- **T28** `the_pack_text_names_no_organisation`: route.v6.toml contains no name from a deny list kept in the test as
  a hash, so the test itself names nobody.

**Telemetry**

- **T29** `route_decided_records_data_clearance_gate_provider_model_origin`. Plus the `TurnRoute` round trip; the
  footer strings (Discord `routed`, CLI `status_line`); the own-bench report columns.

**Layouts**

- **T30** `SESSION_BEFORE_DATA` sample and round trip (`tests_layouts.rs`).

**Live check, on a scratch daemon of the step's build** (`AGENTS.md`, "A live check on a scratch daemon")

- An open thread with an open message routes `trivial` to glm (the z.ai key exists).
- A private message in it taints, and the next turn runs on haiku.
- The DM session never shows glm.
- Then, with the owner's DeepSeek key: one DeepSeek turn, its served model and token counts read from the provider
  (Item 26's rule).

### design.md §10 (lines 841-872)

## 10. Requirement 8: implementation rows

Each row is sized for one Sonnet 5.5 coder, in a lane or in the cloud. Rows that touch shared files (a protocol type,
a dispatch `match`, the CLI's `main.rs`, `cockpit/src/protocol.ts`) say so and land at a join. Every row runs the lane
gate (`THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`), proves its fix against a planted revert, and keeps the token keel.

| Row | Title | Depends | Files | Acceptance |
|---|---|---|---|---|
| **R1** | Catalog origin | — | `catalog.rs` (`ModelOrigin`, `CatalogEntry.origin`, `CatalogRow.origin`, `restricted`), `config.rs` validate (no loosening a built-in cn row), template's `[catalog]` example | T1-T3; `BUILTIN_VERSION` bumped; every built-in row's origin asserted |
| **R2** | DeepSeek provider plumbing | R1; research.md | `catalog.rs` (`fn deepseek`, rows, `TokenRates::of` branch), template (`[providers.deepseek]`, secret, `dsflash`/`ds`), `broker.rs` test, `fixtures/owner_shape.toml` | T4; template parses; a live probe once the owner's key exists (served model, token counts, PDF/vision/thinking behaviour recorded in the catalog's `source`) |
| **R3** | Fail-closed foundation | R1 | new `data.rs` (`DataPass`, `DataWhy`, `restricted`), `provider.rs` (`data_pass` `#[serde(skip)]`, the check in `stream_message_impl`, `ProviderError::DataGate`), `rpc/mod.rs` (restriction fn at build), `route_step.rs` (`route_profiles` marks restricted `unusable = "data"` **unconditionally**), the base gate + `Reason::DataGate` + `[routing.data] enabled/fallback` parse, `Config::validate` for §6.4's keys | T6, T14 (uncleared ⇒ always), T15, T24, T25. **After R3, no request ever reaches a China-origin model: the safe interim state.** |
| **R4a** | Session data state | R3 | `session.rs` (`data`, write-back merge), `store.rs` format bump, `tests_layouts.rs` sample, `data.rs` (`DataState`, `Taint`), `task.rs` propagation beside `external::hold`, report write (`turn.rs:2150-2158`), fork, new `LedgerKind` `session.data` + inspect line | T11, T20 (structure half), T21, T30 |
| **R4b** | Opening open sessions (join: protocol, CLI `main.rs`, Discord verbs) | R4a | `PlaceCeiling.data` (protocol + bindings parse + health), `ThreadNewParams.open`, `TaskStartParams.open`, `task.create` `open`, CLI `--open`, cockpit toggle (generated TS), Discord `/thread open` | Each opening writes `opened`; `data = "private"` refuses an open session in words; protocol TS regenerated |
| **R5** | The open view | R4a | `ceiling.rs` (`PlaceView.open`, `content_class`), every content reader (`turn.rs:797`, `standing.rs`, `ontology.rs`, `session_block.rs`, `prefix.rs` key, `compiler/situation.rs`), recall `Place::Open` (`theseus-memory/src/recall.rs`, core `recall.rs:1215`), tool offer and refusal with `open_paths` + `proc.run` cwd rule (`places.rs:496`, `:593`, `toolrun/order.rs:81`), `[routing.data] open_paths` | T22; goldens of a private session's request byte-identical (a refactor golden first) |
| **R6** | route.v6 and the data answer | — (parallel with R3-R5) | `packs/route.v6.toml`, `pack.rs` EMBEDDED + tests, golden + names fixture, `builders.rs` `InboundInput.data_names`, `judge/inbound.rs` (`ROUTE_PACK`, `verdict` reads `data`), `routing.rs` `Verdict.data`, `adopt.rs` `WHY_ROUTE_V6`, `[routing.data] open_topics/private_topics`, the named string asserts | T26-T28; `inbound.state.json` unchanged without names |
| **R7a** | Clearance in the turn | R4a, R5, R6 | new `turn/data_step.rs` (compute at `compile_first` and the no-wait path, the late grace via `take_late`, the seal inputs, attachments, write `cleared_through`/`taint`), `call_model` mints the pass, `route_profiles(cleared)`, `session.data_clear` (owner, private place) | T7-T10, T17, T18; T14 now for cleared vs. uncleared |
| **R7b** | Routing composition | R3 (pure; parallel with R4-R7a) | `routing.rs` (`Ask.cleared`, `Profile.restricted`, `runs`), `config/routing.rs` (`ModeProfiles.cn_profiles`, the effective list, `glm` out of the code's `trivial` default), detours | T12, T13, T16; existing routing tests unchanged with `cleared = false` and no cn profiles |
| **R7c** | Tool-result screen | R7a | `data::screen` (scrubber + paths), the loop boundary in the turn (stop before the next loop, taint, the line) | T19 |
| **R8** | Background calls obey the gate | R4a, R7a | `turn/compaction.rs`, `consolidate/run.rs`, `keep_warm.rs`, `own_bench/replay.rs`, `learning/audit.rs`, `initiative/speak.rs` | T23; each call path's blocked case writes a row |
| **R9** | Observability (join: protocol) | R7a, R7b | `fact/route.rs`, `route_step.rs` `record_route`, `theseus-protocol/src/route.rs` `TurnData`, Discord `routed`, CLI `status_line`, `provider.call` origin, metric, `own_bench/sampler.rs` + `report.rs` columns | T29; the theseus-lgakz report reads origin/data per turn |
| **R10** | Config, coding guidance, docs | all; research.md | template `[routing.data]` + `cn_profiles` (research's guidance, commented), a proposed live-config diff for the owner and maintainer (`/etc/theseus/theseus.toml`: `[providers.deepseek]`, profiles, `[routing.data]` names and `open_paths`, `cn_profiles`, `glm` back via `trivial.cn_profiles`), spec Part III item, `docs/status.md`, `crates/theseus-core/AGENTS.md` (the data gate in "Invariants" and "Traps") | Template parses; the live diff reviewed by the owner before install |

**The order:**

```
R1 → R3 → R4a → {R5, R4b} → R7a → {R7c, R8} → R9 → R10
       ↘ R7b ─────────────↗
R6 (any time before R7a)        R2 after R1 (+ research.md; its live probe after the owner's key)
```

R3 is the first row that changes behaviour, and it makes the system safe: China-origin models become unreachable
everywhere, visibly (`data_gate` in the footer and rows). The rest open the door only for cleared sessions.

## The research (`research.md` of 2026-10-17; excerpts verbatim)
Evidence labels: [V] vendor docs, [P] independent, [P-weak] unverified summary, [M] our measurement, [I] inference.

### research.md §0 (lines 16-25)

## 0. Answers in one table

| Question | Answer | Evidence |
|---|---|---|
| Exact API id of "V4.1-Flash-1017" | **`deepseek-flash`**; the docs say it is "served by DeepSeek-V4.1-Flash-1017" (MODEL VERSION column). No dated API id exists; the alias floats. Legacy `deepseek-v4-flash` and `deepseek-v4-flash-vision-exp` still work and route to the same model. | [V] pricing page, first-call page, change log 2026-10-17 |
| Best non-flash DeepSeek model | **`deepseek-v4-pro`** (MODEL VERSION DeepSeek-V4-Pro-0813). Still served at its own price. DeepSeek itself says V4.1-Flash beats it and that it is being phased out until a V4.1-Pro ships. | [V] pricing page; news260910; change log 2026-09-10 |
| Newer GLM than 5.3? | **No.** Latest are GLM-5.3 (2026-08-18) and GLM-5.3-Flash/FlashX (2026-08-26). No glm-5.4. Prices match Theseus's catalog. | [V] docs.z.ai release notes and pricing |
| Do the CN models beat Haiku 5.5 on short turns? | **No.** Haiku 5.5 is cheaper per short turn ($0.10/$0.50) and faster (no forced thinking). | [V] prices, [M] Haiku TTFT 0.78 s |
| Do they match Sonnet 5.5 on hard coding? | **No.** Vendor TB 4.0: 42.3 (deepseek-flash) and 37.9 (GLM-5.3) against 70.6 (Sonnet 5.5). | [V] both vendors' tables |
| Where do they pay off? | Contexts over 100K tokens (Haiku's 5x rate applies there), large mechanical/OSS work, test writing, off-peak batch work, cheap second-opinion review. | [V] prices, [I] |

### research.md §1 (lines 29-125)

## 1. DeepSeek: lineup and API facts

Sources: https://api-docs.deepseek.com/quick_start/pricing/ ; https://api-docs.deepseek.com/news/news (first call) ;
https://api-docs.deepseek.com/updates ; https://api-docs.deepseek.com/news/news260910 ;
https://api-docs.deepseek.com/guides/thinking_mode ; https://api-docs.deepseek.com/guides/kv_cache ;
https://api-docs.deepseek.com/quick_start/rate_limit ; https://api-docs.deepseek.com/guides/vision ;
https://api-docs.deepseek.com/guides/anthropic_api ; https://huggingface.co/deepseek-ai/DeepSeek-V4.1-Flash-1017

### 1.1 The two models [V]

| | `deepseek-flash` | `deepseek-v4-pro` |
|---|---|---|
| Served model | DeepSeek-V4.1-Flash-1017 (released 2026-10-17, "API calling method and pricing remain unchanged") | DeepSeek-V4-Pro-0813 (GA 2026-08-13) |
| Architecture | 552B MoE, "Causal Encoder-Decoder", 8B active on input / 16B on output; native vision | (not restated) |
| Weights | open, MIT (HF `deepseek-ai/DeepSeek-V4.1-Flash-1017`) | open |
| Context / max output | 1M / 384K ("MAXIMUM: 384K") | 1M / 384K |
| Input, cache miss, per MTok | **$0.30 peak / $0.15 off-peak** | **$1.32 / $0.66** |
| Input, cache hit | **$0.006 / $0.003** | **$0.044 / $0.022** |
| Output | **$1.20 / $0.60** | **$3.96 / $1.98** |
| Cache write | no write charge listed: a miss is billed at the miss rate | same |
| Thinking | both modes; **thinking is ON by default, effort high** | same |
| Tool calls, JSON output, Anthropic API, Responses API | yes | yes |
| Vision | **yes** (JPEG/PNG/GIF/WebP; user messages only) | **not supported** |
| Concurrency limit (per account) | 2,500 | 500 |

**Off-peak hours** [V]: off-peak is half the peak rate. Peak = 01:00-04:00 and 06:00-10:00 UTC, Monday-Friday,
excluding Chinese public holidays. In Phoenix time (UTC-7, no DST) peak is **18:00-21:00 and 23:00-03:00, Sunday
evening through Friday 03:00**. The owner's weekday daytime and all weekends are off-peak. [I] Theseus's catalog has
no time-of-day pricing, so pricing at peak over-estimates by at most 2x. That is the safe direction.

**History worth knowing** [V]: on 2026-09-10 DeepSeek announced that all `deepseek-v4-pro` requests would route to
V4.1-Flash from 2026-09-14. The change log then says it reversed this: "we have decided to continue providing API
services for DeepSeek V4 Pro after September 14, 2026, with the billing method remaining unchanged". Today's pricing
page lists V4-Pro-0813 separately at its own price, so it is served. **Floating alias risk** [I]: `deepseek-flash`
changed model today with no id change. Benchmarks and the ledger must record the date, and a behaviour change can
arrive silently.

Search & Fetch server tools: $0.75 per 1,000 requests. They are free through Claude Code / DeepSeek Harness until
2026-11-01 00:00 Beijing time [V].

### 1.2 Thinking and effort [V]

- Anthropic format: `thinking: {type: "enabled"|"disabled"}`, with **`budget_tokens` ignored**. Effort goes in
  `output_config: {effort: "low"|"high"|"max"}`, the only `output_config` field honoured.
- Effort mapping: minimal->low, low->low, **medium->high**, high->high, **xhigh->high**, max->max, ultra->max. So there
  are three real levels: low, high, max.
- Thinking mode ignores temperature (no error is raised). `top_p` behaves differently per model (flash: honoured in
  thinking mode, floor 0.95).
- **With tools, prior turns' reasoning must be passed back in every later request, or the API returns 400.** In
  Anthropic format, `thinking` blocks are "Supported" and `redacted_thinking` is "Not Supported". [I] Theseus already
  replays Claude thinking blocks. Whether DeepSeek's blocks carry a `signature`, and whether Theseus's replay keeps
  them byte-exact, needs checking on the first live call.

### 1.3 Anthropic-compatible endpoint [V]

Base URL: **`https://api.deepseek.com/anthropic`** (auth `x-api-key`, "Fully Supported").

| Field | Status |
|---|---|
| `anthropic-version` header | ignored |
| `anthropic-beta` | ignored, except that `files-api-2025-04-14` is needed for `image` with `source.type = file` |
| `model` | must be a DeepSeek id. **`claude-opus*` -> deepseek-v4-pro; any other `claude*` -> deepseek-flash**; any other unknown name -> 400 |
| `max_tokens`, `stream`, `system`, `stop_sequences` | fully supported |
| `temperature` | supported, 0.0-2.0 (no effect in thinking mode) |
| `thinking` | supported, `budget_tokens` ignored |
| `output_config` | only `effort` |
| `top_k` | **ignored** |
| `top_p` | model-dependent (above) |
| `metadata` | only `user_id` (used for content-safety, KV-cache and scheduling isolation). The docs say not to put private info in it |
| `container`, `mcp_servers`, `service_tier` | ignored |
| `tools[].cache_control`, text/tool_use/tool_result `cache_control` | **ignored** (caching is automatic) |
| `tool_choice` | none / auto / any / tool supported. `disable_parallel_tool_use` ignored |
| `tool_result.is_error` | **ignored** [I: put the failure in the result text] |
| `image` blocks | supported: base64 (jpeg/png/gif/webp), url, file. **User messages only**: images in system or assistant messages -> 400. Up to 1,024 tokens per image, 600 images per request, 48 MiB body |
| `document` blocks | **not supported** |
| `search_result`, `code_execution_tool_result`, `mcp_tool_use/result`, `container_upload` | not supported |
| `server_tool_use`, `web_search_tool_result` | supported |
| `citations` | ignored |

**Not documented** (could not confirm): the Anthropic-format **usage fields**. The OpenAI format reports
`prompt_cache_hit_tokens` / `prompt_cache_miss_tokens`. Whether the Anthropic format fills `cache_read_input_tokens`
and `cache_creation_input_tokens`, and the exact SSE event shapes, are not on the page. [I] The first live call should
log the raw usage object. If `cache_creation_input_tokens` appears, price it at the miss rate, since there is no write
premium.

**Streaming keep-alive** [V]: while waiting, non-streaming responses send empty lines and streaming responses send SSE
comments (`: keep-alive`). The connection closes if inference has not started within 10 minutes. Theseus's SSE parser
must skip comment lines.

### 1.4 Prompt caching [V]

Automatic disk cache, on for everyone, best-effort. A hit needs a **full match of a persisted prefix unit**. Units are
persisted at the end of the user input and the end of the model output of each request, at common prefixes detected
across requests, and at fixed token intervals for long inputs. An append-only agent loop (A+B, then A+B+C) hits. A
mid-prefix edit misses until a common prefix is persisted. Building the cache takes seconds. Unused cache clears in
"a few hours to a few days". No breakpoints are honoured and there is no TTL choice.
[I] Theseus's 5m/1h TTL logic is moot here, and so is any cache-write cost.

### research.md §2 (lines 158-212)

## 2. GLM (Z.ai): lineup and API facts

Sources: https://docs.z.ai/guides/overview/pricing ; https://docs.z.ai/release-notes/new-released ;
https://docs.z.ai/guides/llm/glm-5.3 ; https://docs.z.ai/guides/vlm/glm-5.3-flash ;
https://docs.z.ai/guides/capabilities/thinking-mode ; https://docs.z.ai/guides/capabilities/cache ;
https://docs.z.ai/devpack/notice/api-key-management ; https://docs.z.ai/scenario-example/develop-tools/claude

### 2.1 Models and prices [V]

| Model id | Input | Cached input | Cache storage | Output | Context / max out | Input modalities |
|---|---|---|---|---|---|---|
| `glm-5.3` | $1.40 | $0.26 | limited-time free | $4.40 | 1M / 128K | **text only** |
| `glm-5.2` | $1.40 | $0.26 | free | $4.40 | 1M / 128K | text |
| `glm-5.3-flash` | $0.15 | $0.03 | free | $0.50 | 1M / 128K | text, image, video, file |
| `glm-5.3-flashx` | $0.37 | $0.075 | free | $1.25 | 1M / 128K | same as flash; ~200 tok/s |

GLM-5.3: 753B total / ~40B active MoE (AWS Bedrock page), the same base as 5.2, with gains from post-training only.
GLM-5.3-Flash: 320B total / 18B active, hybrid linear+sparse attention [V].
No off-peak pricing on the pay-as-you-go API. Off-peak discounts apply only to Coding Plan points [V].

### 2.2 Thinking [V]

- **GLM-5.3 and GLM-5.3-Flash always think; `thinking.type: "disabled"` fails** ("the request will fail").
- `reasoning_effort`: low / high / max, **default `max`** (documented on the OpenAI-format endpoint).
- Interleaved thinking (between tool calls) is on by default. "Preserved thinking" keeps prior turns' reasoning: on by
  default on the Coding Plan endpoint, off on the standard endpoint (`clear_thinking: false` turns it on). Returned
  reasoning must match byte for byte.
- **Not documented**: whether the Anthropic endpoint honours `output_config.effort` or `thinking` budgets. [I] If it
  ignores them, every Theseus GLM call (including the `trivial` detour) runs at **max** effort.

### 2.3 Anthropic endpoint and keys [V]

- Base URL `https://api.z.ai/api/anthropic` (the "general endpoint"). Z.ai publishes **no field-by-field
  compatibility table** for it. What we know comes from Theseus's own probes: no document blocks (theseus-c9l6), and
  glm-5.3 answered a PNG with empty text (theseus-9g2, 2026-09-29) [M].
- Caching is implicit/automatic. The OpenAI format reports `usage.prompt_tokens_details.cached_tokens`. Whether the
  Anthropic endpoint honours `cache_control` and reports `cache_creation_input_tokens` is undocumented.
- **Key classification change, effective 2026-10-16**: Standard keys use pay-as-you-go. Keys created before that date
  on an account that ever had a Coding Plan are "Compatible": on `/api/anthropic` they **draw plan quota and stop
  working when the plan expires**. The GLM-5.3 page adds that accounts that ever had a Coding Plan "can currently
  access the model API only through the OpenAI Chat Completion-compatible protocol". [I] Worth checking which kind of
  key the vault's z.ai item is (from the console, not by printing it).
- Rate limits: per account, shown only in the console. Not public.

### 2.4 Theseus's catalog against the docs (crates/theseus-core/src/catalog.rs, `fn glm`, lines 538-668)

| Field | Catalog | Docs | Verdict |
|---|---|---|---|
| prices (5.3, 5.2, flash, flashx) | 1.40/4.40/0.26; 0.15/0.50/0.03; 0.37/1.25/0.075 | same | ok |
| context / max out | 1,000,000/128,000; flash 1,048,576/131,072 | "1M" / "128K" | ok (rounding) |
| vision | 5.3/5.2 false; flash true | 5.3 text-only; flash image/video/file | ok |
| pdf | false | Anthropic endpoint takes no documents (our probe) | ok |
| **thinking / effort** | **`ThinkingMode::None`, `effort: false`** | **always thinks; effort low/high/max, default max** | **wrong: the catalog doesn't know GLM thinks, so Theseus can't lower its effort** |
| cache_write | 5.3/5.2: 1.40 (= input); **flash/flashx: 0.0** | no write premium; a write is billed as uncached input | **inconsistent**: if the endpoint reports `cache_creation_input_tokens`, flash/flashx price them at $0 and under-count. Should be 0.15 / 0.37 |
| source | "OpenClaw model catalog, 2026-09" | docs.z.ai pricing, 2026-10-17 | update the source line |

### research.md §4.4 (lines 320-325)

### 4.4 Implication for the gate [I]

**Judge origin by provider endpoint and model family, never by the model-id string.** A profile with
`model = "claude-sonnet-5-5"` and `api_base = https://api.deepseek.com/anthropic` is silently a DeepSeek model (§1.3).
Fail closed: classify by provider/api_base host. A DeepSeek or GLM family on any host (Bedrock, Fireworks, OpenRouter
`deepseek/*`, `z-ai/*`) counts as China-origin. An unknown host or family counts as not-allowed for gated content.

### research.md §7 (lines 415-498)

## 7. Recommendations

### 7.1 Per mode, when the data gate allows China-origin models

| Mode | Recommended list | China-origin role | Why | Confidence |
|---|---|---|---|---|
| trivial | `["haiku", "glm", cheapest]` (unchanged) | fallback only | Haiku is 2-3x cheaper per short turn and faster. GLM-5.3-Flash forces thinking | high (price arithmetic) |
| quick | `["haiku"]` (unchanged) | none | same as trivial | high |
| chat / other | session's own (Sonnet medium), unchanged | none | a switch cold-starts the cache; Sonnet cache reads are $0.10; quality matters in the owner's main conversation | high |
| sophisticated | `["opus", "fable"]` (unchanged) | none | HLE no-tools 42-44 vs Opus 5 56; no evidence of design/judgment parity | high |
| deep_coding | `["sonnethi", "opus"]` (unchanged) | none live; bench arm only | TB 4.0: 42.3 / 37.9 vs 70.6. Unknown-cause debugging needs exactly what the gap measures | medium-high |
| routine_coding | `["haikuhi", "sonnet"]` now; after a bench, **`["dsflash", "haikuhi", "sonnet"]` for gated-allowed sessions over ~100K context only** | primary for long-context routine work, once a bench confirms it | below 100K Haiku 5.5 high costs the same as deepseek-flash, is measured [M], and has better availability. Above 100K Haiku's 5x rate applies and deepseek-flash is 3-8x cheaper with a 384K output ceiling | medium on cost, low on quality |
| review | `["opushi", "fable"]` primary (unchanged) **plus an optional independent second pass by `dsflashmax`** on allowed diffs (OSS, Theseus) | extra reviewer, never the only one | a different model family catches different bugs; costs about $0.05 a pass. No measurement exists | low |
| compaction / consolidation (not a mode) | `haikuhi`; for gate-clean sessions over 100K, consider `dsflash` off-peak | cost lever | 40-70% cheaper on long compaction prompts, but losing a detail is expensive, so bench first | low |

### 7.2 Coding sub-kinds

| Sub-kind | Mode | Claude default | China-origin option (gate allows) | Verdict |
|---|---|---|---|---|
| Mechanical edits (renames, config, formatting, lockfiles) | routine | Haiku 5.5 high | deepseek-flash high, or glm-5.3-flash | equivalent below 100K (keep Haiku); **deepseek-flash above 100K** |
| Test writing (behaviour already specified) | routine | Haiku high -> Sonnet medium | **deepseek-flash high** | good fit: tests are self-checking, so a weak attempt is caught cheaply. Medium-low confidence |
| Known-cause fixes | routine | Haiku high | deepseek-flash high | acceptable; prefer Haiku below 100K |
| Large mechanical refactor / migration across many files | routine (Opus plans) | Sonnet medium executes | **deepseek-flash high executes Opus's plan** | good fit: 1M context, 384K output, $0.006 cache hits. Keep the plan and final review on Claude |
| Design-level refactor | deep / sophisticated | Opus plans, Sonnet executes | none | stay on Claude |
| Unknown-cause debugging | deep | Sonnet high -> Opus | none | stay on Claude |
| New design / architecture | sophisticated | Opus high (Fable) | none | stay on Claude |
| Review / verification | review | Opus high (Fable) | deepseek-flash max as an extra pass only | never the sole or final reviewer |
| Security-flavoured work (tooling, exploits) | - | Sonnet with refusal fallback | **not recommended** as a way around refusals | an owner policy decision. The data gate decides what may go to a CN model, not whether a refused task should be retried elsewhere |

### 7.3 Profiles to add (proposal)

```toml
[providers.deepseek]
api_base = "https://api.deepseek.com/anthropic"   # key: not yet in the vault (owner to add)

[profiles.dsflash]          # routine work over 100K, test writing, large mechanical refactors
provider = "deepseek"
model = "deepseek-flash"    # floating alias: V4.1-Flash-1017 since 2026-10-17
effort = "high"             # DeepSeek maps medium->high anyway; low/high/max are the only real levels

[profiles.dsflashlo]        # optional cheap detour fallback (thinking off); ranks below haiku
provider = "deepseek"
model = "deepseek-flash"
effort = "low"

[profiles.dsflashmax]       # second-opinion review on allowed diffs
provider = "deepseek"
model = "deepseek-flash"
effort = "max"

[profiles.dspro]            # catalogued per the owner's ask; NOT placed in any mode list
provider = "deepseek"
model = "deepseek-v4-pro"   # 4.4x flash's price, no vision; DeepSeek says flash beats it
effort = "high"
```

**Remove `glm53` from routing** (keep the catalog row). GLM-5.3 costs more per agentic trial than Sonnet 5.5
($0.34-0.47 vs $0.30) and scores below deepseek-flash on every shared vendor row. Keep `glm` (5.3-Flash) as the
trivial fallback and as a cheap grounded-QA model [M].

### 7.4 Catalog entries to add or fix

- `deepseek-flash`: provider `deepseek`; window 1,000,000 (the docs say "1M"; exact token count unpublished); max out
  384K (written "384K"; whether 384,000 or 393,216 is unconfirmed); input 0.30, output 1.20, cache read 0.006, cache
  write 0.30 (= input: no premium); TTL-split writes are moot; thinking: on by default and can be turned off (closest
  to `Adaptive`); effort true with levels low/high/max; vision true (user messages only); pdf false; tool_search false;
  refusal_fallbacks false; caches true (automatic); bytes_per_token **unknown, measure it** (`TokenRates::of` falls
  back to Claude's densest rates, which over-estimates: safe for the token keel); source "api-docs.deepseek.com
  pricing, 2026-10-17 (peak rates)".
- `deepseek-v4-pro`: as above with 1.32 / 3.96 / 0.044 / 1.32; vision false.
- GLM rows: thinking = always (not `None`), effort true (low/high/max, default max); flash/flashx cache_write -> 0.15 /
  0.37; source line -> docs.z.ai 2026-10-17.
- Unrelated but noted again: Sonnet 5.5 cache read 0.20 -> **0.10**.

### 7.5 Checks before relying on any of this

1. **First live DeepSeek call** (after the owner adds a key; gate-allowed content only): log the raw usage object.
   Confirm `cache_read_input_tokens` / `cache_creation_input_tokens` and the SSE event names, check that `: keep-alive`
   comments are tolerated, and check thinking-block round-tripping with tools.
2. **Does Z.ai's Anthropic endpoint honour `output_config.effort`?** If not, every `glm` detour runs at max. The
   ledger's output-token counts on glm calls will show it.
3. **Bench arm** (the guide's §8 arm 7, updated): deepseek-flash high vs Haiku 5.5 high on the 14-task night set
   (~$1-2 off-peak), plus a long-context routine subset over 100K. This decides §7.1's routine_coding change.
4. **Review arm**: add dsflashmax to the guide's planted-bug review arm (§8 #5).

## Rules for this row
- Model roles: you write the code; keep it minimal and elegant (the owner's next step is a trim review).
- No new model tool and no always-loaded text: first-request budgets stay (cli/task 8,000, dm 15,000 tokens; the turn
  bench and the owner-config fixture test must pass). FAST: nothing new on a plain turn (a plain turn stays 5 frames).
- Every behaviour gets a test and a planted revert (break it, see the named test fail, restore); list them in the report.
- No store format change unless unavoidable (then take the next free number and say so). Never loosen a test, budget
  or ceiling. No person's name anywhere in the repo.
- Read the crates' AGENTS.md/CLAUDE.md you touch first. Keep CLOUD_REPORT.md honest: what you ran, what passed, what
  you could not run in the cloud.
