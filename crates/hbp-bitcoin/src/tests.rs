use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::hashes::Hash;
use bitcoin::key::CompressedPublicKey;
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1::{Message, Secp256k1};
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::{
    Address, Amount, Network as BtcNetwork, OutPoint, PublicKey, ScriptBuf, Txid,
};
use hbp_core::Network;

use crate::fund::{build_funding_psbt, extract_signed_funding_tx, FundingCoin, FundingRequest};
use crate::p2wsh::escrow_at;
use crate::spend::{build_burn_psbt, extract_wsh_tx};
use crate::watch::{import_watch, scan_watch, WatchKind};

fn xprv(seed: u8) -> Xpriv {
    Xpriv::new_master(BtcNetwork::Testnet, &[seed; 32]).expect("xprv")
}

fn account_xpub(seed: u8, path: &str) -> String {
    let secp = Secp256k1::new();
    let master = xprv(seed);
    let path: DerivationPath = path.parse().unwrap();
    let acc = master.derive_priv(&secp, &path).unwrap();
    Xpub::from_priv(&secp, &acc).to_string()
}

fn child_sk(seed: u8, path: &str) -> bitcoin::secp256k1::SecretKey {
    let secp = Secp256k1::new();
    let master = xprv(seed);
    let path: DerivationPath = path.parse().unwrap();
    master.derive_priv(&secp, &path).unwrap().private_key
}

fn wpkh_addr(seed: u8, path: &str) -> (Address, ScriptBuf, bitcoin::secp256k1::SecretKey) {
    let secp = Secp256k1::new();
    let sk = child_sk(seed, path);
    let pk = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &sk);
    let cp = CompressedPublicKey(pk);
    let addr = Address::p2wpkh(&cp, BtcNetwork::Signet);
    let spk = addr.script_pubkey();
    (addr, spk, sk)
}

fn fake_outpoint(n: u8) -> OutPoint {
    OutPoint {
        txid: Txid::from_byte_array([n; 32]),
        vout: 0,
    }
}

fn sign_wpkh(psbt: &mut Psbt, index: usize, sk: &bitcoin::secp256k1::SecretKey) {
    let secp = Secp256k1::new();
    let pk = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, sk);
    let utxo = psbt.inputs[index].witness_utxo.clone().unwrap();
    let mut cache = SighashCache::new(&psbt.unsigned_tx);
    let sighash = cache
        .p2wpkh_signature_hash(
            index,
            &utxo.script_pubkey,
            utxo.value,
            EcdsaSighashType::All,
        )
        .unwrap();
    let msg = Message::from_digest(*sighash.as_byte_array());
    let sig = secp.sign_ecdsa(&msg, sk);
    let mut ser = sig.serialize_der().to_vec();
    ser.push(EcdsaSighashType::All as u8);
    let ecdsa = bitcoin::ecdsa::Signature::from_slice(&ser).unwrap();
    psbt.inputs[index]
        .partial_sigs
        .insert(PublicKey::new(pk), ecdsa);
}

fn sign_wsh(psbt: &mut Psbt, sk: &bitcoin::secp256k1::SecretKey) {
    let secp = Secp256k1::new();
    let pk = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, sk);
    let utxo = psbt.inputs[0].witness_utxo.clone().unwrap();
    let ws = psbt.inputs[0].witness_script.clone().unwrap();
    let mut cache = SighashCache::new(&psbt.unsigned_tx);
    let sighash = cache
        .p2wsh_signature_hash(0, &ws, utxo.value, EcdsaSighashType::All)
        .unwrap();
    let msg = Message::from_digest(*sighash.as_byte_array());
    let sig = secp.sign_ecdsa(&msg, sk);
    let mut ser = sig.serialize_der().to_vec();
    ser.push(EcdsaSighashType::All as u8);
    let ecdsa = bitcoin::ecdsa::Signature::from_slice(&ser).unwrap();
    psbt.inputs[0]
        .partial_sigs
        .insert(PublicKey::new(pk), ecdsa);
}

