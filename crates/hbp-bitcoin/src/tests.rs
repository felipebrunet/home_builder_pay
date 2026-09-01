use std::str::FromStr;

use bitcoin::absolute::LockTime;
use bitcoin::bip32::{Xpriv, Xpub};
use bitcoin::consensus::encode::serialize_hex;
use bitcoin::hashes::Hash;
use bitcoin::key::CompressedPublicKey;
use bitcoin::secp256k1::{rand::rngs::OsRng, Message, PublicKey, Secp256k1, SecretKey};
use bitcoin::sighash::{EcdsaSighashType, SighashCache};
use bitcoin::{Address, Amount, Network, OutPoint, ScriptBuf, Transaction, TxIn, TxOut, Txid};
use hbp_core::{NonceJournal, Unit};

use crate::musig::{consume_nonce_seed, finish_coop_signature, verify_aggregated};
use crate::spend::{
    apply_key_spend_sig, build_key_spend_tx, build_script_path_tx, build_split_key_spend_tx,
    build_unwind_tx, key_spend_sighash, sign_arbiter_leaf, sign_unwind, verify_key_spend_sig,
    verify_unwind_control_block,
};
use crate::taproot::{assert_output_key_matches, bond_descriptor, partida_descriptor, ArbiterWith};
use crate::validate::{validate_funding_tx, ExpectedFunding};
use crate::{sign_arbiter, sign_body, sign_quote, verify_arbiter, verify_body, verify_quote};

#[test]
fn identity_restore_from_secret() {
    let id = crate::generate_identity(hbp_core::Network::Regtest).unwrap();
    let rest = crate::identity_from_secret(hbp_core::Network::Regtest, &id.secret_key).unwrap();
    assert_eq!(id.public_key, rest.public_key);
    assert_eq!(id.secret_key, rest.secret_key);
    assert!(crate::identity_from_secret(hbp_core::Network::Regtest, "00").is_err());
}

fn pair() -> (SecretKey, PublicKey, SecretKey, PublicKey) {
    let secp = Secp256k1::new();
    let m_sk = SecretKey::new(&mut OsRng);
    let c_sk = SecretKey::new(&mut OsRng);
    let m_pk = PublicKey::from_secret_key(&secp, &m_sk);
    let c_pk = PublicKey::from_secret_key(&secp, &c_sk);
    (m_sk, m_pk, c_sk, c_pk)
}

fn dummy_outpoint() -> OutPoint {
    OutPoint {
        txid: Txid::from_byte_array([9u8; 32]),
        vout: 0,
    }
}

#[test]
fn taproot_output_key_matches_musig_tweak() {
    let (_ms, mp, _cs, cp) = pair();
    let escrow = partida_descriptor(&mp, &cp, 1_700_000_000).unwrap();
    assert_output_key_matches(&escrow, &mp, &cp).unwrap();
    verify_unwind_control_block(&escrow).unwrap();
}

#[test]
fn unwind_script_path_signs() {
    let (m_sk, m_pk, _c_sk, c_pk) = pair();
    let escrow = partida_descriptor(&m_pk, &c_pk, 1_700_000_000).unwrap();
    let dest = bitcoin::Address::p2tr_tweaked(escrow.output_key(), Network::Regtest);
    let prev = TxOut {
        value: Amount::from_sat(20_000),
        script_pubkey: escrow.script_pubkey(),
    };
    let tx = build_unwind_tx(
        &escrow,
        dummy_outpoint(),
        prev.value,
        &dest,
        Amount::from_sat(200),
    )
    .unwrap();
    assert_eq!(tx.lock_time, LockTime::from_consensus(1_700_000_000));
    let signed = sign_unwind(&escrow, tx, &prev, &m_sk).unwrap();
    assert_eq!(signed.input[0].witness.len(), 3);

    let sighash_leaf = {
        use bitcoin::sighash::{Prevouts, SighashCache, TapSighashType};
        use bitcoin::taproot::LeafVersion;
        let mut cache = SighashCache::new(&signed);
        let leaf = bitcoin::taproot::TapLeafHash::from_script(
            &escrow.unwind_script,
            LeafVersion::TapScript,
        );
        cache
            .taproot_script_spend_signature_hash(
                0,
                &Prevouts::All(&[&prev]),
                leaf,
                TapSighashType::Default,
            )
            .unwrap()
    };
    let secp = bitcoin::secp256k1::Secp256k1::verification_only();
    let sig =
        bitcoin::secp256k1::schnorr::Signature::from_slice(signed.input[0].witness.nth(0).unwrap())
            .unwrap();
    secp.verify_schnorr(
        &sig,
        &bitcoin::secp256k1::Message::from_digest(sighash_leaf.to_byte_array()),
        &m_pk.x_only_public_key().0,
    )
    .expect("mandante schnorr must verify against unwind leaf");
}

