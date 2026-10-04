#!/usr/bin/env bash
# Tests del workspace. No habla con un daemon.
set -euo pipefail

raiz="$(cd "$(dirname "$0")/.." && pwd)"
cd "$raiz"

cargo test --workspace
