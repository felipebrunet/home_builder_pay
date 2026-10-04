//! Caja Monero de la ventana. Stagenet, un solo nodo, secretos fuera del DHT.
//!
//! La semilla y el share viven en `KONSTRUADO_DATOS/xmr` con modo 0600.
//! El dominio pasa a Encerrada o Pagada recién cuando este módulo vio la
//! transacción en un bloque. 1 unidad del trato = 0,00002 XMR: la garantía
//! de 2000 son 0,04 XMR por lado.
//!
//! La billetera personal (saldo, recibir, enviar) usa la misma semilla.
//! El libro de salidas está en `xmr/libro.json`, también en 0600.

use std::collections::{HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rand_core::{OsRng, RngCore};
use serde::Deserialize;
use serde::Serialize;

use xmr_joint::backup::{self, SeedBackup};
use xmr_joint::chain::{self, Daemon};
use xmr_joint::coop::{self, Proposal, Skeleton};
use xmr_joint::dkg::{DkgParty, JointAccount, Party, ViewAnnounce};
use xmr_joint::fund::{self, view_del_mandante};
use xmr_joint::personal::{self, elegir_montos};
use xmr_joint::spend::{self, SpendProposal, SpendSession, SpendSigned};
use xmr_joint::wallet::SingleWallet;
use xmr_joint::{OutputWithDecoys, Net, FEE_CUSHION, PICONERO, STAGENET_DAEMON};

use konstruado_core::{EstadoObra, Obra, PartidaEstado, Persona, Rol};
use konstruado_net::{CajaMsg, Nodo};

/// 1 unidad del trato, en piconero. 2000 unidades = 0,04 XMR.
pub const PICONERO_POR_UNIDAD: u64 = 20_000_000;

const LOOKBACK: usize = 40;
/// Hasta dónde camina solo el fondeo si la billetera todavía no vio la salida. ~4 semanas en stagenet.
const MAX_HISTORIA: usize = 20_000;
const PAUSA: Duration = Duration::from_secs(20);

pub fn a_piconero(unidades: u64) -> Option<u64> {
    unidades.checked_mul(PICONERO_POR_UNIDAD)
}

pub fn fmt_xmr(pico: u64) -> String {
    let whole = pico / PICONERO;
    let mut frac = format!("{:012}", pico % PICONERO);
    while frac.len() > 2 && frac.ends_with('0') {
        frac.pop();
    }
    format!("{whole}.{frac}")
}

pub fn escala(es: bool) -> String {
    if es {
        "En stagenet, 1 unidad son 0,00002 XMR. La garantía de 2000 son 0,04 XMR por lado.".into()
    } else {
        "On stagenet, 1 unit is 0.00002 XMR. A guarantee of 2000 is 0.04 XMR per side.".into()
    }
}

/// Lo máximo que se puede tipear en Enviar: el saldo libre menos el margen del fee.
pub fn maximo_envio(libre: u64) -> Option<String> {
    let piso = libre.checked_sub(FEE_CUSHION)?;
    if piso == 0 {
        None
    } else {
        Some(fmt_xmr(piso))
    }
}

#[derive(Clone, Debug)]
pub struct CajaVista {
    pub daemon: String,
    pub tip: Option<usize>,
    pub personal: Option<String>,
    pub tiene_semilla: bool,
    pub cajas: Vec<(String, String)>,
    pub lineas: Vec<Linea>,
    pub billetera: BilleteraVista,
}

/// Saldo de la billetera personal. Los montos van en piconero.
#[derive(Clone, Debug)]
pub struct BilleteraVista {
    pub total: u64,
    pub libre: u64,
    pub trabado: u64,
    pub desde: Option<usize>,
    pub hasta: Option<usize>,
    pub retro: usize,
    pub buscando: bool,
    pub enviando: bool,
    pub aviso: Option<String>,
    pub ultimo: Option<String>,
    pub ultimo_fee: Option<u64>,
    pub ultimo_cambio: Option<u64>,
    pub movs: Vec<Mov>,
}

/// Una entrada vista en la billetera personal.
#[derive(Clone, Debug)]
pub struct Mov {
    pub monto: u64,
    pub altura: usize,
    pub libre: bool,
}

impl CajaVista {
    pub fn vacia() -> Self {
        Self {
            daemon: STAGENET_DAEMON.to_string(),
            tip: None,
            personal: None,
            tiene_semilla: false,
            cajas: Vec::new(),
            lineas: Vec::new(),
            billetera: BilleteraVista::vacia(),
        }
    }

    pub fn linea(&self, obra: &str, partida: usize) -> Option<&Texto> {
        self.lineas
            .iter()
            .find(|l| l.obra == obra && l.partida == Some(partida))
            .map(|l| &l.texto)
    }

    pub fn caja_de(&self, obra: &str) -> Option<&str> {
        self.cajas
            .iter()
            .find(|(id, _)| id == obra)
            .map(|(_, addr)| addr.as_str())
    }
}

impl BilleteraVista {
    fn vacia() -> Self {
        Self {
            total: 0,
            libre: 0,
            trabado: 0,
            desde: None,
            hasta: None,
            retro: 0,
            buscando: false,
            enviando: false,
            aviso: None,
            ultimo: None,
            ultimo_fee: None,
            ultimo_cambio: None,
            movs: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Linea {
    pub obra: String,
    pub partida: Option<usize>,
    pub texto: Texto,
}

#[derive(Clone, Debug)]
pub enum Texto {
    Armando,
    Fondeando,
    EsperandoFondeo(String),
    Gastando,
    EsperandoPago(String),
    SinOtro,
    SinSemilla,
    /// La billetera todavía no muestra una salida libre. El scan sigue hacia atrás.
    BuscandoMonedas,
    /// El contratista ya pidió el fondeo. Falta la propuesta del mandante.
    EsperandoPropuesta,
    /// El mandante ya firmó su parte. Falta el contratista.
    EsperandoContratista,
    /// Hay plata, pero el candado de ~10 bloques no soltó.
    Trabadas,
    /// Se miró el historial reciente y no hay una salida que alcance.
    SinSaldo,
    /// La caja 2-de-2 de la obra todavía no existe en este equipo.
    SinCaja,
    /// El gasto no ve las dos salidas de la caja.
    SinSaldoCaja,
    /// Esas salidas de la caja siguen bajo el candado de 10 bloques.
    TrabadasCaja,
    /// Falta la dirección personal del otro para pagar.
    SinDireccion,
    /// El otro avisó que su billetera no alcanza.
    SinSaldoOtro,
    /// El otro avisó que sus monedas siguen trabadas.
    TrabadasOtro,
    SinSemillaOtro,
    SinCajaOtro,
    SinDireccionOtro,
    /// El otro todavía está mirando su billetera. Acá no se arma nada.
    BuscandoOtro,
    Falla(String),
}

impl Texto {
    pub fn mostrar(&self, es: bool) -> String {
        match self {
            Texto::Armando => {
                if es {
                    "Armando la caja 2-de-2… los dos tienen que seguir en línea.".into()
                } else {
                    "Building the 2-of-2 box… both have to stay online.".into()
                }
            }
            Texto::Fondeando => {
                if es {
                    "Fondeando los dos lados en una sola transacción…".into()
                } else {
                    "Funding both sides in one transaction…".into()
                }
            }
            Texto::EsperandoFondeo(tx) => {
                if es {
                    format!("Fondeo {tx} a la espera de un bloque.")
                } else {
                    format!("Funding {tx} is waiting for a block.")
                }
            }
            Texto::Gastando => {
                if es {
                    "Firmando el pago entre los dos…".into()
                } else {
                    "Both sides are signing the payment…".into()
                }
            }
            Texto::EsperandoPago(tx) => {
                if es {
                    format!("Pago {tx} a la espera de un bloque.")
                } else {
                    format!("Payment {tx} is waiting for a block.")
                }
            }
            Texto::SinOtro => {
                if es {
                    "El otro no anunció su identidad. Tiene que tener Konstruado abierto.".into()
                } else {
                    "The other person has not announced their identity. Konstruado has to be open.".into()
                }
            }
            Texto::SinSemilla => {
                if es {
                    "Primero creá la billetera de stagenet en Cuenta.".into()
                } else {
                    "Create the stagenet wallet in Account first.".into()
                }
            }
            Texto::BuscandoMonedas => {
                if es {
                    "Estoy mirando tu billetera para este encierre. Si el faucet es viejo, sigo hacia atrás.".into()
                } else {
                    "Looking through your wallet for this lock. Older faucet coins are included.".into()
                }
            }
            Texto::EsperandoPropuesta => {
                if es {
                    "Pedido enviado. La billetera del mandante está armando la transacción.".into()
                } else {
                    "Request sent. The client's wallet is building the transaction.".into()
                }
            }
            Texto::EsperandoContratista => {
                if es {
                    "Ya puse mi parte. Falta la firma del contratista, con la ventana abierta.".into()
                } else {
                    "Your part is in. Waiting for the contractor to sign, with the window open.".into()
                }
            }
            Texto::Trabadas => {
                if es {
                    "Las monedas están, pero siguen trabadas unos 10 bloques. No armo la transacción hasta que se suelten.".into()
                } else {
                    "The coins are there, but they stay locked for about 10 blocks. The transaction waits until they unlock.".into()
                }
            }
            Texto::SinSaldo => {
                if es {
                    "No alcanza el saldo libre. No armé la transacción. Si el faucet es más viejo que lo ya mirado, en Billetera pedí mirar más atrás.".into()
                } else {
                    "Unlocked balance is not enough. The transaction was not built. If the faucet is older than the scan, look further back in Wallet.".into()
                }
            }
            Texto::SinCaja => {
                if es {
                    "La caja de los dos todavía no está armada. No sigo hasta que los dos estén en línea.".into()
                } else {
                    "The shared box is not ready yet. Nothing proceeds until both stay online.".into()
                }
            }
            Texto::SinSaldoCaja => {
                if es {
                    "La caja no muestra las dos salidas libres de esta partida. No armé el pago.".into()
                } else {
                    "The box does not show this stage's two unlocked outputs. The payment was not built.".into()
                }
            }
            Texto::TrabadasCaja => {
                if es {
                    "Las salidas de la caja siguen trabadas unos 10 bloques. No armo el pago hasta que se suelten.".into()
                } else {
                    "The box outputs stay locked for about 10 blocks. The payment waits until they unlock.".into()
                }
            }
            Texto::SinDireccion => {
                if es {
                    "Falta la dirección personal del otro. No armo el pago.".into()
                } else {
                    "The other person's personal address is missing. The payment was not built.".into()
                }
            }
            Texto::SinSaldoOtro => {
                if es {
                    "El otro no tiene saldo libre que alcance. No armo nada de este lado.".into()
                } else {
                    "The other person does not have enough unlocked balance. Nothing is built on this side.".into()
                }
            }
            Texto::TrabadasOtro => {
                if es {
                    "El otro tiene las monedas trabadas unos 10 bloques. No armo hasta que se suelten.".into()
                } else {
                    "The other person's coins stay locked for about 10 blocks. Nothing is built until they unlock.".into()
                }
            }
            Texto::SinSemillaOtro => {
                if es {
                    "Al otro le falta la billetera de stagenet.".into()
                } else {
                    "The other person has not created a stagenet wallet.".into()
                }
            }
            Texto::SinCajaOtro => {
                if es {
                    "Al otro todavía no le armó la caja.".into()
                } else {
                    "The other person does not have the shared box yet.".into()
                }
            }
            Texto::SinDireccionOtro => {
                if es {
                    "Al otro le falta una dirección personal. El pago no arranca.".into()
                } else {
                    "The other person is missing a personal address. The payment does not start.".into()
                }
            }
            Texto::BuscandoOtro => {
                if es {
                    "El otro está mirando su billetera. Todavía no armo la transacción.".into()
                } else {
                    "The other person is checking their wallet. The transaction is not being built yet.".into()
                }
            }
            Texto::Falla(s) => aviso_humano(s, es),
        }
    }
}

/// Un freno ya conocido: hay que mostrarlo y no seguir armando.
pub fn es_freno(texto: &Texto) -> bool {
    matches!(
        texto,
        Texto::Falla(_)
            | Texto::SinSaldo
            | Texto::Trabadas
            | Texto::SinSemilla
            | Texto::SinCaja
            | Texto::SinSaldoCaja
            | Texto::TrabadasCaja
            | Texto::SinDireccion
            | Texto::SinSaldoOtro
            | Texto::TrabadasOtro
            | Texto::SinSemillaOtro
            | Texto::SinCajaOtro
            | Texto::SinDireccionOtro
    )
}

/// Textos de la billetera y códigos del motor, en el idioma de la ventana.
pub fn aviso_humano(aviso: &str, es: bool) -> String {
    let (esp, ing) = match aviso {
        "codigo:sin-saldo" | "sin-saldo" | "sin-saldo-caja" => (
            "No alcanza el saldo libre para este paso. No armé la transacción.",
            "Unlocked balance is not enough for this step. The transaction was not built.",
        ),
        "codigo:trabadas" | "trabadas" | "trabadas-caja" => (
            "El saldo está, pero sigue trabado unos 10 bloques. No armé la transacción.",
            "The balance is there, but it stays locked for about 10 blocks. The transaction was not built.",
        ),
        "codigo:sin-punta" => (
            "Todavía no llega la punta del nodo. No armé el envío.",
            "The node tip has not arrived yet. The send was not built.",
        ),
        "codigo:sin-semilla" | "sin-semilla" => (
            "Primero creá la billetera de stagenet.",
            "Create the stagenet wallet first.",
        ),
        "codigo:en-curso" => (
            "Ya hay un envío en curso.",
            "A send is already in progress.",
        ),
        "codigo:destino" => (
            "Falta la dirección de destino.",
            "The destination address is missing.",
        ),
        "codigo:monto-cero" | "el monto es cero" => ("El monto es cero.", "The amount is zero."),
        "monto inválido" => ("El monto no es válido.", "The amount is not valid."),
        "demasiados decimales" => (
            "El monto tiene demasiados decimales.",
            "The amount has too many decimal places.",
        ),
        "el monto es demasiado grande" => (
            "El monto es demasiado grande.",
            "The amount is too large.",
        ),
        "primero creá la billetera de stagenet" | "falta la billetera de stagenet" => (
            "Primero creá la billetera de stagenet.",
            "Create the stagenet wallet first.",
        ),
        "ya hay un envío en curso" => (
            "Ya hay un envío en curso.",
            "A send is already in progress.",
        ),
        "falta la dirección de destino" => (
            "Falta la dirección de destino.",
            "The destination address is missing.",
        ),
        "no está esa partida" => ("No está esa partida.", "That stage is not here."),
        "el encierre no está propuesto" => (
            "El encierre no está propuesto.",
            "The lock has not been proposed.",
        ),
        "el monto no entra en piconero" => (
            "El monto no entra en piconero.",
            "The amount does not fit in piconero.",
        ),
        "no hay un porcentaje sobre la mesa" => (
            "No hay un porcentaje sobre la mesa.",
            "There is no percentage on the table.",
        ),
        "falta el porcentaje" => ("Falta el porcentaje.", "The percentage is missing."),
        "falta la dirección personal del otro" | "sin-direccion" => (
            "Falta la dirección personal del otro.",
            "The other person's personal address is missing.",
        ),
        "la caja de la obra todavía no está armada" | "sin-caja" => (
            "La caja de la obra todavía no está armada.",
            "The job's shared box is not ready yet.",
        ),
        "no estás en esta obra" => ("No estás en esta obra.", "You are not on this job."),
        "propuesta ilegible" => (
            "La propuesta de fondeo no se puede leer. No seguí armando.",
            "The funding proposal cannot be read. Building stopped.",
        ),
        "fee del nodo inválido" => (
            "El nodo no entregó un fee válido. No armé el pago.",
            "The node did not return a valid fee. The payment was not built.",
        ),
        other => return other.to_string(),
    };
    if es { esp.into() } else { ing.into() }
}

#[derive(Clone, Debug)]
pub struct Hecho {
    pub obra: String,
    pub partida: usize,
    pub fondeo: Option<String>,
    pub pago: Option<String>,
}

#[derive(Clone)]
pub struct Caja {
    inner: Arc<Mutex<Motor>>,
}

impl PartialEq for Caja {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for Caja {}

impl Caja {
    pub fn nueva() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Motor::cargar())),
        }
    }

    pub fn vista(&self) -> CajaVista {
        self.inner.lock().unwrap().vista.clone()
    }

    pub fn crear_semilla(&self) -> Result<String, String> {
        let mut m = self.inner.lock().unwrap();
        let addr = m.crear_semilla()?;
        m.armar_vista(&[]);
        Ok(addr)
    }

    pub fn guardar_palabras(&self, path: &Path) -> Result<(), String> {
        self.inner.lock().unwrap().exportar_semilla(path)
    }

    /// Pide un envío desde la billetera personal. `monto` está en XMR (`0.04` o `0,04`).
    pub fn pedir_envio(&self, destino: &str, monto: &str) -> Result<(), String> {
        self.inner.lock().unwrap().pedir_envio(destino, monto)
    }

    /// Vuelve a mirar los bloques recientes.
    pub fn pedir_actualizacion(&self) {
        let mut m = self.inner.lock().unwrap();
        m.forzar = true;
        m.scan_pausa = None;
        m.tip_en = None;
        m.scan_aviso = None;
    }

    /// Extiende el scan 200 bloques hacia atrás.
    pub fn pedir_atras(&self) {
        let mut m = self.inner.lock().unwrap();
        m.retro = m.retro.saturating_add(200);
        m.scan_pausa = None;
        m.scan_aviso = None;
        m.guardar_libro();
    }

    pub fn pedir_fondeo(&self, obra: &Obra, partida: usize, yo: &Persona) -> Result<(), String> {
        self.inner.lock().unwrap().pedir_fondeo(obra, partida, yo)
    }

    /// Vuelve a armar el fondeo sin borrar el pedido a mitad de una transacción ya publicada.
    pub fn reintentar_fondeo(&self, obra: &Obra, partida: usize, yo: &Persona) -> Result<(), String> {
        self.inner.lock().unwrap().reintentar_fondeo(obra, partida, yo)
    }

    pub fn cancelar_fondeo(&self, obra: &str, partida: usize) {
        self.inner.lock().unwrap().cancelar_fondeo(obra, partida);
    }

    pub fn pedir_gasto(&self, obra: &Obra, partida: usize, yo: &Persona) -> Result<(), String> {
        self.inner.lock().unwrap().pedir_gasto(obra, partida, yo)
    }

    pub fn tick(&self, nodo: &Nodo, yo: &Persona, obras: &[Obra]) -> Vec<Hecho> {
        let mut m = self.inner.lock().unwrap();
        let entradas = nodo.tomar_caja();
        m.ingerir(entradas, yo, obras);
        m.limpiar_resueltos(obras);
        m.avanzar_dkg(nodo, yo, obras);
        m.mandar_direccion(nodo, yo, obras);
        m.soltar_avisos_con_moneda();
        m.habilitar_pedidos();
        m.avisar_pedidos(nodo, yo);
        m.cerrar_si_puede(nodo, yo);
        m.firmar_si_puede(nodo, yo);
        m.lanzar_si_toca(nodo, yo);
        m.tomar_listo(nodo, yo);
        let hechos = m.hechos(yo, obras);
        m.armar_vista(obras);
        hechos
    }
}

struct Motor {
    wallet: Option<SingleWallet>,
    cuentas: HashMap<String, JointAccount>,
    dkg: HashMap<String, DkgRun>,
    dkg_inbox: HashMap<String, DkgInbox>,
    fondeos: HashMap<(String, usize), Fondeo>,
    gastos: HashMap<(String, usize), Gasto>,
    pares: HashMap<String, String>,
    pedir_view: HashSet<String>,
    tip: Option<usize>,
    tip_en: Option<Instant>,
    vista: CajaVista,
    ocupado: Option<Pendiente>,
    generacion: u64,
    libro: Libro,
    pedido_envio: Option<PedidoEnvio>,
    retro: usize,
    forzar: bool,
    buscando: bool,
    enviando: bool,
    scan_aviso: Option<String>,
    envio_aviso: Option<String>,
    ultimo_envio: Option<String>,
    ultimo_fee: Option<u64>,
    ultimo_cambio: Option<u64>,
    scan_pausa: Option<Instant>,
}

struct Pendiente {
    gen: u64,
    celda: Arc<Mutex<Option<Listo>>>,
}

struct DkgInbox {
    commit_otro: Option<Vec<u8>>,
    share_otro: Option<Vec<u8>>,
    view_otro: Option<Vec<u8>>,
}

struct DkgRun {
    party: DkgParty,
    commit: Vec<u8>,
    commit_otro: Option<Vec<u8>>,
    share_otro: Option<Vec<u8>>,
    view_otro: Option<Vec<u8>>,
    envie_commit: bool,
    envie_share: bool,
    envie_view: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AvisoFondeo {
    SinSaldo,
    Trabadas,
    SinSemilla,
    SinCaja,
    SinSaldoOtro,
    TrabadasOtro,
    SinSemillaOtro,
    SinCajaOtro,
}

struct Fondeo {
    peer: String,
    capital: u64,
    soy_mandante: bool,
    propuesta: Option<Proposal>,
    outputs: Option<Vec<OutputWithDecoys>>,
    esqueleto: Option<Skeleton>,
    sobre: Option<SobreFondeo>,
    blob: Option<Vec<u8>>,
    txid: Option<String>,
    visto: bool,
    error: Option<String>,
    aviso: Option<AvisoFondeo>,
    /// El aviso local ya se mandó al otro. Los avisos *Otro no se reenvían.
    aviso_enviado: bool,
    ultimo: Option<Instant>,
    avisar: bool,
    /// Ya salió `fund-pedir`. Un reintento tiene que abortar antes de pedir de nuevo.
    avise_pedir: bool,
    /// Esta billetera todavía no puede poner su parte. No se le pide nada al otro.
    espera_moneda: bool,
    /// El otro ya mandó `buscando` y este lado solo lo muestra.
    dije_busqueda: bool,
    /// El pedido lo abrió el otro. No hay que devolverle `fund-pedir`.
    viene_del_par: bool,
    /// No es un fondeo real: solo muestra lo que avisó el otro.
    solo_aviso: bool,
    /// El otro dijo que sigue mirando su billetera.
    par_buscando: bool,
    /// Primero se avisa el aborto al otro; al tick siguiente se vuelve a pedir.
    abortar: bool,
}

struct Gasto {
    peer: String,
    capital: u64,
    pct: u32,
    soy_mandante: bool,
    propuesta: Option<SpendProposal>,
    session: Option<SpendSession>,
    signed: Option<SpendSigned>,
    pre_otro: Option<Vec<u8>>,
    share_otro: Option<Vec<u8>>,
    envie_open: bool,
    envie_pre: bool,
    envie_share: bool,
    blob: Option<Vec<u8>>,
    txid: Option<String>,
    visto: bool,
    error: Option<String>,
    ultimo: Option<Instant>,
    avisar: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct SobreFondeo {
    proposal: String,
    view: String,
    direccion: String,
}

#[derive(Serialize, Deserialize)]
struct SobreGasto {
    proposal: String,
    pre: String,
}

enum Listo {
    Punta(usize),
    Entradas {
        obra: String,
        partida: usize,
        pago: bool,
        decoys: Vec<OutputWithDecoys>,
        fee: (u64, u64),
    },
    Visto {
        obra: String,
        partida: usize,
        pago: bool,
        si: bool,
    },
    Publicado {
        obra: String,
        partida: usize,
        pago: bool,
        txid: String,
        ok: Result<(), String>,
    },
    Fallo {
        obra: String,
        partida: Option<usize>,
        pago: Option<bool>,
        msg: String,
    },
    Saldo {
        desde: usize,
        hasta: usize,
        tip: usize,
        retro: bool,
        entradas: Vec<EntradaNueva>,
    },
    Aviso(String),
    Envio {
        ok: Result<EnvioHecho, String>,
        usadas: Vec<(String, u64)>,
    },
}

impl Motor {
    fn cargar() -> Self {
        let mut m = Self {
            wallet: leer_semilla(),
            cuentas: HashMap::new(),
            dkg: HashMap::new(),
            dkg_inbox: HashMap::new(),
            fondeos: HashMap::new(),
            gastos: HashMap::new(),
            pares: HashMap::new(),
            pedir_view: HashSet::new(),
            tip: None,
            tip_en: None,
            vista: CajaVista::vacia(),
            ocupado: None,
            generacion: 0,
            libro: Libro::vacio(),
            pedido_envio: None,
            retro: 0,
            forzar: false,
            buscando: false,
            enviando: false,
            scan_aviso: None,
            envio_aviso: None,
            ultimo_envio: None,
            ultimo_fee: None,
            ultimo_cambio: None,
            scan_pausa: None,
        };
        m.cargar_libro();
        m.cargar_shares();
        m.cargar_pares();
        m.cargar_esperas();
        m.armar_vista(&[]);
        m
    }

    fn crear_semilla(&mut self) -> Result<String, String> {
        if self.wallet.is_some() {
            return Err("ya hay una semilla en este equipo".into());
        }
        let (wallet, words) = SingleWallet::generate(&mut OsRng, Net::Stagenet).map_err(|e| e.to_string())?;
        let backup = SeedBackup {
            net: Net::Stagenet,
            address: wallet.address().to_string(),
            words,
        };
        backup::write_secret_file(&semilla_path(), &backup.to_text()).map_err(|e| e.to_string())?;
        let addr = wallet.address().to_string();
        self.wallet = Some(wallet);
        self.libro = Libro::nueva(&addr);
        self.retro = 0;
        self.ultimo_envio = None;
        self.ultimo_fee = None;
        self.ultimo_cambio = None;
        self.envio_aviso = None;
        self.scan_aviso = None;
        self.guardar_libro();
        Ok(addr)
    }

    fn pedir_envio(&mut self, destino: &str, monto: &str) -> Result<(), String> {
        if self.wallet.is_none() {
            let code = "codigo:sin-semilla";
            self.envio_aviso = Some(code.into());
            return Err(code.into());
        }
        if self.pedido_envio.is_some() || self.enviando {
            let code = "codigo:en-curso";
            self.envio_aviso = Some(code.into());
            return Err(code.into());
        }
        let destino = destino.trim();
        if destino.is_empty() {
            let code = "codigo:destino";
            self.envio_aviso = Some(code.into());
            return Err(code.into());
        }
        coop::parse_address(destino, Net::Stagenet.oxide()).map_err(|e| e.to_string())?;
        let monto = personal::piconero_de(monto).map_err(|e| e.to_string())?;
        if monto == 0 {
            let code = "codigo:monto-cero";
            self.envio_aviso = Some(code.into());
            return Err(code.into());
        }
        if let Some(code) = self.corte_envio(monto) {
            self.envio_aviso = Some(code.into());
            return Err(code.into());
        }
        self.envio_aviso = None;
        self.pedido_envio = Some(PedidoEnvio {
            destino: destino.to_string(),
            monto,
        });
        Ok(())
    }

    fn exportar_semilla(&self, path: &Path) -> Result<(), String> {
        let text = backup::read_secret_file(&semilla_path()).map_err(|e| e.to_string())?;
        let parsed = SeedBackup::parse(&text).map_err(|e| e.to_string())?;
        backup::write_secret_file(path, &parsed.to_text()).map_err(|e| e.to_string())
    }

    fn pedir_fondeo(&mut self, obra: &Obra, partida: usize, yo: &Persona) -> Result<(), String> {
        self.insertar_fondeo(obra, partida, yo, false)
    }

    fn insertar_fondeo(
        &mut self,
        obra: &Obra,
        partida: usize,
        yo: &Persona,
        desde_par: bool,
    ) -> Result<(), String> {
        let p = obra.partidas.get(partida).ok_or("no está esa partida")?;
        if p.estado != PartidaEstado::Encerrando {
            return Err("el encierre no está propuesto".into());
        }
        if rol_en(obra, &yo.id).is_none() {
            return Err("no estás en esta obra".into());
        }
        let capital = a_piconero(p.capital(obra.garantia)).ok_or("el monto no entra en piconero")?;
        let key = (obra.id.clone(), partida);
        if self.fondeos.contains_key(&key) {
            return Ok(());
        }
        let soy_mandante = obra.mandante.id == yo.id;
        let minimo = if soy_mandante {
            capital.saturating_add(FEE_CUSHION)
        } else {
            capital
        };
        let (aviso, espera) = self.clasificar_fondeo(&obra.id, minimo);
        let hay_aviso = aviso.is_some();
        self.fondeos.insert(
            key,
            Fondeo {
                peer: otro_id(obra, &yo.id).to_string(),
                capital,
                soy_mandante,
                propuesta: None,
                outputs: None,
                esqueleto: None,
                sobre: None,
                blob: None,
                txid: None,
                visto: false,
                error: None,
                aviso,
                aviso_enviado: !hay_aviso,
                ultimo: None,
                avisar: !desde_par && !hay_aviso && !espera,
                avise_pedir: false,
                espera_moneda: espera,
                dije_busqueda: !espera,
                viene_del_par: desde_par,
                solo_aviso: false,
                par_buscando: false,
                abortar: false,
            },
        );
        Ok(())
    }

    fn reintentar_fondeo(&mut self, obra: &Obra, partida: usize, yo: &Persona) -> Result<(), String> {
        let key = (obra.id.clone(), partida);
        let ya_pidio = self.fondeos.get(&key).is_some_and(|f| f.avise_pedir);
        let sigue_publicado = self
            .fondeos
            .get(&key)
            .is_some_and(|f| f.txid.is_some() && f.error.is_none());
        if sigue_publicado {
            return Ok(());
        }
        if self
            .fondeos
            .get(&key)
            .is_some_and(|f| f.solo_aviso && f.txid.is_none())
        {
            self.fondeos.remove(&key);
            return self.pedir_fondeo(obra, partida, yo);
        }
        if self.fondeos.contains_key(&key) {
            let peer = self
                .fondeos
                .get(&key)
                .map(|f| f.peer.clone())
                .unwrap_or_default();
            let soy_mandante = self.fondeos.get(&key).is_some_and(|f| f.soy_mandante);
            let capital = self.fondeos.get(&key).map(|f| f.capital).unwrap_or(0);
            let viene = self.fondeos.get(&key).is_some_and(|f| f.viene_del_par);
            let minimo = if soy_mandante {
                capital.saturating_add(FEE_CUSHION)
            } else {
                capital
            };
            let (aviso, espera) = self.clasificar_fondeo(&obra.id, minimo);
            let hay_aviso = aviso.is_some();
            self.fondeos.insert(
                key,
                Fondeo {
                    peer,
                    capital,
                    soy_mandante,
                    propuesta: None,
                    outputs: None,
                    esqueleto: None,
                    sobre: None,
                    blob: None,
                    txid: None,
                    visto: false,
                    error: None,
                    aviso,
                    aviso_enviado: !hay_aviso,
                    ultimo: None,
                    avisar: !ya_pidio && !viene && !hay_aviso && !espera,
                    avise_pedir: false,
                    espera_moneda: espera,
                    dije_busqueda: !espera,
                    viene_del_par: viene,
                    solo_aviso: false,
                    par_buscando: false,
                    abortar: ya_pidio,
                },
            );
            return Ok(());
        }
        self.pedir_fondeo(obra, partida, yo)
    }

    fn cancelar_fondeo(&mut self, obra: &str, partida: usize) {
        if let Some(f) = self.fondeos.get(&(obra.to_string(), partida)) {
            if f.txid.is_some() {
                return;
            }
        }
        self.fondeos.remove(&(obra.to_string(), partida));
        let _ = std::fs::remove_file(espera_path(obra, partida, false));
    }

    fn pedir_gasto(&mut self, obra: &Obra, partida: usize, yo: &Persona) -> Result<(), String> {
        let p = obra.partidas.get(partida).ok_or("no está esa partida")?;
        if p.estado != PartidaEstado::EnTrato {
            return Err("no hay un porcentaje sobre la mesa".into());
        }
        if rol_en(obra, &yo.id).is_none() {
            return Err("no estás en esta obra".into());
        }
        let pct = p.propuesto.ok_or("falta el porcentaje")?;
        let capital = a_piconero(p.capital(obra.garantia)).ok_or("el monto no entra en piconero")?;
        let key = (obra.id.clone(), partida);
        if let Some(g) = self.gastos.get(&key) {
            if g.error.is_none() {
                return Ok(());
            }
        }
        let error = if self.wallet.is_none() {
            Some("sin-semilla".to_string())
        } else if !self.cuentas.contains_key(&obra.id) {
            Some("sin-caja".to_string())
        } else if self.pares.get(&obra.id).is_none() {
            Some("sin-direccion".to_string())
        } else {
            None
        };
        self.gastos.insert(
            key,
            Gasto {
                peer: otro_id(obra, &yo.id).to_string(),
                capital,
                pct,
                soy_mandante: obra.mandante.id == yo.id,
                propuesta: None,
                session: None,
                signed: None,
                pre_otro: None,
                share_otro: None,
                envie_open: false,
                envie_pre: false,
                envie_share: false,
                blob: None,
                txid: None,
                visto: false,
                error: error.clone(),
                ultimo: None,
                avisar: true,
            },
        );
        Ok(())
    }

    fn ingerir(&mut self, msgs: Vec<CajaMsg>, yo: &Persona, obras: &[Obra]) {
        for m in msgs {
            if m.de == yo.id {
                continue;
            }
            match m.paso.as_str() {
                "direccion" => {
                    if let Ok(s) = String::from_utf8(m.cuerpo) {
                        self.guardar_par(&m.obra, s.trim());
                    }
                }
                "dkg-commit" => self.buffer_dkg(&m.obra, "commit", m.cuerpo),
                "dkg-share" => self.buffer_dkg(&m.obra, "share", m.cuerpo),
                "dkg-view" => self.buffer_dkg(&m.obra, "view", m.cuerpo),
                "dkg-pedir" => {
                    self.pedir_view.insert(m.obra);
                }
                "fund-pedir" => self.abrir_fondeo_red(&m, obras, yo),
                "fund-proposal" => self.tomar_propuesta(&m),
                "fund-skeleton" => self.tomar_esqueleto(&m),
                "fund-tx" => self.tomar_txid(&m, false),
                "fund-abort" => {
                    if let Ok(i) = parse_idx(&m.cuerpo) {
                        self.cancelar_fondeo(&m.obra, i);
                    }
                }
                "fund-error" => self.fallo_par(&m, false),
                "spend-pedir" => self.abrir_gasto_red(&m, obras, yo),
                "spend-open" => self.tomar_apertura(&m),
                "spend-pre" => self.poner_pre(&m),
                "spend-share" => self.poner_share(&m),
                "spend-tx" => self.tomar_txid(&m, true),
                "spend-error" => self.fallo_par(&m, true),
                _ => {}
            }
        }
    }

    fn buffer_dkg(&mut self, obra: &str, cual: &str, cuerpo: Vec<u8>) {
        if self.cuentas.contains_key(obra) {
            if cual == "view" {
                self.pedir_view.insert(obra.to_string());
            }
            return;
        }
        let poner = |slot: &mut Option<Vec<u8>>| {
            if slot.is_none() {
                *slot = Some(cuerpo);
            }
        };
        if let Some(run) = self.dkg.get_mut(obra) {
            match cual {
                "commit" => poner(&mut run.commit_otro),
                "share" => poner(&mut run.share_otro),
                "view" => poner(&mut run.view_otro),
                _ => {}
            }
            return;
        }
        let inbox = self.dkg_inbox.entry(obra.to_string()).or_insert(DkgInbox {
            commit_otro: None,
            share_otro: None,
            view_otro: None,
        });
        match cual {
            "commit" => poner(&mut inbox.commit_otro),
            "share" => poner(&mut inbox.share_otro),
            "view" => poner(&mut inbox.view_otro),
            _ => {}
        }
    }

    fn abrir_fondeo_red(&mut self, m: &CajaMsg, obras: &[Obra], yo: &Persona) {
        let Ok(partida) = parse_idx(&m.cuerpo) else {
            return;
        };
        let Some(obra) = obras.iter().find(|o| o.id == m.obra) else {
            return;
        };
        let key = (obra.id.clone(), partida);
        if let Some(f) = self.fondeos.get(&key) {
            if f.solo_aviso && f.txid.is_none() {
                self.fondeos.remove(&key);
            } else {
                if let Some(f) = self.fondeos.get_mut(&key) {
                    f.par_buscando = false;
                    if f.aviso.is_some_and(aviso_es_del_otro) {
                        f.aviso = None;
                    }
                }
                return;
            }
        }
        let _ = self.insertar_fondeo(obra, partida, yo, true);
    }

    fn abrir_gasto_red(&mut self, m: &CajaMsg, obras: &[Obra], yo: &Persona) {
        let Ok(partida) = parse_idx(&m.cuerpo) else {
            return;
        };
        let Some(obra) = obras.iter().find(|o| o.id == m.obra) else {
            return;
        };
        if self.gastos.contains_key(&(obra.id.clone(), partida)) {
            return;
        }
        let _ = self.pedir_gasto(obra, partida, yo);
    }

    fn tomar_propuesta(&mut self, m: &CajaMsg) {
        let Ok(sobre) = serde_json::from_slice::<SobreFondeo>(&m.cuerpo) else {
            return;
        };
        let Some((_, f)) = self.fondeos.iter_mut().find(|(k, _)| k.0 == m.obra && f_peer(k, &m.de)) else {
            return;
        };
        if f.soy_mandante {
            return;
        }
        f.sobre = Some(sobre);
        f.error = None;
        soltar_aviso_ajeno(f);
    }

    fn tomar_esqueleto(&mut self, m: &CajaMsg) {
        let Ok(skel) = coop::decode_bincode::<Skeleton>(&m.cuerpo) else {
            return;
        };
        if let Some(f) = self
            .fondeos
            .iter_mut()
            .find(|(k, f)| k.0 == m.obra && f.peer == m.de)
            .map(|(_, f)| f)
        {
            if f.soy_mandante {
                f.esqueleto = Some(skel);
                f.error = None;
                soltar_aviso_ajeno(f);
            }
        }
    }

    fn tomar_txid(&mut self, m: &CajaMsg, pago: bool) {
        let Ok(txid) = String::from_utf8(m.cuerpo.clone()) else {
            return;
        };
        let txid = txid.trim().to_string();
        if txid.len() < 64 {
            return;
        }
        if pago {
            if let Some(g) = self
                .gastos
                .iter_mut()
                .find(|(k, g)| k.0 == m.obra && g.peer == m.de)
                .map(|(_, g)| g)
            {
                g.txid = Some(txid);
                g.error = None;
            }
        } else if let Some(f) = self
            .fondeos
            .iter_mut()
            .find(|(k, f)| k.0 == m.obra && f.peer == m.de)
            .map(|(_, f)| f)
        {
            f.txid = Some(txid);
            f.error = None;
        }
    }

    fn fallo_par(&mut self, m: &CajaMsg, pago: bool) {
        let Ok(txt) = String::from_utf8(m.cuerpo.clone()) else {
            return;
        };
        let (partida_msg, code) = partir_aviso(&txt);
        let partida = partida_msg.or_else(|| {
            if pago {
                self.gastos
                    .keys()
                    .find(|(obra, _)| obra == &m.obra)
                    .map(|(_, i)| *i)
            } else {
                self.fondeos
                    .keys()
                    .find(|(obra, _)| obra == &m.obra)
                    .map(|(_, i)| *i)
            }
        });
        let Some(partida) = partida else {
            return;
        };
        if pago {
            let shown = codigo_del_otro(code);
            let key = (m.obra.clone(), partida);
            if let Some(g) = self.gastos.get_mut(&key) {
                g.error = Some(shown);
                return;
            }
            self.gastos.insert(
                key,
                Gasto {
                    peer: m.de.clone(),
                    capital: 0,
                    pct: 0,
                    soy_mandante: false,
                    propuesta: None,
                    session: None,
                    signed: None,
                    pre_otro: None,
                    share_otro: None,
                    envie_open: false,
                    envie_pre: false,
                    envie_share: false,
                    blob: None,
                    txid: None,
                    visto: false,
                    error: Some(shown),
                    ultimo: None,
                    avisar: false,
                },
            );
            return;
        }
        if code == "buscando" {
            self.marcar_busqueda_par(&m.obra, partida, &m.de);
            return;
        }
        if let Some(aviso) = aviso_del_otro(code) {
            self.poner_aviso_par(&m.obra, partida, &m.de, aviso);
            return;
        }
        let key = (m.obra.clone(), partida);
        if let Some(f) = self.fondeos.get_mut(&key) {
            f.error = Some(code.to_string());
            f.aviso_enviado = true;
            return;
        }
        self.fondeos
            .insert(key, fondeo_vacio(&m.de, Some(code.to_string()), None));
    }

    fn poner_pre(&mut self, m: &CajaMsg) {
        if let Some(g) = self
            .gastos
            .iter_mut()
            .find(|(k, g)| k.0 == m.obra && g.peer == m.de)
            .map(|(_, g)| g)
        {
            g.pre_otro = Some(m.cuerpo.clone());
        }
    }

    fn poner_share(&mut self, m: &CajaMsg) {
        if let Some(g) = self
            .gastos
            .iter_mut()
            .find(|(k, g)| k.0 == m.obra && g.peer == m.de)
            .map(|(_, g)| g)
        {
            g.share_otro = Some(m.cuerpo.clone());
        }
    }

    fn tomar_apertura(&mut self, m: &CajaMsg) {
        let Ok(sobre) = serde_json::from_slice::<SobreGasto>(&m.cuerpo) else {
            return;
        };
        let Ok(prop) = coop::decode_bincode::<SpendProposal>(&hex_bytes(&sobre.proposal)) else {
            return;
        };
        let Ok(pre) = hex::decode(sobre.pre) else {
            return;
        };
        if let Some(g) = self
            .gastos
            .iter_mut()
            .find(|(k, g)| k.0 == m.obra && g.peer == m.de)
            .map(|(_, g)| g)
        {
            if !g.soy_mandante {
                g.propuesta = Some(prop);
                g.pre_otro = Some(pre);
                g.error = None;
            }
        }
    }

    fn limpiar_resueltos(&mut self, obras: &[Obra]) {
        let muertos: Vec<_> = self
            .fondeos
            .iter()
            .filter_map(|((obra, i), _)| {
                let p = obras.iter().find(|o| o.id == *obra)?.partidas.get(*i)?;
                (p.estado != PartidaEstado::Encerrando).then_some((obra.clone(), *i))
            })
            .collect();
        for (obra, i) in muertos {
            self.fondeos.remove(&(obra.clone(), i));
            let _ = std::fs::remove_file(espera_path(&obra, i, false));
        }
        let muertos: Vec<_> = self
            .gastos
            .iter()
            .filter_map(|((obra, i), _)| {
                let p = obras.iter().find(|o| o.id == *obra)?.partidas.get(*i)?;
                (p.estado != PartidaEstado::EnTrato).then_some((obra.clone(), *i))
            })
            .collect();
        for (obra, i) in muertos {
            self.gastos.remove(&(obra.clone(), i));
            let _ = std::fs::remove_file(espera_path(&obra, i, true));
        }
    }

    fn avanzar_dkg(&mut self, nodo: &Nodo, yo: &Persona, obras: &[Obra]) {
        for obra in obras {
            if !trato_firme(obra.estado) {
                continue;
            }
            let Some(rol) = rol_en(obra, &yo.id) else {
                continue;
            };
            let peer = otro_id(obra, &yo.id);
            if self.cuentas.contains_key(&obra.id) {
                if rol == Party::Mandante && self.pedir_view.remove(&obra.id) {
                    if let Some(cuenta) = self.cuentas.get(&obra.id) {
                        if let Ok(anuncio) = anuncio_de(cuenta) {
                            let _ = nodo.enviar_caja(&obra.id, peer, &yo.id, "dkg-view", &anuncio.encode());
                        }
                    }
                }
                continue;
            }
            if self.wallet.is_none() {
                continue;
            }
            if !self.dkg.contains_key(&obra.id) {
                match DkgParty::start(rol, &obra.id, Net::Stagenet, &mut OsRng) {
                    Ok((party, commit)) => {
                        let prev = self.dkg_inbox.remove(&obra.id).unwrap_or(DkgInbox {
                            commit_otro: None,
                            share_otro: None,
                            view_otro: None,
                        });
                        self.dkg.insert(
                            obra.id.clone(),
                            DkgRun {
                                party,
                                commit,
                                commit_otro: prev.commit_otro,
                                share_otro: prev.share_otro,
                                view_otro: prev.view_otro,
                                envie_commit: false,
                                envie_share: false,
                                envie_view: false,
                            },
                        );
                    }
                    Err(e) => {
                        self.nota(&obra.id, None, Texto::Falla(e.to_string()));
                        continue;
                    }
                }
            }
            let mut nota = None;
            let mut cerrar = None;
            {
                let Some(run) = self.dkg.get_mut(&obra.id) else {
                    continue;
                };
                if !run.envie_commit {
                    run.envie_commit = nodo.enviar_caja(&obra.id, peer, &yo.id, "dkg-commit", &run.commit.clone());
                    if !run.envie_commit {
                        nota = Some(Texto::SinOtro);
                    }
                }
                if let Some(bytes) = run.commit_otro.clone() {
                    match run.party.ingest_commit(&bytes, &mut OsRng) {
                        Ok(share) => {
                            run.commit_otro = None;
                            run.envie_share = nodo.enviar_caja(&obra.id, peer, &yo.id, "dkg-share", &share);
                        }
                        Err(e) if e.to_string().contains("no toca") => run.commit_otro = None,
                        Err(e) => nota = Some(Texto::Falla(e.to_string())),
                    }
                }
                if let Some(bytes) = run.share_otro.clone() {
                    match run.party.ingest_share(&bytes, &mut OsRng) {
                        Ok(out) => {
                            run.share_otro = None;
                            if let Some(view) = out.view {
                                run.envie_view = nodo.enviar_caja(&obra.id, peer, &yo.id, "dkg-view", &view.encode());
                            }
                            if let Some(account) = out.account {
                                cerrar = Some(account);
                            }
                        }
                        Err(e) if e.to_string().contains("no toca") => run.share_otro = None,
                        Err(e) => nota = Some(Texto::Falla(e.to_string())),
                    }
                }
                if cerrar.is_none() {
                    if let Some(bytes) = run.view_otro.clone() {
                        match ViewAnnounce::decode(&bytes) {
                            Ok(view) => match run.party.ingest_view(&view) {
                                Ok(account) => cerrar = Some(account),
                                Err(e) if e.to_string().contains("no toca") => run.view_otro = None,
                                Err(e) => nota = Some(Texto::Falla(e.to_string())),
                            },
                            Err(e) => nota = Some(Texto::Falla(e.to_string())),
                        }
                    }
                }
            }
            if let Some(account) = cerrar {
                let id = obra.id.clone();
                self.guardar_cuenta(account);
                self.dkg.remove(&id);
            }
            if let Some(texto) = nota {
                self.nota(&obra.id, None, texto);
            }
            if rol == Party::Contratista && !self.cuentas.contains_key(&obra.id) {
                let _ = nodo.enviar_caja(&obra.id, peer, &yo.id, "dkg-pedir", b"");
            }
        }
    }

    fn guardar_cuenta(&mut self, account: JointAccount) {
        let Ok(backup) = account.backup() else {
            return;
        };
        let path = share_path(account.obra_id());
        if !path.exists() {
            let _ = backup::write_secret_file(&path, &backup.to_text());
        }
        self.cuentas.insert(account.obra_id().to_string(), account);
    }

    fn mandar_direccion(&mut self, nodo: &Nodo, yo: &Persona, obras: &[Obra]) {
        let Some(addr) = self.wallet.as_ref().map(|w| w.address().to_string()) else {
            return;
        };
        for obra in obras {
            if rol_en(obra, &yo.id).is_none() || !trato_firme(obra.estado) {
                continue;
            }
            let _ = nodo.enviar_caja(&obra.id, otro_id(obra, &yo.id), &yo.id, "direccion", addr.as_bytes());
        }
    }

    fn avisar_pedidos(&mut self, nodo: &Nodo, yo: &Persona) {
        let abortos: Vec<_> = self
            .fondeos
            .iter()
            .filter(|(_, f)| f.abortar)
            .map(|((o, i), f)| (o.clone(), *i, f.peer.clone()))
            .collect();
        for (obra, i, peer) in abortos {
            let cuerpo = i.to_string().into_bytes();
            if nodo.enviar_caja(&obra, &peer, &yo.id, "fund-abort", &cuerpo) {
                if let Some(f) = self.fondeos.get_mut(&(obra, i)) {
                    f.abortar = false;
                    f.avisar = f.aviso.is_none() && !f.espera_moneda && !f.viene_del_par;
                }
            }
        }
        let pedidos: Vec<_> = self
            .fondeos
            .iter()
            .filter(|(_, f)| f.avisar && !f.abortar)
            .map(|((o, i), f)| (o.clone(), *i, f.peer.clone()))
            .collect();
        for (obra, i, peer) in pedidos {
            let cuerpo = i.to_string().into_bytes();
            if nodo.enviar_caja(&obra, &peer, &yo.id, "fund-pedir", &cuerpo) {
                if let Some(f) = self.fondeos.get_mut(&(obra, i)) {
                    f.avisar = false;
                    f.avise_pedir = true;
                    f.espera_moneda = false;
                }
            }
        }
        let pedidos: Vec<_> = self
            .gastos
            .iter()
            .filter(|(_, g)| g.avisar && g.error.is_none())
            .map(|((o, i), g)| (o.clone(), *i, g.peer.clone()))
            .collect();
        for (obra, i, peer) in pedidos {
            let cuerpo = i.to_string().into_bytes();
            if nodo.enviar_caja(&obra, &peer, &yo.id, "spend-pedir", &cuerpo) {
                if let Some(g) = self.gastos.get_mut(&(obra, i)) {
                    g.avisar = false;
                }
            }
        }
    }

    fn cerrar_si_puede(&mut self, nodo: &Nodo, yo: &Persona) {
        let listos: Vec<(String, usize)> = self
            .fondeos
            .iter()
            .filter(|(_, f)| {
                f.soy_mandante
                    && f.esqueleto.is_some()
                    && f.blob.is_none()
                    && f.propuesta.is_some()
                    && f.outputs.is_some()
                    && f.error.is_none()
                    && f.aviso.is_none()
            })
            .map(|((o, i), _)| (o.clone(), *i))
            .collect();
        for (obra, i) in listos {
            if self.ocupado.is_some() {
                break;
            }
            let Some(wallet) = self.wallet.as_ref() else {
                continue;
            };
            let Some(f) = self.fondeos.get(&(obra.clone(), i)) else {
                continue;
            };
            let propuesta = f.propuesta.clone();
            let esqueleto = f.esqueleto.clone();
            let outputs = f.outputs.clone();
            let (Some(propuesta), Some(esqueleto), Some(outputs)) = (propuesta, esqueleto, outputs) else {
                continue;
            };
            match fund::mandante_cierra(Net::Stagenet, &propuesta, &esqueleto, &outputs, wallet.spend_key()) {
                Ok(tx) => {
                    let txid = hex::encode(tx.hash());
                    let blob = tx.serialize();
                    if let Some(f) = self.fondeos.get_mut(&(obra.clone(), i)) {
                        f.blob = Some(blob.clone());
                        f.txid = Some(txid.clone());
                        f.ultimo = Some(Instant::now());
                    }
                    self.guardar_espera(&obra, i, false, &txid);
                    self.publicar(obra, i, false, blob, txid, nodo, yo);
                }
                Err(e) => self.fallar(&obra, i, false, e.to_string(), nodo, yo),
            }
        }
    }

    fn firmar_si_puede(&mut self, nodo: &Nodo, yo: &Persona) {
        let claves: Vec<(String, usize)> = self.gastos.keys().cloned().collect();
        for (obra, i) in claves {
            if self
                .gastos
                .get(&(obra.clone(), i))
                .is_some_and(|g| g.error.is_some())
            {
                continue;
            }
            let Some(cuenta) = self.cuentas.get(&obra) else {
                continue;
            };
            let cuenta_keys_obra = cuenta.obra_id().to_string();
            if cuenta_keys_obra != obra {
                continue;
            }
            // Open local session once we have a proposal.
            let abrir = self.gastos.get(&(obra.clone(), i)).is_some_and(|g| g.propuesta.is_some() && g.session.is_none() && g.signed.is_none());
            if abrir {
                let prop = self.gastos.get(&(obra.clone(), i)).and_then(|g| g.propuesta.clone());
                if let Some(prop) = prop {
                    match SpendSession::open(cuenta, &prop, &mut OsRng) {
                        Ok((session, pre)) => {
                            let peer = self.gastos.get(&(obra.clone(), i)).map(|g| g.peer.clone()).unwrap_or_default();
                            let soy_m = self.gastos.get(&(obra.clone(), i)).is_some_and(|g| g.soy_mandante);
                            if soy_m {
                                let sobre = SobreGasto {
                                    proposal: hex::encode(coop::encode_bincode(&prop).unwrap_or_default()),
                                    pre: hex::encode(&pre),
                                };
                                if let Ok(body) = serde_json::to_vec(&sobre) {
                                    let enviado = nodo.enviar_caja(&obra, &peer, &yo.id, "spend-open", &body);
                                    if let Some(g) = self.gastos.get_mut(&(obra.clone(), i)) {
                                        g.envie_open = enviado;
                                        g.session = Some(session);
                                    }
                                }
                            } else {
                                let enviado = nodo.enviar_caja(&obra, &peer, &yo.id, "spend-pre", &pre);
                                if let Some(g) = self.gastos.get_mut(&(obra.clone(), i)) {
                                    g.envie_pre = enviado;
                                    g.session = Some(session);
                                }
                            }
                        }
                        Err(e) => self.fallar(&obra, i, true, e.to_string(), nodo, yo),
                    }
                }
            }
            let puedo_firmar = self.gastos.get(&(obra.clone(), i)).is_some_and(|g| g.session.is_some() && g.pre_otro.is_some() && g.signed.is_none() && g.error.is_none());
            if puedo_firmar {
                let pre = self.gastos.get(&(obra.clone(), i)).and_then(|g| g.pre_otro.clone());
                let session = self.gastos.get_mut(&(obra.clone(), i)).and_then(|g| g.session.take());
                if let (Some(session), Some(pre)) = (session, pre) {
                    match session.sign(&pre) {
                        Ok((signed, share)) => {
                            let peer = self.gastos.get(&(obra.clone(), i)).map(|g| g.peer.clone()).unwrap_or_default();
                            let enviado = nodo.enviar_caja(&obra, &peer, &yo.id, "spend-share", &share);
                            if let Some(g) = self.gastos.get_mut(&(obra.clone(), i)) {
                                g.signed = Some(signed);
                                g.envie_share = enviado;
                            }
                        }
                        Err(e) => self.fallar(&obra, i, true, e.to_string(), nodo, yo),
                    }
                }
            }
            let puedo_cerrar = self.gastos.get(&(obra.clone(), i)).is_some_and(|g| g.signed.is_some() && g.share_otro.is_some() && g.blob.is_none() && g.error.is_none());
            let soy_m_antes = self.gastos.get(&(obra.clone(), i)).is_some_and(|g| g.soy_mandante);
            if puedo_cerrar && soy_m_antes && self.ocupado.is_some() {
                continue;
            }
            if puedo_cerrar {
                let share = self.gastos.get(&(obra.clone(), i)).and_then(|g| g.share_otro.clone());
                let signed = self.gastos.get_mut(&(obra.clone(), i)).and_then(|g| g.signed.take());
                let soy_m = self.gastos.get(&(obra.clone(), i)).is_some_and(|g| g.soy_mandante);
                if let (Some(signed), Some(share)) = (signed, share) {
                    match signed.complete(&share) {
                        Ok(tx) => {
                            let txid = hex::encode(tx.hash());
                            let blob = tx.serialize();
                            if let Some(g) = self.gastos.get_mut(&(obra.clone(), i)) {
                                g.blob = Some(blob.clone());
                                g.txid = Some(txid.clone());
                            }
                            if soy_m {
                                self.guardar_espera(&obra, i, true, &txid);
                                self.publicar(obra.clone(), i, true, blob, txid, nodo, yo);
                            }
                        }
                        Err(e) => self.fallar(&obra, i, true, e.to_string(), nodo, yo),
                    }
                }
            }
        }
    }

    fn publicar(&mut self, obra: String, partida: usize, pago: bool, blob: Vec<u8>, txid: String, _nodo: &Nodo, _yo: &Persona) {
        if self.ocupado.is_some() {
            return;
        }
        self.generacion = self.generacion.wrapping_add(1);
        let gen = self.generacion;
        let celda = Arc::new(Mutex::new(None));
        self.ocupado = Some(Pendiente {
            gen,
            celda: celda.clone(),
        });
        tokio::spawn(async move {
            let res = match chain::connect(STAGENET_DAEMON).await {
                Ok(rpc) => match chain::publish_bytes(&rpc, &blob).await {
                    Ok(()) => Listo::Publicado {
                        obra,
                        partida,
                        pago,
                        txid,
                        ok: Ok(()),
                    },
                    Err(e) => Listo::Publicado {
                        obra,
                        partida,
                        pago,
                        txid,
                        ok: Err(e.to_string()),
                    },
                },
                Err(e) => Listo::Fallo {
                    obra,
                    partida: Some(partida),
                    pago: Some(pago),
                    msg: e.to_string(),
                },
            };
            *celda.lock().unwrap() = Some(res);
        });
    }

    fn lanzar_si_toca(&mut self, nodo: &Nodo, yo: &Persona) {
        if self.ocupado.is_some() {
            return;
        }
        if self.tip_en.is_none_or(|t| t.elapsed() >= Duration::from_secs(60)) {
            self.spawn_tip();
            return;
        }
        if let Some((obra, i, pago)) = self.busca_ver() {
            self.spawn_ver(obra, i, pago);
            return;
        }
        self.empujar_historia();
        self.cerrar_busqueda_vacia();
        self.emitir_bloqueos(nodo, yo);
        if let Some((obra, i)) = self.busca_entradas(false) {
            self.spawn_entradas(obra, i, false, nodo, yo);
            return;
        }
        if let Some((obra, i)) = self.busca_entradas(true) {
            self.spawn_entradas(obra, i, true, nodo, yo);
            return;
        }
        if self.lanzar_envio() {
            return;
        }
        self.lanzar_saldo();
    }

    fn lanzar_envio(&mut self) -> bool {
        if self.pedido_envio.is_none() || self.ocupado.is_some() {
            return false;
        }
        if self.wallet.is_none() {
            self.pedido_envio = None;
            self.envio_aviso = Some("codigo:sin-semilla".into());
            return true;
        }
        if self.tip.is_none() {
            self.envio_aviso = Some("codigo:sin-punta".into());
            return true;
        }
        if let Some(pedido) = &self.pedido_envio {
            if let Some(code) = self.corte_envio(pedido.monto) {
                self.pedido_envio = None;
                self.envio_aviso = Some(code.into());
                return true;
            }
        }
        let armado = match self.armar_envio() {
            Ok(job) => job,
            Err(msg) => {
                self.pedido_envio = None;
                self.envio_aviso = Some(msg);
                return true;
            }
        };
        self.pedido_envio = None;
        self.enviando = true;
        self.envio_aviso = None;
        let celda = self.ocupar();
        tokio::spawn(async move {
            let ok = personal::publicar(
                armado.spend,
                armado.view,
                armado.crudas,
                &armado.destino,
                armado.monto,
                Net::Stagenet,
            )
            .await
            .map(|e| EnvioHecho {
                txid: e.txid,
                fee: e.fee,
                cambio: e.cambio,
            })
            .map_err(|e| e.to_string());
            let usadas = if ok.is_ok() { armado.usadas } else { Vec::new() };
            *celda.lock().unwrap() = Some(Listo::Envio { ok, usadas });
        });
        true
    }

    fn armar_envio(&self) -> Result<EnvioJob, String> {
        let pedido = self.pedido_envio.as_ref().ok_or("no hay envío")?;
        let wallet = self
            .wallet
            .as_ref()
            .ok_or("primero creá la billetera de stagenet")?;
        let tip = self.tip.ok_or("todavía no llega la punta del nodo")?;
        let libres: Vec<&Entrada> = self
            .libro
            .entradas
            .iter()
            .filter(|e| tip >= e.altura.saturating_add(10))
            .collect();
        let montos: Vec<u64> = libres.iter().map(|e| e.monto).collect();
        let necesita = pedido.monto.saturating_add(FEE_CUSHION);
        let idxs = match elegir_montos(&montos, necesita) {
            Ok(idxs) => idxs,
            Err(_) => {
                let todos: Vec<u64> = self.libro.entradas.iter().map(|e| e.monto).collect();
                let code = corte_envio(&montos, &todos, pedido.monto, true)
                    .unwrap_or("codigo:sin-saldo");
                return Err(code.to_string());
            }
        };
        let mut crudas = Vec::new();
        let mut usadas = Vec::new();
        for i in idxs {
            crudas.push(libres[i].raw.clone());
            usadas.push((libres[i].tx.clone(), libres[i].indice));
        }
        Ok(EnvioJob {
            spend: wallet.spend_key().clone(),
            view: wallet.view_pair(),
            crudas,
            usadas,
            destino: pedido.destino.clone(),
            monto: pedido.monto,
        })
    }

    fn lanzar_saldo(&mut self) {
        if self.wallet.is_none() || self.ocupado.is_some() || !frio(self.scan_pausa) {
            return;
        }
        let Some(tip) = self.tip else {
            return;
        };
        if !self.libro.listo {
            let desde = tip.saturating_sub(LOOKBACK);
            self.libro.desde = desde;
            self.libro.hasta = desde.saturating_sub(1);
            self.libro.listo = true;
            if let Some(w) = self.wallet.as_ref() {
                self.libro.direccion = w.address().to_string();
            }
            self.guardar_libro();
        }
        if self.forzar {
            self.forzar = false;
            let piso = self.libro.desde.saturating_sub(1);
            self.libro.hasta = self.libro.hasta.saturating_sub(8).max(piso);
        }
        let view = self.wallet.as_ref().unwrap().view_pair();
        if self.libro.hasta < tip {
            let desde = self.libro.hasta.saturating_add(1);
            let hasta = desde.saturating_add(7).min(tip);
            self.spawn_saldo(view, desde, hasta, false);
            return;
        }
        if self.retro > 0 && self.libro.desde > 0 {
            let n = self.retro.min(8).min(self.libro.desde);
            let hasta = self.libro.desde - 1;
            let desde = hasta + 1 - n;
            self.spawn_saldo(view, desde, hasta, true);
        }
    }

    fn spawn_saldo(&mut self, view: xmr_joint::ViewPair, desde: usize, hasta: usize, retro: bool) {
        self.buscando = true;
        let celda = self.ocupar();
        tokio::spawn(async move {
            let listo = match chain::connect(STAGENET_DAEMON).await {
                Ok(rpc) => match chain::scan_marcado(&rpc, view, desde, hasta).await {
                    Ok(marcadas) => {
                        let mut entradas = Vec::new();
                        for (altura, output) in marcadas {
                            entradas.push(EntradaNueva {
                                altura,
                                monto: output.commitment().amount,
                                tx: hex::encode(output.transaction()),
                                indice: output.index_in_transaction(),
                                raw: output.serialize(),
                            });
                        }
                        let tip = chain::tip(&rpc).await.unwrap_or(hasta);
                        Listo::Saldo {
                            desde,
                            hasta,
                            tip,
                            retro,
                            entradas,
                        }
                    }
                    Err(e) => Listo::Aviso(e.to_string()),
                },
                Err(e) => Listo::Aviso(e.to_string()),
            };
            *celda.lock().unwrap() = Some(listo);
        });
    }

    fn busca_ver(&self) -> Option<(String, usize, bool)> {
        for ((obra, i), f) in &self.fondeos {
            if f.txid.is_some() && !f.visto && frio(f.ultimo) {
                return Some((obra.clone(), *i, false));
            }
        }
        for ((obra, i), g) in &self.gastos {
            if g.txid.is_some() && !g.visto && frio(g.ultimo) {
                return Some((obra.clone(), *i, true));
            }
        }
        None
    }

    fn busca_entradas(&self, pago: bool) -> Option<(String, usize)> {
        if pago {
            for ((obra, i), g) in &self.gastos {
                let falta = if g.soy_mandante {
                    g.propuesta.is_none()
                } else {
                    false
                };
                if falta
                    && g.txid.is_none()
                    && g.error.is_none()
                    && frio(g.ultimo)
                    && self.cuentas.contains_key(obra)
                {
                    return Some((obra.clone(), *i));
                }
            }
        } else {
            for ((obra, i), f) in &self.fondeos {
                let falta = if f.soy_mandante {
                    f.propuesta.is_none()
                } else {
                    f.sobre.is_some() && f.esqueleto.is_none()
                };
                let minimo = if f.soy_mandante {
                    f.capital.saturating_add(FEE_CUSHION)
                } else {
                    f.capital
                };
                if falta
                    && !f.solo_aviso
                    && !f.espera_moneda
                    && f.txid.is_none()
                    && f.error.is_none()
                    && f.aviso.is_none()
                    && frio(f.ultimo)
                    && self.salida_libre(minimo).is_some()
                {
                    return Some((obra.clone(), *i));
                }
            }
        }
        None
    }

    fn spawn_tip(&mut self) {
        self.tip_en = Some(Instant::now());
        let celda = self.ocupar();
        tokio::spawn(async move {
            let listo = match chain::connect(STAGENET_DAEMON).await {
                Ok(rpc) => match chain::tip(&rpc).await {
                    Ok(n) => Listo::Punta(n),
                    Err(e) => Listo::Fallo {
                        obra: String::new(),
                        partida: None,
                        pago: None,
                        msg: e.to_string(),
                    },
                },
                Err(e) => Listo::Fallo {
                    obra: String::new(),
                    partida: None,
                    pago: None,
                    msg: e.to_string(),
                },
            };
            *celda.lock().unwrap() = Some(listo);
        });
    }

    fn spawn_ver(&mut self, obra: String, partida: usize, pago: bool) {
        let Some(view) = self.vista_de(&obra, pago) else {
            return;
        };
        let txid = if pago {
            self.gastos.get(&(obra.clone(), partida)).and_then(|g| g.txid.clone())
        } else {
            self.fondeos.get(&(obra.clone(), partida)).and_then(|f| f.txid.clone())
        };
        let Some(txid) = txid else {
            return;
        };
        if pago {
            if let Some(g) = self.gastos.get_mut(&(obra.clone(), partida)) {
                g.ultimo = Some(Instant::now());
            }
        } else if let Some(f) = self.fondeos.get_mut(&(obra.clone(), partida)) {
            f.ultimo = Some(Instant::now());
        }
        let celda = self.ocupar();
        tokio::spawn(async move {
            let listo = ver_txid(view, &txid)
                .await
                .map(|si| Listo::Visto {
                    obra: obra.clone(),
                    partida,
                    pago,
                    si,
                })
                .unwrap_or_else(|msg| Listo::Fallo {
                    obra,
                    partida: Some(partida),
                    pago: Some(pago),
                    msg,
                });
            *celda.lock().unwrap() = Some(listo);
        });
    }

    fn spawn_entradas(&mut self, obra: String, partida: usize, pago: bool, nodo: &Nodo, yo: &Persona) {
        if !pago {
            let minimo = self
                .fondeos
                .get(&(obra.clone(), partida))
                .map(|f| {
                    if f.soy_mandante {
                        f.capital.saturating_add(FEE_CUSHION)
                    } else {
                        f.capital
                    }
                })
                .unwrap_or(0);
            let Some(raw) = self.salida_libre(minimo) else {
                return;
            };
            if let Some(f) = self.fondeos.get_mut(&(obra.clone(), partida)) {
                f.ultimo = Some(Instant::now());
            }
            let celda = self.ocupar();
            tokio::spawn(async move {
                let listo = match chain::anillar(raw).await {
                    Ok((decoys, fee)) => Listo::Entradas {
                        obra: obra.clone(),
                        partida,
                        pago: false,
                        decoys,
                        fee,
                    },
                    Err(e) => Listo::Fallo {
                        obra,
                        partida: Some(partida),
                        pago: Some(false),
                        msg: e.to_string(),
                    },
                };
                *celda.lock().unwrap() = Some(listo);
            });
            return;
        }
        let Some(cuenta) = self.cuentas.get(&obra) else {
            return;
        };
        let view = match cuenta.view_pair() {
            Ok(v) => v,
            Err(e) => {
                self.fallar(&obra, partida, true, e.to_string(), nodo, yo);
                return;
            }
        };
        let minimo = self
            .gastos
            .get(&(obra.clone(), partida))
            .map(|g| g.capital)
            .unwrap_or(0);
        let cuantos = 2;
        let exacto = self.gastos.get(&(obra.clone(), partida)).map(|g| g.capital);
        if let Some(g) = self.gastos.get_mut(&(obra.clone(), partida)) {
            g.ultimo = Some(Instant::now());
        }
        let celda = self.ocupar();
        tokio::spawn(async move {
            let listo = juntar_entradas(view, minimo, cuantos, exacto)
                .await
                .map(|(decoys, fee)| Listo::Entradas {
                    obra: obra.clone(),
                    partida,
                    pago,
                    decoys,
                    fee,
                })
                .unwrap_or_else(|msg| Listo::Fallo {
                    obra,
                    partida: Some(partida),
                    pago: Some(true),
                    msg,
                });
            *celda.lock().unwrap() = Some(listo);
        });
    }

    fn vista_de(&self, obra: &str, pago: bool) -> Option<xmr_joint::ViewPair> {
        let _ = pago;
        // Funding confirmation and spend inputs both sit on the joint view.
        // A personal-wallet confirmation is only needed for the spend outputs,
        // and the joint scan also sees them if we are a recipient... no:
        // spend outputs go to personal addresses. Scan the personal view for pago.
        if pago {
            return self.wallet.as_ref().map(|w| w.view_pair());
        }
        self.cuentas.get(obra).and_then(|c| c.view_pair().ok())
    }

    fn ocupar(&mut self) -> Arc<Mutex<Option<Listo>>> {
        self.generacion = self.generacion.wrapping_add(1);
        let celda = Arc::new(Mutex::new(None));
        self.ocupado = Some(Pendiente {
            gen: self.generacion,
            celda: celda.clone(),
        });
        celda
    }

    fn tomar_listo(&mut self, nodo: &Nodo, yo: &Persona) {
        let Some(pend) = &self.ocupado else {
            return;
        };
        let listo = pend.celda.lock().unwrap().take();
        let Some(listo) = listo else {
            return;
        };
        self.ocupado = None;
        match listo {
            Listo::Punta(n) => self.tip = Some(n),
            Listo::Fallo { obra, partida, pago, msg } => {
                if obra.is_empty() && partida.is_none() {
                    self.scan_aviso = Some(msg);
                } else if let Some(i) = partida {
                    let pago = pago.unwrap_or(false);
                    if let Some(job) = if pago {
                        self.gastos.get_mut(&(obra.clone(), i)).map(|g| &mut g.ultimo)
                    } else {
                        self.fondeos.get_mut(&(obra.clone(), i)).map(|f| &mut f.ultimo)
                    } {
                        *job = Some(Instant::now());
                    }
                    self.fallar(&obra, i, pago, msg, nodo, yo);
                }
            }
            Listo::Visto { obra, partida, pago, si } => {
                if pago {
                    if let Some(g) = self.gastos.get_mut(&(obra, partida)) {
                        g.visto = si;
                        g.ultimo = Some(Instant::now());
                        if si {
                            g.error = None;
                        }
                    }
                } else if let Some(f) = self.fondeos.get_mut(&(obra, partida)) {
                    f.visto = si;
                    f.ultimo = Some(Instant::now());
                    if si {
                        f.error = None;
                    }
                }
            }
            Listo::Entradas {
                obra,
                partida,
                pago,
                decoys,
                fee,
            } => {
                if pago {
                    self.armar_gasto(&obra, partida, decoys, fee, nodo, yo);
                } else {
                    self.armar_fondeo(&obra, partida, decoys, fee, nodo, yo);
                }
            }
            Listo::Publicado {
                obra,
                partida,
                pago,
                txid,
                ok,
            } => {
                let peer = if pago {
                    self.gastos.get(&(obra.clone(), partida)).map(|g| g.peer.clone())
                } else {
                    self.fondeos.get(&(obra.clone(), partida)).map(|f| f.peer.clone())
                };
                match ok {
                    Ok(()) => {
                        if let Some(peer) = peer {
                            let paso = if pago { "spend-tx" } else { "fund-tx" };
                            let _ = nodo.enviar_caja(&obra, &peer, &yo.id, paso, txid.as_bytes());
                        }
                    }
                    Err(e) => self.fallar(&obra, partida, pago, e, nodo, yo),
                }
            }
            Listo::Saldo {
                desde,
                hasta,
                tip,
                retro,
                entradas,
            } => {
                self.buscando = false;
                self.scan_pausa = None;
                self.scan_aviso = None;
                self.tip = Some(tip);
                self.fundir_entradas(entradas);
                if retro {
                    self.libro.desde = desde;
                    let n = hasta.saturating_sub(desde).saturating_add(1);
                    self.retro = self.retro.saturating_sub(n);
                } else {
                    self.libro.hasta = hasta;
                }
                self.guardar_libro();
            }
            Listo::Aviso(msg) => {
                self.buscando = false;
                self.scan_pausa = Some(Instant::now());
                self.scan_aviso = Some(msg);
            }
            Listo::Envio { ok, usadas } => {
                self.enviando = false;
                match ok {
                    Ok(hecho) => {
                        self.libro.entradas.retain(|e| {
                            !usadas.iter().any(|(tx, indice)| e.tx == *tx && e.indice == *indice)
                        });
                        self.guardar_libro();
                        self.ultimo_envio = Some(hecho.txid);
                        self.ultimo_fee = Some(hecho.fee);
                        self.ultimo_cambio = Some(hecho.cambio);
                        self.envio_aviso = None;
                    }
                    Err(e) => self.envio_aviso = Some(e),
                }
            }
        }
    }

    fn fundir_entradas(&mut self, entradas: Vec<EntradaNueva>) {
        for nueva in entradas {
            let ya = self
                .libro
                .entradas
                .iter()
                .any(|e| e.tx == nueva.tx && e.indice == nueva.indice);
            if ya {
                continue;
            }
            self.libro.entradas.push(Entrada {
                altura: nueva.altura,
                monto: nueva.monto,
                tx: nueva.tx,
                indice: nueva.indice,
                raw: nueva.raw,
            });
        }
    }

    fn armar_fondeo(
        &mut self,
        obra: &str,
        partida: usize,
        decoys: Vec<OutputWithDecoys>,
        fee: (u64, u64),
        nodo: &Nodo,
        yo: &Persona,
    ) {
        let key = (obra.to_string(), partida);
        let Some(wallet) = self.wallet.as_ref() else {
            self.fallar(obra, partida, false, "sin-semilla".into(), nodo, yo);
            return;
        };
        let Some(cuenta) = self.cuentas.get(obra) else {
            self.fallar(obra, partida, false, "sin-caja".into(), nodo, yo);
            return;
        };
        let joint = cuenta.address().to_string();
        let soy_m = self.fondeos.get(&key).is_some_and(|f| f.soy_mandante);
        let capital = self.fondeos.get(&key).map(|f| f.capital).unwrap_or(0);
        let peer = self.fondeos.get(&key).map(|f| f.peer.clone()).unwrap_or_default();
        if soy_m {
            let mut ovk = [0u8; 32];
            OsRng.fill_bytes(&mut ovk);
            match fund::mandante_proposal(
                Net::Stagenet,
                obra,
                partida as u32,
                &joint,
                capital,
                &decoys,
                wallet.spend_key(),
                fee,
                ovk,
            ) {
                Ok(prop) => {
                    let sobre = SobreFondeo {
                        proposal: hex::encode(coop::encode_bincode(&prop).unwrap_or_default()),
                        view: hex::encode(wallet.view_private_bytes()),
                        direccion: wallet.address().to_string(),
                    };
                    let body = serde_json::to_vec(&sobre).unwrap_or_default();
                    let _ = nodo.enviar_caja(obra, &peer, &yo.id, "fund-proposal", &body);
                    if let Some(f) = self.fondeos.get_mut(&key) {
                        f.propuesta = Some(prop);
                        f.outputs = Some(decoys);
                        f.error = None;
                    }
                }
                Err(e) => self.fallar(obra, partida, false, e.to_string(), nodo, yo),
            }
        } else {
            let sobre = self.fondeos.get(&key).and_then(|f| f.sobre.as_ref()).cloned();
            let Some(sobre) = sobre else {
                return;
            };
            let Ok(bytes) = hex::decode(&sobre.proposal) else {
                self.fallar(obra, partida, false, "propuesta ilegible".into(), nodo, yo);
                return;
            };
            let Ok(prop) = coop::decode_bincode::<Proposal>(&bytes) else {
                self.fallar(obra, partida, false, "propuesta ilegible".into(), nodo, yo);
                return;
            };
            let Ok(view_bytes) = hex::decode(&sobre.view) else {
                self.fallar(obra, partida, false, "propuesta ilegible".into(), nodo, yo);
                return;
            };
            match view_del_mandante(&sobre.direccion, &view_bytes) {
                Ok(view) => {
                    match fund::contratista_responde(
                        Net::Stagenet,
                        &joint,
                        capital,
                        &prop,
                        view,
                        &decoys,
                        wallet.spend_key(),
                        wallet.address(),
                    ) {
                        Ok(skel) => {
                            let body = coop::encode_bincode(&skel).unwrap_or_default();
                            let _ = nodo.enviar_caja(obra, &peer, &yo.id, "fund-skeleton", &body);
                            if let Some(f) = self.fondeos.get_mut(&key) {
                                f.esqueleto = Some(skel);
                                f.error = None;
                            }
                        }
                        Err(e) => self.fallar(obra, partida, false, e.to_string(), nodo, yo),
                    }
                }
                Err(e) => self.fallar(obra, partida, false, e.to_string(), nodo, yo),
            }
        }
    }

    fn armar_gasto(
        &mut self,
        obra: &str,
        partida: usize,
        decoys: Vec<OutputWithDecoys>,
        fee: (u64, u64),
        nodo: &Nodo,
        yo: &Persona,
    ) {
        let key = (obra.to_string(), partida);
        let Some(g0) = self.gastos.get(&key) else {
            return;
        };
        if !g0.soy_mandante {
            return;
        }
        let capital = g0.capital;
        let pct = g0.pct;
        let peer_addr = self.pares.get(obra).cloned();
        let propia = self.wallet.as_ref().map(|w| w.address().to_string());
        let (Some(peer_addr), Some(propia)) = (peer_addr, propia) else {
            self.fallar(obra, partida, true, "sin-direccion".into(), nodo, yo);
            return;
        };
        let soy_m = self.cuentas.get(obra).is_some_and(|c| c.role() == Party::Mandante);
        let (contratista, mandante) = if soy_m {
            (peer_addr, propia)
        } else {
            (propia, peer_addr)
        };
        let Ok(c_addr) = xmr_joint::coop::parse_address(&contratista, Net::Stagenet.oxide()) else {
            self.fallar(obra, partida, true, "sin-direccion".into(), nodo, yo);
            return;
        };
        let Ok(m_addr) = xmr_joint::coop::parse_address(&mandante, Net::Stagenet.oxide()) else {
            self.fallar(obra, partida, true, "sin-direccion".into(), nodo, yo);
            return;
        };
        let Ok(rate) = fund::fee_rate_from_parts(fee.0, fee.1) else {
            self.fallar(obra, partida, true, "fee del nodo inválido".into(), nodo, yo);
            return;
        };
        match spend::propose(&mut OsRng, obra, capital, pct, &c_addr, &m_addr, decoys, rate) {
            Ok((prop, _)) => {
                if let Some(g) = self.gastos.get_mut(&key) {
                    g.propuesta = Some(prop);
                    g.error = None;
                }
            }
            Err(e) => self.fallar(obra, partida, true, e.to_string(), nodo, yo),
        }
    }

    fn hechos(&self, yo: &Persona, obras: &[Obra]) -> Vec<Hecho> {
        let mut out = Vec::new();
        for ((obra, i), f) in &self.fondeos {
            if !f.visto {
                continue;
            }
            let Some(txid) = &f.txid else {
                continue;
            };
            let Some(o) = obras.iter().find(|o| o.id == *obra) else {
                continue;
            };
            let Some(p) = o.partidas.get(*i) else {
                continue;
            };
            let confirmo = p.estado == PartidaEstado::Encerrando
                && p.encerrado_por.as_ref().is_some_and(|q| q.id != yo.id);
            if confirmo {
                out.push(Hecho {
                    obra: obra.clone(),
                    partida: *i,
                    fondeo: Some(txid.clone()),
                    pago: None,
                });
            }
        }
        for ((obra, i), g) in &self.gastos {
            if !g.visto {
                continue;
            }
            let Some(txid) = &g.txid else {
                continue;
            };
            let Some(o) = obras.iter().find(|o| o.id == *obra) else {
                continue;
            };
            let Some(p) = o.partidas.get(*i) else {
                continue;
            };
            let rol = rol_en(o, &yo.id);
            let toca = p.estado == PartidaEstado::EnTrato && p.turno == rol.map(party_a_rol);
            if toca {
                out.push(Hecho {
                    obra: obra.clone(),
                    partida: *i,
                    fondeo: None,
                    pago: Some(txid.clone()),
                });
            }
        }
        out
    }

    fn nota(&mut self, obra: &str, partida: Option<usize>, texto: Texto) {
        self.vista.lineas.retain(|l| !(l.obra == obra && l.partida == partida));
        self.vista.lineas.push(Linea {
            obra: obra.to_string(),
            partida,
            texto,
        });
    }

    fn fallar(
        &mut self,
        obra: &str,
        partida: usize,
        pago: bool,
        msg: String,
        nodo: &Nodo,
        yo: &Persona,
    ) {
        let key = (obra.to_string(), partida);
        let (peer, paso) = if pago {
            let peer = self.gastos.get(&key).map(|g| g.peer.clone()).unwrap_or_default();
            if let Some(g) = self.gastos.get_mut(&key) {
                g.error = Some(msg.clone());
                g.ultimo = Some(Instant::now());
                g.avisar = false;
            }
            (peer, "spend-error")
        } else {
            let peer = self.fondeos.get(&key).map(|f| f.peer.clone()).unwrap_or_default();
            if let Some(f) = self.fondeos.get_mut(&key) {
                f.error = Some(msg.clone());
                f.ultimo = Some(Instant::now());
                f.espera_moneda = false;
            }
            (peer, "fund-error")
        };
        if peer.is_empty() {
            return;
        }
        let body = format!("p{partida}:{msg}");
        let _ = nodo.enviar_caja(obra, &peer, &yo.id, paso, body.as_bytes());
    }

    fn clasificar_fondeo(&self, obra: &str, minimo: u64) -> (Option<AvisoFondeo>, bool) {
        if self.wallet.is_none() {
            return (Some(AvisoFondeo::SinSemilla), false);
        }
        if !self.cuentas.contains_key(obra) {
            return (Some(AvisoFondeo::SinCaja), false);
        }
        match self.estado_de(minimo) {
            EstadoMonedas::Libre => (None, false),
            EstadoMonedas::Trabadas => (Some(AvisoFondeo::Trabadas), false),
            EstadoMonedas::SinSaldo => (Some(AvisoFondeo::SinSaldo), false),
            EstadoMonedas::Buscando => (None, true),
        }
    }

    fn estado_de(&self, minimo: u64) -> EstadoMonedas {
        let entradas: Vec<(u64, usize)> = self
            .libro
            .entradas
            .iter()
            .map(|e| (e.monto, e.altura))
            .collect();
        estado_monedas(
            &entradas,
            self.tip,
            self.libro.listo,
            self.libro.desde,
            self.libro.hasta,
            self.retro,
            minimo,
        )
    }

    /// Frena el envío personal cuando el libro ya está al día y no alcanza.
    fn corte_envio(&self, monto: u64) -> Option<&'static str> {
        let Some(tip) = self.tip else {
            return None;
        };
        let al_dia = self.libro.listo && self.libro.hasta >= tip && self.retro == 0;
        if !al_dia {
            return None;
        }
        let libres: Vec<u64> = self
            .libro
            .entradas
            .iter()
            .filter(|e| tip >= e.altura.saturating_add(10))
            .map(|e| e.monto)
            .collect();
        let todos: Vec<u64> = self.libro.entradas.iter().map(|e| e.monto).collect();
        corte_envio(&libres, &todos, monto, true)
    }

    fn habilitar_pedidos(&mut self) {
        let claves: Vec<_> = self.fondeos.keys().cloned().collect();
        for key in claves {
            let Some(f) = self.fondeos.get(&key) else {
                continue;
            };
            if f.solo_aviso || f.abortar || f.txid.is_some() || f.aviso.is_some() {
                continue;
            }
            if !f.espera_moneda {
                continue;
            }
            let minimo = if f.soy_mandante {
                f.capital.saturating_add(FEE_CUSHION)
            } else {
                f.capital
            };
            if self.salida_libre(minimo).is_none() {
                continue;
            }
            let viene = f.viene_del_par;
            if let Some(f) = self.fondeos.get_mut(&key) {
                f.espera_moneda = false;
                f.avisar = !viene;
            }
        }
    }

    fn emitir_bloqueos(&mut self, nodo: &Nodo, yo: &Persona) {
        let avisos: Vec<_> = self
            .fondeos
            .iter()
            .filter(|(_, f)| f.aviso.is_some() && !f.aviso_enviado && !f.peer.is_empty())
            .map(|((o, i), f)| (o.clone(), *i, f.peer.clone(), f.aviso.unwrap()))
            .collect();
        for (obra, i, peer, aviso) in avisos {
            let Some(code) = codigo_aviso(aviso) else {
                if let Some(f) = self.fondeos.get_mut(&(obra, i)) {
                    f.aviso_enviado = true;
                }
                continue;
            };
            let body = format!("p{i}:{code}");
            if nodo.enviar_caja(&obra, &peer, &yo.id, "fund-error", body.as_bytes()) {
                if let Some(f) = self.fondeos.get_mut(&(obra, i)) {
                    f.aviso_enviado = true;
                }
            }
        }
        let busquedas: Vec<_> = self
            .fondeos
            .iter()
            .filter(|(_, f)| f.espera_moneda && !f.dije_busqueda && !f.peer.is_empty() && !f.solo_aviso)
            .map(|((o, i), f)| (o.clone(), *i, f.peer.clone()))
            .collect();
        for (obra, i, peer) in busquedas {
            let body = format!("p{i}:buscando");
            if nodo.enviar_caja(&obra, &peer, &yo.id, "fund-error", body.as_bytes()) {
                if let Some(f) = self.fondeos.get_mut(&(obra, i)) {
                    f.dije_busqueda = true;
                }
            }
        }
        let gastos: Vec<_> = self
            .gastos
            .iter()
            .filter(|(_, g)| {
                g.error.as_deref().is_some_and(codigo_gasto_local) && !g.peer.is_empty() && g.avisar
            })
            .map(|((o, i), g)| (o.clone(), *i, g.peer.clone(), g.error.clone().unwrap_or_default()))
            .collect();
        for (obra, i, peer, code) in gastos {
            let body = format!("p{i}:{code}");
            if nodo.enviar_caja(&obra, &peer, &yo.id, "spend-error", body.as_bytes()) {
                if let Some(g) = self.gastos.get_mut(&(obra, i)) {
                    g.avisar = false;
                }
            }
        }
    }

    fn poner_aviso_par(&mut self, obra: &str, partida: usize, de: &str, aviso: AvisoFondeo) {
        let key = (obra.to_string(), partida);
        if let Some(f) = self.fondeos.get_mut(&key) {
            f.aviso = Some(aviso);
            f.aviso_enviado = true;
            f.espera_moneda = false;
            f.par_buscando = false;
            f.error = None;
            return;
        }
        self.fondeos.insert(key, fondeo_vacio(de, None, Some(aviso)));
    }

    fn marcar_busqueda_par(&mut self, obra: &str, partida: usize, de: &str) {
        let key = (obra.to_string(), partida);
        if let Some(f) = self.fondeos.get_mut(&key) {
            if f.propuesta.is_some() || f.sobre.is_some() || f.txid.is_some() {
                return;
            }
            f.par_buscando = true;
            return;
        }
        let mut f = fondeo_vacio(de, None, None);
        f.par_buscando = true;
        f.solo_aviso = true;
        self.fondeos.insert(key, f);
    }

    fn salida_libre(&self, minimo: u64) -> Option<Vec<u8>> {
        let tip = self.tip?;
        let pares: Vec<(u64, usize)> = self
            .libro
            .entradas
            .iter()
            .map(|e| (e.monto, e.altura))
            .collect();
        let i = indice_salida_libre(&pares, tip, minimo)?;
        Some(self.libro.entradas[i].raw.clone())
    }

    /// Fondeos que todavía necesitan una salida de la billetera.
    fn fondeo_sin_moneda(&self) -> Vec<(String, usize, u64)> {
        let mut out = Vec::new();
        for ((obra, i), f) in &self.fondeos {
            if f.solo_aviso || f.txid.is_some() || f.error.is_some() || f.aviso.is_some() {
                continue;
            }
            let falta = if f.espera_moneda {
                true
            } else if f.soy_mandante {
                f.propuesta.is_none()
            } else {
                f.sobre.is_some() && f.esqueleto.is_none()
            };
            if !falta {
                continue;
            }
            let minimo = if f.soy_mandante {
                f.capital.saturating_add(FEE_CUSHION)
            } else {
                f.capital
            };
            if self.salida_libre(minimo).is_some() {
                continue;
            }
            out.push((obra.clone(), *i, minimo));
        }
        out
    }

    fn soltar_avisos_con_moneda(&mut self) {
        let claves: Vec<_> = self.fondeos.keys().cloned().collect();
        for key in claves {
            let Some(f) = self.fondeos.get(&key) else {
                continue;
            };
            if f.solo_aviso || f.txid.is_some() {
                continue;
            }
            let minimo = if f.soy_mandante {
                f.capital.saturating_add(FEE_CUSHION)
            } else {
                f.capital
            };
            let se_puede = self.salida_libre(minimo).is_some();
            let caja_ok = self.cuentas.contains_key(&key.0) && self.wallet.is_some();
            let suelta = match f.aviso {
                Some(AvisoFondeo::SinSemilla) => self.wallet.is_some() && se_puede,
                Some(AvisoFondeo::SinCaja) => caja_ok && se_puede,
                Some(AvisoFondeo::Trabadas) | Some(AvisoFondeo::SinSaldo) => se_puede,
                _ => false,
            };
            if !suelta {
                continue;
            }
            let viene = f.viene_del_par;
            if let Some(f) = self.fondeos.get_mut(&key) {
                f.aviso = None;
                f.aviso_enviado = true;
                f.espera_moneda = false;
                if f.propuesta.is_none() && f.sobre.is_none() && !f.abortar {
                    f.avisar = !viene;
                }
            }
        }
    }

    fn empujar_historia(&mut self) {
        let Some(tip) = self.tip else {
            return;
        };
        if self.retro > 0 || !self.libro.listo || self.libro.hasta < tip {
            return;
        }
        let quiere = self.fondeo_sin_moneda().into_iter().any(|(_, _, minimo)| {
            matches!(self.estado_de(minimo), EstadoMonedas::Buscando)
        });
        if quiere && self.libro.desde > tip.saturating_sub(MAX_HISTORIA) {
            self.retro = 200;
        }
    }

    fn cerrar_busqueda_vacia(&mut self) {
        let pendientes = self.fondeo_sin_moneda();
        for (obra, i, minimo) in pendientes {
            let aviso = match self.estado_de(minimo) {
                EstadoMonedas::Trabadas => Some(AvisoFondeo::Trabadas),
                EstadoMonedas::SinSaldo => Some(AvisoFondeo::SinSaldo),
                EstadoMonedas::Libre | EstadoMonedas::Buscando => None,
            };
            let Some(aviso) = aviso else {
                continue;
            };
            if let Some(f) = self.fondeos.get_mut(&(obra, i)) {
                if f.aviso == Some(aviso) {
                    continue;
                }
                f.aviso = Some(aviso);
                f.aviso_enviado = false;
                f.espera_moneda = false;
            }
        }
    }

    fn texto_fondeo(&self, f: &Fondeo) -> Texto {
        if f.visto {
            if let Some(tx) = &f.txid {
                return Texto::EsperandoFondeo(tx.clone());
            }
        }
        if let Some(e) = &f.error {
            return texto_de_codigo(e);
        }
        if let Some(aviso) = f.aviso {
            return texto_aviso(aviso);
        }
        if let Some(tx) = &f.txid {
            return Texto::EsperandoFondeo(tx.clone());
        }
        if f.solo_aviso && f.par_buscando {
            return Texto::BuscandoOtro;
        }
        if f.soy_mandante && f.propuesta.is_none() {
            return self.texto_monedas(f.capital.saturating_add(FEE_CUSHION));
        }
        if !f.soy_mandante && f.sobre.is_none() {
            if f.espera_moneda {
                return self.texto_monedas(f.capital);
            }
            if f.par_buscando {
                return Texto::BuscandoOtro;
            }
            return Texto::EsperandoPropuesta;
        }
        if f.soy_mandante && f.esqueleto.is_none() {
            return Texto::EsperandoContratista;
        }
        if !f.soy_mandante && f.esqueleto.is_none() {
            return self.texto_monedas(f.capital);
        }
        Texto::Fondeando
    }

    fn texto_monedas(&self, minimo: u64) -> Texto {
        match self.estado_de(minimo) {
            EstadoMonedas::Libre => Texto::Fondeando,
            EstadoMonedas::Trabadas => Texto::Trabadas,
            EstadoMonedas::Buscando => Texto::BuscandoMonedas,
            EstadoMonedas::SinSaldo => Texto::SinSaldo,
        }
    }

    fn texto_gasto(&self, g: &Gasto) -> Texto {
        if g.visto {
            if let Some(tx) = &g.txid {
                return Texto::EsperandoPago(tx.clone());
            }
        }
        if let Some(e) = &g.error {
            return texto_de_codigo(e);
        }
        if let Some(tx) = &g.txid {
            return Texto::EsperandoPago(tx.clone());
        }
        Texto::Gastando
    }

    fn armar_vista(&mut self, obras: &[Obra]) {
        let mut v = CajaVista::vacia();
        v.tip = self.tip;
        v.tiene_semilla = self.wallet.is_some();
        v.personal = self.wallet.as_ref().map(|w| w.address().to_string());
        for (id, cuenta) in &self.cuentas {
            v.cajas.push((id.clone(), cuenta.address().to_string()));
        }
        for obra in obras {
            if self.dkg.contains_key(&obra.id) && !self.cuentas.contains_key(&obra.id) {
                v.lineas.push(Linea {
                    obra: obra.id.clone(),
                    partida: None,
                    texto: Texto::Armando,
                });
            }
        }
        for ((obra, i), f) in &self.fondeos {
            let texto = self.texto_fondeo(f);
            v.lineas.push(Linea {
                obra: obra.clone(),
                partida: Some(*i),
                texto,
            });
        }
        for ((obra, i), g) in &self.gastos {
            let texto = self.texto_gasto(g);
            v.lineas.push(Linea {
                obra: obra.clone(),
                partida: Some(*i),
                texto,
            });
        }
        if self.wallet.is_none() {
            for obra in obras {
                if rol_en(obra, "").is_some() {
                    continue;
                }
            }
        }
        v.billetera = self.vista_billetera();
        self.vista = v;
    }

    fn vista_billetera(&self) -> BilleteraVista {
        let tip = self.tip.unwrap_or(0);
        let mut total = 0u64;
        let mut libre = 0u64;
        let mut movs = Vec::new();
        for e in &self.libro.entradas {
            total = total.saturating_add(e.monto);
            let suelta = self.tip.is_some() && tip >= e.altura.saturating_add(10);
            if suelta {
                libre = libre.saturating_add(e.monto);
            }
            movs.push(Mov {
                monto: e.monto,
                altura: e.altura,
                libre: suelta,
            });
        }
        movs.sort_by(|a, b| b.altura.cmp(&a.altura));
        movs.truncate(6);
        let mut aviso = None;
        if let Some(a) = &self.envio_aviso {
            aviso = Some(a.clone());
        } else if let Some(a) = &self.scan_aviso {
            aviso = Some(a.clone());
        }
        BilleteraVista {
            total,
            libre,
            trabado: total.saturating_sub(libre),
            desde: self.libro.listo.then_some(self.libro.desde),
            hasta: self.libro.listo.then_some(self.libro.hasta),
            retro: self.retro,
            buscando: self.buscando,
            enviando: self.enviando,
            aviso,
            ultimo: self.ultimo_envio.clone(),
            ultimo_fee: self.ultimo_fee,
            ultimo_cambio: self.ultimo_cambio,
            movs,
        }
    }

    fn cargar_libro(&mut self) {
        let Some(addr) = self.wallet.as_ref().map(|w| w.address().to_string()) else {
            return;
        };
        let Ok(text) = backup::read_secret_file(&libro_path()) else {
            self.libro = Libro::nueva(&addr);
            return;
        };
        let Ok(disco) = serde_json::from_str::<LibroDisco>(&text) else {
            self.libro = Libro::nueva(&addr);
            return;
        };
        if disco.direccion != addr {
            self.libro = Libro::nueva(&addr);
            return;
        }
        self.retro = disco.retro as usize;
        self.libro = Libro {
            direccion: disco.direccion,
            desde: disco.desde as usize,
            hasta: disco.hasta as usize,
            listo: disco.listo,
            entradas: disco
                .entradas
                .into_iter()
                .filter_map(|e| {
                    let raw = hex::decode(e.raw).ok()?;
                    Some(Entrada {
                        altura: e.altura as usize,
                        monto: e.monto,
                        tx: e.tx,
                        indice: e.indice,
                        raw,
                    })
                })
                .collect(),
        };
    }

    fn guardar_libro(&self) {
        if self.libro.direccion.is_empty() {
            return;
        }
        let disco = LibroDisco {
            direccion: self.libro.direccion.clone(),
            desde: self.libro.desde as u64,
            hasta: self.libro.hasta as u64,
            listo: self.libro.listo,
            retro: self.retro as u64,
            entradas: self
                .libro
                .entradas
                .iter()
                .map(|e| EntradaDisco {
                    altura: e.altura as u64,
                    monto: e.monto,
                    tx: e.tx.clone(),
                    indice: e.indice,
                    raw: hex::encode(&e.raw),
                })
                .collect(),
        };
        let Ok(text) = serde_json::to_string(&disco) else {
            return;
        };
        let _ = escribir_0600(&libro_path(), &text);
    }

    fn cargar_shares(&mut self) {
        let Ok(rd) = std::fs::read_dir(xmr_dir()) else {
            return;
        };
        for ent in rd.flatten() {
            let path = ent.path();
            if path.extension().and_then(|e| e.to_str()) != Some("share") {
                continue;
            }
            let Ok(text) = backup::read_secret_file(&path) else {
                continue;
            };
            let Ok(share) = xmr_joint::backup::ShareBackup::parse(&text) else {
                continue;
            };
            if let Ok(account) = JointAccount::from_backup(&share) {
                self.cuentas.insert(account.obra_id().to_string(), account);
            }
        }
    }

    fn cargar_pares(&mut self) {
        let Ok(rd) = std::fs::read_dir(xmr_dir()) else {
            return;
        };
        for ent in rd.flatten() {
            let name = ent.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Some(obra) = name.strip_prefix("par-").and_then(|s| s.strip_suffix(".txt")) else {
                continue;
            };
            if let Ok(text) = std::fs::read_to_string(ent.path()) {
                let addr = text.trim();
                if !addr.is_empty() {
                    self.pares.insert(obra.to_string(), addr.to_string());
                }
            }
        }
    }

    fn cargar_esperas(&mut self) {
        let Ok(rd) = std::fs::read_dir(xmr_dir()) else {
            return;
        };
        for ent in rd.flatten() {
            let name = ent.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !name.starts_with("espera-") || !name.ends_with(".json") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(ent.path()) else {
                continue;
            };
            let Ok(e) = serde_json::from_str::<EsperaDisco>(&text) else {
                continue;
            };
            if e.pago {
                self.gastos.insert(
                    (e.obra.clone(), e.partida),
                    Gasto {
                        peer: String::new(),
                        capital: 0,
                        pct: e.pct.unwrap_or(0),
                        soy_mandante: false,
                        propuesta: None,
                        session: None,
                        signed: None,
                        pre_otro: None,
                        share_otro: None,
                        envie_open: true,
                        envie_pre: true,
                        envie_share: true,
                        blob: None,
                        txid: Some(e.txid),
                        visto: false,
                        error: None,
                        ultimo: None,
                        avisar: false,
                    },
                );
            } else {
                self.fondeos.insert(
                    (e.obra.clone(), e.partida),
                    Fondeo {
                        peer: String::new(),
                        capital: 0,
                        soy_mandante: false,
                        propuesta: None,
                        outputs: None,
                        esqueleto: None,
                        sobre: None,
                        blob: None,
                        txid: Some(e.txid),
                        visto: false,
                        error: None,
                        aviso: None,
                        aviso_enviado: true,
                        ultimo: None,
                        avisar: false,
                        avise_pedir: true,
                        espera_moneda: false,
                        dije_busqueda: true,
                        viene_del_par: false,
                        solo_aviso: false,
                        par_buscando: false,
                        abortar: false,
                    },
                );
            }
        }
    }

    fn guardar_par(&mut self, obra: &str, addr: &str) {
        if obra.is_empty() || addr.is_empty() || !id_sano(obra) {
            return;
        }
        self.pares.insert(obra.to_string(), addr.to_string());
        let _ = std::fs::create_dir_all(xmr_dir());
        let _ = std::fs::write(xmr_dir().join(format!("par-{obra}.txt")), addr);
    }

    fn guardar_espera(&self, obra: &str, partida: usize, pago: bool, txid: &str) {
        if !id_sano(obra) {
            return;
        }
        let disco = EsperaDisco {
            obra: obra.to_string(),
            partida,
            pago,
            txid: txid.to_string(),
            pct: None,
        };
        if let Ok(raw) = serde_json::to_string(&disco) {
            let _ = std::fs::create_dir_all(xmr_dir());
            let _ = std::fs::write(espera_path(obra, partida, pago), raw);
        }
    }
}

#[derive(Serialize, Deserialize)]
struct EsperaDisco {
    obra: String,
    partida: usize,
    pago: bool,
    txid: String,
    pct: Option<u32>,
}

fn anuncio_de(cuenta: &JointAccount) -> Result<ViewAnnounce, String> {
    let backup = cuenta.backup().map_err(|e| e.to_string())?;
    Ok(ViewAnnounce {
        view_private: *backup.view_private,
        address: cuenta.address().to_string(),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EstadoMonedas {
    Libre,
    Trabadas,
    Buscando,
    SinSaldo,
}

/// Qué se puede hacer con el libro que ya está en disco, sin armar anillos.
fn estado_monedas(
    entradas: &[(u64, usize)],
    tip: Option<usize>,
    listo: bool,
    desde: usize,
    hasta: usize,
    retro: usize,
    minimo: u64,
) -> EstadoMonedas {
    let Some(tip) = tip else {
        return EstadoMonedas::Buscando;
    };
    if indice_salida_libre(entradas, tip, minimo).is_some() {
        return EstadoMonedas::Libre;
    }
    if !listo || hasta < tip || retro > 0 {
        return EstadoMonedas::Buscando;
    }
    if entradas
        .iter()
        .any(|&(monto, altura)| monto >= minimo && tip < altura.saturating_add(10))
    {
        return EstadoMonedas::Trabadas;
    }
    if desde > tip.saturating_sub(MAX_HISTORIA) {
        return EstadoMonedas::Buscando;
    }
    EstadoMonedas::SinSaldo
}

/// `None` si se puede armar el envío. Si no, un código para mostrar y no firmar.
fn corte_envio(libres: &[u64], todos: &[u64], monto: u64, al_dia: bool) -> Option<&'static str> {
    if !al_dia {
        return None;
    }
    let need = monto.saturating_add(FEE_CUSHION);
    if elegir_montos(libres, need).is_ok() {
        return None;
    }
    let total = todos.iter().fold(0u64, |acc, n| acc.saturating_add(*n));
    if total >= need {
        Some("codigo:trabadas")
    } else {
        Some("codigo:sin-saldo")
    }
}

fn partir_aviso(txt: &str) -> (Option<usize>, &str) {
    let Some(rest) = txt.strip_prefix('p') else {
        return (None, txt);
    };
    let Some((idx, code)) = rest.split_once(':') else {
        return (None, txt);
    };
    match idx.parse::<usize>() {
        Ok(i) if !code.is_empty() => (Some(i), code),
        _ => (None, txt),
    }
}

fn codigo_aviso(aviso: AvisoFondeo) -> Option<&'static str> {
    match aviso {
        AvisoFondeo::SinSaldo => Some("sin-saldo"),
        AvisoFondeo::Trabadas => Some("trabadas"),
        AvisoFondeo::SinSemilla => Some("sin-semilla"),
        AvisoFondeo::SinCaja => Some("sin-caja"),
        AvisoFondeo::SinSaldoOtro
        | AvisoFondeo::TrabadasOtro
        | AvisoFondeo::SinSemillaOtro
        | AvisoFondeo::SinCajaOtro => None,
    }
}

fn aviso_es_del_otro(aviso: AvisoFondeo) -> bool {
    codigo_aviso(aviso).is_none()
}

fn aviso_del_otro(code: &str) -> Option<AvisoFondeo> {
    match code {
        "sin-saldo" | "sin-saldo-caja" => Some(AvisoFondeo::SinSaldoOtro),
        "trabadas" | "trabadas-caja" => Some(AvisoFondeo::TrabadasOtro),
        "sin-semilla" => Some(AvisoFondeo::SinSemillaOtro),
        "sin-caja" => Some(AvisoFondeo::SinCajaOtro),
        _ => None,
    }
}

fn codigo_del_otro(code: &str) -> String {
    match code {
        "sin-saldo" | "sin-saldo-caja" | "codigo:sin-saldo" => "sin-saldo-otro".into(),
        "trabadas" | "trabadas-caja" | "codigo:trabadas" => "trabadas-otro".into(),
        "sin-semilla" | "codigo:sin-semilla" => "sin-semilla-otro".into(),
        "sin-caja" => "sin-caja-otro".into(),
        "sin-direccion" => "sin-direccion-otro".into(),
        other => other.to_string(),
    }
}

fn codigo_gasto_local(code: &str) -> bool {
    matches!(
        code,
        "sin-semilla" | "sin-caja" | "sin-direccion" | "sin-saldo-caja" | "trabadas-caja"
    )
}

fn texto_aviso(aviso: AvisoFondeo) -> Texto {
    match aviso {
        AvisoFondeo::SinSaldo => Texto::SinSaldo,
        AvisoFondeo::Trabadas => Texto::Trabadas,
        AvisoFondeo::SinSemilla => Texto::SinSemilla,
        AvisoFondeo::SinCaja => Texto::SinCaja,
        AvisoFondeo::SinSaldoOtro => Texto::SinSaldoOtro,
        AvisoFondeo::TrabadasOtro => Texto::TrabadasOtro,
        AvisoFondeo::SinSemillaOtro => Texto::SinSemillaOtro,
        AvisoFondeo::SinCajaOtro => Texto::SinCajaOtro,
    }
}

fn soltar_aviso_ajeno(f: &mut Fondeo) {
    f.par_buscando = false;
    if f.aviso.is_some_and(aviso_es_del_otro) {
        f.aviso = None;
    }
}

fn texto_de_codigo(code: &str) -> Texto {
    match code {
        "sin-semilla" | "codigo:sin-semilla" => Texto::SinSemilla,
        "sin-caja" => Texto::SinCaja,
        "sin-direccion" => Texto::SinDireccion,
        "sin-saldo" | "codigo:sin-saldo" => Texto::SinSaldo,
        "sin-saldo-caja" => Texto::SinSaldoCaja,
        "trabadas" | "codigo:trabadas" => Texto::Trabadas,
        "trabadas-caja" => Texto::TrabadasCaja,
        "sin-saldo-otro" => Texto::SinSaldoOtro,
        "trabadas-otro" => Texto::TrabadasOtro,
        "sin-semilla-otro" => Texto::SinSemillaOtro,
        "sin-caja-otro" => Texto::SinCajaOtro,
        "sin-direccion-otro" => Texto::SinDireccionOtro,
        "buscando" => Texto::BuscandoOtro,
        other => Texto::Falla(other.to_string()),
    }
}

fn fondeo_vacio(peer: &str, error: Option<String>, aviso: Option<AvisoFondeo>) -> Fondeo {
    Fondeo {
        peer: peer.to_string(),
        capital: 0,
        soy_mandante: false,
        propuesta: None,
        outputs: None,
        esqueleto: None,
        sobre: None,
        blob: None,
        txid: None,
        visto: false,
        error,
        aviso,
        aviso_enviado: true,
        ultimo: None,
        avisar: false,
        avise_pedir: false,
        espera_moneda: false,
        dije_busqueda: true,
        viene_del_par: true,
        solo_aviso: true,
        par_buscando: false,
        abortar: false,
    }
}

/// Índice de la salida libre más chica que cubre `minimo`. `entradas` es (monto, altura).
fn indice_salida_libre(entradas: &[(u64, usize)], tip: usize, minimo: u64) -> Option<usize> {
    let mut mejor: Option<(usize, u64)> = None;
    for (i, &(monto, altura)) in entradas.iter().enumerate() {
        if monto < minimo || tip < altura.saturating_add(10) {
            continue;
        }
        if mejor.is_none_or(|(_, otra)| monto < otra) {
            mejor = Some((i, monto));
        }
    }
    mejor.map(|(i, _)| i)
}

fn frio(t: Option<Instant>) -> bool {
    match t {
        None => true,
        Some(t) => t.elapsed() >= PAUSA,
    }
}

fn f_peer(k: &(String, usize), de: &str) -> bool {
    let _ = (k, de);
    true
}

fn parse_idx(b: &[u8]) -> Result<usize, ()> {
    let s = std::str::from_utf8(b).map_err(|_| ())?.trim();
    s.parse().map_err(|_| ())
}

fn hex_bytes(s: &str) -> Vec<u8> {
    hex::decode(s).unwrap_or_default()
}

fn rol_en(obra: &Obra, yo: &str) -> Option<Party> {
    if obra.mandante.id == yo {
        Some(Party::Mandante)
    } else if obra.contratista.id == yo {
        Some(Party::Contratista)
    } else {
        None
    }
}

fn otro_id<'a>(obra: &'a Obra, yo: &str) -> &'a str {
    if obra.mandante.id == yo {
        &obra.contratista.id
    } else {
        &obra.mandante.id
    }
}

fn party_a_rol(p: Party) -> Rol {
    match p {
        Party::Mandante => Rol::Mandante,
        Party::Contratista => Rol::Contratista,
    }
}

fn trato_firme(e: EstadoObra) -> bool {
    matches!(
        e,
        EstadoObra::Acordada | EstadoObra::EnMarcha | EstadoObra::Cerrada | EstadoObra::Abandonada
    )
}

fn id_sano(id: &str) -> bool {
    !id.is_empty()
        && id.len() < 80
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn xmr_dir() -> PathBuf {
    crate::persist::dir().join("xmr")
}

struct PedidoEnvio {
    destino: String,
    monto: u64,
}

struct EnvioHecho {
    txid: String,
    fee: u64,
    cambio: u64,
}

struct EnvioJob {
    spend: xmr_joint::LlaveGasto,
    view: xmr_joint::ViewPair,
    crudas: Vec<Vec<u8>>,
    usadas: Vec<(String, u64)>,
    destino: String,
    monto: u64,
}

struct EntradaNueva {
    altura: usize,
    monto: u64,
    tx: String,
    indice: u64,
    raw: Vec<u8>,
}

#[derive(Clone)]
struct Entrada {
    altura: usize,
    monto: u64,
    tx: String,
    indice: u64,
    raw: Vec<u8>,
}

struct Libro {
    direccion: String,
    desde: usize,
    hasta: usize,
    listo: bool,
    entradas: Vec<Entrada>,
}

impl Libro {
    fn vacio() -> Self {
        Self {
            direccion: String::new(),
            desde: 0,
            hasta: 0,
            listo: false,
            entradas: Vec::new(),
        }
    }

    fn nueva(direccion: &str) -> Self {
        Self {
            direccion: direccion.to_string(),
            desde: 0,
            hasta: 0,
            listo: false,
            entradas: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct LibroDisco {
    direccion: String,
    desde: u64,
    hasta: u64,
    listo: bool,
    retro: u64,
    entradas: Vec<EntradaDisco>,
}

#[derive(Serialize, Deserialize)]
struct EntradaDisco {
    altura: u64,
    monto: u64,
    tx: String,
    indice: u64,
    raw: String,
}

fn escribir_0600(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    Ok(())
}

fn semilla_path() -> PathBuf {
    xmr_dir().join("semilla.txt")
}

fn libro_path() -> PathBuf {
    xmr_dir().join("libro.json")
}

fn share_path(obra: &str) -> PathBuf {
    xmr_dir().join(format!("{obra}.share"))
}

fn espera_path(obra: &str, partida: usize, pago: bool) -> PathBuf {
    let clase = if pago { "pago" } else { "fondeo" };
    xmr_dir().join(format!("espera-{obra}-{partida}-{clase}.json"))
}

fn leer_semilla() -> Option<SingleWallet> {
    let text = backup::read_secret_file(&semilla_path()).ok()?;
    let backup = SeedBackup::parse(&text).ok()?;
    if backup.net != Net::Stagenet {
        return None;
    }
    let wallet = SingleWallet::restore(Net::Stagenet, backup.words.as_str()).ok()?;
    if wallet.address() != backup.address {
        return None;
    }
    Some(wallet)
}

async fn juntar_entradas(
    view: xmr_joint::ViewPair,
    minimo: u64,
    cuantos: usize,
    exacto: Option<u64>,
) -> Result<(Vec<OutputWithDecoys>, (u64, u64)), String> {
    let rpc = chain::connect(STAGENET_DAEMON).await.map_err(|e| e.to_string())?;
    let (decoys, fee) = entradas_con(&rpc, view, minimo, cuantos, exacto).await?;
    Ok((decoys, fee))
}

async fn entradas_con(
    rpc: &Daemon,
    view: xmr_joint::ViewPair,
    minimo: u64,
    cuantos: usize,
    exacto: Option<u64>,
) -> Result<(Vec<OutputWithDecoys>, (u64, u64)), String> {
    let tip = chain::tip(rpc).await.map_err(|e| e.to_string())?;
    let from = tip.saturating_sub(LOOKBACK);
    let marcadas = chain::scan_marcado(rpc, view, from, tip)
        .await
        .map_err(|e| e.to_string())?;
    let elegidos = if let Some(amount) = exacto {
        let mut libres = Vec::new();
        let mut trabadas = 0usize;
        for (altura, output) in marcadas {
            if output.commitment().amount != amount {
                continue;
            }
            if tip >= altura.saturating_add(10) {
                libres.push(output);
            } else {
                trabadas += 1;
            }
        }
        if libres.len() != cuantos {
            if libres.len() + trabadas >= cuantos {
                return Err("trabadas-caja".into());
            }
            return Err("sin-saldo-caja".into());
        }
        libres
    } else {
        let libres: Vec<_> = marcadas
            .into_iter()
            .filter_map(|(altura, output)| (tip >= altura.saturating_add(10)).then_some(output))
            .collect();
        let uno = fund::pick_output(libres, minimo).map_err(|_| "sin-saldo-caja".to_string())?;
        vec![uno]
    };
    let rate = chain::fee_rate(rpc).await.map_err(|e| e.to_string())?;
    let fee = fund::fee_parts(&rate);
    let mut decoys = Vec::with_capacity(elegidos.len());
    for output in elegidos {
        decoys.push(chain::with_decoys(rpc, output, tip).await.map_err(|e| e.to_string())?);
    }
    Ok((decoys, fee))
}

async fn ver_txid(view: xmr_joint::ViewPair, txid: &str) -> Result<bool, String> {
    let rpc = chain::connect(STAGENET_DAEMON).await.map_err(|e| e.to_string())?;
    let tip = chain::tip(&rpc).await.map_err(|e| e.to_string())?;
    let from = tip.saturating_sub(LOOKBACK);
    let outs = chain::scan(&rpc, view, from, tip).await.map_err(|e| e.to_string())?;
    Ok(outs.iter().any(|o| hex::encode(o.transaction()) == txid))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dos_mil_son_cero_cero_cuatro_xmr() {
        let pico = a_piconero(2_000).unwrap();
        assert_eq!(pico, 40_000_000_000);
        assert_eq!(fmt_xmr(pico), "0.04");
        assert!(a_piconero(u64::MAX).is_none());
        assert_eq!(maximo_envio(pico).as_deref(), Some("0.039"));
        assert!(maximo_envio(FEE_CUSHION).is_none());
    }

    #[test]
    fn el_fondeo_usa_la_salida_libre_mas_chica() {
        let entradas = [(500, 1), (2_000, 90), (5_000, 50), (9_000, 95)];
        assert_eq!(indice_salida_libre(&entradas, 100, 1_000), Some(1));
        assert_eq!(indice_salida_libre(&entradas, 100, 400), Some(0));
        assert_eq!(indice_salida_libre(&entradas, 100, 8_000), None);
        assert_eq!(indice_salida_libre(&entradas, 110, 8_000), Some(3));
    }

    #[test]
    fn no_arma_si_no_hay_saldo_o_sigue_trabado() {
        let trabada = [(5_000u64, 95usize)];
        assert_eq!(
            estado_monedas(&trabada, Some(100), true, 60, 100, 0, 1_000),
            EstadoMonedas::Trabadas
        );
        assert_eq!(
            estado_monedas(&trabada, Some(105), true, 60, 105, 0, 1_000),
            EstadoMonedas::Libre
        );
        assert_eq!(
            estado_monedas(&[], Some(100), true, 60, 100, 0, 1_000),
            EstadoMonedas::Buscando
        );
        assert_eq!(
            estado_monedas(&[], Some(100), true, 0, 100, 0, 1_000),
            EstadoMonedas::SinSaldo
        );
        assert_eq!(
            estado_monedas(&[], Some(100), true, 0, 100, 50, 1_000),
            EstadoMonedas::Buscando
        );
        assert_eq!(
            estado_monedas(&trabada, None, false, 0, 0, 0, 1_000),
            EstadoMonedas::Buscando
        );
    }

    #[test]
    fn el_envio_no_arma_si_el_libro_no_alcanza() {
        let cubre = FEE_CUSHION + 2_000;
        assert_eq!(corte_envio(&[500], &[500], 1_000, true), Some("codigo:sin-saldo"));
        assert_eq!(
            corte_envio(&[], &[cubre], 1_000, true),
            Some("codigo:trabadas")
        );
        assert_eq!(corte_envio(&[cubre], &[cubre], 1_000, true), None);
        assert_eq!(corte_envio(&[], &[], 1_000, false), None);
        assert_eq!(partir_aviso("p3:sin-saldo"), (Some(3), "sin-saldo"));
        assert_eq!(partir_aviso("bloque 3: timeout"), (None, "bloque 3: timeout"));
        assert!(es_freno(&Texto::SinSaldo));
        assert!(!es_freno(&Texto::BuscandoMonedas));
        assert!(aviso_humano("codigo:sin-saldo", false).contains("not enough"));
    }
}
