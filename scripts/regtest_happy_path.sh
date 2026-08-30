#!/usr/bin/env bash
# Camino feliz on-chain: 60k USD, 2 partidas de 30k, boleta 20k, 5s de obra cada una.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BTC="/home/felipe/projects/btc_clients"
BCLI="$BTC/bin/bitcoin-cli -conf=$BTC/bitcoin.conf -datadir=$BTC/data/bitcoind"
HBP="$ROOT/target/debug/hbp"
DEMO="${DEMO_DIR:-/tmp/hbp-regtest-demo}"
PRICE=100000          # USD por BTC (demo)
PARTIDA_BTC="0.30000000"   # 30_000 USD
BOND_BTC="0.20000000"      # 20_000 USD
PARTIDA_SATS=30000000
BOND_SATS=20000000
FEE_SATS=200
WORK_SECS=5

log() { printf '\n==== %s ====\n' "$*"; }

rpc() { $BCLI "$@"; }
wrpc() {
  local w="$1"; shift
  $BCLI -rpcwallet="$w" "$@"
}

ensure_wallet() {
  local name="$1"
  if rpc listwallets | grep -q "\"$name\""; then
    return 0
  fi
  if rpc loadwallet "$name" >/dev/null 2>&1; then
    return 0
  fi
  rpc createwallet "$name" >/dev/null
}

btc_to_sats() {
  python3 -c "print(int(round(float('$1') * 100_000_000)))"
}

"$BTC/start-bitcoind.sh" >/dev/null
cargo build -p hbp-cli --quiet --manifest-path "$ROOT/Cargo.toml"

rm -rf "$DEMO"
mkdir -p "$DEMO"
cd "$DEMO"

log "Wallets hot regtest"
ensure_wallet miner
ensure_wallet hbp_mandante
ensure_wallet hbp_contratista
MINE_ADDR="$(wrpc miner getnewaddress)"
M_RECV="$(wrpc hbp_mandante getnewaddress)"
C_RECV="$(wrpc hbp_contratista getnewaddress)"
echo "miner $MINE_ADDR"
echo "mandante recv $M_RECV"
echo "contratista recv $C_RECV"

log "Madurar coinbase del miner y fondear hot wallets (10 BTC c/u)"
rpc generatetoaddress 101 "$MINE_ADDR" >/dev/null
echo "miner trusted $(wrpc miner getbalances | python3 -c 'import json,sys; print(json.load(sys.stdin)["mine"]["trusted"])')"
wrpc miner sendtoaddress "$M_RECV" 10 >/dev/null
wrpc miner sendtoaddress "$C_RECV" 10 >/dev/null
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
echo "mandante trusted $(wrpc hbp_mandante getbalances | python3 -c 'import json,sys; print(json.load(sys.stdin)["mine"]["trusted"])')"
echo "contratista trusted $(wrpc hbp_contratista getbalances | python3 -c 'import json,sys; print(json.load(sys.stdin)["mine"]["trusted"])')"

NOW="$(date +%s)"
T1=$((NOW + 20))
T2=$((NOW + 40))
TPROJ=$((NOW + 60))
echo "now=$NOW t_partida1=$T1 t_partida2=$T2 t_project=$TPROJ (margen de segundos; obra simulada=${WORK_SECS}s)"

log "Identidades HBP (claves de contrato, distintas a las hot wallets)"
$HBP --dir .m init --network regtest --role mandante
$HBP --dir .c init --network regtest --role contratista

log "Contrato 60k USD / 2x30k / boleta 20k"
$HBP --dir .m new --unit USD --bond-bps 3333 --t-project "$TPROJ"
$HBP --dir .m add-partida --desc Cimentacion --amount 30000 --plazo "$T1"
$HBP --dir .m add-partida --desc Muros --amount 30000 --plazo "$T2"
$HBP --dir .m offer
$HBP --dir .c accept .m/00-offer.json
$HBP --dir .m commit .c/01-accepted.pending.json
CID="$(cat .m/CURRENT)"
echo "contract_id=$CID"
$HBP --dir .c import ".m/contracts/$CID/01-accepted.json"
$HBP --dir .m quote --btc-price "$PRICE" --bond-sats "$BOND_SATS" --fx-note "demo $PRICE USD/BTC"
$HBP --dir .c accept-quote ".m/contracts/$CID/02-quote.json"
$HBP --dir .m accept-quote ".c/contracts/$CID/02-quote.json"

