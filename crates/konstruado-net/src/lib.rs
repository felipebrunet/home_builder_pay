//! Swarm over Tor (SOCKS) and local TCP. DHT is a small gossip store
//! keyed by the hardcoded network code so two copies of the app meet
//! without exchanging addresses first.

mod dht;
mod proto;
mod tor;

pub use dht::Nodo;
pub use proto::{Msg, PeerAddr};
pub use tor::{EstadoTor, Tor};

/// Hardcoded rendezvous. Every build joins this swarm.
pub const RED: &str = "konstruado-red-1";

/// Local TCP port used as first-hop bootstrap on the same machine.
pub const PUERTO_LOCAL: u16 = 17432;

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
