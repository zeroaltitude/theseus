#!/usr/bin/env bash
# The daemon as a systemd user service (theseus-w1nf): one script to check the machine, install the
# unit, and run it day to day. `theseusd install --user` writes the unit; this script adds the checks
# before it and the steps after it. The how-to is docs/user-service.md.
#
#   user-service.sh check       read-only: is this machine ready, and what is running now?
#   user-service.sh install     check, show the plan, write the unit, start it, prove it answers
#   user-service.sh status      systemctl --user status theseusd
#   user-service.sh logs        journalctl --user -u theseusd -f   (more arguments go to journalctl)
#   user-service.sh restart     systemctl --user restart theseusd
#   user-service.sh stop        systemctl --user stop theseusd
#   user-service.sh start       systemctl --user start theseusd
#   user-service.sh uninstall   stop the service and remove its unit; your store is not touched
#
# Options, before or after the command:
#   --dry-run              print each command instead of running it; questions are assumed "yes"
#   --yes, -y              answer yes to every question
#   --op-token-file FILE   the file that holds the 1Password service-account token (default:
#                          $THESEUS_OP_TOKEN_FILE, else the file the installed unit names)
#   --help, -h
#
# The daemon's own variables are read the way `theseusd install --user` reads them, and the plan
# writes this shell's values into the unit: THESEUS_CONFIG, THESEUS_STATE_DIR, THESEUS_SOCKET, and
# THESEUS_OP_TOKEN_FILE. Run it from the shell you start the daemon in today. `check` reads the config
# the unit would get from the plan's `config:` line, not from THESEUS_CONFIG alone: a build's built-in
# default can be a file you do not have, and then the check fails until THESEUS_CONFIG names your note.
#
# It never prints a secret. The token file is only checked with `stat`: it is never opened. Every
# wait is bounded, and a daemon is found by its socket answering `theseus health`, never by
# matching processes. Exit status: 0 done (or ready); 1 a check or a step failed; 2 a usage error.
set -u
set -o pipefail
exec 3>&1 # the terminal: "+ command" lines must reach it from inside a $(...)

