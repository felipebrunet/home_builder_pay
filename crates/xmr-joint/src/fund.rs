//! Fondeo atómico de una partida.
//!
//! Los dos ponen `capital` en una sola transacción hacia la dirección 2-de-2.
//! Sin los dos CLSAG el blob no es válido. El mandante paga el fee desde su cambio.
//! Alice del protocolo es el mandante. Bob es el contratista.

use std::io::Cursor;

use monero_wallet::{
    ed25519::CompressedPoint,
    transaction::Transaction,
    OutputWithDecoys, ViewPair,
};
use rand_core::OsRng;
use zeroize::Zeroizing;

use crate::coop::messages::{Contribution, Proposal, SessionId};
use crate::coop::sign_partial::{bob_partials_message, MaskAssignment};
use crate::coop::{
    alice_pre_sign_checks, assemble::skeleton_message, assemble_signable, assign_pseudo_out_masks,
    build_unsigned_skeleton, fill_clsags_into_tx, key_image_sort_order, sign_local_inputs,
    verify_mask_closure, verify_partial_signatures, CoopSession, OwnedInputIndex, OxidePartialSigner,
    Role,
};
use crate::network::{self, Net};
use crate::wallet::{key_image_bytes, one_time_spend};
use crate::{Error, Result};

/// Arma la propuesta del mandante. `pay_each` es el capital de la partida.
pub fn mandante_proposal(
    net: Net,
    obra_id: &str,
    partida: u32,
    joint_address: &str,
    capital: u64,
    mandante_outputs: &[OutputWithDecoys],
    mandante_spend: &Zeroizing<monero_wallet::ed25519::Scalar>,
    fee_rate: (u64, u64),
    outgoing_view_key: [u8; 32],
) -> Result<Proposal> {
    if capital == 0 {
        return Err(Error::Fund(crate::coop::Error::Protocol(
            "capital cero".into(),
        )));
    }
    let alice = contribution(
        "alice",
        mandante_outputs,
        mandante_spend,
        capital,
        None,
    )?;
    let proposal = Proposal::design_default(
        SessionId::from_bytes(network::fund_session_bytes(obra_id, partida, net)),
        outgoing_view_key,
        joint_address.to_string(),
        alice,
        fee_rate,
        capital,
    );
    proposal
        .validate()
        .map_err(|e| Error::Fund(crate::coop::Error::Protocol(e)))?;
    if proposal.destination_c != joint_address {
        return Err(Error::Fund(crate::coop::Error::Protocol(
            "el destino no es la caja de la obra".into(),
        )));
    }
    Ok(proposal)
}

/// Contratista verifica, firma sus inputs y devuelve el esqueleto.
pub fn contratista_responde(
    net: Net,
    joint_address: &str,
    capital: u64,
    proposal: &Proposal,
    mandante_view: ViewPair,
    contratista_outputs: &[OutputWithDecoys],
    contratista_spend: &Zeroizing<monero_wallet::ed25519::Scalar>,
    contratista_address: &str,
) -> Result<crate::coop::Skeleton> {
    check_dest(proposal, joint_address, capital)?;
    let mut session = CoopSession::new(Role::Bob);
    let bob = contribution(
        "bob",
        contratista_outputs,
        contratista_spend,
        capital,
        Some(contratista_address.to_string()),
    )?;
    let signable = assemble_signable(proposal, &bob, mandante_view, net.oxide())?;
    let unsigned = build_unsigned_skeleton(signable, &proposal.alice, &bob)?;
    let kis: Vec<CompressedPoint> = unsigned.sorted.iter().map(|s| s.key_image).collect();
    let sum_masks = unsigned.signable.sum_output_masks(&kis);
    let mut rng = OsRng;
    let masks = assign_pseudo_out_masks(&mut rng, unsigned.sorted.len(), sum_masks)?;
    verify_mask_closure(&masks)?;
    let spends = spends_for(
        &unsigned.sorted,
        OwnedInputIndex::Bob,
        contratista_spend,
        contratista_outputs,
    )?;
    let partials = sign_local_inputs(
        &mut rng,
        &OxidePartialSigner,
        &unsigned.sorted,
        OwnedInputIndex::Bob,
        &spends,
        &masks,
        unsigned.signature_hash,
    )?;
    let body = bob_partials_message(proposal.session_id, partials, &masks);
    let skeleton = skeleton_message(proposal.session_id, &unsigned, bob, body);
    skeleton
        .validate()
        .map_err(|e| Error::Fund(crate::coop::Error::Protocol(e)))?;
    session.bob_signed(proposal.clone(), skeleton.clone())?;
    Ok(skeleton)
}

/// Mandante verifica los CLSAG del contratista, firma los suyos y deja la transacción lista.
pub fn mandante_cierra(
    net: Net,
    proposal: &Proposal,
    skeleton: &crate::coop::Skeleton,
    mandante_outputs: &[OutputWithDecoys],
    mandante_spend: &Zeroizing<monero_wallet::ed25519::Scalar>,
) -> Result<Transaction> {
    let mut session = CoopSession::new(Role::Alice);
    session.alice_propose(proposal.clone())?;
    session.alice_receive_skeleton(skeleton.clone(), net.oxide())?;
    let sorted = key_image_sort_order(&proposal.alice, &skeleton.bob)?;
    let unsigned_tx = Transaction::read(&mut Cursor::new(skeleton.unsigned_tx.as_slice()))
        .map_err(|e| Error::Fund(crate::coop::Error::Protocol(format!("tx: {e}"))))?;
    verify_partial_signatures(
        &sorted,
        &skeleton.bob_partials.partials,
        OwnedInputIndex::Bob,
        &skeleton.signature_hash,
        &unsigned_tx,
    )?;
    alice_pre_sign_checks(proposal, skeleton, true, net.oxide())?;
    let masks = MaskAssignment::from_wire_bytes(
        &skeleton.bob_partials.all_masks,
        &skeleton.bob_partials.sum_output_masks,
    )?;
    verify_mask_closure(&masks)?;
    let spends = spends_for(&sorted, OwnedInputIndex::Alice, mandante_spend, mandante_outputs)?;
    let mut rng = OsRng;
    let alice_partials = sign_local_inputs(
        &mut rng,
        &OxidePartialSigner,
        &sorted,
        OwnedInputIndex::Alice,
        &spends,
        &masks,
        skeleton.signature_hash,
    )?;
    let mut all = skeleton.bob_partials.partials.clone();
    all.extend(alice_partials);
    let signed = fill_clsags_into_tx(unsigned_tx, &sorted, &all)?;
    Ok(signed)
}

