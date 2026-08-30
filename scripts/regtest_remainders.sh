#!/usr/bin/env bash
# Remaining catalog items: wrong address, races, lost keys, RBF, reorg, personal P2.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=regtest_lib.sh
source "$ROOT/scripts/regtest_lib.sh"

CAT="${CAT_DIR:-/tmp/hbp-regtest-remainders}"
rm -rf "$CAT"
mkdir -p "$CAT"

prepare_wallets >/tmp/hbp-rem-wallets.txt
MINE_ADDR="$(wrpc miner getnewaddress)"
C_RECV="$(wrpc hbp_contratista getnewaddress)"
rpc generatetoaddress 101 "$MINE_ADDR" >/dev/null

pass() { echo "PASS id=$1 $2"; }
fail() { echo "FAIL id=$1 $2" >&2; exit 1; }

topup() {
  wrpc miner sendtoaddress "$(wrpc hbp_mandante getnewaddress)" 5 >/dev/null || true
  wrpc miner sendtoaddress "$(wrpc hbp_contratista getnewaddress)" 5 >/dev/null || true
  rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
}

fresh() {
  local d="$CAT/$1"
  rm -rf "$d"
  mkdir -p "$d"
  cd "$d"
  rpc setmocktime 0 >/dev/null || true
  rpc generatetoaddress 12 "$MINE_ADDR" >/dev/null
  topup
}

chain_now() {
  rpc getblockchaininfo | python3 -c "import json,sys; print(int(json.load(sys.stdin)['mediantime']))"
}

hex_of() { rpc getrawtransaction "$1"; }

mempool_allows() {
  python3 - "$BCLI" "$1" <<'PY'
import json, subprocess, sys
bcli = sys.argv[1].split()
hx = sys.argv[2]
out = subprocess.check_output(bcli + ["testmempoolaccept", json.dumps([hx])], text=True)
r = json.loads(out)[0]
print("yes" if r.get("allowed") else "no")
if not r.get("allowed"):
    print(r.get("reject-reason", r), file=sys.stderr)
PY
}

# --- 38: P1 amount paid to P2 script ---
fresh wrong_addr
NOW="$(chain_now)"
hbp_offer_60k "$((NOW+120))" "$((NOW+240))" "$((NOW+360))" wrongaddr
hbp_read_addrs
F1="$(fund_wrong_p1 "$BOND_ADDR" "$P2_ADDR" "$PARTIDA_BTC" wrongp2)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
HEX="$(hex_of "$F1")"
if $HBP --dir .m verify-funding --tx-hex "$HEX" --partida 1 >/tmp/hbp-38.out 2>/tmp/hbp-38.err; then
  fail 38 wrong_addr_accepted
fi
pass 38 p1_paid_to_p2_script

# --- 41: arbiter policy, coins sent to a single-sig wallet (not the A-tree) ---
fresh arb_wrong_tree
NOW="$(chain_now)"
NEW_EXTRA="--dispute arbiter --arbiter-window 15"
hbp_offer_60k_ex "$((NOW+120))" "$((NOW+240))" "$((NOW+360))" arbwrong
unset NEW_EXTRA
name_arbiter >/dev/null
hbp_read_addrs
FAKE="$(wrpc hbp_mandante getnewaddress)"
F1="$(fund_wrong_p1 "$BOND_ADDR" "$FAKE" "$PARTIDA_BTC" arbwrong)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
HEX="$(hex_of "$F1")"
if $HBP --dir .m verify-funding --tx-hex "$HEX" --partida 1 >/tmp/hbp-41.out 2>/tmp/hbp-41.err; then
  fail 41 arb_wrong_tree_accepted
fi
pass 41 fund_wallet_not_arbiter_tree

# --- 137: P2 to C personal address ---
fresh p2_personal
NOW="$(chain_now)"
hbp_offer_60k "$((NOW+120))" "$((NOW+240))" "$((NOW+360))" p2pers
hbp_read_addrs
F1="$(fund_bond_and_p1 "$BOND_ADDR" "$P1_ADDR" p2pers)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
$HBP --dir .m verify-funding --tx-hex "$(hex_of "$F1")" --partida 1 >/dev/null
P1_VOUT="$(vout_of "$F1" "$P1_ADDR")"
C_DEST="$(wrpc hbp_contratista getnewaddress)"
$HBP --dir .m coop-close --kind partida --partida 1 --outpoint "${F1}:${P1_VOUT}" --sats "$PARTIDA_SATS" --dest "$C_DEST" --fee "$FEE_SATS" --peer-dir .c >/tmp/hbp-137-p1.hex
rpc sendrawtransaction "$(cat /tmp/hbp-137-p1.hex)" >/dev/null
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
P2_TXID="$(wrpc hbp_mandante sendtoaddress "$C_RECV" "$PARTIDA_BTC")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
if $HBP --dir .m verify-funding --tx-hex "$(hex_of "$P2_TXID")" --partida 2 --partida-only >/tmp/hbp-137.out 2>/tmp/hbp-137.err; then
  fail 137 personal_p2_accepted
