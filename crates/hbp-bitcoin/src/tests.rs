use bitcoin::absolute::LockTime;
use bitcoin::hashes::Hash;
use bitcoin::secp256k1::{rand::rngs::OsRng, PublicKey, Secp256k1, SecretKey};
use bitcoin::{Amount, Network, OutPoint, ScriptBuf, Transaction, TxIn, TxOut, Txid};
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
