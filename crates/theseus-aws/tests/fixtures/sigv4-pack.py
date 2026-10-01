"""Packs the SigV4 test suite that the aws-sigv4 crate ships (its
`aws-signing-test-suite/v4`, from awslabs' aws-c-auth, Apache-2.0) into one
JSON file for theseus-aws's tests.

    python3 sigv4-pack.py <aws-sigv4 crate dir> > sigv4-suite.json
"""
import json
import os
import sys

crate = sys.argv[1]
suite = os.path.join(crate, "aws-signing-test-suite", "v4")


def read(case, name):
    p = os.path.join(suite, case, name)
    if not os.path.exists(p):
        return None
    with open(p, encoding="utf-8") as f:
        return f.read().replace("\r\n", "\n")


cases = []
for case in sorted(os.listdir(suite)):
    ctx = read(case, "context.json")
    cases.append({
        "name": case,
        "context": json.loads(ctx) if ctx else None,
        "request": read(case, "request.txt"),
        "header_signed_request": read(case, "header-signed-request.txt"),
        "header_signature": (read(case, "header-signature.txt") or "").strip() or None,
        "query_signature": (read(case, "query-signature.txt") or "").strip() or None,
        "header_canonical_request": read(case, "header-canonical-request.txt"),
    })

print(json.dumps({
    "source": "aws-sigv4 " + os.path.basename(os.path.normpath(crate)).rsplit("-", 1)[-1]
              + ": aws-signing-test-suite/v4 (from aws-c-auth v0.9.0, Apache-2.0)",
    "cases": cases,
}, indent=1))