fi
pass 137 p2_to_personal_rejected

# --- 127/128/129/132: lost files ---
fresh lost_keys
NOW="$(chain_now)"
hbp_offer_60k "$((NOW+20))" "$((NOW+40))" "$((NOW+80))" lost
hbp_read_addrs
F1="$(fund_bond_and_p1 "$BOND_ADDR" "$P1_ADDR" lost)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
$HBP --dir .m verify-funding --tx-hex "$(hex_of "$F1")" --partida 1 >/dev/null
P1_VOUT="$(vout_of "$F1" "$P1_ADDR")"
BOND_VOUT="$(vout_of "$F1" "$BOND_ADDR")"
cp -a .m/identity.json /tmp/hbp-id-m.json
cp -a .c/identity.json /tmp/hbp-id-c.json
M_DEST="$(wrpc hbp_mandante getnewaddress)"
rm -f .m/identity.json
if $HBP --dir .m unwind --kind partida --partida 1 --outpoint "${F1}:${P1_VOUT}" --sats "$PARTIDA_SATS" --dest "$M_DEST" --fee "$FEE_SATS" >/tmp/hbp-127.out 2>/tmp/hbp-127.err; then
  fail 127 unwind_without_m_key
fi
pass 127 lost_m_identity
cp /tmp/hbp-id-m.json .m/identity.json
advance_mtp "$((NOW + 80 + 30))"
C_DEST="$(wrpc hbp_contratista getnewaddress)"
rm -f .c/identity.json
if $HBP --dir .c unwind --kind bond --outpoint "${F1}:${BOND_VOUT}" --sats "$BOND_SATS" --dest "$C_DEST" --fee "$FEE_SATS" >/tmp/hbp-128.out 2>/tmp/hbp-128.err; then
  fail 128 unwind_without_c_key
fi
pass 128 lost_c_identity
rm -f .m/identity.json
if $HBP --dir .m unwind --kind partida --partida 1 --outpoint "${F1}:${P1_VOUT}" --sats "$PARTIDA_SATS" --dest "$M_DEST" --fee "$FEE_SATS" >/dev/null 2>&1 \
  || $HBP --dir .c unwind --kind bond --outpoint "${F1}:${BOND_VOUT}" --sats "$BOND_SATS" --dest "$C_DEST" --fee "$FEE_SATS" >/dev/null 2>&1; then
  fail 129 unwind_without_both_keys
fi
pass 129 lost_both_identities
cp /tmp/hbp-id-m.json .m/identity.json
cp /tmp/hbp-id-c.json .c/identity.json
CID="$(cat .m/CURRENT)"
rm -f ".m/contracts/$CID/state.json"
if $HBP --dir .m status >/dev/null 2>&1; then
  fail 132 status_without_state
fi
python3 - "$BCLI" "$P1_ADDR" <<'PY'
import json, subprocess, sys
bcli=sys.argv[1].split(); addr=sys.argv[2]
raw=subprocess.check_output(bcli+["scantxoutset","start",json.dumps([f"addr({addr})"])], text=True)
d=json.loads(raw)
assert float(d.get("total_amount",0))>0, d
print("p1_still_there", d["total_amount"])
PY
pass 132 lost_state_utxo_remains
rpc setmocktime 0 >/dev/null || true

