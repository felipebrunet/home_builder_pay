//! Precio USD/XMR de referencia para las obras en dólares.
//!
//! Se pide a varias fuentes **a la vez** (Kraken, Bitfinex, CoinGecko,
//! CoinPaprika) y vale la primera respuesta con precio; cada una tiene
//! [`PLAZO_FUENTE`] para contestar. Va por HTTPS y, si hay SOCKS (tor del
//! escritorio u Orbot), por Tor: la API no ve la IP.
//!
//! Si por Tor no responde ninguna, **no** se sale sin Tor por cuenta propia:
//! la interfaz ofrece reintentar sin Tor (avisando que la API ve la IP) o
//! dejarlo permitido siempre ([`directo_siempre`]). Como último recurso se puede
//! escribir el precio a mano ([`fijar_manual`]). Sin SOCKS configurado (Orbot
//! apagado, tor del escritorio ausente) la app entera ya va directo y el precio
//! también.
//!
//! El XMR de stagenet no vale nada; se usa el precio de mainnet como referencia
//! y la interfaz lo dice. El último precio queda en memoria y en `precio.json`
//! (carpeta de datos) con su hora.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::stream::{FuturesUnordered, StreamExt};
use konstruado_core::{precio_a_centavos, PrecioFijado};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Un precio leído de una API (o escrito a mano).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cotizacion {
    /// Centavos de dólar por 1 XMR.
    pub centavos_por_xmr: u64,
    /// `kraken`, `bitfinex`, `coingecko`, `coinpaprika` o `manual`.
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
/// Lo que tiene cada fuente para contestar (todas van en paralelo).
pub const PLAZO_FUENTE: Duration = Duration::from_secs(8);
/// Nombre de la fuente de un precio escrito a mano.
pub const FUENTE_MANUAL: &str = "manual";

pub struct Fuente {
    pub nombre: &'static str,
    pub host: &'static str,
    pub ruta: &'static str,
    pub leer: fn(&str) -> Option<f64>,
}

/// Todas cotizan XMR contra dólares (no USDT). Coinbase no lista XMR.
pub const FUENTES: [Fuente; 4] = [
    Fuente {
        nombre: "kraken",
        host: "api.kraken.com",
        ruta: "/0/public/Ticker?pair=XMRUSD",
        leer: leer_kraken,
    },
    Fuente {
        nombre: "bitfinex",
        host: "api-pub.bitfinex.com",
        ruta: "/v2/ticker/tXMRUSD",
        leer: leer_bitfinex,
    },
    Fuente {
        nombre: "coingecko",
        host: "api.coingecko.com",
        ruta: "/api/v3/simple/price?ids=monero&vs_currencies=usd",
        leer: leer_coingecko,
    },
    Fuente {
        nombre: "coinpaprika",
        host: "api.coinpaprika.com",
        ruta: "/v1/tickers/xmr-monero?quotes=USD",
        leer: leer_coinpaprika,
    },
];

/// Nombre de una fuente para mostrar.
pub fn nombre_fuente(f: &str, es: bool) -> &str {
    match f {
        "coingecko" => "CoinGecko",
        "kraken" => "Kraken",
        "bitfinex" => "Bitfinex",
        "coinpaprika" => "CoinPaprika",
        FUENTE_MANUAL if es => "precio a mano",
        FUENTE_MANUAL => "manual price",
        otro => otro,
    }
}

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

/// `[BID, BID_SIZE, ASK, ASK_SIZE, DAILY_CHANGE, DAILY_CHANGE_REL, LAST_PRICE, VOLUME, HIGH, LOW]`.
/// Los errores vienen como `["error", 10020, "symbol: invalid"]`.
pub fn leer_bitfinex(json: &str) -> Option<f64> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let a = v.as_array()?;
    if a.len() < 7 {
        return None;
    }
    a[6].as_f64()
}

/// `{"id":"xmr-monero",...,"quotes":{"USD":{"price":540.25,...}}}`
pub fn leer_coinpaprika(json: &str) -> Option<f64> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    if v.get("id")?.as_str()? != "xmr-monero" {
        return None;
    }
    v.get("quotes")?.get("USD")?.get("price")?.as_f64()
}

