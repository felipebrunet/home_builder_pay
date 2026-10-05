//! DKG PedPoP 2-de-2, partido en mensajes.
//!
//! Mandante es el índice FROST 1 y genera la view compartida. Contratista es el 2.
//! La view no sale del DKG: viaja en [`ViewAnnounce`], una sola vez, por el canal de la obra.

use std::collections::HashMap;
use std::io::Cursor;

use dkg_pedpop::KeyGenMachine;
use frost::{
    curve::Ed25519,
    Participant, ThresholdKeys, ThresholdParams,
};
use monero_wallet::{
    ed25519::{Point, Scalar},
    ViewPair,
};
use rand_core::{CryptoRng, RngCore};
use zeroize::{Zeroize, Zeroizing};

use crate::backup::ShareBackup;
use crate::network::{self, Net};
use crate::{Error, Result};

const THRESHOLD: u16 = 2;
const PARTICIPANTS: u16 = 2;

/// Lado de la obra. El índice FROST queda fijo para siempre en el share.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Party {
    Mandante,
    Contratista,
}

impl Party {
    pub fn label(self) -> &'static str {
        match self {
            Self::Mandante => "mandante",
            Self::Contratista => "contratista",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "mandante" => Ok(Self::Mandante),
            "contratista" => Ok(Self::Contratista),
            other => Err(Error::Dkg(format!("rol {other:?}"))),
        }
    }

    fn index(self) -> u16 {
        match self {
            Self::Mandante => 1,
            Self::Contratista => 2,
        }
    }

    fn other(self) -> Self {
        match self {
            Self::Mandante => Self::Contratista,
            Self::Contratista => Self::Mandante,
        }
    }
}

fn participant(party: Party) -> Participant {
    Participant::new(party.index()).expect("índice 1 o 2")
}

/// View privada que el mandante envía al contratista, y la dirección que ambos deben ver.
pub struct ViewAnnounce {
    pub view_private: [u8; 32],
    pub address: String,
}

impl Drop for ViewAnnounce {
    fn drop(&mut self) {
        self.view_private.zeroize();
        self.address.zeroize();
    }
}

impl ViewAnnounce {
    pub fn encode(&self) -> Vec<u8> {
        let addr = self.address.as_bytes();
        let mut out = Vec::with_capacity(4 + 32 + addr.len());
        out.extend((addr.len() as u32).to_le_bytes());
        out.extend(self.view_private);
        out.extend(addr);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 36 {
            return Err(Error::Dkg("view announce corto".into()));
        }
        let len = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
        let view_private: [u8; 32] = bytes[4..36]
            .try_into()
            .map_err(|_| Error::Dkg("view announce".into()))?;
        let addr_bytes = bytes.get(36..36 + len).ok_or_else(|| Error::Dkg("view announce corto".into()))?;
        let address = String::from_utf8(addr_bytes.to_vec()).map_err(|e| Error::Dkg(e.to_string()))?;
        Ok(Self { view_private, address })
    }
}

/// Resultado de ingerir el share del otro.
///
/// El mandante ya tiene la cuenta y la view para enviar. El contratista espera [`DkgParty::ingest_view`].
pub struct ShareOutcome {
    pub account: Option<JointAccount>,
    pub view: Option<ViewAnnounce>,
}

/// Cuenta 2-de-2 de una obra. El share y la view son de esta persona.
pub struct JointAccount {
    role: Party,
    obra_id: String,
    net: Net,
    context: [u8; 32],
    address: String,
    keys: ThresholdKeys<Ed25519>,
    view_private: Zeroizing<Scalar>,
}

impl JointAccount {
    pub fn role(&self) -> Party {
        self.role
    }

    pub fn obra_id(&self) -> &str {
        &self.obra_id
    }

    pub fn net(&self) -> Net {
        self.net
    }

    pub fn address(&self) -> &str {
        &self.address
    }

    /// View key compartida de la caja. Alcanza para ver los movimientos, no para gastar.
    pub fn view_private_bytes(&self) -> [u8; 32] {
        <[u8; 32]>::from(*self.view_private)
    }

    pub fn participant(&self) -> Participant {
        participant(self.role)
    }

    pub fn peer(&self) -> Participant {
        participant(self.role.other())
    }

    pub fn keys(&self) -> ThresholdKeys<Ed25519> {
        self.keys.clone()
    }

    pub fn view_pair(&self) -> Result<ViewPair> {
        let spend_pub = Point::from(self.keys.group_key().0);
        ViewPair::new(spend_pub, self.view_private.clone()).map_err(|e| Error::Dkg(e.to_string()))
    }

