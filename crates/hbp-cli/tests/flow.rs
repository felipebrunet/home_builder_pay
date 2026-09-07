//! End-to-end coordinator: two dirs, hold and burn, no wallet app.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::hashes::Hash;
use bitcoin::key::CompressedPublicKey;
use bitcoin::psbt::Psbt;
use bitcoin::secp256k1::{Message, Secp256k1};
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::{
    Address, Network as BtcNetwork, OutPoint, PublicKey, Txid,
};
use hbp_core::Role;

fn hbp() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_hbp"))
}

fn run(dir: &Path, args: &[&str]) -> (String, String) {
    let out = Command::new(hbp())
        .arg("--dir")
        .arg(dir)
        .args(args)
        .output()
        .expect("run hbp");
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        out.status.success(),
        "hbp {args:?} failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    (stdout, stderr)
}

fn xprv(seed: u8) -> Xpriv {
    Xpriv::new_master(BtcNetwork::Testnet, &[seed; 32]).unwrap()
}

fn account_xpub(seed: u8, path: &str) -> String {
    let secp = Secp256k1::new();
    let path: DerivationPath = path.parse().unwrap();
    let acc = xprv(seed).derive_priv(&secp, &path).unwrap();
    Xpub::from_priv(&secp, &acc).to_string()
}

fn child_sk(seed: u8, path: &str) -> bitcoin::secp256k1::SecretKey {
    let secp = Secp256k1::new();
    let path: DerivationPath = path.parse().unwrap();
    xprv(seed).derive_priv(&secp, &path).unwrap().private_key
}

fn wpkh(seed: u8, path: &str) -> (Address, bitcoin::secp256k1::SecretKey) {
    let secp = Secp256k1::new();
    let sk = child_sk(seed, path);
    let pk = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &sk);
    let addr = Address::p2wpkh(&CompressedPublicKey(pk), BtcNetwork::Regtest);
    (addr, sk)
}

fn fake_outpoint(n: u8) -> String {
    OutPoint {
        txid: Txid::from_byte_array([n; 32]),
        vout: 0,
    }
    .to_string()
}

fn write_coin(dir: &Path, role: Role, seed: u8, op: u8) {
    let (addr, _) = wpkh(seed, "m/84'/1'/0'/0/0");
    let (chg, _) = wpkh(seed, "m/84'/1'/0'/1/0");
    let role = match role {
        Role::Mandante => "mandante",
        Role::Contratista => "contratista",
    };
    let j = serde_json::json!({
        "role": role,
        "outpoint": fake_outpoint(op),
        "sats": 20_000,
        "address": addr.to_string(),
        "change": chg.to_string(),
    });
    fs::write(dir.join("05-coin.json"), serde_json::to_string_pretty(&j).unwrap()).unwrap();
}

fn sign_wpkh(psbt: &mut Psbt, index: usize, sk: &bitcoin::secp256k1::SecretKey) {
    let secp = Secp256k1::new();
    let pk = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, sk);
    let utxo = psbt.inputs[index].witness_utxo.clone().unwrap();
    let mut cache = SighashCache::new(&psbt.unsigned_tx);
    let sighash = cache
        .p2wpkh_signature_hash(index, &utxo.script_pubkey, utxo.value, EcdsaSighashType::All)
        .unwrap();
    let msg = Message::from_digest(*sighash.as_byte_array());
    let sig = secp.sign_ecdsa(&msg, sk);
    let mut ser = sig.serialize_der().to_vec();
    ser.push(EcdsaSighashType::All as u8);
    psbt.inputs[index]
        .partial_sigs
        .insert(PublicKey::new(pk), bitcoin::ecdsa::Signature::from_slice(&ser).unwrap());
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
    psbt.inputs[0]
        .partial_sigs
        .insert(PublicKey::new(pk), bitcoin::ecdsa::Signature::from_slice(&ser).unwrap());
}

fn load_psbt(path: &Path) -> Psbt {
    Psbt::deserialize(&fs::read(path).unwrap()).unwrap()
}

fn write_psbt(path: &Path, psbt: &Psbt) {
    fs::write(path, psbt.serialize()).unwrap();
}

fn scratch(name: &str) -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "hbp-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let m = root.join("m");
    let c = root.join("c");
    fs::create_dir_all(&m).unwrap();
    fs::create_dir_all(&c).unwrap();
    (m, c)
}

fn setup_parties(m: &Path, c: &Path) {
    run(m, &["init", "--network", "regtest", "--role", "mandante"]);
    run(c, &["init", "--network", "regtest", "--role", "contratista"]);
    run(m, &["cosigner", &account_xpub(1, "m/48'/1'/0'/2'")]);
    run(c, &["cosigner", &account_xpub(2, "m/48'/1'/0'/2'")]);
}

