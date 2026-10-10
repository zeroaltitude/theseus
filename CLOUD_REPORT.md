# Cloud report: secrets-shapes (theseus-oyrt, with theseus-d80h)

Branch `cloud/20261010-secrets-shapes`, from `main` at 2fd1f654 (store format 26, unchanged: no stored record changed).
Session: 2026-10-10, 20:15 to about 23:20 UTC on the 4-core VM.

| Commit | What |
| --- | --- |
| `abf03b6` | scrub: YAML's escaped line break reads as nothing, so a value PyYAML folds is withheld (theseus-d80h) |
| `61bffd8` | scrub: the new shapes, found as written and inside every encoding, proved by a planted-secret corpus through a daemon, and the cost cut (theseus-oyrt) |

## Step 1: YAML's escaped line break (theseus-d80h), `abf03b6`

**Found.** As the brief says. `escape_at` had no arm for `\` followed by a line break, so the fold stayed in the decoded
text and split the value. I checked what PyYAML 6 prints on this VM. A long double-quoted value folds at a space,
ending the line in `\` with the next line starting `  \ `. When a line's room runs out just past an escape, it also
folds inside a word (`Inv\xE9\` then `  nted-word`). Both are in the test.

**Changed.** In `scrub/escaped.rs`:

- `escape_at` returns `Option<(Option<char>, usize)>`. `\` + `\n` (or `\r\n`) + the next line's spaces and tabs is an
  escape with no character, so its `Escape` has `dec == dec_end`.
- `decode` pushes nothing for it.
- `start` and `end` already map a match at an empty escape correctly. A match that begins there maps to the escape's
  start; one that ends there maps to the byte before it.
- `joins` used to index `decoded[e.dec]`, which panics for an empty escape at the text's end. It now treats a fold
  between base64 characters as a join.
- `is_escape_letter` and `decode` (the callers of `escape_at`) accept a decode of length zero.

**Proved.**

- `scrub/tests_fold.rs` has three tests:
  - the value as PyYAML prints it, also with `\r\n` line ends inside other output;
  - the fold inside a word;
  - backslashes at line ends that fold nothing a value needs.
- `tests_outside_text.rs` gains three fold pieces (`\⏎`, `\⏎\t `, `\⏎  \ `), so the property tests can put an empty
  escape at either end of a text. Both scrubber properties pass at 2,000 cases.
- `cargo nextest run -p theseus-core -E 'test(/scrub::/) | test(/outside_text/)'`: 39 passed.
- Planted revert (the arm off, `b'\n' | b'\r' if false =>`): `a_value_yaml_folds_across_lines_is_withheld` and
  `a_fold_inside_a_word_is_read_through` failed, with the value through whole (`n = 0`). File restored, touched,
  `git status` clean.

## Step 2: the shapes, the encodings, the corpus, the cost (theseus-oyrt), `61bffd8`

### What I found

- Shapes were found only as written. Every encoding pass (base64, percent, the escapes) looked only for board values.
  So a shape in a Kubernetes secret's base64 `data`, a percent-encoded query string or a `\u`-escaped JSON string got
  through, even for the old shapes (an AWS id in base64, for one).
- The kernel's `redact.rs` keeps no list of shapes. It withholds a job's *granted* values from the spool's raw
  `.out` file, and the core removes that file once the result is absorbed (theseus-wz2). So there was no second list
  to merge. One caveat: while a job runs, its raw `.out` holds any unresolved shape as printed, as it always did.
  Teaching the streaming copier the shapes would mean holding back whole lines and blocks; I left it out (see "Left").
- **A defect in code I was in, fixed here:** `base64_runs` joins a line of 40 or more characters to the next line when
  that line starts with a base64 character. That is how an encoding's end gets glued to the next word (a label such as
  `aws_access_key_id`). The value pass is unaffected (it matches needles). The new shape pass would have decoded the
  glued word into text stuck to the shape: an AWS id followed by letters is not an id. Fixed in the new pass by also
  reading each run cut where its line widths change. The corpus's "all in one output" check found it, and plant P7
  below proves the fix.

### What changed

- **`scrub/shapes.rs`** takes the shape finders out of `scrub.rs` unchanged: private keys, JWTs, AWS, and the prefix
  scan.
  - The prefix table is now rows. Each row gives the body's character class, its fewest and most characters, what a
    random body has (both cases, plus a digit) and whether the prefix must stand alone. The seven old rows keep their
    exact old behaviour (`old(...)`).
  - The scan is one pass over the text, with a per-byte table of the rows each byte can start. That is cheap in
    release; at debug's opt-level 0 a byte loop is slower than seven `str::find`s.
- **New shapes**, each replaced by `[redacted:<shape>]`:
  - OpenAI (`openai_key`): `sk-` followed by 32 or more of `[A-Za-z0-9_-]` with both cases and a digit. That covers
    `sk-proj-`, `sk-svcacct-` and `sk-admin-`. It never matches inside a word (`disk-…`) and never `sk-ant-`, which
    stays `anthropic_key` (tested).
  - Stripe (`stripe_key`): `sk_live_`, `sk_test_`, `rk_live_`, `rk_test_` with 16 or more base62 characters. Also
    `whsec_` (`stripe_webhook_secret`).
  - **`pk_` keys are not scrubbed.** Publishable keys are public by design: Stripe has them embedded in every page that
    takes a payment. Withholding them would only hide a non-secret from the model. One is in the clean corpus and is
    left alone.
  - Google (`google_api_key`): `AIza` and exactly 35 characters of `[0-9A-Za-z_-]`, standing alone. Too short or too
    long is left.
  - Connection URL password (`url_password`): `scheme://user:password@host`, for any scheme.
    - The authority ends at the first `/ ? #`, whitespace, quote, `` ` ``, `<`, `>` or backslash. Its last `@` ends
      the user part, so a raw `@` in a password is still read as the password's.
    - Only the password is replaced: `postgres://app:[redacted:url_password]@db:5432/x`.
    - These are left as they are: no password, an empty one, `user@host`, a placeholder (`${…}`, `$X`, `<…>`, `{{…}}`,
      `%(…)s`, all `*`/`x`), and one already withheld.
  - Other prefixes with a distinctive start:
    - GitHub `ghu_`, `ghs_`, `ghr_`
    - Slack `xoxa-`, `xoxr-`, `xoxs-`, `xoxe-`, `xapp-`
    - GitLab `glpat-`, npm `npm_`, PyPI `pypi-AgEI`
    - Hugging Face `hf_`: 34 letters of both cases, so `hf_hub_download` is a name
    - SendGrid `SG.` (22.43), DigitalOcean `dop_v1_`, Shopify `shpat_` and `shpss_`, Groq `gsk_`
