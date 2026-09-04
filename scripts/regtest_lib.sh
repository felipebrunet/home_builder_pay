# shellcheck shell=bash
# Shared helpers for regtest E2E scripts. Source from the same directory.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BTC="/home/felipe/projects/btc_clients"
BCLI="$BTC/bin/bitcoin-cli -conf=$BTC/bitcoin.conf -datadir=$BTC/data/bitcoind"
HBP="$ROOT/target/debug/hbp"
PRICE="${PRICE:-100000}"
PARTIDA_BTC="${PARTIDA_BTC:-0.30000000}"
BOND_BTC="${BOND_BTC:-0.20000000}"
PARTIDA_SATS="${PARTIDA_SATS:-30000000}"
BOND_SATS="${BOND_SATS:-20000000}"
FEE_SATS="${FEE_SATS:-200}"
FUND_FEE_SATS="${FUND_FEE_SATS:-20000}"

log() { printf '\n==== %s ====\n' "$*"; }
rpc() { $BCLI "$@"; }
wrpc() { local w="$1"; shift; $BCLI -rpcwallet="$w" "$@"; }

ensure_wallet() {
  local name="$1"
  if rpc listwallets | grep -q "\"$name\""; then return 0; fi
  if rpc loadwallet "$name" >/dev/null 2>&1; then return 0; fi
  rpc createwallet "$name" >/dev/null
}

trusted() {
  wrpc "$1" getbalances | python3 -c 'import json,sys; print(json.load(sys.stdin)["mine"]["trusted"])'
}

prepare_wallets() {
  "$BTC/start-bitcoind.sh" >/dev/null
  cargo build -p hbp-cli --quiet --manifest-path "$ROOT/Cargo.toml"
  ensure_wallet miner
  ensure_wallet hbp_mandante
  ensure_wallet hbp_contratista
  MINE_ADDR="$(wrpc miner getnewaddress)"
  M_RECV="$(wrpc hbp_mandante getnewaddress)"
  C_RECV="$(wrpc hbp_contratista getnewaddress)"
  rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
  local m c
  m="$(trusted hbp_mandante)"
  c="$(trusted hbp_contratista)"
  python3 -c "import sys; sys.exit(0 if float('$m')>=2 and float('$c')>=2 else 1)" || {
    rpc generatetoaddress 101 "$MINE_ADDR" >/dev/null
    wrpc miner sendtoaddress "$M_RECV" 10 >/dev/null
    wrpc miner sendtoaddress "$C_RECV" 10 >/dev/null
    rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
  }
  echo "mine $MINE_ADDR"
  echo "mandante $(trusted hbp_mandante) BTC"
  echo "contratista $(trusted hbp_contratista) BTC"
}

vout_of() {
  local txid="$1" addr="$2"
  rpc getrawtransaction "$txid" true | python3 -c "import json,sys; tx=json.load(sys.stdin); a='$addr'
print(next(i for i,o in enumerate(tx['vout']) if o['scriptPubKey'].get('address')==a))"
}

fund_bond_and_p1() {
  local bond_addr="$1" p1_addr="$2" tag="$3"
  python3 - "$BCLI" "$HBP" "$tag" "$FUND_FEE_SATS" <<'PY'
import json, subprocess, sys
bcli = sys.argv[1].split()
hbp, tag, fee = sys.argv[2], sys.argv[3], sys.argv[4]

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
    u = max(utxos, key=lambda x: x["amount"])
    if "address" not in u:
        raise SystemExit(f"utxo in {wallet} has no address")
    return u

def sats(btc):
    return int(round(float(btc) * 100_000_000))

m, c = pick("hbp_mandante"), pick("hbp_contratista")
m_chg = rpc(["getrawchangeaddress"], wallet="hbp_mandante")
c_chg = rpc(["getrawchangeaddress"], wallet="hbp_contratista")
psbt = subprocess.check_output(
    [
        hbp, "--dir", ".m", "fund",
        "--partida", "1",
        "--fee", fee,
        "--m-outpoint", f"{m['txid']}:{m['vout']}",
        "--m-sats", str(sats(m["amount"])),
        "--m-prev", m["address"],
        "--m-change", m_chg,
        "--c-outpoint", f"{c['txid']}:{c['vout']}",
        "--c-sats", str(sats(c["amount"])),
        "--c-prev", c["address"],
        "--c-change", c_chg,
    ],
    text=True,
).strip().splitlines()[-1]
p1 = rpc(["walletprocesspsbt", psbt], wallet="hbp_mandante")
p2 = rpc(["walletprocesspsbt", p1["psbt"]], wallet="hbp_contratista")
fin = rpc(["finalizepsbt", p2["psbt"]])
if not fin.get("complete"):
    raise SystemExit(f"psbt not complete: {fin}")
txid = rpc(["sendrawtransaction", fin["hex"]])
open(f"/tmp/hbp-{tag}-fund1.hex", "w").write(fin["hex"].strip() + "\n")
open(f"/tmp/hbp-{tag}-fund.hex", "w").write(fin["hex"].strip() + "\n")
print(txid)
PY
}