#[test]
fn hold_fund_and_coop() {
    let (m, c) = scratch("hold");
    setup_parties(&m, &c);
    run(
        &m,
        &["new", "--mode", "hold", "--sats", "5000", "--fee", "500"],
    );
    run(&m, &["offer"]);
    run(&c, &["accept", m.join("00-offer.json").to_str().unwrap()]);
    run(&m, &["import", c.join("01-accepted.json").to_str().unwrap()]);

    write_coin(&m, Role::Mandante, 1, 10);
    write_coin(&c, Role::Contratista, 2, 11);
    run(
        &m,
        &[
            "fund",
            "--mine",
            m.join("05-coin.json").to_str().unwrap(),
            "--peer",
            c.join("05-coin.json").to_str().unwrap(),
        ],
    );

    let mut fund = load_psbt(&m.join("funding.psbt"));
    let (_, sk_m) = wpkh(1, "m/84'/1'/0'/0/0");
    let (_, sk_c) = wpkh(2, "m/84'/1'/0'/0/0");
    sign_wpkh(&mut fund, 0, &sk_m);
    write_psbt(&m.join("funding.m.psbt"), &fund);
    // peer signs a copy of the unsigned + their sig only, then combine
    let mut fund_c = load_psbt(&m.join("funding.psbt"));
    sign_wpkh(&mut fund_c, 1, &sk_c);
    write_psbt(&c.join("funding.c.psbt"), &fund_c);

    let (hex, _) = run(
        &m,
        &[
            "combine-fund",
            m.join("funding.m.psbt").to_str().unwrap(),
            c.join("funding.c.psbt").to_str().unwrap(),
        ],
    );
    assert!(hex.trim().len() > 80, "expected funding hex, got {hex}");

    let status = run(&m, &["status"]).0;
    assert!(status.contains("funded"), "{status}");

    let (dest, _) = wpkh(2, "m/84'/1'/0'/0/1");
    run(
        &m,
        &["coop", "--dest", &dest.to_string(), "--fee", "200"],
    );
    let mut coop = load_psbt(&m.join("coop.psbt"));
    let sk48_m = child_sk(1, "m/48'/1'/0'/2'/0/0");
    let sk48_c = child_sk(2, "m/48'/1'/0'/2'/0/0");
    sign_wsh(&mut coop, &sk48_m);
    write_psbt(&m.join("coop.m.psbt"), &coop);
    let mut coop_c = load_psbt(&m.join("coop.psbt"));
    sign_wsh(&mut coop_c, &sk48_c);
    write_psbt(&c.join("coop.c.psbt"), &coop_c);
    let (hex, _) = run(
        &m,
        &[
            "combine-coop",
            m.join("coop.m.psbt").to_str().unwrap(),
            c.join("coop.c.psbt").to_str().unwrap(),
        ],
    );
    assert!(hex.trim().len() > 80);
    let status = run(&m, &["status"]).0;
    assert!(status.contains("closed"), "{status}");
}

#[test]
fn burn_requires_quema_before_funding() {
    let (m, c) = scratch("burn");
    setup_parties(&m, &c);
    run(
        &m,
        &[
            "new",
            "--mode",
            "burn",
            "--sats",
            "5000",
            "--fee",
            "500",
            "--t-unix",
            "1800000000",
        ],
    );
    run(&c, &["accept", m.join("00-offer.json").to_str().unwrap()]);
    run(&m, &["import", c.join("01-accepted.json").to_str().unwrap()]);
    write_coin(&m, Role::Mandante, 1, 20);
    write_coin(&c, Role::Contratista, 2, 21);
    run(
        &m,
        &[
            "fund",
            "--mine",
            m.join("05-coin.json").to_str().unwrap(),
            "--peer",
            c.join("05-coin.json").to_str().unwrap(),
        ],
    );
    assert!(m.join("burn.psbt").exists());

    let mut fund = load_psbt(&m.join("funding.psbt"));
    let (_, sk_m) = wpkh(1, "m/84'/1'/0'/0/0");
    let (_, sk_c) = wpkh(2, "m/84'/1'/0'/0/0");
    sign_wpkh(&mut fund, 0, &sk_m);
    write_psbt(&m.join("funding.m.psbt"), &fund);
    let mut fund_c = load_psbt(&m.join("funding.psbt"));
    sign_wpkh(&mut fund_c, 1, &sk_c);
    write_psbt(&c.join("funding.c.psbt"), &fund_c);

    let fail = Command::new(hbp())
        .arg("--dir")
        .arg(&m)
        .args([
            "combine-fund",
            m.join("funding.m.psbt").to_str().unwrap(),
            c.join("funding.c.psbt").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        !fail.status.success(),
        "funding must wait for combine-burn"
    );

    let mut burn = load_psbt(&m.join("burn.psbt"));
    let sk48_m = child_sk(1, "m/48'/1'/0'/2'/0/0");
    let sk48_c = child_sk(2, "m/48'/1'/0'/2'/0/0");
    sign_wsh(&mut burn, &sk48_m);
    write_psbt(&m.join("burn.m.psbt"), &burn);
    let mut burn_c = load_psbt(&m.join("burn.psbt"));
    sign_wsh(&mut burn_c, &sk48_c);
    write_psbt(&c.join("burn.c.psbt"), &burn_c);
    run(
        &m,
        &[
            "combine-burn",
            m.join("burn.m.psbt").to_str().unwrap(),
            c.join("burn.c.psbt").to_str().unwrap(),
        ],
    );
    run(
        &m,
        &[
            "combine-fund",
            m.join("funding.m.psbt").to_str().unwrap(),
            c.join("funding.c.psbt").to_str().unwrap(),
        ],
    );
    let (hex, _) = run(&m, &["publish-burn"]);
    assert!(hex.trim().len() > 80);
    let status = run(&m, &["status"]).0;
    assert!(status.contains("burned"), "{status}");
}
