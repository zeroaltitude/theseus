# CLOUD_REPORT: toasts (theseus-f0bi9)

Branch `cloud/20261013-toasts`. Work commit `89999ce`.

## What I found
- The TUI's `notice.rs` held the notice queue (policy, 1 s debounce, burst fold) and `Delivery` (bell/osc9/osc777). `theseus status --watch` had no notices.
- **Deviation from the brief, on purpose: the title and body do NOT go to the script as raw arguments.**
  `powershell.exe -Command <script> a b` joins the trailing arguments with spaces and parses them as code, so a title with a space and a quote would be code. The brief's "escape-proof by construction" doesn't hold for raw arguments. The argv is therefore the fixed script plus three words: `x` + the UTF-8 of the title, body and `sound`/`silent`, as lower-case hex. The script decodes them and sets them as XML node text (`InnerText`). Hostile text (`"`, backtick, `$(...)`, `'`) cannot reach the parser, and the test asserts the words are alphanumeric and never all digits. The revisit knob is `toast.rs::word` and `toast_argv`. **Unverified on Windows**: the script was not run (no Windows here). The owner's check below covers it, including a hostile title.

## What I changed (89999ce)
- `theseus_client::notify` (`crates/theseus/src/notify/`):
  - `notices.rs`: `Notices`/`Fired` moved from the TUI. `Fired` gains `title`.
  - `mod.rs`: `Delivery` (`auto|toast|bell|osc9|osc777|off`), `Env` (WSL from `WSL_DISTRO_NAME` or `osrelease`; OSC 9 from `WT_SESSION`, iTerm, kitty, WezTerm, ghostty; injectable), `Notifier`. The environment is read at the first ping; nothing is probed at start.
  - `toast.rs`: the fixed script, `Toast`, `Spawner`, `Powershell` (5 s kill), `Toaster` (a worker thread started at the first toast; one in flight, one queued, newer replaces; a missing binary rings the bell once, says so once on stderr, then the bell at once for later pings).
- TUI: `--notify` takes all six kinds, default `auto`. `Effect::Notice` is now `{title, line, sound}` (one test's assertion rewritten, not deleted). `Runner.notifier`; `Runner::new` defaults to `Bell`, so no test can toast. Without the flag, the config's value comes from `health` (`notify_pinned`, `tui_notify`).
- `theseus status --watch --notify HOW` (default `off`): `status_notify.rs`. Bell/OSC go to /dev/tty, never stdout. It reads a session's name once (`session.list {ids}`), and its one timer (to the next debounce) exists only with `--notify`.
- Config: `[tui] notify = "auto"` (`config/tui.rs`, validated against `notices::NOTIFY_KINDS`, in the template), `HealthResult.tui_notify` (a new defaulted field; TS regenerated). **No store format change.**
- `theseus chat` does not exist on this base. **Join note for the `chat` row (theseus-cyha):** build a `Notifier::new(sink)` once, feed `Notices` the same way `status_notify::Alerts` does (`saw` on snapshots, `moved` on live views, `take_due` at `next_due()`), and write `notifier.deliver(how, &f.title, &f.line, sound)`'s bytes to the terminal.
- AGENTS.md of `theseus` and `theseus-tui` updated.

## How I proved it
- New tests: 12 in `notify/tests.rs` (auto by injected env; probe at first ping only; argv fixed with the text as safe words and a hostile title round-tripping; silent and blank-title cases; control characters and length cut; ten delivers returning at once with one in flight and the newest replacing the rest; a missing binary falling back once, saying so once, and then ringing at once; terminal kinds as bytes; every config kind parses), 2 in `status_notify.rs` (a question toasts after its second and not before; `off` holds nothing), 2 in the TUI's `tests_notice.rs` (a question under toast, a burst of 3 as one "3 questions need you" toast, with no terminal bytes), and 1 in `config/tui.rs` (the template is the default, every kind loads, a wrong kind is refused). The existing TUI tests for words and the burst run through the moved `Notices` unchanged: 72/72 TUI, 356/356 `theseus`.
- **Planted reverts** (the file restored and `touch`ed after each, `git status` checked):
  1. The toast run on the caller's thread (`work` called inline in `submit`): `deliver_never_waits_on_the_toast_...` failed (it blocked on the parked spawner).
  2. The title interpolated into the script: `a_toast_runs_a_fixed_script_with_the_text_as_safe_words` failed.
- **Live check on a scratch daemon** (`theseusd` on the fake model, `[tui] notify = "toast"` in its config, no flag, `theseus-tui` under tmux, a fake `powershell.exe` first on `PATH` logging its argv). A question (`fs_write` under `approve`) made one call: `-NoProfile -NonInteractive -Command <script> <title> <body> <sound>` with the words decoding to `ses a8db08`, `● ses a8db08 asks: fs.write create /tmp/live/beta-summary.txt (2 bytes): ... ? · expires 09:01` and `sound`. The config reached the TUI through `health`. Not run live: a failure and a burst (both covered by unit tests only), and `status --watch --notify`.
- **Not measured:** the "frame times unchanged under a burst of 10" claim. By construction the UI thread does only a mutex store and a thread spawn at the first toast, and the revert test shows the blocking case fails, but I measured no frame times.

## The owner's check (only with his OK, one toast)
On the WSL machine, with the built `theseus-tui`:
`theseus-tui --notify toast` against any daemon, then cause one question. Expected: one Windows toast, titled by the task, with the question line as its body, and a sound. Also try a task titled `Tide "report" $(calc) ` + a backtick: the title must show verbatim and nothing run. If the toast shows garbled text or nothing, `powershell.exe` is rejecting the script (unverified here); `toast.rs::SCRIPT` is the place.

## Gate
`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh`: door, keel, keel tests, fmt, shape, features, clippy, cockpit, test build and the reader rule all pass. The suite phase: 4487 run, 34 failed:
- 33 are the known L1 failures on this VM (theseus-sandbox `contract`, `spawn_100`; theseusd `sandbox`);
- `theseus-discord tests_process::an_answer_the_post_writes_after_the_thinking_is_still_the_answer` (not on the known list) failed under load at 21 s and **passes alone** (1.3 s). I take it as a load flake.

I ran the later phases myself: protocol types clean (the TS is committed), and `cargo deny --offline check bans licenses sources` ok (the advisory database was not fetched). The benches were skipped (NO_BENCH). `python3 scripts/keel-guard.py`: `0 findings acked`; the only "finding" printed is another commit's store test rename, not mine. **Keel findings expected:** none.

## Left / uncertain
- Windows toast text and sound behaviour are unverified (above). The app id is Windows PowerShell's own; a toast may need it to be a registered AUMID, which is the first thing to check if nothing appears.
- Where the toast's missing-binary message goes: stderr, so inside the TUI's alternate screen it prints once over the UI; the fallback bell follows. A cleaner place is the TUI footer; I left it.
- `auto` gives OSC 9 only where the terminal is known to show it; tmux inside it is not specially handled.
- Disk: the 4-core VM's disk filled twice (incremental builds beside gate builds); I ran `cargo clean`. Not a code issue.