- **`scrub/decoded.rs`** is a new pass after the as-written shapes. It finds every shape in the encodings a value is
  looked for in, and maps each match back to the text:
  - **Base64:**
    - Which runs: each run of 20 or more characters that holds a capital. The base64 of any text that holds a shape
      has one, because a quad's first character is a capital for any byte below `h`. A hex hash, a UUID or a
      snake_case name has none, so it is never decoded.
    - How: either alphabet, wrapped or not, decoded once at each of four starts by a lenient engine that keeps a cut
      run's last bits. It is read for text in one pass (printable ASCII and whole UTF-8 characters), and read again cut
      where the line widths change (the glue defect above).
    - What it withholds: a match withholds the run's lines it touches, as a value match does.
  - **Percent-escapes:** escapes of ASCII are decoded, and only the lines an escape is on are read.
  - **JSON's, YAML's (with the fold) and repr's escapes:** decoded once or twice, base64 inside them included, through
    `escaped::found`, which shares `levels`/`back` with the value pass. Only the lines an escape is on are read at
    each level, and a folded break counts as part of the line it folds.
- **`scrub/corpus.rs`** (test-only, std and serde_json only) is the planted-secret corpus.
  - **Planted:** 49 secrets of 22 shapes, the old shapes and the new.
    - Every value is built at run time from split prefixes and a seeded xorshift. A grep of the diff for each prefix
      followed by a long body finds nothing, and the push went through.
    - Each secret appears in 14 encodings:
      - as written;
      - base64 and base64url at offsets 0, 1 and 2;
      - base64 wrapped at 76 columns;
      - percent-encoded in part, and every byte;
      - JSON as serde_json writes it, and every character as `\u`;
      - YAML double-quoted, and folded past the width so the fold lands just past an escape (inside the private key
        body, and inside a URL password that holds `é`).
    - That makes 686 texts. Each one records the pieces of its text that carry the secret: base64 characters that hold
      the secret's bytes alone, split as they lie on lines and folds, each at least 8 characters.
  - **Clean:** code with `sk-` in it, prose naming every prefix, URLs with no password or with placeholders, an SSH
    public key, a PEM public key, a Stripe publishable key, 40 log lines (UUIDs, trace ids, git SHAs, `%2C`), sha256
    and sha512 lines, a git log, a 3,000-byte inline base64 image plus a 6,000-byte one wrapped at 76, pretty JSON with
    nested and escaped strings, and YAML with a fold and a base64 config.
