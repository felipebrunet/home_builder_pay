#!/usr/bin/env bash
# Contratista no ejecuta y se va antes de la partida 2.
# Camino infeliz del protocolo: unwind de la partida al mandante; la boleta
# NO se ejecuta a su favor (vuelve al contratista después de T_proyecto).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BTC="/home/felipe/projects/btc_clients"
BCLI="$BTC/bin/bitcoin-cli -conf=$BTC/bitcoin.conf -datadir=$BTC/data/bitcoind"
HBP="$ROOT/target/debug/hbp"
DEMO="${DEMO_DIR:-/tmp/hbp-regtest-abandon}"
PRICE=100000
PARTIDA_BTC="0.30000000"
BOND_BTC="0.20000000"
PARTIDA_SATS=30000000
BOND_SATS=20000000
FEE_SATS=200
WAIT_SECS=5

log() { printf '\n==== %s ====\n' "$*"; }
rpc() { $BCLI "$@"; }
wrpc() { local w="$1"; shift; $BCLI -rpcwallet="$w" "$@"; }

ensure_wallet() {
  local name="$1"
  if rpc listwallets | grep -q "\"$name\""; then return 0; fi
  if rpc loadwallet "$name" >/dev/null 2>&1; then return 0; fi
  rpc createwallet "$name" >/dev/null
}

fund_bond_and_p1() {
  python3 - "$BCLI" "$1" "$2" "$BOND_BTC" "$PARTIDA_BTC" <<'PY'
import json, subprocess, sys
bcli = sys.argv[1].split()
bond_addr, p1_addr = sys.argv[2], sys.argv[3]
bond, part = float(sys.argv[4]), float(sys.argv[5])
fee = 0.00020000

def rpc(args, wallet=None):
    cmd = list(bcli)
    if wallet:
        cmd.append(f"-rpcwallet={wallet}")
    cmd.extend(args)
    out = subprocess.check_output(cmd, text=True).strip()
    try:
        return json.loads(out)
    except json.JSONDecodeError:
        return out

def pick(wallet):
    utxos = [u for u in rpc(["listunspent", "1"], wallet=wallet) if u["spendable"] and u["amount"] >= 1]
    if not utxos:
        raise SystemExit(f"no utxo in {wallet}")
    return max(utxos, key=lambda u: u["amount"])

m, c = pick("hbp_mandante"), pick("hbp_contratista")
m_chg = round(m["amount"] - part - fee / 2, 8)
c_chg = round(c["amount"] - bond - fee / 2, 8)
ins = [{"txid": m["txid"], "vout": m["vout"]}, {"txid": c["txid"], "vout": c["vout"]}]
outs = {
    bond_addr: bond,
    p1_addr: part,
    rpc(["getrawchangeaddress"], wallet="hbp_mandante"): m_chg,
    rpc(["getrawchangeaddress"], wallet="hbp_contratista"): c_chg,
}
psbt = rpc(["createpsbt", json.dumps(ins), json.dumps(outs)])
p1 = rpc(["walletprocesspsbt", psbt], wallet="hbp_mandante")
p2 = rpc(["walletprocesspsbt", p1["psbt"]], wallet="hbp_contratista")
fin = rpc(["finalizepsbt", p2["psbt"]])
if not fin.get("complete"):
    raise SystemExit(f"psbt not complete: {fin}")
txid = rpc(["sendrawtransaction", fin["hex"]])
open("/tmp/hbp-abandon-fund1.hex", "w").write(fin["hex"].strip() + "\n")
open("/tmp/hbp-abandon-fund1.txid", "w").write(txid.strip() + "\n")
print(txid)
PY
}

"$BTC/start-bitcoind.sh" >/dev/null
cargo build -p hbp-cli --quiet --manifest-path "$ROOT/Cargo.toml"

rm -rf "$DEMO"
mkdir -p "$DEMO"
cd "$DEMO"

log "Wallets"
ensure_wallet miner
ensure_wallet hbp_mandante
ensure_wallet hbp_contratista
MINE_ADDR="$(wrpc miner getnewaddress)"
M_RECV="$(wrpc hbp_mandante getnewaddress)"
C_RECV="$(wrpc hbp_contratista getnewaddress)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
# Solo recargar si el saldo es bajo (el camino feliz ya dejó ~19–20 BTC).
M_BAL="$(wrpc hbp_mandante getbalances | python3 -c 'import json,sys; print(json.load(sys.stdin)["mine"]["trusted"])')"
C_BAL="$(wrpc hbp_contratista getbalances | python3 -c 'import json,sys; print(json.load(sys.stdin)["mine"]["trusted"])')"
python3 - "$M_BAL" "$C_BAL" <<'PY' || true
import sys
m, c = float(sys.argv[1]), float(sys.argv[2])
open("/tmp/hbp-need-fund","w").write("yes\n" if m < 2 or c < 2 else "no\n")
PY
if grep -q yes /tmp/hbp-need-fund; then
  rpc generatetoaddress 101 "$MINE_ADDR" >/dev/null
  wrpc miner sendtoaddress "$M_RECV" 10 >/dev/null
  wrpc miner sendtoaddress "$C_RECV" 10 >/dev/null
  rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
