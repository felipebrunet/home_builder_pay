//! Vanilla P2WSH `sortedmulti(2, A, B)` — same script for hold and burn.

use std::str::FromStr;

use bitcoin::secp256k1::{PublicKey, Secp256k1};
use bitcoin::{Address, ScriptBuf};
use hbp_core::Network;
use miniscript::descriptor::DescriptorType;
use miniscript::{Descriptor, DescriptorPublicKey};

use crate::convert::to_btc_network;
use crate::watch::slip132_to_xpub;
use crate::Error;

/// One side's BIP48 account key, stored as a ranged descriptor key (`…/0/*`).
pub fn normalize_cosigner_key(raw: &str) -> Result<String, Error> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(Error::msg("empty cosigner xpub"));
    }
    if raw.contains("sortedmulti") || raw.starts_with("wsh(") {
        return Err(Error::msg(
            "paste one cosigner xpub (Zpub/Vpub/tpub), not the full 2-of-2 descriptor",
        ));
    }
    if raw.contains('(') {
        return Err(Error::msg(
            "cosigner must be an xpub (optionally [fingerprint/path]xpub…/0/*)",
        ));
    }
    // Already a descriptor key with origin and/or range.
    if raw.contains('[') || raw.contains("/*") {
        let _ = DescriptorPublicKey::from_str(strip_checksum(raw))
            .map_err(|e| Error::Miniscript(e.to_string()))?;
        return Ok(ensure_receive_range(strip_checksum(raw)));
    }
    let (xpub, _) = slip132_to_xpub(raw)?;
    let key = format!("{xpub}/0/*");
    let _ = DescriptorPublicKey::from_str(&key).map_err(|e| Error::Miniscript(e.to_string()))?;
    Ok(key)
}

fn strip_checksum(s: &str) -> &str {
    s.split('#').next().unwrap_or(s).trim()
}

fn ensure_receive_range(s: &str) -> String {
    let s = s.trim().to_string();
    if s.ends_with("/*") {
        return s;
    }
    if s.ends_with("/0/*") {
        return s;
    }
    format!("{s}/0/*")
}

pub fn wsh_sortedmulti(a: &str, b: &str) -> Result<Descriptor<DescriptorPublicKey>, Error> {
    let a = normalize_cosigner_key(a)?;
    let b = normalize_cosigner_key(b)?;
    let s = format!("wsh(sortedmulti(2,{a},{b}))");
    let desc = Descriptor::<DescriptorPublicKey>::from_str(&s)
        .map_err(|e| Error::Miniscript(e.to_string()))?;
    match desc.desc_type() {
        DescriptorType::Wsh | DescriptorType::WshSortedMulti => Ok(desc),
        other => Err(Error::msg(format!("expected wsh sortedmulti, got {other:?}"))),
    }
}

#[derive(Debug, Clone)]
pub struct Escrow {
    pub descriptor: String,
    pub address: Address,
    pub script_pubkey: ScriptBuf,
    pub witness_script: ScriptBuf,
    pub pubkeys: Vec<PublicKey>,
}

pub fn escrow_at(
    mandante_xpub: &str,
    contratista_xpub: &str,
    index: u32,
    network: Network,
) -> Result<Escrow, Error> {
    let desc = wsh_sortedmulti(mandante_xpub, contratista_xpub)?;
    let secp = Secp256k1::verification_only();
    let derived = desc
        .derived_descriptor(&secp, index)
        .map_err(|e| Error::Miniscript(e.to_string()))?;
    let address = derived
        .address(to_btc_network(network))
        .map_err(|e| Error::Address(e.to_string()))?;
    let witness_script = derived
        .explicit_script()
        .map_err(|e| Error::Miniscript(e.to_string()))?;
    let pubkeys = pubkeys_from_multisig(&witness_script)?;
    Ok(Escrow {
        descriptor: desc.to_string(),
        script_pubkey: address.script_pubkey(),
        address,
        witness_script,
        pubkeys,
    })
}

fn pubkeys_from_multisig(script: &ScriptBuf) -> Result<Vec<PublicKey>, Error> {
    // wsh sortedmulti witness script: OP_2 <pk> <pk> OP_2 OP_CHECKMULTISIG
    let mut pks = Vec::new();
    for instr in script.instructions() {
        let instr = instr.map_err(|e| Error::msg(e.to_string()))?;
        if let bitcoin::blockdata::script::Instruction::PushBytes(b) = instr {
            if b.len() == 33 {
                pks.push(PublicKey::from_slice(b.as_bytes()).map_err(|e| Error::msg(e.to_string()))?);
            }
        }
    }
    if pks.len() != 2 {
        return Err(Error::msg(format!(
            "expected 2 pubkeys in witness script, got {}",
            pks.len()
        )));
    }
    Ok(pks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
    use bitcoin::secp256k1::Secp256k1;
    use bitcoin::Network as BtcNetwork;

    fn account_xpub(seed: u8) -> String {
        let secp = Secp256k1::new();
        let xprv = Xpriv::new_master(BtcNetwork::Testnet, &[seed; 32]).unwrap();
        let path: DerivationPath = "m/48'/1'/0'/2'".parse().unwrap();
        let acc = xprv.derive_priv(&secp, &path).unwrap();
        Xpub::from_priv(&secp, &acc).to_string()
    }

    #[test]
    fn address_independent_of_key_order() {
        let a = account_xpub(1);
        let b = account_xpub(2);
        let e1 = escrow_at(&a, &b, 0, Network::Signet).unwrap();
        let e2 = escrow_at(&b, &a, 0, Network::Signet).unwrap();
        assert_eq!(e1.address, e2.address);
        assert_eq!(e1.witness_script, e2.witness_script);
        assert_eq!(e1.pubkeys.len(), 2);
        assert!(e1.address.to_string().starts_with("tb1q"));
    }
}
