#!/usr/bin/env bash
# Full catalog: unit tests + off-chain CLI + mined MAD/arbiter/unwind classes.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
echo "==== cargo test --workspace ===="
cargo test --workspace --quiet
echo "==== cli_catalog ===="
bash "$ROOT/scripts/cli_catalog.sh"
echo "==== regtest_catalog ===="
bash "$ROOT/scripts/regtest_catalog.sh"
echo "CATALOG_OK"