#[test]
fn cooperative_musig_key_path() {
    let (m_sk, m_pk, c_sk, c_pk) = pair();
    let escrow = partida_descriptor(&m_pk, &c_pk, 1_700_000_000).unwrap();
    assert_output_key_matches(&escrow, &m_pk, &c_pk).unwrap();

    let dest = bitcoin::Address::p2tr_tweaked(escrow.output_key(), Network::Regtest);
    let prev = TxOut {
        value: Amount::from_sat(20_000),
        script_pubkey: escrow.script_pubkey(),
    };
    let tx =
        build_key_spend_tx(dummy_outpoint(), prev.value, &dest, Amount::from_sat(200)).unwrap();
    let sighash = key_spend_sighash(&tx, &prev).unwrap();

    let mut journal = NonceJournal::default();
    let s0 = [1u8; 32];
    let s1 = [2u8; 32];
    consume_nonce_seed(&mut journal, s0).unwrap();
    consume_nonce_seed(&mut journal, s1).unwrap();
    assert!(consume_nonce_seed(&mut journal, s0).is_err());

    let sig = finish_coop_signature(
        &m_pk,
        &c_pk,
        &escrow,
        Some(&m_sk),
        Some(&c_sk),
        s0,
        s1,
        &sighash,
    )
    .unwrap();
    verify_aggregated(&m_pk, &c_pk, &escrow, &sig, &sighash).unwrap();
    verify_key_spend_sig(escrow.output_key().to_x_only_public_key(), &sighash, &sig).unwrap();
    let signed = apply_key_spend_sig(tx, &sig);
    assert_eq!(signed.input[0].witness.len(), 1);
}

#[test]
fn cooperative_split_80_20() {
    let (m_sk, m_pk, c_sk, c_pk) = pair();
    let escrow = partida_descriptor(&m_pk, &c_pk, 1_700_000_000).unwrap();
    let pay_dest = bitcoin::Address::p2tr_tweaked(escrow.output_key(), Network::Regtest);
    let refund_dest = bitcoin::Address::p2tr_tweaked(escrow.output_key(), Network::Regtest);
    let prev = TxOut {
        value: Amount::from_sat(30_000_000),
        script_pubkey: escrow.script_pubkey(),
    };
    let tx = build_split_key_spend_tx(
        dummy_outpoint(),
        prev.value,
        &pay_dest,
        Amount::from_sat(24_000_000),
        &refund_dest,
        Amount::from_sat(200),
    )
    .unwrap();
    assert_eq!(tx.output[0].value.to_sat(), 24_000_000);
    assert_eq!(tx.output[1].value.to_sat(), 5_999_800);
    let sighash = key_spend_sighash(&tx, &prev).unwrap();
    let mut journal = NonceJournal::default();
    let s0 = [3u8; 32];
    let s1 = [4u8; 32];
    consume_nonce_seed(&mut journal, s0).unwrap();
    consume_nonce_seed(&mut journal, s1).unwrap();
    let sig = finish_coop_signature(
        &m_pk,
        &c_pk,
        &escrow,
        Some(&m_sk),
        Some(&c_sk),
        s0,
        s1,
        &sighash,
    )
    .unwrap();
    verify_key_spend_sig(escrow.output_key().to_x_only_public_key(), &sighash, &sig).unwrap();
}

#[test]
fn funding_rejects_wrong_partida_amount() {
    let (_ms, mp, _cs, cp) = pair();
    let partida = partida_descriptor(&mp, &cp, 1_700_000_000).unwrap();
    let bond = bond_descriptor(&mp, &cp, 1_800_000_000).unwrap();
    let tx = Transaction {
        version: bitcoin::transaction::Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: dummy_outpoint(),
            script_sig: ScriptBuf::new(),
            sequence: bitcoin::Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: bitcoin::Witness::new(),
        }],
        output: vec![
            TxOut {
                value: Amount::from_sat(50_000),
                script_pubkey: bond.script_pubkey(),
            },
            TxOut {
                value: Amount::from_sat(1), // malicious
                script_pubkey: partida.script_pubkey(),
            },
        ],
    };
    let expected = ExpectedFunding {
        bond_script: bond.script_pubkey(),
        bond_sats: 50_000,
        partida_script: partida.script_pubkey(),
        partida_sats: 20_000,
        change: vec![],
        allow_other_outputs: false,
    };
    let err = validate_funding_tx(&tx, &expected).unwrap_err();
    assert!(err.to_string().contains("partida amount"));
}