hbp_offer_60k() {
  local t1="$1" t2="$2" tproj="$3" note="$4"
  $HBP --dir .m init --network regtest --role mandante
  $HBP --dir .c init --network regtest --role contratista
  $HBP --dir .m new --unit USD --bond-bps 3333 --t-project "$tproj" --dispute unwind
  $HBP --dir .m add-partida --desc Cimentacion --amount 30000 --plazo "$t1"
  $HBP --dir .m add-partida --desc Muros --amount 30000 --plazo "$t2"
  $HBP --dir .m offer
  $HBP --dir .c accept .m/00-offer.json
  $HBP --dir .m commit .c/01-accepted.pending.json
  CID="$(cat .m/CURRENT)"
  $HBP --dir .c import ".m/contracts/$CID/01-accepted.json"
  $HBP --dir .m quote --btc-price "$PRICE" --bond-sats "$BOND_SATS" --fx-note "$note"
  $HBP --dir .c accept-quote ".m/contracts/$CID/02-quote.json"
  $HBP --dir .m accept-quote ".c/contracts/$CID/02-quote.json"
  mapfile -t ADDRS < <($HBP --dir .m addresses)
  BOND_ADDR="$(printf '%s\n' "${ADDRS[@]}" | awk '/^bond bcrt/{print $2}')"
  P1_ADDR="$(printf '%s\n' "${ADDRS[@]}" | awk '/^partida 1 bcrt/{print $3}')"
  P2_ADDR="$(printf '%s\n' "${ADDRS[@]}" | awk '/^partida 2 bcrt/{print $3}')"
  echo "contract_id=$CID"
  echo "bond $BOND_ADDR"
  echo "p1   $P1_ADDR"
  echo "p2   $P2_ADDR"
}

# Optional env: NEW_EXTRA (e.g. --dispute mad --mad-bps 100)
hbp_offer_60k_ex() {
  local t1="$1" t2="$2" tproj="$3" note="$4"
  $HBP --dir .m init --network regtest --role mandante
  $HBP --dir .c init --network regtest --role contratista
  # shellcheck disable=SC2086
  $HBP --dir .m new --unit USD --bond-bps 3333 --t-project "$tproj" ${NEW_EXTRA:---dispute unwind}
  $HBP --dir .m add-partida --desc Cimentacion --amount 30000 --plazo "$t1"
  $HBP --dir .m add-partida --desc Muros --amount 30000 --plazo "$t2"
  $HBP --dir .m offer
  $HBP --dir .c accept .m/00-offer.json
  $HBP --dir .m commit .c/01-accepted.pending.json
  CID="$(cat .m/CURRENT)"
  $HBP --dir .c import ".m/contracts/$CID/01-accepted.json"
  $HBP --dir .m quote --btc-price "$PRICE" --bond-sats "$BOND_SATS" --fx-note "$note"
  $HBP --dir .c accept-quote ".m/contracts/$CID/02-quote.json"
  $HBP --dir .m accept-quote ".c/contracts/$CID/02-quote.json"
}

hbp_read_addrs() {
  mapfile -t ADDRS < <($HBP --dir .m addresses)
  BOND_ADDR="$(printf '%s\n' "${ADDRS[@]}" | awk '/^bond bcrt/{print $2}')"
  P1_ADDR="$(printf '%s\n' "${ADDRS[@]}" | awk '/^partida 1 bcrt/{print $3}')"
  P2_ADDR="$(printf '%s\n' "${ADDRS[@]}" | awk '/^partida 2 bcrt/{print $3}')"
  MAD_ADDR="$(printf '%s\n' "${ADDRS[@]}" | awk '/^mad bcrt/{print $2}')"
}

name_arbiter() {
  $HBP --dir .a init --network regtest
  local apk
  apk="$(python3 -c "import json; print(json.load(open('.a/identity.json'))['public_key'])")"
  $HBP --dir .m propose-arbiter --pubkey "$apk"
  $HBP --dir .c accept-arbiter ".m/contracts/$CID/03-arbiter.json"
  $HBP --dir .m accept-arbiter ".c/contracts/$CID/03-arbiter.json"
  echo "arbiter $apk"
}

