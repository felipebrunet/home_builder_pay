use std::time::Duration;

use tokio::net::TcpStream;
use tokio::time::timeout;

/// Tor via local SOCKS (system `tor` or Tor Browser). Embedded Arti can
/// replace this later without changing the DHT.
#[derive(Clone, Debug)]
pub struct Tor {
    socks: Option<std::net::SocketAddr>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EstadoTor {
    Ausente,
    Listo { socks: String },
}

impl Tor {
    pub async fn detectar() -> Self {
        for port in [9050u16, 9150] {
            let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
            if timeout(Duration::from_millis(400), TcpStream::connect(addr))
                .await
                .ok()
                .and_then(Result::ok)
                .is_some()
            {
                return Self { socks: Some(addr) };
            }
        }
        Self { socks: None }
    }

    pub fn estado(&self) -> EstadoTor {
        match self.socks {
            Some(s) => EstadoTor::Listo {
                socks: s.to_string(),
            },
            None => EstadoTor::Ausente,
        }
    }

    pub fn disponible(&self) -> bool {
        self.socks.is_some()
    }

    pub async fn conectar(&self, host: &str, port: u16) -> std::io::Result<TcpStream> {
        if let Some(socks) = self.socks {
            let s = tokio_socks::tcp::Socks5Stream::connect(socks, (host, port))
                .await
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            Ok(s.into_inner())
        } else {
            TcpStream::connect((host, port)).await
        }
    }
}
