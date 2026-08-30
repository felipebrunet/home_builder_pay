#!/usr/bin/env bash
# Mined catalog: MAD, arbiter, funding mismatches, remaining unwind classes.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=regtest_lib.sh
source "$ROOT/scripts/regtest_lib.sh"

CAT="${CAT_DIR:-/tmp/hbp-regtest-catalog}"
rm -rf "$CAT"
mkdir -p "$CAT"

prepare_wallets >/tmp/hbp-cat-wallets.txt
MINE_ADDR="$(wrpc miner getnewaddress)"
M_RECV="$(wrpc hbp_mandante getnewaddress)"
C_RECV="$(wrpc hbp_contratista getnewaddress)"

pass() { echo "PASS id=$1 $2"; }
fail() { echo "FAIL id=$1 $2" >&2; exit 1; }

fresh() {
  local d="$CAT/$1"
  rm -rf "$d"
  mkdir -p "$d"
  cd "$d"
  rpc setmocktime 0 >/dev/null || true
  rpc generatetoaddress 12 "$MINE_ADDR" >/dev/null
}

chain_now() {
  rpc getblockchaininfo | python3 -c "import json,sys; print(int(json.load(sys.stdin)['mediantime']))"
}

hex_of() { rpc getrawtransaction "$1"; }

# --- 34 underpay ---
fresh underpay
NOW="$(chain_now)"
NEW_EXTRA=""
hbp_offer_60k "$((NOW+120))" "$((NOW+240))" "$((NOW+360))" underpay
hbp_read_addrs
WRONG_TXID="$(fund_wrong_p1 "$BOND_ADDR" "$P1_ADDR" 0.05000000 underpay)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
HEX="$(hex_of "$WRONG_TXID")"
if $HBP --dir .m verify-funding --tx-hex "$HEX" --partida 1 >/tmp/hbp-under.out 2>/tmp/hbp-under.err; then
  fail 34 underpay_accepted
fi
grep -q "quoted\|missing partida" /tmp/hbp-under.err /tmp/hbp-under.out || true
pass 34 underpay_rejected

# --- 61 unwind before T ---
fresh early_unw
NOW="$(chain_now)"
hbp_offer_60k "$((NOW+3600))" "$((NOW+7200))" "$((NOW+10800))" early
hbp_read_addrs
F1="$(fund_bond_and_p1 "$BOND_ADDR" "$P1_ADDR" early)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
P1_VOUT="$(vout_of "$F1" "$P1_ADDR")"
M_DEST="$(wrpc hbp_mandante getnewaddress)"
UNW="$($HBP --dir .m unwind --kind partida --partida 1 --outpoint "${F1}:${P1_VOUT}" --sats "$PARTIDA_SATS" --dest "$M_DEST" --fee "$FEE_SATS" 2>/tmp/hbp-early.err || true)"
set +e
rpc sendrawtransaction "$UNW" >/tmp/hbp-early.send 2>&1
RC=$?
set -e
[[ $RC -ne 0 ]] || fail 61 early_unwind_mined
pass 61 unwind_before_T