/// Por qué no contestó una fuente.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Motivo {
    /// No contestó dentro de [`PLAZO_FUENTE`].
    Tiempo,
    /// El SOCKS (Orbot / tor) no acepta conexiones.
    SocksCaido,
    /// El SOCKS respondió, pero Tor no llegó al servidor.
    Socks(String),
    /// Conexión directa fallida (DNS, sin red...).
    Red(String),
    Tls(String),
    /// `HTTP 429`, `HTTP 403`...
    Http(String),
    /// Respondió, pero sin un precio válido.
    SinPrecio,
}

impl Motivo {
    pub fn texto(&self, es: bool) -> String {
        let segs = PLAZO_FUENTE.as_secs();
        match (self, es) {
            (Motivo::Tiempo, true) => format!("no respondió en {segs} s"),
            (Motivo::Tiempo, false) => format!("no answer within {segs} s"),
            (Motivo::SocksCaido, true) => "el SOCKS de Tor/Orbot no responde".into(),
            (Motivo::SocksCaido, false) => "the Tor/Orbot SOCKS is not answering".into(),
            (Motivo::Socks(e), true) => format!("Tor no llegó al servidor ({e})"),
            (Motivo::Socks(e), false) => format!("Tor could not reach the server ({e})"),
            (Motivo::Red(e), true) => format!("sin conexión ({e})"),
            (Motivo::Red(e), false) => format!("no connection ({e})"),
            (Motivo::Tls(e), true) => format!("falló TLS ({e})"),
            (Motivo::Tls(e), false) => format!("TLS failed ({e})"),
            (Motivo::Http(c), true) if bloqueo_tipico(c) => format!("{c}, suele bloquear a Tor"),
            (Motivo::Http(c), false) if bloqueo_tipico(c) => format!("{c}, it often blocks Tor"),
            (Motivo::Http(c), _) => c.clone(),
            (Motivo::SinPrecio, true) => "respondió sin precio".into(),
            (Motivo::SinPrecio, false) => "answered without a price".into(),
        }
    }
}

fn bloqueo_tipico(c: &str) -> bool {
    matches!(c, "HTTP 403" | "HTTP 429" | "HTTP 503")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallaFuente {
    pub fuente: String,
    pub motivo: Motivo,
}

/// Por qué no hay precio: lo que dijo cada fuente, por Tor y/o sin Tor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Falla {
    /// Intento por Tor (si hubo).
    pub tor: Option<Vec<FallaFuente>>,
    /// Intento sin Tor (si hubo).
    pub directo: Option<Vec<FallaFuente>>,
    pub cuando: i64,
}

fn lista(es: bool, f: &[FallaFuente]) -> String {
    if !f.is_empty() && f.iter().all(|x| x.motivo == Motivo::SocksCaido) {
        return if es {
            "el SOCKS de Tor/Orbot no responde (¿está abierto Orbot?)".into()
        } else {
            "the Tor/Orbot SOCKS is not answering (is Orbot open?)".into()
        };
    }
    f.iter()
        .map(|x| format!("{}: {}", nombre_fuente(&x.fuente, es), x.motivo.texto(es)))
        .collect::<Vec<_>>()
        .join("; ")
}

impl Falla {
    /// `Por Tor: Kraken: no respondió en 8 s; CoinGecko: HTTP 429, suele bloquear a Tor; ...`
    pub fn texto(&self, es: bool) -> String {
        let mut partes = Vec::new();
        if let Some(t) = &self.tor {
            partes.push(format!("{} {}", if es { "Por Tor:" } else { "Over Tor:" }, lista(es, t)));
        }
        if let Some(d) = &self.directo {
            partes.push(format!("{} {}", if es { "Sin Tor:" } else { "Without Tor:" }, lista(es, d)));
        }
        partes.join(" · ")
    }

    /// Falló por Tor y todavía no se probó sin Tor.
    pub fn solo_tor(&self) -> bool {
        self.tor.is_some() && self.directo.is_none()
    }
}

static CACHE: Mutex<Option<Cotizacion>> = Mutex::new(None);

fn archivo() -> std::path::PathBuf {
    crate::persist::dir().join("precio.json")
}