#[test]
fn contract_signatures_roundtrip() {
    let (m_sk, m_pk, c_sk, c_pk) = pair();
    let body = hbp_core::ContractBody {
        network: hbp_core::Network::Regtest,
        unit: Unit::Usd,
        bond_bps: 1000,
        t_project: 1_800_000_000,
        partidas: vec![hbp_core::PartidaSpec {
            id: 1,
            description: "Radier".into(),
            amount_minor: 150_000,
            plazo_unix: 1_700_000_000,
        }],
        mandante_pubkey: hex::encode(m_pk.serialize()),
        contratista_pubkey: Some(hex::encode(c_pk.serialize())),
        dispute: hbp_core::DisputePolicy::Unwind,
    };
    let ms = sign_body(&m_sk, &body).unwrap();
    let cs = sign_body(&c_sk, &body).unwrap();
    verify_body(&body.mandante_pubkey, &ms, &body).unwrap();
    verify_body(body.contratista_pubkey.as_ref().unwrap(), &cs, &body).unwrap();
}

#[test]
fn arbiter_tree_changes_address() {
    let (_ms, mp, _cs, cp) = pair();
    let secp = Secp256k1::new();
    let ask = SecretKey::new(&mut OsRng);
    let ap = PublicKey::from_secret_key(&secp, &ask);
    let plain = partida_descriptor(&mp, &cp, 1_700_000_000).unwrap();
    let ahex = hex::encode(ap.serialize());
    let with_a = crate::taproot::partida_escrow_from_body(
        &hbp_core::ContractBody {
            network: hbp_core::Network::Regtest,
            unit: Unit::Usd,
            bond_bps: 1000,
            t_project: 1_800_000_000,
            partidas: vec![hbp_core::PartidaSpec {
                id: 1,
                description: "x".into(),
                amount_minor: 100,
                plazo_unix: 1_700_000_000,
            }],
            mandante_pubkey: hex::encode(mp.serialize()),
            contratista_pubkey: Some(hex::encode(cp.serialize())),
            dispute: hbp_core::DisputePolicy::Arbiter { window_secs: 15 },
        },
        1,
        Some(ahex.as_str()),
    )
    .unwrap();
    assert_ne!(
        plain.output_key().to_x_only_public_key(),
        with_a.output_key().to_x_only_public_key()
    );
    assert_output_key_matches(&with_a, &mp, &cp).unwrap();
    let unnamed = crate::taproot::partida_escrow_from_body(
        &hbp_core::ContractBody {
            network: hbp_core::Network::Regtest,
            unit: Unit::Usd,
            bond_bps: 1000,
            t_project: 1_800_000_000,
            partidas: vec![hbp_core::PartidaSpec {
                id: 1,
                description: "x".into(),
                amount_minor: 100,
                plazo_unix: 1_700_000_000,
            }],
            mandante_pubkey: hex::encode(mp.serialize()),
            contratista_pubkey: Some(hex::encode(cp.serialize())),
            dispute: hbp_core::DisputePolicy::Arbiter { window_secs: 15 },
        },
        1,
        None,
    );
    assert!(unnamed.is_err());
}

#[test]
fn arbiter_leaf_two_key_witness() {
    let (m_sk, mp, _cs, cp) = pair();
    let secp = Secp256k1::new();
    let ask = SecretKey::new(&mut OsRng);
    let ap = PublicKey::from_secret_key(&secp, &ask);
    let ahex = hex::encode(ap.serialize());
    let body = hbp_core::ContractBody {
        network: hbp_core::Network::Regtest,
        unit: Unit::Usd,
        bond_bps: 1000,
        t_project: 1_800_000_000,
        partidas: vec![hbp_core::PartidaSpec {
            id: 1,
            description: "x".into(),
            amount_minor: 100,
            plazo_unix: 1_700_000_000,
        }],
        mandante_pubkey: hex::encode(mp.serialize()),
        contratista_pubkey: Some(hex::encode(cp.serialize())),
        dispute: hbp_core::DisputePolicy::Arbiter { window_secs: 15 },
    };
    let escrow = crate::taproot::partida_escrow_from_body(&body, 1, Some(ahex.as_str())).unwrap();
    let am = escrow.arbiter_leaf(ArbiterWith::Mandante).unwrap();
    let cb = escrow.control_block_for(am).unwrap();
    let secp = Secp256k1::verification_only();
    assert!(cb.verify_taproot_commitment(&secp, escrow.output_key().to_x_only_public_key(), am));
    let dest = bitcoin::Address::p2tr_tweaked(escrow.output_key(), Network::Regtest);
    let prev = TxOut {
        value: Amount::from_sat(20_000),
        script_pubkey: escrow.script_pubkey(),
    };
    let tx = build_script_path_tx(
        escrow.dispute_locktime,
        dummy_outpoint(),
        Amount::from_sat(20_000),
        &dest,
        Amount::from_sat(200),
    )
    .unwrap();
    let signed = sign_arbiter_leaf(&escrow, ArbiterWith::Mandante, tx, &prev, &ask, &m_sk).unwrap();
    assert_eq!(signed.input[0].witness.len(), 4);
    assert_eq!(signed.lock_time, escrow.dispute_locktime);
    assert_ne!(signed.lock_time, escrow.locktime);
}

