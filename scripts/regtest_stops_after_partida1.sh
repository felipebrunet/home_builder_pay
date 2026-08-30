#!/usr/bin/env bash
# Partida 1 se cobra (camino feliz). El mandante no fondea la partida 2.
# El contratista recupera la boleta por unwind tras T_proyecto.
set -euo pipefail
# shellcheck source=regtest_lib.sh
source "$(cd "$(dirname "$0")" && pwd)/regtest_lib.sh"

DEMO="${DEMO_DIR:-/tmp/hbp-regtest-stop-p1}"
WAIT_SECS=5
rm -rf "$DEMO"
mkdir -p "$DEMO"
cd "$DEMO"

log "Wallets"
prepare_wallets
M_BEFORE="$(trusted hbp_mandante)"
C_BEFORE="$(trusted hbp_contratista)"

NOW="$(date +%s)"
T1=$((NOW + 8))
T2=$((NOW + 16))
TPROJ=$((NOW + 24))
echo "t1=$T1 t2=$T2 t_project=$TPROJ"

log "Contrato 60k / 2x30k / boleta 20k"
hbp_offer_60k "$T1" "$T2" "$TPROJ" "demo stop-after-p1"

log "Fondeo atomico boleta + partida 1"
FUND1_TXID="$(fund_bond_and_p1 "$BOND_ADDR" "$P1_ADDR" "stopp1")"
FUND1_HEX="$(cat /tmp/hbp-stopp1-fund1.hex)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
$HBP --dir .m verify-funding --tx-hex "$FUND1_HEX" --partida 1
$HBP --dir .c verify-funding --tx-hex "$FUND1_HEX" --partida 1
P1_VOUT="$(vout_of "$FUND1_TXID" "$P1_ADDR")"
BOND_VOUT="$(vout_of "$FUND1_TXID" "$BOND_ADDR")"

log "Obra partida 1 (${WAIT_SECS}s) y recepcion MuSig2"
sleep "$WAIT_SECS"
C_PAY="$(wrpc hbp_contratista getnewaddress)"
P1_HEX="$($HBP --dir .m coop-close --kind partida --partida 1 \
  --outpoint "${FUND1_TXID}:${P1_VOUT}" --sats "$PARTIDA_SATS" \
  --dest "$C_PAY" --fee "$FEE_SATS" --peer-dir .c 2>/tmp/hbp-stop-p1.err)"
cat /tmp/hbp-stop-p1.err >&2
P1_PAY_TXID="$(rpc sendrawtransaction "$P1_HEX")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
echo "partida 1 paid $P1_PAY_TXID"
echo "partida 2 NOT funded"

log "Mandante desaparece. Contratista espera T_proyecto y barre la boleta."
rpc setmocktime $((TPROJ + 600))
rpc generatetoaddress 12 "$MINE_ADDR" >/dev/null
C_BOND="$(wrpc hbp_contratista getnewaddress)"
BOND_HEX="$($HBP --dir .c unwind --kind bond \
  --outpoint "${FUND1_TXID}:${BOND_VOUT}" --sats "$BOND_SATS" \
  --dest "$C_BOND" --fee "$FEE_SATS" --peer-dir .m 2>/tmp/hbp-stop-bond.err)"
cat /tmp/hbp-stop-bond.err >&2
BOND_TXID="$(rpc sendrawtransaction "$BOND_HEX")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
rpc setmocktime 0 >/dev/null || true
echo "bond unwound $BOND_TXID"

echo "mandante    $M_BEFORE -> $(trusted hbp_mandante) BTC"
echo "contratista $C_BEFORE -> $(trusted hbp_contratista) BTC"
$HBP --dir .m status | python3 -c 'import json,sys; p=json.load(sys.stdin); print(p["status"], p["bond"]["status"], [(x["id"], x["state"]["status"]) for x in p["partidas"]])'

python3 - <<PY
import json
json.dump({
  "contract_id": "$CID",
  "scenario": "stops_after_partida1_paid",
  "fund1_txid": "$FUND1_TXID",
  "p1_pay_txid": "$P1_PAY_TXID",
  "bond_unwind_txid": "$BOND_TXID",
  "p2_funded": False,
  "mandante_before": float("$M_BEFORE"),
  "mandante_after": float("$(trusted hbp_mandante)"),
  "contratista_before": float("$C_BEFORE"),
  "contratista_after": float("$(trusted hbp_contratista)"),
}, open("$DEMO/summary.json","w"), indent=2)
print("summary $DEMO/summary.json")
PY
log "DEMO STOP-AFTER-P1 OK"
