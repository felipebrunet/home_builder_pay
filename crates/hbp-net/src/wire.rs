//! Length-prefixed JSON RPC over TCP (Tor SOCKS or direct).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::dht::{DhtRecord, PeerInfo};
use crate::message::NetMessage;
use crate::tor::socks5_connect;

const MAX_FRAME: u32 = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerAddr {
    pub host: String,
    pub port: u16,
}

impl PeerAddr {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
        }
    }

    pub fn parse_flexible(s: &str) -> crate::Result<Self> {
        let s = s.trim();
        if s.ends_with(".onion") && !s.contains(':') {
            return Self::parse(&format!("{s}:80"));
        }
        Self::parse(s)
    }

    pub fn parse(s: &str) -> crate::Result<Self> {
        let s = s.trim();
        let (host, port) = s
            .rsplit_once(':')
            .ok_or_else(|| crate::Error::msg(format!("peer '{s}' must be host:port")))?;
        let port: u16 = port
            .parse()
            .map_err(|_| crate::Error::msg(format!("bad port in '{s}'")))?;
        if host.is_empty() {
            return Err(crate::Error::msg("empty host"));
        }
        Ok(Self {
            host: host.trim().to_string(),
            port,
        })
    }

    pub fn is_onion(&self) -> bool {
        self.host.ends_with(".onion")
    }

    pub fn is_loopback(&self) -> bool {
        self.host == "127.0.0.1" || self.host == "localhost" || self.host == "::1"
    }

    pub fn display(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

impl std::fmt::Display for PeerAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    Ping { node_id: String, listen: PeerAddr },
    FindNode { target: String },
    FindValue { key: String },
    Store { record: DhtRecord },
    Deliver { msg: NetMessage },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResBody {
    Pong { node_id: String, listen: PeerAddr },
    Nodes { nodes: Vec<PeerInfo> },
    Value { record: DhtRecord },
    Stored,
    Delivered,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WireMsg {
    Req { id: u64, op: Op },
    Res { id: u64, body: ResBody },
    Err { id: u64, error: String },
}

pub fn write_frame(w: &mut impl Write, bytes: &[u8]) -> crate::Result<()> {
    if bytes.len() > MAX_FRAME as usize {
        return Err(crate::Error::msg("frame too large"));
    }
    w.write_all(&(bytes.len() as u32).to_be_bytes())?;
    w.write_all(bytes)?;
    w.flush()?;
    Ok(())
}

pub fn read_frame(r: &mut impl Read) -> crate::Result<Vec<u8>> {
    let mut ln = [0u8; 4];
    r.read_exact(&mut ln)?;
    let n = u32::from_be_bytes(ln);
    if n > MAX_FRAME {
        return Err(crate::Error::msg("frame too large"));
    }
    let mut buf = vec![0u8; n as usize];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

/// Connect to a peer. `.onion` always goes through SOCKS5. Loopback is always
/// direct (tests + local hidden-service target). Everything else uses SOCKS
/// when a proxy is configured (product path: all WAN via Tor).
pub fn connect_peer(
    addr: &PeerAddr,
    socks: Option<SocketAddr>,
    timeout: Duration,
) -> crate::Result<TcpStream> {
    if addr.is_onion() {
        let socks = socks.ok_or_else(|| {
            crate::Error::msg("onion peer requires a Tor SOCKS proxy (HBP_TOR_SOCKS)")
        })?;
        return socks5_connect(socks, &addr.host, addr.port, timeout);
    }
    if addr.is_loopback() || socks.is_none() {
        let sock: SocketAddr = format!("{}:{}", addr.host, addr.port)
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| crate::Error::msg("peer did not resolve"))?;
        let s = TcpStream::connect_timeout(&sock, timeout)?;
        s.set_read_timeout(Some(timeout))?;
        s.set_write_timeout(Some(timeout))?;
        return Ok(s);
    }
    socks5_connect(socks.unwrap(), &addr.host, addr.port, timeout)
}

pub fn parse_bootstrap_list(s: &str) -> crate::Result<Vec<PeerAddr>> {
    let mut out = Vec::new();
    for part in s.split(|c| c == ',' || c == ';' || c == '\n') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        out.push(PeerAddr::parse(part)?);
    }
    Ok(out)
}

pub fn env_bootstrap_peers() -> Vec<PeerAddr> {
    std::env::var("HBP_DHT_BOOTSTRAP")
        .ok()
        .and_then(|s| parse_bootstrap_list(&s).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_onion_and_loopback() {
        let o = PeerAddr::parse("abc.onion:80").unwrap();
        assert!(o.is_onion());
        assert!(!o.is_loopback());
        assert_eq!(o.display(), "abc.onion:80");
        let l = PeerAddr::parse("127.0.0.1:3848").unwrap();
        assert!(l.is_loopback());
        assert!(!l.is_onion());
    }

    #[test]
    fn bootstrap_list_splits_comma_and_newline() {
        let v = parse_bootstrap_list("abc.onion:80, 127.0.0.1:3848\nxyz.onion:80").unwrap();
        assert_eq!(v.len(), 3);
        assert!(v[0].is_onion());
        assert!(v[1].is_loopback());
    }
}