#[test]
fn arbiter_nomination_signatures_roundtrip() {
    let (m_sk, m_pk, c_sk, c_pk) = pair();
    let cid = "ab".repeat(32);
    let a = hex::encode({
        let secp = Secp256k1::new();
        PublicKey::from_secret_key(&secp, &SecretKey::new(&mut OsRng)).serialize()
    });
    let ms = sign_arbiter(&m_sk, &cid, &a).unwrap();
    let cs = sign_arbiter(&c_sk, &cid, &a).unwrap();
    verify_arbiter(&hex::encode(m_pk.serialize()), &ms, &cid, &a).unwrap();
    verify_arbiter(&hex::encode(c_pk.serialize()), &cs, &cid, &a).unwrap();
    assert!(verify_arbiter(&hex::encode(m_pk.serialize()), &cs, &cid, &a).is_err());
}

#[test]
fn quote_signatures_roundtrip() {
    let (m_sk, m_pk, c_sk, c_pk) = pair();
    let mut quote = hbp_core::Quote {
        contract_id: "ab".repeat(32),
        bond_sats: 20_000,
        partidas: vec![hbp_core::PartidaQuote {
            id: 1,
            sats: 30_000,
        }],
        fx_note: "test".into(),
        quoted_at_unix: 1_700_000_000,
        mandante_sig: None,
        contratista_sig: None,
        mad_sats: None,
    };
    let ms = sign_quote(&m_sk, &quote).unwrap();
    let cs = sign_quote(&c_sk, &quote).unwrap();
    quote.mandante_sig = Some(ms.clone());
    quote.contratista_sig = Some(cs.clone());
    verify_quote(&hex::encode(m_pk.serialize()), &ms, &quote).unwrap();
    verify_quote(&hex::encode(c_pk.serialize()), &cs, &quote).unwrap();
    quote.bond_sats = 21_000;
    assert!(verify_quote(&hex::encode(m_pk.serialize()), &ms, &quote).is_err());
}

#[test]
fn mad_leaf_is_nums() {
    let (_ms, mp, _cs, cp) = pair();
    let mad = crate::mad_escrow(&mp, &cp, 1_800_000_000).unwrap();
    assert_output_key_matches(&mad, &mp, &cp).unwrap();
    verify_unwind_control_block(&mad).unwrap();
}

#[test]
fn funding_psbt_keeps_escrow_amounts_exact() {
    let (_ms, mp, _cs, cp) = pair();
    let bond = bond_descriptor(&mp, &cp, 1_800_000_000).unwrap();
    let part = partida_descriptor(&mp, &cp, 1_700_000_000).unwrap();
    let other = partida_descriptor(&mp, &cp, 1_710_000_000).unwrap();
    let chg = bitcoin::Address::p2tr_tweaked(other.output_key(), Network::Regtest);
    let req = crate::FundingRequest {
        bond: Some((bond.script_pubkey(), 20_000)),
        partida: (part.script_pubkey(), 30_000),
        mad: None,
        fee: 200,
        mandante: crate::FundingCoin {
            outpoint: dummy_outpoint(),
            sats: 1_000_000,
            script_pubkey: chg.script_pubkey(),
        },
        mandante_change: chg.clone(),
        contratista: Some(crate::FundingCoin {
            outpoint: OutPoint {
                txid: Txid::from_byte_array([8u8; 32]),
                vout: 1,
            },
            sats: 500_000,
            script_pubkey: chg.script_pubkey(),
        }),
        contratista_change: Some(chg.clone()),
    };
    let (tx, _) = crate::funding_tx(&req).unwrap();
    validate_funding_tx(
        &tx,
        &ExpectedFunding {
            bond_script: bond.script_pubkey(),
            bond_sats: 20_000,
            partida_script: part.script_pubkey(),
            partida_sats: 30_000,
            change: vec![chg.script_pubkey()],
            allow_other_outputs: false,
        },
    )
    .unwrap();
    let p1 = tx
        .output
        .iter()
        .find(|o| o.script_pubkey == part.script_pubkey())
        .unwrap();
    assert_eq!(p1.value.to_sat(), 30_000);
    let psbt = crate::build_funding_psbt(&req).unwrap();
    assert_eq!(psbt.unsigned_tx.output.len(), tx.output.len());
    assert!(psbt.inputs.iter().all(|i| i.witness_utxo.is_some()));
}

