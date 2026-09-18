use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};

use crate::ctl::Control;
use crate::proto::PeerAddr;
use crate::rendezvous::{RENDEZVOUS_KEY, RENDEZVOUS_ONION, VIRT_PORT};

/// Tor via a private `tor` process (SOCKS + hidden services). The system
/// daemon on 9050 is not used: we cannot ADD_ONION there without group
/// membership.
#[derive(Clone)]
pub struct Tor {
    snap: Arc<Mutex<Snap>>,
}

#[derive(Clone)]
struct Snap {
    estado: EstadoTor,
    socks: Option<SocketAddr>,
    onion: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EstadoTor {
    Ausente,
    Arrancando,
    Listo { onion: String },
    Fallo(String),
}

impl Tor {
    pub fn ausente() -> Self {
        Self {
            snap: Arc::new(Mutex::new(Snap {
                estado: EstadoTor::Ausente,
                socks: None,
                onion: None,
            })),
        }
    }

    pub fn marcar_arrancando(&self) {
        self.snap.lock().unwrap().estado = EstadoTor::Arrancando;
    }

    pub fn marcar_fallo(&self, s: impl Into<String>) {
        self.snap.lock().unwrap().estado = EstadoTor::Fallo(s.into());
    }

    pub fn estado(&self) -> EstadoTor {
        self.snap.lock().unwrap().estado.clone()
    }

    pub fn onion_addr(&self) -> Option<PeerAddr> {
        let g = self.snap.lock().unwrap();
        Some(PeerAddr::Onion {
            host: g.onion.clone()?,
            port: VIRT_PORT,
        })
    }

    pub fn socks(&self) -> Option<SocketAddr> {
        self.snap.lock().unwrap().socks
    }

    pub async fn conectar(&self, host: &str, port: u16) -> std::io::Result<TcpStream> {
        if let Some(socks) = self.socks() {
            let s = tokio_socks::tcp::Socks5Stream::connect(socks, (host, port))
                .await
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            Ok(s.into_inner())
        } else {
            TcpStream::connect((host, port)).await
        }
    }

    /// Own Tor process, personal onion, baked rendezvous onion.
    pub async fn arrancar(local_port: u16) -> std::io::Result<Self> {
        let bin = tor_bin().ok_or_else(|| std::io::Error::other("no está el binario tor"))?;
        let dir = std::env::temp_dir().join(format!("konstruado-tor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        let ctrl_file = dir.join("control-port");
        let log = dir.join("notice.log");
        let mut child = tokio::process::Command::new(&bin)
            .arg("--ignore-missing-torrc")
            .arg("--SocksPort")
            .arg("auto")
            .arg("--ControlPort")
            .arg("auto")
            .arg("--ControlPortWriteToFile")
            .arg(&ctrl_file)
            .arg("--CookieAuthentication")
            .arg("1")
            .arg("--DataDirectory")
            .arg(&dir)
            .arg("--Log")
            .arg(format!("notice file {}", log.display()))
            .arg("--__OwningControllerProcess")
            .arg(std::process::id().to_string())
            .arg("--AvoidDiskWrites")
            .arg("1")
            .arg("--DormantCanceledByStartup")
            .arg("1")
            .kill_on_drop(true)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;

        let addr = wait_control_file(&ctrl_file).await?;
        let cookie = wait_cookie(&dir.join("control_auth_cookie")).await?;
        let mut ctl = Control::conectar(addr, &cookie).await?;
        wait_bootstrap(&mut ctl).await?;
        let socks = parse_socks(&ctl.getinfo("net/listeners/socks").await?)?;
        let onion = ctl.add_onion("NEW", VIRT_PORT, local_port).await?;
        let _ = ctl.add_onion(RENDEZVOUS_KEY, VIRT_PORT, local_port).await;
        drop(ctl);
        let t = Self {
            snap: Arc::new(Mutex::new(Snap {
                estado: EstadoTor::Listo {
                    onion: onion.clone(),
                },
                socks: Some(socks),
                onion: Some(onion),
            })),
        };
        // Keep the child with the Control lifetime: drop of last Tor
        // clone is process end. Park it so kill_on_drop still applies.
        tokio::spawn(async move {
            let _ = child.wait().await;
        });
        Ok(t)
    }
}

pub fn hay_tor() -> bool {
    tor_bin().is_some()
}

fn tor_bin() -> Option<PathBuf> {
    for p in ["tor", "/usr/sbin/tor", "/usr/bin/tor"] {
        let path = PathBuf::from(p);
        if p == "tor" {
            if std::process::Command::new("tor")
                .arg("--version")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
            {
                return Some(path);
            }
        } else if path.exists() {
            return Some(path);
        }
    }
    None
}

async fn wait_control_file(path: &PathBuf) -> std::io::Result<SocketAddr> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(s) = std::fs::read_to_string(path) {
            if let Some(addr) = parse_control_port(&s) {
                return Ok(addr);
            }
        }
        if tokio::time::Instant::now() > deadline {
            return Err(std::io::Error::other("tor no escribió ControlPort"));
        }
        sleep(Duration::from_millis(80)).await;
    }
}

fn parse_control_port(s: &str) -> Option<SocketAddr> {
    let line = s.lines().next()?.trim();
    let rest = line.strip_prefix("PORT=").unwrap_or(line);
    rest.parse().ok()
}

async fn wait_cookie(path: &PathBuf) -> std::io::Result<Vec<u8>> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(b) = std::fs::read(path) {
            if b.len() == 32 {
                return Ok(b);
            }
        }
        if tokio::time::Instant::now() > deadline {
            return Err(std::io::Error::other("tor cookie ausente"));
        }
        sleep(Duration::from_millis(80)).await;
    }
}

async fn wait_bootstrap(ctl: &mut Control) -> std::io::Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(90);
    loop {
        let phase = ctl.getinfo("status/bootstrap-phase").await.unwrap_or_default();
        if crate::ctl::parse_progress(&phase).unwrap_or(0) >= 100 {
            return Ok(());
        }
        if tokio::time::Instant::now() > deadline {
            return Err(std::io::Error::other(format!("tor bootstrap: {phase}")));
        }
        sleep(Duration::from_millis(400)).await;
    }
}

fn parse_socks(s: &str) -> std::io::Result<SocketAddr> {
    for part in s.split_whitespace() {
        let t = part.trim_matches('"');
        if let Ok(a) = t.parse() {
            return Ok(a);
        }
    }
    Err(std::io::Error::other(format!("socks raro: {s}")))
}

pub async fn dial_rendezvous(tor: &Tor) -> std::io::Result<TcpStream> {
    timeout(
        Duration::from_secs(25),
        tor.conectar(RENDEZVOUS_ONION, VIRT_PORT),
    )
    .await
    .map_err(|_| std::io::Error::other("rendezvous timeout"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsea_control_port() {
        assert_eq!(
            parse_control_port("PORT=127.0.0.1:45921\n").unwrap(),
            "127.0.0.1:45921".parse().unwrap()
        );
    }

    #[test]
    fn onion_rendezvous_parece_v3() {
        assert!(RENDEZVOUS_ONION.ends_with(".onion"));
        assert_eq!(RENDEZVOUS_ONION.len(), 56 + 6);
    }
}
