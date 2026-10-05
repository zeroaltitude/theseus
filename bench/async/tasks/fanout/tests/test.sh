#!/bin/bash
# The ledger checked as a record of itself, then the family's outcome: reward.json
# and the ledger's copy in the verifier's log directory (asyncbench.py check).
python3 "$(dirname "$0")/asyncbench.py" check fanout
exit 0
