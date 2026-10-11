#!/bin/bash
# The ledger checked as a record of itself, then the family's order rules and
# outcome: reward.json and the ledger's copy in the verifier's log directory
# (layer2.py check).
python3 "$(dirname "$0")/layer2.py" check api-summary
exit 0
