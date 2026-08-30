use std::str::FromStr;

use bitcoin::hashes::Hash;
use bitcoin::key::TapTweak;
use bitcoin::secp256k1::{PublicKey, Secp256k1, XOnlyPublicKey};
use bitcoin::taproot::{LeafVersion, TaprootBuilder, TaprootSpendInfo};
use bitcoin::{Address, Network as BtcNetwork, ScriptBuf};
use hbp_core::{ContractBody, DisputePolicy, Network};
use miniscript::Miniscript;
use musig2::KeyAggContext;

use crate::convert::{from_musig_pk, to_musig_pk};
use crate::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscrowKind {
    /// Payment for one partida. Unwind after T: mandante alone.
    Partida,
    /// Global boleta. Unwind after T_project: contratista alone.
    Bond,
    /// Symmetric dispute stake. After T, NUMS leaf = burn if no coop.
    Mad,
}

#[derive(Clone)]
pub struct Escrow {
    pub kind: EscrowKind,
    pub spend_info: TaprootSpendInfo,
    pub unwind_script: ScriptBuf,
    pub locktime: bitcoin::absolute::LockTime,
}

impl Escrow {
    pub fn address(&self, network: Network) -> Result<Address, Error> {
        Ok(Address::p2tr_tweaked(
            self.spend_info.output_key(),
            to_btc_network(network),
        ))
    }

    pub fn script_pubkey(&self) -> ScriptBuf {
        ScriptBuf::new_p2tr_tweaked(self.spend_info.output_key())
    }

    pub fn merkle_root(&self) -> [u8; 32] {
        self.spend_info
            .merkle_root()
            .expect("script tree is present")
            .to_byte_array()
    }

    pub fn control_block(&self) -> Result<bitcoin::taproot::ControlBlock, Error> {
        self.spend_info
            .control_block(&(self.unwind_script.clone(), LeafVersion::TapScript))
            .ok_or_else(|| Error::Taproot("missing control block".into()))
    }

    pub fn internal_key(&self) -> bitcoin::key::UntweakedPublicKey {
        self.spend_info.internal_key()
    }

    pub fn output_key(&self) -> bitcoin::key::TweakedPublicKey {
        self.spend_info.output_key()
    }
}

pub fn to_btc_network(n: Network) -> BtcNetwork {
    match n {
        Network::Regtest => BtcNetwork::Regtest,
        Network::Signet => BtcNetwork::Signet,
        Network::Testnet => BtcNetwork::Testnet,
        Network::Bitcoin => BtcNetwork::Bitcoin,
    }
}

pub fn musig_internal_key(
    mandante: &PublicKey,
    contratista: &PublicKey,
) -> Result<XOnlyPublicKey, Error> {
    let ctx = key_agg(mandante, contratista)?;
    let agg: musig2::secp256k1::PublicKey = ctx.aggregated_pubkey();
    let btc = from_musig_pk(&agg)?;
    Ok(btc.x_only_public_key().0)
}

pub fn key_agg(mandante: &PublicKey, contratista: &PublicKey) -> Result<KeyAggContext, Error> {
    // Fixed role order so both parties derive the same aggregate without sorting.
    let keys = [to_musig_pk(mandante)?, to_musig_pk(contratista)?];
    KeyAggContext::new(keys).map_err(|e| Error::Musig(e.to_string()))
}

fn unwind_script(unilateral: &XOnlyPublicKey, locktime: u32) -> Result<ScriptBuf, Error> {
    // and_v(v:pk(X), after(T)) → <T> CLTV DROP <X> CHECKSIG
    let s = format!("and_v(v:pk({unilateral}),after({locktime}))");
    let ms = Miniscript::<XOnlyPublicKey, miniscript::Tap>::from_str(&s)
        .map_err(|e| Error::Miniscript(e.to_string()))?;
    Ok(ms.encode())
}

fn build_escrow(
    mandante: &PublicKey,
    contratista: &PublicKey,
    unwind_key: &XOnlyPublicKey,
    locktime: u32,
    kind: EscrowKind,
) -> Result<Escrow, Error> {
    let script = unwind_script(unwind_key, locktime)?;
    finalize_escrow(mandante, contratista, vec![script], 0, locktime, kind)
}