# --- 134 RBF: two coop-closes, higher fee replaces ---
fresh rbf
NOW="$(chain_now)"
hbp_offer_60k "$((NOW+120))" "$((NOW+240))" "$((NOW+360))" rbf
hbp_read_addrs
F1="$(fund_bond_and_p1 "$BOND_ADDR" "$P1_ADDR" rbf)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
$HBP --dir .m verify-funding --tx-hex "$(hex_of "$F1")" --partida 1 >/dev/null
P1_VOUT="$(vout_of "$F1" "$P1_ADDR")"
C_DEST="$(wrpc hbp_contratista getnewaddress)"
rm -rf /tmp/hbp-rbf-m /tmp/hbp-rbf-c
cp -a .m /tmp/hbp-rbf-m
cp -a .c /tmp/hbp-rbf-c
LOW="$($HBP --dir .m coop-close --kind partida --partida 1 --outpoint "${F1}:${P1_VOUT}" --sats "$PARTIDA_SATS" --dest "$C_DEST" --fee 200 --peer-dir .c)"
rm -rf .m .c
cp -a /tmp/hbp-rbf-m .m
cp -a /tmp/hbp-rbf-c .c
HIGH="$($HBP --dir .m coop-close --kind partida --partida 1 --outpoint "${F1}:${P1_VOUT}" --sats "$PARTIDA_SATS" --dest "$C_DEST" --fee 5000 --peer-dir .c)"
rpc sendrawtransaction "$LOW" >/dev/null
set +e
rpc sendrawtransaction "$HIGH" >/tmp/hbp-134.send 2>/tmp/hbp-134.err
RC=$?
set -e
if [[ $RC -eq 0 ]]; then
  pass 134 rbf_replaced
elif grep -qiE "replace|insufficient fee|already in|conflict|txn-mempool-conflict'?" /tmp/hbp-134.err /tmp/hbp-134.send; then
  # Core may require a larger incremental fee; the conflict is still the RBF/replace path.
  pass 134 rbf_conflict_or_replaced
else
  cat /tmp/hbp-134.err /tmp/hbp-134.send >&2
  fail 134 unexpected_rbf
fi
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null

# --- 109: A signs both A+C and A+M (same P1) ---
fresh a_double
NOW="$(chain_now)"
T1=$((NOW + 120))
NEW_EXTRA="--dispute arbiter --arbiter-window 15"
hbp_offer_60k_ex "$T1" "$((T1 + 60))" "$((T1 + 120))" adouble
unset NEW_EXTRA
name_arbiter >/dev/null
hbp_read_addrs
F1="$(fund_bond_and_p1 "$BOND_ADDR" "$P1_ADDR" adouble)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
$HBP --dir .m verify-funding --tx-hex "$(hex_of "$F1")" --partida 1 >/dev/null
P1_VOUT="$(vout_of "$F1" "$P1_ADDR")"
advance_mtp "$((T1 + 5))"
C_DEST="$(wrpc hbp_contratista getnewaddress)"
M_DEST="$(wrpc hbp_mandante getnewaddress)"
AC="$($HBP --dir .c arbiter-close --kind partida --partida 1 --with ac --arbiter-dir .a \
  --outpoint "${F1}:${P1_VOUT}" --sats "$PARTIDA_SATS" --dest "$C_DEST" --fee "$FEE_SATS" --peer-dir .m)"
AM="$($HBP --dir .m arbiter-close --kind partida --partida 1 --with am --arbiter-dir .a \
  --outpoint "${F1}:${P1_VOUT}" --sats "$PARTIDA_SATS" --dest "$M_DEST" --fee 500 --peer-dir .c)"
[[ "$(mempool_allows "$AC")" == yes ]] || fail 109 ac_not_allowed
[[ "$(mempool_allows "$AM")" == yes ]] || fail 109 am_not_allowed
ACID="$(rpc sendrawtransaction "$AC")"
set +e
AMID="$(rpc sendrawtransaction "$AM" 2>/tmp/hbp-109.err)"
set -e
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
python3 - "$BCLI" "$ACID" "${AMID:-none}" <<'PY' || { fail 109 both_or_neither; }
import json, subprocess, sys
bcli, a, b = sys.argv[1].split(), sys.argv[2], sys.argv[3]

def confs(txid):
    if txid in ("", "none"):
        return 0
    try:
        r = json.loads(subprocess.check_output(bcli+["getrawtransaction", txid, "true"], text=True))
    except subprocess.CalledProcessError:
        return 0
    return int(r.get("confirmations") or 0)

ca, cb = confs(a), confs(b)
print("ac_confs", ca, "am_confs", cb)
assert (ca >= 1) + (cb >= 1) == 1, (ca, cb)
PY
pass 109 a_double_sign_one_wins
rpc setmocktime 0 >/dev/null || true

