# CLOUD_REPORT: fd-soft-fail (theseus-7vtp)

Branch `cloud/20261010-fd-soft-fail`, from main at 2fd1f654 (store format 26, unchanged: no stored record changed).

## What I found

- `serve_socket` returned on an accept error (`accepted?`), so EMFILE ended the daemon. Fixed.
- Every accept loop the daemon runs:
  1. `serve_socket` (theseusd main.rs), the protocol socket. Returned on error. **Fixed.**
  2. The harness's wrapper-notify socket (core harness.rs). Ignored errors (`if let Ok`), so a full table made it spin at 100% CPU. **Fixed** with the same rule (a defect in code I was in).
  3. The web UI (theseusd web.rs): `OwnUser::accept` wraps axum's `TcpListener` accept, which never returns on error: it logs at error and sleeps 1 s. Left as is (already soft, at most one line a second).
  4. The egress proxy (theseus-sandbox egress.rs): returned (ended the job's proxy) on any accept error but WouldBlock/Interrupted. **Fixed**: EMFILE, ENFILE, ENOBUFS, ENOMEM, ECONNABORTED and EPROTO wait 50 ms and go on; a broken listener still ends it.
  5. Test fakes (theseus-sim, theseus-mcp fake, mcp trace): not daemon loops; untouched.

## What I changed

Commit ea2cbef (code), 4b31d4c (theseusd AGENTS.md).

- `theseus-core/src/conns.rs` (new): `raise_nofile` (soft to hard; if the kernel refuses, halves toward it), `ceiling_for` (`[server] max_connections`; 0 derives limit less 256 reserve, half under 512, capped at 8192, at least 1), `Connections` (one atomic counter: held, refused; `admit` returns a guard that gives the place back on drop; no lock on the accept path), `refusal_line` (one JSON-RPC error frame, id null, code -32007 `LIMIT`, "the daemon holds N connections, its ceiling; close one and retry", data.ceiling), `AcceptErrors` (warn at most once a second with `left_out` count, then 50 ms), `accept_until_ok`, `turn_away` (writes the frame, closes, drains up to 250 ms so the close is a FIN).
- theseusd main.rs: `raise_limits` (info line with before/after/hard/ceiling), the accept loop through `accept_until_ok`, the ceiling check, `serve_client`; a debug build's `THESEUS_TEST_ACCEPT_ERRORS=N` plant.
- Health: `push.connections` (`ConnectionsHealth`: held, ceiling, refused, fd_soft, fd_hard) in theseus-protocol's push.rs (lib.rs gained no line; the one in-place doc edit on `LIMIT`); TS regenerated (`ConnectionsHealth.ts`, `PushStatus.ts`, index). The CLI's `connections:` line is `render/connections.rs` (render.rs: a `mod` line and one call). It is silent for a daemon that sends none, so existing goldens are unchanged, and loud (Bad tag) once any connection was refused.
- Config: `[server] max_connections` (default 0) in config.rs (3 lines; the file is now exactly at its 2,927 ceiling) and in the example template with its comment (the template tests parse it).
- Unit template (install/layout.rs `SERVICE_COMMON`, both units) and the three goldens: `LimitNOFILE=65536`, `TasksMax=49152`. install/tests.rs asserts both lines (same assertion count).
- tests/common/mod.rs: `Served::start_with` (a hook on the daemon's command: env, `pre_exec` limits); `start` calls it.
- `scripts/conn-watchers.py`: the live check (stdlib only).

### The unit's values, for the owner's own unit (stopping point's config row)

Add to the owner's unit or a drop-in: `LimitNOFILE=65536` and `TasksMax=49152`. Why: NOFILE 65536 is far above the 8,192 connection cap plus the store, jobs' pipes and terminals, and far below the hard limit. TasksMax: the daemon's threads and children are under 2,000 (tokio workers, a blocking pool of at most 512, the CPU pool, the store's threads, the tender and its children), and the admission ceiling (8 executions) at a job's cgroup cap of 4,096 pids each (`[tools] job_pids_max`) is 32,768; 49,152 = 32,768 + 16,384 of room, so a fork bomb in one job meets its own cap before the unit's. systemd's default (15% of the kernel's pid max) is the figure this replaces; I could not read a job cap beyond the admission ceiling, so this assumes at most one job per execution at its cap.

## How I proved it

- `cargo nextest run -p theseusd --test connections`: 5 tests pass (0.4 s in all):
  - past a ceiling of 5: extra connections get the -32007 frame then EOF, health on a held connection says held 5 / refused 2, a dropped connection's place is taken again, the daemon lives;
  - born with soft 200 / hard 4,096: health says fd_soft 4,096, ceiling 3,840, and 300 connections all fit;
  - born with 200/200 (no raise possible): ceiling 100, 130 opened: 100 served, 30 refused, the daemon lives;
  - 2,000 connections born with the usual 1,024 soft limit: none refused, health after;
  - 4 planted accept errors (EMFILE): the log has exactly one "accept failed" line, the daemon serves.