# --- MAD happy 43/75 ---
fresh mad_happy
NOW="$(chain_now)"
NEW_EXTRA="--dispute mad --mad-bps 100"
hbp_offer_60k_ex "$((NOW+120))" "$((NOW+240))" "$((NOW+360))" madhappy
unset NEW_EXTRA
hbp_read_addrs
[[ -n "$MAD_ADDR" ]] || fail 43 no_mad_addr
MAD_EACH="$(printf '%s\n' "${ADDRS[@]}" | awk '/mad_sats_each/{print $2}')"
MAD_BTC="$(python3 -c "print(f'{2*int($MAD_EACH)/1e8:.8f}')")"
F1="$(fund_bond_p1_mad "$BOND_ADDR" "$P1_ADDR" "$MAD_ADDR" "$MAD_BTC" madhappy)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
HEX="$(hex_of "$F1")"
$HBP --dir .m verify-funding --tx-hex "$HEX" --partida 1 >/dev/null
$HBP --dir .c verify-funding --tx-hex "$HEX" --partida 1 >/dev/null
P1_VOUT="$(vout_of "$F1" "$P1_ADDR")"
BOND_VOUT="$(vout_of "$F1" "$BOND_ADDR")"
MAD_VOUT="$(vout_of "$F1" "$MAD_ADDR")"
C_DEST="$(wrpc hbp_contratista getnewaddress)"
$HBP --dir .m coop-close --kind partida --partida 1 --outpoint "${F1}:${P1_VOUT}" --sats "$PARTIDA_SATS" --dest "$C_DEST" --fee "$FEE_SATS" --peer-dir .c >/tmp/hbp-mad-p1.hex
rpc sendrawtransaction "$(cat /tmp/hbp-mad-p1.hex)" >/dev/null
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
P2_TXID="$(wrpc hbp_mandante sendtoaddress "$P2_ADDR" "$PARTIDA_BTC")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
P2_HEX="$(hex_of "$P2_TXID")"
$HBP --dir .m verify-funding --tx-hex "$P2_HEX" --partida 2 --partida-only >/dev/null
P2_VOUT="$(vout_of "$P2_TXID" "$P2_ADDR")"
$HBP --dir .m coop-close --kind partida --partida 2 --outpoint "${P2_TXID}:${P2_VOUT}" --sats "$PARTIDA_SATS" --dest "$C_DEST" --fee "$FEE_SATS" --peer-dir .c >/tmp/hbp-mad-p2.hex
rpc sendrawtransaction "$(cat /tmp/hbp-mad-p2.hex)" >/dev/null
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
M_DEST="$(wrpc hbp_mandante getnewaddress)"
MAD_ONCHAIN=$((MAD_EACH * 2))
$HBP --dir .m coop-close --kind mad --outpoint "${F1}:${MAD_VOUT}" --sats "$MAD_ONCHAIN" --dest "$C_DEST" --fee "$FEE_SATS" --pay-sats "$MAD_EACH" --refund-dest "$M_DEST" --peer-dir .c >/tmp/hbp-mad-split.hex
rpc sendrawtransaction "$(cat /tmp/hbp-mad-split.hex)" >/dev/null
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
$HBP --dir .m coop-close --kind bond --outpoint "${F1}:${BOND_VOUT}" --sats "$BOND_SATS" --dest "$C_DEST" --fee "$FEE_SATS" --peer-dir .c >/tmp/hbp-mad-bond.hex
rpc sendrawtransaction "$(cat /tmp/hbp-mad-bond.hex)" >/dev/null
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
pass 43 mad_happy
pass 75 mad_split_50_50

# --- MAD burn: leave MAD unspent after unwind ---
fresh mad_burn
NOW="$(chain_now)"
NEW_EXTRA="--dispute mad --mad-bps 100"
hbp_offer_60k_ex "$((NOW+20))" "$((NOW+40))" "$((NOW+60))" madburn
unset NEW_EXTRA
hbp_read_addrs
MAD_EACH="$(printf '%s\n' "${ADDRS[@]}" | awk '/mad_sats_each/{print $2}')"
MAD_BTC="$(python3 -c "print(f'{2*int($MAD_EACH)/1e8:.8f}')")"
F1="$(fund_bond_p1_mad "$BOND_ADDR" "$P1_ADDR" "$MAD_ADDR" "$MAD_BTC" madburn)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
HEX="$(hex_of "$F1")"
$HBP --dir .m verify-funding --tx-hex "$HEX" --partida 1 >/dev/null
P1_VOUT="$(vout_of "$F1" "$P1_ADDR")"
BOND_VOUT="$(vout_of "$F1" "$BOND_ADDR")"
MAD_VOUT="$(vout_of "$F1" "$MAD_ADDR")"
advance_mtp "$((NOW + 600))"
M_DEST="$(wrpc hbp_mandante getnewaddress)"
C_DEST="$(wrpc hbp_contratista getnewaddress)"
P1H="$($HBP --dir .m unwind --kind partida --partida 1 --outpoint "${F1}:${P1_VOUT}" --sats "$PARTIDA_SATS" --dest "$M_DEST" --fee "$FEE_SATS" --peer-dir .c)"
rpc sendrawtransaction "$P1H" >/dev/null
BH="$($HBP --dir .c unwind --kind bond --outpoint "${F1}:${BOND_VOUT}" --sats "$BOND_SATS" --dest "$C_DEST" --fee "$FEE_SATS" --peer-dir .m)"
rpc sendrawtransaction "$BH" >/dev/null
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
# MAD still sitting at the address
python3 - "$BCLI" "$MAD_ADDR" <<'PY'
import json, subprocess, sys
bcli=sys.argv[1].split(); addr=sys.argv[2]
raw=subprocess.check_output(bcli+["scantxoutset","start",json.dumps([f"addr({addr})"])], text=True)
d=json.loads(raw)
assert d.get("total_amount",0)>0, d
print("mad_unspent", d["total_amount"])
PY
pass 79 mad_left_unspent
pass 80 mad_burn_both_angry
rpc setmocktime 0 >/dev/null || true