#[test]
fn funding_rejects_dust_change() {
    let (_ms, mp, _cs, cp) = pair();
    let bond = bond_descriptor(&mp, &cp, 1_800_000_000).unwrap();
    let part = partida_descriptor(&mp, &cp, 1_700_000_000).unwrap();
    let other = partida_descriptor(&mp, &cp, 1_710_000_000).unwrap();
    let chg = bitcoin::Address::p2tr_tweaked(other.output_key(), Network::Regtest);
    // mandante needs 30_000 + 100 fee/2; +1 sat leftover is dust
    let req = crate::FundingRequest {
        bond: Some((bond.script_pubkey(), 20_000)),
        partida: (part.script_pubkey(), 30_000),
        mad: None,
        fee: 200,
        mandante: crate::FundingCoin {
            outpoint: dummy_outpoint(),
            sats: 30_101,
            script_pubkey: chg.script_pubkey(),
        },
        mandante_change: chg.clone(),
        contratista: Some(crate::FundingCoin {
            outpoint: OutPoint {
                txid: Txid::from_byte_array([8u8; 32]),
                vout: 1,
            },
            sats: 20_100,
            script_pubkey: chg.script_pubkey(),
        }),
        contratista_change: Some(chg),
    };
    let err = crate::funding_tx(&req).unwrap_err().to_string();
    assert!(err.contains("dust"), "{err}");
}

#[test]
fn funding_rejects_short_input() {
    let (_ms, mp, _cs, cp) = pair();
    let bond = bond_descriptor(&mp, &cp, 1_800_000_000).unwrap();
    let part = partida_descriptor(&mp, &cp, 1_700_000_000).unwrap();
    let other = partida_descriptor(&mp, &cp, 1_710_000_000).unwrap();
    let chg = bitcoin::Address::p2tr_tweaked(other.output_key(), Network::Regtest);
    let req = crate::FundingRequest {
        bond: Some((bond.script_pubkey(), 20_000)),
        partida: (part.script_pubkey(), 30_000),
        mad: None,
        fee: 200,
        mandante: crate::FundingCoin {
            outpoint: dummy_outpoint(),
            sats: 1_000,
            script_pubkey: chg.script_pubkey(),
        },
        mandante_change: chg.clone(),
        contratista: Some(crate::FundingCoin {
            outpoint: OutPoint {
                txid: Txid::from_byte_array([8u8; 32]),
                vout: 1,
            },
            sats: 500_000,
            script_pubkey: chg.script_pubkey(),
        }),
        contratista_change: Some(chg),
    };
    let err = crate::funding_tx(&req).unwrap_err().to_string();
    assert!(err.contains("mandante input"), "{err}");
}

#[test]
fn funding_zero_fee_rejected() {
    let (_ms, mp, _cs, cp) = pair();
    let part = partida_descriptor(&mp, &cp, 1_700_000_000).unwrap();
    let other = partida_descriptor(&mp, &cp, 1_710_000_000).unwrap();
    let chg = bitcoin::Address::p2tr_tweaked(other.output_key(), Network::Regtest);
    let req = crate::FundingRequest {
        bond: None,
        partida: (part.script_pubkey(), 30_000),
        mad: None,
        fee: 0,
        mandante: crate::FundingCoin {
            outpoint: dummy_outpoint(),
            sats: 40_000,
            script_pubkey: chg.script_pubkey(),
        },
        mandante_change: chg,
        contratista: None,
        contratista_change: None,
    };
    let err = crate::funding_tx(&req).unwrap_err().to_string();
    assert!(err.contains("fee"), "{err}");
}

