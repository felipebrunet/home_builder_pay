#!/usr/bin/env bash
# Mandante pide parar tras partida 1 cobrada; el contratista acepta.
# Liberan la boleta YA (MuSig2), sin esperar T.
set -euo pipefail
source "$(cd "$(dirname "$0")" && pwd)/regtest_lib.sh"

DEMO="${DEMO_DIR:-/tmp/hbp-regtest-cancel-after-p1}"
WAIT_SECS=5
rm -rf "$DEMO"
mkdir -p "$DEMO"
cd "$DEMO"

log "Wallets"
prepare_wallets
M_BEFORE="$(trusted hbp_mandante)"
C_BEFORE="$(trusted hbp_contratista)"
NOW="$(date +%s)"
hbp_offer_60k $((NOW + 120)) $((NOW + 240)) $((NOW + 360)) "demo cancel after p1"

FUND1_TXID="$(fund_bond_and_p1 "$BOND_ADDR" "$P1_ADDR" "cap1")"
FUND1_HEX="$(cat /tmp/hbp-cap1-fund1.hex)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
$HBP --dir .m verify-funding --tx-hex "$FUND1_HEX" --partida 1
$HBP --dir .c verify-funding --tx-hex "$FUND1_HEX" --partida 1
P1_VOUT="$(vout_of "$FUND1_TXID" "$P1_ADDR")"
BOND_VOUT="$(vout_of "$FUND1_TXID" "$BOND_ADDR")"

log "Partida 1 se hace bien (${WAIT_SECS}s) y se paga 100%"
sleep "$WAIT_SECS"
C_PAY="$(wrpc hbp_contratista getnewaddress)"
P1_HEX="$($HBP --dir .m coop-close --kind partida --partida 1 \
  --outpoint "${FUND1_TXID}:${P1_VOUT}" --sats "$PARTIDA_SATS" \
  --dest "$C_PAY" --fee "$FEE_SATS" --peer-dir .c 2>/tmp/hbp-cap1-p1.err)"
cat /tmp/hbp-cap1-p1.err >&2
P1_PAY="$(rpc sendrawtransaction "$P1_HEX")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
echo "p1 paid $P1_PAY"

log "Mandante pide cancelar el resto. Contratista acepta. Boleta MuSig2 ahora."
C_BOND="$(wrpc hbp_contratista getnewaddress)"
BOND_HEX="$($HBP --dir .c coop-close --kind bond \
  --outpoint "${FUND1_TXID}:${BOND_VOUT}" --sats "$BOND_SATS" \
  --dest "$C_BOND" --fee "$FEE_SATS" --peer-dir .m 2>/tmp/hbp-cap1-bond.err)"
cat /tmp/hbp-cap1-bond.err >&2
BOND_PAY="$(rpc sendrawtransaction "$BOND_HEX")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
echo "bond released now $BOND_PAY (no timeout)"

echo "mandante    $M_BEFORE -> $(trusted hbp_mandante) BTC"
echo "contratista $C_BEFORE -> $(trusted hbp_contratista) BTC"
$HBP --dir .m status | python3 -c 'import json,sys; p=json.load(sys.stdin); print(p["status"], p["bond"]["status"], [(x["id"], x["state"]["status"]) for x in p["partidas"]])'
python3 - <<PY
import json
json.dump({
  "contract_id": "$CID",
  "scenario": "cancel_after_p1_agreed",
  "p1_pay_txid": "$P1_PAY",
  "bond_release_txid": "$BOND_PAY",
  "p2_funded": False,
  "mandante_before": float("$M_BEFORE"),
  "mandante_after": float("$(trusted hbp_mandante)"),
  "contratista_before": float("$C_BEFORE"),
  "contratista_after": float("$(trusted hbp_contratista)"),
}, open("$DEMO/summary.json","w"), indent=2)
PY
log "DEMO CANCEL-AFTER-P1 OK"