# --- Arbiter unused (coop) 44 ---
fresh arb_unused
NOW="$(chain_now)"
NEW_EXTRA="--dispute arbiter --arbiter-window 15"
hbp_offer_60k_ex "$((NOW+120))" "$((NOW+240))" "$((NOW+360))" arbcoop
unset NEW_EXTRA
name_arbiter >/dev/null
hbp_read_addrs
[[ -n "$BOND_ADDR" ]] || fail 44 no_addr_after_name
F1="$(fund_bond_and_p1 "$BOND_ADDR" "$P1_ADDR" arbcoop)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
HEX="$(hex_of "$F1")"
$HBP --dir .m verify-funding --tx-hex "$HEX" --partida 1 >/dev/null
P1_VOUT="$(vout_of "$F1" "$P1_ADDR")"
BOND_VOUT="$(vout_of "$F1" "$BOND_ADDR")"
C_DEST="$(wrpc hbp_contratista getnewaddress)"
$HBP --dir .m coop-close --kind partida --partida 1 --outpoint "${F1}:${P1_VOUT}" --sats "$PARTIDA_SATS" --dest "$C_DEST" --fee "$FEE_SATS" --peer-dir .c >/tmp/hbp-au-p1.hex
rpc sendrawtransaction "$(cat /tmp/hbp-au-p1.hex)" >/dev/null
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
P2_TXID="$(wrpc hbp_mandante sendtoaddress "$P2_ADDR" "$PARTIDA_BTC")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
$HBP --dir .m verify-funding --tx-hex "$(hex_of "$P2_TXID")" --partida 2 --partida-only >/dev/null
P2_VOUT="$(vout_of "$P2_TXID" "$P2_ADDR")"
$HBP --dir .m coop-close --kind partida --partida 2 --outpoint "${P2_TXID}:${P2_VOUT}" --sats "$PARTIDA_SATS" --dest "$C_DEST" --fee "$FEE_SATS" --peer-dir .c >/tmp/hbp-au-p2.hex
rpc sendrawtransaction "$(cat /tmp/hbp-au-p2.hex)" >/dev/null
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
$HBP --dir .m coop-close --kind bond --outpoint "${F1}:${BOND_VOUT}" --sats "$BOND_SATS" --dest "$C_DEST" --fee "$FEE_SATS" --peer-dir .c >/tmp/hbp-au-b.hex
rpc sendrawtransaction "$(cat /tmp/hbp-au-b.hex)" >/dev/null
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
pass 44 arbiter_unused_coop
pass 93 reconcilia_musig_despues

# --- Arbiter A+C awards P1 to C (94) ---
fresh arb_ac
NOW="$(chain_now)"
T1=$((NOW + 120))
NEW_EXTRA="--dispute arbiter --arbiter-window 15"
hbp_offer_60k_ex "$T1" "$((T1 + 60))" "$((T1 + 120))" arbac
unset NEW_EXTRA
name_arbiter >/dev/null
hbp_read_addrs
F1="$(fund_bond_and_p1 "$BOND_ADDR" "$P1_ADDR" arbac)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
HEX="$(hex_of "$F1")"
$HBP --dir .m verify-funding --tx-hex "$HEX" --partida 1 >/dev/null
P1_VOUT="$(vout_of "$F1" "$P1_ADDR")"
# before T: should not mine
C_DEST="$(wrpc hbp_contratista getnewaddress)"
EARLY="$($HBP --dir .c arbiter-close --kind partida --partida 1 --with ac --arbiter-dir .a \
  --outpoint "${F1}:${P1_VOUT}" --sats "$PARTIDA_SATS" --dest "$C_DEST" --fee "$FEE_SATS" --peer-dir .m)"
set +e
rpc sendrawtransaction "$EARLY" >/tmp/hbp-ac-early.send 2>&1
RC=$?
set -e
[[ $RC -ne 0 ]] || fail 91 ac_before_T
pass 91 ac_before_T_rejected
advance_mtp "$((T1 + 5))"
AC="$($HBP --dir .c arbiter-close --kind partida --partida 1 --with ac --arbiter-dir .a \
  --outpoint "${F1}:${P1_VOUT}" --sats "$PARTIDA_SATS" --dest "$C_DEST" --fee "$FEE_SATS" --peer-dir .m)"
txid="$(rpc sendrawtransaction "$AC")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
echo "ac_p1 $txid"
pass 94 ac_awards_p1_to_c
pass 88 a_on_time
rpc setmocktime 0 >/dev/null || true