#[test]
fn funding_partida_only_mandante_pays_fee() {
    let (_ms, mp, _cs, cp) = pair();
    let part = partida_descriptor(&mp, &cp, 1_700_000_000).unwrap();
    let other = partida_descriptor(&mp, &cp, 1_710_000_000).unwrap();
    let chg = bitcoin::Address::p2tr_tweaked(other.output_key(), Network::Regtest);
    let req = crate::FundingRequest {
        bond: None,
        partida: (part.script_pubkey(), 30_000),
        mad: None,
        fee: 200,
        mandante: crate::FundingCoin {
            outpoint: dummy_outpoint(),
            sats: 1_000_000,
            script_pubkey: chg.script_pubkey(),
        },
        mandante_change: chg.clone(),
        contratista: None,
        contratista_change: None,
    };
    let (tx, _) = crate::funding_tx(&req).unwrap();
    assert_eq!(tx.input.len(), 1);
    let p1 = tx
        .output
        .iter()
        .find(|o| o.script_pubkey == part.script_pubkey())
        .unwrap();
    assert_eq!(p1.value.to_sat(), 30_000);
    let change = tx
        .output
        .iter()
        .find(|o| o.script_pubkey == chg.script_pubkey())
        .unwrap();
    assert_eq!(change.value.to_sat(), 1_000_000 - 30_000 - 200);
}

#[test]
fn funding_mad_amounts_exact() {
    let (_ms, mp, _cs, cp) = pair();
    let bond = bond_descriptor(&mp, &cp, 1_800_000_000).unwrap();
    let part = partida_descriptor(&mp, &cp, 1_700_000_000).unwrap();
    let mad = crate::mad_escrow(&mp, &cp, 1_800_000_000).unwrap();
    let other = partida_descriptor(&mp, &cp, 1_710_000_000).unwrap();
    let chg = bitcoin::Address::p2tr_tweaked(other.output_key(), Network::Regtest);
    let req = crate::FundingRequest {
        bond: Some((bond.script_pubkey(), 20_000)),
        partida: (part.script_pubkey(), 30_000),
        mad: Some((mad.script_pubkey(), 2_000)),
        fee: 200,
        mandante: crate::FundingCoin {
            outpoint: dummy_outpoint(),
            sats: 1_000_000,
            script_pubkey: chg.script_pubkey(),
        },
        mandante_change: chg.clone(),
        contratista: Some(crate::FundingCoin {
            outpoint: OutPoint {
                txid: Txid::from_byte_array([8u8; 32]),
                vout: 1,
            },
            sats: 500_000,
            script_pubkey: chg.script_pubkey(),
        }),
        contratista_change: Some(chg.clone()),
    };
    let (tx, _) = crate::funding_tx(&req).unwrap();
    let mad_out = tx
        .output
        .iter()
        .find(|o| o.script_pubkey == mad.script_pubkey())
        .unwrap();
    assert_eq!(mad_out.value.to_sat(), 2_000);
    let p1 = tx
        .output
        .iter()
        .find(|o| o.script_pubkey == part.script_pubkey())
        .unwrap();
    assert_eq!(p1.value.to_sat(), 30_000);
    let b = tx
        .output
        .iter()
        .find(|o| o.script_pubkey == bond.script_pubkey())
        .unwrap();
    assert_eq!(b.value.to_sat(), 20_000);
    // M: 30_000 + 1_000 + 100; C: 20_000 + 1_000 + 100
    let m_chg = tx
        .output
        .iter()
        .filter(|o| o.script_pubkey == chg.script_pubkey())
        .map(|o| o.value.to_sat())
        .collect::<Vec<_>>();
    assert!(m_chg.contains(&(1_000_000 - 31_100)), "{m_chg:?}");
    assert!(m_chg.contains(&(500_000 - 21_100)), "{m_chg:?}");
}

