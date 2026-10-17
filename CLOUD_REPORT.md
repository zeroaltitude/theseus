# CLOUD_REPORT: cockpit-notices-policy (theseus-go4ig)

## Step: cockpit desktop notices through the policy
**Found.** Shell.tsx's bell only announced `confirm.list` entries that began waiting. The policy (`theseus_protocol::notices`) is Rust-only; the cockpit had no port and the generated TS holds only its types.

**Changed** (88d93104):
- `cockpit/src/lib/notices.ts`: pure TS port of the table for what the page can see: rule 1 (question), 2 (reminder at half-life), 3 (failed/blocked), 5 (flagged or long root finish; short finish informs), 6/7 (answer quiet + retracts; expiry informs), 8 (task report; interrupts if flagged), a set reminder from `wake.noticed` (purpose `remind`), `deliver`, and a `Notifier` (position rule, seed, due reminders). Spend, forecast, model check-ins and routines are not ported (they inform at most or are the digest's). No Burst: notification `tag`s dedupe per question/work.
- `cockpit/src/components/DesktopNotices.tsx`: renders nothing; mounted only while the bell is on, holds the shared `work.watch` (`useThreadBoard`), seeds on the first snapshot, shows pings as browser Notifications (tag = question id, retraction closes it), silent for finishes, focused session (`/session/:id`) is no ping.
- `Shell.tsx`: `useApprovalNotices` replaced by `useNoticeSwitch`; bell tooltip reworded.
- `cockpit/AGENTS.md`: one entry.
- No Rust, protocol, store or config change; the shared `joinable`/`task_redirect_line` contract is not part of this row and was not added.

**Proof** (in cockpit/): `npm ci`; `npm test` 449 pass, 0 fail (9 new in test/notices.test.ts); `npx tsc --noEmit` clean; `npm run lint` no errors, none for the new files; `npm run build` ok.
- Planted revert: `policy()` ending in `return say('interrupt', ...)` (notify on every change) failed 4 tests: "a progress change makes no notice", "a question reminds once…", "a failure and a block…", "a finish interrupts…". File restored and touched; 449 pass.

**Not run.** The full Rust gate (`scripts/gate.sh`): the change is cockpit-only, so I ran the gate's cockpit phase commands by hand (lint, test, build) instead. Neither cargo setup nor the deny fetch was done. No live check in a real browser.

**Live check for the maintainer.** Build the cockpit, start a scratch daemon, open `/`, click the bell and grant permission. Then (a) trigger a tool needing approval: a desktop notice "Theseus: ● … asks: …" appears, and answering it closes it; (b) `theseus remind` a 1-minute reminder: "⏰ …" appears; (c) a flagged or 5-minute+ turn finishing: a silent "✓ … is ready after …"; (d) while a session page is open and the window focused, its own questions do not ping.

**Uncertain / owner should hear.** (1) The TS table must stay in step with the Rust one (noted in cockpit/AGENTS.md); a wasm or RPC `notice` source would remove the duplicate. (2) The bell now holds a `work.watch` while on, where before it polled `confirm.list` (still polled for the nav badge). (3) Doc to update by the maintainer: spec Part III item and status.md.
