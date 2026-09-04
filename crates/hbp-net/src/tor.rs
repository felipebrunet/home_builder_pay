//! Tor point-to-point for Windows v1.
//!
//! # One-click product path (findable Hidden Service)
//!
//! 1. Find `tor.exe` / `tor` (next to the app, cache dir, PATH, or Tor Browser).
//! 2. If missing, download the official Expert Bundle from dist.torproject.org
//!    into `%LOCALAPPDATA%\home_builder_pay\tor\` (or `~/.local/share/...`).
//! 3. Spawn **our** Tor with a dedicated `torrc` and `HiddenServicePort 80`
//!    → the DHT listen port. Wait for `hidden_service/hostname`.
//! 4. Tor Browser SOCKS **9150** is outbound-only fallback if spawn fails.
//!
//! The user does not need to know “Expert Bundle” vs “Tor Browser”.
//! [`socks5_connect`] is a real SOCKS5 CONNECT (RFC 1928, no auth).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

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

/// Expert Bundle (9050) then Tor Browser (9150).
pub const SOCKS_CANDIDATE_PORTS: &[u16] = &[9050, 9150];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredSocks {
    pub addr: SocketAddr,
    /// Short English label for logs (`Tor`, `Tor Browser`).
    pub label: &'static str,
}

pub fn socks_label(port: u16) -> &'static str {
    match port {
        9150 => "Tor Browser",
        _ => "Tor",
    }
}

pub fn probe_socks_port(host: &str, port: u16) -> bool {
    let Ok(addr) = format!("{host}:{port}").parse::<SocketAddr>() else {
        return false;
    };
    TcpStream::connect_timeout(&addr, Duration::from_millis(400)).is_ok()
}