- **`scrub/tests_corpus.rs`:** every planted text is scrubbed alone (no carried piece in the output, `n ≥ 1`, a stand-in
  present), then all in one output. Every false positive in the clean corpus is counted.
- **`scrub/tests_shapes.rs`:** one test per shape family with its look-alikes; `sk-ant-` stays `anthropic_key`; a shape
  inside each encoding (a Kubernetes base64 `DATABASE_URL`, base64url at each offset, every byte percent-encoded, all
  `\u`, a YAML fold inside a URL's password); and the per-MiB timing.
- **theseusd `tests/planted_secrets.rs`** includes the corpus by `#[path]` and runs a real daemon (`common::Served`, a
  stand-in model, file secrets). The model asks for `cat planted.txt` and `cat clean.txt`. The test then greps every
  sink:
  - the requests the stand-in received, byte for byte;
  - every file under the state dir (WAL, redb index, spool: so the ledger's rows and the trace's spans too);
  - the daemon's log;
  - `session.history`, `ledger.tail` and the results.
  - It also asserts that every shape's stand-in reached the model and the store (so the store is greppable text), and
    that the clean corpus arrived exactly as printed. It runs in 0.8 s.
- `tests_outside_text.rs`: the scrubber's property pieces gain `sk-`, `sk_live_`, `AIza`, `glpat-`, `hf_`, `SG.`,
  `://`, `postgres://u:`, `@` and `:`.
- `crates/theseus-core/AGENTS.md`, the secrets invariant: names `shapes.rs`, `decoded.rs`, the fold and the corpus. A
  new shape lands with its corpus row. Test secrets are built from parts.

### How it was proved

- `cargo nextest run -p theseus-core -p theseusd -E 'test(/scrub::/) | test(/outside_text/) | test(no_planted_secret)'`:
  - every scrub, property and planted test passes (34 scrub tests, the 15 outside-text properties, the daemon test);
  - the one failure in that filter's 51 was theseusd's L1 `sandbox::a_host_beyond_the_list_once_approved_is_outside_text`,
    which matched the name filter and is one of the known L1 failures.
- Printed by the tests:
  - `corpus: 22 shapes (49 secrets), 686 planted texts, 126696 bytes, all withheld`
  - `clean corpus: 30110 bytes, 0 false positives`, and 0 replaced in the 1 MiB timing over the clean corpus repeated
  - `planted: 686 texts, 816 pieces looked for in 6 sinks (545855 bytes of state), none found`

**Planted reverts.** Each one scored against `-E 'test(/scrub::/)'`. Every file was restored and touched, with
`git status` clean after each.

| Plant | Caught by |
| --- | --- |
| P1 OpenAI row dropped | `tests_corpus::every_planted…`, `tests_shapes::openai_keys…` |
| P2 Stripe rows dropped | `every_planted…`, `a_shape_is_withheld_inside_an_encoding`, `stripe_secret…` |
| P3 Google row dropped | `every_planted…`, `google_api_keys…` |
| P4 `url_passwords` off | `every_planted…`, `a_connection_urls_password…`, `a_shape_is_withheld_inside_an_encoding` |
| P5 the other 17 new prefixes dropped | `every_planted…`, `the_other_prefixed_tokens…` |
| P6 the decoded pass off | `every_planted…`, `a_shape_is_withheld_inside_an_encoding`; and theseusd's `no_planted_secret_reaches_any_sink`: 2,781 leak findings (681 each in the model's requests, the state dir and the history, 738 in the results) |
| P7 the cut at width changes off | `every_planted…` (the all-in-one output: an AWS id in base64 glued to the next label) |