UNIT=theseusd.service
SELF=$0
ME=$(id -un)
MY_UID=$(id -u)
: "${HOME:?HOME is not set}"
case ${XDG_CONFIG_HOME:-} in
/*) CONFIG_HOME=$XDG_CONFIG_HOME ;;
*) CONFIG_HOME=$HOME/.config ;;
esac
UNIT_FILE=$CONFIG_HOME/systemd/user/$UNIT
SOCKET=${THESEUS_SOCKET:-$HOME/.theseus/theseus.sock}
TOKEN_FILE=${THESEUS_OP_TOKEN_FILE:-}
DRY=0
YES=0
FAILS=0
WARNS=0
UNIT_PRESENT=0   # 1 installed; 2 unknown (a dry run)
UNIT_STATE=unknown
SYSTEMD_STATE=unknown
LINGER=unknown # yes | no | unknown | maybe (a dry run)
HAND=0         # 1 a daemon started by hand answers on the socket; 2 maybe (a dry run)
TOKEN_ARGS=()
REST=()

usage() {
  cat <<EOF
usage: $SELF [--dry-run] [--yes] [--op-token-file FILE] <command>

  check       read-only: is this machine ready, and what is running now?
  install     check, show the plan, write the unit, start it, prove it answers
  status      systemctl --user status theseusd
  logs        journalctl --user -u theseusd -f   (more arguments go to journalctl)
  restart     systemctl --user restart theseusd
  stop        systemctl --user stop theseusd
  start       systemctl --user start theseusd
  uninstall   stop the service and remove its unit; your store is not touched

  --dry-run   print each command instead of running it
  --yes       answer yes to every question
  --op-token-file FILE
              the file that holds the 1Password service-account token (default:
              \$THESEUS_OP_TOKEN_FILE, else the file the installed unit names)

The how-to: docs/user-service.md
EOF
}

# --- saying and running ---

say() { printf '%s\n' "$*"; }
dry() { [ "$DRY" = 1 ]; }
# done_say TEXT: what came out of it, once it ran (a dry run says what would have).
done_say() {
  if dry; then say "(dry run) $*"; else say "$*"; fi
}
line() { printf '  %-5s %s\n' "$1" "$2"; }
hint() { printf '        %s\n' "$*"; }
ok() { line ok "$*"; }
info() { line info "$*"; }
warn() {
  line WARN "$*"
  WARNS=$((WARNS + 1))
}
fail() {
  line FAIL "$*"
  FAILS=$((FAILS + 1))
}

# A word as a shell would take it back: plain characters as they are, anything else in quotes.
shq() {
  case $1 in
  "" | *[!A-Za-z0-9_./:@%+=,-]*) printf "'%s'" "${1//\'/\'\\\'\'}" ;;
  *) printf '%s' "$1" ;;
  esac
}

# A command as a shell would take it back.
cmd_text() {
  local out="" a
  for a in "$@"; do out+=" $(shq "$a")"; done
  printf '%s' "${out# }"
}
show() { printf '+ %s\n' "$(cmd_text "$@")" >&3; }
note() { printf '  # %s\n' "$*" >&3; }

# run CMD…: say it, then run it (a dry run only says it).
run() {
  show "$@"
  dry && return 0
  "$@"
}

# probe CMD…: a read-only command whose output is wanted; its stderr joins its output. A dry run
# prints it and returns 99, which the caller reads as "unknown".
probe() {
  if dry; then
    show "$@"
    return 99
  fi
  "$@" 2>&1
}

# ask QUESTION: a yes/no question; no unless the answer starts with y.
ask() {
  local reply
  if dry; then
    say "? $1 [y/N] (dry run: assumed yes)"
    return 0
  fi
  if [ "$YES" = 1 ]; then
    say "? $1 [y/N] y (--yes)"
    return 0
  fi
  if [ ! -t 0 ]; then
    say "? $1 [y/N] no: there is no terminal to ask (--yes answers yes)"
    return 1
  fi
  read -r -p "? $1 [y/N] " reply
  case $reply in y | Y | yes | YES | Yes) return 0 ;; *) return 1 ;; esac
}

# --- the unit, the token, the socket ---

detect_unit() {
  probe test -e "$UNIT_FILE" >/dev/null
  case $? in
  0) UNIT_PRESENT=1 ;;
  99) UNIT_PRESENT=2 ;;
  *) UNIT_PRESENT=0 ;;
  esac
}

# exec_flag TEXT FLAG: the word after FLAG in TEXT, the installed unit's command line as systemd
# reads it back. A path with a space in it is not read back.
exec_flag() {
  local text=$1 word take=0 words
  text=${text#*argv\[\]=}
  text=${text%% ; *}
  read -r -a words <<<"$text"
  for word in ${words[@]+"${words[@]}"}; do
    if [ $take = 1 ]; then
      printf '%s\n' "$word"
      return 0
    fi
    [ "$word" = "$2" ] && take=1
  done
  return 1
}

# The socket and the token file: what the options and variables say, else what the unit names.
resolve_from_unit() {
  local text v
  [ "$UNIT_PRESENT" = 0 ] && return 0
  text=$(probe systemctl --user show --property=ExecStart --value "$UNIT") || return 0
  if [ -z "${THESEUS_SOCKET:-}" ] && v=$(exec_flag "$text" --socket); then SOCKET=$v; fi
  if [ -z "$TOKEN_FILE" ] && v=$(exec_flag "$text" --op-token-file); then TOKEN_FILE=$v; fi
  return 0
}

# --- the checks ---

check_platform() {
  local os
  os=$(probe uname -s)
  [ $? = 99 ] && return
  if [ "$os" != Linux ]; then
    fail "platform: $os (this needs Linux with systemd)"
  elif [ "$MY_UID" = 0 ]; then
    fail "platform: you are root. This installs your own user service: run it as yourself, without sudo"
  else
    ok "platform: Linux, as $ME"
  fi
}

check_systemd() {
  local state
  state=$(probe systemctl --user is-system-running)
  [ $? = 99 ] && return
  SYSTEMD_STATE=$state
  case $state in
  running) ok "systemd: your user manager is running" ;;
  degraded) ok "systemd: your user manager is running (degraded: a unit of yours failed; systemctl --user --failed)" ;;
  starting | initializing) warn "systemd: your user manager is still starting; try again in a moment" ;;
  *)
    fail "systemd: no user manager answers (systemctl --user is-system-running said: $(printf '%s' "$state" | head -n 1))"
    if [ -z "${XDG_RUNTIME_DIR:-}" ]; then
      hint "XDG_RUNTIME_DIR is not set: this is not a full login session. Log in again, or try: export XDG_RUNTIME_DIR=/run/user/$MY_UID"
    fi
    hint "on WSL: put systemd=true under [boot] in /etc/wsl.conf, then run \`wsl --shutdown\` from Windows"
    ;;
  esac
}

check_linger() {
  local v
  v=$(probe loginctl show-user "$ME" --property=Linger --value)
  if [ $? = 99 ]; then
    LINGER=maybe
    return
  fi
  case $v in
  yes)
    LINGER=yes
    ok "linger: on for $ME (the service keeps running when you log out)"
    ;;
  no)
    LINGER=no
    warn "linger: off for $ME: your user manager, and the daemon, stop when your last session ends"
    hint "turn it on: loginctl enable-linger $ME (install asks)"
    ;;
  *) warn "linger: could not tell (loginctl said: $(printf '%s' "$v" | head -n 1))" ;;
  esac
}

check_wsl() {
  local rc
  probe grep -qi microsoft /proc/sys/kernel/osrelease >/dev/null
  rc=$?
  if [ $rc = 99 ]; then
    probe grep -Eiq '^[[:space:]]*systemd[[:space:]]*=[[:space:]]*true' /etc/wsl.conf >/dev/null
    return
  fi
  [ $rc = 0 ] || return 0 # not WSL: nothing to say
  if probe grep -Eiq '^[[:space:]]*systemd[[:space:]]*=[[:space:]]*true' /etc/wsl.conf >/dev/null; then
    ok "WSL: /etc/wsl.conf has systemd=true"
  elif [ "$SYSTEMD_STATE" = running ] || [ "$SYSTEMD_STATE" = degraded ]; then
    info "WSL: /etc/wsl.conf has no systemd=true, yet systemd runs (another setting turned it on)"
  else
    fail "WSL: /etc/wsl.conf has no systemd=true"
    hint "add it under [boot] (a line \`[boot]\`, then \`systemd=true\`), then run \`wsl --shutdown\` from Windows"
  fi
}

check_binaries() {
  local exe version
  exe=$(probe command -v theseusd)
  if [ $? = 99 ]; then
    probe theseusd --version >/dev/null
    probe command -v theseus >/dev/null
    probe command -v op >/dev/null
    return
  fi
  if [ -z "$exe" ]; then
    fail "theseusd: not on PATH"
    hint "install it: scripts/build.sh --profile release-thin, then copy it into ~/.local/bin (AGENTS.md, \"Installing\")"
  else
    version=$(probe theseusd --version | head -n 1)
    case $exe in
    */target/*) warn "theseusd: $exe ($version) is a build-tree binary: the next rebuild replaces what the unit runs" ;;
    *) ok "theseusd: $exe ($version)" ;;
    esac
  fi
  exe=$(probe command -v theseus)
  if [ -n "$exe" ]; then ok "theseus: $exe"; else fail "theseus: not on PATH (it asks the daemon for its health, and stops one you started by hand)"; fi
  exe=$(probe command -v op)
  if [ -n "$exe" ]; then
    ok "op: $exe (the unit's PATH is this shell's, so the daemon finds it)"
  else
    fail "op: the 1Password CLI is not on PATH: the daemon cannot read the vault without it"
  fi
}

# The token file, by `stat` alone: a regular file, yours, mode 0600 or stricter, and not empty. The
# same rules as the plan's (`theseusd install --user`).
check_token() {
  local f=${TOKEN_FILE:-FILE} fields type owner mode size bits p problems=()
  if [ -z "$TOKEN_FILE" ] && ! dry; then
    fail "token file: none named. The daemon in the unit reads the 1Password service-account token from a file."
    hint "make one only you can read, for example (the token never appears on a command line):"
    hint "  install -d -m 700 ~/.config/theseus && (umask 077; printf %s \"\$OP_SERVICE_ACCOUNT_TOKEN\" > ~/.config/theseus/op-token)"
    hint "then add: --op-token-file ~/.config/theseus/op-token (or export THESEUS_OP_TOKEN_FILE=…)"
    return
  fi
  fields=$(probe stat -c '%F|%u|%a|%s' -- "$f")
  case $? in
  99) return ;;
  0) ;;
  *)
    case $fields in
    *"No such file"*)
      fail "token file: $f does not exist"
      hint "make it with mode 0600, holding the service-account token (docs/user-service.md shows how)"
      ;;
    *) fail "token file: $f cannot be examined ($(printf '%s' "$fields" | head -n 1))" ;;
    esac
    return
    ;;
  esac
  IFS='|' read -r type owner mode size <<<"$fields"
  case $type in
  "regular file" | "regular empty file") ;;
  *)
    fail "token file: $f is not a regular file ($type): name the file itself"
    return
    ;;
  esac
  [ "$owner" = "$MY_UID" ] || problems+=("owned by uid $owner, not by you ($ME): sudo chown $ME $f")
  bits=$((8#$mode))
  if [ $((bits & ~8#600)) != 0 ]; then
    problems+=("mode $(printf '%04o' "$bits") is looser than 0600: chmod 600 $f")
  elif [ $((bits & 8#400)) = 0 ]; then
    problems+=("mode $(printf '%04o' "$bits") does not let you read it: chmod 600 $f")
  fi
  [ "$size" != 0 ] || problems+=("it is empty: put the 1Password service-account token in it")
  if [ ${#problems[@]} = 0 ]; then
    ok "token file: $f (yours, mode $(printf '%04o' "$bits"), not empty)"
    return
  fi
  fail "token file: $f"
  for p in "${problems[@]}"; do hint "$p"; done
}

# plan_config TEXT: the value on the plan's `config:` line, which is the config the unit would get.
plan_config() {
  local l
  while IFS= read -r l; do
    if [[ $l =~ ^[[:space:]]+config:[[:space:]]+(.*[^[:space:]])[[:space:]]*$ ]]; then
      printf '%s\n' "${BASH_REMATCH[1]}"
      return 0
    fi
  done <<<"$1"
  return 1
}

# The config the unit would get: what the plan (`theseusd install --user`) prints on its `config:` line,
# which is THESEUS_CONFIG, else this build's built-in default, made absolute. The variable alone does not
# say: a build's default can be a file you do not have. A file must be readable; a vault reference must
# have a vault, an item, and a field.
check_config() {
  local plan ref first built_in=""
  token_args
  if dry; then
    probe timeout 20 theseusd ${TOKEN_ARGS[@]+"${TOKEN_ARGS[@]}"} install --user >/dev/null
    note "its \`config:\` line is the config the unit would get: a file is checked with test, a reference by its shape"
    return
  fi
  command -v theseusd >/dev/null 2>&1 || {
    info "config: not checked: theseusd is not on PATH, so there is no plan to read it from"
    return
  }
  plan=$(probe timeout 20 theseusd ${TOKEN_ARGS[@]+"${TOKEN_ARGS[@]}"} install --user)
  if ! ref=$(plan_config "$plan"); then
    first=$(printf '%s' "$plan" | head -n 1)
    fail "config: the plan names none (theseusd install --user said: ${first:-nothing})"
    return
  fi
  [ -n "${THESEUS_CONFIG:-}" ] || built_in="THESEUS_CONFIG is not set here, so this is theseusd's built-in default"
  case $ref in
  op://*)
    if [[ $ref =~ ^op://[^/]+/[^/]+/.+$ ]]; then
      ok "config: $ref (a vault note: the daemon reads it with the token${built_in:+; $built_in})"
    else
      fail "config: $ref is not op://<vault>/<item>/<field>"
    fi
    ;;
  *)
    if [ -f "$ref" ] && [ -r "$ref" ]; then
      ok "config: $ref (a file${built_in:+; $built_in})"
    else
      fail "config: $ref is not a readable file (${built_in:-THESEUS_CONFIG names it})"
      hint "the unit would be written to read it, and the daemon would not start. Name your config's vault note instead,"
      hint "in the shell that runs this script (your profile keeps it):  export THESEUS_CONFIG=op://<vault>/<item>/notesPlain"
      hint "or write the file."
    fi
    ;;
  esac
}

check_unit() {
  local enabled
  case $UNIT_PRESENT in
  0) info "unit: not installed ($UNIT_FILE)" ;;
  1)
    enabled=$(probe systemctl --user is-enabled "$UNIT")
    case $enabled in enabled | disabled | static | masked | linked | indirect | generated | alias | enabled-runtime) ;; *) enabled=unknown ;; esac
    info "unit: installed and $enabled ($UNIT_FILE)"
    ;;
  2) probe systemctl --user is-enabled "$UNIT" >/dev/null ;;
  esac
}

# Who answers on the socket: found by `theseus health` within a bound, never by matching processes.
check_daemon() {
  UNIT_STATE=$(probe systemctl --user is-active "$UNIT")
  if [ $? = 99 ]; then
    UNIT_STATE=unknown
    probe timeout 5 theseus --socket "$SOCKET" health >/dev/null
    HAND=2
    return
  fi
  case $UNIT_STATE in active | inactive | failed | activating | deactivating | reloading) ;; *) UNIT_STATE=unknown ;; esac
  if probe timeout 5 theseus --socket "$SOCKET" health >/dev/null; then
    if [ "$UNIT_STATE" = active ]; then
      ok "daemon: the service's daemon answers on $SOCKET"
    else
      HAND=1
      info "daemon: a daemon you started by hand answers on $SOCKET (the service is $UNIT_STATE)"
    fi
  elif [ "$UNIT_STATE" = active ]; then
    warn "daemon: the service is active but nothing answers on $SOCKET (still starting? $SELF logs)"
  else
    info "daemon: none is running (nothing answers on $SOCKET)"
  fi
}

# check_all: every check; the exit status says whether the machine is ready.
check_all() {
  FAILS=0
  WARNS=0
  say "== check: is this machine ready?"
  say "this machine"
  check_platform
  check_systemd
  check_linger
  check_wsl
  say "the install"
  detect_unit
  resolve_from_unit
  check_binaries
  check_token
  check_config
  say "right now"
  check_unit
  check_daemon
  dry && return 0
  if [ $FAILS -gt 0 ]; then
    say "result: not ready: $FAILS problem(s) above ($WARNS warning(s))"
    return 1
  fi
  say "result: ready ($WARNS warning(s))"
}

# --- waiting, bounded ---

# wait_answers SECONDS: until a daemon answers on the socket.
wait_answers() {
  local i=0 max=$(($1 * 2))
  if dry; then
    show theseus --socket "$SOCKET" health
    note "repeated until it answers, for up to $1 s"
    return 0
  fi
  while [ $i -lt $max ]; do
    timeout 3 theseus --socket "$SOCKET" health >/dev/null 2>&1 && return 0
    sleep 0.5
    i=$((i + 1))
  done
  return 1
}

# wait_gone SECONDS: until nothing answers on the socket.
wait_gone() {
  local i=0 max=$(($1 * 2))
  if dry; then
    show theseus --socket "$SOCKET" health
    note "repeated until it stops answering, for up to $1 s"
    return 0
  fi
  while [ $i -lt $max ]; do
    timeout 3 theseus --socket "$SOCKET" health >/dev/null 2>&1 || return 0
    sleep 0.5
    i=$((i + 1))
  done
  return 1
}

# --- the commands ---

# TOKEN_ARGS: the words that name the token file, before the subcommand (the order every build of
# theseusd reads; an install from before theseus-w1nf rejects them after it).
token_args() {
  TOKEN_ARGS=()
  if [ -n "$TOKEN_FILE" ]; then
    TOKEN_ARGS=(--op-token-file "$TOKEN_FILE")
  elif dry; then
    TOKEN_ARGS=(--op-token-file FILE)
  fi
}

cheat_sheet() {
  local w=$((${#SELF} + 11)) # "uninstall", a space, and the script
  say
  say "theseusd now runs as a systemd user service ($UNIT_FILE)."
  say
  printf '  %-*s %s\n' "$w" "$SELF status" "is it up? (it also says when a newer binary is waiting for a restart)"
  printf '  %-*s %s\n' "$w" "$SELF logs" "follow its journal (Ctrl-C leaves the daemon running)"
  printf '  %-*s %s\n' "$w" "$SELF restart" "a clean stop, then a start: how a newly installed binary takes over"
  printf '  %-*s %s\n' "$w" "$SELF stop" "stop it; it starts again at your next login or boot"
  printf '  %-*s %s\n' "$w" "$SELF check" "is the machine still as it should be?"
  printf '  %-*s %s\n' "$w" "$SELF uninstall" "take the service away; the daemon and its store are yours as before"
  printf '  %-*s %s\n' "$w" "theseus health" "what the daemon says of itself"
}

# The unit, written from the plan, after a question.
write_unit() {
  local rc
  probe theseusd ${TOKEN_ARGS[@]+"${TOKEN_ARGS[@]}"} install --user --check >/dev/null
  rc=$?
  if [ $rc = 0 ]; then
    say "The unit already matches the plan: nothing to write."
  else
    ask "Write $UNIT_FILE as the plan shows?" || {
      say "Nothing was changed."
      return 2
    }
    run theseusd ${TOKEN_ARGS[@]+"${TOKEN_ARGS[@]}"} install --user --apply || return 1
  fi
  run systemctl --user daemon-reload
}

# A daemon started by hand holds the socket and the store: stop it, cleanly, after a question.
stop_hand_daemon() {
  say
  say "== the daemon you started by hand"
  dry && note "only if a daemon started by hand answers on $SOCKET"
  say "It holds the socket $SOCKET and the store, which the service's daemon needs."
  ask "Stop it with \`theseus shutdown\` (a clean stop), then start the service?" || {
    say "Not stopped. The unit is written but not started. When you have stopped it: $SELF start"
    return 2
  }
  run theseus --socket "$SOCKET" shutdown || return 1
  wait_gone 20 || {
    say "It still answers after 20 s. Stop it yourself, then run: $SELF start"
    return 1
  }
}

cmd_install() {
  local rc
  check_all || {
    say
    say "Not installing: fix the FAIL lines above, then run this again."
    return 1
  }
  token_args

  say
  say "== the plan: what theseusd would write (nothing is changed yet)"
  run theseusd ${TOKEN_ARGS[@]+"${TOKEN_ARGS[@]}"} install --user || {
    say
    say "The plan did not finish (the lines above say why): fix that, then run this again."
    return 1
  }

  say
  say "== the unit"
  write_unit
  rc=$?
  case $rc in 0) ;; 2) return 0 ;; *) return 1 ;; esac

  if [ "$LINGER" = no ] || [ "$LINGER" = maybe ]; then
    say
    say "== linger"
    dry && note "only if linger is off"
    if ask "Turn on linger for $ME, so the service keeps running when you log out?"; then
      run loginctl enable-linger "$ME" || return 1
    else
      say "Left off: the service stops when your last session ends."
    fi
  fi

  if [ "$HAND" != 0 ]; then
    stop_hand_daemon
    rc=$?
    case $rc in 0) ;; 2) return 0 ;; *) return 1 ;; esac
  fi

  say
  say "== start it"
  run systemctl --user enable --now "$UNIT" || return 1
  wait_answers 30 || {
    say "Nothing answers on $SOCKET after 30 s."
    run systemctl --user status "$UNIT" --no-pager --lines 15
    say "More: $SELF logs"
    return 1
  }
  if dry; then
    say "(dry run) Then it would show the daemon's first lines of health:"
    show theseus --socket "$SOCKET" health
    note "its first lines"
  else
    say "It answers. What the daemon says of itself (secrets show \`resolving\` for a moment, then \`ready\`):"
    theseus --socket "$SOCKET" health | sed -n '1,8p'
  fi
  cheat_sheet
}

cmd_uninstall() {
  need_theseusd || return 1
  say "== the plan: what would be removed (nothing is changed yet)"
  run theseusd install --user --remove || return 1
  say
  # Only a unit that theseusd wrote is this script's to take away; the plan says "keep" for any other.
  probe theseusd install --user --remove --check >/dev/null
  if [ $? = 0 ]; then
    say "Nothing to remove: no unit that theseusd wrote is installed. Nothing was changed."
    return 0
  fi
  if ! ask "Stop the service (a clean stop), remove its unit, and leave your state dir as it is?"; then
    say "Nothing was changed."
    return 0
  fi
  detect_unit
  if [ "$UNIT_PRESENT" != 0 ]; then
    dry && note "only if the unit is installed"
    run systemctl --user disable --now "$UNIT" || return 1
  fi
  run theseusd install --user --remove --apply || return 1
  run systemctl --user daemon-reload || return 1
  say
  done_say "Removed. Your state dir and store were not touched. To run the daemon by hand again, start theseusd as before."
}

# need_theseusd: the plan and the removal are theseusd's.
need_theseusd() {
  dry && return 0
  command -v theseusd >/dev/null 2>&1 || {
    say "theseusd is not on PATH: install it first (scripts/build.sh --profile release-thin, then copy it into ~/.local/bin)"
    return 1
  }
}

# require_unit: the wrappers need a unit to act on, and use the socket it names.
require_unit() {
  detect_unit
  if [ "$UNIT_PRESENT" = 0 ]; then
    say "The unit is not installed ($UNIT_FILE). Run: $SELF install"
    return 1
  fi
  resolve_from_unit
}

# A daemon whose binary was replaced on disk keeps running the old image until it is restarted.
image_note() {
  local pid exe
  pid=$(probe systemctl --user show --property=MainPID --value "$UNIT")
  if [ $? = 99 ]; then
    probe readlink "/proc/<MainPID>/exe" >/dev/null
    return 0
  fi
  [ -n "$pid" ] && [ "$pid" != 0 ] || return 0
  exe=$(probe readlink "/proc/$pid/exe")
  case $exe in
  *" (deleted)") say "note: the running daemon's binary was replaced on disk since it started. $SELF restart runs the new one." ;;
  esac
}

cmd_status() {
  local rc
  require_unit || return 1
  run systemctl --user status "$UNIT" --no-pager
  rc=$?
  image_note
  return $rc
}

cmd_logs() {
  show journalctl --user -u "$UNIT" -f ${REST[@]+"${REST[@]}"}
  dry && return 0
  exec journalctl --user -u "$UNIT" -f ${REST[@]+"${REST[@]}"}
}

# cmd_service VERB: systemctl --user VERB; start and restart then wait for the daemon to answer.
cmd_service() {
  require_unit || return 1
  run systemctl --user "$1" "$UNIT" || return 1
  case $1 in
  start | restart)
    wait_answers 30 || {
      say "Nothing answers on $SOCKET after 30 s. $SELF status, or $SELF logs"
      return 1
    }
    # Something answers; is it the service's daemon? A daemon started by hand holds the store too.
    UNIT_STATE=$(probe systemctl --user is-active "$UNIT")
    if [ $? != 99 ] && [ "$UNIT_STATE" != active ]; then
      say "Something answers on $SOCKET, but the unit is $UNIT_STATE: a daemon you started by hand may hold the socket and the store."
      say "Stop it (theseus shutdown), then run: $SELF start"
      return 1
    fi
    done_say "It answers on $SOCKET."
    ;;
  stop) done_say "Stopped. It is still enabled, so it starts again at your next login or boot; $SELF uninstall takes it away." ;;
  esac
}

# --- main ---

CMD=""
while [ $# -gt 0 ]; do
  case $1 in
  --dry-run) DRY=1 ;;
  -y | --yes) YES=1 ;;
  --op-token-file)
    [ $# -ge 2 ] || {
      echo "$SELF: --op-token-file needs a file" >&2
      exit 2
    }
    TOKEN_FILE=$2
    shift
    ;;
  --op-token-file=*) TOKEN_FILE=${1#*=} ;;
  -h | --help)
    usage
    exit 0
    ;;
  --)
    shift
    REST+=("$@")
    break
    ;;
  *)
    if [ -n "$CMD" ]; then
      REST+=("$1")
    else
      case $1 in
      -*)
        echo "$SELF: unknown option $1 (--help lists them)" >&2
        exit 2
        ;;
      esac
      CMD=$1
    fi
    ;;
  esac
  shift
done
case $TOKEN_FILE in "~/"*) TOKEN_FILE=$HOME/${TOKEN_FILE#"~/"} ;; esac

if [ -n "$CMD" ] && [ "$CMD" != logs ] && [ ${#REST[@]} -gt 0 ]; then
  echo "$SELF: $CMD takes no further arguments (${REST[*]})" >&2
  exit 2
fi

if dry; then
  say "dry run: every command is printed with a leading +, and none is run"
  case $CMD in
  check | install) [ -n "$TOKEN_FILE" ] || say "FILE stands for the token file you will name with --op-token-file" ;;
  esac
fi
case $CMD in
check) check_all ;;
install) cmd_install ;;
uninstall) cmd_uninstall ;;
status) cmd_status ;;
logs) cmd_logs ;;
restart | stop | start) cmd_service "$CMD" ;;
"")
  usage >&2
  exit 2
  ;;
*)
  echo "$SELF: unknown command $CMD (--help lists them)" >&2
  exit 2
  ;;
esac
