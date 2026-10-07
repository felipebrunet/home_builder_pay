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

/// Host de una URL de daemon (sin esquema, usuario, puerto ni corchetes IPv6).
pub fn host_de_url(url: &str) -> Option<String> {
    let s = url.trim();
    let rest = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .unwrap_or(s);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let authority = authority.rsplit_once('@').map(|(_, h)| h).unwrap_or(authority);
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split_once(']').map(|(h, _)| h).unwrap_or(v6)
    } else {
        authority.rsplit_once(':').map(|(h, _)| h).unwrap_or(authority)
    };
    let host = host.trim().trim_end_matches('.');
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// `true` si el host es de la red local / privada y no tiene sentido mandarlo por Tor:
/// RFC1918 (10/8, 172.16/12, 192.168/16), CGNAT/Tailscale 100.64/10, loopback,
/// link-local, IPv6 ULA (fc00::/7) y link-local (fe80::/10), `localhost` y nombres
/// `.local`, `.lan`, `.home.arpa`, `.internal`, `.ts.net` (MagicDNS de Tailscale).
///
/// Un salida de Tor nunca llega a esas direcciones. El RPC del daemon no usa SOCKS
/// (va directo por TCP); esta clasificación sirve para explicar la ruta y para
/// detectar cuando una VPN de todo el teléfono (Orbot) lo está capturando.
pub fn es_host_local(host: &str) -> bool {
    use std::net::IpAddr;
    let h = host.trim().trim_start_matches('[').trim_end_matches(']').trim_end_matches('.');
    let h = h.to_ascii_lowercase();
    if let Ok(ip) = h.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => ipv4_local(v4),
            IpAddr::V6(v6) => {
                if let Some(v4) = v6.to_ipv4_mapped() {
                    return ipv4_local(v4);
                }
                let seg0 = v6.segments()[0];
                v6.is_loopback()
                    || v6.is_unspecified()
                    || (seg0 & 0xfe00) == 0xfc00 // ULA fc00::/7
                    || (seg0 & 0xffc0) == 0xfe80 // link-local fe80::/10
            }
        };
    }
    h == "localhost"
        || [".localhost", ".local", ".lan", ".home.arpa", ".internal", ".ts.net"]
            .iter()
            .any(|suf| h.ends_with(suf))
}

fn ipv4_local(v4: std::net::Ipv4Addr) -> bool {
    let o = v4.octets();
    v4.is_private()
        || v4.is_loopback()
        || v4.is_link_local()
        || v4.is_unspecified()
        || (o[0] == 100 && (o[1] & 0xc0) == 64) // 100.64.0.0/10 (CGNAT / Tailscale)
}

/// `true` si la URL apunta a un nodo de la red local / Tailscale.
pub fn url_es_local(url: &str) -> bool {
    host_de_url(url).is_some_and(|h| es_host_local(&h))
}

/// `true` si el daemon activo es un nodo local (LAN / Tailscale).
pub fn daemon_es_local() -> bool {
    url_es_local(&daemon_url())
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

    #[test]
    fn hosts_locales() {
        for u in [
            "http://192.168.1.83:38081",
            "http://10.0.0.5:38081/json_rpc",
            "http://172.16.4.2:38081",
            "http://172.31.255.1:38081",
            "http://100.64.0.2:38081",
            "http://100.127.1.1:38081",
            "http://127.0.0.1:38081",
            "http://169.254.3.3:38081",
            "http://[::1]:38081",
            "http://[fd7a:115c:a1e0::1]:38081",
            "http://[fe80::1]:38081",
            "http://user:pw@192.168.0.10:38081",
            "http://localhost:38081",
            "http://monero.local:38081",
            "http://pc.tail1234.ts.net:38081",
        ] {
            assert!(url_es_local(u), "{u} debería ser local");
        }
        for u in [
            STAGENET_DAEMON,
            "http://172.32.0.1:38081",
            "http://100.128.0.1:38081",
            "http://8.8.8.8:38081",
            "http://[2001:db8::1]:38081",
            "https://node.example.com:38089",
            "http://localhost.example.com:1",
        ] {
            assert!(!url_es_local(u), "{u} no debería ser local");
        }
        assert_eq!(host_de_url("http://[::1]:38081").as_deref(), Some("::1"));
        assert_eq!(host_de_url("https://a:b@Host.EXAMPLE:1/x").as_deref(), Some("host.example"));
    }
}