#[test]
fn file_musig_matches_in_process() {
    let (m_sk, m_pk, c_sk, c_pk) = pair();
    let escrow = partida_descriptor(&m_pk, &c_pk, 1_700_000_000).unwrap();
    let dest = bitcoin::Address::p2tr_tweaked(escrow.output_key(), Network::Regtest);
    let prev = TxOut {
        value: Amount::from_sat(20_000),
        script_pubkey: escrow.script_pubkey(),
    };
    let tx = build_key_spend_tx(
        dummy_outpoint(),
        Amount::from_sat(20_000),
        &dest,
        Amount::from_sat(200),
    )
    .unwrap();
    let sighash = key_spend_sighash(&tx, &prev).unwrap();
    let mut j = NonceJournal::default();
    let seed_m = crate::new_nonce_seed(&mut j).unwrap();
    let seed_c = crate::new_nonce_seed(&mut j).unwrap();
    let ctx = crate::tweaked_key_agg(&escrow, &m_pk, &c_pk).unwrap();
    let (_, n_m) = crate::start_round(ctx.clone(), &m_sk, 0, seed_m, &sighash).unwrap();
    let (_, n_c) = crate::start_round(ctx, &c_sk, 1, seed_c, &sighash).unwrap();
    let p_c =
        crate::our_partial_signature(&m_pk, &c_pk, &escrow, &c_sk, 1, seed_c, 0, &n_m, &sighash)
            .unwrap();
    let combined = crate::combine_partials(
        &m_pk, &c_pk, &escrow, &m_sk, 0, seed_m, 1, &n_c, p_c, &sighash,
    )
    .unwrap();
    let in_proc = finish_coop_signature(
        &m_pk,
        &c_pk,
        &escrow,
        Some(&m_sk),
        Some(&c_sk),
        seed_m,
        seed_c,
        &sighash,
    )
    .unwrap();
    assert_eq!(combined, in_proc);
    verify_aggregated(&m_pk, &c_pk, &escrow, &combined, &sighash).unwrap();
}

fn account_tpub() -> String {
    let secp = Secp256k1::new();
    let seed = [7u8; 64];
    let master = Xpriv::new_master(Network::Testnet, &seed).unwrap();
    let path = bitcoin::bip32::DerivationPath::from_str("m/84h/1h/0h").unwrap();
    let acct = master.derive_priv(&secp, &path).unwrap();
    Xpub::from_priv(&secp, &acct).to_string()
}

#[test]
fn watch_vpub_roundtrip_matches_tpub() {
    let tpub = account_tpub();
    let decoded = bitcoin::base58::decode_check(&tpub).unwrap();
    let mut v = decoded.clone();
    v[0..4].copy_from_slice(&0x045F_1CF6u32.to_be_bytes());
    let vpub = bitcoin::base58::encode_check(&v);
    let (back, kind) = crate::slip132_to_xpub(&vpub).unwrap();
    assert_eq!(back, tpub);
    assert_eq!(kind, Some(crate::WatchKind::Wpkh));
}

#[test]
fn watch_import_xpub_derives_same_address_twice() {
    let tpub = account_tpub();
    let acc = crate::import_watch(
        &tpub,
        Some(crate::WatchKind::Wpkh),
        hbp_core::Network::Signet,
        20,
    )
    .unwrap();
    assert!(acc.receive_descriptor.contains("/0/*"));
    assert!(acc.change_descriptor.contains("/1/*"));
    let a = crate::address_at(&acc.receive_descriptor, 0, hbp_core::Network::Signet).unwrap();
    let b = crate::address_at(&acc.receive_descriptor, 0, hbp_core::Network::Signet).unwrap();
    assert_eq!(a, b);
    assert_ne!(
        a.to_string(),
        crate::address_at(&acc.change_descriptor, 0, hbp_core::Network::Signet)
            .unwrap()
            .to_string()
    );
    // xpub never belongs on the offered coin.
    let coin = crate::OfferedCoin {
        role: hbp_core::Role::Contratista,
        outpoint: dummy_outpoint().to_string(),
        sats: 10_000,
        address: a.to_string(),
        change: crate::address_at(&acc.change_descriptor, 0, hbp_core::Network::Signet)
            .unwrap()
            .to_string(),
        prev_tx_hex: None,
    };
    let json = serde_json::to_string(&coin).unwrap();
    assert!(!json.contains("tpub"));
    assert!(!json.contains("xpub"));
    assert!(!json.contains("descriptor"));
}

#[test]
fn watch_scan_stops_at_gap_and_picks_change() {
    let tpub = account_tpub();
    let acc = crate::import_watch(
        &tpub,
        Some(crate::WatchKind::Wpkh),
        hbp_core::Network::Signet,
        3,
    )
    .unwrap();
    let funded = crate::address_at(&acc.receive_descriptor, 0, hbp_core::Network::Signet).unwrap();
    let mut lookups = 0u32;
    let scan = crate::scan_watch(&acc, |addr| {
        lookups += 1;
        if addr == &funded {
            Ok(vec![(dummy_outpoint(), 50_000, true)])
        } else {
            Ok(vec![])
        }
    })
    .unwrap();
    assert_eq!(scan.utxos.len(), 1);
    assert_eq!(scan.utxos[0].sats, 50_000);
    assert_eq!(
        scan.change,
        crate::address_at(&acc.change_descriptor, 0, hbp_core::Network::Signet)
            .unwrap()
            .to_string()
    );
    // receive: index 0 used + 3 unused; change: 3 unused
    assert_eq!(lookups, 1 + 3 + 3);
}

