#!/bin/bash
# The oracle: the one job, waited for.
set -euo pipefail
APP="${ASYNC_ROOT:-}/app"
build-index | sed -n 's/^index built: //p' > "$APP/index-token.txt"
