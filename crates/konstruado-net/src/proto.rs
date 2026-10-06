use serde::{Deserialize, Serialize};

use konstruado_core::{Oferta, Obra, Persona};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PeerAddr {
    Tcp { host: String, port: u16 },
    Onion { host: String, port: u16 },
    /// Nodo sin dirección entrante (celular detrás de Orbot). Solo se le
    /// llega por una sesión que él mismo abrió, o por un relay que la tenga.
    Buzon { node: String },
}

impl PeerAddr {
    /// True si se puede abrir una conexión nueva hacia esta dirección.
    pub fn marcable(&self) -> bool {
        !matches!(self, PeerAddr::Buzon { .. })
    }
}

impl std::fmt::Display for PeerAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PeerAddr::Tcp { host, port } => write!(f, "{host}:{port}"),
            PeerAddr::Onion { host, port } => write!(f, "{host}:{port}"),
            PeerAddr::Buzon { node } => write!(f, "buzon:{}", &node[..node.len().min(8)]),
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
    /// Persona de este nodo. No entra al DHT.
    Soy { node: String, persona: String },
    /// Protocolo de la caja, solo para `para`. `cuerpo` es hex. No entra al DHT.
    Caja {
        obra: String,
        para: String,
        de: String,
        paso: String,
        cuerpo: String,
        /// Saltos de relay. Los pares viejos no lo mandan (0).
        #[serde(default)]
        saltos: u8,
    },
}

/// Mensaje de caja ya decodificado. El hex inválido se tira.
#[derive(Clone, Debug)]
pub struct CajaMsg {
    pub obra: String,
    pub de: String,
    pub paso: String,
    pub cuerpo: Vec<u8>,
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

pub fn encode_presentes(p: &[Persona]) -> Vec<u8> {
    serde_json::to_vec(p).unwrap_or_default()
}

pub fn decode_presentes(b: &[u8]) -> Vec<Persona> {
    serde_json::from_slice(b).unwrap_or_default()
}

pub fn key_hex(k: &[u8; 32]) -> String {
    hex::encode(k)
}
