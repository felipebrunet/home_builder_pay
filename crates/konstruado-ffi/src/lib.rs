//! Fachada UniFFI para Konstruado Android.
//!
//! Reusa tal cual el motor del escritorio (`caja.rs`: DKG, fondeo CLSAG
//! cooperativo, gasto FROST, scan, envío, respaldos), `persist.rs` e `i18n.rs`.
//! La red es `konstruado-net` en modo celular: sesión viva saliente hacia la
//! sala (onion por Orbot SOCKS, o TCP para pruebas) y relay por la PC.
//!
//! Un solo `KonstruadoApp` por proceso: la carpeta de datos va por la variable
//! `KONSTRUADO_DATOS`, igual que en el escritorio.

uniffi::setup_scaffolding!();

use konstruado_motor::{caja, i18n, persist, respaldo};

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use i18n::Idioma;
use konstruado_core::{
    asegurar_clave, fmt_monto, generar_clave, leer_usd, monto_pct, usd_editable, Moneda, n_partidas, oferta_en_tablero, retirar_oferta,
    Aceptacion,
    EstadoObra, Obra, Oferta, PartidaEstado, Persona, Rol, TextoLeido, MAX_NOTA,
};
use konstruado_net::{DiagSocks, EstadoTor, Nodo, PeerAddr, ORBOT_SOCKS, PUERTO_LOCAL, RED, RENDEZVOUS_ONION, VIRT_PORT};

// ---------------------------------------------------------------- idioma

/// Idioma de los textos que arma el motor (es() por defecto). Lo fija la app
/// al arrancar (perfil o idioma del teléfono) y desde Cuenta.
static EN: AtomicBool = AtomicBool::new(false);

fn es() -> bool {
    !EN.load(Ordering::Relaxed)
}

fn l() -> Idioma {
    if es() {
        Idioma::Es
    } else {
        Idioma::En
    }
}

/// Texto fijo en el idioma activo.
fn tr<'a>(es_txt: &'a str, en_txt: &'a str) -> &'a str {
    if es() {
        es_txt
    } else {
        en_txt
    }
}

/// `format!` en el idioma activo: `tf!("hola {x}", "hi {x}")`.
macro_rules! tf {
    ($es:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        if es() { format!($es $(, $arg)*) } else { format!($en $(, $arg)*) }
    };
}

// ---------------------------------------------------------------- errores

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FfiError {
    #[error("{msg}")]
    Fallo { msg: String },
}

fn fallo(m: impl Into<String>) -> FfiError {
    FfiError::Fallo { msg: m.into() }
}

fn err_core(e: konstruado_core::Error) -> FfiError {
    fallo(l().error(&e))
}

fn err_caja(e: String) -> FfiError {
    fallo(caja::aviso_humano(&e, es()))
}

// ---------------------------------------------------------------- vistas

