# CLOUD_REPORT: sonar-ping (theseus-mbulo)

Branch `cloud/20261011-sonar-ping`. Cockpit only (plus `cockpit/AGENTS.md`'s map line and two screenshots).

## Step 1: the sonar ping and its ring

**What I built** (commits 8809071c, 87bdfe26):
- `src/ship/sonar.ts` (pure): gaps (180 to 480 s, seeded like the gull), the ping's voice (sine 1.2 to 1.6 kHz, 6 ms attack, 1.5 to 2.5 s decay, lowpassed feedback echo, a fainter, lower "pong" later), `pingWhat` (ring and sound by Sound/Calm/hidden), `Room` (the one room a ping and a gull share), the scheduler `startSonar`, and the ring's life (`showRing`/`hideRing`/`useRing`).
- `src/ship/sonarsound.ts`: the Web Audio graph on the shared master (gain 0.1, under the roll's 0.15). An unconnected timer oscillator outlasts the echoes and lets go of the master when it ends.
- `ShipAudio.ping`, `pingStop`, `pingsHeard`; `dispose` ends pings.
- `src/ship/Sonar.tsx`: an SVG circle (non-scaling stroke, `--color-live` cyan) from the middle to past the corners, a fainter one trailing 0.45 s behind, 2.2 s, fading as it grows. After the labels and before the panels in `Ship.tsx`, pointer-events none, nothing mounted while idle. CSS hides it in Calm and under prefers-reduced-motion.
- `useSonar(onShip)` in `useShipSound.ts`, mounted in the Shell: the scheduler runs only while the Ship is shown. The ring shows with Sound off; sound only with Sound on (also in Calm); hidden tab: neither, and the due ping is dropped, not queued. The gull's visit now claims the same `Room`, so a gull is let pass while a ping rings, and a ping waits for a gull's pass plus 1.5 s. Dev/bench builds get `window.__shipPing()`.

**Proof:**
- `npm test` 357 pass, 0 fail (`test/sonar.test.ts` is 12 new tests); `npx tsc -b` clean; `npm run lint` 0 errors (existing warnings only).
- Planted reverts (each restored and touched), what failed:
  - gap minimum 180 -> 5: "the gaps" test
  - drop the gull wait: "never with a gull"
  - `sound: true` always: "what a ping does"
  - ring in Calm: "what a ping does"
  - ring never removes itself: "the ring: it shows, restarts..."
  - hidden tab not dropped: "a hidden tab"
  - `pingStop` does nothing: "the graph" (nodes not stopped)
- Screenshots: `docs/assets/cockpit-1012/sonar-ring-early.png` and `sonar-ring-late.png`: headless Chromium (software WebGL) on `/ship?synthetic`, the ring's animation paused mid-way by script (the page is too slow to catch it live). The ring is thin at the edges of the view; the stroke and opacity were raised after the first look.
- No test under load was asked for or run.

**Live check for the maintainer** (a real browser, sound on):
`cd cockpit && npx vite` then open `/ship?synthetic`; click Sound; in the console run `__shipPing()` (ring only, no sound: that hook shows the ring) and wait 3 to 8 minutes for a real ping: tone and echo with the ring from the centre. With Sound off the ring still comes. With Calm on, no ring and (Sound on) the tone still plays.

**Uncertain / for the owner:**
- The ping sounds only while the Ship is the page shown (the owner asked for "the ship main page"); the sea and gull sound everywhere. Easy to widen.
- Levels (gain 0.1, echo 0.35) were set by reasoning, not by ear; nobody listened here. `ShipAudio` could render it offline for a listen.
- The ring colour is the live cyan; it reads on the dark sea but is faint over the bright glow.
- Doc to change: none needed beyond the `cockpit/AGENTS.md` line I added.

## Gate

`TZ=America/Phoenix THESEUS_GATE_NO_BENCH=1 scripts/gate.sh` FAILED in its keel phase, at 7 s, before any build. The findings are about the range from the base, none about my files: moved tests (`config.rs` -> `config/tests.rs`), a renamed bench test, `allow-added` in theseus-index/theseus-protocol, a raised example-toml cap and a raised long-files ceiling; and "scripts/keel-signers lists no key at the base, so no ack can pass yet". My diff touches only `cockpit/` and `docs/assets/`. The Rust phases did not run; I did not build Rust (the task is cockpit only). The shape phase (`scripts/shape.sh`) passes; the cockpit's tests, tsc and lint are as above.

**Keel findings expected:** none from my commits (no cap, ceiling, test deletion or allow).
