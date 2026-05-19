#!/usr/bin/env bash
# Validate every JSON Schema file under schemas/ against Draft 2020-12.
# Uses Python + jsonschema if available; falls back to plain JSON parse if
# jsonschema is not installed (which still catches encoding / syntax bugs).
#
# Slice 1 acceptance gate.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
SCHEMAS_DIR="${ROOT_DIR}/schemas"

if [ ! -d "${SCHEMAS_DIR}" ]; then
  echo "ERROR: schemas/ directory not found at ${SCHEMAS_DIR}" >&2
  exit 2
fi

POLYREF_SCHEMAS_DIR="${SCHEMAS_DIR}" python3 -c '
import json, os, sys
from pathlib import Path

root = Path(os.environ["POLYREF_SCHEMAS_DIR"])
if not root.exists():
    print(f"ERROR: schemas/ not found at {root}", file=sys.stderr)
    sys.exit(2)

count = 0
errors = 0
parsed = {}

for path in sorted(root.rglob("*.json")):
    count += 1
    try:
        text = path.read_text(encoding="utf-8")
        parsed[str(path)] = json.loads(text)
    except Exception as exc:
        errors += 1
        print(f"FAIL  {path}: {exc}", file=sys.stderr)

try:
    from jsonschema import Draft202012Validator
except Exception:
    print(f"OK    {count} schema files parsed (jsonschema not installed; install for full Draft 2020-12 validation)")
    sys.exit(0 if errors == 0 else 1)

for path, doc in parsed.items():
    try:
        Draft202012Validator.check_schema(doc)
    except Exception as exc:
        errors += 1
        print(f"FAIL  {path}: {exc}", file=sys.stderr)

if errors:
    print(f"FAIL  {errors} schema(s) invalid", file=sys.stderr)
    sys.exit(1)
print(f"OK    {count} schema files validate under Draft 2020-12")
'
