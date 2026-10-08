//! Swarm over Tor (SOCKS) and local TCP. DHT is a small gossip store
//! keyed by the hardcoded network code so two copies of the app meet
//! without exchanging addresses first.

mod ctl;
mod dht;
mod proto;
mod rendezvous;
mod tor;

pub use dht::Nodo;
pub use proto::{CajaMsg, Msg, PeerAddr};
pub use rendezvous::{RENDEZVOUS_ONION, VIRT_PORT};
pub use tor::{probar_socks, DiagSocks, EstadoTor, Tor};

/// Hardcoded rendezvous. Every build joins this swarm.
pub const RED: &str = "konstruado-red-1";

/// Local TCP port used as first-hop bootstrap on the same machine.
pub const PUERTO_LOCAL: u16 = 17432;

/// Orbot default SOCKS port (VPN/proxy mode on the device).
pub const ORBOT_SOCKS: u16 = 9050;
/// Orbot default control port (optional; password auth).
pub const ORBOT_CONTROL: u16 = 9051;

pub fn swarm_id() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(RED.as_bytes()).into()
}

pub fn clave_tablero() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(swarm_id());
    h.update(b"tablero");
    h.finalize().into()
}

pub fn clave_obras() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(swarm_id());
    h.update(b"obras");
    h.finalize().into()
}

pub fn clave_presentes() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(swarm_id());
    h.update(b"presentes");
    h.finalize().into()
}

/// Lápidas de ofertas retiradas por su autor. Un par viejo guarda el valor
/// tal cual y lo reenvía; uno nuevo lo une y lo aplica al tablero.
pub fn clave_retiradas() -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(swarm_id());
    h.update(b"retiradas");
    h.finalize().into()
}
