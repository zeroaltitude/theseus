# Running the daemon as a systemd user service

Theseus starts by hand today: you run `theseusd` in a shell, and it lives and dies with that shell's session. This
page sets it up as a systemd **user service** instead, and shows how to live with it. One script does the work:

```
scripts/user-service.sh install
```

Everything below is what that command does, why, and what to do afterwards.

## Why

- **Limits, and cancels that are confirmed.** The unit carries `Delegate=yes`, so systemd gives the daemon a
  cgroup subtree of its own. L1 (roadmap row 17b) puts each job in a cgroup inside it, with `memory.max` (2048 MB by
  default) and `pids.max` (512), and a cancel is `cgroup.kill` that the daemon confirms by reading `cgroup.events`
  (`verified_by: cgroup`, row 18a; see M4 §2.2 in [design/m4-boundaries.md](design/m4-boundaries.md)). A daemon
  started from a shell sits in some other cgroup, which is not delegated, so it has neither. Those two rows are
  still ahead on the spine: the delegation is ready for them now, and the daemon moves into its leaf only when
  the first L1 job needs it.
- **It comes back.** `Restart=on-failure` with `RestartSec=5`: a crash, a kill, or an out-of-memory stop starts the
  daemon again five seconds later, and the write-ahead log replays as it does after any crash. A clean stop is
  not restarted, so `theseus shutdown` and `systemctl --user stop` both end it until you start it again.
- **It has a journal.** Everything the daemon writes to stderr goes to systemd's journal, which keeps and rotates
  it: `scripts/user-service.sh logs` follows it.
- **It needs no terminal.** With *linger* on for your user, your user manager starts when the machine does and
  stays up after your last session ends, and the daemon starts with it.

Nothing else changes. It is the same binary, config reference, state directory (`~/.theseus` unless your shell says
otherwise), socket, and web port, so every client works as before.

## Before the first run

The unit has no shell, so two things it needs are named for it, in the shell you run the script from: your config
(step 0) and a token file.

### Step 0: name your config

The plan writes a config into the unit (`--config`) from the shell you run the script in: `THESEUS_CONFIG`, and when
that is not set, the built-in default of the `theseusd` on your `PATH`. A build's default can be a local file
(`~/.theseus/theseus.toml`) that you do not have, and a unit that names a file that is not there cannot start. So, once,
in the profile of the shell you start the daemon in, name the config your daemon reads today, by the reference of its
vault note:

```
export THESEUS_CONFIG=op://<vault>/<item>/notesPlain
```

Open a new shell (or run the line in this one) before `check`. If you keep your config in a local file instead, leave the
variable out and make sure the file is where the plan says. `check` tells you which case you are in: it reads the config
the unit would get from the plan's own `config:` line, not from the variable alone, so it holds for any build of
`theseusd` (see "The one command" for what it prints).

### A token file

The service has no shell, so it cannot see `OP_SERVICE_ACCOUNT_TOKEN` from yours. It reads the 1Password
service-account token from a file that its `--op-token-file` names. A unit never holds the token itself, and the
installer never opens the file: it only checks, with `stat`, that the file is a regular file, owned by you, mode 0600
or stricter, and not empty. Make the file once, in any of these ways, so that the token never shows on a command
line:

```
install -d -m 700 ~/.config/theseus

# from the variable in your shell:
( umask 077; printf %s "$OP_SERVICE_ACCOUNT_TOKEN" > ~/.config/theseus/op-token )

# or typed, with no echo:
( umask 077; read -rsp 'token: ' t; printf %s "$t" > ~/.config/theseus/op-token; echo )

# or from the vault, with `op` signed in:
( umask 077; op read 'op://<vault>/<item>/<field>' > ~/.config/theseus/op-token )
```

Then name it, either each time (`scripts/user-service.sh --op-token-file ~/.config/theseus/op-token install`) or
once, in the profile of the shell you start the daemon in:

```
export THESEUS_OP_TOKEN_FILE=~/.config/theseus/op-token
```

## The one command

Run it as yourself (not with `sudo`), in the shell where you start the daemon today: the plan copies that shell's
`PATH` and its `THESEUS_CONFIG`, `THESEUS_STATE_DIR`, and `THESEUS_SOCKET` into the unit.

```
scripts/user-service.sh install
```

`--dry-run` prints every command it would run and runs none of them, and `--yes` answers its questions for you.
What it does, in order:

1. **Check.** Read-only. It looks at, and prints one line for each:
   - systemd: your user manager answers (`systemctl --user is-system-running`);
   - linger: on for you (`loginctl show-user`);
   - on WSL, `/etc/wsl.conf` has `systemd=true`;
   - cgroups: v2, and your user manager delegates `memory` and `pids`;
   - `theseusd`, `theseus`, and `op` are on `PATH`, and `theseusd` is not a build-tree binary;
   - the token file is right (`stat` only);
   - the config the unit would get, read from the plan's `config:` line (step 0), is a readable file or an
     `op://<vault>/<item>/<field>` reference;
   - whether the unit is installed, and who answers on the socket.

   A `FAIL` stops the install before it asks anything, and says how to fix it. A `WARN` does not.
