//! DKG PedPoP 2-de-2. Cada parte se queda con su share.
//!
//! Tres mensajes: compromiso, share cifrado, y la view que arma la dirección.
//! La view la genera el mandante y se la pasa al contratista. El share de
//! gasto no viaja.

use std::collections::HashMap;
use dkg_pedpop::{EncryptionKeyMessage, EncryptedMessage, KeyGenMachine, SecretShare};
use frost::curve::{Ciphersuite, Ed25519};
use frost::{Participant, ThresholdKeys, ThresholdParams};
use monero_wallet::address::Network;
use monero_wallet::ed25519::{CompressedPoint, Point};
use monero_wallet::primitives::keccak256;
use rand_core::OsRng;

use crate::error::Error;
use crate::hot::{cursor_de, direccion_de_spend_y_view, escribir, view_compartida};

const UMBRAL: u16 = 2;
const PARTES: u16 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RolCaja {
    Mandante,
    Contratista,
}

pub struct ShareLocal {
    keys: ThresholdKeys<Ed25519>,
    pub spend_pub_hex: String,
    pub view_sec_hex: String,
    pub direccion: String,
}

impl ShareLocal {
    pub fn bytes(&self) -> Vec<u8> {
        self.keys.serialize().to_vec()
    }

    pub fn desde_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let keys = ThresholdKeys::<Ed25519>::read(&mut &bytes[..]).map_err(|_| Error::Caja)?;
        let spend_pub_hex = spend_hex(&keys)?;
        Ok(Self {
            keys,
            spend_pub_hex,
            view_sec_hex: String::new(),
            direccion: String::new(),
        })
    }
}

pub struct MaquinaCompromiso {
    machine: dkg_pedpop::SecretShareMachine<Ed25519>,
    params: ThresholdParams,
}

pub struct MaquinaShares {
    machine: dkg_pedpop::KeyMachine<Ed25519>,
    params: ThresholdParams,
}

pub fn contexto_obra(obra_id: &str) -> [u8; 32] {
    keccak256(format!("konstruado-dkg-v1:{obra_id}").as_bytes())
}

fn participante(rol: RolCaja) -> Participant {
    let i = match rol {
        RolCaja::Mandante => 1,
        RolCaja::Contratista => 2,
    };
    Participant::new(i).expect("índice 1 o 2")
}

fn params(rol: RolCaja) -> Result<ThresholdParams, Error> {
    ThresholdParams::new(UMBRAL, PARTES, participante(rol)).map_err(|_| Error::Protocolo)
}

fn spend_hex(keys: &ThresholdKeys<Ed25519>) -> Result<String, Error> {
    let punto = Point::from(keys.group_key().0);
    Ok(hex::encode(punto.compress().to_bytes()))
}

fn punto_de_hex(hex_s: &str) -> Result<Point, Error> {
    let raw = hex::decode(hex_s.trim()).map_err(|_| Error::Clave)?;
    if raw.len() != 32 {
        return Err(Error::Clave);
    }
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&raw);
    CompressedPoint::from(bytes).decompress().ok_or(Error::Clave)
}

/// Ronda 1. Devuelve la máquina local y el compromiso para el otro.
pub fn ronda_compromiso(rol: RolCaja, obra_id: &str) -> Result<(MaquinaCompromiso, Vec<u8>), Error> {
    let params = params(rol)?;
    let machine = KeyGenMachine::<Ed25519>::new(params, contexto_obra(obra_id));
    let (machine, msg) = machine.generate_coefficients(&mut OsRng);
    let bytes = escribir(|w| msg.write(w)).map_err(|_| Error::Protocolo)?;
    Ok((
        MaquinaCompromiso { machine, params },
        bytes,
    ))
}

/// Ronda 2. El share que sale está cifrado para el otro.
pub fn ronda_shares(
    maquina: MaquinaCompromiso,
    compromiso_otro: &[u8],
    rol_otro: RolCaja,
) -> Result<(MaquinaShares, Vec<u8>), Error> {
    let msg = EncryptionKeyMessage::<Ed25519, dkg_pedpop::Commitments<Ed25519>>::read(
        &mut cursor_de(compromiso_otro),
        maquina.params,
    )
    .map_err(|_| Error::Protocolo)?;
    let mut mapa = HashMap::new();
    mapa.insert(participante(rol_otro), msg);
    let (machine, mut shares) = maquina
        .machine
        .generate_secret_shares(&mut OsRng, mapa)
        .map_err(|_| Error::Protocolo)?;
    let share = shares
        .remove(&participante(rol_otro))
        .ok_or(Error::Protocolo)?;
    let bytes = escribir(|w| share.write(w)).map_err(|_| Error::Protocolo)?;
    Ok((
        MaquinaShares {
            machine,
            params: maquina.params,
        },
        bytes,
    ))
}

/// Cierra el DKG. `view_hex` vacío: el mandante inventa la view.
/// El contratista tiene que recibir esa view (cifrada por la app) y pasarla acá.
pub fn cerrar_dkg(
    maquina: MaquinaShares,
    share_otro: &[u8],
    rol_otro: RolCaja,
    view_hex: &str,
    network: Network,
) -> Result<(ShareLocal, String), Error> {
    let msg = EncryptedMessage::<Ed25519, SecretShare<<Ed25519 as Ciphersuite>::F>>::read(
        &mut cursor_de(share_otro),
        maquina.params,
    )
    .map_err(|_| Error::Protocolo)?;
    let mut mapa = HashMap::new();
    mapa.insert(participante(rol_otro), msg);
    let keys = maquina
        .machine
        .calculate_share(&mut OsRng, mapa)
        .map_err(|_| Error::Protocolo)?
        .complete();
    let spend_pub_hex = spend_hex(&keys)?;
    let view = if view_hex.is_empty() {
        view_compartida(&mut OsRng)
    } else {
        view_hex.to_string()
    };
    let punto = punto_de_hex(&spend_pub_hex)?;
    let direccion = direccion_de_spend_y_view(punto, &view, network)?;
    Ok((
        ShareLocal {
            keys,
            spend_pub_hex,
            view_sec_hex: view.clone(),
            direccion,
        },
        view,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hot::red_laboratorio;

    #[test]
    fn cada_uno_guarda_su_share_y_la_direccion_coincide() {
        let id = "obra-prueba";
        let (m_a, comp_a) = ronda_compromiso(RolCaja::Mandante, id).unwrap();
        let (m_b, comp_b) = ronda_compromiso(RolCaja::Contratista, id).unwrap();
        let (s_a, share_a) = ronda_shares(m_a, &comp_b, RolCaja::Contratista).unwrap();
        let (s_b, share_b) = ronda_shares(m_b, &comp_a, RolCaja::Mandante).unwrap();
        let (caja_a, view) = cerrar_dkg(s_a, &share_b, RolCaja::Contratista, "", red_laboratorio()).unwrap();
        let (caja_b, _) = cerrar_dkg(
            s_b,
            &share_a,
            RolCaja::Mandante,
            &view,
            red_laboratorio(),
        )
        .unwrap();
        assert_eq!(caja_a.spend_pub_hex, caja_b.spend_pub_hex);
        assert_eq!(caja_a.direccion, caja_b.direccion);
        assert_ne!(caja_a.bytes(), caja_b.bytes());
        let recargada = ShareLocal::desde_bytes(&caja_a.bytes()).unwrap();
        assert_eq!(recargada.spend_pub_hex, caja_a.spend_pub_hex);
    }
}
