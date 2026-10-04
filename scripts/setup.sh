#!/usr/bin/env bash
# One command from a checkout to a running daemon (theseus-00me). Theseus is Linux only, by design. In order:
#
#   1. preflight  Linux, not root; your systemd user manager (and linger); cgroup v2; the pinned toolchain
#                 (rust-toolchain.toml) and node 22+ with npm, to build; `op` when the config names op://
#                 references; the token file, if one is named. A FAIL stops the run before anything changes.
#   2. build      the cockpit (npm ci, npm run build: the web UI the daemon embeds), then the five binaries
#                 (scripts/build.sh --profile release-thin, the install profile: the host's glibc).
#   3. install    each binary into the prefix (default ~/.local/bin) by copy-then-rename, so a running daemon
#                 keeps its image until it restarts. A binary that matches the build is left alone.
#   4. config     /etc/theseus/theseus.toml (or --config): when there is none, the template cut to what differs
#                 from the defaults (`theseusd example-config`, then `config --sparse`), every [secrets] entry an
#                 op:// placeholder for you to fill in, never a value. /etc/theseus is made once with sudo, owned
#                 by you, 0750, and the file 0640, so nothing later needs sudo. An existing config is never
#                 overwritten: the run says which keys differ from the template instead.
#   5. check      `theseusd check`: every secret resolves. A placeholder still in [secrets], or no token file for
#                 the config's op:// references, stops the run here, saying what to do.
#   6. service    scripts/user-service.sh install --yes: the systemd user unit, started (a restart when the
#                 binaries changed under a running service). Then the unit's own checks.
#   7. health     `theseus health` on the daemon's socket.
#
# Run it again at any time: a step whose work is done says so and changes nothing. The how-to is docs/setup.md.
#
# Options:
#   --dry-run              print every step and its commands, and change nothing (the preflight's reads run)
#   --op-token-file FILE   the 1Password service-account token's file, mode 0600 (default $THESEUS_OP_TOKEN_FILE):
#                          the check resolves the config's op:// references with it, and the unit names it
#   --prefix DIR           where the binaries go (default ~/.local/bin)
#   --config PATH          the config file (default /etc/theseus/theseus.toml, which theseusd finds by itself)
#   --unit NAME            the user unit (default theseusd). Another name is a second daemon beside yours, and
#   --state-dir DIR          then these two are required, so it never shares your daemon's store
#   --socket PATH            or socket
#   --no-build             install what the target dir's release-thin build holds, without building
#   --help, -h
#
# It never prints a secret, never stores sudo's password (sudo asks for it itself), and never touches a config
# or a unit it did not make. Exit status: 0 done, or nothing to do; 1 a step failed; 2 a usage error; 3 stopped
# before the service for you to act (fill in the config's secrets, or name a token file), as the last lines say.
set -u
set -o pipefail
exec 3>&1 # "+ command" lines reach the terminal from inside a $(...)