advance_mtp() {
  local when="$1"
  rpc setmocktime "$when" >/dev/null
  rpc generatetoaddress 12 "$MINE_ADDR" >/dev/null
}

fund_bond_p1_mad() {
  # MAD outputs come from the signed quote via `hbp fund`; addrs are unused.
  local _bond_addr="$1" _p1_addr="$2" _mad_addr="$3" _mad_btc="$4" tag="$5"
  fund_bond_and_p1 "$_bond_addr" "$_p1_addr" "$tag"
}

# Mandante-only funding of a later partida (bond already locked).
fund_partida_only() {
  local tag="$1" pid="${2:-2}"
  python3 - "$BCLI" "$HBP" "$tag" "$pid" "$FUND_FEE_SATS" <<'PY'
import json, subprocess, sys
bcli = sys.argv[1].split()
hbp, tag, pid, fee = sys.argv[2], sys.argv[3], sys.argv[4], sys.argv[5]

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
    u = max(utxos, key=lambda x: x["amount"])
    if "address" not in u:
        raise SystemExit(f"utxo in {wallet} has no address")
    return u

def sats(btc):
    return int(round(float(btc) * 100_000_000))

m = pick("hbp_mandante")
m_chg = rpc(["getrawchangeaddress"], wallet="hbp_mandante")
psbt = subprocess.check_output(
    [
        hbp, "--dir", ".m", "fund",
        "--partida", pid,
        "--partida-only",
        "--fee", fee,
        "--m-outpoint", f"{m['txid']}:{m['vout']}",
        "--m-sats", str(sats(m["amount"])),
        "--m-prev", m["address"],
        "--m-change", m_chg,
    ],
    text=True,
).strip().splitlines()[-1]
p1 = rpc(["walletprocesspsbt", psbt], wallet="hbp_mandante")
fin = rpc(["finalizepsbt", p1["psbt"]])
if not fin.get("complete"):
    raise SystemExit(f"psbt not complete: {fin}")
txid = rpc(["sendrawtransaction", fin["hex"]])
open(f"/tmp/hbp-{tag}-p{pid}.hex", "w").write(fin["hex"].strip() + "\n")
print(txid)
PY
}

# File MuSig2 close (two laptops): propose → sign → finish. Prints signed tx hex.
coop_close_files() {
  local kind="$1" outpoint="$2" sats="$3" dest="$4" fee="$5"
  local partida="${6:-}"
  local extra=()
  if [[ -n "$partida" ]]; then
    extra+=(--partida "$partida")
  fi
  $HBP --dir .m coop-propose --kind "$kind" --outpoint "$outpoint" --sats "$sats" \
    --dest "$dest" --fee "$fee" "${extra[@]}" >/dev/null
  $HBP --dir .c coop-sign .m/04-coop.json >/dev/null
  $HBP --dir .m coop-finish .c/04-coop.json
}

fund_wrong_p1() {
  local bond_addr="$1" p1_addr="$2" wrong_btc="$3" tag="$4"
  python3 - "$BCLI" "$bond_addr" "$p1_addr" "$BOND_BTC" "$wrong_btc" "$tag" <<'PY'
import json, subprocess, sys
bcli = sys.argv[1].split()
bond_addr, p1_addr = sys.argv[2], sys.argv[3]
bond, part = float(sys.argv[4]), float(sys.argv[5])
tag = sys.argv[6]
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
ins = [{"txid": m["txid"], "vout": m["vout"]}, {"txid": c["txid"], "vout": c["vout"]}]
outs = {
    bond_addr: bond,
    p1_addr: part,
    rpc(["getrawchangeaddress"], wallet="hbp_mandante"): round(m["amount"] - part - fee / 2, 8),
    rpc(["getrawchangeaddress"], wallet="hbp_contratista"): round(c["amount"] - bond - fee / 2, 8),
}
psbt = rpc(["createpsbt", json.dumps(ins), json.dumps(outs)])
p1 = rpc(["walletprocesspsbt", psbt], wallet="hbp_mandante")
p2 = rpc(["walletprocesspsbt", p1["psbt"]], wallet="hbp_contratista")
fin = rpc(["finalizepsbt", p2["psbt"]])
if not fin.get("complete"):
    raise SystemExit(f"psbt not complete: {fin}")
txid = rpc(["sendrawtransaction", fin["hex"]])
open(f"/tmp/hbp-{tag}-fund.hex", "w").write(fin["hex"].strip() + "\n")
print(txid)
PY
}
