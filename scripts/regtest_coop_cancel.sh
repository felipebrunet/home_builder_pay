#!/usr/bin/env bash
# Cancelación cooperativa de la partida 1 (reembolso MuSig2) + boleta al contratista.
# No hay timeout. Partida 2 nunca se fondea.
set -euo pipefail
# shellcheck source=regtest_lib.sh
source "$(cd "$(dirname "$0")" && pwd)/regtest_lib.sh"

DEMO="${DEMO_DIR:-/tmp/hbp-regtest-coop-cancel}"
rm -rf "$DEMO"
mkdir -p "$DEMO"
cd "$DEMO"

log "Wallets"
prepare_wallets
M_BEFORE="$(trusted hbp_mandante)"
C_BEFORE="$(trusted hbp_contratista)"

NOW="$(date +%s)"
T1=$((NOW + 120))
T2=$((NOW + 240))
TPROJ=$((NOW + 360))

log "Contrato 60k / 2x30k / boleta 20k"
hbp_offer_60k "$T1" "$T2" "$TPROJ" "demo coop-cancel"

log "Fondeo atomico boleta + partida 1"
FUND1_TXID="$(fund_bond_and_p1 "$BOND_ADDR" "$P1_ADDR" "cancel")"
FUND1_HEX="$(cat /tmp/hbp-cancel-fund1.hex)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
$HBP --dir .m verify-funding --tx-hex "$FUND1_HEX" --partida 1
$HBP --dir .c verify-funding --tx-hex "$FUND1_HEX" --partida 1
P1_VOUT="$(vout_of "$FUND1_TXID" "$P1_ADDR")"
BOND_VOUT="$(vout_of "$FUND1_TXID" "$BOND_ADDR")"
echo "fund1 $FUND1_TXID p1_vout=$P1_VOUT bond_vout=$BOND_VOUT"

log "Contratista no puede unwind de la partida (solo el mandante)"
set +e
$HBP --dir .c unwind --kind partida --partida 1 \
  --outpoint "${FUND1_TXID}:${P1_VOUT}" --sats "$PARTIDA_SATS" \
  --dest "$(wrpc hbp_contratista getnewaddress)" --fee "$FEE_SATS" \
  >/tmp/hbp-steal-p1.hex 2>/tmp/hbp-steal-p1.err
STEAL_RC=$?
set -e
echo "steal rc=$STEAL_RC"
cat /tmp/hbp-steal-p1.err
[[ $STEAL_RC -ne 0 ]]

log "Cancelacion cooperativa: MuSig2 reembolsa partida 1 al mandante (sin esperar T)"
M_REFUND="$(wrpc hbp_mandante getnewaddress)"
P1_HEX="$($HBP --dir .m coop-close --kind partida --partida 1 --refund \
  --outpoint "${FUND1_TXID}:${P1_VOUT}" --sats "$PARTIDA_SATS" \
  --dest "$M_REFUND" --fee "$FEE_SATS" --peer-dir .c 2>/tmp/hbp-cancel-p1.err)"
cat /tmp/hbp-cancel-p1.err >&2
P1_TXID="$(rpc sendrawtransaction "$P1_HEX")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
echo "p1 refunded $P1_TXID -> $M_REFUND"

log "Boleta cooperativa de vuelta al contratista"
C_BOND="$(wrpc hbp_contratista getnewaddress)"
BOND_HEX="$($HBP --dir .c coop-close --kind bond --refund \
  --outpoint "${FUND1_TXID}:${BOND_VOUT}" --sats "$BOND_SATS" \
  --dest "$C_BOND" --fee "$FEE_SATS" --peer-dir .m 2>/tmp/hbp-cancel-bond.err)"
cat /tmp/hbp-cancel-bond.err >&2
BOND_TXID="$(rpc sendrawtransaction "$BOND_HEX")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
echo "bond returned $BOND_TXID -> $C_BOND"

echo "mandante    $M_BEFORE -> $(trusted hbp_mandante) BTC"
echo "contratista $C_BEFORE -> $(trusted hbp_contratista) BTC"
$HBP --dir .m status | python3 -c 'import json,sys; p=json.load(sys.stdin); print(p["status"], p["bond"]["status"], [(x["id"], x["state"]["status"]) for x in p["partidas"]])'

python3 - <<PY
import json
json.dump({
  "contract_id": "$CID",
  "scenario": "coop_cancel_before_timeout",
  "fund1_txid": "$FUND1_TXID",
  "p1_refund_txid": "$P1_TXID",
  "bond_return_txid": "$BOND_TXID",
  "p2_funded": False,
  "mandante_before": float("$M_BEFORE"),
  "mandante_after": float("$(trusted hbp_mandante)"),
  "contratista_before": float("$C_BEFORE"),
  "contratista_after": float("$(trusted hbp_contratista)"),
}, open("$DEMO/summary.json","w"), indent=2)
print("summary $DEMO/summary.json")
PY
log "DEMO COOP-CANCEL OK"
