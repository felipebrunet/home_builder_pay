//! Offer / accept / commit — same signatures as the file protocol, reusable
//! from the GUI (files or Tor inbox).

use anyhow::{bail, Context, Result};
use hbp_bitcoin::{sign_body, verify_body, Identity};
use hbp_core::{Offer, Role, SignedContract, PRODUCT_NETWORK};

pub fn require_signet_offer(offer: &Offer) -> Result<()> {
    if offer.body.network != PRODUCT_NETWORK {
        bail!("Esta oferta no es de Signet");
    }
    verify_body(
        &offer.body.mandante_pubkey,
        &offer.mandante_sig,
        &offer.body,
    )?;
    Ok(())
}

/// Contratista countersigns and adds their key (pending until mandante commit).
pub fn contratista_accept(offer: Offer, id: &Identity) -> Result<SignedContract> {
    if id.role != Some(Role::Contratista) {
        bail!("solo el contratista acepta la oferta");
    }
    require_signet_offer(&offer)?;
    let mut body = offer.body;
    body.contratista_pubkey = Some(id.public_key.clone());
    body.validate()?;
    let sig = sign_body(&id.secret()?, &body)?;
    Ok(SignedContract {
        body,
        mandante_sig: offer.mandante_sig,
        contratista_sig: sig,
    })
}

/// Mandante binds the full body (both keys) after the contratista accepted.
pub fn mandante_commit(
    offer: &Offer,
    mut pending: SignedContract,
    id: &Identity,
) -> Result<SignedContract> {
    if id.role != Some(Role::Mandante) {
        bail!("solo el mandante confirma el trato");
    }
    require_signet_offer(offer)?;
    if pending.body.terms() != offer.body.terms() {
        bail!("la aceptación no coincide con la oferta");
    }
    if pending.body.mandante_pubkey != id.public_key {
        bail!("esta llave no es la del mandante de la oferta");
    }
    let cpk = pending
        .body
        .contratista_pubkey
        .as_ref()
        .context("falta la llave del contratista")?;
    verify_body(cpk, &pending.contratista_sig, &pending.body)?;
    pending.mandante_sig = sign_body(&id.secret()?, &pending.body)?;
    verify_body(
        &pending.body.mandante_pubkey,
        &pending.mandante_sig,
        &pending.body,
    )?;
    Ok(pending)
}

pub fn import_signed(signed: SignedContract) -> Result<SignedContract> {
    if signed.body.network != PRODUCT_NETWORK {
        bail!("Este contrato no es de Signet");
    }
    verify_body(
        &signed.body.mandante_pubkey,
        &signed.mandante_sig,
        &signed.body,
    )?;
    let cpk = signed
        .body
        .contratista_pubkey
        .as_ref()
        .context("falta la llave del contratista")?;
    verify_body(cpk, &signed.contratista_sig, &signed.body)?;
    signed.body.validate()?;
    Ok(signed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hbp_core::{ContractBody, DisputePolicy, Network, PartidaSpec, Unit, DEFAULT_BOND_BPS};

    fn body(m: &Identity, c: Option<&Identity>) -> ContractBody {
        ContractBody {
            network: Network::Signet,
            unit: Unit::Usd,
            work_name: "Casa".into(),
            bond_bps: DEFAULT_BOND_BPS,
            t_project: 1_800_000_000,
            partidas: vec![PartidaSpec {
                id: 1,
                description: "Radier".into(),
                amount_minor: 100_000,
                plazo_unix: 1_700_000_000,
            }],
            mandante_pubkey: m.public_key.clone(),
            contratista_pubkey: c.map(|x| x.public_key.clone()),
            dispute: DisputePolicy::fee_burn(1_700_000_000, 1_800_000_000),
        }
    }

    #[test]
    fn offer_accept_commit_roundtrip() {
        let mut m = hbp_bitcoin::generate_identity(Network::Signet).unwrap();
        m.role = Some(Role::Mandante);
        let mut c = hbp_bitcoin::generate_identity(Network::Signet).unwrap();
        c.role = Some(Role::Contratista);
        let draft = body(&m, None);
        let offer = Offer {
            mandante_sig: hbp_bitcoin::sign_body(&m.secret().unwrap(), &draft).unwrap(),
            body: draft,
        };
        let pending = contratista_accept(offer.clone(), &c).unwrap();
        let signed = mandante_commit(&offer, pending, &m).unwrap();
        let back = import_signed(signed.clone()).unwrap();
        assert_eq!(back.body.contratista_pubkey, Some(c.public_key));
        assert_ne!(back.mandante_sig, offer.mandante_sig);
    }
}
