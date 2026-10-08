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
    diag: DiagSocks,
}

/// Qué se sabe del camino SOCKS → sala, medido en cada intento real.
///
/// Separa "Orbot no contesta" de "Orbot contesta pero la sala no": con el
/// SOCKS vivo no hay que pedir que enciendan Orbot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiagSocks {
    /// No hay SOCKS configurado (Orbot apagado en la app, o escritorio con tor propio).
    SinSocks,
    /// Todavía no hubo intento.
    SinProbar,
    /// No se pudo abrir TCP al SOCKS, o no habla SOCKS5. Lleva el error corto.
    SocksCaido(String),
    /// El SOCKS aceptó; esperando la respuesta del onion.
    SocksOk,
    /// El SOCKS respondió, pero el onion de destino no (host apagado, onion sin publicar).
    DestinoNoResponde(String),
    /// Hubo sesión con el destino.
    Conectado,
}

/// Error corto y humano de tokio-socks / io.
fn corto_socks(e: &tokio_socks::Error) -> String {
    use tokio_socks::Error as E;
    match e {
        E::HostUnreachable | E::TtlExpired => "onion sin respuesta".into(),
        E::GeneralSocksServerFailure => "falla general de Tor".into(),
        E::NetworkUnreachable => "red inalcanzable".into(),
        E::ConnectionRefused => "conexión rechazada".into(),
        E::Io(io) => io.kind().to_string(),
        otro => otro.to_string(),
    }
}

/// `true` si el error vino después de que el SOCKS respondió (problema del destino).
fn es_del_destino(e: &tokio_socks::Error) -> bool {
    use tokio_socks::Error as E;
    matches!(
        e,
        E::HostUnreachable
            | E::TtlExpired
            | E::GeneralSocksServerFailure
            | E::NetworkUnreachable
            | E::ConnectionRefused
            | E::ConnectionNotAllowedByRuleset
    )
}

