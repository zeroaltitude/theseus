#!/usr/bin/env bash
# The owner's acknowledgement of keel findings, given from chat (issue 15; the owner's "yes to all!" of 2026-11-14).
#
#   scripts/keel-ack.sh --quote WORDS --message ID [--when TIME] 'RULE PATH[, PATH...] — WHY' ['...' ...]
#
# The owner says yes in his DM; the agent runs this in the branch's worktree. It writes one empty commit whose
# trailers are the `Keel:` lines, each reason ending with his words and the message id, and signs it with the OWNER's
# key as scripts/keel-signers names it at main (origin/main, else main): the key the guard will check it against. So
# the signature is given by his own device, at the moment of signing (a fingerprint or face for a 1Password ssh key,
# a touch for a hardware key); this script never holds or reads a secret, and asking him in chat is only how the
# request reaches him. Run from a terminal he can see, or from an agent whose signing prompt reaches his screen.
#
# An agent's own acks (the `routine` class: test-removed) need none of this: an ordinary signed commit carries them.
#
# The owner's key, by its line in the list:
#   owner gpg <fingerprint>        git signs with gpg and that key
#   owner ssh <type> <key>         git signs with ssh, the key named literally, so the ssh agent that holds it
#                                  signs: 1Password's agent, or the operator's own
#   owner none                     no key yet: nothing is written, and it says so (exit 3)
#
# Exit status: 0 written and signed; 1 the commit or its signature failed; 2 usage; 3 no owner key at main.
set -u
quote="" message="" when="" acks=()
usage() {
  echo "usage: scripts/keel-ack.sh --quote WORDS --message ID [--when TIME] 'RULE PATH[, PATH...] — WHY' ..." >&2
  exit 2
}
while [ $# -gt 0 ]; do
  case "$1" in
  --quote | --message | --when) [ $# -ge 2 ] || usage ;;
  esac
  case "$1" in
  --quote) quote=$2; shift 2 ;;
  --message) message=$2; shift 2 ;;
  --when) when=$2; shift 2 ;;
  -h | --help) sed -n '2,22p' "$0"; exit 0 ;;
  *) acks+=("$1"); shift ;;
  esac
done
[ -n "$quote" ] && [ -n "$message" ] && [ "${#acks[@]}" -gt 0 ] || usage
nl='
'
case $quote in *'"'*) echo "keel-ack: the quote may not hold a double quote; it is quoted for you" >&2; exit 2 ;; esac
for v in "$quote" "$message" "$when" "${acks[@]}"; do
  case $v in *"$nl"*) echo "keel-ack: a newline would split the trailers: $v" >&2; exit 2 ;; esac
done
for a in "${acks[@]}"; do
  if ! printf '%s' "$a" | grep -qE '^[a-z]+(-[a-z]+)* [^ ]+(, [^ ]+)* (—|--|-) [^ ].*$'; then
    echo "keel-ack: not of the form 'RULE PATH[, PATH...] — WHY': $a" >&2
    exit 2
  fi
done

base=origin/main
git rev-parse -q --verify "$base^{commit}" >/dev/null || base=main
if ! list=$(git show "$base:scripts/keel-signers" 2>/dev/null); then
  echo "keel-ack: no scripts/keel-signers at $base" >&2
  exit 3
fi
# The owner's key as the guard reads it: an `owner gpg|ssh` line, or a bare fingerprint (the list's older form).
owner=$(printf '%s\n' "$list" | sed 's/#.*//' | awk '
  NF == 1 { print "gpg " $1; exit }
  $1 == "owner" && $2 != "none" { $1 = ""; sub(/^ /, ""); print; exit }')
case $owner in
gpg\ *) sign=(-c gpg.format=openpgp -c "user.signingkey=${owner#gpg }") ;;
ssh\ *)
  set -- ${owner#ssh }
  # A literal key: git 2.34 reads a value starting `ssh-` as one; `key::` is newer git's form for the other types.
  case $1 in ssh-*) lit="$1 $2" ;; *) lit="key::$1 $2" ;; esac
  sign=(-c gpg.format=ssh -c "user.signingkey=$lit")
  ;;
*)
  echo "keel-ack: scripts/keel-signers at $base names no owner key yet," >&2
  echo "keel-ack: so his acknowledgement cannot be signed. Nothing was written." >&2
  exit 3
  ;;
esac

cite="the owner${when:+, $when} (message $message): \"$quote\""
msg=$(mktemp)
trap 'rm -f "$msg"' EXIT
{
  echo "keel: the owner's acknowledgement"
  echo
  echo "Given by the owner in chat and signed with his key, as scripts/keel-ack.sh does."
  echo
  for a in "${acks[@]}"; do echo "Keel: $a; $cite"; done
} >"$msg"
if ! git "${sign[@]}" commit -q --allow-empty -S -F "$msg"; then
  echo "keel-ack: the commit or its signature failed; nothing was written" >&2
  exit 1
fi
echo "keel-ack: written and signed: $(git log -1 --format='%h %G?')"