fi
echo "mandante $(wrpc hbp_mandante getbalances | python3 -c 'import json,sys; print(json.load(sys.stdin)["mine"]["trusted"])') BTC"
echo "contratista $(wrpc hbp_contratista getbalances | python3 -c 'import json,sys; print(json.load(sys.stdin)["mine"]["trusted"])') BTC"
M_BEFORE="$(wrpc hbp_mandante getbalances | python3 -c 'import json,sys; print(json.load(sys.stdin)["mine"]["trusted"])')"
C_BEFORE="$(wrpc hbp_contratista getbalances | python3 -c 'import json,sys; print(json.load(sys.stdin)["mine"]["trusted"])')"

NOW="$(date +%s)"
T1=$((NOW + 8))
T2=$((NOW + 16))
TPROJ=$((NOW + 24))
echo "now=$NOW t1=$T1 t2=$T2 t_project=$TPROJ (obra no ocurre; espera ${WAIT_SECS}s y luego timeout)"

log "Contrato (igual montos que el camino feliz)"
$HBP --dir .m init --network regtest --role mandante
$HBP --dir .c init --network regtest --role contratista
$HBP --dir .m new --unit USD --bond-bps 3333 --t-project "$TPROJ"
$HBP --dir .m add-partida --desc Cimentacion --amount 30000 --plazo "$T1"
$HBP --dir .m add-partida --desc Muros --amount 30000 --plazo "$T2"
$HBP --dir .m offer
$HBP --dir .c accept .m/00-offer.json
$HBP --dir .m commit .c/01-accepted.pending.json
CID="$(cat .m/CURRENT)"
$HBP --dir .c import ".m/contracts/$CID/01-accepted.json"
$HBP --dir .m quote --btc-price "$PRICE" --bond-sats "$BOND_SATS" --fx-note "demo abandon $PRICE USD/BTC"
$HBP --dir .c accept-quote ".m/contracts/$CID/02-quote.json"
$HBP --dir .m accept-quote ".c/contracts/$CID/02-quote.json"
mapfile -t ADDRS < <($HBP --dir .m addresses)
BOND_ADDR="$(printf '%s\n' "${ADDRS[@]}" | awk '/^bond bcrt/{print $2}')"
P1_ADDR="$(printf '%s\n' "${ADDRS[@]}" | awk '/^partida 1 bcrt/{print $3}')"
P2_ADDR="$(printf '%s\n' "${ADDRS[@]}" | awk '/^partida 2 bcrt/{print $3}')"
echo "bond $BOND_ADDR"
echo "p1   $P1_ADDR"
echo "p2   $P2_ADDR (nunca se fondea)"

log "Fondeo atomico boleta + partida 1"
FUND1_TXID="$(fund_bond_and_p1 "$BOND_ADDR" "$P1_ADDR")"
FUND1_HEX="$(cat /tmp/hbp-abandon-fund1.hex)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
$HBP --dir .m verify-funding --tx-hex "$FUND1_HEX" --partida 1
$HBP --dir .c verify-funding --tx-hex "$FUND1_HEX" --partida 1
P1_VOUT="$(rpc getrawtransaction "$FUND1_TXID" true | python3 -c "import json,sys; tx=json.load(sys.stdin); a='$P1_ADDR'
print(next(i for i,o in enumerate(tx['vout']) if o['scriptPubKey'].get('address')==a))")"
BOND_VOUT="$(rpc getrawtransaction "$FUND1_TXID" true | python3 -c "import json,sys; tx=json.load(sys.stdin); a='$BOND_ADDR'
print(next(i for i,o in enumerate(tx['vout']) if o['scriptPubKey'].get('address')==a))")"
echo "fund1 $FUND1_TXID bond_vout=$BOND_VOUT p1_vout=$P1_VOUT"

log "Contratista no trabaja (espera ${WAIT_SECS}s). No hay coop-close. Partida 2 no se fondea."
sleep "$WAIT_SECS"

log "Avanzar tiempo de cadena (MTP) past T_proyecto"
rpc setmocktime $((TPROJ + 600))
rpc generatetoaddress 12 "$MINE_ADDR" >/dev/null
echo "mocktime $(rpc getblockheader "$(rpc getbestblockhash)" | python3 -c 'import json,sys; print(json.load(sys.stdin)["mediantime"])')"