#[test]
fn watch_scan_stops_at_gap_and_picks_change() {
    let acc = import_watch(
        &account_xpub(9, "m/84'/1'/0'"),
        Some(WatchKind::Wpkh),
        Network::Signet,
        5,
    )
    .unwrap();
    let funded = crate::address_at(&acc.receive_descriptor, 0, Network::Signet).unwrap();
    let scan = scan_watch(&acc, |addr| {
        if addr == &funded {
            Ok(vec![(fake_outpoint(1), 50_000, true)])
        } else {
            Ok(vec![])
        }
    })
    .unwrap();
    assert_eq!(scan.utxos.len(), 1);
    assert_eq!(scan.utxos[0].sats, 50_000);
    assert_eq!(
        scan.receive,
        crate::address_at(&acc.receive_descriptor, 1, Network::Signet)
            .unwrap()
            .to_string()
    );
}

#[test]
fn funding_and_burn_roundtrip() {
    let m48_a = account_xpub(1, "m/48'/1'/0'/2'");
    let m48_b = account_xpub(2, "m/48'/1'/0'/2'");
    let escrow = escrow_at(&m48_a, &m48_b, 0, Network::Signet).unwrap();

    let (_addr_a, spk_a, sk_a) = wpkh_addr(1, "m/84'/1'/0'/0/0");
    let (_addr_b, spk_b, sk_b) = wpkh_addr(2, "m/84'/1'/0'/0/0");
    let (chg_a, _, _) = wpkh_addr(1, "m/84'/1'/0'/1/0");
    let (chg_b, _, _) = wpkh_addr(2, "m/84'/1'/0'/1/0");

    let req = FundingRequest {
        escrow: escrow.script_pubkey.clone(),
        escrow_sats: 10_000,
        fee: 500,
        mandante: FundingCoin {
            outpoint: fake_outpoint(10),
            sats: 20_000,
            script_pubkey: spk_a,
        },
        mandante_change: chg_a,
        contratista: FundingCoin {
            outpoint: fake_outpoint(11),
            sats: 20_000,
            script_pubkey: spk_b,
        },
        contratista_change: chg_b,
    };
    let mut psbt = build_funding_psbt(&req).unwrap();
    sign_wpkh(&mut psbt, 0, &sk_a);
    sign_wpkh(&mut psbt, 1, &sk_b);
    let funding = extract_signed_funding_tx(psbt).unwrap();
    assert_eq!(funding.output[0].value, Amount::from_sat(10_000));
    assert_eq!(funding.output[0].script_pubkey, escrow.script_pubkey);
    let txid = funding.compute_txid();

    let t = 1_800_000_000u32;
    let mut burn = build_burn_psbt(
        &escrow,
        OutPoint { txid, vout: 0 },
        10_000,
        t,
    )
    .unwrap();
    assert_eq!(burn.unsigned_tx.lock_time.to_consensus_u32(), t);
    let sk48_a = child_sk(1, "m/48'/1'/0'/2'/0/0");
    let sk48_b = child_sk(2, "m/48'/1'/0'/2'/0/0");
    sign_wsh(&mut burn, &sk48_a);
    sign_wsh(&mut burn, &sk48_b);
    let burn_tx = extract_wsh_tx(burn).unwrap();
    assert_eq!(burn_tx.output.len(), 1);
    assert_eq!(burn_tx.output[0].value, Amount::from_sat(0));
    assert!(burn_tx.output[0].script_pubkey.is_op_return());
    assert_eq!(burn_tx.lock_time.to_consensus_u32(), t);
    // 2-of-2 witness: dummy + 2 sigs + script
    assert_eq!(burn_tx.input[0].witness.len(), 4);
}

#[test]
fn sortedmulti_same_address_swapped_keys() {
    let a = account_xpub(3, "m/48'/1'/0'/2'");
    let b = account_xpub(4, "m/48'/1'/0'/2'");
    let e1 = escrow_at(&a, &b, 0, Network::Signet).unwrap();
    let e2 = escrow_at(&b, &a, 0, Network::Signet).unwrap();
    assert_eq!(e1.address, e2.address);
}

#[test]
fn import_watch_rejects_mainnet_on_signet() {
    // xpub (mainnet version) must fail on signet.
    let main = {
        let secp = Secp256k1::new();
        let x = Xpriv::new_master(BtcNetwork::Bitcoin, &[7u8; 32]).unwrap();
        Xpub::from_priv(&secp, &x).to_string()
    };
    assert!(import_watch(&main, Some(WatchKind::Wpkh), Network::Signet, 5).is_err());
}
