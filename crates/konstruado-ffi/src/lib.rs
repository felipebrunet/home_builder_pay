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

#[allow(dead_code)]
#[path = "../../konstruado/src/persist.rs"]
mod persist;

#[allow(dead_code, clippy::all)]
#[path = "../../konstruado/src/caja.rs"]
mod caja;

#[allow(dead_code)]
#[path = "../../konstruado/src/i18n.rs"]
mod i18n;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use i18n::Idioma;
use konstruado_core::{
    asegurar_clave, generar_clave, monto, monto_pct, n_partidas, oferta_en_tablero, Aceptacion,
    EstadoObra, Obra, Oferta, PartidaEstado, Persona, Rol, TextoLeido, MAX_NOTA,
};
use konstruado_net::{EstadoTor, Nodo, PeerAddr, ORBOT_SOCKS, PUERTO_LOCAL, RED, RENDEZVOUS_ONION, VIRT_PORT};

const L: Idioma = Idioma::Es;
const ES: bool = true;

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
    fallo(L.error(&e))
}

fn err_caja(e: String) -> FfiError {
    fallo(caja::aviso_humano(&e, ES))
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
    pub conectado: bool,
    pub pares: u32,
    pub sesiones_vivas: u32,
    pub socks: Option<String>,
    pub destinos: Vec<String>,
    pub otros: Vec<String>,
    pub red: String,
    pub onion_sala: String,
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
}