- Unit tests in core's conns.rs (6: derived ceiling, admit/refuse/give back, frame, raise, accept errors on the paused clock with 30 errors then Ok, one log a second), the CLI line (1), the egress classifier (1), the install test, and the three golden plans.
- **Planted reverts** (restored and `touch`ed, `git status` clean after each):
  - accept loop dies on an accept error (the plant `exit(1)`s as the old `?` did): `an_accept_error_is_logged_once_and_the_daemon_serves_on` failed ("theseusd exited (exit status: 1)");
  - ceiling unchecked (`admit` never refuses): `past_a_ceiling_…` and `a_daemon_that_cannot_raise_its_limit_refuses_under_it_and_lives` failed;
  - limit not raised: `the_soft_limit_is_raised_to_the_hard_one_…` and `two_thousand_connections_…` failed.
- The gate (`CARGO_INCREMENTAL=0 TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`): keel, keel tests, fmt, shape, features, clippy, cockpit, test build all pass. Suite: 3,695 run, 3,662 passed, 33 failed, all in the known L1 set (theseus-sandbox contract tests and `spawn_100`, theseusd's `tests/sandbox.rs`; this VM is root, theseus-pv6i). Nothing else failed in the final run. Phases after the suite I ran by hand: protocol types (clean), `cargo deny --offline check` (advisories, bans, licenses, sources ok), lifecycle bench (below). The jobs and turn benches are skipped by NO_BENCH (owner's machine).
- An earlier gate run also failed `theseusd::gone_jobs::a_restart_settles_the_jobs_whose_wrappers_went_while_no_daemon_ran` (6.07 s) and `theseus::golden::health_says_where_the_push_stands`. The golden was mine (a new health line in a fixture with no connections): fixed by saying nothing for a daemon that sends none. gone_jobs passed alone (5.8 s) and in the next full run: a timing failure under the suite's load, not named in the brief's list; I did not touch its area.
- Lifecycle, `theseus-sim bench lifecycle --runs 10 --check` (debug binaries, this VM): **LIFECYCLE OK**. Cold start p95 27.4 ms (budget 50), config-copy start 28.2, clean shutdown 15.9 (100), SIGKILL then restart 35.0 (150), binary swap 41.0 (200), cancel 22.0 (100). Not compared against a pre-change run on this VM (the budgets are the check).
- The disk filled once during the gate (`No space left on device`); I removed `target/debug/incremental` (inside the repo) and ran the gate with `CARGO_INCREMENTAL=0`.
- keel: `python3 scripts/keel-guard.py` says `keel: ok (… 27 files changed, 0 findings acked)`. The local `main` ref in this clone was stale (a9ad950) and made the first run list five findings from main's own history; I moved the local ref to origin/main (no checkout), after which it is clean. **Keel findings expected: none.**

## Live check on this VM (done)

Scratch daemon started with `ulimit -Sn 1024` (the owner's unit's soft limit), config `[server] max_connections = 2000`, fake model answering "say hello" after 3 s, `scripts/conn-watchers.py` (each connection `session.watch`es one session, a control connection submits one turn):

- Log: `open files limit; connection ceiling soft_before=1024 soft=20000 hard=20000 ceiling=2000`.
- `--conns 1100`: 1,100 watching, 0 refused, 1,100 of 1,100 heard `turn.ended` (first 3,070 ms, last 3,182 ms; the stand-in held 3,000 ms), OK. This is past the 1,017 that ended the daemon.
- `--conns 3000`: 1,999 watching, 1,001 refused with the frame, 1,999 of 1,999 heard the end, health after: held 2,000, refused 1,001, the daemon alive, `theseus health` shows `connections: 55 of 2000 · 1001 refused · open files 20000 of 20000`.

Maintainer's run, same thing: write a config as above (needs only `[model] api_base`, an `env:` secret, discord/web/index off); then
`theseus-sim fake-model --addr 127.0.0.1:9448 --rules rules.json` (`[{"when":"say hello","text":"hi","hold_ms":3000}]`), `(ulimit -Sn 1024; theseusd --config cfg.toml --state-dir S --socket SOCK)`, `scripts/conn-watchers.py --socket SOCK --conns 1100` then `--conns 3000`. Expect `OK`, the counts above, and `refused` rising on health. For scale's `watchers.py` past 1,100, the same script is the row. On the owner's machine the hard limit is 1,048,576: the default ceiling there is 8,192, so 3,000 all fit unless `max_connections` is set lower.

## What is left or uncertain

- Only socket connections count against the ceiling. The web UI's TCP connections, Discord's and `--stdio`'s are not counted (they use few descriptors); the 256 reserve covers them. A `--stdio` daemon shows ceiling `u64::MAX` and fd 0 in health (it never raises limits); the CLI prints "N held" only.
- The refused connection still costs one descriptor for up to 250 ms while its frame is drained; a flood of refusals is bounded by the reserve, and an EMFILE past that is the soft accept path (logged, 50 ms).
- The core's `[server] max_connections` has no validation (any u64; 0 derives). A value above the limit simply stops being the binding limit; EMFILE then falls to the accept rule.
- Docs the maintainer should update: spec Part III item and status.md (the stopping point's row 1), the config chapter's `[server]` keys (`max_connections`), the health line in the technical overview, and the owner's unit (values above). I did not edit those.
- AGENTS.md of theseus-core could name `conns.rs`; I added the note to theseusd's guide only.
