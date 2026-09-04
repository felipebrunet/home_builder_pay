//! Tor point-to-point for Windows v1.
//!
//! # Windows-workable approach (bundled, not a hidden service factory)
//!
//! 1. Ship or document the **Tor Expert Bundle** (`tor.exe` + `geoip`).
//!    The GUI looks next to `home_builder_pay.exe`, then
//!    `%LOCALAPPDATA%\Tor\tor.exe`, then `TOR_BINARY`.
//! 2. The process talks **only** through SOCKS5 (`127.0.0.1:9050` by default,
//!    override `HBP_TOR_SOCKS` / [`TorConfig::socks`]).
//! 3. Each work may publish a `.onion` hostname *in the offer / announce*
//!    (already-known contact). This crate connects to that onion via SOCKS;
//!    it does not yet spawn `HiddenServiceDir` for you.
//! 4. File-passing remains the fallback if Tor is down ([`crate::FILE_FALLBACK`]).
//!
//! Scaffolding vs ready: [`socks5_connect`] is a real SOCKS5 CONNECT (RFC 1928,
//! no auth). [`tor_status`] probes the port. There is no control-port cookie
//! parser and no auto-launch of `tor.exe` in this PR.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TorConfig {
    pub socks_host: String,
    pub socks_port: u16,
    /// Known peer onion (no `http://`), if the contact already exists.
    pub peer_onion: Option<String>,
    pub peer_port: u16,
    pub connect_timeout_ms: u64,
}

impl Default for TorConfig {
    fn default() -> Self {
        Self {
            socks_host: "127.0.0.1".into(),
            socks_port: 9050,
            peer_onion: None,
            peer_port: 80,
            connect_timeout_ms: 8_000,
        }
    }
}

impl TorConfig {
    pub fn from_env() -> Self {
        let mut c = Self::default();
        if let Ok(s) = std::env::var("HBP_TOR_SOCKS") {
            if let Some((h, p)) = s.rsplit_once(':') {
                c.socks_host = h.to_string();
                if let Ok(port) = p.parse() {
                    c.socks_port = port;
                }
            }
        }
        c
    }

    pub fn socks(&self) -> String {
        format!("{}:{}", self.socks_host, self.socks_port)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TorStatus {
    pub socks: String,
    pub reachable: bool,
    pub detail: String,
    pub suggested_tor_binary: Option<String>,
}

pub fn default_socks_addr() -> SocketAddr {
    "127.0.0.1:9050".parse().expect("static")
}

/// Places a Windows build should look for a local `tor.exe`.
pub fn default_windows_tor_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join("tor.exe"));
            out.push(dir.join("Tor").join("tor.exe"));
        }
    }
    if let Ok(bin) = std::env::var("TOR_BINARY") {
        out.push(PathBuf::from(bin));
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        out.push(PathBuf::from(local).join("Tor").join("tor.exe"));
    }
    out
}

pub fn tor_status(cfg: &TorConfig) -> TorStatus {
    let socks = cfg.socks();
    let suggested = default_windows_tor_paths()
        .into_iter()
        .find(|p| p.exists())
        .map(|p| p.display().to_string());
    match TcpStream::connect_timeout(
        &match socks.to_socket_addrs() {
            Ok(mut it) => match it.next() {
                Some(a) => a,
                None => {
                    return TorStatus {
                        socks,
                        reachable: false,
                        detail: "socks address did not resolve".into(),
                        suggested_tor_binary: suggested,
                    };
                }
            },
            Err(e) => {
                return TorStatus {
                    socks,
                    reachable: false,
                    detail: e.to_string(),
                    suggested_tor_binary: suggested,
                };
            }
        },
        Duration::from_millis(400),
    ) {
        Ok(_) => TorStatus {
            socks,
            reachable: true,
            detail: "SOCKS port accepted a TCP connect (Tor or a proxy is listening)".into(),
            suggested_tor_binary: suggested,
        },
        Err(e) => TorStatus {
            socks,
            reachable: false,
            detail: format!("SOCKS not reachable: {e}"),
            suggested_tor_binary: suggested,
        },
    }
}

/// SOCKS5 CONNECT (no auth) to `dest_host:dest_port` via `socks`.
///
/// `dest_host` may be a `.onion` hostname — Tor resolves it.
pub fn socks5_connect(
    socks: SocketAddr,
    dest_host: &str,
    dest_port: u16,
    timeout: Duration,
) -> crate::Result<TcpStream> {
    if dest_host.is_empty() || dest_host.len() > 255 {
        return Err(crate::Error::msg("bad SOCKS destination host"));
    }
    let mut stream = TcpStream::connect_timeout(&socks, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(&[0x05, 0x01, 0x00])?;
    let mut greet = [0u8; 2];
    stream.read_exact(&mut greet)?;
    if greet[0] != 0x05 || greet[1] != 0x00 {
        return Err(crate::Error::msg(format!(
            "SOCKS5 handshake rejected ({:02x} {:02x})",
            greet[0], greet[1]
        )));
    }
    let host = dest_host.as_bytes();
    let mut req = Vec::with_capacity(7 + host.len());
    req.extend_from_slice(&[0x05, 0x01, 0x00, 0x03, host.len() as u8]);
    req.extend_from_slice(host);
    req.extend_from_slice(&dest_port.to_be_bytes());
    stream.write_all(&req)?;
    let mut hdr = [0u8; 4];
    stream.read_exact(&mut hdr)?;
    if hdr[0] != 0x05 || hdr[1] != 0x00 {
        return Err(crate::Error::msg(format!(
            "SOCKS5 CONNECT failed (rep={})",
            hdr[1]
        )));
    }
    match hdr[3] {
        0x01 => {
            let mut rest = [0u8; 6];
            stream.read_exact(&mut rest)?;
        }
        0x04 => {
            let mut rest = [0u8; 18];
            stream.read_exact(&mut rest)?;
        }
        0x03 => {
            let mut ln = [0u8; 1];
            stream.read_exact(&mut ln)?;
            let mut rest = vec![0u8; ln[0] as usize + 2];
            stream.read_exact(&mut rest)?;
        }
        other => return Err(crate::Error::msg(format!("bad SOCKS atyp {other}"))),
    }
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_search_paths_include_exe_dir_and_env() {
        let paths = default_windows_tor_paths();
        assert!(paths.iter().any(|p| p.ends_with("tor.exe") || p.ends_with("tor")));
    }

    #[test]
    fn status_without_tor_is_honest() {
        let mut cfg = TorConfig::default();
        cfg.socks_port = 1;
        let st = tor_status(&cfg);
        assert!(!st.reachable);
        assert!(st.detail.contains("not reachable") || st.detail.contains("did not resolve"));
    }
}