fn archivo_ajustes() -> std::path::PathBuf {
    crate::persist::dir().join("precio-ajustes.json")
}

#[derive(Default, Serialize, Deserialize)]
struct Ajustes {
    #[serde(default)]
    directo_siempre: bool,
}

/// El usuario permitió pedir el precio sin Tor cuando por Tor no hay (por defecto no).
pub fn directo_siempre() -> bool {
    std::fs::read(archivo_ajustes())
        .ok()
        .and_then(|b| serde_json::from_slice::<Ajustes>(&b).ok())
        .unwrap_or_default()
        .directo_siempre
}

pub fn fijar_directo_siempre(si: bool) {
    if let Ok(b) = serde_json::to_vec(&Ajustes { directo_siempre: si }) {
        let _ = std::fs::create_dir_all(crate::persist::dir());
        let _ = std::fs::write(archivo_ajustes(), b);
    }
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
        // La carpeta puede no existir todavía (escritorio antes de crear la cuenta).
        let _ = std::fs::create_dir_all(crate::persist::dir());
        let _ = std::fs::write(archivo(), b);
    }
}

static ULTIMA_FALLA: Mutex<Option<Falla>> = Mutex::new(None);

/// Por qué falló la última actualización (se borra al tener precio).
pub fn ultima_falla() -> Option<Falla> {
    ULTIMA_FALLA.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// [`ultima_falla`] como texto (en español; sirve para detectar cambios).
pub fn ultimo_error() -> Option<String> {
    ultima_falla().map(|f| f.texto(true))
}

/// La interfaz debe ofrecer "probar sin Tor": por Tor no hubo precio, no se
/// probó sin Tor y no está permitido siempre.
pub fn ofrecer_directo() -> bool {
    ultima_falla().is_some_and(|f| f.solo_tor()) && !directo_siempre()
}

fn registrar(r: &Result<Cotizacion, Falla>) {
    let mut g = ULTIMA_FALLA.lock().unwrap_or_else(|e| e.into_inner());
    match r {
        Ok(q) => {
            guardar(q);
            *g = None;
        }
        Err(f) => *g = Some(f.clone()),
    }
}

async fn pedir(socks: Option<SocketAddr>) -> Result<Cotizacion, Vec<FallaFuente>> {
    obtener_con(|f: &'static Fuente| async move { https_get(socks, f.host, f.ruta).await }, PLAZO_FUENTE).await
}

/// Pide el precio. Con `socks`, por Tor; si por Tor no hay y el usuario lo
/// permitió siempre, reintenta sin Tor. Sin `socks`, directo (la app ya va sin Tor).
pub async fn actualizar(socks: Option<SocketAddr>) -> Result<Cotizacion, Falla> {
    let r = resolver(socks, directo_siempre(), pedir).await;
    registrar(&r);
    r
}

/// Pide el precio sin Tor, porque el usuario lo pidió (la API ve su IP).
pub async fn actualizar_directo() -> Result<Cotizacion, Falla> {
    let tor = ultima_falla().and_then(|f| f.tor);
    let r = pedir(None).await.map_err(|d| Falla { tor, directo: Some(d), cuando: konstruado_core::ahora() });
    registrar(&r);
    r
}

/// La lógica de [`actualizar`] con el pedido dado (en los tests, uno falso).
pub async fn resolver<P, Fut>(socks: Option<SocketAddr>, directo_permitido: bool, pedir: P) -> Result<Cotizacion, Falla>
where
    P: Fn(Option<SocketAddr>) -> Fut,
    Fut: std::future::Future<Output = Result<Cotizacion, Vec<FallaFuente>>>,
{
    let cuando = konstruado_core::ahora;
    match socks {
        None => pedir(None).await.map_err(|d| Falla { tor: None, directo: Some(d), cuando: cuando() }),
        Some(s) => match pedir(Some(s)).await {
            Ok(q) => Ok(q),
            Err(t) if directo_permitido => pedir(None)
                .await
                .map_err(|d| Falla { tor: Some(t), directo: Some(d), cuando: cuando() }),
            Err(t) => Err(Falla { tor: Some(t), directo: None, cuando: cuando() }),
        },
    }
}

/// Precio escrito a mano (`540`, `540,25`, `USD 540.25`): queda como el
/// último precio, con fuente `manual`, y vence igual que los demás.
pub fn fijar_manual(texto: &str) -> Option<Cotizacion> {
    let c = konstruado_core::leer_usd(texto).ok()?;
    if !(100..=1_000_000_000).contains(&c) {
        return None;
    }
    let q = Cotizacion { centavos_por_xmr: c, fuente: FUENTE_MANUAL.into(), cuando: konstruado_core::ahora() };
    registrar(&Ok(q.clone()));
    Some(q)
}

static EN_CURSO: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static ULTIMO_INTENTO: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
/// Tras una falla, cuánto esperar antes de reintentar solo.
const REINTENTO_SEG: i64 = 30;

/// Para el bucle de fondo (cada pocos segundos): si hace falta precio y no hay
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

/// Pide a todas las [`FUENTES`] a la vez con el `get` dado (en los tests, uno
/// falso). Vale la primera con precio; las demás se cancelan. Si ninguna
/// sirve, devuelve qué dijo cada una (en el orden de [`FUENTES`]).
pub async fn obtener_con<F, Fut>(get: F, plazo: Duration) -> Result<Cotizacion, Vec<FallaFuente>>
where
    F: Fn(&'static Fuente) -> Fut,
    Fut: std::future::Future<Output = Result<String, Motivo>>,
{
    let mut pendientes: FuturesUnordered<_> = FUENTES
        .iter()
        .map(|f| {
            let fut = get(f);
            async move { (f, tokio::time::timeout(plazo, fut).await) }
        })
        .collect();
    let mut fallas = Vec::new();
    while let Some((f, r)) = pendientes.next().await {
        let motivo = match r {
            Err(_) => Motivo::Tiempo,
            Ok(Err(m)) => m,
            Ok(Ok(body)) => match (f.leer)(&body).and_then(precio_a_centavos) {
                Some(c) => {
                    return Ok(Cotizacion {
                        centavos_por_xmr: c,
                        fuente: f.nombre.to_string(),
                        cuando: konstruado_core::ahora(),
                    })
                }
                None => Motivo::SinPrecio,
            },
        };
        fallas.push(FallaFuente { fuente: f.nombre.to_string(), motivo });
    }
    fallas.sort_by_key(|x| FUENTES.iter().position(|f| f.nombre == x.fuente));
    Err(fallas)
}

trait Flujo: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Flujo for T {}

/// GET por HTTPS. El plazo lo pone quien llama ([`obtener_con`]).
async fn https_get(socks: Option<SocketAddr>, host: &str, ruta: &str) -> Result<String, Motivo> {
    let tcp: Box<dyn Flujo> = match socks {
        // Por Tor: el nombre lo resuelve el SOCKS (como socks5h, sin DNS local).
        Some(proxy) => Box::new(
            tokio_socks::tcp::Socks5Stream::connect(proxy, (host, 443))
                .await
                .map_err(motivo_socks)?,
        ),
        None => Box::new(
            tokio::net::TcpStream::connect((host, 443))
                .await
                .map_err(|e| Motivo::Red(e.to_string()))?,
        ),
    };
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|e| Motivo::Tls(e.to_string()))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    let nombre = rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|e| Motivo::Tls(e.to_string()))?;
    let mut tls = tokio_rustls::TlsConnector::from(Arc::new(cfg))
        .connect(nombre, tcp)
        .await
        .map_err(|e| Motivo::Tls(e.to_string()))?;
    let pedido = format!(
        "GET {ruta} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: konstruado\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    tls.write_all(pedido.as_bytes()).await.map_err(|e| Motivo::Red(e.to_string()))?;
    let mut crudo = Vec::new();
    // Algunos servidores cierran sin close_notify: lo leído hasta ahí sirve.
    if let Err(e) = tls.read_to_end(&mut crudo).await {
        if crudo.is_empty() {
            return Err(Motivo::Red(e.to_string()));
        }
    }
    cuerpo_http(&crudo).map_err(|e| if e.starts_with("HTTP ") { Motivo::Http(e) } else { Motivo::Red(e) })
}

/// El SOCKS caído (Orbot cerrado) se distingue de "Tor no llegó".
fn motivo_socks(e: tokio_socks::Error) -> Motivo {
    match &e {
        tokio_socks::Error::ProxyServerUnreachable => Motivo::SocksCaido,
        tokio_socks::Error::Io(io)
            if matches!(
                io.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
            ) =>
        {
            Motivo::SocksCaido
        }
        _ => Motivo::Socks(e.to_string()),
    }
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

    const KRAKEN: &str = r#"{"error":[],"result":{"XXMRZUSD":{"a":["154.4","1","1.0"],"c":["160.004","0.5"]}}}"#;
    const BITFINEX: &str = "[538.8,199.9,539.64,45.0,-14.69,-0.026,538.29,3327.3,555.41,517.08]";
    const GECKO: &str = r#"{"monero":{"usd":150.5}}"#;
    const PAPRIKA: &str = r#"{"id":"xmr-monero","symbol":"XMR","quotes":{"USD":{"price":540.2511867,"volume_24h":1.0}}}"#;

    #[test]
    fn lee_las_cuatro_apis() {
        assert_eq!(leer_coingecko(r#"{"monero":{"usd":154.32}}"#), Some(154.32));
        assert_eq!(leer_coingecko(r#"{"status":{"error_code":429}}"#), None);
        assert_eq!(leer_kraken(KRAKEN), Some(160.004));
        assert_eq!(leer_kraken(r#"{"error":["EQuery:Unknown asset pair"]}"#), None);
        assert_eq!(leer_bitfinex(BITFINEX), Some(538.29));
        assert_eq!(leer_bitfinex(r#"["error",10020,"symbol: invalid"]"#), None);
        assert_eq!(leer_bitfinex("[]"), None);
        assert_eq!(leer_coinpaprika(PAPRIKA), Some(540.2511867));
        assert_eq!(leer_coinpaprika(r#"{"id":"btc-bitcoin","quotes":{"USD":{"price":1.0}}}"#), None);
        assert_eq!(leer_coinpaprika(r#"{"error":"id not found"}"#), None);
        for f in FUENTES.iter() {
            assert_eq!((f.leer)("<html>captcha</html>"), None, "{}", f.nombre);
        }
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
        tokio::runtime::Builder::new_current_thread().enable_time().start_paused(true).build().unwrap()
    }

    const PLAZO: Duration = Duration::from_secs(8);

    fn cuerpo(f: &Fuente) -> &'static str {
        match f.nombre {
            "kraken" => KRAKEN,
            "bitfinex" => BITFINEX,
            "coingecko" => GECKO,
            _ => PAPRIKA,
        }
    }

    #[test]
    fn gana_la_primera_que_responde() {
        // Kraken tarda 5 s, Bitfinex 1 s, CoinGecko falla, CoinPaprika 3 s: gana Bitfinex.
        let q = rt()
            .block_on(obtener_con(
                |f: &'static Fuente| async move {
                    let ms = match f.nombre {
                        "kraken" => 5_000,
                        "bitfinex" => 1_000,
                        "coingecko" => return Err(Motivo::Http("HTTP 429".into())),
                        _ => 3_000,
                    };
                    tokio::time::sleep(Duration::from_millis(ms)).await;
                    Ok(cuerpo(f).to_string())
                },
                PLAZO,
            ))
            .unwrap();
        assert_eq!((q.fuente.as_str(), q.centavos_por_xmr), ("bitfinex", 53_829));
    }

    #[test]
    fn una_bloqueada_no_frena_a_las_demas() {
        // CoinGecko (primera en contestar) da una página sin precio; Kraken sí.
        let q = rt()
            .block_on(obtener_con(
                |f: &'static Fuente| async move {
                    match f.nombre {
                        "coingecko" => Ok("<html>captcha</html>".to_string()),
                        "kraken" => {
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            Ok(KRAKEN.to_string())
                        }
                        _ => std::future::pending().await,
                    }
                },
                PLAZO,
            ))
            .unwrap();
        assert_eq!((q.fuente.as_str(), q.centavos_por_xmr), ("kraken", 16_000));
    }

    #[test]
    fn sin_ninguna_dice_que_paso_con_cada_una() {
        let fallas = rt()
            .block_on(obtener_con(
                |f: &'static Fuente| async move {
                    match f.nombre {
                        "kraken" => std::future::pending().await,
                        "bitfinex" => Err(Motivo::Socks("host unreachable".into())),
                        "coingecko" => Err(Motivo::Http("HTTP 403".into())),
                        _ => Ok("{}".to_string()),
                    }
                },
                PLAZO,
            ))
            .unwrap_err();
        let motivos: Vec<_> = fallas.iter().map(|f| (f.fuente.as_str(), f.motivo.clone())).collect();
        assert_eq!(
            motivos,
            vec![
                ("kraken", Motivo::Tiempo),
                ("bitfinex", Motivo::Socks("host unreachable".into())),
                ("coingecko", Motivo::Http("HTTP 403".into())),
                ("coinpaprika", Motivo::SinPrecio),
            ]
        );
        let f = Falla { tor: Some(fallas), directo: None, cuando: 0 };
        let es = f.texto(true);
        assert!(es.starts_with("Por Tor: Kraken: no respondió en 8 s"), "{es}");
        assert!(es.contains("CoinGecko: HTTP 403, suele bloquear a Tor"), "{es}");
        assert!(es.contains("CoinPaprika: respondió sin precio"), "{es}");
        let en = f.texto(false);
        assert!(en.contains("Bitfinex: Tor could not reach the server (host unreachable)"), "{en}");
        assert!(f.solo_tor());
    }

    #[test]
    fn el_plazo_es_por_fuente_y_en_paralelo() {
        // Las cuatro colgadas: el total es un plazo, no cuatro.
        let rt = rt();
        let t0 = rt.block_on(async { tokio::time::Instant::now() });
        let r = rt.block_on(obtener_con(|_f: &'static Fuente| std::future::pending::<Result<String, Motivo>>(), PLAZO));
        let t1 = rt.block_on(async { tokio::time::Instant::now() });
        assert_eq!(r.unwrap_err().len(), 4);
        assert!(t1 - t0 < PLAZO + Duration::from_secs(1), "{:?}", t1 - t0);
    }

    #[test]
    fn socks_caido_da_pista_de_orbot() {
        let todas = FUENTES.iter().map(|f| FallaFuente { fuente: f.nombre.into(), motivo: Motivo::SocksCaido }).collect();
        let f = Falla { tor: Some(todas), directo: None, cuando: 0 };
        assert_eq!(f.texto(true), "Por Tor: el SOCKS de Tor/Orbot no responde (¿está abierto Orbot?)");
    }

    fn q(fuente: &str) -> Cotizacion {
        Cotizacion { centavos_por_xmr: 54_000, fuente: fuente.into(), cuando: 1 }
    }

    fn falla(m: Motivo) -> Vec<FallaFuente> {
        vec![FallaFuente { fuente: "kraken".into(), motivo: m }]
    }

    #[test]
    fn sin_tor_solo_si_se_permitio() {
        let tor: SocketAddr = "127.0.0.1:9050".parse().unwrap();
        let rt = rt();
        let pedir = |s: Option<SocketAddr>| async move {
            match s {
                Some(_) => Err(falla(Motivo::Tiempo)),
                None => Ok(q("kraken")),
            }
        };
        // Por defecto: falla por Tor y NO se sale sin Tor.
        let f = rt.block_on(resolver(Some(tor), false, pedir)).unwrap_err();
        assert!(f.solo_tor());
        assert_eq!(f.tor, Some(falla(Motivo::Tiempo)));
        // Permitido siempre: reintenta sin Tor.
        assert_eq!(rt.block_on(resolver(Some(tor), true, pedir)).unwrap().fuente, "kraken");
        // Si por Tor anda, no se toca la conexión directa.
        let solo_tor = |s: Option<SocketAddr>| async move {
            assert!(s.is_some(), "no debe salir sin Tor");
            Ok(q("bitfinex"))
        };
        assert_eq!(rt.block_on(resolver(Some(tor), true, solo_tor)).unwrap().fuente, "bitfinex");
        // Sin SOCKS (la app entera va directo): directo.
        assert_eq!(rt.block_on(resolver(None, false, pedir)).unwrap().fuente, "kraken");
        // Las dos fallan: el mensaje tiene las dos partes.
        let nada = |_s: Option<SocketAddr>| async move { Err::<Cotizacion, _>(falla(Motivo::Red("dns".into()))) };
        let f = rt.block_on(resolver(Some(tor), true, nada)).unwrap_err();
        assert!(!f.solo_tor());
        assert_eq!(f.texto(true), "Por Tor: Kraken: sin conexión (dns) · Sin Tor: Kraken: sin conexión (dns)");
    }

    #[test]
    fn fijar_con_la_cotizacion() {
        let q = Cotizacion { centavos_por_xmr: 16_000, fuente: "coingecko".into(), cuando: 100 };
        let p = q.fijar(5_000).unwrap();
        assert_eq!(p.piconero, 312_500_000_000);
        assert_eq!((p.fuente.as_str(), p.cuando), ("coingecko", 100));
        assert_eq!(q.edad_seg(160), 60);
    }

    #[test]
    fn nombres_de_fuentes() {
        assert_eq!(nombre_fuente("bitfinex", true), "Bitfinex");
        assert_eq!(nombre_fuente(FUENTE_MANUAL, true), "precio a mano");
        assert_eq!(nombre_fuente(FUENTE_MANUAL, false), "manual price");
    }
}

#[cfg(test)]
mod con_archivos {
    //! Tocan `KONSTRUADO_DATOS` y el estado global: un solo test.
    use super::*;

    #[test]
    fn manual_y_ajuste_sin_tor() {
        let _g = crate::persist::datos_test_lock();
        let dir = std::env::temp_dir().join(format!("konstruado-precio-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let prev = std::env::var_os("KONSTRUADO_DATOS");
        unsafe { std::env::set_var("KONSTRUADO_DATOS", &dir) };
        assert!(!directo_siempre(), "por defecto no se sale sin Tor");
        fijar_directo_siempre(true);
        assert!(directo_siempre());
        fijar_directo_siempre(false);
        assert!(!directo_siempre());

        assert_eq!(fijar_manual("cero"), None);
        assert_eq!(fijar_manual("0,50"), None);
        let q = fijar_manual("USD 540,25").unwrap();
        assert_eq!((q.centavos_por_xmr, q.fuente.as_str()), (54_025, FUENTE_MANUAL));
        assert_eq!(ultima(), Some(q.clone()));
        let p = para_fijar(konstruado_core::ahora()).unwrap().fijar(5_000).unwrap();
        assert_eq!(p.fuente, FUENTE_MANUAL);
        assert!(p.coherente());
        // Vence como cualquier otro precio.
        assert!(para_fijar(q.cuando + MAX_EDAD_FIJAR_SEG + 1).is_none());

        // Una falla por Tor ofrece probar sin Tor; con el ajuste puesto, no hace falta preguntar.
        registrar(&Err(Falla { tor: Some(vec![]), directo: None, cuando: 1 }));
        assert!(ofrecer_directo());
        fijar_directo_siempre(true);
        assert!(!ofrecer_directo());
        match prev {
            Some(v) => unsafe { std::env::set_var("KONSTRUADO_DATOS", v) },
            None => unsafe { std::env::remove_var("KONSTRUADO_DATOS") },
        }
        let _ = std::fs::remove_dir_all(&dir);
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
            let t = std::time::Instant::now();
            let r = rt.block_on(async {
                tokio::time::timeout(super::PLAZO_FUENTE, super::https_get(socks, f.host, f.ruta)).await
            });
            let r = match r {
                Ok(Ok(b)) => format!("{:?}", (f.leer)(&b)),
                Ok(Err(m)) => m.texto(true),
                Err(_) => super::Motivo::Tiempo.texto(true),
            };
            println!("{}: {r} ({:.1} s)", f.nombre, t.elapsed().as_secs_f32());
        }
        let q = rt.block_on(super::pedir(socks));
        println!("en paralelo: {q:?}");
    }
}
