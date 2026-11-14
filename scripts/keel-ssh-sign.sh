#!/usr/bin/env bash
# git's ssh signing program for the owner's acknowledgement (issue 15): scripts/keel-ack.sh passes it as
# `-c gpg.ssh.program=<this file>` for that one commit, never as config.
#
# The owner's key is an ssh key held by 1Password on the Windows side of a WSL machine. Linux `ssh-keygen` inside WSL
# does not reach 1Password's agent; Windows' own ssh-keygen does, and 1Password asks him to approve each signature.
# So a signature (`-Y sign`) on WSL, when the Windows program exists, is made by it:
#   - the files git names (the public key after -f, and the buffer to sign, the last argument) are copied into a
#     directory Windows can read, and given to it as Windows paths (`wslpath -w`);
#   - `-U` is left out: given a public key file, ssh-keygen signs through the agent anyway, and that is the form
#     measured on this machine;
#   - the signature it writes beside the copy (`<buffer>.sig`) is copied back beside git's buffer, where git reads it.
# Everything else (verification, and any machine that is not WSL or has no Windows program) is plain `ssh-keygen`.
#
# What a WSL machine needs: 1Password's ssh agent on, holding the key; Windows' own "OpenSSH Authentication Agent"
# service stopped (it otherwise holds the agent's place); Windows OpenSSH at the path below.
#
# For tests, and for another layout:
#   KEEL_SSH_WINDOWS  the Windows ssh-keygen, as a Linux path (default /mnt/c/Windows/System32/OpenSSH/ssh-keygen.exe)
#   KEEL_SSH_WINTMP   a Linux path to a directory Windows can read and write (default: Windows' %TEMP%)
#   KEEL_SSH_WSL      1 or 0, to say whether this is WSL (default: read from the kernel's release)
set -u
win=${KEEL_SSH_WINDOWS:-/mnt/c/Windows/System32/OpenSSH/ssh-keygen.exe}
wsl=${KEEL_SSH_WSL:-}
if [ -z "$wsl" ]; then
  case "$(cat /proc/sys/kernel/osrelease 2>/dev/null)" in *[Mm]icrosoft*) wsl=1 ;; *) wsl=0 ;; esac
fi
if [ "${1:-} ${2:-}" != "-Y sign" ] || [ "$wsl" != 1 ] || [ ! -x "$win" ]; then
  exec ssh-keygen "$@"
fi

wintmp=${KEEL_SSH_WINTMP:-}
if [ -z "$wintmp" ]; then
  t=$(cmd.exe /c 'echo %TEMP%' 2>/dev/null | tr -d '\r')
  [ -n "$t" ] && wintmp=$(wslpath -u "$t")
fi
if [ -z "$wintmp" ] || [ ! -d "$wintmp" ]; then
  echo "keel-ssh-sign: no directory Windows can read (KEEL_SSH_WINTMP)" >&2
  exit 1
fi
dir=$(mktemp -d "$wintmp/keel-sign.XXXXXX") || exit 1
trap 'rm -rf "$dir"' EXIT

args=() buf=""
n=$#
i=0
while [ $# -gt 0 ]; do
  i=$((i + 1))
  case "$1" in
  -U) shift; continue ;;
  -f)
    cp "$2" "$dir/key.pub" || exit 1
    args+=(-f "$(wslpath -w "$dir/key.pub")")
    i=$((i + 1))
    shift 2
    continue
    ;;
  esac
  if [ "$i" -eq "$n" ] && [ -f "$1" ]; then
    buf=$1
    cp "$buf" "$dir/buffer" || exit 1
    args+=("$(wslpath -w "$dir/buffer")")
  else
    args+=("$1")
  fi
  shift
done
[ -n "$buf" ] || { echo "keel-ssh-sign: no file to sign" >&2; exit 1; }

"$win" "${args[@]}"
rc=$?
[ "$rc" -eq 0 ] || exit "$rc"
cp "$dir/buffer.sig" "$buf.sig" || { echo "keel-ssh-sign: the Windows program wrote no signature" >&2; exit 1; }