log "Mandante recupera la partida 1 (unwind script path)"
M_REFUND="$(wrpc hbp_mandante getnewaddress)"
P1_UNW="$($HBP --dir .m unwind --kind partida --partida 1 \
  --outpoint "${FUND1_TXID}:${P1_VOUT}" --sats "$PARTIDA_SATS" \
  --dest "$M_REFUND" --fee "$FEE_SATS" --peer-dir .c 2>/tmp/hbp-unw-p1.err)"
cat /tmp/hbp-unw-p1.err >&2
P1_UNW_TXID="$(rpc sendrawtransaction "$P1_UNW")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
echo "partida 1 unwound $P1_UNW_TXID -> $M_REFUND"

log "Mandante intenta ejecutar la boleta (debe FALLAR: la hoja es pk(contratista))"
C_STEAL="$(wrpc hbp_mandante getnewaddress)"
set +e
BOND_BAD="$($HBP --dir .m unwind --kind bond \
  --outpoint "${FUND1_TXID}:${BOND_VOUT}" --sats "$BOND_SATS" \
  --dest "$C_STEAL" --fee "$FEE_SATS" 2>/tmp/hbp-unw-bond-bad.err)"
HBP_BOND_RC=$?
set -e
if [[ $HBP_BOND_RC -eq 0 ]]; then
  set +e
  rpc sendrawtransaction "$BOND_BAD" >/tmp/hbp-unw-bond-bad.send 2>&1
  SEND_RC=$?
  set -e
  echo "hbp unwind produced hex; sendrawtransaction rc=$SEND_RC"
  cat /tmp/hbp-unw-bond-bad.send
  if [[ $SEND_RC -eq 0 ]]; then
    echo "UNEXPECTED: mandante stole the bond" >&2
    exit 1
  fi
else
  echo "hbp unwind as mandante on bond failed (ok):"
  cat /tmp/hbp-unw-bond-bad.err
fi

log "Boleta: el contratista (aunque abandono la obra) puede recuperarla despues de T"
C_BOND="$(wrpc hbp_contratista getnewaddress)"
BOND_OK="$($HBP --dir .c unwind --kind bond \
  --outpoint "${FUND1_TXID}:${BOND_VOUT}" --sats "$BOND_SATS" \
  --dest "$C_BOND" --fee "$FEE_SATS" --peer-dir .m 2>/tmp/hbp-unw-bond-ok.err)"
cat /tmp/hbp-unw-bond-ok.err >&2
BOND_UNW_TXID="$(rpc sendrawtransaction "$BOND_OK")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
echo "bond unwound $BOND_UNW_TXID -> $C_BOND"

rpc setmocktime 0 >/dev/null || true

M_AFTER="$(wrpc hbp_mandante getbalances | python3 -c 'import json,sys; print(json.load(sys.stdin)["mine"]["trusted"])')"
C_AFTER="$(wrpc hbp_contratista getbalances | python3 -c 'import json,sys; print(json.load(sys.stdin)["mine"]["trusted"])')"
echo "mandante    $M_BEFORE -> $M_AFTER BTC"
echo "contratista $C_BEFORE -> $C_AFTER BTC"
echo "status:"
$HBP --dir .m status | python3 -c 'import json,sys; p=json.load(sys.stdin); print(p["status"], p["bond"]["status"], [(x["id"], x["state"]["status"]) for x in p["partidas"]])'

python3 - <<PY
import json
json.dump({
  "contract_id": "$CID",
  "scenario": "contractor_abandons_before_partida_2",
  "bond_sats": $BOND_SATS,
  "partida_sats": $PARTIDA_SATS,
  "wait_secs": $WAIT_SECS,
  "t1": $T1, "t2": $T2, "t_project": $TPROJ,
  "bond_addr": "$BOND_ADDR",
  "p1_addr": "$P1_ADDR",
  "p2_addr": "$P2_ADDR",
  "fund1_txid": "$FUND1_TXID",
  "p1_vout": int("$P1_VOUT"),
  "bond_vout": int("$BOND_VOUT"),
  "p1_unwind_txid": "$P1_UNW_TXID",
  "bond_unwind_txid": "$BOND_UNW_TXID",
  "p2_funded": False,
  "mandante_before": float("$M_BEFORE"),
  "mandante_after": float("$M_AFTER"),
  "contratista_before": float("$C_BEFORE"),
  "contratista_after": float("$C_AFTER"),
}, open("$DEMO/summary.json","w"), indent=2)
print("summary $DEMO/summary.json")
PY
log "DEMO ABANDON OK"