ROOT=$(cd "$(dirname "$0")/.." && pwd -P)
SELF=$0
ME=$(id -un)
MY_GROUP=$(id -gn)
: "${HOME:?HOME is not set}"
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
case ${XDG_CONFIG_HOME:-} in
/*) CONFIG_HOME=$XDG_CONFIG_HOME ;;
*) CONFIG_HOME=$HOME/.config ;;
esac
SHIPPED=(theseusd theseus theseus-tui theseus-sim theseus-index) # scripts/build.sh --shipped
DEFAULT_CONFIG_FILE=/etc/theseus/theseus.toml
DRY=0
BUILD=1
PREFIX=$HOME/.local/bin
CONFIG=$DEFAULT_CONFIG_FILE
TOKEN=${THESEUS_OP_TOKEN_FILE:-}
UNIT_NAME=theseusd
STATE_DIR=""
SOCKET_OPT=""
FAILS=0
CHANGED_BINS=0

usage() {
  sed -n '2,/^set -u$/p' "$0" | sed -e '/^set -u$/d' -e 's/^# \{0,1\}//'
}

# --- saying and running (as scripts/user-service.sh does) ---

say() { printf '%s\n' "$*"; }
dry() { [ "$DRY" = 1 ]; }
line() { printf '  %-5s %s\n' "$1" "$2"; }
hint() { printf '        %s\n' "$*"; }
ok() { line ok "$*"; }
info() { line info "$*"; }
warn() { line WARN "$*"; }
fail() {
  line FAIL "$*"
  FAILS=$((FAILS + 1))
}
step() {
  say
  say "== $*"
}

# A word as a shell would take it back: plain characters as they are, anything else in quotes.
shq() {
  case $1 in
  "" | *[!A-Za-z0-9_./:@%+=,-]*) printf "'%s'" "${1//\'/\'\\\'\'}" ;;
  *) printf '%s' "$1" ;;
  esac
}
cmd_text() {
  local out="" a
  for a in "$@"; do out+=" $(shq "$a")"; done
  printf '%s' "${out# }"
}
show() { printf '+ %s\n' "$(cmd_text "$@")" >&3; }
note() { printf '  # %s\n' "$*" >&3; }

# run CMD...: say it, then run it (a dry run only says it).
run() {
  show "$@"
  dry && return 0
  "$@"
}

# Long runs of letters and digits look like a token: never shown, whatever printed them.
mask() { sed -E 's/[A-Za-z0-9_-]{40,}/<masked>/g'; }

stop_here() { # stop_here CODE WORDS...: the last lines of a run that stops early
  local code=$1
  shift
  say
  for w in "$@"; do say "$w"; done
  exit "$code"
}

# --- options ---

need_value() { [ $# -ge 2 ] && [ -n "$2" ] || {
  echo "$SELF: $1 needs a value (--help lists the options)" >&2
  exit 2
}; }
while [ $# -gt 0 ]; do
  case $1 in
  --dry-run) DRY=1 ;;
  --no-build) BUILD=0 ;;
  --op-token-file) need_value "$@"; TOKEN=$2; shift ;;
  --op-token-file=*) TOKEN=${1#*=} ;;
  --prefix) need_value "$@"; PREFIX=$2; shift ;;
  --prefix=*) PREFIX=${1#*=} ;;
  --config) need_value "$@"; CONFIG=$2; shift ;;
  --config=*) CONFIG=${1#*=} ;;
  --unit) need_value "$@"; UNIT_NAME=$2; shift ;;
  --unit=*) UNIT_NAME=${1#*=} ;;
  --state-dir) need_value "$@"; STATE_DIR=$2; shift ;;
  --state-dir=*) STATE_DIR=${1#*=} ;;
  --socket) need_value "$@"; SOCKET_OPT=$2; shift ;;
  --socket=*) SOCKET_OPT=${1#*=} ;;
  -h | --help)
    usage
    exit 0
    ;;
  *)
    echo "$SELF: unknown option $1 (--help lists them)" >&2
    exit 2
    ;;
  esac
  shift
done

# Paths as a unit needs them: `~` expanded, and absolute.
absolute() {
  local p=$1
  case $p in "~") p=$HOME ;; "~/"*) p=$HOME/${p#"~/"} ;; esac
  case $p in /*) ;; *) p=$PWD/$p ;; esac
  printf '%s' "$p"
}
PREFIX=$(absolute "$PREFIX")
CONFIG=$(absolute "$CONFIG")
[ -n "$TOKEN" ] && TOKEN=$(absolute "$TOKEN")
[ -n "$STATE_DIR" ] && STATE_DIR=$(absolute "$STATE_DIR")
[ -n "$SOCKET_OPT" ] && SOCKET_OPT=$(absolute "$SOCKET_OPT")
UNIT_NAME=${UNIT_NAME%.service}
case $UNIT_NAME in
"" | *[!A-Za-z0-9._-]*)
  echo "$SELF: --unit '$UNIT_NAME': a unit's name is letters, digits, -, _, and ." >&2
  exit 2
  ;;
esac
UNIT_ARGS=()
if [ "$UNIT_NAME" != theseusd ]; then
  UNIT_ARGS=(--unit "$UNIT_NAME")
  if [ -z "$STATE_DIR" ] || [ -z "$SOCKET_OPT" ]; then
    echo "$SELF: --unit $UNIT_NAME is a second daemon beside yours: give it its own --state-dir and --socket" >&2
    exit 2
  fi
fi
UNIT=$UNIT_NAME.service
UNIT_FILE=$CONFIG_HOME/systemd/user/$UNIT
SOCKET=${SOCKET_OPT:-${THESEUS_SOCKET:-$HOME/.theseus/theseus.sock}}
TARGET=${CARGO_TARGET_DIR:-$ROOT/target}/release-thin
TOKEN_ARGS=()
[ -n "$TOKEN" ] && TOKEN_ARGS=(--op-token-file "$TOKEN")

# What the service's daemon is given, as scripts/user-service.sh writes it into the unit: the config by its path
# (so the unit reads it whatever else appears later), the state dir and socket when named, and a PATH that finds
# the installed binaries first.
export THESEUS_CONFIG=$CONFIG
[ -n "$STATE_DIR" ] && export THESEUS_STATE_DIR=$STATE_DIR
[ -n "$SOCKET_OPT" ] && export THESEUS_SOCKET=$SOCKET_OPT
case ":$PATH:" in *":$PREFIX:"*) PREFIX_ON_PATH=1 ;; *) PREFIX_ON_PATH=0 ;; esac
export PATH="$PREFIX:$PATH"

# --- reading the config ---

# The [secrets] entries of a config file, one `name<TAB>value` line each, the value unquoted.
secrets_of() {
  awk '
    /^[[:space:]]*\[/ { t = $0; gsub(/[[:space:]]/, "", t); insec = (t == "[secrets]"); next }
    insec && /^[[:space:]]*[A-Za-z0-9_."-]+[[:space:]]*=/ {
      k = substr($0, 1, index($0, "=") - 1); gsub(/[[:space:]"]/, "", k)
      v = substr($0, index($0, "=") + 1)
      if (match(v, /"[^"]*"/)) v = substr(v, RSTART + 1, RLENGTH - 2); else { gsub(/^[[:space:]]+|[[:space:]].*$/, "", v) }
      print k "\t" v
    }' "$1"
}

# A config document as `[table] key<TAB>value` lines (a multi-line array joined), to compare two by key.
flat() {
  awk '
    function trim(s) { gsub(/^[[:space:]]+|[[:space:]]+$/, "", s); return s }
    /^[[:space:]]*#/ || /^[[:space:]]*$/ { next }
    inarr { val = val " " trim($0); if (trim($0) ~ /^\]/) { print tbl key "\t" val; inarr = 0 }; next }
    /^[[:space:]]*\[/ { tbl = trim($0) " "; next }
    index($0, "=") > 0 {
      key = trim(substr($0, 1, index($0, "=") - 1)); val = trim(substr($0, index($0, "=") + 1))
      if (val ~ /^\[/ && val !~ /\][[:space:]]*(#.*)?$/) { inarr = 1; next }
      print tbl key "\t" val
    }' "$1"
}

# What differs between two configs by key (yours, the template's), never a value.
differs() {
  awk -F '\t' '
    FNR == NR { a[$1] = a[$1] "\x1f" $2; next }
    { b[$1] = b[$1] "\x1f" $2 }
    END {
      for (k in a) if (!(k in b)) printf "          only yours:            %s\n", k
      for (k in b) if (!(k in a)) printf "          only the template'"'"'s:  %s\n", k
      for (k in a) if ((k in b) && a[k] != b[k]) printf "          another value:         %s\n", k
    }' <(flat "$1") <(flat "$2") | sort
}

# ===================================================================================================
say "Theseus setup: Linux only, by design. Every step says what it does; a step already done changes nothing."
dry && say "dry run: every command that would change something is printed with a leading +, and none is run"
say "  checkout:  $ROOT"
say "  binaries:  $PREFIX"
say "  config:    $CONFIG"
say "  unit:      $UNIT ($UNIT_FILE)"
state_said="the [server] state_dir of the config (default ~/.theseus)"
say "  state dir: ${STATE_DIR:-${THESEUS_STATE_DIR:-$state_said}}"
say "  socket:    $SOCKET"
say "  token:     ${TOKEN:-none named}"

# ===================================================================================================
step "1. preflight: is this machine ready?"

os=$(uname -s)
if [ "$os" != Linux ]; then
  fail "platform: $os. Theseus is Linux only, by design (cgroups, namespaces, pidfds, seccomp)"
elif [ "$(id -u)" = 0 ]; then
  fail "platform: you are root. Run it as yourself: the daemon is your user service (sudo is asked for once, for /etc/theseus)"
else
  ok "platform: Linux ($(uname -m)), as $ME"
fi

sys=$(systemctl --user is-system-running 2>&1)
case $sys in
running) ok "systemd: your user manager is running" ;;
degraded) ok "systemd: your user manager is running (degraded: a unit of yours failed; systemctl --user --failed)" ;;
starting | initializing) warn "systemd: your user manager is still starting; the service step waits for it" ;;
*)
  fail "systemd: no user manager answers (systemctl --user is-system-running said: $(printf '%s' "$sys" | head -n 1))"
  [ -n "${XDG_RUNTIME_DIR:-}" ] || hint "XDG_RUNTIME_DIR is not set: log in again, or: export XDG_RUNTIME_DIR=/run/user/$(id -u)"
  grep -qi microsoft /proc/sys/kernel/osrelease 2>/dev/null &&
    hint "on WSL: put systemd=true under [boot] in /etc/wsl.conf, then run \`wsl --shutdown\` from Windows"
  ;;
esac
linger=$(loginctl show-user "$ME" --property=Linger --value 2>/dev/null)
case $linger in
yes) ok "linger: on for $ME (the daemon keeps running when you log out)" ;;
*) info "linger: off for $ME: the service step turns it on (loginctl enable-linger $ME), so the daemon outlives your sessions" ;;
esac

if [ "$(stat -fc %T /sys/fs/cgroup 2>/dev/null)" = cgroup2fs ]; then
  ok "cgroup v2: /sys/fs/cgroup is the unified hierarchy"
else
  fail "cgroup v2: /sys/fs/cgroup is not the unified hierarchy; the kernel's job control needs it"
  hint "boot with systemd.unified_cgroup_hierarchy=1 (every current distribution's default)"
fi

CHANNEL=$(sed -n 's/^channel *= *"\(.*\)"/\1/p' "$ROOT/rust-toolchain.toml" | head -n 1)
TOOLCHAIN_MISSING=0
if [ "$BUILD" = 1 ]; then
  if ! command -v rustup >/dev/null 2>&1 || ! command -v cargo >/dev/null 2>&1; then
    fail "toolchain: rustup and cargo are not on PATH (nor in ${CARGO_HOME:-$HOME/.cargo}/bin)"
    hint "install rustup (rustup.rs), then run this again: the build installs the pinned release itself"
  elif rustup toolchain list 2>/dev/null | grep -q "^$CHANNEL-"; then
    ok "toolchain: $CHANNEL, pinned by rust-toolchain.toml"
  else
    TOOLCHAIN_MISSING=1
    info "toolchain: $CHANNEL (rust-toolchain.toml) is not installed yet: the build installs it (rustup toolchain install)"
  fi
  node_v=$(node --version 2>/dev/null)
  node_major=${node_v#v}
  node_major=${node_major%%.*}
  if [ -z "$node_v" ] || ! command -v npm >/dev/null 2>&1; then
    fail "node: node and npm are needed to build the cockpit, the web UI the daemon embeds (node 22 or later)"
  elif [ "$node_major" -lt 22 ] 2>/dev/null; then
    fail "node: $node_v is older than 22, which the cockpit's build needs"
  else
    ok "node: $node_v and npm $(npm --version 2>/dev/null) (the cockpit's build)"
  fi
else
  info "build: skipped (--no-build): the binaries come from $TARGET"
fi

# The config: is there one, and does it name vault references?
CONFIG_STATE=missing
if [ -e "$CONFIG" ]; then
  if [ -f "$CONFIG" ] && [ -r "$CONFIG" ]; then
    CONFIG_STATE=present
    mode=$(stat -c %a "$CONFIG")
    owner=$(stat -c %U "$CONFIG")
    ok "config: $CONFIG exists ($owner, mode $mode): it is left as it is"
    case $mode in *[2367]? | *?[1-7]) warn "config: mode $mode lets others read or write it: chmod 640 $CONFIG" ;; esac
  else
    fail "config: $CONFIG is there but is not a file you can read"
  fi
else
  cdir=$(dirname "$CONFIG")
  if [ -d "$cdir" ]; then
    if [ -w "$cdir" ]; then
      info "config: $CONFIG does not exist: it is written from the template into $cdir"
    else
      fail "config: $cdir is not yours to write: sudo chown $ME:$MY_GROUP $cdir && sudo chmod 750 $cdir, or name another --config"
    fi
  else
    up=$cdir
    while [ ! -d "$up" ]; do up=$(dirname "$up"); done
    if [ -w "$up" ]; then
      info "config: $CONFIG does not exist: $cdir is made (0750), and the template written into it"
    elif command -v sudo >/dev/null 2>&1; then
      info "config: $CONFIG does not exist, and $up is root's: $cdir is made once with sudo, owned by you"
      hint "(sudo install -d -o $ME -g $MY_GROUP -m 0750 $cdir; sudo asks for your password itself, and nothing keeps it)"
    else
      fail "config: $cdir must be made as root, and there is no sudo: make it yourself, once:"
      hint "install -d -o $ME -g $MY_GROUP -m 0750 $cdir   (as root), then run this again"
    fi
  fi
fi
REFS=0
if [ "$CONFIG_STATE" = present ]; then
  REFS=$(secrets_of "$CONFIG" | awk -F '\t' '$2 ~ /^op:\/\// { n++ } END { print n + 0 }')
fi
if command -v op >/dev/null 2>&1; then
  ok "op: $(command -v op) (the daemon reads the vault through it)"
elif [ "$CONFIG_STATE" = present ] && [ "$REFS" -gt 0 ]; then
  fail "op: the 1Password CLI is not on PATH, and the config names $REFS op:// reference(s): the daemon reads them through it"
elif [ "$CONFIG_STATE" = present ]; then
  info "op: not on PATH, and not needed: the config names no op:// reference"
else
  warn "op: the 1Password CLI is not on PATH: install it before you fill in the template's op:// references"
fi

# The token file, by `stat` alone, as the unit's installer checks it: never opened here.
if [ -n "$TOKEN" ]; then
  if fields=$(stat -c '%F|%u|%a|%s' -- "$TOKEN" 2>&1); then
    IFS='|' read -r type owner mode size <<<"$fields"
    bits=$((8#$mode))
    if [ "$type" != "regular file" ]; then
      fail "token file: $TOKEN is not a regular file ($type)"
    elif [ "$owner" != "$(id -u)" ]; then
      fail "token file: $TOKEN is owned by uid $owner, not by you"
    elif [ $((bits & ~8#600)) != 0 ] || [ $((bits & 8#400)) = 0 ]; then
      fail "token file: $TOKEN has mode $mode: it must be 0600 (chmod 600 $TOKEN)"
    elif [ "$size" = 0 ]; then
      fail "token file: $TOKEN is empty"
    else
      ok "token file: $TOKEN (yours, mode $(printf '%04o' "$bits"), not empty)"
    fi
  else
    fail "token file: $TOKEN cannot be examined ($(printf '%s' "$fields" | head -n 1))"
  fi
else
  info "token file: none named (--op-token-file FILE): the run stops after the config, before the check and the service"
fi

if [ "$PREFIX_ON_PATH" = 0 ]; then
  warn "PATH: $PREFIX is not on your PATH: add it in your profile, so your shells find theseus (the unit gets it)"
fi
if [ "$CONFIG" = "$DEFAULT_CONFIG_FILE" ] && [ -e "$HOME/.theseus/theseus.toml" ]; then
  info "note: ~/.theseus/theseus.toml exists, and a theseusd you start by hand without --config reads it first;"
  hint "the unit names $CONFIG itself"
fi

if [ $FAILS -gt 0 ]; then
  stop_here 1 "Not set up: fix the FAIL lines above (nothing was changed), then run this again."
fi

# ===================================================================================================
step "2. build: the cockpit, then the five binaries (release-thin)"
if [ "$BUILD" = 0 ]; then
  info "skipped (--no-build)"
else
  cd "$ROOT" || exit 1
  if [ "$TOOLCHAIN_MISSING" = 1 ]; then
    run rustup toolchain install || stop_here 1 "Not set up: the pinned toolchain did not install (above)."
  fi
  # npm in cockpit/, quietly: its last 40 lines only when it fails, as the gate does.
  npm_step() {
    local log
    show sh -c "cd cockpit && npm $*"
    dry && return 0
    log=$(mktemp)
    if (cd cockpit && npm "$@") >"$log" 2>&1; then
      rm -f "$log"
      return 0
    fi
    tail -n 40 "$log" | sed 's/^/        /'
    rm -f "$log"
    return 1
  }
  dist=crates/theseusd/cockpit/dist/index.html
  if [ -f cockpit/node_modules/.package-lock.json ] && [ cockpit/node_modules/.package-lock.json -nt cockpit/package-lock.json ]; then
    ok "cockpit: its node_modules matches package-lock.json (npm ci not needed)"
  else
    npm_step ci --no-audit --no-fund || stop_here 1 "Not set up: npm ci failed in cockpit/ (above)."
    dry || ok "cockpit: npm ci installed its packages"
  fi
  newer=""
  [ -f "$dist" ] && newer=$(find cockpit -path cockpit/node_modules -prune -o -type f -newer "$dist" -print -quit)
  if [ -f "$dist" ] && [ -z "$newer" ]; then
    ok "cockpit: its build ($dist) is newer than every source (npm run build not needed)"
  else
    npm_step run -s build || stop_here 1 "Not set up: the cockpit's build failed (above)."
    dry || ok "cockpit: built into $(dirname "$dist")"
  fi
  run scripts/build.sh --profile release-thin || stop_here 1 "Not set up: the build failed (above)."
  dry && note "cargo builds what changed; a second run finds everything fresh"
fi
if ! dry; then
  for b in "${SHIPPED[@]}"; do
    [ -x "$TARGET/$b" ] || stop_here 1 "Not set up: $TARGET/$b is missing: build first (scripts/build.sh --profile release-thin)."
  done
fi

# ===================================================================================================
step "3. install: the binaries into $PREFIX, each by copy-then-rename"
if [ ! -d "$PREFIX" ]; then run mkdir -p "$PREFIX" || exit 1; fi
same=0
for b in "${SHIPPED[@]}"; do
  if dry; then
    show install -m 0755 "$TARGET/$b" "$PREFIX/.$b.new"
    show mv -f "$PREFIX/.$b.new" "$PREFIX/$b"
    continue
  fi
  if cmp -s "$TARGET/$b" "$PREFIX/$b"; then
    same=$((same + 1))
    continue
  fi
  run install -m 0755 "$TARGET/$b" "$PREFIX/.$b.new" && run mv -f "$PREFIX/.$b.new" "$PREFIX/$b" ||
    stop_here 1 "Not set up: $b did not install (above)."
  CHANGED_BINS=$((CHANGED_BINS + 1))
done
if dry; then
  note "each one only if it differs from the build (cmp): a binary that matches is left alone"
elif [ $CHANGED_BINS = 0 ]; then
  ok "all ${#SHIPPED[@]} match the build: nothing to install"
else
  ok "installed $CHANGED_BINS ($same already matched): $("$PREFIX/theseusd" --version 2>/dev/null)"
fi
THESEUSD=$PREFIX/theseusd

# ===================================================================================================
step "4. config: $CONFIG"
# The template, cut to what differs from the defaults, with the operator's projects dir: what a new config is,
# and what an existing one is compared with. Every [secrets] entry is an op:// placeholder: asserted.
cut_template() { # cut_template DIR: writes DIR/template.toml, DIR/config.toml
  "$THESEUSD" example-config >"$1/template.toml" &&
    env -u OP_SERVICE_ACCOUNT_TOKEN -u THESEUS_OP_TOKEN_FILE "$THESEUSD" --config "$1/template.toml" config --sparse \
      >"$1/sparse.toml" 2>"$1/sparse.err" || {
    say "the template did not cut: $(tail -n 3 "$1/sparse.err" | mask)"
    return 1
  }
  {
    echo "# Theseus's config on this machine, written by scripts/setup.sh on $(date '+%Y-%m-%d %H:%M %Z') from the template"
    echo "# (\`theseusd example-config\`), cut to what differs from the defaults: \`theseusd example-config\` documents"
    echo "# every key. Each [secrets] entry is an op:// reference, never a value: put your own vault's in place of"
    echo "# each placeholder (op://<vault>/<item>/<field>), or, for a service you don't use, delete its line and turn"
    echo "# that part off in its own table (for Discord, [discord] enabled = false)."
    awk '
      /^\[/ { intools = ($0 == "[tools]") }
      intools && /^projects_dir[[:space:]]*=/ { print "projects_dir = \"~/projects\""; next }
      { print }' "$1/sparse.toml"
  } >"$1/config.toml"
  # No value is ever written: every secret is a placeholder reference, and nothing anywhere looks like a token.
  local bad
  bad=$(secrets_of "$1/config.toml" | awk -F '\t' '$2 !~ /^op:\/\/<[^>]+>\/<[^>]+>\/./ { print $1 }')
  if [ -n "$bad" ]; then
    say "refused: a [secrets] entry is not an op:// placeholder: $(printf '%s ' $bad)"
    return 1
  fi
  if grep -Eq '[A-Za-z0-9_+/=-]{40,}' "$1/config.toml"; then
    say "refused: the config holds a run of 40 or more characters that could be a token"
    return 1
  fi
  # It loads, as the daemon will read it.
  env -u OP_SERVICE_ACCOUNT_TOKEN -u THESEUS_OP_TOKEN_FILE "$THESEUSD" --config "$1/config.toml" config --sparse \
    >/dev/null 2>"$1/load.err" || {
    say "the new config does not load: $(tail -n 3 "$1/load.err" | mask)"
    return 1
  }
}

if [ "$CONFIG_STATE" = present ]; then
  if dry; then
    note "it exists, so it is left as it is; a real run says which keys differ from the template (never a value)"
  else
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT
    if cut_template "$work" &&
      env -u OP_SERVICE_ACCOUNT_TOKEN -u THESEUS_OP_TOKEN_FILE "$THESEUSD" --config "$CONFIG" config --sparse \
        >"$work/yours.toml" 2>"$work/yours.err"; then
      d=$(differs "$work/yours.toml" "$work/config.toml")
      if [ -z "$d" ]; then
        ok "it exists, and says what the template says: nothing to do"
      else
        ok "it exists: left as it is. Where it differs from the template (keys only; values are never shown):"
        printf '%s\n' "$d"
      fi
    else
      fail "it exists, but does not load: $(tail -n 3 "$work/yours.err" 2>/dev/null | mask)"
      stop_here 1 "Not set up: fix $CONFIG (\`theseusd --config $CONFIG config\` says why), then run this again."
    fi
  fi
else
  cdir=$(dirname "$CONFIG")
  if [ ! -d "$cdir" ]; then
    up=$cdir
    while [ ! -d "$up" ]; do up=$(dirname "$up"); done
    if [ -w "$up" ]; then
      run install -d -m 0750 "$cdir" || stop_here 1 "Not set up: $cdir could not be made."
    else
      say "$cdir is made once with sudo, because $up is root's. It will be yours ($ME:$MY_GROUP, 0750), so nothing"
      say "later needs sudo. sudo asks for your password itself; this script never sees or keeps it."
      run sudo install -d -o "$ME" -g "$MY_GROUP" -m 0750 "$cdir" ||
        stop_here 1 "Not set up: sudo did not make $cdir. Make it yourself, once, then run this again:" \
          "  sudo install -d -o $ME -g $MY_GROUP -m 0750 $cdir"
    fi
  fi
  if dry; then
    show "$THESEUSD" example-config
    show "$THESEUSD" --config TEMPLATE config --sparse
    note "the template cut to what differs from the defaults, [tools] projects_dir set to ~/projects, a header"
    note "asserted before anything is written: every [secrets] entry is an op:// placeholder, and no token-like text"
    show install -m 0640 CONFIG.new "$CONFIG"
  else
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT
    cut_template "$work" || stop_here 1 "Not set up: the config was not written (above)."
    install -m 0640 "$work/config.toml" "$CONFIG.new" && mv -f "$CONFIG.new" "$CONFIG" ||
      stop_here 1 "Not set up: $CONFIG could not be written."
    names=$(secrets_of "$CONFIG" | cut -f1 | tr '\n' ' ')
    ok "wrote $CONFIG ($(stat -c '%U:%G %a' "$CONFIG"), $(wc -l <"$CONFIG") lines): $(secrets_of "$CONFIG" | wc -l) secrets, each an op:// placeholder"
    hint "${names% }"
    CONFIG_STATE=written
  fi
fi

# ===================================================================================================
step "5. check: every secret resolves"
if dry; then
  show "$THESEUSD" ${TOKEN_ARGS[@]+"${TOKEN_ARGS[@]}"} --config "$CONFIG" check
  note "not run while a [secrets] entry is still a placeholder, or the config names op:// references and no token file is named"
else
  placeholders=$(secrets_of "$CONFIG" | awk -F '\t' '$2 ~ /^op:\/\/<|^op:\/\/[^\/]*\/</ { print $1 }' | tr '\n' ' ')
  refs=$(secrets_of "$CONFIG" | awk -F '\t' '$2 ~ /^op:\/\// { print $1 }' | tr '\n' ' ')
  if [ -n "$placeholders" ]; then
    info "the config's [secrets] still holds placeholders: ${placeholders% }"
    again="run this again."
    [ -n "$TOKEN" ] || again="run this again with --op-token-file FILE (a file, mode 0600, that holds the service account's token)."
    stop_here 3 "Stopped before the check and the service: your turn. In $CONFIG, put your own vault's" \
      "op://<vault>/<item>/<field> in place of each placeholder, or, for a service you don't use, delete its line and" \
      "turn that part off in its own table (for Discord, [discord] enabled = false). Then $again"
  fi
  if [ -n "$refs" ] && [ -z "$TOKEN" ]; then
    info "the config names op:// references, which a token would resolve: ${refs% }"
    stop_here 3 "Stopped before the check and the service: name the 1Password service account's token file," \
      "  $SELF --op-token-file FILE" \
      "(a file only you can read, mode 0600; docs/user-service.md shows how to make one)."
  fi
  show "$THESEUSD" ${TOKEN_ARGS[@]+"${TOKEN_ARGS[@]}"} --config "$CONFIG" check
  out=$(THESEUS_LOG=warn NO_COLOR=1 "$THESEUSD" ${TOKEN_ARGS[@]+"${TOKEN_ARGS[@]}"} --config "$CONFIG" check 2>&1)
  rc=$?
  if [ $rc != 0 ]; then
    printf '%s\n' "$out" | mask | tail -n 12 | sed 's/^/        /'
    stop_here 1 "Not set up: the check failed (above). Fix $CONFIG, then run this again."
  fi
  printf '%s\n' "$out" | mask | sed 's/^/        /'
  ok "the config loads, and every secret resolves"
  if [ -z "$TOKEN" ]; then
    stop_here 3 "Stopped before the service: the unit reads the vault with a token file it names, so name one:" \
      "  $SELF --op-token-file FILE"
  fi
fi

# ===================================================================================================
step "6. service: $UNIT"
us=("$ROOT/scripts/user-service.sh" ${TOKEN_ARGS[@]+"${TOKEN_ARGS[@]}"} ${UNIT_ARGS[@]+"${UNIT_ARGS[@]}"})
if dry; then
  show "${us[@]}" install --yes
  note "or, when the unit already matches the plan and its daemon answers: nothing, or a restart if a binary changed"
  note "what it would do, from its own dry run:"
  "${us[@]}" --dry-run install 2>&1 | sed 's/^/    /'
else
  matches=0
  "$THESEUSD" ${TOKEN_ARGS[@]+"${TOKEN_ARGS[@]}"} install --user ${UNIT_ARGS[@]+"${UNIT_ARGS[@]}"} --check >/dev/null 2>&1 && matches=1
  state=$(systemctl --user is-active "$UNIT" 2>/dev/null)
  answers=0
  timeout 5 "$PREFIX/theseus" --socket "$SOCKET" health >/dev/null 2>&1 && answers=1
  if [ $matches = 1 ] && [ "$state" = active ] && [ $answers = 1 ] && [ $CHANGED_BINS = 0 ]; then
    ok "the unit matches the plan, is active, and its daemon answers on $SOCKET: nothing to do"
  elif [ $matches = 1 ] && [ "$state" = active ] && [ $CHANGED_BINS -gt 0 ]; then
    info "the unit matches the plan and is active, and $CHANGED_BINS binaries changed: a restart runs the new ones"
    run "${us[@]}" restart || stop_here 1 "Not set up: the restart failed (above; ${us[*]} logs)."
  else
    run "${us[@]}" install --yes || stop_here 1 "Not set up: the service did not install (above)."
  fi
  # The unit's own checks: no delegated cgroup and no stop hook (theseus-gyin), and a stop that signals the
  # daemon alone, so running jobs finish.
  if grep -q -E '^(ExecStopPost|Delegate)=' "$UNIT_FILE"; then
    fail "unit: $UNIT_FILE delegates a cgroup or has a stop hook: run ${us[*]} install --yes"
  elif ! grep -q '^KillMode=process' "$UNIT_FILE"; then
    fail "unit: $UNIT_FILE has no KillMode=process"
  else
    ok "unit: no delegated cgroup, no stop hook, KillMode=process"
  fi
  [ $FAILS = 0 ] || stop_here 1 "Not set up: the unit is not as it should be (above)."
fi

# ===================================================================================================
step "7. health: what the daemon says of itself"
if dry; then
  show "$PREFIX/theseus" --socket "$SOCKET" health
  note "repeated for up to 40 s, until its secrets have resolved"
  say
  say "(dry run) Nothing was changed."
  exit 0
fi
h=""
for _ in $(seq 1 40); do
  if now=$(timeout 5 "$PREFIX/theseus" --socket "$SOCKET" health 2>/dev/null); then
    h=$now
    case $h in *$'\n'"secrets: resolving"*) ;; *) break ;; esac
  fi
  sleep 1
done
if [ -z "$h" ]; then
  stop_here 1 "Not set up: nothing answers on $SOCKET (${us[*]} status, or ${us[*]} logs)."
fi
printf '%s\n' "$h" | mask | grep -E '^(theseus |startup:|config:|secrets:|sandbox:|index:|discord:)' | cut -c1-240 | sed 's/^/  /'
say "  unit: $(systemctl --user show "$UNIT" -p ActiveState -p SubState -p MainPID | tr '\n' ' ')"
cli=theseus
[ -n "$SOCKET_OPT" ] && cli="theseus --socket $SOCKET_OPT"
say
say "Done: theseusd runs as the user service $UNIT, from $PREFIX, on $CONFIG."
say "  talk to it:        $cli ask \"Hello! What can you do?\"   ($cli tui: every session at once)"
say "  the service:       scripts/user-service.sh${UNIT_ARGS[*]:+ ${UNIT_ARGS[*]}} status, logs, or restart (docs/user-service.md)"
say "  after a git pull:  this same command again: it builds, installs, and restarts what changed"