fn check_dest(proposal: &Proposal, joint_address: &str, capital: u64) -> Result<()> {
    proposal
        .validate()
        .map_err(|e| Error::Fund(crate::coop::Error::Protocol(e)))?;
    if proposal.destination_c != joint_address {
        return Err(Error::Fund(crate::coop::Error::Protocol(
            "destino distinto de la caja".into(),
        )));
    }
    if proposal.pay_each().map_err(crate::coop::Error::Protocol)? != capital {
        return Err(Error::Fund(crate::coop::Error::Protocol(
            "el monto no es el capital de la partida".into(),
        )));
    }
    Ok(())
}

fn contribution(
    party: &str,
    outputs: &[OutputWithDecoys],
    spend: &Zeroizing<monero_wallet::ed25519::Scalar>,
    net_to_destination: u64,
    change_address: Option<String>,
) -> Result<Contribution> {
    if outputs.is_empty() {
        return Err(Error::Fund(crate::coop::Error::Protocol(format!(
            "{party} no tiene inputs"
        ))));
    }
    let mut claimed = 0u64;
    let mut blobs = Vec::new();
    let mut images = Vec::new();
    for output in outputs {
        claimed = claimed.saturating_add(output.commitment().amount);
        blobs.push(output.serialize());
        images.push(key_image_bytes(spend, output));
    }
    if claimed < net_to_destination {
        return Err(Error::Fund(crate::coop::Error::Protocol(format!(
            "{party} aporta {claimed} y el capital es {net_to_destination}"
        ))));
    }
    Ok(Contribution {
        party: party.into(),
        outputs_with_decoys: blobs,
        key_images: images,
        claimed_input_sum: claimed,
        net_to_destination,
        change_address,
    })
}

fn spends_for(
    sorted: &[crate::coop::SortedInput],
    owner: OwnedInputIndex,
    spend: &Zeroizing<monero_wallet::ed25519::Scalar>,
    outputs: &[OutputWithDecoys],
) -> Result<Vec<Zeroizing<monero_wallet::ed25519::Scalar>>> {
    let mut out = Vec::new();
    for input in sorted.iter().filter(|s| s.owner == owner) {
        let image = input.key_image.to_bytes();
        let found = outputs.iter().find(|o| key_image_bytes(spend, o) == image);
        let Some(output) = found else {
            return Err(Error::Fund(crate::coop::Error::Protocol(
                "no está el output de un input propio".into(),
            )));
        };
        out.push(one_time_spend(spend, output));
    }
    Ok(out)
}

/// Elige el output más chico que cubre `min`.
pub fn pick_output(mut outputs: Vec<monero_wallet::WalletOutput>, min: u64) -> Result<monero_wallet::WalletOutput> {
    outputs.retain(|o| o.commitment().amount >= min);
    outputs.sort_by_key(|o| o.commitment().amount);
    outputs
        .into_iter()
        .next()
        .ok_or_else(|| Error::Chain(format!("hace falta un output de al menos {min} piconero")))
}

/// View del mandante, para que el contratista arme el cambio. Solo por el canal de la obra.
pub fn view_del_mandante(address: &str, view_private: &[u8]) -> Result<ViewPair> {
    if view_private.len() != 32 {
        return Err(Error::Fund(crate::coop::Error::Protocol(
            "la view del mandante no mide 32".into(),
        )));
    }
    let addr = crate::coop::parse_address(address, crate::network::Net::Stagenet.oxide())
        .map_err(Error::Fund)?;
    let scalar = monero_wallet::ed25519::Scalar::read(&mut &view_private[..])
        .map_err(|e| Error::Fund(crate::coop::Error::Protocol(e.to_string())))?;
    ViewPair::new(addr.spend(), zeroize::Zeroizing::new(scalar))
        .map_err(|e| Error::Fund(crate::coop::Error::Protocol(e.to_string())))
}

/// Arma el `FeeRate` que el gasto vuelve a pedirle a oxide.
pub fn fee_rate_from_parts(per_weight: u64, mask: u64) -> Result<monero_wallet::interface::FeeRate> {
    monero_wallet::interface::FeeRate::new(per_weight, mask)
        .ok_or_else(|| Error::Chain("fee inválido".into()))
}

/// Partes `(per_weight, mask)` que el protocolo guarda en la propuesta.
pub fn fee_parts(rate: &monero_wallet::interface::FeeRate) -> (u64, u64) {
    let raw = rate.serialize();
    let per_weight = u64::from_le_bytes(raw[0..8].try_into().expect("fee"));
    let mask = u64::from_le_bytes(raw[8..16].try_into().expect("fee mask"));
    (per_weight, mask)
}