/// First reachable SOCKS: explicit env, then 9050, then 9150 (Tor Browser).
pub fn discover_socks() -> Option<DiscoveredSocks> {
    let mut cands: Vec<(SocketAddr, &'static str)> = Vec::new();
    if let Ok(s) = std::env::var("HBP_TOR_SOCKS") {
        if let Ok(addr) = s.parse::<SocketAddr>() {
            cands.push((addr, socks_label(addr.port())));
        } else if let Some(addr) = s.to_socket_addrs().ok().and_then(|mut it| it.next()) {
            cands.push((addr, socks_label(addr.port())));
        }
    }
    for port in SOCKS_CANDIDATE_PORTS {
        let addr = SocketAddr::from(([127, 0, 0, 1], *port));
        if !cands.iter().any(|(a, _)| a == &addr) {
            cands.push((addr, socks_label(*port)));
        }
    }
    for (addr, label) in cands {
        if probe_socks_port(&addr.ip().to_string(), addr.port()) {
            return Some(DiscoveredSocks { addr, label });
        }
    }
    None
}

fn control_ports_for(socks_port: u16) -> Vec<u16> {
    if let Ok(s) = std::env::var("HBP_TOR_CONTROL") {
        if let Ok(p) = s.parse() {
            return vec![p];
        }
    }
    match socks_port {
        9150 => vec![9151, 9051],
        _ => vec![9051, 9151],
    }
}

/// Places a Windows build should look for a local `tor.exe`.
pub fn default_windows_tor_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(bin) = std::env::var("TOR_BINARY") {
        out.push(PathBuf::from(bin));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join("tor.exe"));
            out.push(dir.join("Tor").join("tor.exe"));
            out.push(dir.join("tor").join("tor"));
        }
    }
    let cache = crate::bundle::tor_cache_dir();
    out.push(cache.join("unpacked"));
    if let Some(found) = crate::bundle::find_tor_in_dir(&cache) {
        out.insert(0, found);
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let local = PathBuf::from(local);
        out.push(local.join("home_builder_pay").join("tor").join("unpacked"));
        out.push(local.join("Tor").join("tor.exe"));
        out.push(
            local
                .join("Tor Browser")
                .join("Browser")
                .join("TorBrowser")
                .join("Tor")
                .join("tor.exe"),
        );
    }
    for home_key in ["USERPROFILE", "HOME"] {
        if let Ok(home) = std::env::var(home_key) {
            let home = PathBuf::from(home);
            for rel in [
                "Desktop/Tor Browser/Browser/TorBrowser/Tor/tor.exe",
                "OneDrive/Desktop/Tor Browser/Browser/TorBrowser/Tor/tor.exe",
            ] {
                out.push(home.join(rel));
            }
        }
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

    #[test]
    fn socks_candidates_include_tor_browser() {
        assert!(SOCKS_CANDIDATE_PORTS.contains(&9050));
        assert!(SOCKS_CANDIDATE_PORTS.contains(&9150));
        assert_eq!(socks_label(9150), "Tor Browser");
        assert!(!probe_socks_port("127.0.0.1", 1));
    }

    #[test]
    fn reuse_product_tor_when_socks_and_hostname_exist() {
        let dir = std::env::temp_dir().join(format!("hbp-reuse-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let hs = dir.join("tor").join("hidden_service");
        std::fs::create_dir_all(&hs).unwrap();
        std::fs::write(hs.join("hostname"), "abcxyz.onion\n").unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::fs::write(
            dir.join("tor").join("runtime.json"),
            format!(r#"{{"socks":"127.0.0.1:{port}","onion":"abcxyz.onion"}}"#),
        )
        .unwrap();
        let rt = reuse_running_product_tor(&dir).expect("reuse");
        assert!(rt.findable);
        assert_eq!(rt.onion.as_deref(), Some("abcxyz.onion"));
        assert!(rt.hint_es.contains("puedes ser encontrado"));
        assert!(!rt.hint_es.to_ascii_lowercase().contains("expert"));
        drop(listener);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn writes_windows_style_torrc_with_hidden_service() {
        let dir = std::env::temp_dir().join(format!("hbp-torrc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let torrc = write_product_torrc(&dir, 19050, 19051, 3848).unwrap();
        let txt = std::fs::read_to_string(&torrc).unwrap();
        assert!(txt.contains("SocksPort 127.0.0.1:19050"));
        assert!(txt.contains("HiddenServicePort 80 127.0.0.1:3848"));
        assert!(txt.contains("CookieAuthentication 1"));
        assert!(txt.contains("HiddenServiceDir \""));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Search `tor.exe` / `tor` next to the app, cache, env, LOCALAPPDATA, then PATH.
pub fn find_tor_binary() -> Option<PathBuf> {
    for p in default_windows_tor_paths() {
        if p.is_file() {
            return Some(p);
        }
        if p.is_dir() {
            if let Some(f) = crate::bundle::find_tor_in_dir(&p) {
                return Some(f);
            }
        }
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) {
            for name in ["tor.exe", "tor"] {
                let c = dir.join(name);
                if c.is_file() {
                    return Some(c);
                }
            }
        }
    }
    None
}

fn torrc_path(p: &Path) -> String {
    format!("\"{}\"", p.to_string_lossy().replace('\\', "/"))
}

pub fn write_product_torrc(
    root: &Path,
    socks_port: u16,
    control_port: u16,
    app_port: u16,
) -> crate::Result<PathBuf> {
    let data = root.join("data");
    let hs = root.join("hidden_service");
    std::fs::create_dir_all(&data)?;
    std::fs::create_dir_all(&hs)?;
    let torrc = root.join("torrc");
    let mut body = format!(
        "DataDirectory {}\n\
         SocksPort 127.0.0.1:{socks_port}\n\
         ControlPort 127.0.0.1:{control_port}\n\
         CookieAuthentication 1\n\
         HiddenServiceDir {}\n\
         HiddenServicePort 80 127.0.0.1:{app_port}\n\
         HiddenServiceVersion 3\n",
        torrc_path(&data),
        torrc_path(&hs)
    );
    if let Some(bin) = find_tor_binary() {
        if let Some(dir) = bin.parent() {
            let geo = dir.join("geoip");
            let geo6 = dir.join("geoip6");
            if geo.is_file() {
                body.push_str(&format!("GeoIPFile {}\n", torrc_path(&geo)));
            }
            if geo6.is_file() {
                body.push_str(&format!("GeoIPv6File {}\n", torrc_path(&geo6)));
            }
        }
    }
    std::fs::write(&torrc, body)?;
    Ok(torrc)
}

pub fn spawn_tor(binary: &Path, torrc: &Path) -> crate::Result<Child> {
    spawn_tor_logged(binary, torrc, None)
}

fn spawn_tor_logged(binary: &Path, torrc: &Path, stderr: Option<&Path>) -> crate::Result<Child> {
    let mut cmd = Command::new(binary);
    cmd.arg("-f").arg(torrc).stdin(Stdio::null()).stdout(Stdio::null());
    if let Some(p) = stderr {
        let f = std::fs::File::create(p)?;
        cmd.stderr(Stdio::from(f));
    } else {
        cmd.stderr(Stdio::null());
    }
    cmd.spawn()
        .map_err(|e| crate::Error::msg(format!("spawn tor: {e}")))
}

pub fn read_onion_hostname(hs_dir: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(hs_dir.join("hostname")).ok()?;
    let line = raw.lines().next()?.trim();
    if line.ends_with(".onion") {
        Some(line.to_string())
    } else {
        None
    }
}

pub fn wait_for_onion_hostname(hs_dir: &Path, timeout: Duration) -> Option<String> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Some(h) = read_onion_hostname(hs_dir) {
            return Some(h);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    None
}

pub struct TorRuntime {
    pub child: Option<Child>,
    pub onion: Option<String>,
    pub socks: SocketAddr,
    pub detail: String,
    /// One-line Spanish status for the product GUI (no env-var jargon).
    pub hint_es: String,
    /// True when we have a Hidden Service onion others can dial.
    pub findable: bool,
}

impl Drop for TorRuntime {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
        }
    }
}

const PRODUCT_SOCKS_BASE: u16 = 19_050;
const FINDABLE_HINT: &str = "Conectado. Ya puedes ser encontrado.";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedRuntime {
    socks: String,
    onion: String,
}

fn pick_free_port(start: u16) -> u16 {
    for p in start..start.saturating_add(30) {
        if std::net::TcpListener::bind(("127.0.0.1", p)).is_ok() {
            return p;
        }
    }
    start
}

fn ensure_tor_binary(progress: &mut impl FnMut(&str)) -> crate::Result<PathBuf> {
    progress("Buscando Tor…");
    if let Some(bin) = find_tor_binary() {
        return Ok(bin);
    }
    let cache = crate::bundle::tor_cache_dir();
    crate::bundle::download_expert_bundle(&cache, progress)
}

fn save_runtime(root: &Path, socks: SocketAddr, onion: &str) {
    let rec = SavedRuntime {
        socks: socks.to_string(),
        onion: onion.to_string(),
    };
    let _ = std::fs::write(
        root.join("tor").join("runtime.json"),
        serde_json::to_string_pretty(&rec).unwrap_or_default(),
    );
}

fn reuse_running_product_tor(root: &Path) -> Option<TorRuntime> {
    let p = root.join("tor").join("runtime.json");
    let rec: SavedRuntime = serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()?;
    let socks: SocketAddr = rec.socks.parse().ok()?;
    if !probe_socks_port(&socks.ip().to_string(), socks.port()) {
        return None;
    }
    let onion = read_onion_hostname(&root.join("tor").join("hidden_service")).or(Some(rec.onion))?;
    Some(TorRuntime {
        child: None,
        onion: Some(onion.clone()),
        socks,
        detail: format!("reused product Tor at {socks}"),
        hint_es: format!("{FINDABLE_HINT} Tu código: {onion}"),
        findable: true,
    })
}

fn outbound_fallback(app_port: u16) -> Option<TorRuntime> {
    let found = discover_socks()?;
    let socks = found.addr;
    for control_port in control_ports_for(socks.port()) {
        if let Some(onion) = try_add_onion_via_control(control_port, app_port) {
            return Some(TorRuntime {
                child: None,
                onion: Some(onion.clone()),
                socks,
                detail: format!("attached via ADD_ONION on {}", found.label),
                hint_es: format!("{FINDABLE_HINT} Tu código: {onion}"),
                findable: true,
            });
        }
    }
    Some(TorRuntime {
        child: None,
        onion: None,
        socks,
        detail: format!("{} SOCKS {} outbound only", found.label, socks),
        hint_es: "Conectado solo para hablar. Aún no te pueden encontrar. Vuelve a pulsar Conectar red.".into(),
        findable: false,
    })
}

/// Bring up Tor for the product DHT listen port.
///
/// Product path: **always try to spawn our own Hidden Service** so this
/// node is findable. Tor Browser (9150) is outbound fallback only.
pub fn bring_up_tor(root: &Path, app_port: u16) -> crate::Result<TorRuntime> {
    bring_up_tor_with_hint(root, app_port, |_| {})
}

pub fn bring_up_tor_with_hint(
    root: &Path,
    app_port: u16,
    mut progress: impl FnMut(&str),
) -> crate::Result<TorRuntime> {
    if let Some(rt) = reuse_running_product_tor(root) {
        progress(&rt.hint_es);
        return Ok(rt);
    }

    match ensure_tor_binary(&mut progress) {
        Ok(bin) => {
            progress("Arrancando Tor y creando tu dirección…");
            let socks_port = pick_free_port(PRODUCT_SOCKS_BASE);
            let control_port = pick_free_port(socks_port.saturating_add(1));
            let socks = SocketAddr::from(([127, 0, 0, 1], socks_port));
            let tor_root = root.join("tor");
            let torrc = write_product_torrc(&tor_root, socks_port, control_port, app_port)?;
            let log = tor_root.join("tor.stderr");
            match spawn_tor_logged(&bin, &torrc, Some(&log)) {
                Ok(mut child) => {
                    let hs = tor_root.join("hidden_service");
                    let onion = wait_for_onion_hostname(&hs, Duration::from_secs(90));
                    if onion.is_none() {
                        if let Ok(Some(st)) = child.try_wait() {
                            let tail = std::fs::read_to_string(&log).unwrap_or_default();
                            let tail: String = tail.chars().rev().take(400).collect::<String>().chars().rev().collect();
                            if let Some(fb) = outbound_fallback(app_port) {
                                return Ok(fb);
                            }
                            return Ok(TorRuntime {
                                child: None,
                                onion: None,
                                socks,
                                detail: format!("tor exited {st}; {tail}"),
                                hint_es: "No pude arrancar Tor. Revisa internet y vuelve a pulsar Conectar red.".into(),
                                findable: false,
                            });
                        }
                    }
                    if let Some(onion) = onion {
                        save_runtime(root, socks, &onion);
                        return Ok(TorRuntime {
                            child: Some(child),
                            onion: Some(onion.clone()),
                            socks,
                            detail: format!("spawned {} HS ready", bin.display()),
                            hint_es: format!("{FINDABLE_HINT} Tu código: {onion}"),
                            findable: true,
                        });
                    }
                    // Still booting — keep the child; user can reconnect (reuse hostname later).
                    Ok(TorRuntime {
                        child: Some(child),
                        onion: None,
                        socks,
                        detail: "spawned; hostname not yet written".into(),
                        hint_es: "Tor está arrancando. Espera un momento y pulsa otra vez Conectar red.".into(),
                        findable: false,
                    })
                }
                Err(e) => {
                    if let Some(fb) = outbound_fallback(app_port) {
                        return Ok(fb);
                    }
                    Ok(TorRuntime {
                        child: None,
                        onion: None,
                        socks,
                        detail: e.to_string(),
                        hint_es: "No pude arrancar Tor. Vuelve a pulsar Conectar red.".into(),
                        findable: false,
                    })
                }
            }
        }
        Err(e) => {
            if let Some(fb) = outbound_fallback(app_port) {
                return Ok(fb);
            }
            Ok(TorRuntime {
                child: None,
                onion: None,
                socks: default_socks_addr(),
                detail: e.to_string(),
                hint_es: "No encontré Tor y no pude bajarlo. Revisa internet y pulsa Conectar red.".into(),
                findable: false,
            })
        }
    }
}

fn try_add_onion_via_control(control_port: u16, app_port: u16) -> Option<String> {
    let addr: SocketAddr = format!("127.0.0.1:{control_port}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(800)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(3)));
    let cookie = control_cookie_bytes()?;
    let auth = format!("AUTHENTICATE {}\r\n", hex::encode(cookie));
    s.write_all(auth.as_bytes()).ok()?;
    if !control_ok(&mut s) {
        return None;
    }
    let cmd = format!("ADD_ONION NEW:ED25519-V3 Flags=Detach Port=80,127.0.0.1:{app_port}\r\n");
    s.write_all(cmd.as_bytes()).ok()?;
    let reply = control_read(&mut s)?;
    for line in reply.lines() {
        if let Some(id) = line.strip_prefix("250-ServiceID=") {
            return Some(format!("{}.onion", id.trim()));
        }
    }
    None
}

fn control_cookie_bytes() -> Option<Vec<u8>> {
    let mut cands = Vec::new();
    if let Ok(p) = std::env::var("HBP_TOR_COOKIE") {
        cands.push(PathBuf::from(p));
    }
    if let Ok(home) = std::env::var("HOME") {
        cands.push(PathBuf::from(home).join(".tor").join("control_auth_cookie"));
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let local = PathBuf::from(local);
        cands.push(local.join("Tor").join("control_auth_cookie"));
        cands.push(
            local
                .join("Tor Browser")
                .join("Browser")
                .join("TorBrowser")
                .join("Data")
                .join("Tor")
                .join("control_auth_cookie"),
        );
    }
    for p in cands {
        if let Ok(b) = std::fs::read(&p) {
            if !b.is_empty() {
                return Some(b);
            }
        }
    }
    None
}

fn control_ok(s: &mut TcpStream) -> bool {
    control_read(s)
        .map(|r| r.contains("250"))
        .unwrap_or(false)
}

fn control_read(s: &mut TcpStream) -> Option<String> {
    let mut buf = [0u8; 4096];
    let n = s.read(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf[..n]).into_owned())
}