/// Saludo SOCKS5 mínimo (sin autenticación) a `socks`. `Ok` si contesta `05 00`.
///
/// No abre ninguna conexión a través del proxy: solo prueba que hay un SOCKS5
/// escuchando. Sirve para saber si Orbot está andando sin depender de la sala.
pub async fn probar_socks(socks: SocketAddr, espera: Duration) -> Result<(), String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = match timeout(espera, TcpStream::connect(socks)).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Err(e.kind().to_string()),
        Err(_) => return Err("sin respuesta".into()),
    };
    let r = timeout(espera, async {
        s.write_all(&[5, 1, 0]).await?;
        let mut b = [0u8; 2];
        s.read_exact(&mut b).await?;
        Ok::<_, std::io::Error>(b)
    })
    .await;
    match r {
        Ok(Ok([5, 0])) => Ok(()),
        Ok(Ok(_)) => Err("no habla SOCKS5".into()),
        Ok(Err(e)) => Err(e.kind().to_string()),
        Err(_) => Err("sin respuesta".into()),
    }
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
                diag: DiagSocks::SinSocks,
            })),
            ctl: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// Use an external SOCKS proxy (Orbot on Android, system tor, etc.).
    /// Does not spawn a `tor` process and does not open a control port,
    /// so ADD_ONION / hospedar_sala is unavailable on this path.
    pub fn socks_externo(host: impl Into<String>, port: u16) -> Self {
        let host = host.into();
        let socks: SocketAddr = format!("{host}:{port}")
            .parse()
            .unwrap_or_else(|_| SocketAddr::from(([127, 0, 0, 1], port)));
        Self {
            snap: Arc::new(Mutex::new(Snap {
                estado: EstadoTor::Listo {
                    onion: "(socks externo / Orbot)".into(),
                },
                socks: Some(socks),
                onion: Some("(socks externo / Orbot)".into()),
                hospeda: false,
                diag: DiagSocks::SinProbar,
            })),
            ctl: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    /// Reconfigure SOCKS without spawning tor. Clears any prior control handle.
    pub fn configurar_socks(&self, host: &str, port: u16) {
        let socks: SocketAddr = format!("{host}:{port}")
            .parse()
            .unwrap_or_else(|_| SocketAddr::from(([127, 0, 0, 1], port)));
        {
            let mut g = self.snap.lock().unwrap();
            g.socks = Some(socks);
            g.onion = Some("(socks externo / Orbot)".into());
            g.hospeda = false;
            g.diag = DiagSocks::SinProbar;
            g.estado = EstadoTor::Listo {
                onion: "(socks externo / Orbot)".into(),
            };
        }
        // Drop control so we never try ADD_ONION without a real controller.
        if let Ok(mut g) = self.ctl.try_lock() {
            *g = None;
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

    pub fn diag(&self) -> DiagSocks {
        self.snap.lock().unwrap().diag.clone()
    }

    pub fn fijar_diag(&self, d: DiagSocks) {
        self.snap.lock().unwrap().diag = d;
    }

    /// Sin SOCKS (p. ej. el usuario apagó Orbot en la app).
    pub fn quitar_socks(&self) {
        let mut g = self.snap.lock().unwrap();
        g.socks = None;
        g.diag = DiagSocks::SinSocks;
        g.estado = EstadoTor::Ausente;
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

    /// Conecta a `host:port` por el SOCKS si hay. Deja el diagnóstico en `diag()`:
    /// TCP al SOCKS caído, SOCKS vivo esperando al onion, o el onion sin respuesta.
    pub async fn conectar(&self, host: &str, port: u16) -> std::io::Result<TcpStream> {
        if let Some(socks) = self.socks() {
            let dest = format!("{host}:{port}");
            let tcp = match timeout(Duration::from_secs(8), TcpStream::connect(socks)).await {
                Ok(Ok(t)) => t,
                Ok(Err(e)) => {
                    self.fijar_diag(DiagSocks::SocksCaido(e.kind().to_string()));
                    return Err(e);
                }
                Err(_) => {
                    self.fijar_diag(DiagSocks::SocksCaido("sin respuesta".into()));
                    return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "socks sin respuesta"));
                }
            };
            // El proxy aceptó TCP: si después falla, es el destino (o no es SOCKS5).
            // Sin pisar "la sala no responde" ni "conectado" en cada reintento:
            // así la pantalla no parpadea entre estados cada 4 s.
            if matches!(self.diag(), DiagSocks::SinProbar | DiagSocks::SocksCaido(_)) {
                self.fijar_diag(DiagSocks::SocksOk);
            }
            match tokio_socks::tcp::Socks5Stream::connect_with_socket(tcp, dest.as_str()).await {
                Ok(s) => Ok(s.into_inner()),
                Err(e) => {
                    let corto = corto_socks(&e);
                    if es_del_destino(&e) {
                        self.fijar_diag(DiagSocks::DestinoNoResponde(corto));
                    } else {
                        self.fijar_diag(DiagSocks::SocksCaido(corto));
                    }
                    Err(std::io::Error::other(e.to_string()))
                }
            }
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
        Duration::from_secs(45),
        tor.conectar(RENDEZVOUS_ONION, VIRT_PORT),
    )
    .await
    .map_err(|_| std::io::Error::other("timeout"))?
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

    #[test]
    fn diag_arranca_segun_socks() {
        assert_eq!(Tor::ausente().diag(), DiagSocks::SinSocks);
        let t = Tor::socks_externo("127.0.0.1", 9050);
        assert_eq!(t.diag(), DiagSocks::SinProbar);
        t.quitar_socks();
        assert_eq!(t.diag(), DiagSocks::SinSocks);
        assert!(t.socks().is_none());
    }

    #[tokio::test]
    async fn socks_cerrado_se_ve_como_socks_caido() {
        // Puerto libre sin nadie escuchando: Orbot "apagado".
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        let t = Tor::socks_externo("127.0.0.1", port);
        assert!(t.conectar("abc.onion", 1).await.is_err());
        assert!(matches!(t.diag(), DiagSocks::SocksCaido(_)), "{:?}", t.diag());
        let addr = t.socks().unwrap();
        assert!(probar_socks(addr, Duration::from_secs(1)).await.is_err());
    }

    /// SOCKS5 de juguete: saluda bien y responde `codigo` al CONNECT.
    async fn socks_falso(codigo: u8) -> SocketAddr {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else { return };
                tokio::spawn(async move {
                    let mut b = [0u8; 3];
                    if s.read_exact(&mut b).await.is_err() {
                        return;
                    }
                    let _ = s.write_all(&[5, 0]).await;
                    // CONNECT: VER CMD RSV ATYP(3) LEN host PORT
                    let mut h = [0u8; 5];
                    if s.read_exact(&mut h).await.is_err() {
                        return;
                    }
                    let mut resto = vec![0u8; h[4] as usize + 2];
                    let _ = s.read_exact(&mut resto).await;
                    let _ = s.write_all(&[5, codigo, 0, 1, 0, 0, 0, 0, 0, 0]).await;
                });
            }
        });
        addr
    }

    #[tokio::test]
    async fn socks_vivo_pero_onion_caido_no_es_culpa_de_orbot() {
        let addr = socks_falso(4).await; // 04 = host unreachable
        assert!(probar_socks(addr, Duration::from_secs(1)).await.is_ok());
        let t = Tor::socks_externo("127.0.0.1", addr.port());
        assert!(t.conectar("abc.onion", 17432).await.is_err());
        assert_eq!(t.diag(), DiagSocks::DestinoNoResponde("onion sin respuesta".into()));
    }

    #[tokio::test]
    async fn socks_vivo_y_onion_vivo_conecta() {
        let addr = socks_falso(0).await;
        let t = Tor::socks_externo("127.0.0.1", addr.port());
        assert!(t.conectar("abc.onion", 17432).await.is_ok());
        assert_eq!(t.diag(), DiagSocks::SocksOk);
    }

    #[test]
    fn socks_externo_queda_listo() {
        let t = Tor::socks_externo("127.0.0.1", 9050);
        assert_eq!(t.socks().unwrap().port(), 9050);
        assert!(matches!(t.estado(), EstadoTor::Listo { .. }));
        assert!(!t.hospeda_sala());
    }
}