#[derive(Clone, Debug, uniffi::Record)]
pub struct PerfilVista {
    pub tiene_cuenta: bool,
    pub id: String,
    pub nombre: String,
    /// "mandante" | "contratista" | ""
    pub rol: String,
    pub rol_label: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RedVista {
    /// Una línea legible: estado de Orbot/sala y quién está.
    pub linea: String,
    /// Diagnóstico real de Orbot → sala (sin saber si Orbot está instalado).
    pub sala: SalaEstado,
    pub conectado: bool,
    pub pares: u32,
    pub sesiones_vivas: u32,
    pub socks: Option<String>,
    pub destinos: Vec<String>,
    pub otros: Vec<String>,
    pub red: String,
    pub onion_sala: String,
}

/// Estado de la conexión con la sala, medido (no supuesto).
///
/// `tipo`: "conectado", "socks_ok" (Orbot responde, llamando a la sala),
/// "sala_no_responde", "socks_caido", "sin_orbot" (no instalado),
/// "orbot_apagado_en_app", "probando", "tcp".
/// `tono`: "ok", "espera", "error", "apagado" (mismos que la billetera).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SalaEstado {
    pub tipo: String,
    pub tono: String,
    pub titulo: String,
    pub detalle: String,
    pub socks: Option<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct OfertaVista {
    pub id: String,
    pub nombre: String,
    pub trabajo: u64,
    pub garantia_sugerida: u64,
    pub n_partidas: u32,
    pub mandante: String,
    pub mia: bool,
    pub detalles: Vec<String>,
    pub resumen: String,
    /// Oferta en dólares (las viejas van en unidades).
    pub usd: bool,
    /// La garantía sugerida para el campo editable (`200` / `200.50`).
    pub garantia_editable: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct PreviaPublicar {
    pub ok: bool,
    pub n_partidas: u32,
    pub texto: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct PreviaAceptar {
    pub ok: bool,
    pub contra: bool,
    pub n_partidas: u32,
    pub texto: String,
    pub detalles: Vec<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct AvisoVista {
    pub texto: String,
    pub obra_id: String,
    pub partida: Option<u32>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ObraFila {
    pub id: String,
    pub nombre: String,
    pub estado: String,
    pub estado_label: String,
    pub en_curso: bool,
    pub con_quien: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct TableroVista {
    pub red: RedVista,
    pub avisos: Vec<AvisoVista>,
    pub mis_ofertas: Vec<OfertaVista>,
    pub ofertas: Vec<OfertaVista>,
    pub obras: Vec<ObraFila>,
    pub pista: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct PartidaFila {
    pub indice: u32,
    pub titulo: String,
    pub label: String,
    pub estado: String,
    pub por_lado: String,
    pub saldo_corto: Option<String>,
    pub activa: bool,
    /// «Se libera en ~N bloques»: el fondeo todavía no juntó 10 confirmaciones.
    pub traba_corta: Option<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ExtraVista {
    pub texto: String,
    pub mia: bool,
    pub monto: String,
    pub por: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct MiradaVista {
    pub bloques: String,
    pub retro: Option<String>,
    pub aviso: Option<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ObraVista {
    pub id: String,
    pub nombre: String,
    pub estado: String,
    pub estado_label: String,
    pub resumen: String,
    pub soy_mandante: bool,
    pub soy_contratista: bool,
    pub abierta: bool,
    pub sincronizando: bool,
    pub contra: bool,
    pub contra_texto: Option<String>,
    pub abandonada: bool,
    pub cierre_mio: bool,
    pub cierre_de: Option<String>,
    pub hay_riesgo: bool,
    pub extra: Option<ExtraVista>,
    pub puede_extra: bool,
    pub caja_direccion: Option<String>,
    pub armando_caja: bool,
    pub mirada: Option<MiradaVista>,
    pub partidas: Vec<PartidaFila>,
    /// Obra en dólares (montos USD, XMR fijo por partida al fondear).
    pub usd: bool,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct NotaVista {
    pub cabeza: String,
    pub cuerpo: String,
    pub cifrada: bool,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct PartidaVista {
    pub obra_id: String,
    pub obra_nombre: String,
    pub indice: u32,
    pub titulo: String,
    pub label: String,
    pub estado: String,
    pub lead: String,
    pub saldo_estado: Option<String>,
    pub saldo_detalle: Option<String>,
    pub candado: Option<String>,
    pub xmr_por_lado: Option<String>,
    /// Obras en USD: el XMR de la partida (fijo si ya tiene precio, aproximado si no).
    pub xmr_partida: Option<String>,
    /// Precio que propuso el otro, para quien confirma y fondea.
    pub precio_propuesto: Option<String>,
    /// El precio fijado se aleja del actual.
    pub aviso_precio: Option<String>,
    /// Obras en USD sin precio fijado: estado del precio de referencia.
    pub estado_precio: Option<String>,
    pub caja_direccion: Option<String>,
    pub fondeo_txid: Option<String>,
    pub pago_txid: Option<String>,
    /// Lo que está haciendo el motor de Monero para esta partida.
    pub linea: Option<String>,
    pub linea_freno: bool,
    pub sincronizando: bool,
    pub cortada: bool,
    pub detalle: String,
    pub puede_editar: bool,
    pub recibo: Option<String>,
    pub cerrado_texto: Option<String>,
    pub encerro: Option<String>,
    pub notas: Vec<NotaVista>,
    // Qué botones van.
    pub pista: Option<String>,
    pub puede_proponer_encerrar: bool,
    pub puede_cancelar_propuesta: bool,
    pub puede_reintentar_fondeo: bool,
    /// Tras un rechazo del nodo: reinicio total con decoys frescos.
    pub puede_empezar_fondeo_de_nuevo: bool,
    pub puede_confirmar_fondear: bool,
    pub puede_no_encerrar: bool,
    pub puede_avisar_termino: bool,
    pub en_trato: bool,
    pub propuesto: Option<u32>,
    pub propuesto_texto: Option<String>,
    pub mi_turno: bool,
    pub espera_a: Option<String>,
    pub max_nota: u32,
    /// En trato, me toca y no hay un pago ya andando (regla de `caja::acciones_partida`).
    pub puede_aceptar_pago: bool,
    pub puede_contraofertar: bool,
    /// "Abandonar partida (solo este equipo)": solo cuando hay un fondeo local que limpiar.
    pub puede_salir_local: bool,
    /// Texto corto de lo que está en curso ("Pago esperando bloque"…), para un chip.
    pub en_curso: Option<String>,
    /// El pago 2-de-2 ya se está firmando o espera bloque.
    pub pago_en_curso: bool,
    /// Fondeo sin 10 confirmaciones: «Podés marcarla terminada en ~N bloques…».
    /// Mientras esté, «Terminé» y «Aceptar y pagar» van deshabilitados.
    pub traba: Option<String>,
    pub traba_corta: Option<String>,
    /// «Avisar que terminé» va, pero deshabilitado (contratista, fondeo sin confirmar).
    pub termino_trabado: bool,
    /// «Aceptar y pagar» va, pero deshabilitado (me toca, fondeo sin confirmar).
    pub pago_trabado: bool,
}

/// Estado del respaldo completo (regla compartida `respaldo::estado`).
#[derive(Clone, Debug, uniffi::Record)]
pub struct RespaldoEstado {
    pub linea: String,
    /// "ok", "espera", "error", "apagado".
    pub tono: String,
    /// Hay obras o cajas nuevas que el último respaldo no tiene (recordatorio).
    pub falta: bool,
    pub ultimo: Option<String>,
    pub ayuda: Vec<String>,
    pub nombre_archivo: String,
    pub clave_minima: u32,
}

/// Lo que trae un respaldo completo, ya validado, antes de restaurarlo.
#[derive(Clone, Debug, uniffi::Record)]
pub struct RespaldoResumen {
    pub nombre: String,
    pub rol: String,
    pub creado: String,
    pub app: String,
    pub n_obras: u32,
    pub n_ofertas: u32,
    pub n_shares: u32,
    pub direccion: Option<String>,
    pub altura: Option<u64>,
    /// En este equipo ya hay cuenta, semilla o shares: hace falta la confirmación de peligro.
    pub hay_datos: bool,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SemillaVistaFfi {
    /// Las 25 palabras en una sola línea, separadas por espacios.
    pub palabras: String,
    pub altura: Option<u64>,
    pub direccion: String,
    /// Advertencias a mostrar antes / junto a las palabras.
    pub avisos: Vec<String>,
    /// Texto al copiar (incluye el aviso de borrado del portapapeles).
    pub aviso_copia: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct LlavesBilleteraFfi {
    pub direccion: String,
    pub view_key: String,
    pub ayuda: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct MovVista {
    pub monto: String,
    pub detalle: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct CajaFila {
    pub obra_id: String,
    pub obra_nombre: String,
    pub direccion: String,
    pub mirada: Option<MiradaVista>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct BilleteraVista {
    pub daemon: String,
    pub tip: Option<u64>,
    pub direccion: Option<String>,
    pub tiene_semilla: bool,
    pub total: String,
    pub libre: String,
    pub trabado: String,
    pub visto: String,
    pub buscando: bool,
    pub enviando: bool,
    pub retro: Option<String>,
    pub aviso: Option<String>,
    pub ultimo: Option<String>,
    pub movs: Vec<MovVista>,
    pub escala: String,
    pub cajas: Vec<CajaFila>,
    /// Línea de estado de alto fijo (regla compartida `caja::estado_billetera`).
    pub estado_linea: String,
    /// "ok", "espera", "error", "apagado".
    pub estado_tono: String,
    /// Saldo total en piconeros, para formatear sin perder precisión.
    pub total_pico: u64,
    /// Ayuda del envío (regla compartida `caja::ayuda_envio`).
    pub ayuda_envio: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct DaemonPrueba {
    /// true = tip OK
    pub ok: bool,
    pub url: String,
    pub tip: Option<u64>,
    /// Milisegundos que tardó connect+tip (o el fallo).
    pub ms: u64,
    /// Texto listo para mostrar (español).
    pub mensaje: String,
    /// Nodo de la red local / Tailscale (no pasa por Tor nunca).
    pub local: bool,
    /// Ruta legible: «directo por la red local, sin Tor», «por la VPN del teléfono…».
    pub ruta: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct OrbotHint {
    pub socks_host: String,
    pub socks_port: u16,
    pub onion_sala: String,
    pub pasos: Vec<String>,
}

// ---------------------------------------------------------------- estado

struct Sesion {
    yo: Option<Persona>,
    rol: Option<Rol>,
    clave_sec: String,
    spend_sec: String,
    tema: String,
    idioma: String,
    idioma_fijo: bool,
}

/// Panel del precio USD/XMR (Publicar obra, encerrar partida).
#[derive(Clone, Debug, uniffi::Record)]
pub struct PrecioVista {
    /// Precio actual o por qué no hay (dice qué fuente falló y por qué).
    pub texto: String,
    /// Hay un precio reciente para fijar una partida.
    pub listo: bool,
    /// La última falla (por fuente), si la hubo.
    pub error: Option<String>,
    /// Por Tor no hubo precio: ofrecer "probar sin Tor" (con [`Self::aviso_sin_tor`]).
    pub ofrecer_sin_tor: bool,
    pub aviso_sin_tor: String,
    /// Se pide por Orbot.
    pub por_tor: bool,
}

#[derive(uniffi::Object)]
pub struct KonstruadoApp {
    rt: tokio::runtime::Runtime,
    nodo: Nodo,
    caja: caja::Caja,
    ses: Arc<Mutex<Sesion>>,
    datos: PathBuf,
    ultima_prueba: Mutex<Option<DaemonPrueba>>,
    /// Huella del último respaldo armado, hasta que Android confirma que lo guardó.
    respaldo_pendiente: Mutex<Option<respaldo::Huella>>,
}

fn parse_destino(s: &str) -> Option<PeerAddr> {
    let s = s.trim();
    let (host, port) = s.rsplit_once(':')?;
    let port: u16 = port.parse().ok()?;
    if host.is_empty() {
        return None;
    }
    if host.ends_with(".onion") {
        Some(PeerAddr::Onion {
            host: host.to_string(),
            port,
        })
    } else {
        Some(PeerAddr::Tcp {
            host: host.to_string(),
            port,
        })
    }
}

fn destino_txt(d: &PeerAddr) -> String {
    match d {
        PeerAddr::Tcp { host, port } => format!("{host}:{port}"),
        PeerAddr::Onion { host, port } if host == RENDEZVOUS_ONION => {
            tf!("sala {}…:{port} (Orbot)", "room {}…:{port} (Orbot)", &host[..10.min(host.len())])
        }
        PeerAddr::Onion { host, port } => format!("{host}:{port}"),
        PeerAddr::Buzon { .. } => tr("buzón", "mailbox").into(),
    }
}

fn aplicar_idioma(codigo: &str) {
    EN.store(Idioma::parse(codigo) == Idioma::En, Ordering::Relaxed);
}

/// Perfil elegido (o `en` de antes, que solo se guarda si alguien lo eligió);
/// si no, el idioma del teléfono cuando es es/en; si no, ES.
fn idioma_de_arranque(guardado: &str, fijo: bool, dispositivo: &str) -> &'static str {
    if fijo || guardado.eq_ignore_ascii_case("en") {
        return Idioma::parse(guardado).codigo();
    }
    let d = dispositivo.trim().to_ascii_lowercase();
    if d.starts_with("en") {
        "en"
    } else {
        "es"
    }
}

fn rol_txt(r: Option<Rol>) -> String {
    match r {
        Some(Rol::Mandante) => "mandante".into(),
        Some(Rol::Contratista) => "contratista".into(),
        None => String::new(),
    }
}

fn parse_rol(s: &str) -> Result<Rol, FfiError> {
    match s.trim().to_lowercase().as_str() {
        "mandante" => Ok(Rol::Mandante),
        "contratista" => Ok(Rol::Contratista),
        _ => Err(fallo(tr("Elegí si pagás la obra o la construís.", "Choose whether you pay for the job or build it."))),
    }
}

fn estado_obra_txt(e: EstadoObra) -> String {
    format!("{e:?}").to_lowercase()
}

fn estado_partida_txt(e: PartidaEstado) -> String {
    match e {
        PartidaEstado::Pendiente => "pendiente".into(),
        PartidaEstado::Encerrando => "en fondeo".into(),
        PartidaEstado::Encerrada => "en obra".into(),
        PartidaEstado::EnTrato => "en trato".into(),
        PartidaEstado::Pagada => "pagada".into(),
    }
}

fn obra_en_curso(e: EstadoObra) -> bool {
    matches!(
        e,
        EstadoObra::Publicada | EstadoObra::Contra | EstadoObra::Acordada | EstadoObra::EnMarcha
    )
}

fn otro_de(obra: &Obra, yo: &str) -> String {
    if yo == obra.mandante.id {
        obra.contratista.id.clone()
    } else {
        obra.mandante.id.clone()
    }
}

fn sellar_guardadas(nodo: &Nodo, yo: &Persona, sec: &str) {
    caja::sellar_obras_guardadas(nodo, yo, sec);
}

fn aplicar_monero(nodo: &Nodo, yo: &Persona, sec: &str, hechos: &[caja::Hecho]) {
    caja::aplicar_hechos_monero(nodo, yo, sec, hechos);
}

fn mirada_de(v: &caja::CajaVista, obra: &str) -> Option<MiradaVista> {
    v.caja_de(obra)?;
    let m = v.miradas.iter().find(|m| m.obra == obra);
    Some(match m {
        Some(m) => MiradaVista {
            bloques: tf!("La caja mira {} bloques hacia atrás.", "The box looks {} blocks back.", m.bloques),
            retro: (m.retro > 0).then(|| tf!("Quedan {} bloques por mirar en la caja.", "{} blocks left to scan in the box.", m.retro)),
            aviso: m.aviso.as_ref().map(|a| caja::aviso_humano(a, es())),
        },
        None => MiradaVista {
            bloques: tr("La caja arranca por los últimos 40 bloques.", "The box starts from the last 40 blocks.").into(),
            retro: None,
            aviso: None,
        },
    })
}

fn avisos_para(mid: &str, obras: &[Obra], sec: &str) -> Vec<AvisoVista> {
    let mut out = Vec::new();
    for obra in obras {
        if obra.mandante.id != mid && obra.contratista.id != mid {
            continue;
        }
        if matches!(
            obra.estado,
            EstadoObra::Rechazada | EstadoObra::Abandonada | EstadoObra::Cerrada
        ) {
            continue;
        }
        if obra.estado == EstadoObra::Contra && obra.mandante.id == mid {
            out.push(AvisoVista {
                texto: tf!(
                    "{}: {} propone garantía {}", "{}: {} proposes a guarantee of {}",
                    obra.nombre,
                    obra.contratista.nombre,
                    mm(obra.moneda, obra.garantia)
                ),
                obra_id: obra.id.clone(),
                partida: None,
            });
        }
        if let Some(ex) = obra.extra.as_ref() {
            if ex.por.id != mid {
                let detalle = match obra.leer_extra(sec) {
                    TextoLeido::Plano(t) => t,
                    TextoLeido::Cerrado => tr("Texto cifrado", "Encrypted text").into(),
                };
                out.push(AvisoVista {
                    texto: tf!(
                        "{}: {} propone extra {} ({})", "{}: {} proposes an extra {} ({})",
                        obra.nombre,
                        ex.por.nombre,
                        detalle,
                        mm(obra.moneda, ex.monto)
                    ),
                    obra_id: obra.id.clone(),
                    partida: None,
                });
            }
        }
        if let Some(cl) = obra.cierre.as_ref() {
            if cl.id != mid {
                out.push(AvisoVista {
                    texto: tf!("{}: {} quiere cortar el trato", "{}: {} wants to end the deal", obra.nombre, cl.nombre),
                    obra_id: obra.id.clone(),
                    partida: None,
                });
            }
        }
        let mi_rol = if obra.mandante.id == mid {
            Rol::Mandante
        } else {
            Rol::Contratista
        };
        for (i, p) in obra.partidas.iter().enumerate() {
            let titulo = l().titulo_partida(i, &p.detalle);
            if p.estado == PartidaEstado::Encerrando
                && p.encerrado_por.as_ref().map(|q| q.id.as_str()) != Some(mid)
            {
                out.push(AvisoVista {
                    texto: tf!("{} · {}: te toca confirmar el encierre", "{} · {}: your turn to confirm the lock", obra.nombre, titulo),
                    obra_id: obra.id.clone(),
                    partida: Some(i as u32),
                });
            }
            if p.estado == PartidaEstado::EnTrato && p.turno == Some(mi_rol) {
                let pct = p.propuesto.unwrap_or(0);
                out.push(AvisoVista {
                    texto: tf!("{} · {}: te toca responder ({pct}%)", "{} · {}: your turn to answer ({pct}%)", obra.nombre, titulo),
                    obra_id: obra.id.clone(),
                    partida: Some(i as u32),
                });
            }
            if p.estado == PartidaEstado::Encerrada && mi_rol == Rol::Contratista {
                let quien = p
                    .encerrado_por
                    .as_ref()
                    .map(|q| q.nombre.as_str())
                    .unwrap_or(tr("el mandante", "the client"));
                out.push(AvisoVista {
                    texto: tf!(
                        "{} · {}: {quien} encerró, avisá cuando termines", "{} · {}: {quien} locked it, report when you finish",
                        obra.nombre, titulo
                    ),
                    obra_id: obra.id.clone(),
                    partida: Some(i as u32),
                });
            }
        }
    }
    out
}

fn oferta_vista(o: &Oferta, mid: &str) -> OfertaVista {
    let mia = o.mandante.id == mid;
    let resumen = if mia {
        tf!(
            "Trabajo {} · garantía sugerida {} · {} partidas", "Job {} · suggested guarantee {} · {} stages",
            mm(o.moneda, o.trabajo),
            mm(o.moneda, o.garantia_sugerida),
            o.n_partidas_sugeridas
        )
    } else {
        tf!(
            "{} ofrece trabajo por {}. Garantía sugerida {} ({} partidas).", "{} offers a job for {}. Suggested guarantee {} ({} stages).",
            o.mandante.nombre,
            mm(o.moneda, o.trabajo),
            mm(o.moneda, o.garantia_sugerida),
            o.n_partidas_sugeridas
        )
    };
    OfertaVista {
        id: o.id.clone(),
        nombre: o.nombre.clone(),
        trabajo: o.trabajo,
        garantia_sugerida: o.garantia_sugerida,
        n_partidas: o.n_partidas_sugeridas,
        mandante: o.mandante.nombre.clone(),
        mia,
        detalles: o.detalles.clone(),
        resumen,
        usd: o.moneda.es_usd(),
        garantia_editable: match o.moneda {
            Moneda::Usd => usd_editable(o.garantia_sugerida),
            Moneda::Unidades => o.garantia_sugerida.to_string(),
        },
    }
}

fn parse_num(s: &str) -> u64 {
    s.chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

/// Un monto escrito a mano según la moneda de la obra u oferta.
fn leer(moneda: Moneda, s: &str) -> u64 {
    match moneda {
        Moneda::Usd => leer_usd(s).unwrap_or(0),
        Moneda::Unidades => parse_num(s),
    }
}

fn mm(moneda: Moneda, n: u64) -> String {
    fmt_monto(moneda, n, es())
}

fn recorta_nota(s: String) -> String {
    if s.chars().count() <= MAX_NOTA {
        s
    } else {
        s.chars().take(MAX_NOTA).collect()
    }
}

// ---------------------------------------------------------------- objeto

impl KonstruadoApp {
    fn yo(&self) -> Result<Persona, FfiError> {
        self.ses
            .lock()
            .unwrap()
            .yo
            .clone()
            .ok_or_else(|| fallo(tr("Falta tu nombre en este equipo.", "Your name is missing on this device.")))
    }

    fn sec(&self) -> String {
        self.ses.lock().unwrap().clave_sec.clone()
    }

    fn mid(&self) -> String {
        self.ses
            .lock()
            .unwrap()
            .yo
            .as_ref()
            .map(|p| p.id.clone())
            .unwrap_or_default()
    }

    fn obra(&self, id: &str) -> Result<Obra, FfiError> {
        self.nodo
            .obras_todas()
            .into_iter()
            .find(|o| o.id == id)
            .ok_or_else(|| fallo(tr("La obra todavía no llegó a este equipo.", "The job has not reached this device yet.")))
    }

    fn persistir(&self) {
        let s = self.ses.lock().unwrap();
        persist::guardar(&persist::EstadoDisco {
            yo: s.yo.clone(),
            rol: s.rol,
            ofertas: self.nodo.tablero(),
            obras: self.nodo.obras_todas(),
            presentes: self.nodo.presentes(),
            tema: s.tema.clone(),
            idioma: s.idioma.clone(),
            idioma_fijo: s.idioma_fijo,
            clave_sec: s.clave_sec.clone(),
            spend_sec: s.spend_sec.clone(),
            obras_salidas: self.nodo.obras_salidas(),
            retiradas: self.nodo.retiradas(),
        });
    }

    /// Igual que el escritorio: la red arrancó, hay nombre y el trato está alineado.
    fn exigir_sesion(&self, obra: &Obra) -> Result<Persona, FfiError> {
        let p = self.yo()?;
        let otro = otro_de(obra, &p.id);
        let n = &self.nodo;
        if matches!(n.estado_tor(), EstadoTor::Arrancando { .. }) && n.n_peers() == 0 {
            return Err(fallo(
                tr("Sincronizando el trato… esperá a que baje el estado del otro.", "Syncing the deal… wait for the other side's state to arrive."),
            ));
        }
        if n.trato_alineado(&p.id, &otro) {
            Ok(p)
        } else if n.sesion_viva(&p.id, &otro) && !n.sync_reciente() {
            Err(fallo(
                tr("Sincronizando el trato… todavía no bajó lo último del otro.", "Syncing the deal… the other side's latest state has not arrived yet."),
            ))
        } else {
            Err(fallo(
                tr("El otro no está en línea. Tiene que tener Konstruado abierto.", "The other person is not online. They need Konstruado open."),
            ))
        }
    }

    fn sincronizando(&self, obra: &Obra) -> bool {
        let Ok(p) = self.yo() else { return false };
        let otro = otro_de(obra, &p.id);
        let n = &self.nodo;
        if n.trato_alineado(&p.id, &otro) {
            return false;
        }
        (matches!(n.estado_tor(), EstadoTor::Arrancando { .. }) && n.n_peers() == 0)
            || (n.sesion_viva(&p.id, &otro) && !n.sync_reciente())
    }

    fn publicar_trato(&self, mut obra: Obra, q: &Persona) -> Result<(), FfiError> {
        let sec = self.sec();
        obra.preparar_para_red(&q.id, &q.clave_pub, &sec)
            .map_err(err_core)?;
        self.nodo.publicar_obra(obra);
        Ok(())
    }

    /// Acción sobre una obra con sesión exigida, y publicación al final.
    fn accion<F>(&self, obra_id: &str, exigir: bool, f: F) -> Result<(), FfiError>
    where
        F: FnOnce(&mut Obra, &Persona) -> Result<(), konstruado_core::Error>,
    {
        let mut obra = self.obra(obra_id)?;
        let q = if exigir {
            self.exigir_sesion(&obra)?
        } else {
            self.yo()?
        };
        f(&mut obra, &q).map_err(err_core)?;
        self.publicar_trato(obra, &q)
    }

    fn tmp(&self, nombre: &str) -> PathBuf {
        let dir = self.datos.join("tmp");
        let _ = std::fs::create_dir_all(&dir);
        let mut b = [0u8; 6];
        use rand_core::RngCore;
        rand_core::OsRng.fill_bytes(&mut b);
        dir.join(format!("{}-{nombre}", hex::encode(b)))
    }

    fn lanzar_bucle(&self) {
        let nodo = self.nodo.clone();
        let caja = self.caja.clone();
        let ses = self.ses.clone();
        self.rt.spawn(async move {
            loop {
                // Precio USD/XMR siempre al día (como el escritorio), en cualquier
                // pantalla: antes solo se pedía con "Publicar obra" abierta.
                konstruado_motor::cotizacion::refrescar_en_fondo(nodo.socks());
                let (yo, rol, sec, disco) = {
                    let s = ses.lock().unwrap();
                    (
                        s.yo.clone(),
                        s.rol,
                        s.clave_sec.clone(),
                        (s.tema.clone(), s.idioma.clone(), s.spend_sec.clone(), s.idioma_fijo),
                    )
                };
                if let Some(p) = &yo {
                    nodo.fijar_persona(&p.id);
                    nodo.anunciar(p.clone());
                    nodo.anunciar_persona();
                    sellar_guardadas(&nodo, p, &sec);
                    let obras = nodo.obras();
                    let hechos = tokio::task::block_in_place(|| caja.tick(&nodo, p, &obras));
                    aplicar_monero(&nodo, p, &sec, &hechos);
                }
                if let Some(r) = rol {
                    nodo.entrar_en_sala(r == Rol::Mandante);
                }
                persist::guardar(&persist::EstadoDisco {
                    yo,
                    rol,
                    ofertas: nodo.tablero(),
                    obras: nodo.obras_todas(),
                    presentes: nodo.presentes(),
                    tema: disco.0,
                    idioma: disco.1,
                    idioma_fijo: disco.3,
                    clave_sec: sec,
                    spend_sec: disco.2,
                    obras_salidas: nodo.obras_salidas(),
                    retiradas: nodo.retiradas(),
                });
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
    }

    fn red_interna(&self) -> RedVista {
        let n = &self.nodo;
        let mid = self.mid();
        let pares = n.n_peers();
        let vivos = n.n_vivos();
        let mut otros: Vec<String> = if pares == 0 {
            Vec::new()
        } else {
            n.presentes()
                .into_iter()
                .filter(|p| p.id != mid)
                .filter(|p| konstruado_core::ahora().saturating_sub(p.visto) <= 60)
                .map(|p| p.nombre)
                .collect()
        };
        otros.sort();
        otros.dedup();
        let tor = n.estado_tor();
        let socks = n.socks().map(|s| s.to_string());
        let hay_tcp = n.destinos().iter().any(|d| matches!(d, PeerAddr::Tcp { .. }));
        let sala = sala_de(&n.diag_socks(), socks.as_deref(), vivos, hay_tcp, None);
        let estado = match &tor {
            _ if socks.is_some() || vivos > 0 => sala.titulo.clone(),
            EstadoTor::Listo { .. } if vivos > 0 => tr("Conectado a la sala", "Connected to the room").to_string(),
            EstadoTor::Listo { .. } => tr("Orbot listo, buscando sala", "Orbot ready, looking for the room").to_string(),
            EstadoTor::Arrancando { paso } => l().paso_tor(paso),
            EstadoTor::Fallo(s) => format!("Orbot: {s}"),
            EstadoTor::Ausente if vivos > 0 => tr("Conectado por TCP", "Connected over TCP").to_string(),
            EstadoTor::Ausente => tr("Sin Orbot ni destino TCP", "No Orbot and no TCP target").to_string(),
        };
        let gente = if otros.is_empty() {
            if pares == 0 {
                tr("nadie más en la red", "nobody else on the network").to_string()
            } else {
                tf!("{pares} par(es), todavía sin nombre", "{pares} peer(s), no name yet")
            }
        } else {
            otros.join(", ")
        };
        RedVista {
            linea: format!("{estado} · {gente}"),
            sala,
            conectado: vivos > 0,
            pares: pares as u32,
            sesiones_vivas: vivos as u32,
            socks,
            destinos: n.destinos().iter().map(destino_txt).collect(),
            otros,
            red: RED.into(),
            onion_sala: RENDEZVOUS_ONION.into(),
        }
    }
}

/// Traduce el diagnóstico medido a un estado para la pantalla.
/// Solo pide encender Orbot cuando su SOCKS de verdad no contesta.
fn sala_de(
    diag: &DiagSocks,
    socks: Option<&str>,
    vivos: usize,
    hay_tcp: bool,
    orbot_instalado: Option<bool>,
) -> SalaEstado {
    let e = |tipo: &str, tono: caja::Tono, titulo: String, detalle: String| SalaEstado {
        tipo: tipo.into(),
        tono: tono.codigo().into(),
        titulo,
        detalle,
        socks: socks.map(str::to_string),
    };
    let s = socks.unwrap_or("127.0.0.1:9050");
    if vivos > 0 {
        return e(
            "conectado",
            caja::Tono::Ok,
            tr("Conectado a la sala", "Connected to the room").into(),
            tf!("Sesiones vivas: {vivos}.", "Live sessions: {vivos}."),
        );
    }
    match diag {
        DiagSocks::SinSocks if hay_tcp => e(
            "tcp",
            caja::Tono::Espera,
            tr("Buscando por TCP…", "Looking over TCP…").into(),
            tr("Sin Orbot: se marca solo el destino TCP de Avanzado.", "No Orbot: only the TCP target from Advanced is dialed.").into(),
        ),
        DiagSocks::SinSocks => e(
            "orbot_apagado_en_app",
            caja::Tono::Apagado,
            tr("Orbot apagado en Konstruado", "Orbot is off in Konstruado").into(),
            tr("Activá «Usar Orbot» en Red para marcar la sala.", "Turn on “Use Orbot” under Network to dial the room.").into(),
        ),
        DiagSocks::SinProbar => e(
            "probando",
            caja::Tono::Espera,
            tr("Probando Orbot…", "Testing Orbot…").into(),
            tf!("Primer intento por el SOCKS {s}.", "First attempt through SOCKS {s}."),
        ),
        DiagSocks::SocksCaido(_) if orbot_instalado == Some(false) => e(
            "sin_orbot",
            caja::Tono::Error,
            tr("Orbot no está instalado", "Orbot is not installed").into(),
            tr("Instalalo desde F-Droid o Google Play y encendelo.", "Install it from F-Droid or Google Play and start it.").into(),
        ),
        DiagSocks::SocksCaido(err) => e(
            "socks_caido",
            caja::Tono::Error,
            tf!("Orbot no responde en {s}", "Orbot does not answer on {s}"),
            tf!("Abrí Orbot y tocá Iniciar, o revisá el puerto SOCKS ({err}).", "Open Orbot and tap Start, or check the SOCKS port ({err})."),
        ),
        DiagSocks::SocksOk => e(
            "socks_ok",
            caja::Tono::Espera,
            tr("Orbot responde · llamando a la sala…", "Orbot answers · calling the room…").into(),
            tr("Tor puede tardar hasta un minuto en encontrar la sala.", "Tor can take up to a minute to find the room.").into(),
        ),
        DiagSocks::DestinoNoResponde(err) => e(
            "sala_no_responde",
            // Ámbar: Orbot anda; falta el otro lado (PC apagado / sin Konstruado).
            caja::Tono::Espera,
            tr("La sala no responde", "The room does not answer").into(),
            tf!("Orbot funciona. ¿Está abierto Konstruado en el PC? ({err})", "Orbot works. Is Konstruado open on the PC? ({err})"),
        ),
        DiagSocks::Conectado => e(
            "socks_ok",
            caja::Tono::Espera,
            tr("Reconectando con la sala…", "Reconnecting to the room…").into(),
            tr("Orbot funciona; se cortó la sesión y se vuelve a marcar.", "Orbot works; the session dropped and is being dialed again.").into(),
        ),
    }
}

fn guardar_prueba(app_prueba: &Mutex<Option<DaemonPrueba>>, p: DaemonPrueba) -> DaemonPrueba {
    if let Ok(mut g) = app_prueba.lock() {
        *g = Some(p.clone());
    }
    p
}

#[uniffi::export]
impl KonstruadoApp {
    /// Abre (o crea) el perfil en `datos_dir` y arranca la red sola.
    ///
    /// `socks_host`/`socks_port`: Orbot (127.0.0.1:9050). Con SOCKS se marca el
    /// onion de la sala. `destinos`: extras `host:puerto` por TCP (emulador
    /// `10.0.2.2:17432`, `adb reverse`, LAN). `escritorio=true` arranca como el
    /// escritorio (escucha en 17432, sin modo celular): solo para pruebas en PC.
    #[uniffi::constructor]
    pub fn nuevo(
        datos_dir: String,
        socks_host: Option<String>,
        socks_port: Option<u16>,
        destinos: Vec<String>,
        escritorio: bool,
    ) -> Result<Arc<Self>, FfiError> {
        let datos = PathBuf::from(&datos_dir);
        std::fs::create_dir_all(&datos).map_err(|e| fallo(tf!("Carpeta de datos: {e}", "Data folder: {e}")))?;
        std::env::set_var("KONSTRUADO_DATOS", &datos_dir);
        // Un respaldo completo restaurado se aplica antes de leer nada (ver respaldo.rs).
        respaldo::aplicar_pendiente(&datos).map_err(|e| fallo(tf!("Restaurar respaldo: {e}", "Restore backup: {e}")))?;
        persist::cargar_daemon_al_arrancar().map_err(fallo)?;
        let mut g = persist::cargar();
        if let Some(yo) = g.yo.as_mut() {
            let (sec, pubk) = asegurar_clave(&g.clave_sec, &yo.clave_pub);
            let cambio = g.clave_sec != sec || yo.clave_pub != pubk;
            yo.clave_pub = pubk;
            g.clave_sec = sec;
            if cambio {
                persist::guardar(&g);
            }
        }
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(3)
            .thread_name("konstruado")
            .enable_all()
            .build()
            .map_err(|e| fallo(format!("Runtime: {e}")))?;
        let caja = {
            let _e = rt.enter();
            caja::Caja::nueva()
        };
        let socks = match (socks_host, socks_port) {
            (Some(h), Some(p)) if !h.trim().is_empty() && p > 0 => Some((h.trim().to_string(), p)),
            _ => None,
        };
        let dest: Vec<PeerAddr> = destinos.iter().filter_map(|s| parse_destino(s)).collect();
        let nodo = rt
            .block_on(async {
                if escritorio {
                    let n = Nodo::arrancar_en(PUERTO_LOCAL).await?;
                    for d in dest {
                        n.agregar_destino(d);
                    }
                    Ok::<_, std::io::Error>(n)
                } else {
                    Nodo::arrancar_movil(socks, dest).await
                }
            })
            .map_err(|e| fallo(tf!("Red: {e}", "Network: {e}")))?;
        // Hidratar como el escritorio: obras propias selladas, presentes propios.
        let mut obras0 = g.obras.clone();
        if let Some(p) = g.yo.clone() {
            for o in &mut obras0 {
                if o.participa(&p.id) {
                    let _ = o.preparar_para_red(&p.id, &p.clave_pub, &g.clave_sec);
                }
            }
        }
        let yo_id = g.yo.as_ref().map(|p| p.id.clone());
        let presentes0: Vec<Persona> = g
            .presentes
            .iter()
            .filter(|p| Some(&p.id) == yo_id.as_ref())
            .cloned()
            .collect();
        {
            let _e = rt.enter();
            nodo.fijar_obras_salidas(g.obras_salidas.clone());
            nodo.fijar_retiradas(g.retiradas.clone());
            nodo.hidratar(g.ofertas.clone(), obras0, presentes0);
            if let Some(p) = g.yo.clone() {
                nodo.actualizar_yo(p);
            }
        }
        let app = Arc::new(Self {
            rt,
            nodo,
            caja,
            ses: Arc::new(Mutex::new(Sesion {
                yo: g.yo.clone(),
                rol: g.rol,
                clave_sec: g.clave_sec.clone(),
                spend_sec: g.spend_sec.clone(),
                tema: if g.tema.is_empty() { "vivo".into() } else { g.tema.clone() },
                idioma: g.idioma.clone(),
                idioma_fijo: g.idioma_fijo,
            })),
            datos,
            ultima_prueba: Mutex::new(None),
            respaldo_pendiente: Mutex::new(None),
        });
        app.lanzar_bucle();
        Ok(app)
    }

    // ------------------------------------------------------------ idioma

    /// Idioma activo: `es` o `en`.
    pub fn idioma(&self) -> String {
        l().codigo().into()
    }

    /// Al arrancar: el idioma elegido en el perfil (compartido con el
    /// escritorio) o, si nunca se eligió, el del teléfono si es es/en; si no, ES.
    /// Lo aplica a los textos del motor y lo devuelve.
    pub fn idioma_inicial(&self, dispositivo: String) -> String {
        let s = self.ses.lock().unwrap();
        let codigo = idioma_de_arranque(&s.idioma, s.idioma_fijo, &dispositivo);
        drop(s);
        aplicar_idioma(codigo);
        codigo.into()
    }

    /// ES/EN desde Cuenta: se guarda en el perfil y se aplica ya.
    pub fn fijar_idioma(&self, codigo: String) -> String {
        let i = Idioma::parse(&codigo);
        aplicar_idioma(i.codigo());
        {
            let mut s = self.ses.lock().unwrap();
            s.idioma = i.codigo().into();
            s.idioma_fijo = true;
        }
        self.persistir();
        i.codigo().into()
    }

    /// URL del repositorio (`CARGO_PKG_REPOSITORY`), para «Código en GitHub».
    pub fn repositorio(&self) -> String {
        env!("CARGO_PKG_REPOSITORY").into()
    }

    pub fn version(&self) -> String {
        format!(
            "konstruado-ffi {} · {} · monero_fn={}",
            env!("CARGO_PKG_VERSION"),
            RED,
            xmr_joint::monero_fn()
        )
    }

    // ------------------------------------------------------------ cuenta

    pub fn perfil(&self) -> PerfilVista {
        let s = self.ses.lock().unwrap();
        PerfilVista {
            tiene_cuenta: s.yo.is_some() && s.rol.is_some(),
            id: s.yo.as_ref().map(|p| p.id.clone()).unwrap_or_default(),
            nombre: s.yo.as_ref().map(|p| p.nombre.clone()).unwrap_or_default(),
            rol: rol_txt(s.rol),
            rol_label: s.rol.map(|r| l().rol(r).to_string()).unwrap_or_default(),
        }
    }

    pub fn crear_cuenta(&self, nombre: String, rol: String) -> Result<PerfilVista, FfiError> {
        let r = parse_rol(&rol)?;
        let mut p = Persona::nueva(nombre).map_err(err_core)?;
        let (sec, pubk) = generar_clave();
        p.clave_pub = pubk;
        {
            let mut s = self.ses.lock().unwrap();
            s.yo = Some(p.clone());
            s.rol = Some(r);
            s.clave_sec = sec;
        }
        let _e = self.rt.enter();
        self.nodo.entrar_en_sala(r == Rol::Mandante);
        self.nodo.actualizar_yo(p);
        drop(_e);
        self.persistir();
        Ok(self.perfil())
    }

    pub fn guardar_cuenta(&self, nombre: String, rol: String) -> Result<PerfilVista, FfiError> {
        let r = parse_rol(&rol)?;
        let mut p = self.yo()?;
        p.renombrar(nombre).map_err(err_core)?;
        {
            let mut s = self.ses.lock().unwrap();
            s.yo = Some(p.clone());
            s.rol = Some(r);
        }
        let _e = self.rt.enter();
        self.nodo.actualizar_yo(p);
        self.nodo.entrar_en_sala(r == Rol::Mandante);
        drop(_e);
        self.persistir();
        Ok(self.perfil())
    }

    // ------------------------------------------------------------ red

    pub fn red(&self) -> RedVista {
        self.red_interna()
    }

    /// Estado de la sala con lo que sabe Android (si Orbot está instalado).
    pub fn estado_sala(&self, orbot_instalado: bool) -> SalaEstado {
        let n = &self.nodo;
        let socks = n.socks().map(|s| s.to_string());
        let hay_tcp = n.destinos().iter().any(|d| matches!(d, PeerAddr::Tcp { .. }));
        sala_de(&n.diag_socks(), socks.as_deref(), n.n_vivos(), hay_tcp, Some(orbot_instalado))
    }

    /// Prueba ya el SOCKS de Orbot (saludo SOCKS5, 3 s) y devuelve el estado.
    /// No espera a la sala: eso lo mide el intento que corre en segundo plano.
    pub fn probar_orbot(&self, orbot_instalado: bool) -> SalaEstado {
        if let Some(addr) = self.nodo.socks() {
            let r = self
                .rt
                .block_on(konstruado_net::probar_socks(addr, std::time::Duration::from_secs(3)));
            let d = self.nodo.diag_socks();
            match r {
                Ok(()) => {
                    if matches!(d, DiagSocks::SinProbar | DiagSocks::SocksCaido(_)) {
                        self.nodo.fijar_diag_socks(DiagSocks::SocksOk);
                    }
                }
                Err(e) => self.nodo.fijar_diag_socks(DiagSocks::SocksCaido(e)),
            }
        }
        self.estado_sala(orbot_instalado)
    }

    pub fn configurar_socks(&self, host: String, port: u16) {
        let _e = self.rt.enter();
        self.nodo.configurar_socks(&host, port);
        self.nodo.agregar_destino(PeerAddr::Onion {
            host: RENDEZVOUS_ONION.into(),
            port: VIRT_PORT,
        });
    }

    pub fn agregar_destino(&self, destino: String) -> Result<(), FfiError> {
        let d = parse_destino(&destino)
            .ok_or_else(|| fallo(tr("Destino inválido. Usá host:puerto, p. ej. 10.0.2.2:17432.", "Invalid target. Use host:port, e.g. 10.0.2.2:17432.")))?;
        let _e = self.rt.enter();
        self.nodo.agregar_destino(d);
        Ok(())
    }

    pub fn quitar_destino(&self, destino: String) {
        if let Some(d) = parse_destino(&destino) {
            self.nodo.quitar_destino(&d);
        }
        // "sala …" quita el onion.
        if destino.starts_with("sala") || destino.starts_with("room") {
            self.nodo.quitar_destino(&PeerAddr::Onion {
                host: RENDEZVOUS_ONION.into(),
                port: VIRT_PORT,
            });
            self.nodo.quitar_socks();
        }
    }

    /// Fuerza un empuje de gossip ("Buscar ofertas").
    pub fn buscar(&self) {
        let _e = self.rt.enter();
        self.nodo.buscar();
    }

    pub fn orbot_hint(&self) -> OrbotHint {
        OrbotHint {
            socks_host: "127.0.0.1".into(),
            socks_port: ORBOT_SOCKS,
            onion_sala: RENDEZVOUS_ONION.into(),
            pasos: vec![
                tr("Instalá Orbot y encendelo. No hace falta el modo VPN: Konstruado usa el proxy SOCKS de Orbot para la sala.", "Install Orbot and start it. VPN mode is not needed: Konstruado uses Orbot's SOCKS proxy for the room.").into(),
                tr("Dejá el proxy SOCKS de Orbot en 127.0.0.1:9050 (viene así).", "Leave Orbot's SOCKS proxy on 127.0.0.1:9050 (the default).").into(),
                tr("En Cuenta → Red tocá «Usar Orbot». El teléfono marca la sala horneada.", "In Account → Network tap “Use Orbot”. The phone dials the built-in room.").into(),
                tr("El nodo Monero no usa el SOCKS: Konstruado lo llama directo. Un nodo de tu red local o Tailscale (192.168.x, 10.x, 100.x) va directo y nunca por Tor; un nodo público también va directo (ve tu IP), salvo que la VPN de Orbot capture a Konstruado, y ahí va por Tor.", "The Monero node does not use the SOCKS proxy: Konstruado calls it directly. A node on your LAN or Tailscale (192.168.x, 10.x, 100.x) is always direct, never over Tor; a public node is also direct (it sees your IP), unless Orbot's VPN captures Konstruado, and then it goes over Tor.").into(),
                tr("Si usás la VPN de Orbot con un nodo local, dejá Konstruado afuera: en «Elegir aplicaciones» marcá otra app y no Konstruado (sin ninguna marcada Orbot captura todo el teléfono), o usá «Modo de usuarie avanzado» (solo SOCKS, sin VPN).", "If you use Orbot's VPN with a local node, leave Konstruado out: in “Choose apps” tick another app and not Konstruado (with none ticked Orbot captures the whole phone), or use “Power user mode” (SOCKS only, no VPN).").into(),
                tr("La sala la hospeda una PC: el mandante de escritorio, o `konstruado-sala`.", "A PC hosts the room: the desktop client, or `konstruado-sala`.").into(),
                tr("Dos teléfonos se hablan a través de esa PC (relay).", "Two phones talk through that PC (relay).").into(),
            ],
        }
    }

    // ------------------------------------------------------------ tablero

    pub fn tablero(&self) -> TableroVista {
        let mid = self.mid();
        let sec = self.sec();
        let soy_m = self.ses.lock().unwrap().rol == Some(Rol::Mandante);
        // Incluye archivadas: así una obra archivada no hace reaparecer su oferta.
        let todas = self.nodo.obras_todas();
        let ofertas = self.nodo.tablero();
        let mut mias: Vec<&Oferta> = ofertas
            .iter()
            .filter(|o| o.mandante.id == mid && oferta_en_tablero(&o.id, &todas))
            .collect();
        mias.sort_by(|a, b| b.actualizado.cmp(&a.actualizado).then(b.id.cmp(&a.id)));
        let mut ajenas: Vec<&Oferta> = ofertas
            .iter()
            .filter(|o| o.mandante.id != mid && oferta_en_tablero(&o.id, &todas))
            .collect();
        ajenas.sort_by(|a, b| b.actualizado.cmp(&a.actualizado).then(b.id.cmp(&a.id)));
        let mut mis_obras: Vec<Obra> = self
            .nodo
            .obras()
            .into_iter()
            .filter(|o| {
                o.participa(&mid)
                    && !matches!(
                        o.estado,
                        EstadoObra::Rechazada | EstadoObra::Abandonada | EstadoObra::Cerrada
                    )
            })
            .collect();
        mis_obras.sort_by(|a, b| b.actualizado.cmp(&a.actualizado).then(b.id.cmp(&a.id)));
        let red = self.red_interna();
        // Antes decía siempre "Encendé Orbot" con cero pares, aunque el SOCKS
        // anduviera. Ahora usa el diagnóstico medido de Orbot → sala.
        let pista = if red.pares == 0 {
            if red.sala.tipo == "conectado" {
                tr("Conectado a la sala. Nadie más todavía.", "Connected to the room. Nobody else yet.").to_string()
            } else {
                format!("{}. {}", red.sala.titulo, red.sala.detalle)
            }
        } else if soy_m {
            tr("Publicá una obra; el contratista la ve en su tablero.", "Post a job; the contractor sees it on their board.").to_string()
        } else if red.otros.is_empty() {
            tr("No hay avisos. El mandante tiene que publicar; podés tocar Buscar ofertas.", "No offers. The client has to post one; you can tap Look for offers.").to_string()
        } else {
            tf!(
                "{} está en la red. Si no ves el aviso, tocá Buscar ofertas.", "{} is on the network. If you do not see the offer, tap Look for offers.",
                red.otros.join(", ")
            )
        };
        TableroVista {
            avisos: avisos_para(&mid, &mis_obras, &sec),
            mis_ofertas: mias.iter().map(|o| oferta_vista(o, &mid)).collect(),
            ofertas: ajenas.iter().map(|o| oferta_vista(o, &mid)).collect(),
            obras: mis_obras
                .iter()
                .map(|o| ObraFila {
                    id: o.id.clone(),
                    nombre: o.nombre.clone(),
                    estado: estado_obra_txt(o.estado),
                    estado_label: l().label_estado(o.estado).into(),
                    en_curso: obra_en_curso(o.estado),
                    con_quien: if o.mandante.id == mid {
                        tf!("con {}", "with {}", o.contratista.nombre)
                    } else {
                        tf!("con {}", "with {}", o.mandante.nombre)
                    },
                })
                .collect(),
            pista,
            red,
        }
    }

    pub fn oferta(&self, oferta_id: String) -> Result<OfertaVista, FfiError> {
        let mid = self.mid();
        self.nodo
            .tablero()
            .iter()
            .find(|o| o.id == oferta_id)
            .map(|o| oferta_vista(o, &mid))
            .ok_or_else(|| fallo(tr("Esa oferta ya no está en el tablero.", "That offer is no longer on the board.")))
    }

    pub fn publicar_oferta(
        &self,
        nombre: String,
        trabajo: String,
        garantia: String,
        detalles: Vec<String>,
    ) -> Result<OfertaVista, FfiError> {
        let m = self.yo()?;
        // Obras nuevas en dólares (centavos).
        let t = leer_usd(&trabajo).map_err(err_core)?;
        let g = leer_usd(&garantia).map_err(|_| err_core(konstruado_core::Error::Garantia))?;
        let mut o = Oferta::publicar_usd(m, nombre, t, g, detalles).map_err(err_core)?;
        o.sellar_retiro(&self.sec());
        let v = oferta_vista(&o, &self.mid());
        let _e = self.rt.enter();
        self.nodo.publicar(o);
        Ok(v)
    }

    /// Cuántas partidas salen con esa garantía, y si es contra.
    pub fn previa_aceptar(&self, oferta_id: String, garantia: String) -> PreviaAceptar {
        let Some(o) = self.nodo.tablero().into_iter().find(|o| o.id == oferta_id) else {
            return PreviaAceptar {
                ok: false,
                contra: false,
                n_partidas: 0,
                texto: tr("Esa oferta ya no está.", "That offer is gone.").into(),
                detalles: vec![],
            };
        };
        let g = leer(o.moneda, &garantia);
        let contra = g != o.garantia_sugerida;
        let gtxt = if o.moneda.es_usd() { caja::texto_usd_aprox(es(), g) } else { mm(o.moneda, g) };
        match n_partidas(o.trabajo, g) {
            Ok(n) => {
                let mut d = o.detalles.clone();
                d.resize(n as usize, String::new());
                PreviaAceptar {
                    ok: true,
                    contra,
                    n_partidas: n,
                    texto: if contra {
                        tf!("Contra: {n} partidas de {gtxt}. El mandante tiene que confirmar.", "Counter: {n} stages of {gtxt}. The client has to confirm.")
                    } else {
                        tf!("Aceptás {n} partidas. Los dos encierran {gtxt} en cada una.", "You accept {n} stages. Both lock {gtxt} in each one.")
                    },
                    detalles: d,
                }
            }
            Err(e) => PreviaAceptar {
                ok: false,
                contra,
                n_partidas: 0,
                texto: l().error(&e),
                detalles: vec![],
            },
        }
    }

    /// Aceptar (o contra si cambia la garantía). Devuelve el id de la obra.
    pub fn aceptar_oferta(
        &self,
        oferta_id: String,
        garantia: String,
        detalles: Vec<String>,
    ) -> Result<String, FfiError> {
        let c = self.yo()?;
        let oferta = self
            .nodo
            .tablero()
            .into_iter()
            .find(|o| o.id == oferta_id)
            .ok_or_else(|| fallo(tr("Esa oferta ya no está en el tablero.", "That offer is no longer on the board.")))?;
        let g = leer(oferta.moneda, &garantia);
        let contra = g != oferta.garantia_sugerida;
        let dets = if contra { detalles } else { oferta.detalles.clone() };
        let acc = Aceptacion::de_con(&oferta, c.clone(), g, dets).map_err(err_core)?;
        let obra = Obra::desde_oferta(oferta, acc).map_err(err_core)?;
        let id = obra.id.clone();
        let _e = self.rt.enter();
        self.publicar_trato(obra, &c)?;
        self.nodo.quitar(&id);
        Ok(id)
    }

    // ------------------------------------------------------------ obra

    pub fn obra_vista(&self, obra_id: String) -> Result<ObraVista, FfiError> {
        let obra = self.obra(&obra_id)?;
        let mid = self.mid();
        if !obra.participa(&mid) {
            return Err(fallo(tr("Esta obra es de otras dos personas.", "This job belongs to two other people.")));
        }
        let sec = self.sec();
        let v = self.caja.vista();
        let soy_m = obra.mandante.id == mid;
        let soy_c = obra.contratista.id == mid;
        let estado = obra.estado;
        let abierta = !matches!(
            estado,
            EstadoObra::Cerrada | EstadoObra::Rechazada | EstadoObra::Abandonada
        );
        let contra = estado == EstadoObra::Contra;
        let extra = obra.extra.as_ref().map(|ex| {
            let texto = match obra.leer_extra(&sec) {
                TextoLeido::Plano(t) => t,
                TextoLeido::Cerrado => tr("Texto cifrado", "Encrypted text").into(),
            };
            ExtraVista {
                texto,
                mia: ex.por.id == mid,
                monto: mm(obra.moneda, ex.monto),
                por: ex.por.nombre.clone(),
            }
        });
        let caja_dir = v.caja_de(&obra.id).map(|s| s.to_string());
        let activa = obra.activa();
        Ok(ObraVista {
            id: obra.id.clone(),
            nombre: obra.nombre.clone(),
            estado: estado_obra_txt(estado),
            estado_label: l().label_estado(estado).into(),
            resumen: tf!(
                "Mandante {} · contratista {} · {} partidas · trabajo {}", "Client {} · contractor {} · {} stages · job {}",
                obra.mandante.nombre,
                obra.contratista.nombre,
                obra.n_partidas,
                mm(obra.moneda, obra.trabajo)
            ),
            soy_mandante: soy_m,
            soy_contratista: soy_c,
            abierta,
            sincronizando: abierta && self.sincronizando(&obra),
            contra,
            contra_texto: contra.then(|| {
                tf!(
                    "El contratista propone garantía {} ({} partidas).", "The contractor proposes a guarantee of {} ({} stages).",
                    mm(obra.moneda, obra.garantia),
                    obra.n_partidas
                )
            }),
            abandonada: estado == EstadoObra::Abandonada,
            cierre_mio: obra.cierre.as_ref().is_some_and(|c| c.id == mid),
            cierre_de: obra
                .cierre
                .as_ref()
                .filter(|c| c.id != mid)
                .map(|c| c.nombre.clone()),
            hay_riesgo: obra.hay_riesgo(),
            puede_extra: abierta && extra.is_none() && !contra && (soy_m || soy_c),
            extra: if abierta { extra } else { None },
            armando_caja: caja_dir.is_none()
                && matches!(estado, EstadoObra::Acordada | EstadoObra::EnMarcha),
            mirada: mirada_de(&v, &obra.id),
            usd: obra.moneda.es_usd(),
            caja_direccion: caja_dir,
            partidas: obra
                .partidas
                .iter()
                .enumerate()
                .map(|(i, p)| PartidaFila {
                    indice: i as u32,
                    titulo: l().titulo_partida(i, &p.detalle),
                    label: l().label_partida(p),
                    estado: estado_partida_txt(p.estado),
                    por_lado: tf!("{} por lado", "{} per side", mm(obra.moneda, p.capital(obra.garantia))),
                    saldo_corto: caja::saldo_corto(
                        es(),
                        p.estado,
                        obra.piconero_partida(i),
                        p.fondeo_txid.is_some(),
                    ),
                    activa: activa == Some(i),
                    traba_corta: caja::traba_corta(v.traba(&obra, i), es()),
                })
                .collect(),
        })
    }

    pub fn confirmar_contra(&self, obra_id: String) -> Result<(), FfiError> {
        let mid = self.mid();
        self.accion(&obra_id, true, |o, _| o.confirmar_contra(&mid))
    }

    /// No acepta la garantía del contratista: la oferta vuelve al tablero.
    pub fn rechazar_contra(&self, obra_id: String) -> Result<(), FfiError> {
        let mut obra = self.obra(&obra_id)?;
        let q = self.exigir_sesion(&obra)?;
        obra.rechazar_contra(&q.id).map_err(err_core)?;
        let gpub = if obra.garantia_publicada > 0 {
            obra.garantia_publicada
        } else {
            obra.garantia
        };
        let dets: Vec<String> = obra.partidas.iter().map(|p| p.detalle.clone()).collect();
        let mut oferta = Oferta::publicar(
            obra.mandante.clone(),
            obra.nombre.clone(),
            obra.trabajo,
            gpub,
            dets,
        )
        .map_err(err_core)?;
        oferta.id = obra.id.clone();
        oferta.sellar_retiro(&self.sec());
        let _e = self.rt.enter();
        self.publicar_trato(obra, &q)?;
        self.nodo.publicar(oferta);
        Ok(())
    }

    pub fn aceptar_cierre(&self, obra_id: String) -> Result<(), FfiError> {
        self.accion(&obra_id, true, |o, q| o.aceptar_cierre(q))
    }

    pub fn rechazar_cierre(&self, obra_id: String) -> Result<(), FfiError> {
        self.accion(&obra_id, true, |o, q| o.rechazar_cierre(q))
    }

    /// Sin riesgo abandona; con partidas encerradas propone cierre (el otro acepta).
    pub fn abandonar(&self, obra_id: String) -> Result<(), FfiError> {
        let obra = self.obra(&obra_id)?;
        let exigir = obra.hay_riesgo();
        self.accion(&obra_id, exigir, |o, q| o.abandonar(q))
    }

    pub fn proponer_extra(&self, obra_id: String, texto: String, monto_lado: String) -> Result<(), FfiError> {
        if texto.trim().is_empty() {
            return Err(fallo(tr("La extra necesita un texto.", "The extra needs a description.")));
        }
        let moneda = self.obra(&obra_id)?.moneda;
        let m = leer(moneda, &monto_lado);
        if m == 0 {
            return Err(fallo(tr("La extra lleva un monto mayor a cero.", "The extra needs an amount above zero.")));
        }
        self.accion(&obra_id, true, |o, q| o.proponer_extra(q, texto, m))
    }

    pub fn aceptar_extra(&self, obra_id: String) -> Result<(), FfiError> {
        let sec = self.sec();
        self.accion(&obra_id, true, |o, q| {
            o.abrir_extra(&sec)?;
            o.aceptar_extra(q)
        })
    }

    pub fn rechazar_extra(&self, obra_id: String) -> Result<(), FfiError> {
        self.accion(&obra_id, true, |o, q| o.rechazar_extra(q))
    }

    // ------------------------------------------------------------ partida

    pub fn partida_vista(&self, obra_id: String, indice: u32) -> Result<PartidaVista, FfiError> {
        let obra = self.obra(&obra_id)?;
        let i = indice as usize;
        let p = obra
            .partidas
            .get(i)
            .cloned()
            .ok_or_else(|| fallo(tr("No está esa partida.", "That stage does not exist.")))?;
        let mid = self.mid();
        if !obra.participa(&mid) {
            return Err(fallo(tr("Esta obra es de otras dos personas.", "This job belongs to two other people.")));
        }
        let sec = self.sec();
        let v = self.caja.vista();
        let soy_m = obra.mandante.id == mid;
        let soy_c = obra.contratista.id == mid;
        let espera_a = match p.turno {
            Some(Rol::Mandante) if !soy_m => Some(obra.mandante.nombre.clone()),
            Some(Rol::Contratista) if !soy_c => Some(obra.contratista.nombre.clone()),
            _ => None,
        };
        let garantia = obra.garantia;
        let activa = obra.activa() == Some(i);
        let contra = obra.estado == EstadoObra::Contra;
        let cortada = matches!(
            obra.estado,
            EstadoObra::Abandonada | EstadoObra::Cerrada | EstadoObra::Rechazada
        );
        let soy_prop_enc = p.encerrado_por.as_ref().is_some_and(|q| q.id == mid);
        let fondeo_curso = v.linea(&obra.id, i).cloned();
        // Una sola regla para Dioxus y Compose: qué botones existen en cada estado.
        let acc = caja::acciones_partida_con(&obra, i, &mid, &v.lineas_de(&obra.id, i), v.traba(&obra, i));
        let frenado = acc.frenado;
        let nada_en_curso = acc.en_curso == caja::EnCurso::Nada;
        let pagando = matches!(acc.en_curso, caja::EnCurso::PagoFirmando | caja::EnCurso::PagoEnRed);
        let saldo = caja::saldo_partida(
            es(),
            p.estado,
            obra.piconero_partida(i),
            p.fondeo_txid.is_some(),
            &obra.mandante.nombre,
            &obra.contratista.nombre,
        );
        let usd = obra.moneda.es_usd();
        let xmr_por_lado = if saldo.is_none() && !usd {
            caja::xmr_partida(es(), &obra, i)
        } else {
            None
        };
        let xmr_partida = if usd { caja::xmr_partida(es(), &obra, i) } else { None };
        let precio_propuesto = p
            .precio
            .as_ref()
            .filter(|_| p.estado == PartidaEstado::Encerrando && !soy_prop_enc)
            .map(|pr| tf!("Precio que propone: cada lado pone {}. Confirmar acepta ese precio.", "Proposed price: each side puts in {}. Confirming accepts that price.", caja::texto_precio_fijado(es(), pr)));
        let aviso_precio = precio_propuesto
            .as_ref()
            .and(p.precio.as_ref())
            .and_then(|pr| caja::aviso_diferencia_precio(es(), pr));
        let estado_precio = (usd && p.precio.is_none() && p.estado == PartidaEstado::Pendiente)
            .then(|| caja::estado_cotizacion(es()));
        let notas = p
            .notas
            .iter()
            .map(|n| {
                let cabeza = format!(
                    "{} · {}% · {}",
                    n.autor_nombre,
                    n.porcentaje,
                    l().fmt_cuando(n.cuando)
                );
                match obra.leer_nota(n, &sec) {
                    TextoLeido::Plano(t) => NotaVista { cabeza, cuerpo: t, cifrada: false },
                    TextoLeido::Cerrado => NotaVista {
                        cabeza,
                        cuerpo: tr("Nota cifrada", "Encrypted note").into(),
                        cifrada: true,
                    },
                }
            })
            .collect();
        let pendiente = !cortada && p.estado == PartidaEstado::Pendiente;
        let encerrando = !cortada && p.estado == PartidaEstado::Encerrando;
        let pista = if pendiente && contra {
            Some(tr("Primero hay que confirmar la contra de la obra.", "The job's counteroffer has to be confirmed first.").to_string())
        } else if pendiente && !activa {
            Some(tr("Todavía no toca. Cerrá la partida que está en curso.", "Not yet. Close the stage in progress first.").to_string())
        } else if pendiente {
            Some(tr("Los dos tienen que confirmar el encierre. El otro tiene que estar en línea.", "Both have to confirm the lock. The other person has to be online.").to_string())
        } else if encerrando && soy_prop_enc && !frenado && nada_en_curso {
            Some(tr("Esperando que el otro confirme el encierre.", "Waiting for the other person to confirm the lock.").to_string())
        } else if encerrando && acc.confirmar_fondeo {
            Some(tr("El otro quiere encerrar esta partida. Confirmar arma una sola transacción con los dos.", "The other person wants to lock this stage. Confirming builds one transaction with both.").to_string())
        } else if encerrando && acc.en_curso == caja::EnCurso::FondeoEnRed {
            Some(tr("El fondeo ya está en la red. No se puede cancelar; se encierra solo cuando entra en un bloque.", "The funding is already on the network. It cannot be cancelled; the stage locks once it is in a block.").to_string())
        } else if !cortada && p.estado == PartidaEstado::EnTrato && pagando {
            Some(tf!(
                "Pago del {}% en curso. No hace falta volver a aceptar; se cierra cuando la transacción entra en un bloque.", "Payment of {}% in progress. No need to accept again; it closes when the transaction is in a block.",
                p.propuesto.unwrap_or(0)
            ))
        } else if !cortada && p.estado == PartidaEstado::Encerrada && soy_m {
            Some(tr("El contratista avisa cuando termina y propone cuánto se paga.", "The contractor reports when finished and proposes how much is paid.").to_string())
        } else {
            None
        };
        let en_trato = !cortada && p.estado == PartidaEstado::EnTrato;
        Ok(PartidaVista {
            obra_id: obra.id.clone(),
            obra_nombre: obra.nombre.clone(),
            indice,
            titulo: l().titulo_partida(i, &p.detalle),
            label: l().label_partida(&p),
            estado: estado_partida_txt(p.estado),
            lead: tf!(
                "{} por lado. Mandante {} · contratista {}", "{} per side. Client {} · contractor {}",
                mm(obra.moneda, p.capital(garantia)),
                obra.mandante.nombre,
                obra.contratista.nombre
            ),
            saldo_estado: saldo.as_ref().map(|s| s.estado.clone()),
            saldo_detalle: saldo.as_ref().map(|s| s.detalle.clone()),
            candado: saldo.as_ref().and_then(|s| s.candado.clone()),
            xmr_por_lado,
            caja_direccion: v.caja_de(&obra.id).map(|s| s.to_string()),
            fondeo_txid: p.fondeo_txid.clone(),
            pago_txid: p.pago_txid.clone(),
            linea: fondeo_curso.as_ref().map(|t| t.mostrar(es())),
            linea_freno: frenado,
            sincronizando: !cortada && self.sincronizando(&obra),
            cortada,
            detalle: p.detalle.clone(),
            puede_editar: acc.editar_texto,
            recibo: if p.estado == PartidaEstado::Pagada {
                p.recibo.as_ref().map(|r| {
                    tf!(
                        "Recibo · {} · pagó {}% · {} · aceptó {} · {}", "Receipt · {} · paid {}% · {} · accepted by {} · {}",
                        r.titulo,
                        r.porcentaje,
                        mm(obra.moneda, r.monto),
                        r.acepto_nombre,
                        l().fmt_cuando(r.cuando)
                    )
                })
            } else {
                None
            },
            cerrado_texto: (p.estado == PartidaEstado::Pagada && p.recibo.is_none()).then(|| {
                tf!(
                    "Cerró al {}% ({}). El hilo quedó guardado.", "Closed at {}% ({}). The thread is kept.",
                    p.pago.unwrap_or(0),
                    mm(obra.moneda, monto_pct(garantia, p.pago.unwrap_or(0)))
                )
            }),
            encerro: p
                .encerrado_por
                .as_ref()
                .map(|q| tf!("Encerró {} · {}", "Locked by {} · {}", q.nombre, l().fmt_cuando(p.encerrado_cuando))),
            notas,
            pista,
            // Todo sale de `caja::acciones_partida`, igual que en el escritorio.
            // `puede_reintentar_fondeo` queda como alias de UI (Android ORs ambos).
            puede_proponer_encerrar: acc.proponer_encierre,
            puede_cancelar_propuesta: acc.cancelar_propuesta,
            puede_reintentar_fondeo: acc.empezar_fondeo_de_nuevo,
            puede_empezar_fondeo_de_nuevo: acc.empezar_fondeo_de_nuevo,
            puede_confirmar_fondear: acc.confirmar_fondeo,
            puede_no_encerrar: acc.no_encerrar,
            puede_avisar_termino: acc.avisar_termino,
            en_trato,
            propuesto: if en_trato { p.propuesto } else { None },
            propuesto_texto: if en_trato {
                p.propuesto
                    .map(|n| tf!("Sobre la mesa: {n}% ({}).", "On the table: {n}% ({}).", mm(obra.moneda, monto_pct(garantia, n))))
            } else {
                None
            },
            mi_turno: en_trato && acc.me_toca,
            espera_a: if en_trato { espera_a } else { None },
            max_nota: MAX_NOTA as u32,
            puede_aceptar_pago: acc.aceptar_pago,
            puede_contraofertar: acc.contraofertar,
            puede_salir_local: acc.salir_local,
            en_curso: caja::en_curso_corto(acc.en_curso, es()).map(str::to_string),
            pago_en_curso: pagando,
            traba: caja::texto_traba(acc.traba, soy_c, es()),
            traba_corta: caja::traba_corta(acc.traba, es()),
            termino_trabado: !cortada && soy_c && p.estado == PartidaEstado::Encerrada && acc.traba.trabada(),
            pago_trabado: en_trato && acc.me_toca && !pagando && acc.traba.trabada(),
            xmr_partida,
            precio_propuesto,
            aviso_precio,
            estado_precio,
        })
    }

    pub fn editar_detalle(&self, obra_id: String, indice: u32, texto: String) -> Result<(), FfiError> {
        self.accion(&obra_id, false, |o, q| o.editar_detalle(indice as usize, q, texto))
    }

    pub fn proponer_encerrar(&self, obra_id: String, indice: u32) -> Result<(), FfiError> {
        // Obras en USD: el XMR queda fijo con el precio de ahora (el otro lo acepta al fondear).
        let obra = self.obra(&obra_id)?;
        let precio = caja::precio_para_encerrar(&obra, indice as usize).map_err(err_caja)?;
        self.accion(&obra_id, true, |o, q| o.encerrar_proponer_con(indice as usize, q, precio))
    }

    // ------------------------------------------------------------ precio USD/XMR

    /// Pide el precio ahora (Kraken, Bitfinex, CoinGecko y CoinPaprika en
    /// paralelo), por Orbot si está configurado. Sin Tor solo si el usuario lo
    /// permitió siempre. Bloquea hasta ~8 s (16 s si reintenta sin Tor).
    pub fn actualizar_precio(&self) -> PrecioVista {
        let socks = self.nodo.socks();
        let _ = self.rt.block_on(konstruado_motor::cotizacion::actualizar(socks));
        self.precio_vista()
    }

    /// Reintenta sin Tor porque el usuario lo pidió (la API ve su IP).
    pub fn actualizar_precio_sin_tor(&self) -> PrecioVista {
        let _ = self.rt.block_on(konstruado_motor::cotizacion::actualizar_directo());
        self.precio_vista()
    }

    /// Último recurso: precio de 1 XMR en dólares escrito a mano.
    pub fn fijar_precio_manual(&self, texto: String) -> Result<PrecioVista, FfiError> {
        caja::precio_manual(es(), &texto).map_err(|e| fallo(&e))?;
        Ok(self.precio_vista())
    }

    /// Permitir siempre pedir el precio sin Tor si por Tor no hay (por defecto no).
    pub fn precio_sin_tor_siempre(&self) -> bool {
        konstruado_motor::cotizacion::directo_siempre()
    }

    pub fn fijar_precio_sin_tor_siempre(&self, si: bool) {
        konstruado_motor::cotizacion::fijar_directo_siempre(si);
    }

    /// Estado del precio para el panel (sin red; el bucle de fondo lo mantiene).
    pub fn precio_vista(&self) -> PrecioVista {
        use konstruado_motor::cotizacion as c;
        PrecioVista {
            texto: caja::estado_cotizacion(es()),
            listo: caja::precio_listo(),
            error: c::ultima_falla().map(|f| f.texto(es())),
            ofrecer_sin_tor: c::ofrecer_directo(),
            aviso_sin_tor: caja::aviso_sin_tor(es()).to_string(),
            por_tor: self.nodo.socks().is_some(),
        }
    }

    /// Estado del precio de referencia (último conocido o por qué no hay).
    pub fn estado_precio(&self) -> String {
        let _e = self.rt.enter();
        konstruado_motor::cotizacion::refrescar_en_fondo(self.nodo.socks());
        caja::estado_cotizacion(es())
    }

    /// Nota fija: montos en USD, precio de mainnet como referencia en stagenet.
    pub fn nota_precio(&self) -> String {
        caja::nota_precio_stagenet(es())
    }

    /// Vista previa del formulario de publicar (en USD).
    pub fn previa_publicar(&self, trabajo: String, garantia: String) -> PreviaPublicar {
        let t = leer_usd(&trabajo).unwrap_or(0);
        let g = leer_usd(&garantia).unwrap_or(0);
        match n_partidas(t, g) {
            Ok(n) => PreviaPublicar {
                ok: true,
                n_partidas: n,
                texto: tf!("{n} partidas. En cada una los dos encierran {}.", "{n} stages. In each one both lock {}.", caja::texto_usd_aprox(es(), g)),
            },
            Err(e) => PreviaPublicar { ok: false, n_partidas: 0, texto: l().error(&e) },
        }
    }

    /// "Cancelar propuesta" / "No encerrar".
    pub fn cancelar_encerrar(&self, obra_id: String, indice: u32) -> Result<(), FfiError> {
        self.accion(&obra_id, true, |o, q| o.encerrar_cancelar(indice as usize, q))
    }

    /// "Confirmar y fondear": arma el CLSAG cooperativo de los dos lados.
    /// La partida pasa a Encerrada recién cuando el motor ve la transacción.
    pub fn confirmar_y_fondear(&self, obra_id: String, indice: u32) -> Result<(), FfiError> {
        let obra = self.obra(&obra_id)?;
        let q = self.exigir_sesion(&obra)?;
        let _e = self.rt.enter();
        self.caja.pedir_fondeo(&obra, indice as usize, &q).map_err(err_caja)
    }

    pub fn reintentar_fondeo(&self, obra_id: String, indice: u32) -> Result<(), FfiError> {
        self.empezar_fondeo_de_nuevo(obra_id, indice)
    }

    /// «Empezar el fondeo de nuevo»: aborta sesión CLSAG en ambos lados y pide decoys frescos.
    pub fn empezar_fondeo_de_nuevo(&self, obra_id: String, indice: u32) -> Result<(), FfiError> {
        let obra = self.obra(&obra_id)?;
        let q = self.exigir_sesion(&obra)?;
        let _e = self.rt.enter();
        self.caja
            .empezar_fondeo_de_nuevo(&obra, indice as usize, &q, &self.nodo)
            .map_err(err_caja)
    }

    pub fn avisar_termino(&self, obra_id: String, indice: u32, pct: String, nota: String) -> Result<(), FfiError> {
        let pct = parse_num(&pct) as u32;
        let nota = recorta_nota(nota);
        self.accion(&obra_id, true, |o, q| o.avisar_termino(indice as usize, q, pct, nota))
    }

    /// "Aceptar X% y pagar": sesión de gasto FROST 2-de-2 y broadcast.
    /// La partida pasa a Pagada recién cuando el motor ve el pago.
    pub fn aceptar_y_pagar(&self, obra_id: String, indice: u32) -> Result<(), FfiError> {
        let obra = self.obra(&obra_id)?;
        let q = self.exigir_sesion(&obra)?;
        let _e = self.rt.enter();
        self.caja.pedir_gasto(&obra, indice as usize, &q).map_err(err_caja)
    }

    pub fn contra_pago(&self, obra_id: String, indice: u32, pct: String, nota: String) -> Result<(), FfiError> {
        let pct = parse_num(&pct) as u32;
        let nota = recorta_nota(nota);
        self.accion(&obra_id, true, |o, q| o.contra_pago(indice as usize, q, pct, nota))
    }

    // ------------------------------------------------------------ billetera

    pub fn billetera(&self) -> BilleteraVista {
        let v = self.caja.vista();
        let b = &v.billetera;
        let est = caja::estado_billetera(b, v.personal.is_some(), v.tip, es());
        let obras = self.nodo.obras();
        let cajas = v
            .cajas
            .iter()
            .map(|(id, addr)| CajaFila {
                obra_id: id.clone(),
                obra_nombre: obras
                    .iter()
                    .find(|o| &o.id == id)
                    .map(|o| o.nombre.clone())
                    .unwrap_or_else(|| id.clone()),
                direccion: addr.clone(),
                mirada: mirada_de(&v, id),
            })
            .collect();
        BilleteraVista {
            daemon: xmr_joint::daemon_url(),
            tip: v.tip.map(|t| t as u64),
            direccion: v.personal.clone(),
            tiene_semilla: v.tiene_semilla,
            total: caja::fmt_xmr(b.total),
            libre: caja::fmt_xmr(b.libre),
            trabado: caja::fmt_xmr(b.trabado),
            visto: match (b.desde, b.hasta) {
                (Some(d), Some(h)) => tf!("Visto desde el bloque {d} hasta el {h}.", "Scanned from block {d} to {h}."),
                _ => tr("Todavía no miré la cadena. Arranco por los últimos 40 bloques.", "The chain has not been scanned yet. Starting from the last 40 blocks.").into(),
            },
            buscando: b.buscando,
            enviando: b.enviando,
            retro: (b.retro > 0).then(|| tf!("Quedan {} bloques por mirar hacia atrás.", "{} blocks left to scan backwards.", b.retro)),
            aviso: b.aviso.as_ref().map(|a| caja::aviso_humano(a, es())),
            ultimo: b.ultimo.as_ref().map(|tx| {
                tf!(
                    "Último envío {tx}. Fee {} XMR. Cambio {} XMR, vuelve en el próximo bloque.", "Last send {tx}. Fee {} XMR. Change {} XMR, back in the next block.",
                    caja::fmt_xmr(b.ultimo_fee.unwrap_or(0)),
                    caja::fmt_xmr(b.ultimo_cambio.unwrap_or(0))
                )
            }),
            movs: b
                .movs
                .iter()
                .map(|m| MovVista {
                    monto: format!("{} XMR", caja::fmt_xmr(m.monto)),
                    detalle: tf!(
                        "bloque {} · {}", "block {} · {}",
                        m.altura,
                        if m.libre { tr("libre", "free") } else { tr("trabado", "locked") }
                    ),
                })
                .collect(),
            escala: caja::estado_cotizacion(es()),
            cajas,
            estado_linea: est.1,
            estado_tono: est.0.codigo().into(),
            total_pico: b.total,
            ayuda_envio: caja::ayuda_envio(es()).to_string(),
        }
    }

    pub fn crear_semilla(&self) -> Result<String, FfiError> {
        let _e = self.rt.enter();
        self.caja.crear_semilla().map_err(err_caja)
    }

    // ------------------------------------------------ respaldo completo (respaldo.rs)

    pub fn estado_respaldo(&self) -> RespaldoEstado {
        let mid = self.mid();
        let (sem, cajas) = self.caja.claves_respaldo();
        let obras = self.nodo.obras_todas();
        let est = respaldo::estado(&self.datos, &respaldo::huella_de_partes(&mid, &obras, sem, cajas));
        let (tono, linea) = respaldo::texto_estado(&est, l());
        RespaldoEstado {
            linea,
            tono: tono.codigo().into(),
            falta: est.falta && est.hay_algo,
            ultimo: est.ultimo.map(|t| l().fmt_cuando(t)),
            ayuda: respaldo::ayuda(es()).into_iter().map(str::to_string).collect(),
            nombre_archivo: respaldo::nombre_archivo(),
            clave_minima: respaldo::CLAVE_MINIMA as u32,
        }
    }

    /// Arma y cifra el respaldo completo. Android lo escribe con el selector (SAF)
    /// y después llama a [`Self::respaldo_guardado`].
    pub fn exportar_respaldo(&self, clave: String) -> Result<Vec<u8>, FfiError> {
        self.persistir();
        let ahora = chrono::Utc::now().timestamp();
        let (bytes, h) = respaldo::exportar(&self.datos, persist::cargar(), self.caja.material_respaldo(), &clave, ahora)
            .map_err(|e| fallo(respaldo::aviso(&e, es())))?;
        *self.respaldo_pendiente.lock().unwrap() = Some(h);
        Ok(bytes)
    }

    /// El archivo quedó escrito: anota la fecha del último respaldo.
    pub fn respaldo_guardado(&self) -> Result<(), FfiError> {
        let h = self.respaldo_pendiente.lock().unwrap().take().ok_or_else(|| fallo(tr("No hay un respaldo armado.", "There is no backup ready.")))?;
        respaldo::marcar_hecho(&self.datos, h, chrono::Utc::now().timestamp()).map_err(fallo)
    }

    /// Descifra y valida sin escribir nada.
    pub fn revisar_respaldo(&self, datos: Vec<u8>, clave: String) -> Result<RespaldoResumen, FfiError> {
        let r = respaldo::revisar(&self.datos, &datos, &clave).map_err(|e| fallo(respaldo::aviso(&e, es())))?;
        Ok(RespaldoResumen {
            nombre: r.nombre,
            rol: l().rol(r.rol).to_string(),
            creado: l().fmt_cuando(r.creado),
            app: r.app,
            n_obras: r.n_obras as u32,
            n_ofertas: r.n_ofertas as u32,
            n_shares: r.n_shares as u32,
            direccion: r.direccion,
            altura: r.altura,
            hay_datos: r.hay_datos,
        })
    }

    /// Deja todo listo para el reinicio (atómico). Android reinicia el proceso
    /// enseguida; al arrancar, `nuevo` aplica el cambio antes de leer nada.
    pub fn restaurar_respaldo(&self, datos: Vec<u8>, clave: String, reemplazar: bool) -> Result<(), FfiError> {
        respaldo::preparar(&self.datos, &datos, &clave, reemplazar)
            .map(|_| ())
            .map_err(|e| fallo(respaldo::aviso(&e, es())))
    }

    /// Las 25 palabras de la billetera personal. Solo después de la advertencia
    /// en la UI. Android pone FLAG_SECURE y limpia el portapapeles.
    pub fn ver_semilla(&self) -> Result<SemillaVistaFfi, FfiError> {
        let v = self.caja.ver_semilla().map_err(err_caja)?;
        Ok(SemillaVistaFfi {
            palabras: v.palabras.as_str().to_string(),
            altura: v.altura,
            direccion: v.direccion,
            avisos: caja::aviso_ver_semilla(es()),
            aviso_copia: caja::aviso_copia_semilla(es()),
        })
    }

    /// Dirección + view key privada (solo lectura) de la billetera personal.
    pub fn llaves_billetera(&self) -> Option<LlavesBilleteraFfi> {
        self.caja.llaves_billetera().map(|l| LlavesBilleteraFfi {
            direccion: l.direccion,
            view_key: l.view_key,
            ayuda: caja::ayuda_view_key_billetera(es()).into(),
        })
    }

    /// Segundos tras los que Android (y el escritorio, si puede) borran el
    /// portapapeles si todavía tiene la semilla.
    pub fn semilla_portapapeles_seg(&self) -> u64 {
        caja::SEMILLA_PORTAPAPELES_SEG
    }

    /// Textos de la advertencia previa a mostrar las 25 palabras (sin leer la semilla).
    pub fn avisos_ver_semilla(&self) -> Vec<String> {
        caja::aviso_ver_semilla(es())
    }

    /// Importa obras/ofertas de un respaldo. No trae seed ni share; avisa que puede estar viejo.
    pub fn importar_obras(&self, texto: String) -> Result<String, FfiError> {
        let r = persist::importar_perfil_obras(&texto).map_err(fallo)?;
        let n_obras = r.obras.len();
        let n_ofertas = r.ofertas.len();
        let _e = self.rt.enter();
        for o in r.ofertas {
            self.nodo.publicar(o);
        }
        for mut obra in r.obras {
            self.nodo.olvidar_salida_obra(&obra.id);
            if let Ok(q) = self.yo() {
                if obra.participa(&q.id) {
                    let _ = obra.preparar_para_red(&q.id, &q.clave_pub, &self.sec());
                }
            }
            self.nodo.publicar_obra(obra);
        }
        drop(_e);
        self.persistir();
        Ok(tf!(
            "Importé {n_obras} obra(s) y {n_ofertas} oferta(s). El estado puede estar desfasado respecto al otro; la cadena y el share mandan para el dinero. Si tenés el share, recuperalo después.", "Imported {n_obras} job(s) and {n_ofertas} offer(s). The state may lag behind the other side; the chain and the share rule for money. If you have the share, restore it afterwards."
        ))
    }

    /// Archiva la obra solo en este equipo (oculta del tablero / Mis obras).
    /// Conserva obra+share en disco. No vacía la caja ni firma gasto.
    pub fn salir_obra_local(&self, obra_id: String) -> Result<String, FfiError> {
        self.archivar_obra_local(obra_id)
    }

    /// Archiva la obra conjunta solo aquí. No es «salir del trato» on-chain.
    pub fn archivar_obra_local(&self, obra_id: String) -> Result<String, FfiError> {
        let obra = self.obra(&obra_id)?;
        let yo = self.yo()?;
        if !obra.participa(&yo.id) {
            return Err(fallo(tr("Esta obra es de otras dos personas.", "This job belongs to two other people.")));
        }
        if matches!(
            obra.estado,
            EstadoObra::Abandonada | EstadoObra::Cerrada | EstadoObra::Rechazada
        ) {
            // Ya terminal: solo archivar/ocultar.
        }
        let _e = self.rt.enter();
        for i in 0..obra.partidas.len() {
            self.caja.cancelar_fondeo(&obra.id, i);
        }
        // Por si la oferta original sigue en el tablero local.
        self.nodo.quitar(&obra.id);
        self.nodo.archivar_obra_local(&obra.id);
        drop(_e);
        self.persistir();
        Ok(
            tr("Archivé la obra en este equipo. Ya no se ve en el tablero ni en Mis obras. No se movieron fondos; el share y el contexto quedan en disco.", "Archived the job on this device. It no longer shows on the board or in My jobs. No funds moved; the share and context stay on disk.")
                .into(),
        )
    }

    /// Retira una oferta propia que ningún contratista tomó. Deja una lápida que
    /// se replica: la oferta no vuelve con el gossip del otro y desaparece de su
    /// tablero también. Misma regla que el escritorio (`retirar_oferta` del core).
    pub fn quitar_mi_oferta(&self, oferta_id: String) -> Result<String, FfiError> {
        let yo = self.yo()?;
        let oferta = self
            .nodo
            .tablero()
            .into_iter()
            .find(|o| o.id == oferta_id)
            .ok_or_else(|| fallo(tr("Esa oferta ya no está en el tablero.", "That offer is no longer on the board.")))?;
        let retiro = retirar_oferta(&oferta, &yo.id, &self.sec(), &self.nodo.obras_todas())
            .map_err(err_core)?;
        let _e = self.rt.enter();
        self.nodo.retirar(retiro);
        drop(_e);
        self.persistir();
        Ok(tr("Quité la oferta. Tampoco va a aparecer en el tablero del contratista.", "Offer removed. It will not show on the contractor's board either.").into())
    }

    /// Cancela fondeo/propuesta de encierre de una partida solo en este equipo. No mueve fondos en cadena.
    pub fn salir_partida_local(&self, obra_id: String, indice: u32) -> Result<String, FfiError> {
        let mut obra = self.obra(&obra_id)?;
        let yo = self.yo()?;
        if !obra.participa(&yo.id) {
            return Err(fallo(tr("Esta obra es de otras dos personas.", "This job belongs to two other people.")));
        }
        let i = indice as usize;
        if obra.partidas.get(i).is_none() {
            return Err(fallo(tr("No está esa partida.", "That stage does not exist.")));
        }
        let _e = self.rt.enter();
        self.caja.cancelar_fondeo(&obra.id, i);
        let mut aviso = String::from(
            tr("Cancelé el fondeo local de esta partida. Los fondos ya en la caja 2-de-2 no se tocan.", "Cancelled this stage's local funding. Funds already in the 2-of-2 box are untouched."),
        );
        if obra.partidas[i].estado == PartidaEstado::Encerrando {
            match obra.encerrar_cancelar(i, &yo) {
                Ok(()) => {
                    let _ = self.publicar_trato(obra.clone(), &yo);
                    aviso.push_str(tr(" También volví la propuesta de encierre a Pendiente en este equipo.", " The lock proposal also went back to Pending on this device."));
                }
                Err(_) => {
                    aviso.push_str(tr(" La propuesta de encierre no se pudo revertir sola (hace falta el otro o ya no está Encerrando).", " The lock proposal could not be reverted alone (the other person is needed or it is no longer Locking)."));
                }
            }
        }
        let peer = otro_de(&obra, &yo.id);
        let cuerpo = indice.to_string();
        let _ = self.nodo.enviar_caja(
            &obra.id,
            &peer,
            &yo.id,
            "partida-salida",
            cuerpo.as_bytes(),
        );
        drop(_e);
        self.persistir();
        Ok(aviso)
    }

    pub fn restaurar_semilla(&self, texto: String) -> Result<String, FfiError> {
        let path = self.tmp("semilla-in.txt");
        std::fs::write(&path, texto).map_err(|e| fallo(e.to_string()))?;
        let _e = self.rt.enter();
        let r = self.caja.restaurar_semilla(&path);
        let _ = std::fs::remove_file(&path);
        r.map(|c| caja::listo_humano(c, es())).map_err(err_caja)
    }

    pub fn restaurar_share(&self, texto: String) -> Result<String, FfiError> {
        let yo = self.yo()?;
        let path = self.tmp("caja-in.share");
        std::fs::write(&path, texto).map_err(|e| fallo(e.to_string()))?;
        let obras = self.nodo.obras();
        let _e = self.rt.enter();
        let r = self.caja.restaurar_share(&path, &yo, &obras);
        let _ = std::fs::remove_file(&path);
        r.map(|c| caja::listo_humano(c, es())).map_err(err_caja)
    }

    /// View key de la caja (hex). Muestra movimientos; no gasta.
    pub fn view_key_caja(&self, obra_id: String) -> Option<String> {
        self.caja.view_de(&obra_id)
    }

    pub fn enviar(&self, destino: String, monto_xmr: String) -> Result<(), FfiError> {
        let _e = self.rt.enter();
        self.caja.pedir_envio(&destino, &monto_xmr).map_err(err_caja)
    }

    pub fn maximo_envio(&self) -> Option<String> {
        caja::maximo_envio(self.caja.vista().billetera.libre)
    }

    pub fn actualizar_saldo(&self) {
        self.caja.pedir_actualizacion();
    }

    pub fn mirar_atras(&self) {
        self.caja.pedir_atras();
    }

    pub fn mirar_atras_caja(&self, obra_id: String) -> Result<(), FfiError> {
        self.caja.pedir_atras_caja(&obra_id).map_err(err_caja)
    }

    // ------------------------------------------------------------ daemon Monero

    /// Nodo público de stagenet (fallback).
    pub fn daemon_por_defecto(&self) -> String {
        xmr_joint::STAGENET_DAEMON.to_string()
    }

    /// URL en uso ahora (propia o pública).
    pub fn daemon_activo(&self) -> String {
        xmr_joint::daemon_url()
    }

    pub fn daemon_es_defecto(&self) -> bool {
        xmr_joint::daemon_es_defecto()
    }

    /// Guarda y activa un nodo propio. Vacío o solo espacios = error (usá `usar_daemon_por_defecto`).
    pub fn fijar_daemon(&self, url: String) -> Result<String, FfiError> {
        let ok = persist::fijar_daemon_persistido(Some(&url)).map_err(fallo)?;
        self.caja.pedir_actualizacion();
        Ok(ok)
    }

    /// Vuelve al nodo público y borra la URL guardada.
    pub fn usar_daemon_por_defecto(&self) -> Result<String, FfiError> {
        let ok = persist::fijar_daemon_persistido(None).map_err(fallo)?;
        if let Ok(mut g) = self.ultima_prueba.lock() {
            *g = None;
        }
        self.caja.pedir_actualizacion();
        Ok(ok)
    }

    /// Pide la punta (get_info / tip) al nodo activo por RPC HTTP(S).
    ///
    /// No gasta monedas: solo mide si el daemon responde. Guarda el último
    /// resultado (éxito o fallo) para mostrarlo en Cuenta y Billetera.
    ///
    /// `vpn_captura`: lo que Kotlin ve en `ConnectivityManager` (la red por defecto
    /// de la app es una VPN, p. ej. Orbot en modo VPN). `None` = no se sabe.
    pub fn probar_daemon(&self, vpn_captura: Option<bool>) -> DaemonPrueba {
        let vpn = match vpn_captura {
            Some(true) => caja::VpnApp::Captura,
            Some(false) => caja::VpnApp::Ninguna,
            None => caja::VpnApp::Desconocida,
        };
        let r = self.rt.block_on(caja::probar_daemon_con_vpn(es(), vpn));
        let prueba = DaemonPrueba {
            ok: r.ok,
            url: r.url,
            tip: r.tip,
            ms: r.ms,
            mensaje: r.mensaje,
            local: r.local,
            ruta: r.ruta,
        };
        guardar_prueba(&self.ultima_prueba, prueba)
    }

    /// `true` si el nodo activo es de la red local / Tailscale (va directo, nunca por Tor).
    pub fn daemon_es_local(&self) -> bool {
        xmr_joint::daemon_es_local()
    }

    /// Aviso para mostrar antes de probar: nodo local + VPN capturando la app.
    pub fn aviso_vpn_daemon(&self, vpn_captura: bool) -> Option<String> {
        (vpn_captura && xmr_joint::daemon_es_local()).then(|| caja::pista_vpn_local(es()).to_string())
    }

    /// Último resultado de «Probar RPC del nodo» (éxito o fallo), si ya se probó.
    pub fn ultima_prueba_daemon(&self) -> Option<DaemonPrueba> {
        self.ultima_prueba.lock().ok().and_then(|g| g.clone())
    }

    /// Guarda ya (onPause).
    pub fn guardar(&self) {
        self.persistir();
    }
}

#[cfg(test)]
mod ffi_tests {
    use super::*;

    /// El idioma es global: los tests que leen textos no corren a la vez.
    static IDIOMA: std::sync::Mutex<()> = std::sync::Mutex::new(());
    fn idioma_fijo_en_test() -> std::sync::MutexGuard<'static, ()> {
        IDIOMA.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn con_socks_vivo_no_se_pide_encender_orbot() {
        let _g = idioma_fijo_en_test();
        aplicar_idioma("es");
        let s = Some("127.0.0.1:9050");
        // El caso de Felipe: Orbot en VPN por app (Konstruado afuera), SOCKS OK, PC apagado.
        let e = sala_de(&DiagSocks::DestinoNoResponde("onion sin respuesta".into()), s, 0, false, Some(true));
        assert_eq!(e.tipo, "sala_no_responde");
        assert!(e.detalle.contains("¿Está abierto Konstruado en el PC?"));
        assert!(!e.titulo.contains("Encend") && !e.detalle.contains("Encend"));
        let e = sala_de(&DiagSocks::SocksOk, s, 0, false, Some(true));
        assert_eq!(e.tipo, "socks_ok");
        assert!(!e.detalle.contains("Iniciar"));
        // Solo con el SOCKS caído se pide abrir Orbot; si no está instalado, se dice eso.
        let e = sala_de(&DiagSocks::SocksCaido("connection refused".into()), s, 0, false, Some(true));
        assert_eq!(e.tipo, "socks_caido");
        assert!(e.detalle.contains("Iniciar"));
        let e = sala_de(&DiagSocks::SocksCaido("connection refused".into()), s, 0, false, Some(false));
        assert_eq!(e.tipo, "sin_orbot");
        // Con sesión viva manda "conectado", diga lo que diga el último intento.
        assert_eq!(sala_de(&DiagSocks::DestinoNoResponde("x".into()), s, 1, false, None).tipo, "conectado");
        assert_eq!(sala_de(&DiagSocks::SinSocks, None, 0, false, None).tipo, "orbot_apagado_en_app");
        assert_eq!(sala_de(&DiagSocks::SinSocks, None, 0, true, None).tipo, "tcp");
    }

    #[test]
    fn destinos_se_leen() {
        let _g = idioma_fijo_en_test();
        aplicar_idioma("es");
        assert!(matches!(parse_destino("10.0.2.2:17432"), Some(PeerAddr::Tcp { port: 17432, .. })));
        assert!(matches!(parse_destino("abc.onion:80"), Some(PeerAddr::Onion { .. })));
        assert!(parse_destino("sinpuerto").is_none());
        // Idioma: perfil elegido > teléfono es/en > ES.
        aplicar_idioma("en");
        assert_eq!(tr("Hola", "Hi"), "Hi");
        assert_eq!(tf!("{} partidas", "{} stages", 3), "3 stages");
        aplicar_idioma("es");
        assert_eq!(tf!("{} partidas", "{} stages", 3), "3 partidas");
        assert_eq!(idioma_de_arranque("es", true, "en-US"), "es");
        assert_eq!(idioma_de_arranque("en", false, "es-CL"), "en");
        assert_eq!(idioma_de_arranque("es", false, "en-GB"), "en");
        assert_eq!(idioma_de_arranque("es", false, "es-CL"), "es");
        assert_eq!(idioma_de_arranque("es", false, "pt-BR"), "es");
    }

    #[test]
    fn numeros_y_notas() {
        let _g = idioma_fijo_en_test();
        aplicar_idioma("es");
        assert_eq!(parse_num("2.000"), 2000);
        assert_eq!(recorta_nota("x".repeat(MAX_NOTA + 5)).chars().count(), MAX_NOTA);
    }

    /// Sin claves faltantes: todo texto con pinta de español fuera de los tests
    /// va dentro de `tr(es, en)` o `tf!(es, en, …)`.
    #[test]
    fn textos_del_motor_tienen_ingles() {
        let fuente = include_str!("lib.rs");
        let codigo = &fuente[..fuente.find("#[cfg(test)]").unwrap()];
        let internas = ["es", "en", "pendiente", "en fondeo", "en obra", "en trato", "pagada", "mandante", "contratista", "sala"];
        let palabras = [
            " el ", " la ", " los ", " las ", " de ", " que ", " una ", " en ", " por ", " para ", " con ", " sin ", " no ", " ya ",
        ];
        let mut sueltos = Vec::new();
        let lineas: Vec<&str> = codigo.lines().collect();
        for (n, linea) in lineas.iter().enumerate() {
            let t = linea.trim_start();
            if t.starts_with("//") || t.starts_with("#[") {
                continue;
            }
            let previa = lineas[..n].iter().rev().find(|l| !l.trim().is_empty()).map(|l| l.trim_end()).unwrap_or("");
            let envuelta = linea.contains("tr(") || linea.contains("tf!(") || previa.ends_with("tf!(");
            for (i, trozo) in linea.split('"').enumerate() {
                if i % 2 == 0 || internas.contains(&trozo) {
                    continue;
                }
                let plano = format!(" {} ", trozo.to_lowercase());
                let es = trozo.chars().any(|c| "áéíóúñ¿¡«".contains(c)) || palabras.iter().any(|p| plano.contains(p));
                if es && !envuelta {
                    sueltos.push(format!("{}: {trozo}", n + 1));
                }
            }
        }
        assert!(sueltos.is_empty(), "textos sin tr/tf:\n{}", sueltos.join("\n"));
    }
}