    pub fn backup(&self) -> Result<ShareBackup> {
        Ok(ShareBackup {
            role: self.role.label().into(),
            obra_id: self.obra_id.clone(),
            net: self.net,
            address: self.address.clone(),
            context: self.context,
            view_private: Zeroizing::new(<[u8; 32]>::from(*self.view_private)),
            threshold_keys: Zeroizing::new(serialize_keys(&self.keys)?),
        })
    }

    pub fn from_backup(share: &ShareBackup) -> Result<Self> {
        let role = Party::parse(&share.role)?;
        let keys = ThresholdKeys::<Ed25519>::read(&mut Cursor::new(share.threshold_keys.as_slice()))
            .map_err(|e| Error::Dkg(format!("share ilegible: {e}")))?;
        if keys.params().i() != participant(role) {
            return Err(Error::Dkg("el índice FROST no coincide con el rol".into()));
        }
        let view_private = Zeroizing::new(
            Scalar::read(&mut share.view_private.as_slice()).map_err(|e| Error::Dkg(e.to_string()))?,
        );
        let account = Self::assemble(role, share.obra_id.clone(), share.net, share.context, keys, view_private)?;
        if account.address != share.address {
            return Err(Error::Dkg("la dirección del respaldo no coincide con las llaves".into()));
        }
        if account.context != share.context {
            return Err(Error::Dkg("el contexto del respaldo no coincide".into()));
        }
        Ok(account)
    }

    fn assemble(
        role: Party,
        obra_id: String,
        net: Net,
        context: [u8; 32],
        keys: ThresholdKeys<Ed25519>,
        view_private: Zeroizing<Scalar>,
    ) -> Result<Self> {
        let spend_pub = Point::from(keys.group_key().0);
        let view = ViewPair::new(spend_pub, view_private.clone()).map_err(|e| Error::Dkg(e.to_string()))?;
        let address = view.legacy_address(net.oxide()).to_string();
        Ok(Self {
            role,
            obra_id,
            net,
            context,
            address,
            keys,
            view_private,
        })
    }
}

fn serialize_keys(keys: &ThresholdKeys<Ed25519>) -> Result<Vec<u8>> {
    let raw = keys.serialize();
    Ok(raw.as_slice().to_vec())
}

enum Phase {
    Commit(dkg_pedpop::SecretShareMachine<Ed25519>),
    Share(dkg_pedpop::KeyMachine<Ed25519>),
    AwaitView(ThresholdKeys<Ed25519>),
    Done,
}

/// Una parte del DKG. El estado entre mensajes se queda en memoria.
///
/// Si el proceso muere a la mitad, se empieza de nuevo. El respaldo es la cuenta terminada.
pub struct DkgParty {
    role: Party,
    obra_id: String,
    net: Net,
    context: [u8; 32],
    params: ThresholdParams,
    peer: Participant,
    phase: Phase,
}

impl DkgParty {
    /// Arranca y devuelve el compromiso para el otro.
    pub fn start<R: RngCore + CryptoRng>(
        role: Party,
        obra_id: &str,
        net: Net,
        rng: &mut R,
    ) -> Result<(Self, Vec<u8>)> {
        if obra_id.is_empty() {
            return Err(Error::Dkg("la obra no tiene id".into()));
        }
        let context = network::dkg_context(obra_id, net);
        let params = ThresholdParams::new(THRESHOLD, PARTICIPANTS, participant(role))
            .map_err(|e| Error::Dkg(format!("{e:?}")))?;
        let (machine, commit) =
            KeyGenMachine::<Ed25519>::new(params, context).generate_coefficients(rng);
        let party = Self {
            role,
            obra_id: obra_id.to_string(),
            net,
            context,
            params,
            peer: participant(role.other()),
            phase: Phase::Commit(machine),
        };
        Ok((party, commit.serialize()))
    }

    /// Toma el compromiso del otro y devuelve el share cifrado para él.
    pub fn ingest_commit<R: RngCore + CryptoRng>(
        &mut self,
        peer_commit: &[u8],
        rng: &mut R,
    ) -> Result<Vec<u8>> {
        let Phase::Commit(machine) = std::mem::replace(&mut self.phase, Phase::Done) else {
            return Err(Error::Dkg("no toca el compromiso".into()));
        };
        let commit = dkg_pedpop::EncryptionKeyMessage::read(&mut Cursor::new(peer_commit), self.params)
            .map_err(|e| Error::Dkg(format!("compromiso: {e}")))?;
        let mut inbox = HashMap::new();
        inbox.insert(self.peer, commit);
        let (next, mut shares) = machine
            .generate_secret_shares(rng, inbox)
            .map_err(|e| Error::Dkg(format!("{e:?}")))?;
        let share = shares
            .remove(&self.peer)
            .ok_or_else(|| Error::Dkg("no salió el share para el otro".into()))?;
        self.phase = Phase::Share(next);
        Ok(share.serialize())
    }

