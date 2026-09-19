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
    ctl: Arc<tokio::sync::Mutex<Option<Control>>>,
}

#[derive(Clone)]
struct Snap {
    estado: EstadoTor,
    socks: Option<SocketAddr>,
    onion: Option<String>,
    hospeda: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EstadoTor {
    Ausente,
    Arrancando { paso: String },
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
                hospeda: false,
            })),
            ctl: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    pub fn marcar_arrancando(&self, paso: impl Into<String>) {
        self.snap.lock().unwrap().estado = EstadoTor::Arrancando { paso: paso.into() };
    }

    pub fn marcar_fallo(&self, s: impl Into<String>) {
        self.snap.lock().unwrap().estado = EstadoTor::Fallo(s.into());
    }

    pub fn marcar_listo(&self) {
        let mut g = self.snap.lock().unwrap();
        if let Some(onion) = g.onion.clone() {
            g.estado = EstadoTor::Listo { onion };
        }
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

    pub fn hospeda_sala(&self) -> bool {
        self.snap.lock().unwrap().hospeda
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

    /// Own Tor process and a personal onion. Does not host the swarm
    /// onion: that is a later election so two nodes do not talk to themselves.
    pub async fn subir(&self, local_port: u16) -> std::io::Result<()> {
        self.marcar_arrancando("buscando tor");
        let bin = tor_bin().ok_or_else(|| std::io::Error::other("no está el binario tor"))?;
        let dir = std::env::temp_dir().join(format!("konstruado-tor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir)?;
        let socks_port = puerto_libre()?;
        let ctrl_port = puerto_libre()?;
        let torrc = dir.join("torrc");
        let log = dir.join("notice.log");
        let stderr_log = dir.join("stderr.log");
        let cookie_path = dir.join("control_auth_cookie");
        std::fs::write(
            &torrc,
            format!(
                "SocksPort 127.0.0.1:{socks_port}\n\
                 ControlPort 127.0.0.1:{ctrl_port}\n\
                 CookieAuthentication 1\n\
                 DataDirectory {}\n\
                 Log notice file {}\n\
                 AvoidDiskWrites 1\n\
                 DormantCanceledByStartup 1\n\
                 __OwningControllerProcess {}\n",
                dir.display(),
                log.display(),
                std::process::id()
            ),
        )?;
        self.marcar_arrancando("lanzando tor");
        let mut child = tokio::process::Command::new(&bin)
            .arg("-f")
            .arg(&torrc)
            .arg("--defaults-torrc")
            .arg("/dev/null")
            .kill_on_drop(true)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::fs::File::create(&stderr_log)?)
            .spawn()?;

        self.marcar_arrancando("control");
        let ctrl_addr = std::net::SocketAddr::from(([127, 0, 0, 1], ctrl_port));
        wait_cookie(&mut child, &cookie_path, &log, &stderr_log).await?;
        let cookie = std::fs::read(&cookie_path)?;
        let mut ctl = Control::conectar(ctrl_addr, &cookie).await?;
        wait_bootstrap(&mut ctl, self).await?;
        let socks = std::net::SocketAddr::from(([127, 0, 0, 1], socks_port));
        self.marcar_arrancando("onion personal");
        let onion = ctl.add_onion("NEW", VIRT_PORT, local_port).await?;
        *self.ctl.lock().await = Some(ctl);
        {
            let mut g = self.snap.lock().unwrap();
            g.socks = Some(socks);
            g.onion = Some(onion.clone());
            g.estado = EstadoTor::Listo { onion };
        }
        tokio::spawn(async move {
            let _ = child.wait().await;
        });
        Ok(())
    }

    pub async fn hospedar_sala(&self, local_port: u16) -> std::io::Result<()> {
        let mut g = self.ctl.lock().await;
        let ctl = g.as_mut().ok_or_else(|| std::io::Error::other("sin control"))?;
        let onion = ctl.add_onion(RENDEZVOUS_KEY, VIRT_PORT, local_port).await?;
        if onion != RENDEZVOUS_ONION {
            return Err(std::io::Error::other(format!(
                "sala inesperada: {onion}"
            )));
        }
        self.snap.lock().unwrap().hospeda = true;
        Ok(())
    }

    pub async fn dejar_sala(&self) -> std::io::Result<()> {
        self.snap.lock().unwrap().hospeda = false;
        let mut g = self.ctl.lock().await;
        let ctl = g.as_mut().ok_or_else(|| std::io::Error::other("sin control"))?;
        ctl.del_onion(RENDEZVOUS_ONION).await
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

fn puerto_libre() -> std::io::Result<u16> {
    let l = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(l.local_addr()?.port())
}

fn cola_log(path: &PathBuf, n: usize) -> String {
    std::fs::read_to_string(path).map(|s| {
        s.lines()
            .rev()
            .take(n)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(" | ")
    }).unwrap_or_default()
}

async fn wait_cookie(
    child: &mut tokio::process::Child,
    path: &PathBuf,
    log: &PathBuf,
    stderr: &PathBuf,
) -> std::io::Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
    loop {
        if let Ok(Some(st)) = child.try_wait() {
            let msg = format!(
                "tor salió ({st}). {} {}",
                cola_log(stderr, 4),
                cola_log(log, 4)
            );
            return Err(std::io::Error::other(msg));
        }
        if let Ok(b) = std::fs::read(path) {
            if b.len() == 32 {
                return Ok(());
            }
        }
        if tokio::time::Instant::now() > deadline {
            return Err(std::io::Error::other(format!(
                "tor cookie ausente. {} {}",
                cola_log(stderr, 4),
                cola_log(log, 4)
            )));
        }
        sleep(Duration::from_millis(80)).await;
    }
}

async fn wait_bootstrap(ctl: &mut Control, tor: &Tor) -> std::io::Result<()> {
    let mut visto = 0u32;
    let mut cambio = tokio::time::Instant::now();
    let limite = tokio::time::Instant::now() + Duration::from_secs(15 * 60);
    loop {
        let phase = ctl.getinfo("status/bootstrap-phase").await.unwrap_or_default();
        let n = crate::ctl::parse_progress(&phase).unwrap_or(0);
        if n > visto {
            visto = n;
            cambio = tokio::time::Instant::now();
        }
        tor.marcar_arrancando(format!("bootstrap {n}%"));
        if n >= 100 {
            return Ok(());
        }
        if cambio.elapsed() > Duration::from_secs(5 * 60) {
            return Err(std::io::Error::other(format!(
                "bootstrap trabado en {n}%"
            )));
        }
        if tokio::time::Instant::now() > limite {
            return Err(std::io::Error::other(format!("bootstrap lento ({n}%)")));
        }
        sleep(Duration::from_millis(500)).await;
    }
}

pub async fn dial_rendezvous(tor: &Tor) -> std::io::Result<TcpStream> {
    timeout(
        Duration::from_secs(8),
        tor.conectar(RENDEZVOUS_ONION, VIRT_PORT),
    )
    .await
    .map_err(|_| std::io::Error::other("rendezvous timeout"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onion_rendezvous_parece_v3() {
        assert!(RENDEZVOUS_ONION.ends_with(".onion"));
        assert_eq!(RENDEZVOUS_ONION.len(), 56 + 6);
        assert_eq!(RENDEZVOUS_KEY.len(), 88);
    }
}
