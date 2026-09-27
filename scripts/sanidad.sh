#!/usr/bin/env bash
# Toda la suite: tests del workspace y los casos de regtest (un solo monerod).
set -euo pipefail

raiz="$(cd "$(dirname "$0")/.." && pwd)"
cd "$raiz"

if [[ -z "${MONEROD:-}" ]] && ! command -v monerod >/dev/null 2>&1; then
  echo "Falta monerod. Exportá MONEROD=/ruta/monerod y volvé a correr este script." >&2
  exit 1
fi

echo "== tests del workspace =="
# dkg-pedpop vive dentro del repo y Cargo lo toma como miembro. Sus tests
# propios no son de Konstruado y no compilan acá.
cargo test --workspace --exclude dkg-pedpop

echo "== regtest =="
cargo test -p xmr-joint --test regtest_pago -- --ignored --test-threads=1 --nocapture
