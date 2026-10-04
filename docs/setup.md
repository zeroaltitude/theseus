# Setting it up: one command

Theseus runs on Linux only, by design. On a Linux machine with systemd, from a checkout of this repository:

```
scripts/setup.sh --dry-run                                     # what it would do; changes nothing
scripts/setup.sh --op-token-file ~/.config/theseus/op-token    # do it
```

The script takes a machine from a checkout to a daemon running as a systemd user service, its config at
`/etc/theseus/theseus.toml`, and every secret a 1Password `op://` reference. Run it again whenever you like: a step
whose work is done says so and changes nothing, so after a `git pull` the same command builds what changed,
installs it, and restarts the service.

## What you need

- **Linux with systemd**, and your user manager running (`systemctl --user` answers). On WSL2, `/etc/wsl.conf` needs
  `systemd=true` under `[boot]`; [user-service.md](user-service.md#wsl) has the WSL notes.
- **cgroup v2**, the unified hierarchy: every current distribution's default.
- **rustup**. The build installs the one release that `rust-toolchain.toml` pins, if it is missing.
- **Node.js 22 or later, and npm**, to build the cockpit, the web UI the daemon embeds.
- **1Password**: the `op` CLI, and a service account whose token is in a file only you can read (mode 0600).
  [user-service.md](user-service.md#a-token-file) shows three ways to make the file without the token ever showing
  on a command line. Where there is no vault (a container, CI), a secret can come from an environment variable or a
  private file instead (`env:` and `file:` in the template's `[secrets]`).
- **sudo, once**, if `/etc/theseus` does not exist yet. The directory is made yours, so nothing later needs it.
- An Anthropic API key in your vault, or a key for another provider that speaks the same API.

## What it does

Each step prints one line per thing it checks or does, and the commands it runs with a leading `+`.

1. **Preflight**, read-only: Linux and not root; your systemd user manager, and whether linger is on; cgroup v2;
   the pinned toolchain; node and npm; `op`, when the config names `op://` references; and the token file, by
   `stat` alone (a regular file, yours, mode 0600 or stricter, not empty). A `FAIL` stops the run before anything
   changes, and says how to fix it.
2. **Build**: in `cockpit/`, `npm ci` (skipped when `node_modules` matches the lock file) and `npm run build`
   (skipped when its output is newer than every source), then `scripts/build.sh --profile release-thin`, the
   install profile: the host's glibc build of the five shipped binaries (`theseusd`, `theseus`, `theseus-tui`,
   `theseus-sim`, `theseus-index`). Cargo rebuilds only what changed.
3. **Install**: each binary into `~/.local/bin` (`--prefix`), by a copy to a temporary name and a rename over the
   old one, so a running daemon keeps its image until it restarts. A binary that matches the build is left alone.
4. **Config**: when `/etc/theseus/theseus.toml` (`--config`) does not exist, the script writes it from the
   template: `theseusd example-config`, cut by `theseusd config --sparse` to what differs from the defaults, with
   `[tools] projects_dir` set to `~/projects` and a header that says where it came from. Every `[secrets]` entry is
   an `op://<vault>/<item>/<field>` placeholder for you to fill in, and the script asserts that before it writes:
   no secret value, and no run of characters that could be a token, is ever written. When `/etc/theseus` is
   missing, it is made once with `sudo install -d -o <you> -g <your group> -m 0750 /etc/theseus`, and the file is
   yours, mode 0640. sudo asks for your password itself; the script never sees or keeps it. **An existing config
   is never overwritten**: the script says which keys differ from the template (only yours, only the template's,
   or another value), and never a value.
5. **Check**: `theseusd check` with the installed binary, which resolves every secret and exits. While a
   `[secrets]` entry is still a placeholder, or the config names `op://` references and no token file is named,
   the run stops here (exit status 3) and says what to do.
6. **Service**: `scripts/user-service.sh install --yes`, which writes the user unit, turns on linger, and starts
   it ([user-service.md](user-service.md) has every step), then the unit's own checks: no delegated cgroup, no stop
   hook, `KillMode=process`. When the unit already matches the plan and its daemon answers, there is nothing to
   do; when a binary changed under it, a restart runs the new one.
7. **Health**: `theseus health` on the daemon's socket, once its secrets have resolved.

## The first run, and the next

On a new machine the first run builds, installs, and writes the config, then stops at step 5: the config's
`[secrets]` are placeholders. Put your own vault's reference in place of each one, or, for a service you don't use,
delete its line and turn that part off in its own table (for Discord, `[discord] enabled = false`). Then run the same
command again. It checks the config, installs and starts the service, and shows its health.

Every run after that changes only what is out of date. A run with nothing to do says so at each step and exits 0.

## Where the config lives

`theseusd` finds its config in this order (theseus-5aqz):

1. `--config <file or op:// reference>`;
2. `THESEUS_CONFIG`;
3. `~/.theseus/theseus.toml`, if it exists;
4. `/etc/theseus/theseus.toml`, if it exists.

`theseusd config` prints the source on its first line, and how the lookup found it; `theseusd check` names it on
its `ok:` line; and `theseus health` says `config: file <path>`. The unit names the file outright (`--config` in its
`ExecStart`), so a file that appears later in another place changes nothing for the service.

The file holds only what differs from the defaults: `theseusd example-config` documents every key, and `theseusd
config --sparse` cuts a config to what differs. To change it: edit it, run `theseusd check`, then
`scripts/user-service.sh restart`, because the daemon reads a config file only when it starts. Install a build that
knows a new table before you add the table: the loader refuses a key it does not know.

## Options

| Option | What it does |
|---|---|
| `--dry-run` | Prints every step and every command that would change something, and runs none of them. The preflight's reads run; nothing else does. |
| `--op-token-file FILE` | The service account's token file (default `$THESEUS_OP_TOKEN_FILE`). The check resolves the config's references with it, and the unit names it. |
| `--prefix DIR` | Where the binaries go (default `~/.local/bin`). |
| `--config PATH` | The config file (default `/etc/theseus/theseus.toml`). |
| `--unit NAME` | The user unit (default `theseusd`). Another name is a second daemon beside yours, and needs the next two. |
| `--state-dir DIR`, `--socket PATH` | The daemon's state dir and socket, written into its unit. |
| `--no-build` | Installs what the target dir's `release-thin` build holds, without building. |

Exit status: 0 done, or nothing to do; 1 a step failed; 2 a usage error; 3 stopped before the service for you to act,
as its last lines say.

## A second daemon beside yours

To try a build, or to prove the script itself, without touching your own daemon, give a scratch run its own of
everything:

```
D=/tmp/theseus-scratch
scripts/setup.sh --prefix $D/bin --config $D/etc/theseus.toml --unit theseus-scratch \
  --state-dir $D/state --socket $D/sock --op-token-file ~/.config/theseus/op-token
```

Its config must keep off your daemon's places: `[discord] enabled = false` (one bot answers in one place), and
`[web] enabled = false` or a `[web] port` of its own (yours is 7433). Take it away with `scripts/user-service.sh
--unit theseus-scratch uninstall`, then delete the directory.

## By hand

The same steps, one at a time:

```bash
(cd cockpit && npm ci && npm run build)          # the cockpit first: it is embedded in theseusd
scripts/build.sh --profile release-thin
for b in theseusd theseus theseus-tui theseus-sim theseus-index; do
  cp target/release-thin/$b ~/.local/bin/.$b.new && mv -f ~/.local/bin/.$b.new ~/.local/bin/$b
done
sudo install -d -o "$(id -un)" -g "$(id -gn)" -m 0750 /etc/theseus
theseusd example-config > /tmp/template.toml
theseusd --config /tmp/template.toml config --sparse > /etc/theseus/theseus.toml && chmod 640 /etc/theseus/theseus.toml
#   then edit it: put your vault's op:// references in [secrets], and your projects directory in [tools].
theseusd --op-token-file ~/.config/theseus/op-token check
scripts/user-service.sh --op-token-file ~/.config/theseus/op-token install
theseus health
```