mapfile -t ADDRS < <($HBP --dir .m addresses)
BOND_ADDR="$(printf '%s\n' "${ADDRS[@]}" | awk '/^bond bcrt/{print $2}')"
P1_ADDR="$(printf '%s\n' "${ADDRS[@]}" | awk '/^partida 1 bcrt/{print $3}')"
P2_ADDR="$(printf '%s\n' "${ADDRS[@]}" | awk '/^partida 2 bcrt/{print $3}')"
echo "bond    $BOND_ADDR"
echo "p1      $P1_ADDR"
echo "p2      $P2_ADDR"

log "Fondeo atomico boleta + partida 1 (PSBT 2 wallets)"
python3 - "$BCLI" "$BOND_ADDR" "$P1_ADDR" "$BOND_BTC" "$PARTIDA_BTC" <<'PY'
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
    if not out:
        return None
    try:
        return json.loads(out)
    except json.JSONDecodeError:
        return out

def pick(wallet):
    utxos = rpc(["listunspent", "1"], wallet=wallet)
    utxos = [u for u in utxos if u["spendable"] and u["amount"] >= 1]
    if not utxos:
        raise SystemExit(f"no utxo in {wallet}")
    return max(utxos, key=lambda u: u["amount"])

m = pick("hbp_mandante")
c = pick("hbp_contratista")
m_chg = round(m["amount"] - part - fee / 2, 8)
c_chg = round(c["amount"] - bond - fee / 2, 8)
if m_chg < 0.0001 or c_chg < 0.0001:
    raise SystemExit(f"change too small m={m_chg} c={c_chg}")
m_addr = rpc(["getrawchangeaddress"], wallet="hbp_mandante")
c_addr = rpc(["getrawchangeaddress"], wallet="hbp_contratista")
ins = [
    {"txid": m["txid"], "vout": m["vout"]},
    {"txid": c["txid"], "vout": c["vout"]},
]
outs = {bond_addr: bond, p1_addr: part, m_addr: m_chg, c_addr: c_chg}
psbt = rpc(["createpsbt", json.dumps(ins), json.dumps(outs)])
p1 = rpc(["walletprocesspsbt", psbt], wallet="hbp_mandante")
p2 = rpc(["walletprocesspsbt", p1["psbt"]], wallet="hbp_contratista")
fin = rpc(["finalizepsbt", p2["psbt"]])
if not fin.get("complete"):
    raise SystemExit(f"psbt not complete: {fin}")
txid = rpc(["sendrawtransaction", fin["hex"]])
print(fin["hex"])
print(txid, file=sys.stderr)
open("/tmp/hbp-fund1.hex", "w").write(fin["hex"].strip() + "\n")
open("/tmp/hbp-fund1.txid", "w").write(txid.strip() + "\n")
PY
FUND1_HEX="$(cat /tmp/hbp-fund1.hex)"
FUND1_TXID="$(cat /tmp/hbp-fund1.txid)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
echo "fund1 txid $FUND1_TXID"
$HBP --dir .m verify-funding --tx-hex "$FUND1_HEX" --partida 1
$HBP --dir .c verify-funding --tx-hex "$FUND1_HEX" --partida 1
# vouts from decoded tx
P1_VOUT="$(rpc getrawtransaction "$FUND1_TXID" true | python3 -c "import json,sys; tx=json.load(sys.stdin); a='$P1_ADDR'
print(next(i for i,o in enumerate(tx['vout']) if o['scriptPubKey'].get('address')==a))")"
BOND_VOUT="$(rpc getrawtransaction "$FUND1_TXID" true | python3 -c "import json,sys; tx=json.load(sys.stdin); a='$BOND_ADDR'
print(next(i for i,o in enumerate(tx['vout']) if o['scriptPubKey'].get('address')==a))")"
echo "bond outpoint ${FUND1_TXID}:${BOND_VOUT}"
echo "p1   outpoint ${FUND1_TXID}:${P1_VOUT}"

log "Obra partida 1 (${WORK_SECS}s simulados)"
sleep "$WORK_SECS"
C_PAY="$(wrpc hbp_contratista getnewaddress)"
echo "pago partida 1 -> $C_PAY"
P1_HEX="$($HBP --dir .m coop-close --kind partida --partida 1 \
  --outpoint "${FUND1_TXID}:${P1_VOUT}" --sats "$PARTIDA_SATS" \
  --dest "$C_PAY" --fee "$FEE_SATS" --peer-dir .c 2>/tmp/hbp-p1.err)"
