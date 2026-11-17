# Theseus installs what its own loop joined (issue 22, last line)

On a machine whose service runs programs from a directory only the administrator can write
(`/usr/local/lib/theseus/<commit8>/`), the self-improvement loop can join a change to main and cannot make it the
build that runs (issue 20 makes the loop say so). The owner, 2026-11-16: it should
be able to. This note says first what that directory protects today, because that is what is being traded, then the
helper that does the one thing, the administrator's one-time step, and the daemon's side.

## What the root-owned directory protects today, and what it does not

The service is a **user** unit. `theseusd install --user` writes it to `~/.config/systemd/user/theseusd.service`
(`crates/theseusd/src/install/modes.rs`, `user_unit_path`), owned by the operator, and `ExecStart` names the daemon's
program by absolute path. Nothing makes that file, or the user manager that reads it, the administrator's:

- the floor (`crates/theseus-core/src/toolrun/guarded.rs`) names the config, the store, the token and the sealed sets,
  and does not name the unit or `~/.config/systemd`;
- an L0 job runs as the operator (AGENTS.md, "The security posture": "an L0 job can strip its environment, or talk
  to the socket itself"), and so can any other program the operator runs;
- `systemctl --user daemon-reload` and `restart` ask for no password, and a drop-in
  (`~/.config/systemd/user/theseusd.service.d/*.conf`, which `docs/user-service.md` says is never touched) overrides
  the unit's `ExecStart`.

So a program running as the owner can already make the next start of the daemon run **any program of its choice**:
write its own build anywhere it can write, point the unit (or a drop-in) at it, restart. It did not need the
administrator, and it still does not.

What the root-owned directory therefore stops, exactly:

- **In-place replacement** of the program the unit names, and of the three programs beside it (`theseus`,
  `theseus-tui`, and `theseus-index`, the index tender the daemon starts from its own directory). A program running
  as the owner cannot overwrite `/usr/local/lib/theseus/<commit8>/theseusd`; to change what that path holds it needs
  the administrator's password.
- **A silent swap.** Changing the running build that way is a change of the unit, which is a file the owner can see
  and `status` reports; it is not a quiet overwrite of a file whose path never changes.
- **An accident**: the loop's own install (issue 20) copying a build over a program the service runs.

What it does not stop: re-pointing the unit; shadowing a tool the daemon finds on `PATH` (`~/.local/bin` is in the
unit's `PATH`); anything an owner's program can do by being the owner. It is a separation of mistakes and a visible
step, not a boundary against the owner's account. (The other layout, `theseusd install --separate`, is a real boundary:
a system unit, the daemon as its own user, a binary at `/usr/local/lib/theseus/theseusd` that the operator cannot
replace or re-point. **This work is not offered there** and the helper never touches that file; see "The helper writes
one directory".)

### What the administrator's step is today, and what it checks

The step the loop prints on a root-owned layout (issue 20, `rsi/join/layout.rs`, `step`) is:

```
sudo install -d -m 0755 /usr/local/lib/theseus/<commit8> && sudo install -m 0755 <build>/release/{theseusd,theseus,theseus-tui,theseus-index} /usr/local/lib/theseus/<commit8>/ && PATH=/usr/local/lib/theseus/<commit8>:$PATH scripts/user-service.sh install --yes && scripts/user-service.sh restart
```

Its checks are two: sudo asks for the administrator's password, and `user-service.sh install` checks that `theseusd` is on
`PATH` and is not a build-tree binary, then writes the unit. **It checks nothing about what the programs are**: the
person at the keyboard decides what to copy. That judgement is what the owner trades for self-sufficiency.

## What is given up

A program running as the owner can now ask the helper too. It can put four programs of its own choosing into a new
`/usr/local/lib/theseus/<commit8>/` with no password. What it could not do before is **have root-owned files in that
directory**; what it could always do (point the unit at its own build) it still can. So the directory is protected by
what the helper checks, and no longer by the administrator's password. Plainly, the new things a program running as the
owner can now do, and the limits on each:

1. Put root-owned, mode 0755 executables in `/usr/local/lib/theseus/<commit8>/`. They are inert unless something runs
   them: **nothing on the machine may run a program from that directory as root or as another user** (the
   administrator's own checklist says so). The helper sets no setuid bit and no other mode.
2. Fill the disk under `/usr/local/lib/theseus`. The helper refuses a build over a count of 20 directories there
   (`MAX_BUILDS`); the administrator prunes old ones by hand.
3. Choose the directory's name by its commit: only the first eight hex digits of a commit it names. An existing
   directory is never overwritten, so it cannot replace a build the unit already names.

What the helper cannot give: the programs are not run by it (as root nothing the owner supplied is executed), no path,
option or environment is read from the caller, and it writes one directory.

**Who vouches that the programs are the commit's.** The helper cannot prove it. A build stamp in the daemon's image
names the commit (catches a mistake: a stale directory, the wrong build), and an attacker who can write the stamp can
write anything. What vouches is the loop: its own gate, the joined merge's signed commit, and the daemon's rule that it
asks only for a commit its own join row names and that is main's head on the remote at that moment. All of that runs as
the owner. So the helper protects the directory only as far as the owner's account is trusted.

## The helper

`crates/theseus-root-install`, one binary, `theseus-root-install <commit>`. Standard library and `libc` only.

**Inputs it reads.** Its one argument (exactly forty lowercase hex digits; anything else, none, or a second argument,
is refused). Nothing from its environment: it clears it and sets `PATH` to `/usr/sbin:/usr/bin:/sbin:/bin` (it runs no
program, so the variable only closes a door). It reads no file but the staging directory's four files. The paths are
compiled in; a test asserts the real program carries no override.

**The staging directory.** `/var/lib/theseus-root-install/stage`, derived by the helper. Required, walking each
component from `/` with `O_NOFOLLOW`: `/var/lib/theseus-root-install` and every directory above it root-owned and not
group- or world-writable; the stage directory itself a directory (not a link), not group- or world-writable, owned by a
non-root user (the administrator's step makes it the service's user's, mode 0700). It must hold exactly
`theseusd`, `theseus`, `theseus-tui`, `theseus-index`, nothing else.

**What it checks of each file.** Opened with `O_NOFOLLOW` relative to the directory's descriptor; a regular file
(`fstat` on the open descriptor, not the name); owned by the staging directory's owner (so a hard link to a file the
owner does not own is refused); one link; not empty; under 1 GiB; begins with the ELF magic. It is **copied** into a
fresh directory it makes beside the target, from the open descriptor, and every later check reads the copy, which the
owner can no longer change (this closes the race between check and copy: the source is read once, the copy is what is
examined and what is renamed).

**The build stamp.** The copy of `theseusd` must contain the stamp the daemon carries
(`theseus-build-stamp:<commit>:`, `crates/theseusd/src/main.rs`) and every stamp in it must name the commit asked for.
It does not run `--version`: running a program the owner wrote **as root** would hand the owner root. It reads the
bytes. This catches a mistake, not an attacker. The other three programs are only required to be ELF files of the right
names: only `theseusd` carries a stamp today.

**What it writes: one directory.** It creates `/usr/local/lib/theseus/.install-<commit8>-<pid>` (mode 0700, root),
writes the four copies there (0755, root:root, created `O_EXCL|O_NOFOLLOW`), makes the directory 0755, and
`renameat2(RENAME_NOREPLACE)`s it to `/usr/local/lib/theseus/<commit8>`, then syncs `/usr/local/lib/theseus`.
`/usr/local/lib/theseus` must already exist (the administrator's step makes it) and be root-owned and not
group/world-writable. If `<commit8>` exists: identical content in the four files and nothing else means success
("already installed", changes nothing); anything else is a refusal. It never removes a directory it did not just make
(its own temporary one on a failure). It changes no unit, no `PATH`, no other directory and no file of
`/usr/local/lib/theseus` itself, so the `--separate` layout's `/usr/local/lib/theseus/theseusd` is out of its reach (a
commit's eight hex digits cannot be that name).

**What it says.** One line to the system log (`LOG_AUTH`, tag `theseus-root-install`) for each install, each
"already installed", and each refusal, with the reason; and the same line on its stderr. Exit 0 or 1.

**Sudoers.** One rule, `<service user> ALL=(root) NOPASSWD: NOSETENV: /usr/local/sbin/theseus-root-install
[0-9a-f]...[0-9a-f]` (forty classes: the rule's own pattern allows exactly forty hex digits and no space, in addition to
the helper's own check). `scripts/root-install.sh` writes it (with a `Defaults!` line for the program that makes sudo clear the caller's environment, `LD_*` included, which the helper cannot do for itself: the dynamic loader reads them before `main`), and checks it with `visudo -cf` before it is moved into place.

## The administrator's one-time step

`scripts/root-install.sh install` (run by the administrator, with `sudo`; `docs/root-install.md`): it puts the helper in
`/usr/local/sbin`, makes `/usr/local/lib/theseus` and the staging directory (the service's user's, 0700), writes the
sudoers rule, and prints what it did. `scripts/root-install.sh remove` is the reverse and removes nothing it did not
make. Nothing here is run by a runner on any machine; Tabitha does it on the owner's machine with his yes.

## The daemon's side

Code: `crates/theseus-core/src/rsi/join/root_install/` (`offer.rs` the card, `answer.rs` the yes and the run,
`verify.rs` the start after, `unit.rs` the unit's text, `machine.rs` `sudo -n` and the user unit).

`[self] install` is `"ask"` (default), `"auto"`, or `"off"` (a `true` of today's file reads as `"auto"`, a `false` as
`"off"`). It is itself a sensitive setting (`[self]` is on the sensitive list), and `"auto"` is the one that matters
most: it needs a **fresh** second proof to turn on once the proof is wired in. It applies on a root-owned layout, that
is, where the user unit's `ExecStart` names `/usr/local/lib/theseus/<hex>/theseusd`. On a user's own layout only
`"auto"` installs, as `true` did.

After a join on that layout, when the setting is not `"off"` and the helper is installed (`sudo -n -l` says the rule
lets the service's user run it; `health` says so, in `binary.helper`):

- `ask`: the build is made in L1 (as every install builds), and a card is put to the owner: "Install the change Theseus
  joined: <title>, <commit8>?", sensitive, through the same seam as `config.propose` (`config_change/proof_seam.rs`).
  Until the proof is wired in, **no card is made** (it would be refused when answered, and no build is spent on it), and
  the owner is told why, once, with the manual step.
- `auto`: no card; the same run.
- no helper: nothing is installed, and the owner is told the plain step.

**Who may answer the card.** Only the owner himself: Discord's button from his account, his own terminal, the cockpit.
`approval::owners_alone` names the card's question (`self.install.ack`), so a process that `peer_proc` finds to be an
agent (Claude Code, Codex, anything with no terminal) is refused on the CLI and on the web socket, and draws no button.
In `auto` there is no card, which is why that setting needs the fresh proof.

**On the yes.** The card's answer settles in one frame, and the install runs as a task off the answer's path:

1. The commit must be one the loop's own `self.joined` row names (the ledger) **and** main's head on the remote after a
   fetch, at that moment. Else: nothing, and a ledger row says which.
2. The unit must still be what the card named: the same single `ExecStart` program, in a per-commit directory of the
   program directory, not already the new one.
3. The programs the join's L1 build made are copied into the staging directory, and the helper runs through `sudo -n`.
   A refusal ends the install with nothing changed.
4. The memory of the install is written to the **store** (META `self.install.pending`): the commit, the program the
   service ran, the unit's text as it was. Then the unit is written with its one `ExecStart` program changed and not a byte
   else. (The brief said "by the same code the setup's install uses": re-running `theseusd install --user` from the
   transient unit would rebuild the whole unit from that unit's environment and could change its `PATH` and flags, so
   only the program path is changed, with a test that the rest is untouched.)
5. A transient `systemd-run --user` unit, whose script is its argument (no file is written for it and none read back),
   restarts the service, waits up to four minutes for the new daemon's confirmation, and otherwise writes the unit's old
   text back (embedded in the script, from the store's copy) and restarts once more. A restart that fails goes back at
   once. At most two restarts, onto the build and back; never a loop.

**The start after.** The daemon that finds the install's record and is the build it names waits up to two minutes for
health (it serves its socket, its store is open, and the index tender is off or not `down` or `stalled`), writes the
confirmation, records `self.root_install` `verified`, and only then does the join's notice say "installed and running"
(`self_installed_as` holds back while an install of that commit is pending). The daemon that finds the record and is not
that build says the install went back, once. Every step is a `self.root_install` ledger row.

**Nothing the daemon trusts lives where a tool can write it.** Which commit, which program ran before, and the unit as it
was are in the store; the commit joined is the ledger's; the card's arguments are the question's. The one file kept,
`<state>/self-install/confirmed`, is the new daemon's go-ahead for the script; it is on the floor
(`toolrun/guarded.rs`), and a test plants files under that directory and shows they change nothing. The staging
directory's content is user-writable by nature, which is why the helper copies from open descriptors and checks the copy.

## What a reviewer should hunt for

Assume a program running as the owner that wants the daemon to run code of its choice: anything this branch gives it it
did not have (above); any input to the helper it controls beyond the staging directory's content; a race between check
and copy; a path component it can replace; the helper writing anywhere but its one directory; a sudoers rule that allows
more than the one program; the daemon installing a commit its loop did not join; a rollback that can be steered.
