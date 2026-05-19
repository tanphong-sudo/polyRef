#!/usr/bin/env bash
# Drift check between hand-written Rust types in polyref-core and the JSON
# Schema package. Slice 1 stub; full implementation lands when hard blocker
# F-8 is resolved (codegen vs hand-written + drift check).
#
# Currently the script confirms that the Rust enum members for Outcome,
# UnknownReason, BrokenReason, ArtifactKind, Language, and CorrespondenceKind
# match the schema enums character-for-character.
#
# Run from polyref/.

set -euo pipefail

SCHEMAS=schemas
CORE=crates/polyref-core/src

if [ ! -d "${SCHEMAS}" ] || [ ! -d "${CORE}" ]; then
  echo "ERROR: run from polyref/ root" >&2
  exit 2
fi

python3 - <<'PY'
import json
import re
import sys
from pathlib import Path

# Map: schema enum file -> Rust source file declaring matching variants.
PAIRS = {
    "schemas/unknown-reason.json": "crates/polyref-core/src/status.rs",
    "schemas/broken-reason.json": "crates/polyref-core/src/status.rs",
    "schemas/validation-status.json": "crates/polyref-core/src/status.rs",
    "schemas/artifact-kind.json": "crates/polyref-core/src/artifact_kind.rs",
    "schemas/language.json": "crates/polyref-core/src/language.rs",
    "schemas/correspondence-kind.json": "crates/polyref-core/src/correspondence_kind.rs",
}

def screaming_snake(s: str) -> str:
    return s.upper()

def pascal(s: str) -> str:
    return "".join(p.capitalize() for p in s.split("_"))

bad = 0
for schema_path, rust_path in PAIRS.items():
    sp = Path(schema_path)
    rp = Path(rust_path)
    if not sp.exists() or not rp.exists():
        print(f"SKIP  {schema_path}: missing pair", file=sys.stderr)
        continue
    schema = json.loads(sp.read_text(encoding="utf-8"))
    src = rp.read_text(encoding="utf-8")
    enum = schema.get("enum") or []
    missing = [v for v in enum if pascal(v) not in src]
    if missing:
        bad += 1
        print(f"FAIL  {schema_path}: variants missing in {rust_path}: {missing}", file=sys.stderr)
    else:
        print(f"OK    {schema_path} ↔ {rust_path}")

sys.exit(1 if bad else 0)
PY