2. **The plan.** The script runs `theseusd install --user`, which prints the unit it would write and changes
   nothing. It refuses, and says what to run, when the token file is wrong.
3. **The unit.** After a `y/N`, `theseusd install --user --apply` writes `~/.config/systemd/user/theseusd.service`.
   Then `systemctl --user daemon-reload`.
4. **Linger.** If it is off, a `y/N`, then `loginctl enable-linger`.
5. **A daemon you started by hand.** It is found by the socket answering `theseus health` within five seconds,
   never by looking at processes. It holds the socket and the store the service's daemon needs, so after a `y/N`
   the script runs `theseus shutdown` (a clean stop) and waits, for up to 20 seconds, until the socket stops
   answering.
6. **Start.** `systemctl --user enable --now theseusd.service`, then it waits up to 30 seconds for the socket to
   answer, prints the first lines of `theseus health`, and prints a cheat sheet of the commands below.

Nothing is written before the first `y/N`, and answering no stops the install with the machine as it was.

What `check` prints on a machine that is ready (`install` starts with the same lines):

```
== check: is this machine ready?
this machine
  ok    platform: Linux, as ada
  ok    systemd: your user manager is running
  ok    linger: on for ada (the service keeps running when you log out)
  ok    WSL: /etc/wsl.conf has systemd=true
  ok    cgroups: v2, and your user manager delegates memory and pids (L1 limits work)
the install
  ok    theseusd: /home/ada/.local/bin/theseusd (theseusd 0.0.1)
  ok    theseus: /home/ada/.local/bin/theseus
  ok    op: /usr/local/bin/op (the unit's PATH is this shell's, so the daemon finds it)
  ok    token file: /home/ada/.config/theseus/op-token (yours, mode 0600, not empty)
  ok    config: op://<vault>/<item>/notesPlain (a vault note: the daemon reads it with the token)
right now
  info  unit: not installed (/home/ada/.config/systemd/user/theseusd.service)
  info  daemon: a daemon you started by hand answers on /home/ada/.theseus/theseus.sock (the service is inactive)
result: ready (0 warning(s))
```

The `config:` line is the one to look at before you answer a question. In a shell that has not exported
`THESEUS_CONFIG`, on a build whose default is a local file you do not have, it reads:

```
  FAIL  config: /home/ada/.theseus/theseus.toml is not a readable file (THESEUS_CONFIG is not set here, so this is theseusd's built-in default)
        the unit would be written to read it, and the daemon would not start. Name your config's vault note instead,
        in the shell that runs this script (your profile keeps it):  export THESEUS_CONFIG=op://<vault>/<item>/notesPlain
        or write the file.
```

`install` stops there: nothing has been asked, written, or stopped. With the variable exported, or the file written, the
line is `ok` and says which it is.

## Day to day

```
scripts/user-service.sh status      is it up? (and: is a newer binary waiting for a restart?)
scripts/user-service.sh logs        follow the journal; Ctrl-C leaves the daemon running
scripts/user-service.sh restart     a clean stop, then a start
scripts/user-service.sh stop        stop it
scripts/user-service.sh start       start it
scripts/user-service.sh check       is the machine still as it should be?
theseus health                      what the daemon says of itself
```

Each says the command it runs. `logs` passes any further arguments to `journalctl`, so
`scripts/user-service.sh logs -n 200` shows the last 200 lines and follows. `start` and `restart` wait for the
socket to answer, so a returned prompt means it is up.

**Installing a new binary.** An install is a copy to a temporary name and a rename over `~/.local/bin/theseusd`
(AGENTS.md, "Installing"). The running daemon keeps the image it started with, so nothing changes until you
run `scripts/user-service.sh restart`. That is a clean stop (the unit's `KillSignal=SIGINT`: the daemon writes
`server.stopping`, checkpoints, and removes its socket) and a start of the new file at the same path, which the
unit's `ExecStart` names. `status` tells you when the running daemon's binary was replaced on disk: it reads
`/proc/<pid>/exe`, which says `(deleted)` after a rename over it.

**A config change.** When the vault's config note changes, the daemon restarts itself in place: an `exec` of
`/proc/self/exe` with the same pid. Systemd sees no restart; the unit stays active with the same main pid, and the
journal has the daemon's own line about it. That `exec` runs the *same image* again, so it never picks up a binary
you installed in the meantime. Only `restart` swaps the binary.

**What a stop does to jobs.** `KillMode=process`: a stop signals the daemon alone, so jobs that are running finish,
and the next daemon reads their results from the spool. A crash recovers the same way.