    /// Toma el share del otro. El mandante cierra y publica la view. El contratista espera.
    pub fn ingest_share<R: RngCore + CryptoRng>(
        &mut self,
        peer_share: &[u8],
        rng: &mut R,
    ) -> Result<ShareOutcome> {
        let Phase::Share(machine) = std::mem::replace(&mut self.phase, Phase::Done) else {
            return Err(Error::Dkg("no toca el share".into()));
        };
        let enc = dkg_pedpop::EncryptedMessage::read(&mut Cursor::new(peer_share), self.params)
            .map_err(|e| Error::Dkg(format!("share: {e}")))?;
        let mut inbox = HashMap::new();
        inbox.insert(self.peer, enc);
        let keys = machine
            .calculate_share(rng, inbox)
            .map_err(|e| Error::Dkg(format!("{e:?}")))?
            .complete();
        if u16::from(keys.params().t()) != THRESHOLD || u16::from(keys.params().n()) != PARTICIPANTS {
            return Err(Error::Dkg("el share no es 2-de-2".into()));
        }
        match self.role {
            Party::Mandante => {
                let view_private = Zeroizing::new(Scalar::random(rng));
                let account = JointAccount::assemble(
                    self.role,
                    self.obra_id.clone(),
                    self.net,
                    self.context,
                    keys,
                    view_private.clone(),
                )?;
                let view = ViewAnnounce {
                    view_private: <[u8; 32]>::from(*view_private),
                    address: account.address.clone(),
                };
                self.phase = Phase::Done;
                Ok(ShareOutcome {
                    account: Some(account),
                    view: Some(view),
                })
            }
            Party::Contratista => {
                self.phase = Phase::AwaitView(keys);
                Ok(ShareOutcome {
                    account: None,
                    view: None,
                })
            }
        }
    }

    /// Contratista: cierra con la view que mandó el mandante.
    pub fn ingest_view(&mut self, view: &ViewAnnounce) -> Result<JointAccount> {
        let Phase::AwaitView(keys) = std::mem::replace(&mut self.phase, Phase::Done) else {
            return Err(Error::Dkg("no toca la view".into()));
        };
        let view_private = Zeroizing::new(
            Scalar::read(&mut view.view_private.as_slice()).map_err(|e| Error::Dkg(e.to_string()))?,
        );
        let account = JointAccount::assemble(
            self.role,
            self.obra_id.clone(),
            self.net,
            self.context,
            keys,
            view_private,
        )?;
        if account.address != view.address {
            return Err(Error::Dkg("la dirección anunciada no sale de esta view".into()));
        }
        self.phase = Phase::Done;
        Ok(account)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;

    #[test]
    fn los_dos_cierran_la_misma_direccion_y_el_respaldo_vuelve() {
        let mut rng = OsRng;
        let (mut mandante, c1) = DkgParty::start(Party::Mandante, "obra-test", Net::Stagenet, &mut rng).unwrap();
        let (mut contratista, c2) =
            DkgParty::start(Party::Contratista, "obra-test", Net::Stagenet, &mut rng).unwrap();
        let s1 = mandante.ingest_commit(&c2, &mut rng).unwrap();
        let s2 = contratista.ingest_commit(&c1, &mut rng).unwrap();
        let done_c = contratista.ingest_share(&s1, &mut rng).unwrap();
        assert!(done_c.account.is_none());
        let done_m = mandante.ingest_share(&s2, &mut rng).unwrap();
        let account_m = done_m.account.expect("mandante");
        let view = done_m.view.expect("view");
        let wire = view.encode();
        let view2 = ViewAnnounce::decode(&wire).unwrap();
        let account_c = contratista.ingest_view(&view2).unwrap();
        assert_eq!(account_m.address(), account_c.address());
        assert_ne!(account_m.address(), "");
        let restored = JointAccount::from_backup(&account_m.backup().unwrap()).unwrap();
        assert_eq!(restored.address(), account_m.address());
        assert_eq!(restored.role(), Party::Mandante);
        let restored_c = JointAccount::from_backup(&account_c.backup().unwrap()).unwrap();
        assert_eq!(restored_c.role(), Party::Contratista);
        assert_eq!(restored_c.address(), account_c.address());
    }
}
