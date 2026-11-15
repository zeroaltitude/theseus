#!/usr/bin/env bash
# gate-judged.sh LOG [REMOTE REF]: is the commit a gate judged the commit that is pushed? (issue 14)
#
# A gate's verdict names the commit it judged (`gate: judged commit <sha>, tree clean`), and a push is a separate act, so
# nothing recorded whether the two were the same. This reads the LAST such line in LOG and the remote's tip of REF
# (default: origin, and the branch this tree has checked out) and says which it is:
#   exit 0  SAME       the gate judged the commit that is pushed, on a clean tree
#   exit 1  DIFFERENT  the gate judged another commit, or a tree with edits beyond its commit: not this push's verdict
#   exit 2  UNKNOWN    the log names no judged commit, or the remote does not answer: nothing is shown either way
# A tree with edits beyond its commit is DIFFERENT even when the sha matches, since the edits are not in the push.
set -u
log=${1:-}
remote=${2:-origin}
if [ -z "$log" ] || [ ! -r "$log" ]; then
  echo "gate-judged: UNKNOWN: no readable gate log (usage: gate-judged.sh LOG [REMOTE REF])"
  exit 2
fi
ref=${3:-$(git rev-parse --abbrev-ref HEAD 2>/dev/null || true)}
line=$(grep -E '^gate: judged commit [0-9a-f]{7,64}, ' "$log" | tail -1 || true)
if [ -z "$line" ]; then
  echo "gate-judged: UNKNOWN: $log names no judged commit (a gate of before this line, or one that never began)"
  exit 2
fi
sha=$(sed -E 's/^gate: judged commit ([0-9a-f]+), .*/\1/' <<<"$line")
pushed=$(git ls-remote "$remote" "refs/heads/$ref" 2>/dev/null | cut -f1 | head -1)
if [ -z "$pushed" ]; then
  echo "gate-judged: UNKNOWN: $remote answers nothing for refs/heads/$ref, so what is pushed is not shown"
  exit 2
fi
if [ "$sha" != "$pushed" ]; then
  echo "gate-judged: DIFFERENT: the gate judged ${sha:0:12} and $remote/$ref is at ${pushed:0:12}: this gate is not the verdict for what is pushed"
  exit 1
fi
case $line in
  *"tree clean"*) echo "gate-judged: SAME: the gate judged ${sha:0:12}, on a clean tree, and that is what $remote/$ref holds" ;;
  *)
    echo "gate-judged: DIFFERENT: the gate judged ${sha:0:12} with edits beyond it, and the push holds only the commit"
    exit 1
    ;;
esac