**Why a restart never waits on a job.** The first L1 job gives jobs cgroups of their own, which switches the job
limits on in the daemon's cgroup. While they are on, the kernel refuses to start the next daemon there if a job of
the old one is still running. So the unit runs `theseusd cgroup-release` after every stop and crash
(`ExecStopPost=`), which switches them off. A job that is still running keeps going without its limits, and the
next daemon's first L1 job switches them on again. The daemon gives jobs cgroups only under a unit that has this
line; without it, health's `sandbox:` line says why, and L1 jobs run without a memory limit.

**Changing the unit.** Don't edit `theseusd.service`: a later `--apply` rewrites it. Put your own settings in a
drop-in (`systemctl --user edit theseusd`), which is never touched. To change what the plan writes (the config
reference, the state directory, the socket, the token file, or `PATH`), run `scripts/user-service.sh install`
again from a shell that has the new values. `theseusd install --user --check` lists what differs.

**If the unit won't stay up.** `scripts/user-service.sh check` first, then `logs`. The usual causes:
- *The token file.* The daemon refuses a token file that group or others can read, or an empty one, and says so in
  the journal. `check` names the fix.
- *Another daemon holds the store.* The store is single-process, so a daemon started by hand beside the unit makes
  the unit's daemon fail and retry every five seconds. Stop the one you started (`theseus shutdown`).
- *A different socket.* The unit uses the `THESEUS_SOCKET` and `THESEUS_STATE_DIR` of the shell that installed it.
  The script reads the socket and the token file from the installed unit, so it follows them.

## Undoing it

```
scripts/user-service.sh uninstall
```

It prints the plan (`theseusd install --user --remove`), asks `y/N`, then runs `systemctl --user disable --now
theseusd.service` (a clean stop), `theseusd install --user --remove --apply`, and `systemctl --user daemon-reload`. The
installer removes the unit file only if it wrote it: a file that does not start with its header is kept. **It never
touches your state directory, your store, or your token file**, and it leaves linger as it found it
(`loginctl disable-linger` turns that off). Afterwards you start `theseusd` by hand, as before.

## WSL

The machine this was written on is WSL2 on Ubuntu 22.04, with systemd 249. The rest holds for any WSL2 distro that
runs systemd.

- **systemd must be on.** `/etc/wsl.conf` needs a `[boot]` section with `systemd=true`. After you add it, run
  `wsl --shutdown` from Windows once; the setting takes effect when the distro next starts. `check` verifies it.
- **Delegation.** This systemd's user manager delegates `memory` and `pids` and not `cpu`, so L1 will have memory and
  process limits and no CPU limit (M4 decision 14). `check` shows what your user manager delegates.
- **Linger matters more here.** It is what starts your user manager, and so the daemon, when the distro starts
  without a login, and what keeps them up after the last terminal closes.
- **When WSL stops the distro** (`wsl --shutdown`, a Windows restart or update, or the VM running out of memory
  or disk) the daemon stops with it, and not always cleanly. The next start replays the write-ahead log, which
  is the crash path the daemon is built for. WSL starts the distro again when you open a terminal or run any `wsl`
  command, and with linger the service then starts with it.
- **The journal.** `journalctl --user` needs read access to the journal, which members of the `adm` and
  `systemd-journal` groups have. If it prints "No journal files were found", check `id` for one of them.

## The unit, and the commands under the script

`theseusd install --user` is what writes the unit. The script wraps it, and you can run it yourself: it prints a
plan and changes nothing, `--apply` performs it, `--check` compares the machine with it and exits 1 if anything
differs, and `--remove` is its inverse. The flag that names the token file works before the subcommand or after it:
`theseusd --op-token-file F install --user` and `theseusd install --user --op-token-file F` are the same. The script
uses the first form, which every build of `theseusd` reads.

The unit, as it is written:

```
# Written by `theseusd install --user`. A later --apply rewrites it:
# put your own changes in a drop-in (`systemctl --user edit theseusd`).
[Unit]
Description=Theseus daemon

[Service]
Type=exec
ExecStart=/home/ada/.local/bin/theseusd --config op://<vault>/<item>/notesPlain --op-token-file /home/ada/.config/theseus/op-token
Environment="PATH=/home/ada/.local/bin:/usr/local/bin:/usr/bin:/bin"
Environment="LANG=C.UTF-8"
ExecStopPost=-/home/ada/.local/bin/theseusd cgroup-release
Delegate=yes
KillSignal=SIGINT
KillMode=process
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
```

`Delegate=yes` is the delegated cgroup; `ExecStopPost` switches the job limits off after a stop or a crash, so a
restart never waits for a running job; `KillSignal=SIGINT` is the daemon's clean stop; `KillMode=process` leaves
running jobs alone on a stop; `Restart=on-failure` brings it back after a crash. `ExecStart` is the binary you ran
the install with, so install a reviewed build into `~/.local/bin` and run the script from there, not a build in a
`target/` directory (`check` warns about one). `--state-dir` and `--socket` appear there only when your shell had
set them.
