#!/usr/bin/env bash
# The administrator's one-time step for "Theseus installs what its own loop joined" (issue 22): put the one-purpose
# helper `theseus-root-install` in a root-owned place and add ONE sudoers rule that lets the service's user run exactly
# that program, with a commit and nothing else, and no password. `docs/root-install.md` is the how-to, and
# `docs/design/self-install.md` says what this gives up. Run it as root (`sudo`), once, by the person who is the
# administrator of the machine. Nothing here is run by Theseus.
#
#   root-install.sh install --user NAME --helper FILE   install the helper, the directories and the rule
#   root-install.sh remove                              take the helper and the rule away again
#   root-install.sh check                               read-only: is it installed, and as it should be?
#
# Options:  --dry-run   print each command instead of running it
#           --yes, -y   do not ask
#           --root DIR  every path below DIR instead of `/`, run as a user other than root (for the script's own tests)
#
# What `install` makes, and `remove` takes away (`remove` takes away only what `install` made, and never a program
# directory the daemon may be running from):
#   /usr/local/sbin/theseus-root-install            the helper, root:root 0755
#   /usr/local/lib/theseus                          where the builds go, root:root 0755 (made if missing; never removed)
#   /var/lib/theseus-root-install                   root:root 0755
#   /var/lib/theseus-root-install/stage             the staging directory, NAME's, 0700 (the only place the helper reads)
#   /etc/sudoers.d/theseus-root-install             one rule, root:root 0440, checked with `visudo -cf` first
# Exit status: 0 done (or as it should be); 1 a check or a step failed; 2 a usage error.
set -u
set -o pipefail

CMD=""
USER_NAME=""
HELPER_SRC=""
DRY=0
YES=0
ROOT=""

usage() { sed -n '2,25p' "$0" | sed 's/^# \{0,1\}//'; }
die() { echo "root-install: $*" >&2; exit 1; }
usage_error() { echo "root-install: $*" >&2; echo "run it with --help" >&2; exit 2; }

while [ $# -gt 0 ]; do
  case $1 in
  install | remove | check) CMD=$1 ;;
  --user) USER_NAME=${2:-}; shift ;;
  --helper) HELPER_SRC=${2:-}; shift ;;
  --root) ROOT=${2:-}; shift ;;
  --dry-run) DRY=1 ;;
  --yes | -y) YES=1 ;;
  --help | -h) usage; exit 0 ;;
  *) usage_error "unknown argument: $1" ;;
  esac
  shift
done
[ -n "$CMD" ] || usage_error "say what to do: install, remove or check"

# Paths: below --root in a test, `/` otherwise.
HELPER=$ROOT/usr/local/sbin/theseus-root-install
LIB=$ROOT/usr/local/lib/theseus
STATE=$ROOT/var/lib/theseus-root-install
STAGE=$STATE/stage
SUDOERS=$ROOT/etc/sudoers.d/theseus-root-install
HEADER="# Written by scripts/root-install.sh (issue 22)."
# The rule's own pattern: exactly forty lowercase hex digits and no space, besides the helper's own check.
HEX40=$(printf '[0-9a-f]%.0s' $(seq 40))

as_root=1
if [ -n "$ROOT" ]; then as_root=0; fi
if [ "$as_root" = 1 ] && [ "$CMD" != check ] && [ "$(id -u)" != 0 ] && [ $DRY = 0 ]; then
  die "$CMD changes root-owned places: run it with sudo (or --dry-run to see what it would do)"
fi

say() { echo "$*"; }
run() { echo "+ $*"; [ $DRY = 1 ] || "$@" || die "step failed: $*"; }
# `install` with an owner only as root: a test runs as its own user.
own() { if [ "$as_root" = 1 ]; then echo "-o $1 -g $2"; fi; }

ask() {
  [ $YES = 1 ] || [ $DRY = 1 ] && return 0
  printf '%s [y/N] ' "$1"
  read -r a
  [ "$a" = y ] || [ "$a" = Y ]
}