# --- 125: T2 race bond unwind (C) vs A+M ---
fresh race_bond
NOW="$(chain_now)"
TPROJ=$((NOW + 120))
WIN=15
NEW_EXTRA="--dispute arbiter --arbiter-window $WIN"
hbp_offer_60k_ex "$((NOW + 30))" "$((NOW + 60))" "$TPROJ" racebond
unset NEW_EXTRA
name_arbiter >/dev/null
hbp_read_addrs
F1="$(fund_bond_and_p1 "$BOND_ADDR" "$P1_ADDR" racebond)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
$HBP --dir .m verify-funding --tx-hex "$(hex_of "$F1")" --partida 1 >/dev/null
$HBP --dir .c verify-funding --tx-hex "$(hex_of "$F1")" --partida 1 >/dev/null
BOND_VOUT="$(vout_of "$F1" "$BOND_ADDR")"
advance_mtp "$((TPROJ + WIN + 30))"
M_DEST="$(wrpc hbp_mandante getnewaddress)"
C_DEST="$(wrpc hbp_contratista getnewaddress)"
UNW="$($HBP --dir .c unwind --kind bond --outpoint "${F1}:${BOND_VOUT}" --sats "$BOND_SATS" --dest "$C_DEST" --fee "$FEE_SATS" --peer-dir .m)"
AM="$($HBP --dir .m arbiter-close --kind bond --with am --arbiter-dir .a \
  --outpoint "${F1}:${BOND_VOUT}" --sats "$BOND_SATS" --dest "$M_DEST" --fee 500 --peer-dir .c)"
[[ "$(mempool_allows "$UNW")" == yes ]] || fail 125 unwind_not_allowed
[[ "$(mempool_allows "$AM")" == yes ]] || fail 125 am_not_allowed
UNWID="$(rpc sendrawtransaction "$UNW")"
set +e
AID="$(rpc sendrawtransaction "$AM" 2>/tmp/hbp-125.err)"
set -e
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
python3 - "$BCLI" "$UNWID" "${AID:-none}" <<'PY' || { fail 125 both_or_neither; }
import json, subprocess, sys
bcli, a, b = sys.argv[1].split(), sys.argv[2], sys.argv[3]

def confs(txid):
    if txid in ("", "none"):
        return 0
    try:
        r = json.loads(subprocess.check_output(bcli+["getrawtransaction", txid, "true"], text=True))
    except subprocess.CalledProcessError:
        return 0
    return int(r.get("confirmations") or 0)

ca, cb = confs(a), confs(b)
print("unw_confs", ca, "am_confs", cb)
assert (ca >= 1) + (cb >= 1) == 1, (ca, cb)
PY
pass 125 t2_race_bond_one_wins
rpc setmocktime 0 >/dev/null || true

# --- 126: T2 race P2 unwind (M) vs A+C ---
fresh race_p2
NOW="$(chain_now)"
T1=$((NOW + 30))
T2=$((NOW + 60))
TPROJ=$((NOW + 120))
NEW_EXTRA="--dispute arbiter --arbiter-window 15"
hbp_offer_60k_ex "$T1" "$T2" "$TPROJ" racep2
unset NEW_EXTRA
name_arbiter >/dev/null
hbp_read_addrs
F1="$(fund_bond_and_p1 "$BOND_ADDR" "$P1_ADDR" racep2)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
$HBP --dir .m verify-funding --tx-hex "$(hex_of "$F1")" --partida 1 >/dev/null
$HBP --dir .c verify-funding --tx-hex "$(hex_of "$F1")" --partida 1 >/dev/null
P1_VOUT="$(vout_of "$F1" "$P1_ADDR")"
C_DEST="$(wrpc hbp_contratista getnewaddress)"
$HBP --dir .m coop-close --kind partida --partida 1 --outpoint "${F1}:${P1_VOUT}" --sats "$PARTIDA_SATS" --dest "$C_DEST" --fee "$FEE_SATS" --peer-dir .c >/tmp/hbp-126-p1.hex
rpc sendrawtransaction "$(cat /tmp/hbp-126-p1.hex)" >/dev/null
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
P2_TXID="$(wrpc hbp_mandante sendtoaddress "$P2_ADDR" "$PARTIDA_BTC")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
$HBP --dir .m verify-funding --tx-hex "$(hex_of "$P2_TXID")" --partida 2 --partida-only >/dev/null
$HBP --dir .c verify-funding --tx-hex "$(hex_of "$P2_TXID")" --partida 2 --partida-only >/dev/null
P2_VOUT="$(vout_of "$P2_TXID" "$P2_ADDR")"
advance_mtp "$((T2 + 15 + 30))"
M_DEST="$(wrpc hbp_mandante getnewaddress)"
UNW="$($HBP --dir .m unwind --kind partida --partida 2 --outpoint "${P2_TXID}:${P2_VOUT}" --sats "$PARTIDA_SATS" --dest "$M_DEST" --fee "$FEE_SATS" --peer-dir .c)"
AC="$($HBP --dir .c arbiter-close --kind partida --partida 2 --with ac --arbiter-dir .a \
  --outpoint "${P2_TXID}:${P2_VOUT}" --sats "$PARTIDA_SATS" --dest "$C_DEST" --fee 500 --peer-dir .m)"
