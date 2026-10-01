#!/usr/bin/env bash
# The offline checks for Theseus's CloudFormation (P4, theseus-mgw.4): cfn-lint over the four
# templates, the rules of test/rules.py (tags, encryption, retention, sizes, the guards), and the
# tests of the rules and of the TTL reaper. No AWS call is made.
#
# cfn-lint comes from THESEUS_CFN_LINT_VENV (default ~/.cache/theseus-cfn-lint, a venv with
# cfn-lint installed), or from PATH. The rules read the botocore models of the `aws` CLI on PATH,
# or of THESEUS_BOTOCORE_DATA.
set -euo pipefail
cd "$(dirname "$0")"
export PYTHONDONTWRITEBYTECODE=1
venv="${THESEUS_CFN_LINT_VENV:-$HOME/.cache/theseus-cfn-lint}"
if [ -x "$venv/bin/cfn-lint" ]; then
  cfn_lint="$venv/bin/cfn-lint"
  py="$venv/bin/python"
elif command -v cfn-lint >/dev/null; then
  cfn_lint="$(command -v cfn-lint)"
  py=python3
else
  echo "infra/aws: cfn-lint not found; set THESEUS_CFN_LINT_VENV" >&2
  exit 1
fi
echo "infra/aws: $("$cfn_lint" --version)"
"$cfn_lint" --regions us-west-2 --include-checks I -- theseus-*.yaml
"$py" test/rules.py theseus-*.yaml
"$py" -m unittest discover -s test -p 'test_*.py'
echo "infra/aws: ok"
