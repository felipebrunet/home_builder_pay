use bitcoin::secp256k1::{PublicKey, SecretKey};
use hbp_core::{NonceJournal, Role};
use musig2::{
    AggNonce, CompactSignature, FirstRound, KeyAggContext, PartialSignature, PubNonce,
    SecNonceSpices, SecondRound,
};
use serde::{Deserialize, Serialize};

use crate::convert::to_musig_sk;
use crate::taproot::tweaked_key_agg;
use crate::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoopSession {
    pub kind: String,
    pub partida_id: Option<u32>,
    pub signer_index: usize,
    pub pubnonce: String,
    pub sighash: String,
    pub tx_hex: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial_sig: Option<String>,
}

pub fn signer_index(role: Role) -> usize {
    match role {
        Role::Mandante => 0,
        Role::Contratista => 1,
    }
}

pub fn new_nonce_seed(journal: &mut NonceJournal) -> Result<[u8; 32], Error> {
    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).map_err(|e| Error::msg(e.to_string()))?;
    journal.consume_seed(&seed)?;
    Ok(seed)
}

/// Test helper: consume a known seed (so reuse can be asserted).
pub fn consume_nonce_seed(journal: &mut NonceJournal, seed: [u8; 32]) -> Result<(), Error> {
    journal.consume_seed(&seed)?;
    Ok(())
}

pub fn start_round(
    key_agg: KeyAggContext,
    secret: &SecretKey,
    signer_index: usize,
    nonce_seed: [u8; 32],
    message: &[u8; 32],
) -> Result<(FirstRound, PubNonce), Error> {
    let musig_sk = to_musig_sk(secret)?;
    let round = FirstRound::new(
        key_agg,
        nonce_seed,
        signer_index,
        SecNonceSpices::new()
            .with_seckey(musig_sk)
            .with_message(message),
    )
    .map_err(|e| Error::Musig(e.to_string()))?;
    let pubnonce = round.our_public_nonce();
    Ok((round, pubnonce))
}

pub fn finish_coop_signature(
    mandante: &PublicKey,
    contratista: &PublicKey,
    escrow: &crate::taproot::Escrow,
    mandante_secret: Option<&SecretKey>,
    contratista_secret: Option<&SecretKey>,
    mandante_seed: [u8; 32],
    contratista_seed: [u8; 32],
    message: &[u8; 32],
) -> Result<[u8; 64], Error> {
    // In-process helper for tests: both parties sign the same sighash.
    let m_sk = mandante_secret.ok_or_else(|| Error::msg("mandante secret required"))?;
    let c_sk = contratista_secret.ok_or_else(|| Error::msg("contratista secret required"))?;

    let ctx_m = tweaked_key_agg(escrow, mandante, contratista)?;
    let ctx_c = tweaked_key_agg(escrow, mandante, contratista)?;

    let (mut r0, n0) = start_round(ctx_m, m_sk, 0, mandante_seed, message)?;
    let (mut r1, n1) = start_round(ctx_c, c_sk, 1, contratista_seed, message)?;
    r0.receive_nonce(1, n1.clone())
        .map_err(|e| Error::Musig(e.to_string()))?;
    r1.receive_nonce(0, n0.clone())
        .map_err(|e| Error::Musig(e.to_string()))?;

    let mut s0: SecondRound<[u8; 32]> = r0
        .finalize(to_musig_sk(m_sk)?, *message)
        .map_err(|e| Error::Musig(e.to_string()))?;
    let mut s1: SecondRound<[u8; 32]> = r1
        .finalize(to_musig_sk(c_sk)?, *message)
        .map_err(|e| Error::Musig(e.to_string()))?;

    let p0: PartialSignature = s0.our_signature();
    let p1: PartialSignature = s1.our_signature();
    s0.receive_signature(1, p1)
        .map_err(|e| Error::Musig(e.to_string()))?;
    s1.receive_signature(0, p0)
        .map_err(|e| Error::Musig(e.to_string()))?;

    let compact: CompactSignature = s0.finalize().map_err(|e| Error::Musig(e.to_string()))?;
    Ok(compact.serialize())
}

pub fn parse_pubnonce(hex_str: &str) -> Result<PubNonce, Error> {
    hex_str
        .parse()
        .map_err(|e: musig2::errors::DecodeError<PubNonce>| Error::Musig(e.to_string()))
}

pub fn agg_nonce(a: &PubNonce, b: &PubNonce) -> AggNonce {
    [a.clone(), b.clone()].iter().sum()
}

pub fn encode_pubnonce(n: &PubNonce) -> String {
    n.to_string()
}

pub fn verify_aggregated(
    mandante: &PublicKey,
    contratista: &PublicKey,
    escrow: &crate::taproot::Escrow,
    sig: &[u8; 64],
    message: &[u8; 32],
) -> Result<(), Error> {
    let ctx = tweaked_key_agg(escrow, mandante, contratista)?;
    let pk: musig2::secp256k1::PublicKey = ctx.aggregated_pubkey();
    musig2::verify_single(pk, sig, message).map_err(|e| Error::Musig(e.to_string()))
}
