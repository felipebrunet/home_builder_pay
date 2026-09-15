use serde::{Deserialize, Serialize};

use konstruado_core::{Oferta, Obra};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PeerAddr {
    Tcp { host: String, port: u16 },
    Onion { host: String, port: u16 },
}

impl std::fmt::Display for PeerAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PeerAddr::Tcp { host, port } => write!(f, "{host}:{port}"),
            PeerAddr::Onion { host, port } => write!(f, "{host}:{port}"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Msg {
    Hola {
        node: String,
        addr: PeerAddr,
        swarm: String,
    },
    Peers {
        list: Vec<(String, PeerAddr)>,
    },
    Put {
        key: String,
        val: Vec<u8>,
    },
    Get {
        key: String,
    },
    Got {
        key: String,
        val: Option<Vec<u8>>,
    },
    Ping,
    Pong,
}

pub fn encode_tablero(ofertas: &[Oferta]) -> Vec<u8> {
    serde_json::to_vec(ofertas).unwrap_or_default()
}

pub fn decode_tablero(b: &[u8]) -> Vec<Oferta> {
    serde_json::from_slice(b).unwrap_or_default()
}

pub fn encode_obras(obras: &[Obra]) -> Vec<u8> {
    serde_json::to_vec(obras).unwrap_or_default()
}

pub fn decode_obras(b: &[u8]) -> Vec<Obra> {
    serde_json::from_slice(b).unwrap_or_default()
}

pub fn key_hex(k: &[u8; 32]) -> String {
    hex::encode(k)
}