fn and_two_keys_after(
    a: &XOnlyPublicKey,
    b: &XOnlyPublicKey,
    locktime: u32,
) -> Result<ScriptBuf, Error> {
    let s = format!("and_v(v:pk({a}),and_v(v:pk({b}),after({locktime})))");
    let ms = Miniscript::<XOnlyPublicKey, miniscript::Tap>::from_str(&s)
        .map_err(|e| Error::Miniscript(e.to_string()))?;
    Ok(ms.encode())
}

fn finalize_escrow(
    mandante: &PublicKey,
    contratista: &PublicKey,
    leaves: Vec<ScriptBuf>,
    unwind_idx: usize,
    locktime: u32,
    kind: EscrowKind,
) -> Result<Escrow, Error> {
    let secp = Secp256k1::new();
    let internal = musig_internal_key(mandante, contratista)?;
    if leaves.is_empty() {
        return Err(Error::Taproot("no leaves".into()));
    }
    let mut b = TaprootBuilder::new();
    match leaves.len() {
        1 => {
            b = b
                .add_leaf(0, leaves[0].clone())
                .map_err(|e| Error::Taproot(e.to_string()))?;
        }
        2 => {
            b = b
                .add_leaf(1, leaves[0].clone())
                .map_err(|e| Error::Taproot(e.to_string()))?
                .add_leaf(1, leaves[1].clone())
                .map_err(|e| Error::Taproot(e.to_string()))?;
        }
        3 => {
            b = b
                .add_leaf(1, leaves[0].clone())
                .map_err(|e| Error::Taproot(e.to_string()))?
                .add_leaf(2, leaves[1].clone())
                .map_err(|e| Error::Taproot(e.to_string()))?
                .add_leaf(2, leaves[2].clone())
                .map_err(|e| Error::Taproot(e.to_string()))?;
        }
        n => return Err(Error::Taproot(format!("unsupported leaf count {n}"))),
    }
    let spend_info = b
        .finalize(&secp, internal)
        .map_err(|_| Error::Taproot("not finalizable".into()))?;
    Ok(Escrow {
        kind,
        spend_info,
        unwind_script: leaves[unwind_idx].clone(),
        locktime: bitcoin::absolute::LockTime::from_consensus(locktime),
    })
}

/// BIP341 NUMS x-only (unspendable). Used as the MAD burn leaf.
pub fn nums_xonly() -> Result<XOnlyPublicKey, Error> {
    let raw = hex::decode("50929b74c1a04954b78b4b6035e97a5e078a5a0f28ec96d547bfee9ace803ac0")
        .expect("hex");
    XOnlyPublicKey::from_slice(&raw).map_err(|e| Error::Taproot(e.to_string()))
}

fn t2(t: u32, window: u32) -> Result<u32, Error> {
    t.checked_add(window)
        .ok_or_else(|| Error::msg("arbiter T2 overflow"))
}

fn arbiter_escrow(
    mandante: &PublicKey,
    contratista: &PublicKey,
    arbiter: &PublicKey,
    t_dispute: u32,
    window_secs: u32,
    kind: EscrowKind,
) -> Result<Escrow, Error> {
    let a = arbiter.x_only_public_key().0;
    let m = mandante.x_only_public_key().0;
    let c = contratista.x_only_public_key().0;
    let t_end = t2(t_dispute, window_secs)?;
    let am = and_two_keys_after(&a, &m, t_dispute)?;
    let ac = and_two_keys_after(&a, &c, t_dispute)?;
    let last = match kind {
        EscrowKind::Partida => unwind_script(&m, t_end)?,
        EscrowKind::Bond => unwind_script(&c, t_end)?,
        EscrowKind::Mad => return Err(Error::Taproot("mad has no arbiter tree".into())),
    };
    finalize_escrow(mandante, contratista, vec![am, ac, last], 2, t_end, kind)
}

pub fn partida_descriptor(
    mandante: &PublicKey,
    contratista: &PublicKey,
    plazo_unix: u32,
) -> Result<Escrow, Error> {
    let unwind = mandante.x_only_public_key().0;
    build_escrow(
        mandante,
        contratista,
        &unwind,
        plazo_unix,
        EscrowKind::Partida,
    )
}

pub fn bond_descriptor(
    mandante: &PublicKey,
    contratista: &PublicKey,
    t_project: u32,
) -> Result<Escrow, Error> {
    let unwind = contratista.x_only_public_key().0;
    build_escrow(mandante, contratista, &unwind, t_project, EscrowKind::Bond)
}

