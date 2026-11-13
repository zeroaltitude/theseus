# b7-arm-reachability-watch — nothing on main watched whether the ARM host was reachable

Board line 37 (B7, P3), greenfield: no card and no prior commit. Recorded 2026-11-13 by the factory-four worker.

**This note is a record, not a product change.** The script and units it describes live in
`~/reports/theseus-arm-host/lane/` and are not tracked in this repository, which has no home for the ARM host
operator scripts.

## The complaint

> The M1 Max can be unreachable for hours with nothing saying so; nothing on main watches it.

ARM reachability was only ever tested by a cut that needed it, so the cost landed at the worst moment: a
departure discovered the host was down *at the point it needed a second host*, with the cut already in flight.

## The change

`lane/arm-reach.sh`, beside `arm.sh`, plus `arm-reach.service` and `arm-reach.timer` written next to it and
**deliberately not installed**.

- `arm-reach.sh probe` — one `ssh -o BatchMode=yes -o ConnectTimeout=10` running `echo ok`, appending one TSV
  line to `lane/reach.log`: `epoch · iso-local · state · rtt_ms|reason`. The same reachability `arm.sh` itself
  needs; a host that answers ping but will not take an ssh session is not usable by a cut, so ping is not the
  test.
- `arm-reach.sh status` — **one line**, in the shape `conductor-liveness.sh` already uses (one line, `OK …` /
  `DOWN …`, exit 0 always). "Since when" is derived by walking `reach.log` back to the first record of the
  current run, so a restart of the timer or a reboot of this machine loses nothing a conductor needs.
- `arm-reach.sh history [N]` — the last N records.

The log *is* the state; there is no second state file. There is **no loop and no daemon**: one `probe` writes
one line and the user timer makes it periodic. It never pages, never messages, never writes into `depart.log`,
and never touches a lane.

### Three states, not two — and the third was found by running it once

The first live probe answered `Permission denied (publickey,password)`, because `arm.sh` sources
`~/.keychain/zeroradeons-sh` for its agent and this script did not. **A host that refuses a key has answered**:
it is up, listening and almost certainly healthy, and reporting it as "unreachable for three hours" would send
a conductor hunting a dead machine that is alive. So:

- `up` — answered `ok`.
- `auth` — `Permission denied`, `Host key verification failed`, `Too many authentication failures`,
  `no matching host key`. The host is **UP**; the ssh agent on *this* machine is the fault, which is a
  completely different repair.
- `down` — refused, timed out, no route, name not resolving. A cut cannot use it.

`status` also appends `[STALE: last probe N min ago; is arm-reach.timer running?]` when the newest record is
more than 15 minutes old, because otherwise "up since" is a lie told by a dead timer.

### The timer

`OnCalendar=*-*-* *:00/5:00 America/Phoenix`, `Persistent=false`, `AccuracySec=1s` — the house pattern from
`depart.timer` (now :00/:30) and `precut-fleet.timer` (:20). Five minutes: the question is "has the host been
down for hours", and a five-minute grain is accurate enough for a cut that fires every thirty, at one ssh
handshake per fire. `Persistent=false` because a backfilled probe would record the wrong minute for
"unreachable since", which is the one thing this exists to get right.

## How it was proved, with no compile

Six fixture cases against a synthetic `reach.log` (no ssh, no host), plus one live probe:

| case | output |
| --- | --- |
| no log yet | `UNKNOWN no probe recorded yet in … (enable arm-reach.timer)` |
| up, 3 consecutive probes | `OK ARM 192.168.1.98 reachable, up since 2026-11-13 09:41:46 MST (20 min, 3 consecutive probes)` |
| was up, now down 4 probes | `DOWN ARM 192.168.1.98 unreachable for 20 min, since … (4 probes; rc=255 ssh: connect to host … No route to host)` |
| recovered after a long down | `OK … up since 2026-11-13 10:00:46 MST (1 min, 1 consecutive probes)` |
| the timer itself died | `OK … (95 min, 1 consecutive probes) [STALE: last probe 95 min ago; is arm-reach.timer running?]` |
| answering, refusing our key | `AUTH ARM 192.168.1.98 answering but refusing our key for 10 min, since … -- the host is UP; the ssh agent on this machine is the fault` |

**One live probe, after sourcing the keychain: `up`, 196 ms.** The M1 Max was reachable at 2026-11-13 10:02:32
MST. That probe cost one `echo ok` and started nothing on the host.

## What was deliberately left

- **I did not install the units and I did not enable the timer**, as the line required. They sit beside the
  script. Enabling is one owner/conductor action:
  ```
  cp ~/reports/theseus-arm-host/lane/arm-reach.{service,timer} ~/.config/systemd/user/
  systemctl --user daemon-reload && systemctl --user enable --now arm-reach.timer
  ```
  and reading it is `~/reports/theseus-arm-host/lane/arm-reach.sh status`.
- No new notification channel was invented and no daemon loop was added.
- `arm.sh` itself was not modified.