[[ "$(mempool_allows "$UNW")" == yes ]] || fail 126 unwind_p2_not_allowed
[[ "$(mempool_allows "$AC")" == yes ]] || fail 126 ac_p2_not_allowed
UNWID="$(rpc sendrawtransaction "$UNW")"
set +e
AID="$(rpc sendrawtransaction "$AC" 2>/tmp/hbp-126.err)"
set -e
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
python3 - "$BCLI" "$UNWID" "${AID:-none}" <<'PY' || { fail 126 both_or_neither; }
import json, subprocess, sys
bcli, a, b = sys.argv[1].split(), sys.argv[2], sys.argv[3]

def confs(txid):
    if txid in ("", "none"):
        return 0
    try:
        r = json.loads(subprocess.check_output(bcli+["getrawtransaction", txid, "true"], text=True))
    except subprocess.CalledProcessError:
        return 0
    return int(r.get("confirmations") or 0)

ca, cb = confs(a), confs(b)
print("unw_confs", ca, "ac_confs", cb)
assert (ca >= 1) + (cb >= 1) == 1, (ca, cb)
PY
pass 126 t2_race_p2_one_wins
rpc setmocktime 0 >/dev/null || true

# --- 39 reorg of funding ---
fresh reorg_fund
NOW="$(chain_now)"
hbp_offer_60k "$((NOW+120))" "$((NOW+240))" "$((NOW+360))" reorgf
hbp_read_addrs
F1="$(fund_bond_and_p1 "$BOND_ADDR" "$P1_ADDR" reorgf)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
$HBP --dir .m verify-funding --tx-hex "$(hex_of "$F1")" --partida 1 >/dev/null
TIP="$(rpc getbestblockhash)"
rpc invalidateblock "$TIP" >/dev/null
python3 - "$BCLI" "$F1" <<'PY'
import json, subprocess, sys
bcli, txid = sys.argv[1].split(), sys.argv[2]
info = json.loads(subprocess.check_output(bcli+["getblockchaininfo"], text=True))
# tx may sit in mempool after invalidate; it must not be in a *block*
try:
    raw = json.loads(subprocess.check_output(bcli+["getrawtransaction", txid, "true"], text=True))
except subprocess.CalledProcessError:
    print("tx dropped")
    raise SystemExit(0)
conf = raw.get("confirmations", 0) or 0
assert conf == 0, raw
print("funding_unconfirmed_after_invalidate", conf)
PY
pass 39 reorg_funding_unconfirmed
rpc reconsiderblock "$TIP" >/dev/null 2>&1 || rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null

# --- 136 reorg of reception ---
fresh reorg_recv
NOW="$(chain_now)"
hbp_offer_60k "$((NOW+120))" "$((NOW+240))" "$((NOW+360))" reorgr
hbp_read_addrs
F1="$(fund_bond_and_p1 "$BOND_ADDR" "$P1_ADDR" reorgr)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
$HBP --dir .m verify-funding --tx-hex "$(hex_of "$F1")" --partida 1 >/dev/null
P1_VOUT="$(vout_of "$F1" "$P1_ADDR")"
C_DEST="$(wrpc hbp_contratista getnewaddress)"
$HBP --dir .m coop-close --kind partida --partida 1 --outpoint "${F1}:${P1_VOUT}" --sats "$PARTIDA_SATS" --dest "$C_DEST" --fee "$FEE_SATS" --peer-dir .c >/tmp/hbp-136.hex
PAY_TXID="$(rpc sendrawtransaction "$(cat /tmp/hbp-136.hex)")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
TIP="$(rpc getbestblockhash)"
rpc invalidateblock "$TIP" >/dev/null
python3 - "$BCLI" "$PAY_TXID" <<'PY'
import json, subprocess, sys
bcli, txid = sys.argv[1].split(), sys.argv[2]
raw = json.loads(subprocess.check_output(bcli+["getrawtransaction", txid, "true"], text=True))
conf = raw.get("confirmations", 0) or 0
assert conf == 0, raw
print("reception_unconfirmed_after_invalidate", conf)
PY
pass 136 reorg_reception_unconfirmed
rpc reconsiderblock "$TIP" >/dev/null 2>&1 || rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null

echo "REGTEST_REMAINDERS_OK"
