//! Red Monero y separadores de dominio. El binario de prueba usa stagenet.

use sha2::{Digest, Sha256};

use monero_wallet::address::Network;

/// 1 XMR en piconero.
pub const PICONERO: u64 = 1_000_000_000_000;

/// Daemon público de stagenet (HTTPS, RPC restringido). El binario puede
/// pasar otro con `--daemon`. La ventana usa solo este.
pub const STAGENET_DAEMON: &str = "https://stagenet.xmr.kernal.eu:38089";

/// Anillo CLSAG de Bulletproof+.
pub const RING_LEN: u8 = 16;

/// Margen que el mandante necesita por encima del capital para el fee.
///
/// 0,001 XMR. Si el daemon pide más, el fondeo falla con un error de fondos
/// y hay que cubrir la billetera personal.
pub const FEE_CUSHION: u64 = PICONERO / 1_000;

/// Red lógica. `regtest` no se ofrece: el esqueleto se prueba en stagenet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Net {
    Stagenet,
    Testnet,
    Mainnet,
}

impl Net {
    pub fn parse(s: &str) -> std::result::Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "stagenet" => Ok(Self::Stagenet),
            "testnet" => Ok(Self::Testnet),
            "mainnet" => Ok(Self::Mainnet),
            other => Err(format!("red {other:?} no está en el esqueleto")),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Stagenet => "stagenet",
            Self::Testnet => "testnet",
            Self::Mainnet => "mainnet",
        }
    }

    pub fn oxide(self) -> Network {
        match self {
            Self::Stagenet => Network::Stagenet,
            Self::Testnet => Network::Testnet,
            Self::Mainnet => Network::Mainnet,
        }
    }

    pub fn daemon_default(self) -> Option<&'static str> {
        match self {
            Self::Stagenet => Some(STAGENET_DAEMON),
            Self::Testnet | Self::Mainnet => None,
        }
    }
}

fn tagged_hash(tag: &[u8], net: Net, extra: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(tag);
    h.update([0]);
    h.update(net.label().as_bytes());
    h.update([0]);
    h.update(extra);
    h.finalize().into()
}

/// Contexto PedPoP de 32 bytes. Incluye la obra para que dos tratos no compartan dominio.
pub fn dkg_context(obra_id: &str, net: Net) -> [u8; 32] {
    tagged_hash(b"konstruado/xmr-joint/dkg/v1", net, obra_id.as_bytes())
}

/// Sesión de fondeo atada a la obra, la partida y la red. No es un secreto.
pub fn fund_session_bytes(obra_id: &str, partida: u32, net: Net) -> [u8; 32] {
    let mut extra = obra_id.as_bytes().to_vec();
    extra.push(0);
    extra.extend(partida.to_le_bytes());
    tagged_hash(b"konstruado/xmr-joint/fund/v1", net, &extra)
}