cat /tmp/hbp-p1.err >&2
P1_PAY_TXID="$(rpc sendrawtransaction "$P1_HEX")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
echo "partida 1 paid $P1_PAY_TXID"

log "Fondeo partida 2 (solo mandante)"
P2_TXID="$(wrpc hbp_mandante sendtoaddress "$P2_ADDR" "$PARTIDA_BTC")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
P2_HEX="$(rpc getrawtransaction "$P2_TXID")"
$HBP --dir .m verify-funding --tx-hex "$P2_HEX" --partida 2 --partida-only
$HBP --dir .c verify-funding --tx-hex "$P2_HEX" --partida 2 --partida-only
P2_VOUT="$(rpc getrawtransaction "$P2_TXID" true | python3 -c "import json,sys; tx=json.load(sys.stdin); a='$P2_ADDR'
print(next(i for i,o in enumerate(tx['vout']) if o['scriptPubKey'].get('address')==a))")"
echo "p2 outpoint ${P2_TXID}:${P2_VOUT}"

log "Obra partida 2 (${WORK_SECS}s simulados)"
sleep "$WORK_SECS"
C_PAY2="$(wrpc hbp_contratista getnewaddress)"
P2_PAY_HEX="$($HBP --dir .m coop-close --kind partida --partida 2 \
  --outpoint "${P2_TXID}:${P2_VOUT}" --sats "$PARTIDA_SATS" \
  --dest "$C_PAY2" --fee "$FEE_SATS" --peer-dir .c 2>/tmp/hbp-p2.err)"
cat /tmp/hbp-p2.err >&2
P2_PAY_TXID="$(rpc sendrawtransaction "$P2_PAY_HEX")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
echo "partida 2 paid $P2_PAY_TXID"

log "Liberar boleta al contratista (MuSig2)"
C_BOND="$(wrpc hbp_contratista getnewaddress)"
BOND_HEX="$($HBP --dir .c coop-close --kind bond \
  --outpoint "${FUND1_TXID}:${BOND_VOUT}" --sats "$BOND_SATS" \
  --dest "$C_BOND" --fee "$FEE_SATS" --peer-dir .m 2>/tmp/hbp-bond.err)"
cat /tmp/hbp-bond.err >&2
BOND_PAY_TXID="$(rpc sendrawtransaction "$BOND_HEX")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
echo "bond released $BOND_PAY_TXID"

log "Saldos finales"
echo "mandante    $(wrpc hbp_mandante getbalances | python3 -c 'import json,sys; print(json.load(sys.stdin)["mine"]["trusted"])') BTC"
echo "contratista $(wrpc hbp_contratista getbalances | python3 -c 'import json,sys; print(json.load(sys.stdin)["mine"]["trusted"])') BTC"
echo "status mandante:"
$HBP --dir .m status | python3 -c 'import json,sys; p=json.load(sys.stdin); print(p["status"], p["bond"]["status"], [(x["id"], x["state"]["status"]) for x in p["partidas"]])'

# Persist a machine-readable summary for the markdown
python3 - <<PY
import json
json.dump({
  "contract_id": "$CID",
  "price_usd_per_btc": $PRICE,
  "bond_sats": $BOND_SATS,
  "partida_sats": $PARTIDA_SATS,
  "work_secs": $WORK_SECS,
  "t1": $T1, "t2": $T2, "t_project": $TPROJ,
  "bond_addr": "$BOND_ADDR",
  "p1_addr": "$P1_ADDR",
  "p2_addr": "$P2_ADDR",
  "fund1_txid": "$FUND1_TXID",
  "p1_vout": int("$P1_VOUT"),
  "bond_vout": int("$BOND_VOUT"),
  "p1_pay_txid": "$P1_PAY_TXID",
  "p2_fund_txid": "$P2_TXID",
  "p2_vout": int("$P2_VOUT"),
  "p2_pay_txid": "$P2_PAY_TXID",
  "bond_pay_txid": "$BOND_PAY_TXID",
}, open("$DEMO/summary.json","w"), indent=2)
print("summary $DEMO/summary.json")
PY
log "DEMO REGTEST OK"
