use bitcoin::absolute::LockTime;
use bitcoin::hashes::Hash;
use bitcoin::key::Keypair;
use bitcoin::secp256k1::{Message, Secp256k1, SecretKey};
use bitcoin::sighash::{Prevouts, SighashCache, TapSighashType};
use bitcoin::taproot::LeafVersion;
use bitcoin::{Address, Amount, OutPoint, ScriptBuf, Sequence, Transaction, TxIn, TxOut, Witness};

use crate::taproot::Escrow;
use crate::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnwindRole {
    Mandante,
    Contratista,
}

pub fn build_key_spend_tx(
    outpoint: OutPoint,
    prevout_value: Amount,
    dest: &Address,
    fee: Amount,
) -> Result<Transaction, Error> {
    let send = prevout_value
        .checked_sub(fee)
        .ok_or_else(|| Error::msg("fee exceeds input"))?;
    if send < Amount::from_sat(546) {
        return Err(Error::msg("output would be dust"));
    }
    Ok(Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: outpoint,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: send,
            script_pubkey: dest.script_pubkey(),
        }],
    })
}

pub fn build_unwind_tx(
    escrow: &Escrow,
    outpoint: OutPoint,
    prevout_value: Amount,
    dest: &Address,
    fee: Amount,
) -> Result<Transaction, Error> {
    let send = prevout_value
        .checked_sub(fee)
        .ok_or_else(|| Error::msg("fee exceeds input"))?;
    if send < Amount::from_sat(546) {
        return Err(Error::msg("output would be dust"));
    }
    Ok(Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: escrow.locktime,
        input: vec![TxIn {
            previous_output: outpoint,
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_LOCKTIME_NO_RBF,
            witness: Witness::new(),
        }],
        output: vec![TxOut {
            value: send,
            script_pubkey: dest.script_pubkey(),
        }],
    })
}

pub fn sign_unwind(
    escrow: &Escrow,
    mut tx: Transaction,
    prevout: &TxOut,
    secret: &SecretKey,
) -> Result<Transaction, Error> {
    let secp = Secp256k1::new();
    let leaf_hash =
        bitcoin::taproot::TapLeafHash::from_script(&escrow.unwind_script, LeafVersion::TapScript);
    let mut cache = SighashCache::new(&tx);
    let sighash = cache
        .taproot_script_spend_signature_hash(
            0,
            &Prevouts::All(&[prevout]),
            leaf_hash,
            TapSighashType::Default,
        )
        .map_err(|e| Error::Sighash(e.to_string()))?;
    let msg = Message::from_digest(sighash.to_byte_array());
    let keypair = Keypair::from_secret_key(&secp, secret);
    let sig = secp.sign_schnorr_no_aux_rand(&msg, &keypair);
    let cb = escrow.control_block()?;
    let mut witness = Witness::new();
    witness.push(sig.as_ref());
    witness.push(escrow.unwind_script.as_bytes());
    witness.push(&cb.serialize());
    tx.input[0].witness = witness;
    Ok(tx)
}

pub fn key_spend_sighash(tx: &Transaction, prevout: &TxOut) -> Result<[u8; 32], Error> {
    let mut cache = SighashCache::new(tx);
    let sighash = cache
        .taproot_key_spend_signature_hash(0, &Prevouts::All(&[prevout]), TapSighashType::Default)
        .map_err(|e| Error::Sighash(e.to_string()))?;
    Ok(sighash.to_byte_array())
}

pub fn apply_key_spend_sig(mut tx: Transaction, sig64: &[u8; 64]) -> Transaction {
    let mut witness = Witness::new();
    witness.push(sig64);
    tx.input[0].witness = witness;
    tx
}

pub fn verify_key_spend_sig(
    output_key: bitcoin::secp256k1::XOnlyPublicKey,
    sighash: &[u8; 32],
    sig64: &[u8; 64],
) -> Result<(), Error> {
    let secp = Secp256k1::verification_only();
    let sig = bitcoin::secp256k1::schnorr::Signature::from_slice(sig64)?;
    secp.verify_schnorr(&sig, &Message::from_digest(*sighash), &output_key)?;
    Ok(())
}

pub fn verify_unwind_control_block(escrow: &Escrow) -> Result<(), Error> {
    let secp = Secp256k1::verification_only();
    let cb = escrow.control_block()?;
    if !cb.verify_taproot_commitment(
        &secp,
        escrow.output_key().to_x_only_public_key(),
        &escrow.unwind_script,
    ) {
        return Err(Error::Taproot(
            "control block does not commit to unwind script".into(),
        ));
    }
    Ok(())
}