#[derive(uniffi::Object)]
pub struct KonstruadoApp {
    rt: tokio::runtime::Runtime,
    nodo: Nodo,
    caja: caja::Caja,
    ses: Arc<Mutex<Sesion>>,
    datos: PathBuf,
    ultima_prueba: Mutex<Option<DaemonPrueba>>,
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
            format!("sala {}…:{port} (Orbot)", &host[..10.min(host.len())])
        }
        PeerAddr::Onion { host, port } => format!("{host}:{port}"),
        PeerAddr::Buzon { .. } => "buzón".into(),
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
        _ => Err(fallo("Elegí si pagás la obra o la construís.")),
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
            bloques: format!("La caja mira {} bloques hacia atrás.", m.bloques),
            retro: (m.retro > 0).then(|| format!("Quedan {} bloques por mirar en la caja.", m.retro)),
            aviso: m.aviso.as_ref().map(|a| caja::aviso_humano(a, ES)),
        },
        None => MiradaVista {
            bloques: "La caja arranca por los últimos 40 bloques.".into(),
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
                texto: format!(
                    "{}: {} propone garantía {}",
                    obra.nombre,
                    obra.contratista.nombre,
                    monto(obra.garantia)
                ),
                obra_id: obra.id.clone(),
                partida: None,
            });
        }
        if let Some(ex) = obra.extra.as_ref() {
            if ex.por.id != mid {
                let detalle = match obra.leer_extra(sec) {
                    TextoLeido::Plano(t) => t,
                    TextoLeido::Cerrado => "Texto cifrado".into(),
                };
                out.push(AvisoVista {
                    texto: format!(
                        "{}: {} propone extra {} ({})",
                        obra.nombre,
                        ex.por.nombre,
                        detalle,
                        monto(ex.monto)
                    ),
                    obra_id: obra.id.clone(),
                    partida: None,
                });
            }
        }
        if let Some(cl) = obra.cierre.as_ref() {
            if cl.id != mid {
                out.push(AvisoVista {
                    texto: format!("{}: {} quiere cortar el trato", obra.nombre, cl.nombre),
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
            let titulo = L.titulo_partida(i, &p.detalle);
            if p.estado == PartidaEstado::Encerrando
                && p.encerrado_por.as_ref().map(|q| q.id.as_str()) != Some(mid)
            {
                out.push(AvisoVista {
                    texto: format!("{} · {}: te toca confirmar el encierre", obra.nombre, titulo),
                    obra_id: obra.id.clone(),
                    partida: Some(i as u32),
                });
            }
            if p.estado == PartidaEstado::EnTrato && p.turno == Some(mi_rol) {
                let pct = p.propuesto.unwrap_or(0);
                out.push(AvisoVista {
                    texto: format!("{} · {}: te toca responder ({pct}%)", obra.nombre, titulo),
                    obra_id: obra.id.clone(),
                    partida: Some(i as u32),
                });
            }
            if p.estado == PartidaEstado::Encerrada && mi_rol == Rol::Contratista {
                let quien = p
                    .encerrado_por
                    .as_ref()
                    .map(|q| q.nombre.as_str())
                    .unwrap_or("el mandante");
                out.push(AvisoVista {
                    texto: format!(
                        "{} · {}: {quien} encerró, avisá cuando termines",
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
        format!(
            "Trabajo {} · garantía sugerida {} · {} partidas",
            monto(o.trabajo),
            monto(o.garantia_sugerida),
            o.n_partidas_sugeridas
        )
    } else {
        format!(
            "{} ofrece trabajo por {}. Garantía sugerida {} ({} partidas).",
            o.mandante.nombre,
            monto(o.trabajo),
            monto(o.garantia_sugerida),
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
    }
}

fn parse_num(s: &str) -> u64 {
    s.chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
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
            .ok_or_else(|| fallo("Falta tu nombre en este equipo."))
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
            .ok_or_else(|| fallo("La obra todavía no llegó a este equipo."))
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
            clave_sec: s.clave_sec.clone(),
            spend_sec: s.spend_sec.clone(),
            obras_salidas: self.nodo.obras_salidas(),
        });
    }

    /// Igual que el escritorio: la red arrancó, hay nombre y el trato está alineado.
    fn exigir_sesion(&self, obra: &Obra) -> Result<Persona, FfiError> {
        let p = self.yo()?;
        let otro = otro_de(obra, &p.id);
        let n = &self.nodo;
        if matches!(n.estado_tor(), EstadoTor::Arrancando { .. }) && n.n_peers() == 0 {
            return Err(fallo(
                "Sincronizando el trato… esperá a que baje el estado del otro.",
            ));
        }
        if n.trato_alineado(&p.id, &otro) {
            Ok(p)
        } else if n.sesion_viva(&p.id, &otro) && !n.sync_reciente() {
            Err(fallo(
                "Sincronizando el trato… todavía no bajó lo último del otro.",
            ))
        } else {
            Err(fallo(
                "El otro no está en línea. Tiene que tener Konstruado abierto.",
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
                let (yo, rol, sec, disco) = {
                    let s = ses.lock().unwrap();
                    (
                        s.yo.clone(),
                        s.rol,
                        s.clave_sec.clone(),
                        (s.tema.clone(), s.idioma.clone(), s.spend_sec.clone()),
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
                    clave_sec: sec,
                    spend_sec: disco.2,
                    obras_salidas: nodo.obras_salidas(),
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
        let estado = match &tor {
            EstadoTor::Listo { .. } if vivos > 0 => "Conectado a la sala".to_string(),
            EstadoTor::Listo { .. } => "Orbot listo, buscando sala".to_string(),
            EstadoTor::Arrancando { paso } => L.paso_tor(paso),
            EstadoTor::Fallo(s) => format!("Orbot: {s}"),
            EstadoTor::Ausente if vivos > 0 => "Conectado por TCP".to_string(),
            EstadoTor::Ausente => "Sin Orbot ni destino TCP".to_string(),
        };
        let gente = if otros.is_empty() {
            if pares == 0 {
                "nadie más en la red".to_string()
            } else {
                format!("{pares} par(es), todavía sin nombre")
            }
        } else {
            otros.join(", ")
        };
        RedVista {
            linea: format!("{estado} · {gente}"),
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
        std::fs::create_dir_all(&datos).map_err(|e| fallo(format!("Carpeta de datos: {e}")))?;
        std::env::set_var("KONSTRUADO_DATOS", &datos_dir);
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
            .map_err(|e| fallo(format!("Red: {e}")))?;
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
                idioma: "es".into(),
            })),
            datos,
            ultima_prueba: Mutex::new(None),
        });
        app.lanzar_bucle();
        Ok(app)
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
            rol_label: s.rol.map(|r| L.rol(r).to_string()).unwrap_or_default(),
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
            .ok_or_else(|| fallo("Destino inválido. Usá host:puerto, p. ej. 10.0.2.2:17432."))?;
        let _e = self.rt.enter();
        self.nodo.agregar_destino(d);
        Ok(())
    }

    pub fn quitar_destino(&self, destino: String) {
        if let Some(d) = parse_destino(&destino) {
            self.nodo.quitar_destino(&d);
        }
        // "sala …" quita el onion.
        if destino.starts_with("sala") {
            self.nodo.quitar_destino(&PeerAddr::Onion {
                host: RENDEZVOUS_ONION.into(),
                port: VIRT_PORT,
            });
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
                "Instalá Orbot y encendelo (modo VPN recomendado: así también pasa el daemon de Monero).".into(),
                "Dejá el proxy SOCKS de Orbot en 127.0.0.1:9050 (viene así).".into(),
                "En Cuenta → Red tocá «Usar Orbot». El teléfono marca la sala horneada.".into(),
                "La sala la hospeda una PC: el mandante de escritorio, o `konstruado-sala`.".into(),
                "Dos teléfonos se hablan a través de esa PC (relay).".into(),
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
        let pista = if red.pares == 0 {
            "Nadie más todavía. Encendé Orbot (o agregá un destino TCP) y esperá la sala.".to_string()
        } else if soy_m {
            "Publicá una obra; el contratista la ve en su tablero.".to_string()
        } else if red.otros.is_empty() {
            "No hay avisos. El mandante tiene que publicar; podés tocar Buscar ofertas.".to_string()
        } else {
            format!(
                "{} está en la red. Si no ves el aviso, tocá Buscar ofertas.",
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
                    estado_label: L.label_estado(o.estado).into(),
                    en_curso: obra_en_curso(o.estado),
                    con_quien: if o.mandante.id == mid {
                        format!("con {}", o.contratista.nombre)
                    } else {
                        format!("con {}", o.mandante.nombre)
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
            .ok_or_else(|| fallo("Esa oferta ya no está en el tablero."))
    }

    pub fn publicar_oferta(
        &self,
        nombre: String,
        trabajo: String,
        garantia: String,
        detalles: Vec<String>,
    ) -> Result<OfertaVista, FfiError> {
        let m = self.yo()?;
        let t = parse_num(&trabajo);
        let g = parse_num(&garantia);
        let o = Oferta::publicar(m, nombre, t, g, detalles).map_err(err_core)?;
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
                texto: "Esa oferta ya no está.".into(),
                detalles: vec![],
            };
        };
        let g = parse_num(&garantia);
        let contra = g != o.garantia_sugerida;
        match n_partidas(o.trabajo, g) {
            Ok(n) => {
                let mut d = o.detalles.clone();
                d.resize(n as usize, String::new());
                PreviaAceptar {
                    ok: true,
                    contra,
                    n_partidas: n,
                    texto: if contra {
                        format!("Contra: {n} partidas de {g}. El mandante tiene que confirmar.")
                    } else {
                        format!("Aceptás {n} partidas. Los dos encierran {g} en cada una.")
                    },
                    detalles: d,
                }
            }
            Err(e) => PreviaAceptar {
                ok: false,
                contra,
                n_partidas: 0,
                texto: L.error(&e),
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
            .ok_or_else(|| fallo("Esa oferta ya no está en el tablero."))?;
        let g = parse_num(&garantia);
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
            return Err(fallo("Esta obra es de otras dos personas."));
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
                TextoLeido::Cerrado => "Texto cifrado".into(),
            };
            ExtraVista {
                texto,
                mia: ex.por.id == mid,
                monto: monto(ex.monto),
                por: ex.por.nombre.clone(),
            }
        });
        let caja_dir = v.caja_de(&obra.id).map(|s| s.to_string());
        let activa = obra.activa();
        Ok(ObraVista {
            id: obra.id.clone(),
            nombre: obra.nombre.clone(),
            estado: estado_obra_txt(estado),
            estado_label: L.label_estado(estado).into(),
            resumen: format!(
                "Mandante {} · contratista {} · {} partidas · trabajo {}",
                obra.mandante.nombre,
                obra.contratista.nombre,
                obra.n_partidas,
                monto(obra.trabajo)
            ),
            soy_mandante: soy_m,
            soy_contratista: soy_c,
            abierta,
            sincronizando: abierta && self.sincronizando(&obra),
            contra,
            contra_texto: contra.then(|| {
                format!(
                    "El contratista propone garantía {} ({} partidas).",
                    monto(obra.garantia),
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
            caja_direccion: caja_dir,
            partidas: obra
                .partidas
                .iter()
                .enumerate()
                .map(|(i, p)| PartidaFila {
                    indice: i as u32,
                    titulo: L.titulo_partida(i, &p.detalle),
                    label: L.label_partida(p),
                    estado: estado_partida_txt(p.estado),
                    por_lado: format!("{} por lado", monto(p.capital(obra.garantia))),
                    saldo_corto: caja::saldo_corto(
                        ES,
                        p.estado,
                        p.capital(obra.garantia),
                        p.fondeo_txid.is_some(),
                    ),
                    activa: activa == Some(i),
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
            return Err(fallo("La extra necesita un texto."));
        }
        let m = parse_num(&monto_lado);
        if m == 0 {
            return Err(fallo("La extra lleva un monto mayor a cero."));
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
            .ok_or_else(|| fallo("No está esa partida."))?;
        let mid = self.mid();
        if !obra.participa(&mid) {
            return Err(fallo("Esta obra es de otras dos personas."));
        }
        let sec = self.sec();
        let v = self.caja.vista();
        let soy_m = obra.mandante.id == mid;
        let soy_c = obra.contratista.id == mid;
        let mi_turno = p
            .turno
            .map(|r| match r {
                Rol::Mandante => soy_m,
                Rol::Contratista => soy_c,
            })
            .unwrap_or(false);
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
        let frenado = fondeo_curso.as_ref().is_some_and(caja::es_freno);
        let saldo = caja::saldo_partida(
            ES,
            p.estado,
            p.capital(garantia),
            p.fondeo_txid.is_some(),
            &obra.mandante.nombre,
            &obra.contratista.nombre,
        );
        let xmr_por_lado = if saldo.is_none() {
            caja::a_piconero(p.capital(garantia)).map(|pico| format!("{} XMR por lado en stagenet.", caja::fmt_xmr(pico)))
        } else {
            None
        };
        let notas = p
            .notas
            .iter()
            .map(|n| {
                let cabeza = format!(
                    "{} · {}% · {}",
                    n.autor_nombre,
                    n.porcentaje,
                    L.fmt_cuando(n.cuando)
                );
                match obra.leer_nota(n, &sec) {
                    TextoLeido::Plano(t) => NotaVista { cabeza, cuerpo: t, cifrada: false },
                    TextoLeido::Cerrado => NotaVista {
                        cabeza,
                        cuerpo: "Nota cifrada".into(),
                        cifrada: true,
                    },
                }
            })
            .collect();
        let pendiente = !cortada && p.estado == PartidaEstado::Pendiente;
        let encerrando = !cortada && p.estado == PartidaEstado::Encerrando;
        let pista = if pendiente && contra {
            Some("Primero hay que confirmar la contra de la obra.".to_string())
        } else if pendiente && !activa {
            Some("Todavía no toca. Cerrá la partida que está en curso.".to_string())
        } else if pendiente {
            Some("Los dos tienen que confirmar el encierre. El otro tiene que estar en línea.".to_string())
        } else if encerrando && soy_prop_enc && !frenado && fondeo_curso.is_none() {
            Some("Esperando que el otro confirme el encierre.".to_string())
        } else if encerrando && !soy_prop_enc && !frenado && fondeo_curso.is_none() {
            Some("El otro quiere encerrar esta partida. Confirmar arma una sola transacción con los dos.".to_string())
        } else if !cortada && p.estado == PartidaEstado::Encerrada && soy_m {
            Some("El contratista avisa cuando termina y propone cuánto se paga.".to_string())
        } else {
            None
        };
        let en_trato = !cortada && p.estado == PartidaEstado::EnTrato;
        Ok(PartidaVista {
            obra_id: obra.id.clone(),
            obra_nombre: obra.nombre.clone(),
            indice,
            titulo: L.titulo_partida(i, &p.detalle),
            label: L.label_partida(&p),
            estado: estado_partida_txt(p.estado),
            lead: format!(
                "{} por lado. Mandante {} · contratista {}",
                monto(p.capital(garantia)),
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
            linea: fondeo_curso.as_ref().map(|t| t.mostrar(ES)),
            linea_freno: frenado,
            sincronizando: !cortada && self.sincronizando(&obra),
            cortada,
            detalle: p.detalle.clone(),
            puede_editar: pendiente && (soy_m || soy_c),
            recibo: if p.estado == PartidaEstado::Pagada {
                p.recibo.as_ref().map(|r| {
                    format!(
                        "Recibo · {} · pagó {}% · {} · aceptó {} · {}",
                        r.titulo,
                        r.porcentaje,
                        monto(r.monto),
                        r.acepto_nombre,
                        L.fmt_cuando(r.cuando)
                    )
                })
            } else {
                None
            },
            cerrado_texto: (p.estado == PartidaEstado::Pagada && p.recibo.is_none()).then(|| {
                format!(
                    "Cerró al {}% ({}). El hilo quedó guardado.",
                    p.pago.unwrap_or(0),
                    monto(monto_pct(garantia, p.pago.unwrap_or(0)))
                )
            }),
            encerro: p
                .encerrado_por
                .as_ref()
                .map(|q| format!("Encerró {} · {}", q.nombre, L.fmt_cuando(p.encerrado_cuando))),
            notas,
            pista,
            puede_proponer_encerrar: pendiente && !contra && activa,
            puede_cancelar_propuesta: encerrando && soy_prop_enc && !frenado,
            // Misma regla que el escritorio (`caja::puede_empezar_fondeo_de_nuevo`).
            // `puede_reintentar_fondeo` queda como alias de UI (Android ORs ambos).
            puede_reintentar_fondeo: caja::puede_empezar_fondeo_de_nuevo(encerrando, frenado),
            puede_empezar_fondeo_de_nuevo: caja::puede_empezar_fondeo_de_nuevo(encerrando, frenado),
            puede_confirmar_fondear: encerrando && !soy_prop_enc && !frenado && fondeo_curso.is_none(),
            puede_no_encerrar: encerrando && !(soy_prop_enc && !frenado),
            puede_avisar_termino: !cortada && p.estado == PartidaEstado::Encerrada && soy_c,
            en_trato,
            propuesto: if en_trato { p.propuesto } else { None },
            propuesto_texto: if en_trato {
                p.propuesto
                    .map(|n| format!("Sobre la mesa: {n}% ({}).", monto(monto_pct(garantia, n))))
            } else {
                None
            },
            mi_turno: en_trato && mi_turno,
            espera_a: if en_trato { espera_a } else { None },
            max_nota: MAX_NOTA as u32,
        })
    }

    pub fn editar_detalle(&self, obra_id: String, indice: u32, texto: String) -> Result<(), FfiError> {
        self.accion(&obra_id, false, |o, q| o.editar_detalle(indice as usize, q, texto))
    }

    pub fn proponer_encerrar(&self, obra_id: String, indice: u32) -> Result<(), FfiError> {
        self.accion(&obra_id, true, |o, q| o.encerrar_proponer(indice as usize, q))
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
                (Some(d), Some(h)) => format!("Visto desde el bloque {d} hasta el {h}."),
                _ => "Todavía no miré la cadena. Arranco por los últimos 40 bloques.".into(),
            },
            buscando: b.buscando,
            enviando: b.enviando,
            retro: (b.retro > 0).then(|| format!("Quedan {} bloques por mirar hacia atrás.", b.retro)),
            aviso: b.aviso.as_ref().map(|a| caja::aviso_humano(a, ES)),
            ultimo: b.ultimo.as_ref().map(|tx| {
                format!(
                    "Último envío {tx}. Fee {} XMR. Cambio {} XMR, vuelve en el próximo bloque.",
                    caja::fmt_xmr(b.ultimo_fee.unwrap_or(0)),
                    caja::fmt_xmr(b.ultimo_cambio.unwrap_or(0))
                )
            }),
            movs: b
                .movs
                .iter()
                .map(|m| MovVista {
                    monto: format!("{} XMR", caja::fmt_xmr(m.monto)),
                    detalle: format!(
                        "bloque {} · {}",
                        m.altura,
                        if m.libre { "libre" } else { "trabado" }
                    ),
                })
                .collect(),
            escala: caja::escala(ES),
            cajas,
        }
    }

    pub fn crear_semilla(&self) -> Result<String, FfiError> {
        let _e = self.rt.enter();
        self.caja.crear_semilla().map_err(err_caja)
    }

    /// JSON del perfil de obras/ofertas (sin seed ni share). Puede estar desfasado vs el peer.
    pub fn exportar_obras(&self) -> Result<String, FfiError> {
        let mid = self.mid();
        let mut obras: Vec<Obra> = self
            .nodo
            .obras_todas()
            .into_iter()
            .filter(|o| mid.is_empty() || o.participa(&mid))
            .collect();
        obras.sort_by(|a, b| b.actualizado.cmp(&a.actualizado).then(b.id.cmp(&a.id)));
        let ofertas = self
            .nodo
            .tablero()
            .into_iter()
            .filter(|o| mid.is_empty() || o.mandante.id == mid)
            .collect::<Vec<_>>();
        persist::exportar_perfil_obras(&obras, &ofertas).map_err(fallo)
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
        Ok(format!(
            "Importé {n_obras} obra(s) y {n_ofertas} oferta(s). El estado puede estar desfasado respecto al otro; la cadena y el share mandan para el dinero. Si tenés el share, recuperalo después."
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
            return Err(fallo("Esta obra es de otras dos personas."));
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
            "Archivé la obra en este equipo. Ya no se ve en el tablero ni en Mis obras. No se movieron fondos; el share y el contexto quedan en disco."
                .into(),
        )
    }

    /// Quita una oferta propia del tablero (sin contratista todavía).
    pub fn quitar_mi_oferta(&self, oferta_id: String) -> Result<String, FfiError> {
        let yo = self.yo()?;
        let oferta = self
            .nodo
            .tablero()
            .into_iter()
            .find(|o| o.id == oferta_id)
            .ok_or_else(|| fallo("Esa oferta ya no está en el tablero."))?;
        if oferta.mandante.id != yo.id {
            return Err(fallo("Solo podés quitar tus propias ofertas."));
        }
        let todas = self.nodo.obras_todas();
        if !oferta_en_tablero(&oferta_id, &todas) {
            return Err(fallo(
                "Esa oferta ya tiene obra conjunta. Archivá la obra; no la quites como oferta.",
            ));
        }
        let _e = self.rt.enter();
        self.nodo.quitar(&oferta_id);
        drop(_e);
        self.persistir();
        Ok("Quité la oferta del tablero.".into())
    }

    /// Cancela fondeo/propuesta de encierre de una partida solo en este equipo. No mueve fondos en cadena.
    pub fn salir_partida_local(&self, obra_id: String, indice: u32) -> Result<String, FfiError> {
        let mut obra = self.obra(&obra_id)?;
        let yo = self.yo()?;
        if !obra.participa(&yo.id) {
            return Err(fallo("Esta obra es de otras dos personas."));
        }
        let i = indice as usize;
        if obra.partidas.get(i).is_none() {
            return Err(fallo("No está esa partida."));
        }
        let _e = self.rt.enter();
        self.caja.cancelar_fondeo(&obra.id, i);
        let mut aviso = String::from(
            "Cancelé el fondeo local de esta partida. Los fondos ya en la caja 2-de-2 no se tocan.",
        );
        if obra.partidas[i].estado == PartidaEstado::Encerrando {
            match obra.encerrar_cancelar(i, &yo) {
                Ok(()) => {
                    let _ = self.publicar_trato(obra.clone(), &yo);
                    aviso.push_str(" También volví la propuesta de encierre a Pendiente en este equipo.");
                }
                Err(_) => {
                    aviso.push_str(" La propuesta de encierre no se pudo revertir sola (hace falta el otro o ya no está Encerrando).");
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

    /// Texto del respaldo de las 25 palabras (para guardarlo con el selector de Android).
    pub fn exportar_semilla(&self) -> Result<String, FfiError> {
        let path = self.tmp("semilla.txt");
        let r = self.caja.guardar_palabras(&path).map_err(err_caja);
        let txt = r.and_then(|_| std::fs::read_to_string(&path).map_err(|e| fallo(e.to_string())));
        let _ = std::fs::remove_file(&path);
        txt
    }

    pub fn restaurar_semilla(&self, texto: String) -> Result<String, FfiError> {
        let path = self.tmp("semilla-in.txt");
        std::fs::write(&path, texto).map_err(|e| fallo(e.to_string()))?;
        let _e = self.rt.enter();
        let r = self.caja.restaurar_semilla(&path);
        let _ = std::fs::remove_file(&path);
        r.map(|c| caja::listo_humano(c, ES)).map_err(err_caja)
    }

    /// Texto del share FROST de la caja de esta obra. Puede gastar junto al del otro.
    pub fn exportar_share(&self, obra_id: String) -> Result<String, FfiError> {
        let path = self.tmp("caja.share");
        let r = self.caja.guardar_share(&obra_id, &path).map_err(err_caja);
        let txt = r.and_then(|_| std::fs::read_to_string(&path).map_err(|e| fallo(e.to_string())));
        let _ = std::fs::remove_file(&path);
        txt
    }

    pub fn restaurar_share(&self, texto: String) -> Result<String, FfiError> {
        let yo = self.yo()?;
        let path = self.tmp("caja-in.share");
        std::fs::write(&path, texto).map_err(|e| fallo(e.to_string()))?;
        let obras = self.nodo.obras();
        let _e = self.rt.enter();
        let r = self.caja.restaurar_share(&path, &yo, &obras);
        let _ = std::fs::remove_file(&path);
        r.map(|c| caja::listo_humano(c, ES)).map_err(err_caja)
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
    pub fn probar_daemon(&self) -> DaemonPrueba {
        let r = self.rt.block_on(caja::probar_daemon(ES));
        let prueba = DaemonPrueba {
            ok: r.ok,
            url: r.url,
            tip: r.tip,
            ms: r.ms,
            mensaje: r.mensaje,
        };
        guardar_prueba(&self.ultima_prueba, prueba)
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

    #[test]
    fn destinos_se_leen() {
        assert!(matches!(parse_destino("10.0.2.2:17432"), Some(PeerAddr::Tcp { port: 17432, .. })));
        assert!(matches!(parse_destino("abc.onion:80"), Some(PeerAddr::Onion { .. })));
        assert!(parse_destino("sinpuerto").is_none());
    }

    #[test]
    fn numeros_y_notas() {
        assert_eq!(parse_num("2.000"), 2000);
        assert_eq!(recorta_nota("x".repeat(MAX_NOTA + 5)).chars().count(), MAX_NOTA);
    }


}
