//! Precio USD/XMR de referencia para las obras en dólares.
//!
//! Primero CoinGecko (`simple/price`), si falla Kraken (ticker público XMRUSD).
//! Va por HTTPS y, si hay SOCKS (tor del escritorio u Orbot), por Tor: la API
//! no ve la IP. El XMR de stagenet no vale nada; se usa el precio de mainnet
//! como referencia y la interfaz lo dice. El último precio queda en memoria y
//! en `precio.json` (carpeta de datos) con su hora.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use konstruado_core::{precio_a_centavos, PrecioFijado};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Un precio leído de una API.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cotizacion {
    /// Centavos de dólar por 1 XMR.
    pub centavos_por_xmr: u64,
    /// `coingecko` o `kraken`.
    pub fuente: String,
    /// Unix, segundos.
    pub cuando: i64,
}

impl Cotizacion {
    pub fn edad_seg(&self, ahora: i64) -> i64 {
        (ahora - self.cuando).max(0)
    }

    /// Fija los dólares de una partida con este precio.
    pub fn fijar(&self, usd_centavos: u64) -> Option<PrecioFijado> {
        PrecioFijado::nuevo(usd_centavos, self.centavos_por_xmr, &self.fuente, self.cuando).ok()
    }
}

/// Cada cuánto se vuelve a pedir el precio en segundo plano.
pub const REFRESCO_SEG: i64 = 10 * 60;
/// Edad máxima para fijar una partida. Más viejo hay que actualizar primero.
pub const MAX_EDAD_FIJAR_SEG: i64 = 30 * 60;
/// Diferencia (en %) entre el precio fijado y el actual que se avisa al confirmar.
pub const AVISO_DIFERENCIA_PCT: u64 = 5;

const TIEMPO: Duration = Duration::from_secs(40);

pub struct Fuente {
    pub nombre: &'static str,
    pub host: &'static str,
    pub ruta: &'static str,
    pub leer: fn(&str) -> Option<f64>,
}

pub const FUENTES: [Fuente; 2] = [
    Fuente {
        nombre: "coingecko",
        host: "api.coingecko.com",
        ruta: "/api/v3/simple/price?ids=monero&vs_currencies=usd",
        leer: leer_coingecko,
    },
    Fuente {
        nombre: "kraken",
        host: "api.kraken.com",
        ruta: "/0/public/Ticker?pair=XMRUSD",
        leer: leer_kraken,
    },
];

/// `{"monero":{"usd":154.32}}`
pub fn leer_coingecko(json: &str) -> Option<f64> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    v.get("monero")?.get("usd")?.as_f64()
}

/// `{"error":[],"result":{"XXMRZUSD":{"c":["154.32","0.5"],...}}}`
pub fn leer_kraken(json: &str) -> Option<f64> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    if v.get("error").and_then(|e| e.as_array()).is_some_and(|e| !e.is_empty()) {
        return None;
    }
    let res = v.get("result")?.as_object()?;
    let par = res.values().next()?;
    par.get("c")?.get(0)?.as_str()?.parse().ok()
}

static CACHE: Mutex<Option<Cotizacion>> = Mutex::new(None);

fn archivo() -> std::path::PathBuf {
    crate::persist::dir().join("precio.json")
}

/// El último precio conocido (memoria o `precio.json`), de cualquier edad.
pub fn ultima() -> Option<Cotizacion> {
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if c.is_none() {
        *c = std::fs::read(archivo())
            .ok()
            .and_then(|b| serde_json::from_slice::<Cotizacion>(&b).ok())
            .filter(|q| q.centavos_por_xmr > 0);
    }
    c.clone()
}

/// El último precio si sirve para fijar una partida (no más viejo que [`MAX_EDAD_FIJAR_SEG`]).
pub fn para_fijar(ahora: i64) -> Option<Cotizacion> {
    ultima().filter(|q| q.edad_seg(ahora) <= MAX_EDAD_FIJAR_SEG)
}

/// Hace falta pedir de nuevo (no hay o tiene más de [`REFRESCO_SEG`]).
pub fn hace_falta(ahora: i64) -> bool {
    ultima().is_none_or(|q| q.edad_seg(ahora) >= REFRESCO_SEG)
}

fn guardar(q: &Cotizacion) {
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some(q.clone());
    if let Ok(b) = serde_json::to_vec(q) {
        let _ = std::fs::write(archivo(), b);
    }
}

static ULTIMO_ERROR: Mutex<Option<String>> = Mutex::new(None);