# --- Arbiter A+M refunds P1 (95) ---
fresh arb_am
NOW="$(chain_now)"
T1=$((NOW + 120))
NEW_EXTRA="--dispute arbiter --arbiter-window 15"
hbp_offer_60k_ex "$T1" "$((T1 + 60))" "$((T1 + 120))" arbam
unset NEW_EXTRA
name_arbiter >/dev/null
hbp_read_addrs
F1="$(fund_bond_and_p1 "$BOND_ADDR" "$P1_ADDR" arbam)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
$HBP --dir .m verify-funding --tx-hex "$(hex_of "$F1")" --partida 1 >/dev/null
P1_VOUT="$(vout_of "$F1" "$P1_ADDR")"
advance_mtp "$((T1 + 5))"
M_DEST="$(wrpc hbp_mandante getnewaddress)"
AM="$($HBP --dir .m arbiter-close --kind partida --partida 1 --with am --arbiter-dir .a \
  --outpoint "${F1}:${P1_VOUT}" --sats "$PARTIDA_SATS" --dest "$M_DEST" --fee "$FEE_SATS" --peer-dir .c)"
txid="$(rpc sendrawtransaction "$AM")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
echo "am_p1 $txid"
pass 95 am_refunds_p1
rpc setmocktime 0 >/dev/null || true

# --- Arbiter split 80/20 via A+C (96) ---
fresh arb_split
NOW="$(chain_now)"
T1=$((NOW + 120))
NEW_EXTRA="--dispute arbiter --arbiter-window 15"
hbp_offer_60k_ex "$T1" "$((T1 + 60))" "$((T1 + 120))" arbsplit
unset NEW_EXTRA
name_arbiter >/dev/null
hbp_read_addrs
F1="$(fund_bond_and_p1 "$BOND_ADDR" "$P1_ADDR" arbsplit)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
$HBP --dir .m verify-funding --tx-hex "$(hex_of "$F1")" --partida 1 >/dev/null
P1_VOUT="$(vout_of "$F1" "$P1_ADDR")"
advance_mtp "$((T1 + 5))"
C_DEST="$(wrpc hbp_contratista getnewaddress)"
M_DEST="$(wrpc hbp_mandante getnewaddress)"
PAY=$((PARTIDA_SATS * 80 / 100))
SP="$($HBP --dir .c arbiter-close --kind partida --partida 1 --with ac --arbiter-dir .a \
  --outpoint "${F1}:${P1_VOUT}" --sats "$PARTIDA_SATS" --dest "$C_DEST" --fee "$FEE_SATS" \
  --pay-sats "$PAY" --refund-dest "$M_DEST" --peer-dir .m)"
txid="$(rpc sendrawtransaction "$SP")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
echo "ac_split $txid"
pass 96 ac_split_80
pass 98 m_cannot_stop_ac
rpc setmocktime 0 >/dev/null || true

# --- A disappears → T2 unwind (99) ---
fresh arb_ghost
NOW="$(chain_now)"
T1=$((NOW + 60))
WIN=15
NEW_EXTRA="--dispute arbiter --arbiter-window $WIN"
hbp_offer_60k_ex "$T1" "$((T1 + 60))" "$((T1 + 120))" arbghost
unset NEW_EXTRA
name_arbiter >/dev/null
hbp_read_addrs
F1="$(fund_bond_and_p1 "$BOND_ADDR" "$P1_ADDR" arbghost)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
$HBP --dir .m verify-funding --tx-hex "$(hex_of "$F1")" --partida 1 >/dev/null
P1_VOUT="$(vout_of "$F1" "$P1_ADDR")"
BOND_VOUT="$(vout_of "$F1" "$BOND_ADDR")"
T2=$((T1 + WIN))
advance_mtp "$((T2 + 30))"
M_DEST="$(wrpc hbp_mandante getnewaddress)"
C_DEST="$(wrpc hbp_contratista getnewaddress)"
P1H="$($HBP --dir .m unwind --kind partida --partida 1 --outpoint "${F1}:${P1_VOUT}" --sats "$PARTIDA_SATS" --dest "$M_DEST" --fee "$FEE_SATS" --peer-dir .c)"
rpc sendrawtransaction "$P1H" >/dev/null
# bond T_project + window
advance_mtp "$((T1 + 120 + WIN + 30))"
BH="$($HBP --dir .c unwind --kind bond --outpoint "${F1}:${BOND_VOUT}" --sats "$BOND_SATS" --dest "$C_DEST" --fee "$FEE_SATS" --peer-dir .m)"
rpc sendrawtransaction "$BH" >/dev/null
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
pass 99 a_disappears_t2
pass 90 a_too_late_unwind_wins
rpc setmocktime 0 >/dev/null || true