#[test]
fn watch_rejects_mainnet_xpub_on_signet() {
    let secp = Secp256k1::new();
    let master = Xpriv::new_master(Network::Bitcoin, &[3u8; 64]).unwrap();
    let xpub = Xpub::from_priv(&secp, &master).to_string();
    let err = crate::import_watch(&xpub, None, hbp_core::Network::Signet, 20)
        .unwrap_err()
        .to_string();
    assert!(err.contains("network"), "{err}");
}

#[test]
fn offered_coin_to_funding_coin() {
    let tpub = account_tpub();
    let acc = crate::import_watch(&tpub, None, hbp_core::Network::Regtest, 5).unwrap();
    let addr = crate::address_at(&acc.receive_descriptor, 1, hbp_core::Network::Regtest).unwrap();
    let chg = crate::address_at(&acc.change_descriptor, 0, hbp_core::Network::Regtest).unwrap();
    let coin = crate::OfferedCoin {
        role: hbp_core::Role::Mandante,
        outpoint: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:2".into(),
        sats: 12_345,
        address: addr.to_string(),
        change: chg.to_string(),
        prev_tx_hex: None,
    };
    let fc = coin.funding_coin(hbp_core::Network::Regtest).unwrap();
    assert_eq!(fc.sats, 12_345);
    assert_eq!(fc.script_pubkey, addr.script_pubkey());
    assert_eq!(fc.outpoint.vout, 2);
}

fn p2wpkh_pair() -> (SecretKey, Address, SecretKey, Address) {
    let secp = Secp256k1::new();
    let s1 = SecretKey::new(&mut OsRng);
    let s2 = SecretKey::new(&mut OsRng);
    let p1 = PublicKey::from_secret_key(&secp, &s1);
    let p2 = PublicKey::from_secret_key(&secp, &s2);
    let a1 = Address::p2wpkh(&CompressedPublicKey(p1), Network::Regtest);
    let a2 = Address::p2wpkh(&CompressedPublicKey(p2), Network::Regtest);
    (s1, a1, s2, a2)
}

fn sign_p2wpkh_input(psbt: &mut bitcoin::psbt::Psbt, index: usize, sk: &SecretKey) {
    let secp = Secp256k1::new();
    let pk = PublicKey::from_secret_key(&secp, sk);
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
    let msg = Message::from_digest(sighash.to_byte_array());
    let sig = secp.sign_ecdsa(&msg, sk);
    psbt.inputs[index].partial_sigs.insert(
        bitcoin::PublicKey::new(pk),
        bitcoin::ecdsa::Signature::sighash_all(sig),
    );
}

#[test]
fn combine_two_blue_style_partial_psbts() {
    let (s1, a1, s2, a2) = p2wpkh_pair();
    let (_ms, mp, _cs, cp) = pair();
    let bond = bond_descriptor(&mp, &cp, 1_800_000_000).unwrap();
    let part = partida_descriptor(&mp, &cp, 1_700_000_000).unwrap();
    let req = crate::FundingRequest {
        bond: Some((bond.script_pubkey(), 20_000)),
        partida: (part.script_pubkey(), 30_000),
        mad: None,
        fee: 200,
        mandante: crate::FundingCoin {
            outpoint: dummy_outpoint(),
            sats: 100_000,
            script_pubkey: a1.script_pubkey(),
        },
        mandante_change: a1.clone(),
        contratista: Some(crate::FundingCoin {
            outpoint: OutPoint {
                txid: Txid::from_byte_array([8u8; 32]),
                vout: 1,
            },
            sats: 80_000,
            script_pubkey: a2.script_pubkey(),
        }),
        contratista_change: Some(a2.clone()),
    };
    let unsigned = crate::build_funding_psbt(&req).unwrap();
    let mut only_m = unsigned.clone();
    let mut only_c = unsigned.clone();
    sign_p2wpkh_input(&mut only_m, 0, &s1);
    sign_p2wpkh_input(&mut only_c, 1, &s2);
    let combined = crate::combine_psbts(&[only_m, only_c]).unwrap();
    let tx = crate::extract_signed_funding_tx(combined).unwrap();
    assert_eq!(tx.input.len(), 2);
    assert!(tx.input.iter().all(|i| i.witness.len() == 2));
    let _ = serialize_hex(&tx);
}