pub fn partida_escrow_from_body(body: &ContractBody, partida_id: u32) -> Result<Escrow, Error> {
    let (m, c) = keys_from_body(body)?;
    let spec = body.partida(partida_id)?;
    match &body.dispute {
        DisputePolicy::Arbiter {
            pubkey,
            window_secs,
        } => {
            let a = crate::convert::parse_btc_pk(pubkey)?;
            arbiter_escrow(
                &m,
                &c,
                &a,
                spec.plazo_unix,
                *window_secs,
                EscrowKind::Partida,
            )
        }
        _ => partida_descriptor(&m, &c, spec.plazo_unix),
    }
}

pub fn bond_escrow_from_body(body: &ContractBody) -> Result<Escrow, Error> {
    let (m, c) = keys_from_body(body)?;
    match &body.dispute {
        DisputePolicy::Arbiter {
            pubkey,
            window_secs,
        } => {
            let a = crate::convert::parse_btc_pk(pubkey)?;
            arbiter_escrow(&m, &c, &a, body.t_project, *window_secs, EscrowKind::Bond)
        }
        _ => bond_descriptor(&m, &c, body.t_project),
    }
}

/// Combined MAD stake: key path MuSig2 (return/split); after T only NUMS (burn).
pub fn mad_escrow(mandante: &PublicKey, contratista: &PublicKey, t: u32) -> Result<Escrow, Error> {
    let nums = nums_xonly()?;
    let leaf = unwind_script(&nums, t)?;
    finalize_escrow(mandante, contratista, vec![leaf], 0, t, EscrowKind::Mad)
}

pub fn mad_escrow_from_body(body: &ContractBody) -> Result<Escrow, Error> {
    if !matches!(body.dispute, DisputePolicy::Mad { .. }) {
        return Err(Error::msg("contract dispute is not mad"));
    }
    let (m, c) = keys_from_body(body)?;
    mad_escrow(&m, &c, body.t_project)
}

pub fn partida_address(body: &ContractBody, partida_id: u32) -> Result<Address, Error> {
    partida_escrow_from_body(body, partida_id)?.address(body.network)
}

pub fn bond_address(body: &ContractBody) -> Result<Address, Error> {
    bond_escrow_from_body(body)?.address(body.network)
}

pub fn mad_address(body: &ContractBody) -> Result<Address, Error> {
    mad_escrow_from_body(body)?.address(body.network)
}

pub fn keys_from_body(body: &ContractBody) -> Result<(PublicKey, PublicKey), Error> {
    let c_hex = body
        .contratista_pubkey
        .as_deref()
        .ok_or_else(|| Error::msg("contratista pubkey missing"))?;
    Ok((
        crate::convert::parse_btc_pk(&body.mandante_pubkey)?,
        crate::convert::parse_btc_pk(c_hex)?,
    ))
}

/// KeyAgg context tweaked with the escrow's taproot merkle root. Used for key-path MuSig2.
pub fn tweaked_key_agg(
    escrow: &Escrow,
    mandante: &PublicKey,
    contratista: &PublicKey,
) -> Result<KeyAggContext, Error> {
    key_agg(mandante, contratista)?
        .with_taproot_tweak(&escrow.merkle_root())
        .map_err(|e| Error::Musig(e.to_string()))
}

/// Sanity: MuSig2 taproot tweak must match rust-bitcoin's output key.
pub fn assert_output_key_matches(
    escrow: &Escrow,
    mandante: &PublicKey,
    contratista: &PublicKey,
) -> Result<(), Error> {
    let ctx = tweaked_key_agg(escrow, mandante, contratista)?;
    let agg: musig2::secp256k1::PublicKey = ctx.aggregated_pubkey();
    let btc = from_musig_pk(&agg)?;
    let (xonly, _) = btc.x_only_public_key();
    let secp = Secp256k1::new();
    let (tweaked, _) = escrow
        .internal_key()
        .tap_tweak(&secp, escrow.spend_info.merkle_root());
    if xonly != tweaked.to_x_only_public_key() {
        return Err(Error::Taproot(format!(
            "musig2 tweaked key {xonly} != taproot output {}",
            tweaked.to_x_only_public_key()
        )));
    }
    Ok(())
}