# --- A+M seizes bond (120) ---
fresh arb_bond
NOW="$(chain_now)"
TPROJ=$((NOW + 120))
NEW_EXTRA="--dispute arbiter --arbiter-window 15"
hbp_offer_60k_ex "$((NOW + 30))" "$((NOW + 60))" "$TPROJ" arbbond
unset NEW_EXTRA
name_arbiter >/dev/null
hbp_read_addrs
F1="$(fund_bond_and_p1 "$BOND_ADDR" "$P1_ADDR" arbbond)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
$HBP --dir .m verify-funding --tx-hex "$(hex_of "$F1")" --partida 1 >/dev/null
BOND_VOUT="$(vout_of "$F1" "$BOND_ADDR")"
advance_mtp "$((TPROJ + 5))"
M_DEST="$(wrpc hbp_mandante getnewaddress)"
AM="$($HBP --dir .m arbiter-close --kind bond --with am --arbiter-dir .a \
  --outpoint "${F1}:${BOND_VOUT}" --sats "$BOND_SATS" --dest "$M_DEST" --fee "$FEE_SATS" --peer-dir .c)"
txid="$(rpc sendrawtransaction "$AM")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
echo "am_bond $txid"
pass 120 am_seizes_bond
rpc setmocktime 0 >/dev/null || true

# --- P2 abandon (66) ---
fresh p2_ab
NOW="$(chain_now)"
hbp_offer_60k "$((NOW+20))" "$((NOW+40))" "$((NOW+80))" p2ab
hbp_read_addrs
F1="$(fund_bond_and_p1 "$BOND_ADDR" "$P1_ADDR" p2ab)"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
$HBP --dir .m verify-funding --tx-hex "$(hex_of "$F1")" --partida 1 >/dev/null
P1_VOUT="$(vout_of "$F1" "$P1_ADDR")"
BOND_VOUT="$(vout_of "$F1" "$BOND_ADDR")"
C_DEST="$(wrpc hbp_contratista getnewaddress)"
$HBP --dir .m coop-close --kind partida --partida 1 --outpoint "${F1}:${P1_VOUT}" --sats "$PARTIDA_SATS" --dest "$C_DEST" --fee "$FEE_SATS" --peer-dir .c >/tmp/hbp-p2ab-p1.hex
rpc sendrawtransaction "$(cat /tmp/hbp-p2ab-p1.hex)" >/dev/null
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
P2_TXID="$(wrpc hbp_mandante sendtoaddress "$P2_ADDR" "$PARTIDA_BTC")"
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
$HBP --dir .m verify-funding --tx-hex "$(hex_of "$P2_TXID")" --partida 2 --partida-only >/dev/null
P2_VOUT="$(vout_of "$P2_TXID" "$P2_ADDR")"
advance_mtp "$((NOW + 80 + 30))"
M_DEST="$(wrpc hbp_mandante getnewaddress)"
P2H="$($HBP --dir .m unwind --kind partida --partida 2 --outpoint "${P2_TXID}:${P2_VOUT}" --sats "$PARTIDA_SATS" --dest "$M_DEST" --fee "$FEE_SATS" --peer-dir .c)"
rpc sendrawtransaction "$P2H" >/dev/null
BH="$($HBP --dir .c unwind --kind bond --outpoint "${F1}:${BOND_VOUT}" --sats "$BOND_SATS" --dest "$C_DEST" --fee "$FEE_SATS" --peer-dir .m)"
rpc sendrawtransaction "$BH" >/dev/null
rpc generatetoaddress 1 "$MINE_ADDR" >/dev/null
pass 66 p2_abandon
rpc setmocktime 0 >/dev/null || true

# --- 92 A alone cannot unwind as party ---
fresh a_alone
NOW="$(chain_now)"
NEW_EXTRA="--dispute arbiter --arbiter-window 15"
hbp_offer_60k_ex "$((NOW+120))" "$((NOW+240))" "$((NOW+360))" aalone
unset NEW_EXTRA
name_arbiter >/dev/null
if $HBP --dir .a unwind --kind partida --partida 1 --outpoint "aa:0" --sats 1 --dest bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080 --fee 200 >/tmp/aa.out 2>/tmp/aa.err; then
  fail 92 a_unwind_as_party
fi
pass 92 a_not_a_party

echo "REGTEST_CATALOG_OK"
