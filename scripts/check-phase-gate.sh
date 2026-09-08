#!/usr/bin/env bash
set -euo pipefail

case "${1:-}" in
  [0-9]|10) phase="$1" ;;
  *) echo "Usage: bash scripts/check-phase-gate.sh <0-10> [--check-only]" >&2; exit 2 ;;
esac
if [[ $# -gt 2 || ( $# -eq 2 && "$2" != "--check-only" ) ]]; then
  echo "Expected only a phase from 0 to 10 and optional --check-only" >&2
  exit 2
fi
cd "$(dirname "$0")/.."

# A green executable suite is not sufficient when normative scenarios are still open.
awk -F '|' -v phase="$phase" '
  /^\| / {
    responsible=$(NF-2); state=$(NF-1); identifier=$2
    gsub(/^[[:space:]]+|[[:space:]]+$/, "", responsible)
    gsub(/^[[:space:]]+|[[:space:]]+$/, "", state)
    gsub(/^[[:space:]]+|[[:space:]]+$/, "", identifier)
    if (responsible ~ /^[0-9]+$/ && responsible+0 <= phase+0 && state != "passing") {
      print identifier ": " state " (phase " responsible ")"; blocked=1
    }
  }
  END { exit blocked ? 1 : 0 }
' docs/test-matrix.md

if [[ "${2:-}" == "--check-only" ]]; then exit 0; fi
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo build --all-features
cargo test --all-features