/// Por qué falló la última actualización (se borra al tener precio).
pub fn ultimo_error() -> Option<String> {
    ULTIMO_ERROR.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Pide el precio: CoinGecko y, si falla, Kraken. Por Tor si hay `socks`.
pub async fn actualizar(socks: Option<SocketAddr>) -> Result<Cotizacion, String> {
    let r = obtener_con(|f: &'static Fuente| async move { https_get(socks, f.host, f.ruta).await }).await;
    match &r {
        Ok(q) => {
            guardar(q);
            *ULTIMO_ERROR.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
        Err(e) => *ULTIMO_ERROR.lock().unwrap_or_else(|e| e.into_inner()) = Some(e.clone()),
    }
    r
}

static EN_CURSO: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static ULTIMO_INTENTO: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
/// Tras una falla, cuánto esperar antes de reintentar solo.
const REINTENTO_SEG: i64 = 60;

/// Para el bucle de la interfaz (cada segundo): si hace falta precio y no hay
/// un pedido en curso, lanza uno en segundo plano (runtime de tokio actual).
pub fn refrescar_en_fondo(socks: Option<SocketAddr>) {
    use std::sync::atomic::Ordering;
    let ahora = konstruado_core::ahora();
    if !hace_falta(ahora) || ahora - ULTIMO_INTENTO.load(Ordering::Relaxed) < REINTENTO_SEG {
        return;
    }
    if EN_CURSO.swap(true, Ordering::AcqRel) {
        return;
    }
    ULTIMO_INTENTO.store(ahora, Ordering::Relaxed);
    let Ok(rt) = tokio::runtime::Handle::try_current() else {
        EN_CURSO.store(false, Ordering::Release);
        return;
    };
    rt.spawn(async move {
        let _ = actualizar(socks).await;
        EN_CURSO.store(false, Ordering::Release);
    });
}

/// Recorre [`FUENTES`] en orden con el `get` dado (en los tests, uno falso).
pub async fn obtener_con<F, Fut>(get: F) -> Result<Cotizacion, String>
where
    F: Fn(&'static Fuente) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    let mut fallas = Vec::new();
    for f in FUENTES.iter() {
        match get(f).await {
            Ok(body) => match (f.leer)(&body).and_then(precio_a_centavos) {
                Some(c) => {
                    return Ok(Cotizacion {
                        centavos_por_xmr: c,
                        fuente: f.nombre.to_string(),
                        cuando: konstruado_core::ahora(),
                    })
                }
                None => fallas.push(format!("{}: respuesta sin precio", f.nombre)),
            },
            Err(e) => fallas.push(format!("{}: {e}", f.nombre)),
        }
    }
    Err(fallas.join("; "))
}

trait Flujo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Flujo for T {}

async fn https_get(socks: Option<SocketAddr>, host: &str, ruta: &str) -> Result<String, String> {
    tokio::time::timeout(TIEMPO, https_get_sin_tiempo(socks, host, ruta))
        .await
        .map_err(|_| "tiempo agotado".to_string())?
}

async fn https_get_sin_tiempo(socks: Option<SocketAddr>, host: &str, ruta: &str) -> Result<String, String> {
    let tcp: Box<dyn Flujo> = match socks {
        // Por Tor: el nombre lo resuelve el SOCKS (sin DNS local).
        Some(proxy) => Box::new(
            tokio_socks::tcp::Socks5Stream::connect(proxy, (host, 443))
                .await
                .map_err(|e| format!("SOCKS: {e}"))?,
        ),
        None => Box::new(
            tokio::net::TcpStream::connect((host, 443))
                .await
                .map_err(|e| e.to_string())?,
        ),
    };
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let nombre = rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|e| e.to_string())?;
    let mut tls = tokio_rustls::TlsConnector::from(Arc::new(cfg))
        .connect(nombre, tcp)
        .await
        .map_err(|e| format!("TLS: {e}"))?;
    let pedido = format!(
        "GET {ruta} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: konstruado\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    tls.write_all(pedido.as_bytes()).await.map_err(|e| e.to_string())?;
    let mut crudo = Vec::new();
    // Algunos servidores cierran sin close_notify: lo leído hasta ahí sirve.
    if let Err(e) = tls.read_to_end(&mut crudo).await {
        if crudo.is_empty() {
            return Err(e.to_string());
        }
    }
    cuerpo_http(&crudo)
}

/// Separa el cuerpo de una respuesta HTTP/1.1 (con o sin `chunked`).
pub fn cuerpo_http(crudo: &[u8]) -> Result<String, String> {
    let fin = crudo
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("respuesta HTTP incompleta")?;
    let cab = String::from_utf8_lossy(&crudo[..fin]).to_ascii_lowercase();
    let estado = cab.lines().next().unwrap_or_default();
    let codigo = estado.split_whitespace().nth(1).unwrap_or_default();
    if codigo != "200" {
        return Err(format!("HTTP {codigo}"));
    }
    let resto = &crudo[fin + 4..];
    let cuerpo = if cab.contains("transfer-encoding: chunked") {
        let mut out = Vec::new();
        let mut i = 0;
        loop {
            let nl = resto[i..]
                .windows(2)
                .position(|w| w == b"\r\n")
                .ok_or("chunk incompleto")?;
            let tam_txt = String::from_utf8_lossy(&resto[i..i + nl]);
            let tam = usize::from_str_radix(tam_txt.split(';').next().unwrap_or("").trim(), 16)
                .map_err(|_| "chunk ilegible")?;
            i += nl + 2;
            if tam == 0 {
                break;
            }
            let fin_chunk = i.checked_add(tam).filter(|&f| f <= resto.len()).ok_or("chunk cortado")?;
            out.extend_from_slice(&resto[i..fin_chunk]);
            i = fin_chunk + 2;
            if i > resto.len() {
                break;
            }
        }
        out
    } else {
        resto.to_vec()
    };
    String::from_utf8(cuerpo).map_err(|_| "cuerpo no es texto".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_las_dos_apis() {
        assert_eq!(leer_coingecko(r#"{"monero":{"usd":154.32}}"#), Some(154.32));
        assert_eq!(leer_coingecko(r#"{"status":{"error_code":429}}"#), None);
        let k = r#"{"error":[],"result":{"XXMRZUSD":{"a":["154.4","1","1.0"],"c":["154.35000000","0.5"]}}}"#;
        assert_eq!(leer_kraken(k), Some(154.35));
        assert_eq!(leer_kraken(r#"{"error":["EQuery:Unknown asset pair"]}"#), None);
    }

    #[test]
    fn cuerpo_http_normal_y_chunked() {
        let r = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"a\":1}";
        assert_eq!(cuerpo_http(r).unwrap(), "{\"a\":1}");
        let c = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"a\"\r\n3\r\n:1}\r\n0\r\n\r\n";
        assert_eq!(cuerpo_http(c).unwrap(), "{\"a\":1}");
        let e = b"HTTP/1.1 429 Too Many Requests\r\n\r\n";
        assert_eq!(cuerpo_http(e), Err("HTTP 429".into()));
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().build().unwrap()
    }

    #[test]
    fn si_coingecko_falla_usa_kraken() {
        let q = rt()
            .block_on(obtener_con(|f: &'static Fuente| async move {
                match f.nombre {
                    "coingecko" => Err("HTTP 429".to_string()),
                    _ => Ok(r#"{"error":[],"result":{"XXMRZUSD":{"c":["160.004","1"]}}}"#.to_string()),
                }
            }))
            .unwrap();
        assert_eq!(q.fuente, "kraken");
        assert_eq!(q.centavos_por_xmr, 16_000);
    }

    #[test]
    fn coingecko_primero() {
        let q = rt()
            .block_on(obtener_con(|f: &'static Fuente| async move {
                match f.nombre {
                    "coingecko" => Ok(r#"{"monero":{"usd":150.5}}"#.to_string()),
                    _ => panic!("no hace falta Kraken"),
                }
            }))
            .unwrap();
        assert_eq!((q.fuente.as_str(), q.centavos_por_xmr), ("coingecko", 15_050));
    }

    #[test]
    fn sin_ninguna_da_error_claro() {
        let e = rt()
            .block_on(obtener_con(|f: &'static Fuente| async move {
                match f.nombre {
                    "coingecko" => Err("SOCKS: connection refused".to_string()),
                    _ => Ok("{}".to_string()),
                }
            }))
            .unwrap_err();
        assert!(e.contains("coingecko: SOCKS"), "{e}");
        assert!(e.contains("kraken: respuesta sin precio"), "{e}");
    }

    #[test]
    fn fijar_con_la_cotizacion() {
        let q = Cotizacion { centavos_por_xmr: 16_000, fuente: "coingecko".into(), cuando: 100 };
        let p = q.fijar(5_000).unwrap();
        assert_eq!(p.piconero, 312_500_000_000);
        assert_eq!((p.fuente.as_str(), p.cuando), ("coingecko", 100));
        assert_eq!(q.edad_seg(160), 60);
    }
}

#[cfg(test)]
mod en_vivo {
    /// `cargo test -p konstruado-motor precio_en_vivo -- --ignored --nocapture`
    /// (con `KONSTRUADO_SOCKS=127.0.0.1:9050` va por Tor).
    #[test]
    #[ignore]
    fn precio_en_vivo() {
        let socks = std::env::var("KONSTRUADO_SOCKS").ok().and_then(|s| s.parse().ok());
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        for f in super::FUENTES.iter() {
            let r = rt.block_on(super::https_get(socks, f.host, f.ruta));
            println!("{}: {:?}", f.nombre, r.as_ref().map(|b| (f.leer)(b)));
        }
    }
}
