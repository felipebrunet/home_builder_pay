#!/usr/bin/env bash
# Partida 1 al 100%. Partida 2 aceptada al 80% (pintura fallida) por acuerdo.
set -euo pipefail
# shellcheck source=regtest_lib.sh
source "$(cd "$(dirname "$0")" && pwd)/regtest_lib.sh"

DEMO="${DEMO_DIR:-/tmp/hbp-regtest-split80}"
WAIT_SECS=5
PAY80=24000000   # 80% de 30_000_000 sats
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
hbp_offer_60k "$T1" "$T2" "$TPROJ" "demo split 80% partida 2"

log "Fondeo boleta + partida 1 y pago 100%"
FUND1_TXID="$(fund_bond_and_p1 "$BOND_ADDR" "$P1_ADDR" "split80")"
FUND1_HEX="$(cat /tmp/hbp-split80-fund1.hex)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
$HBP --dir .m verify-funding --tx-hex "$FUND1_HEX" --partida 1
$HBP --dir .c verify-funding --tx-hex "$FUND1_HEX" --partida 1
P1_VOUT="$(vout_of "$FUND1_TXID" "$P1_ADDR")"
BOND_VOUT="$(vout_of "$FUND1_TXID" "$BOND_ADDR")"
sleep "$WAIT_SECS"
C_PAY1="$(wrpc hbp_contratista getnewaddress)"
P1_HEX="$($HBP --dir .m coop-close --kind partida --partida 1 \
  --outpoint "${FUND1_TXID}:${P1_VOUT}" --sats "$PARTIDA_SATS" \
  --dest "$C_PAY1" --fee "$FEE_SATS" --peer-dir .c 2>/tmp/hbp-s80-p1.err)"
cat /tmp/hbp-s80-p1.err >&2
P1_PAY="$(rpc sendrawtransaction "$P1_HEX")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
echo "partida 1 paid 100% $P1_PAY"

log "Fondeo partida 2"
P2_TXID="$(wrpc hbp_mandante sendtoaddress "$P2_ADDR" "$PARTIDA_BTC")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
P2_HEX_FUND="$(rpc getrawtransaction "$P2_TXID")"
$HBP --dir .m verify-funding --tx-hex "$P2_HEX_FUND" --partida 2 --partida-only
$HBP --dir .c verify-funding --tx-hex "$P2_HEX_FUND" --partida 2 --partida-only
P2_VOUT="$(vout_of "$P2_TXID" "$P2_ADDR")"

log "Obra partida 2 (${WAIT_SECS}s): pintura fallida. Acuerdan pagar 80%."
sleep "$WAIT_SECS"
C_PAY2="$(wrpc hbp_contratista getnewaddress)"
M_REFUND="$(wrpc hbp_mandante getnewaddress)"
P2_HEX="$($HBP --dir .m coop-close --kind partida --partida 2 \
  --outpoint "${P2_TXID}:${P2_VOUT}" --sats "$PARTIDA_SATS" \
  --dest "$C_PAY2" --pay-sats "$PAY80" --refund-dest "$M_REFUND" \
  --fee "$FEE_SATS" --peer-dir .c 2>/tmp/hbp-s80-p2.err)"
cat /tmp/hbp-s80-p2.err >&2
P2_PAY="$(rpc sendrawtransaction "$P2_HEX")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
echo "partida 2 split 80/20 $P2_PAY"

log "Liberar boleta (proyecto cerrado por acuerdo)"
C_BOND="$(wrpc hbp_contratista getnewaddress)"
BOND_HEX="$($HBP --dir .c coop-close --kind bond \
  --outpoint "${FUND1_TXID}:${BOND_VOUT}" --sats "$BOND_SATS" \
  --dest "$C_BOND" --fee "$FEE_SATS" --peer-dir .m 2>/tmp/hbp-s80-bond.err)"
cat /tmp/hbp-s80-bond.err >&2
BOND_PAY="$(rpc sendrawtransaction "$BOND_HEX")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
echo "bond released $BOND_PAY"

echo "mandante    $M_BEFORE -> $(trusted hbp_mandante) BTC"
echo "contratista $C_BEFORE -> $(trusted hbp_contratista) BTC"
$HBP --dir .m status | python3 -c 'import json,sys; p=json.load(sys.stdin); print(p["status"], p["bond"]["status"], [(x["id"], x["state"]["status"]) for x in p["partidas"]])'

python3 - <<PY
import json
json.dump({
  "contract_id": "$CID",
  "scenario": "partida2_agreed_80_percent",
  "fund1_txid": "$FUND1_TXID",
  "p1_pay_txid": "$P1_PAY",
  "p2_fund_txid": "$P2_TXID",
  "p2_split_txid": "$P2_PAY",
  "bond_release_txid": "$BOND_PAY",
  "pay80_sats": $PAY80,
  "mandante_before": float("$M_BEFORE"),
  "mandante_after": float("$(trusted hbp_mandante)"),
  "contratista_before": float("$C_BEFORE"),
  "contratista_after": float("$(trusted hbp_contratista)"),
}, open("$DEMO/summary.json","w"), indent=2)
print("summary $DEMO/summary.json")
PY
log "DEMO SPLIT 80% OK"