**The live check on this VM** (`/tmp/claude-0/live/run.sh`, not committed, restated for the maintainer below).

- A scratch daemon from a sparse config (`[model] api_base` set to a `theseus-sim fake-model --rules` stand-in, file
  secrets, Discord, web and the index tender off, `enforcement = "notify"`) and a `.env` holding:
  - one random invented secret each: OpenAI project and legacy keys, Stripe `sk_live_` and `rk_test_`, `whsec_`,
    Google, GitLab, npm, Hugging Face, Slack `xapp-`;
  - `DATABASE_URL=postgres://app:<pw>@db.internal:5432/orders`.
- `theseus ask "show me the env"` ran `proc.run [cat .env]`.
- `theseus history` shows the result as
  `OPENAI_API_KEY=[redacted:openai_key] ⏎ OPENAI_LEGACY=[redacted:openai_key] ⏎ STRIPE_SECRET_KEY=[redacted:stripe_key] …`.
- A grep of the state dir (WAL, spool, index), the log and the history found **0 of 11** values.
- The WAL holds the stand-ins (`openai_key` ×2, `stripe_key` ×2, `url_password`, `stripe_webhook_secret`,
  `slack_token`, `npm_token`, `huggingface_token`, `google_api_key`, `gitlab_token`).

**Load.** The brief asks for no runs under load for this row, and I ran none.

### The cost

I timed the same texts before (a worktree at `abf03b6`, the scrubber as `main` has it plus the fold) and after:
1 MiB of the clean corpus repeated (about half of it base64 images, which is the stress case for the new pass) and its
first 64 KB. Each scrub ran with one board value, as a mean of 5 and 100 runs.

| Build | Before (ms/MiB) | After (ms/MiB) |
| --- | --- | --- |
| theseus-core at opt-level 3 (`--config 'profile.dev.package.theseus-core.opt-level=3'`), 1 MiB | 35.2 (another run: 29.1) | 56.7 |
| the same, 64 KB | 29.1 | 62.9 |
| debug (opt-level 0), 1 MiB | 118.5 | 296 (the committed timing test, one scrub) |

**Where it went.** The first working version cost 1,242 ms/MiB in debug and 103 at opt-level 3. Cuts:

- read only the lines an escape or a percent-escape is on, at each level (the escape levels had re-read the whole text
  twice);
- decode a base64 run only if it holds a capital;
- decode each run once per start and split at the cuts, instead of decoding again at every cut;
- scan for text in one pass, without `from_utf8` retries over binary;
- use a per-byte row table for the prefix scan.

At opt-level 3 the 1 MiB scrub now splits into: the decoded pass 28.5 ms, the as-written shapes 11.4 (AWS 5.2,
prefixes 5.3), the run scan 3.8 (it runs twice, once for values and once for shapes), and the rest the value passes.

**Allocation.** `base64_runs` used to build a `Vec` for every candidate word of ten or more letters (most of them
discarded), and `base64_spans` copied every run into a fresh `String`. Now one flat `Runs` holds every run's lines, a
one-line run is matched where it lies, and the multi-line buffer is reused. The decoded pass reuses its buffers too. I
did not measure allocation on its own; the numbers above are totals.

**Net.** About 1.6 to 2x a scrub's time at opt-level 3 on this base64-heavy text. That is the price of reading shapes in
four encodings. Plain text with no long capitalized base64, no `%XX` and no backslash pays only the as-written shapes
and one run scan.

**The tool-call turn bench is unmoved.** `theseus-sim bench lifecycle --runs 10 --check` passed: `LIFECYCLE OK in 52.4 s`.

- cold start p95 32.2 ms (budget 50)
- clean shutdown p95 29.8 (budget 100)
- SIGKILL then restart p95 30.8 (budget 150)
- swap p95 31.7 (budget 200)
- cancel p95 13.7 (budget 100)