# A directory on the way is not a link and not writable by anyone but root: the helper checks the same.
require_plain_dir() {
  local d=$1
  [ -e "$d" ] || return 0
  [ ! -L "$d" ] || die "$d is a link: refusing"
  [ -d "$d" ] || die "$d is not a directory"
  if [ "$as_root" = 1 ]; then
    [ "$(stat -c %u "$d")" = 0 ] || die "$d is not owned by root"
  fi
  local m
  m=$(stat -c %a "$d")
  [ $((8#$m & 8#022)) = 0 ] || die "$d is writable by its group or others (mode $m)"
}

check_install() {
  [ -n "$USER_NAME" ] || usage_error "install needs --user NAME (the service's user)"
  [[ $USER_NAME =~ ^[a-z_][a-z0-9_-]*$ ]] || usage_error "--user: not a user name: $USER_NAME"
  id "$USER_NAME" >/dev/null 2>&1 || die "no such user: $USER_NAME"
  [ "$(id -u "$USER_NAME")" != 0 ] || die "--user must not be root: the helper reads what that user wrote"
  [ -n "$HELPER_SRC" ] || usage_error "install needs --helper FILE (target/release/theseus-root-install)"
  [ -f "$HELPER_SRC" ] && [ -x "$HELPER_SRC" ] || die "--helper $HELPER_SRC is not an executable file"
  [ ! -L "$HELPER_SRC" ] || die "--helper $HELPER_SRC is a link"
  [ "$(head -c 4 "$HELPER_SRC")" = "$(printf '\177ELF')" ] || die "--helper $HELPER_SRC is not a program (no ELF header)"
  for d in "$ROOT/usr/local/sbin" "$ROOT/usr/local/lib" "$ROOT/var/lib" "$ROOT/etc/sudoers.d"; do
    [ -d "$d" ] || die "$d does not exist"
    require_plain_dir "$d"
  done
  require_plain_dir "$LIB"
  require_plain_dir "$STATE"
  if [ -e "$SUDOERS" ] && [ ! -r "$SUDOERS" ] && [ $DRY = 1 ]; then
    # The rule is mode 0440, root's: a dry run as another user cannot read its header, and says so rather than refusing.
    say "+ (the rule at $SUDOERS is not readable by this user, so a dry run does not check whose it is)"
  else
    [ ! -e "$SUDOERS" ] || head -n1 "$SUDOERS" | grep -qxF "$HEADER" || die "$SUDOERS exists and is not this script's: refusing"
  fi
}

rule_text() {
  printf '%s\n# The service user may run exactly this program, with a commit of forty hex digits, as root, and nothing else.\n' "$HEADER"
  # Not a rule: it only makes sudo clear the caller's environment (the loader's LD_* included) for this program, as it
  # does by default, in case a site turned that off.
  printf 'Defaults!/usr/local/sbin/theseus-root-install env_reset\n'
  printf '%s ALL=(root) NOPASSWD: NOSETENV: /usr/local/sbin/theseus-root-install %s\n' "$USER_NAME" "$HEX40"
}

do_install() {
  check_install
  local group
  group=$(id -gn "$USER_NAME")
  say "This puts the helper in /usr/local/sbin and lets $USER_NAME run it, and only it, as root without a password."
  say "What that gives up is in docs/design/self-install.md. It is undone by: $0 remove"
  ask "Go ahead?" || die "not done: you answered no"
  run install -d $(own root root) -m 0755 "$LIB"
  run install -d $(own root root) -m 0755 "$STATE"
  run install -d $(own "$USER_NAME" "$group") -m 0700 "$STAGE"
  # 0700 until the copy has been checked: a source swapped for a link to a root-readable file is not readable by anyone
  # else for the moment it takes to find out.
  run install $(own root root) -m 0700 "$HELPER_SRC" "$HELPER.new"
  if [ $DRY = 0 ]; then
    # What was checked above is the source; what is installed is this copy, in a root-owned directory, so it is the
    # copy that is checked again (a source swapped for a link since then is copied as the file it points at).
    if [ -L "$HELPER.new" ] || [ ! -f "$HELPER.new" ] || [ "$(head -c 4 "$HELPER.new")" != "$(printf '\177ELF')" ]; then
      rm -f "$HELPER.new"
      die "the copy of the helper is not a program: nothing was installed (build it in a directory the service user cannot write)"
    fi
  fi
  run chmod 0755 "$HELPER.new"
  run mv -f "$HELPER.new" "$HELPER"
  local tmp
  # As root the temporary file is made beside the rule, in a root-owned directory (sudo ignores a name with a dot in
  # it), never in a TMPDIR the caller named: a file the owner can replace between `visudo -cf` and `install` would be
  # a sudoers file of the owner's own.
  local tmpdir=${TMPDIR:-/tmp}
  [ "$as_root" = 1 ] && tmpdir=$ROOT/etc/sudoers.d
  if [ $DRY = 1 ]; then
    # A dry run writes nothing, not even the temporary file: it names the step and the rule it would check and install.
    tmp="$tmpdir/theseus-root-install.sudoers.XXXXXX"
    say "+ (write the rule below to $tmp, check it with visudo -cf, and install it)"
    rule_text | sed 's/^/+   /'
  else
    tmp=$(mktemp "$tmpdir/theseus-root-install.sudoers.XXXXXX") || die "no temporary file"
    rule_text >"$tmp" || die "could not write the temporary rule"
    if [ "$as_root" = 1 ]; then
      command -v visudo >/dev/null || { rm -f "$tmp"; die "visudo is not installed: the rule is not written unchecked"; }
      visudo -cf "$tmp" >/dev/null || { rm -f "$tmp"; die "visudo does not accept the rule: nothing written"; }
    fi
  fi
  run install $(own root root) -m 0440 "$tmp" "$SUDOERS"
  [ $DRY = 1 ] || rm -f "$tmp"
  [ $DRY = 1 ] || do_check || die "installed files failed verification"
  say "done. The daemon's health says the helper is installed once it has looked again."
  say "check it:  $0 check --user $USER_NAME      (and, as $USER_NAME: sudo -n -l)"
}

do_remove() {
  if [ -e "$SUDOERS" ]; then
    head -n1 "$SUDOERS" | grep -qxF "$HEADER" || die "$SUDOERS is not this script's: left alone"
    run rm -f "$SUDOERS"
  else
    say "no rule at $SUDOERS"
  fi
  if [ -e "$HELPER" ]; then run rm -f "$HELPER"; else say "no helper at $HELPER"; fi
  if [ -d "$STAGE" ]; then
    if [ -z "$(ls -A "$STAGE")" ]; then run rmdir "$STAGE"; else say "kept $STAGE: it holds staged programs; remove them yourself"; fi
  fi
  if [ -d "$STATE" ]; then
    if [ -z "$(ls -A "$STATE")" ]; then run rmdir "$STATE"; else say "kept $STATE: it is not empty"; fi
  fi
  say "kept $LIB and every build in it: the service may be running from one. Remove old ones by hand."
  if [ $DRY = 0 ] && { [ -e "$SUDOERS" ] || [ -L "$SUDOERS" ] || [ -e "$HELPER" ] || [ -L "$HELPER" ]; }; then
    die "removal did not remove the helper and rule"
  fi
  say "removed. The daemon says the helper is not installed once it has looked again."
}

do_check() {
  local bad=0
  ok() { say "  ok    $*"; }
  no() { say "  FAIL  $*"; bad=1; }
  if [ -f "$HELPER" ] && [ ! -L "$HELPER" ]; then ok "helper: $HELPER"; else no "helper: $HELPER is missing"; fi
  if [ -f "$SUDOERS" ] && head -n1 "$SUDOERS" | grep -qxF "$HEADER"; then
    ok "rule: $SUDOERS"
    [ "$(grep -v '^#' "$SUDOERS" | grep -vc '^Defaults!')" = 1 ] && ok "the file holds one rule" || no "the file holds more than one rule"
  else
    no "rule: $SUDOERS is missing or not this script's"
  fi
  if [ -d "$STAGE" ]; then ok "staging directory: $STAGE"; else no "staging directory: $STAGE is missing"; fi
  if [ -d "$LIB" ]; then ok "program directory: $LIB"; else no "program directory: $LIB is missing"; fi
  [ $bad = 0 ] && say "result: installed" || say "result: not installed as it should be"
  return $bad
}

case $CMD in
install) do_install ;;
remove) do_remove ;;
check) do_check ;;
esac
