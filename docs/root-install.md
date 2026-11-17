# Letting Theseus install what its own loop joined

Read this only if the daemon runs from a directory only the administrator can write, the way `docs/user-service.md`'s
unit does after you install a build by hand into `/usr/local/lib/theseus/<commit>/`. Without a step from the
administrator, the self-improvement loop can join a change to `main` and cannot make it the build that runs: it says so
(`theseus self pipeline`), and prints the one command that installs it. This page is the one-time step that lets
Theseus do that command itself, after the owner says yes on a card.

**Read what it gives up first**, in [design/self-install.md](design/self-install.md). In short: a program running as the
owner could already point the service at any program of its choice, so the root-owned directory only ever protected
the programs the unit names from being replaced in place. After this step a program running as the owner can also put
root-owned programs into that directory without the administrator's password, so the directory is protected by what
the helper checks (below) and no longer by the password. Theseus asks the helper only for a commit its own loop joined,
and only after the owner's card (and the second proof, once it is built in).

## What it installs

`scripts/root-install.sh install --user NAME --helper FILE`, run by the administrator with `sudo`, once. `NAME` is the
user the service runs as; `FILE` is the helper built from this checkout (`cargo build --release -p
theseus-root-install`, then `target/release/theseus-root-install`; read it and the script before you run them).

| Path | What | Owner and mode |
|---|---|---|
| `/usr/local/sbin/theseus-root-install` | the helper, the one program the rule allows | root, 0755 |
| `/usr/local/lib/theseus` | where the builds go (made if missing) | root, 0755 |
| `/var/lib/theseus-root-install` | the helper's own directory | root, 0755 |
| `/var/lib/theseus-root-install/stage` | the only place the helper reads programs from | `NAME`, 0700 |
| `/etc/sudoers.d/theseus-root-install` | one rule (below), checked with `visudo -cf` before it is written | root, 0440 |

The rule:

```
Defaults!/usr/local/sbin/theseus-root-install env_reset
NAME ALL=(root) NOPASSWD: NOSETENV: /usr/local/sbin/theseus-root-install [0-9a-f][0-9a-f]... (forty classes)
```

(The `Defaults!` line is not a rule: it makes sudo clear the caller's environment, the loader's `LD_*` variables
included, for this program, as it does by default.)

It lets `NAME` run that one program as root with no password, with a commit of exactly forty lowercase hex digits as its
only argument (sudo's own pattern, besides the helper's check), and with no environment of its own (`NOSETENV`).
`scripts/root-install.sh --dry-run install ...` prints every command and changes nothing; `check` says whether it is
all there; `sudo -n -l`, run as `NAME`, shows the one rule.

## What the helper does, and cannot do

`theseus-root-install <commit>` reads the four programs (`theseusd`, `theseus`, `theseus-tui`, `theseus-index`) from the
staging directory, and nothing else, and copies them into `/usr/local/lib/theseus/<first eight digits of the commit>/`
as root-owned 0755 files, by a temporary directory beside it and a rename. It refuses a staging directory that holds
anything else, a link, a file that is not a program or is not the staging user's, a program whose build stamp does not
name the commit, an existing directory whose content differs (it never overwrites), and a twenty-first build
directory. It runs none of the programs. It changes no unit, no `PATH` and no other directory. One line goes to the
system log for each install and each refusal (`journalctl -t theseus-root-install`).

It cannot prove the programs were built from the commit: the build stamp catches a mistake, not an attacker. What
vouches for them is the loop's own gate and the joined merge's signed commit, which all run as the owner.

**Nothing on the machine may run a program from `/usr/local/lib/theseus/*/` as root or as another user.** The helper
puts root-owned executables there on the owner's word alone.

Old build directories pile up (the helper stops at twenty): remove the ones the unit does not name, as root. The daemon
keeps the one before the current build, for the way back.

## Turning it on

In the config, `[self] install = "ask"` (the default) puts a card to the owner after a join: "Install the change Theseus
joined?". `"auto"` installs with no card (a sensitive setting, which asks for the second proof); `"off"` never does, and
the loop prints the manual step as before. `theseus health` says whether the helper is installed.

## Taking it away

```
sudo scripts/root-install.sh remove
```

It removes the rule, the helper and the helper's own directories (only if empty: a staged program is kept and named),
and keeps `/usr/local/lib/theseus` and every build in it, because the service may be running from one. After it,
the loop is back to joining a change and printing the step.

## By hand

The whole of `install`, for the administrator who reads before running (replace `NAME`):

```bash
sudo install -d -o root -g root -m 0755 /usr/local/lib/theseus /var/lib/theseus-root-install
sudo install -d -o NAME -g "$(id -gn NAME)" -m 0700 /var/lib/theseus-root-install/stage
sudo install -o root -g root -m 0755 target/release/theseus-root-install /usr/local/sbin/theseus-root-install
{ echo 'Defaults!/usr/local/sbin/theseus-root-install env_reset'
  printf 'NAME ALL=(root) NOPASSWD: NOSETENV: /usr/local/sbin/theseus-root-install %s\n' \
    "$(printf '[0-9a-f]%.0s' $(seq 40))"; } > /tmp/theseus-root-install.sudoers
sudo visudo -cf /tmp/theseus-root-install.sudoers && sudo install -o root -g root -m 0440 /tmp/theseus-root-install.sudoers /etc/sudoers.d/theseus-root-install
```