The gate's `bench turn` held its frames: plain 5 (budget 5), tool 9 (budget 9).

## The live check for the maintainer

From a release-thin or debug build, in a scratch dir. Never the operator's daemon or socket.

```bash
L=$(mktemp -d); B=target/debug   # or ~/.local/bin after install
mkdir -p "$L/projects" "$L/bin"
python3 - "$L" <<'PY'
import random, string, sys
L = sys.argv[1]; r = random.Random(); a = string.ascii_letters + string.digits
g = lambda n, al=a: "".join(r.choice(al) for _ in range(n))
v = {"OPENAI_API_KEY": "s"+"k-proj-"+g(100), "STRIPE_SECRET_KEY": "sk"+"_live_"+g(99),
     "GOOGLE_API_KEY": "AI"+"za"+g(35, a+"_-"), "GITLAB_TOKEN": "gl"+"pat-"+g(20),
     "HF_TOKEN": "h"+"f_"+g(34, string.ascii_letters), "SLACK_APP_TOKEN": "xa"+"pp-1-"+g(60)}
pw = g(20)
open(L+"/projects/.env","w").write("".join(f"{k}={x}\n" for k,x in v.items()) + f"DATABASE_URL=postgres://app:{pw}@db:5432/x\n")
open(L+"/secrets.txt","w").write("\n".join(list(v.values())+[pw])+"\n")
PY
printf 'tv-invented' > "$L/key"; chmod 600 "$L/key"; printf '#!/bin/sh\nexit 1\n' > "$L/bin/op"; chmod 755 "$L/bin/op"
echo '[{"when":"show me the env","calls":[{"name":"proc_run","input":{"argv":["cat",".env"]}}]},{"when":"","text":"Done."}]' > "$L/rules.json"
cat > "$L/config.toml" <<T
[model]
api_base = "http://127.0.0.1:9461"
[secrets]
anthropic_api_key = "file:$L/key"
zai_api_key = "file:$L/key"
jev_api_key = "file:$L/key"
aws_access_key_id = "file:$L/key"
aws_secret_access_key = "file:$L/key"
discord_bot_token = "file:$L/key"
brave_api_key = "file:$L/key"
[discord]
enabled = false
[web]
enabled = false
[index]
enabled = false
[tools]
projects_dir = "$L/projects"
[policy]
enforcement = "notify"
T
$B/theseus-sim fake-model --addr 127.0.0.1:9461 --rules "$L/rules.json" & F=$!
PATH="$L/bin:$PATH" $B/theseusd --config "$L/config.toml" --state-dir "$L/state" --socket "$L/sock" 2>"$L/d.log" & D=$!
sleep 1; $B/theseus --socket "$L/sock" ask "show me the env"
$B/theseus --socket "$L/sock" history | tee "$L/h.txt" | grep redacted
$B/theseus --socket "$L/sock" shutdown; kill $F
while read -r s; do grep -rqF -- "$s" "$L/state" "$L/d.log" "$L/h.txt" && echo "FOUND ${s:0:6}"; done < "$L/secrets.txt"; echo checked
```

It should show:

- the reply `Done.`;
- a history line `OPENAI_API_KEY=[redacted:openai_key] ⏎ STRIPE_SECRET_KEY=[redacted:stripe_key] ⏎ GOOGLE_API_KEY=[redacted:google_api_key] …`,
  with `DATABASE_URL=postgres://app:[redacted:url_password]@db:5432/x` (the history cuts long results; the WAL holds
  the whole);
- no `FOUND` line before `checked`.

**Claude Code (a claim to check, not a run).** I believe Claude Code would show the same `.env` unredacted. Its `Read`
tool and a `Bash` `cat .env` both return the file's bytes to the model as they are: it has permission rules that can
refuse the read, but nothing that rewrites output. Check by asking it to `cat` a file holding an invented key of one of
these shapes and reading the transcript.

## Left, uncertain, and for the owner

**Shapes that remain unknown**, because a shape with no prefix can't be found without false positives:

