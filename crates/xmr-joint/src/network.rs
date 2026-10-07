//! Red Monero y separadores de dominio. El binario de prueba usa stagenet.

use std::sync::RwLock;

use sha2::{Digest, Sha256};

use monero_wallet::address::Network;

/// 1 XMR en piconero.
pub const PICONERO: u64 = 1_000_000_000_000;

/// Daemon público de stagenet (HTTPS, RPC restringido). Fallback cuando no hay
/// nodo propio configurado. El binario puede pasar otro con `--daemon`.
pub const STAGENET_DAEMON: &str = "https://stagenet.xmr.kernal.eu:38089";

/// Anillo CLSAG de Bulletproof+.
pub const RING_LEN: u8 = 16;

/// Margen que el mandante necesita por encima del capital para el fee.
///
/// 0,001 XMR. Si el daemon pide más, el fondeo falla con un error de fondos
/// y hay que cubrir la billetera personal.
pub const FEE_CUSHION: u64 = PICONERO / 1_000;

/// URL propia de daemon (proceso). `None` = usar [`STAGENET_DAEMON`].
static DAEMON_CUSTOM: RwLock<Option<String>> = RwLock::new(None);

/// Valida una URL de daemon de stagenet: `http(s)://host:puerto` (path opcional).
///
/// Ejemplos: `http://100.64.0.2:38081`, `https://stagenet.xmr.kernal.eu:38089`.
pub fn validar_daemon_url(raw: &str) -> Result<String, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("La URL del nodo está vacía.".into());
    }
    if s.chars().any(|c| c.is_whitespace()) {
        return Err("La URL del nodo no puede tener espacios.".into());
    }
    let (scheme, rest) = if let Some(r) = s.strip_prefix("https://") {
        ("https", r)
    } else if let Some(r) = s.strip_prefix("http://") {
        ("http", r)
    } else {
        return Err("La URL tiene que empezar con http:// o https://.".into());
    };
    if rest.is_empty() || rest.starts_with('/') {
        return Err("Falta el host del nodo.".into());
    }
    let authority = rest.split('/').next().unwrap_or(rest);
    if authority.is_empty() {
        return Err("Falta el host del nodo.".into());
    }
    let port_part = if let Some(rest_v6) = authority.strip_prefix('[') {
        let Some((_, port)) = rest_v6.split_once("]:") else {
            return Err("Falta el puerto (ej. :38081).".into());
        };
        port
    } else {
        let Some((_host, port)) = authority.rsplit_once(':') else {
            return Err("Falta el puerto (ej. :38081).".into());
        };
        if _host.is_empty() {
            return Err("Falta el host del nodo.".into());
        }
        port
    };
    let port: u16 = port_part
        .parse()
        .map_err(|_| "El puerto no es válido.".to_string())?;
    if port == 0 {
        return Err("El puerto no es válido.".into());
    }
    Ok(format!("{scheme}://{rest}").trim_end_matches('/').to_string())
}

/// URL del daemon de stagenet en uso (propia o la pública).
pub fn daemon_url() -> String {
    DAEMON_CUSTOM
        .read()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_else(|| STAGENET_DAEMON.to_string())
}

/// `true` si no hay nodo propio y se usa el público.
pub fn daemon_es_defecto() -> bool {
    DAEMON_CUSTOM
        .read()
        .ok()
        .map(|g| g.is_none())
        .unwrap_or(true)
}

/// Fija un daemon propio. `None` o vacío vuelve al público [`STAGENET_DAEMON`].
pub fn fijar_daemon(url: Option<&str>) -> Result<(), String> {
    let mut g = DAEMON_CUSTOM
        .write()
        .map_err(|_| "No pude cambiar el nodo (lock).".to_string())?;
    match url.map(str::trim).filter(|s| !s.is_empty()) {
        None => {
            *g = None;
            Ok(())
        }
        Some(u) => {
            let ok = validar_daemon_url(u)?;
            *g = Some(ok);
            Ok(())
        }
    }
}

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

    /// Daemon activo para esta red (respeta nodo propio en stagenet).
    pub fn daemon(self) -> Option<String> {
        match self {
            Self::Stagenet => Some(daemon_url()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valida_urls_ok() {
        assert!(validar_daemon_url("http://100.64.0.2:38081").is_ok());
        assert!(validar_daemon_url("https://stagenet.xmr.kernal.eu:38089").is_ok());
        assert!(validar_daemon_url("http://[::1]:38081").is_ok());
        assert_eq!(
            validar_daemon_url("http://10.0.0.5:38081/").unwrap(),
            "http://10.0.0.5:38081"
        );
    }

    #[test]
    fn valida_urls_mal() {
        assert!(validar_daemon_url("ftp://x:1").is_err());
        assert!(validar_daemon_url("http://sinpuerto").is_err());
        assert!(validar_daemon_url("http://:38081").is_err());
        assert!(validar_daemon_url("https://host:0").is_err());
        assert!(validar_daemon_url("http://host:abc").is_err());
        assert!(validar_daemon_url("  ").is_err());
    }

    #[test]
    fn fijar_y_limpiar() {
        let _ = fijar_daemon(None);
        assert!(daemon_es_defecto());
        assert_eq!(daemon_url(), STAGENET_DAEMON);
        fijar_daemon(Some("http://100.64.0.2:38081")).unwrap();
        assert!(!daemon_es_defecto());
        assert_eq!(daemon_url(), "http://100.64.0.2:38081");
        fijar_daemon(None).unwrap();
        assert!(daemon_es_defecto());
    }
}
