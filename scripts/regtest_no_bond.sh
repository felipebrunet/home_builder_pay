#!/usr/bin/env bash
# El contratista no pone boleta: el mandante no puede dejar locked una partida.
set -euo pipefail
source "$(cd "$(dirname "$0")" && pwd)/regtest_lib.sh"

DEMO="${DEMO_DIR:-/tmp/hbp-regtest-no-bond}"
rm -rf "$DEMO"
mkdir -p "$DEMO"
cd "$DEMO"

log "Wallets + contrato (sin fondeo 2-de-2)"
prepare_wallets
NOW="$(date +%s)"
T1=$((NOW + 120))
hbp_offer_60k "$T1" $((NOW + 240)) $((NOW + 360)) "demo no-bond"

log "Mandante intenta fondear solo la partida 1 (el contratista no aporta boleta)"
P1_TXID="$(wrpc hbp_mandante sendtoaddress "$P1_ADDR" "$PARTIDA_BTC")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
P1_HEX="$(rpc getrawtransaction "$P1_TXID")"
echo "p1-only tx $P1_TXID"

set +e
$HBP --dir .m verify-funding --tx-hex "$P1_HEX" --partida 1 >/tmp/hbp-nobond-full.out 2>/tmp/hbp-nobond-full.err
FULL_RC=$?
$HBP --dir .m verify-funding --tx-hex "$P1_HEX" --partida 1 --partida-only >/tmp/hbp-nobond-po.out 2>/tmp/hbp-nobond-po.err
PO_RC=$?
set -e
echo "verify-funding (espera bond+p1) rc=$FULL_RC"
cat /tmp/hbp-nobond-full.err
echo "verify-funding --partida-only rc=$PO_RC"
cat /tmp/hbp-nobond-po.err
[[ $FULL_RC -ne 0 && $PO_RC -ne 0 ]]
echo "status: $($HBP --dir .m status | python3 -c 'import json,sys; p=json.load(sys.stdin); print(p["status"], p["bond"]["status"])')"

log "El envio a la address Taproot SI existe on-chain. Tras T el mandante la recupera (unwind)."
P1_VOUT="$(vout_of "$P1_TXID" "$P1_ADDR")"
rpc setmocktime $((T1 + 600))
rpc generatetoaddress 12 "$MINE_ADDR" >/dev/null
M_REFUND="$(wrpc hbp_mandante getnewaddress)"
UNW="$($HBP --dir .m unwind --kind partida --partida 1 \
  --outpoint "${P1_TXID}:${P1_VOUT}" --sats "$PARTIDA_SATS" \
  --dest "$M_REFUND" --fee "$FEE_SATS" 2>/tmp/hbp-nobond-unw.err)"
cat /tmp/hbp-nobond-unw.err >&2
UNW_TXID="$(rpc sendrawtransaction "$UNW")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
rpc setmocktime 0 >/dev/null || true
echo "recovered $UNW_TXID"
log "DEMO NO-BOND OK"