- Twilio: the auth token and the API key's secret are 32 hex or base62 characters with no prefix. `AC…` and `SK…` are
  SIDs, which are identifiers, not secrets.
- Azure storage account keys: 88 characters of base64 with no prefix.
- Datadog API and app keys: 32 and 40 hex characters.
- Mailgun's old `key-` and 32 hex: `key-` is too common a word start to scan for.
- Heroku API keys: UUIDs.
- Discord bot tokens and Telegram bot tokens have a structure but no fixed prefix. They could be found by structure,
  with some false-positive risk.
- Any secret after an ordinary label in a `.env` (`DB_PASSWORD=…`, `SECRET_KEY=…`).

**Design choice for the owner: a labeled rule.** The AWS `labeled` finder's idea could extend to `*_PASSWORD`,
`*_SECRET`, `*_TOKEN` and `*_API_KEY` names followed by a value. That would close most of the `.env` gap that
`proc-env` worries about, at a false-positive cost on placeholders and test values. I didn't build it, because it is a
policy call.

**Other things left:**

- The spool's raw `.out` file holds unresolved shapes while a job runs; it is removed once the result is absorbed. The
  kernel's streaming `Redactor` knows only granted values. Giving it shapes means holding back up to a line, or a whole
  private-key block, across reads. Worth a row if the owner counts the transient file as a sink.
- The base64 shape pass reads runs of 20 or more characters that hold a capital. A shape's base64 shorter than 20
  characters (15 bytes) is not read; every shape here is longer.
- Percent-escapes past ASCII are kept as they are, so a shape whose body is percent-encoded UTF-8 is not decoded. No
  shape's body holds UTF-8; a URL password can, and its `%C3%A9` stays inside the password the rule withholds.
- Cost: about 1.6 to 2x at opt-level 3 on base64-heavy output (numbers above). If the owner wants it lower, the next
  cuts are the two `base64_runs` scans per scrub (values, then shapes) and decoding only one start when a run is
  preceded by a non-base64 character.
- The kernel's redaction keeps no shape list, so there was nothing to unify.

**Docs the maintainer may want to update** (I didn't edit them):

- spec §3.10, `docs/spec/04-part1-s3.10.md` around line 552, names the shapes ("AWS access key ids …, private-key
  blocks …, and JWTs"). Add OpenAI, Stripe, Google, the other prefixes and a URL's password, and say that shapes are
  found in base64, percent and escaped text too, and that YAML folds are read through.
- `docs/status.md`: the recently landed step and the R4 row.
- Part III: the item.

**Keel findings expected:** none. `THESEUS_KEEL_BASE=2fd1f65 python3 scripts/keel-guard.py` →
`keel: ok (2fd1f65..the working tree, 2fd1f65413; 12 files changed, 0 findings acked)`. This VM's local `main` is a
stale a9ad950, and against it the guard reports five findings in other people's commits. So I gave the gate
`THESEUS_KEEL_BASE=2fd1f65`, the base the brief names.

## The gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 THESEUS_KEEL_BASE=2fd1f65 CARGO_INCREMENTAL=0 scripts/gate.sh`, before each
commit:

| Commit | Earlier phases | Suite | Later phases, run by hand |
| --- | --- | --- | --- |
| `abf03b6` | keel, fmt, shape, features, clippy, cockpit, test build: ok | 3,652 passed, 33 failed, 44 skipped | protocol types ok, `bench turn` ok (5 and 9 frames), `cargo deny --offline check` ok |
| `61bffd8` | the same: ok | 3,662 passed, 33 failed, 44 skipped | the same, all ok |

The 33 failures are all the known L1-as-root ones (theseus-pv6i), the same in both runs:

- 20 in `theseus-sandbox::contract`;
- `theseus-sandbox::bench spawn_100`;
- 12 in `theseusd::sandbox`.

No timing test from the flaky list failed. The `CLOUD_REPORT.md` commit changes only this file.

**On this VM's disk.** `CARGO_INCREMENTAL=0` is there because mixing incremental and non-incremental builds left two
sets of test binaries in `target/` (19.5 GB in 176 executables). The first gate run died on ENOSPC, and I cleared the
stale set before running again.
