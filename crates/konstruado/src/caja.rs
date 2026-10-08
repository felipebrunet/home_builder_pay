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

use xmr_joint::backup::{self, SeedBackup, ShareBackup};
use xmr_joint::chain;
use xmr_joint::coop::{self, Proposal, Skeleton};
use xmr_joint::dkg::{DkgParty, JointAccount, Party, ViewAnnounce};
use xmr_joint::fund::{self, view_del_mandante};
use xmr_joint::personal::{self, elegir_montos};
use xmr_joint::spend::{self, SpendProposal, SpendSession, SpendSigned};
use xmr_joint::wallet::SingleWallet;
use xmr_joint::{daemon_url, url_es_local, OutputWithDecoys, Net, FEE_CUSHION, PICONERO};

use konstruado_core::{EstadoObra, Obra, PartidaEstado, Persona, Rol};
use konstruado_net::{CajaMsg, Nodo};

/// 1 unidad del trato, en piconero. 2000 unidades = 0,04 XMR.
pub const PICONERO_POR_UNIDAD: u64 = 20_000_000;

const LOOKBACK: usize = 40;
/// Hasta dónde camina solo el fondeo si la billetera todavía no vio la salida. ~4 semanas en stagenet.
const MAX_HISTORIA: usize = 20_000;
/// Lo que suma un click de "mirar más atrás" en la caja. No se escanea de una sola vez.
const PASO_ATRAS_CAJA: usize = 200;
/// Bloques de la caja por turno del nodo. El primero sigue siendo [`LOOKBACK`].
const PASO_SCAN: usize = 8;
const PAUSA: Duration = Duration::from_secs(20);

pub fn a_piconero(unidades: u64) -> Option<u64> {
    unidades.checked_mul(PICONERO_POR_UNIDAD)
}

pub fn fmt_xmr(pico: u64) -> String {
    let whole = pico / PICONERO;
    let mut frac = format!("{:012}", pico % PICONERO);
    // Mostrar al menos 4 decimales (piconero/1e12); recortar ceros solo después de eso.
    while frac.len() > 4 && frac.ends_with('0') {
        frac.pop();
    }
    format!("{whole}.{frac}")
}

pub struct AporteFondeo {
    pub por_lado: u64,
    pub total: u64,
}

/// Lo que entra a la caja si los dos fondean: dos salidas iguales, una por lado.
pub fn aporte_fondeo(capital_unidades: u64) -> Option<AporteFondeo> {
    let por_lado = a_piconero(capital_unidades)?;
    let total = por_lado.checked_mul(2)?;
    Some(AporteFondeo { por_lado, total })
}

pub struct SaldoPartida {
    pub estado: String,
    pub detalle: String,
    pub candado: Option<String>,
}

/// Texto de la ficha: estado del fondeo y cuánto quedó en la caja.
pub fn saldo_partida(
    es: bool,
    estado: PartidaEstado,
    capital_unidades: u64,
    tiene_fondeo: bool,
    mandante: &str,
    contratista: &str,
) -> Option<SaldoPartida> {
    let aporte = aporte_fondeo(capital_unidades)?;
    let lado = fmt_xmr(aporte.por_lado);
    let total = fmt_xmr(aporte.total);
    if estado == PartidaEstado::Pagada {
        return Some(SaldoPartida {
            estado: if es {
                "Pagada. Esta partida ya no tiene saldo en la caja.".into()
            } else {
                "Paid. This stage no longer has a balance in the box.".into()
            },
            detalle: if es {
                format!("Se habían encerrado {total} XMR: {lado} de {mandante} y {lado} de {contratista}.")
            } else {
                format!("They had locked {total} XMR: {lado} from {mandante} and {lado} from {contratista}.")
            },
            candado: None,
        });
    }
    if tiene_fondeo
        && matches!(
            estado,
            PartidaEstado::Encerrando | PartidaEstado::Encerrada | PartidaEstado::EnTrato
        )
    {
        let estado_txt = if estado == PartidaEstado::EnTrato {
            if es {
                "En trato. El saldo sigue en la caja hasta el pago."
            } else {
                "In deal. The balance stays in the box until payment."
            }
        } else {
            if es {
                "Fondeada. El saldo sigue en la caja hasta el pago."
            } else {
                "Funded. The balance stays in the box until payment."
            }
        };
        return Some(SaldoPartida {
            estado: estado_txt.into(),
            detalle: if es {
                format!("Total en la caja: {total} XMR. {mandante} aportó {lado} XMR. {contratista} aportó {lado} XMR.")
            } else {
                format!("Total in the box: {total} XMR. {mandante} put in {lado} XMR. {contratista} put in {lado} XMR.")
            },
            candado: Some(if es {
                "Esas salidas se pueden gastar después de 10 bloques desde el bloque del fondeo.".into()
            } else {
                "Those outputs can be spent 10 blocks after the funding block.".into()
            }),
        });
    }
    if estado == PartidaEstado::Encerrando {
        return Some(SaldoPartida {
            estado: if es {
                "Todavía no hay saldo en la caja; falta completar el fondeo.".into()
            } else {
                "No balance in the box yet; funding is still in progress.".into()
            },
            detalle: if es {
                format!("Si se fondea, cada lado pone {lado} XMR. El total en la caja sería {total} XMR.")
            } else {
                format!("If it funds, each side puts in {lado} XMR. The box total would be {total} XMR.")
            },
            candado: None,
        });
    }
    None
}

/// Una línea corta para la lista de partidas.
pub fn saldo_corto(es: bool, estado: PartidaEstado, capital_unidades: u64, tiene_fondeo: bool) -> Option<String> {
    if estado == PartidaEstado::Pagada {
        return Some(if es {
            "Pagada · la caja de esta partida quedó en cero".into()
        } else {
            "Paid · this stage's box is empty".into()
        });
    }
    if tiene_fondeo
        && matches!(
            estado,
            PartidaEstado::Encerrando | PartidaEstado::Encerrada | PartidaEstado::EnTrato
        )
    {
        let total = fmt_xmr(aporte_fondeo(capital_unidades)?.total);
        return Some(if es {
            format!("{total} XMR en la caja")
        } else {
            format!("{total} XMR in the box")
        });
    }
    if estado == PartidaEstado::Encerrando {
        return Some(if es {
            "Todavía sin fondear".into()
        } else {
            "Not funded yet".into()
        });
    }
    None
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
    /// Qué tan atrás mira cada caja. `retro` es lo que todavía falta caminar.
    pub miradas: Vec<MiradaCaja>,
}

/// Ventana de scan de una caja. No incluye llaves.
#[derive(Clone, Debug)]
pub struct MiradaCaja {
    pub obra: String,
    pub retro: usize,
    pub bloques: usize,
    pub aviso: Option<String>,
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
            daemon: daemon_url(),
            tip: None,
            personal: None,
            tiene_semilla: false,
            cajas: Vec::new(),
            lineas: Vec::new(),
            billetera: BilleteraVista::vacia(),
            miradas: Vec::new(),
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
    /// La caja todavía no cubrió el rango que el usuario pidió.
    BuscandoCaja,
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
                    "La caja no muestra las dos salidas libres de esta partida. No armé el pago. Si el fondeo es más viejo, pedí mirar más bloques.".into()
                } else {
                    "The box does not show this stage's two unlocked outputs. The payment was not built. If the funding is older, scan further back.".into()
                }
            }
            Texto::BuscandoCaja => {
                if es {
                    "Estoy mirando más bloques de la caja. Todavía no armo el pago.".into()
                } else {
                    "Looking further back through the box. The payment is not being built yet.".into()
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


fn es_rechazo_fondeo(msg: &str) -> bool {
    let low = msg.to_ascii_lowercase();
    low.contains("rejected")
        || low.contains("double spend")
        || low.contains("key image")
        || low.contains("already spent")
        || low.contains("decoy")
        || low.contains("failed to get")
        || low.contains("not in the mainchain")
        || low.contains("invalid input")
}

/// Traduce errores crudos del RPC/daemon (timeout, refused, …) a español/inglés.
pub fn humanizar_error_cadena(raw: &str, es: bool) -> String {
    let s = raw.trim();
    let low = s.to_ascii_lowercase();
    let cuerpo = s
        .strip_prefix("cadena: ")
        .or_else(|| s.strip_prefix("Chain: "))
        .unwrap_or(s);
    let low_c = cuerpo.to_ascii_lowercase();

    let (esp, ing) = if low_c.contains("timeout")
        || low_c.contains("elapsed")
        || low_c.contains("timed out")
        || low.contains("timeout reached")
    {
        (
            "El nodo no respondió a tiempo (timeout). Revisá que monerod esté en marcha, el puerto (RPC stagenet suele ser 38081; el público usa 38089) y que el teléfono alcance esa IP por LAN o Tailscale.",
            "The node did not answer in time (timeout). Check that monerod is running, the port (stagenet RPC is often 38081; the public node uses 38089), and that this phone can reach that IP on LAN or Tailscale.",
        )
    } else if low_c.contains("connection refused")
        || low_c.contains("actively refused")
        || low_c.contains("econnrefused")
    {
        (
            "El nodo rechazó la conexión. ¿monerod escucha en esa IP:puerto? En la PC: --rpc-bind-ip 0.0.0.0 --confirm-external-bind (y sin restricted-rpc si querés RPC completo).",
            "The node refused the connection. Is monerod listening on that IP:port? On the PC use --rpc-bind-ip 0.0.0.0 --confirm-external-bind.",
        )
    } else if low_c.contains("connection reset")
        || low_c.contains("reset by peer")
        || low_c.contains("econnreset")
        || low_c.contains("connectionreset")
    {
        (
            "La conexión con el nodo se cortó apenas abrió (connection reset). Si el nodo es de tu red local y Orbot está en modo VPN capturando a Konstruado, Orbot la manda por Tor y Tor no llega a IPs privadas: dejá Konstruado fuera de la VPN de Orbot. Si no, revisá que monerod siga en marcha.",
            "The node connection was reset right after opening. If the node is on your local network and Orbot's VPN mode is capturing Konstruado, Orbot sends it through Tor and Tor cannot reach private IPs: keep Konstruado out of Orbot's VPN. Otherwise check that monerod is still running.",
        )
    } else if low_c.contains("network is unreachable")
        || low_c.contains("no route to host")
        || low_c.contains("host is unreachable")
        || low_c.contains("ehostunreach")
        || low_c.contains("enetunreach")
    {
        (
            "No hay ruta hasta esa IP. Si usás Tailscale, confirmá que PC y teléfono estén en la misma red Tailscale; si es LAN, misma Wi‑Fi y que el firewall no bloquee el puerto.",
            "No route to that IP. On Tailscale, both devices must be on the same tailnet; on LAN, same Wi‑Fi and an open RPC port.",
        )
    } else if low_c.contains("name or service not known")
        || low_c.contains("nodename nor servname")
        || low_c.contains("dns")
        || low_c.contains("failed to lookup")
    {
        (
            "No pude resolver el host de la URL. Revisá el nombre o usá la IP (Tailscale/LAN).",
            "Could not resolve the host in the URL. Check the name or use the Tailscale/LAN IP.",
        )
    } else if low_c.contains("certificate")
        || low_c.contains("tls")
        || low_c.contains("ssl")
        || low_c.contains("handshake")
    {
        (
            "Falló el HTTPS/TLS con ese nodo. Un monerod local de stagenet casi siempre es http://IP:38081 (sin https).",
            "HTTPS/TLS failed with that node. A local stagenet monerod is almost always http://IP:38081 (not https).",
        )
    } else if low_c.contains("invalid uri") || low_c.contains("builder error") {
        (
            "La URL del nodo no es válida. Tiene que ser http:// o https://host:puerto.",
            "The node URL is not valid. Use http:// or https://host:port.",
        )
    } else if low_c.contains("rejected")
        || low_c.contains("double spend")
        || low_c.contains("key image")
        || low_c.contains("already spent")
    {
        (
            "El nodo rechazó la transacción. Suele ser una salida ya gastada, fee insuficiente o el nodo atrasado. Actualizá el saldo (y podá fantasmas si usás monerod propio) y reintentá.",
            "The node rejected the transaction. Often a spent output, low fee, or a lagging node. Refresh the balance and try again.",
        )
    } else if cuerpo.trim().is_empty()
        || cuerpo.trim() == "()"
        || low_c.ends_with("rejected ()")
        || low_c.ends_with("rejected()")
    {
        (
            "El nodo rechazó la transacción sin detalle. Probá Actualizar saldo, revisá el monerod y reintentá el fondeo.",
            "The node rejected the transaction without a reason. Refresh the balance, check monerod, and retry funding.",
        )
    } else {
        return if es {
            format!(
                "No pude hablar con el nodo Monero: {cuerpo}. Revisá la URL, el puerto y que el teléfono alcance esa máquina (LAN/Tailscale)."
            )
        } else {
            format!(
                "Could not talk to the Monero node: {cuerpo}. Check the URL, port, and that this phone can reach that machine (LAN/Tailscale)."
            )
        };
    };
    if es { esp.into() } else { ing.into() }
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
        "codigo:semilla-otra" => (
            "Ya hay otra semilla en este equipo. No la reemplazo: el otro tiene esta dirección para pagarte.",
            "This machine already has a different seed. It stays: the other person pays this address.",
        ),
        "codigo:semilla-red" | "codigo:share-red" => (
            "Ese archivo no es de stagenet.",
            "That file is not for stagenet.",
        ),
        "codigo:semilla-archivo" => (
            "Ese archivo no es una semilla de 25 palabras de Konstruado.",
            "That file is not a Konstruado 25-word seed.",
        ),
        "codigo:semilla-direccion" => (
            "La dirección del archivo no coincide con las 25 palabras.",
            "The address in the file does not match the 25 words.",
        ),
        "codigo:semilla-rota" => (
            "Ya hay un archivo de semilla en este equipo y no lo pude leer. No lo piso.",
            "A seed file is already here and I could not read it. I left it in place.",
        ),
        "codigo:share-obra" => (
            "Esa obra no está en este perfil. El share no rehace tu identidad ni el trato.",
            "That job is not in this profile. The share does not rebuild your identity or the deal.",
        ),
        "codigo:share-rol" => (
            "Ese share es del otro lado, o de alguien que no está en la obra. Cada equipo guarda el suyo.",
            "That share belongs to the other side, or to someone not on the job. Each machine keeps its own.",
        ),
        "codigo:share-distinto" => (
            "Ya hay otro share de esta obra. No lo reemplazo.",
            "This job already has a different share. I did not replace it.",
        ),
        "codigo:share-archivo" => (
            "Ese archivo no es un share de Konstruado.",
            "That file is not a Konstruado share.",
        ),
        "codigo:share-no" => (
            "El share de esta obra no está en este equipo.",
            "This machine does not have the share for this job.",
        ),
        "codigo:caja-tope" => (
            "Ya miré el tope de historia de la caja, unos 20 000 bloques.",
            "The box scan already reached its limit, about 20,000 blocks.",
        ),
        "codigo:archivo-existe" => (
            "Esa ruta ya tiene un archivo. Elegí otro nombre.",
            "That path already has a file. Choose another name.",
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
        "fondeo-rechazado" | "codigo:fondeo-rechazado" => (
            "El nodo rechazó el fondeo (decoys viejos o salida ya usada). Tocá «Empezar el fondeo de nuevo» para armar anillos frescos; la obra no se pierde.",
            "The node rejected the funding (stale decoys or a spent output). Tap Start funding again for fresh rings; the job is kept.",
        ),
        "fondeo-reinicio" | "codigo:fondeo-reinicio" => (
            "Reiniciando el fondeo con anillos nuevos…",
            "Restarting funding with fresh rings…",
        ),
        "fondeo-abortado-par" | "codigo:fondeo-abortado-par" => (
            "El otro reinició el fondeo. Cuando toque, confirmá o pedí de nuevo con anillos frescos.",
            "The other side restarted funding. When it is your turn, confirm or request again with fresh rings.",
        ),
        "par-salio-obra" | "codigo:par-salio-obra" => (
            "El otro salió de esta obra en su equipo. No mueve fondos; la caja 2-de-2 sigue si hay share.",
            "The other side left this job on their device. Funds are not moved; the 2-of-2 box stays if shares remain.",
        ),
        "par-salio-partida" | "codigo:par-salio-partida" => (
            "El otro canceló el fondeo o la propuesta de esta partida en su equipo. Los fondos en cadena no se tocan.",
            "The other side cancelled funding or the proposal for this stage on their device. On-chain funds are untouched.",
        ),
        "semilla-sin-altura" | "codigo:semilla-sin-altura" => (
            "Ese respaldo de semilla no trae altura de bloque (archivo viejo). El scan arranca en la ventana reciente; si el faucet es más viejo, pedí mirar más atrás.",
            "That seed backup has no block height (old file). Scan starts at the recent window; if the faucet is older, ask to look further back.",
        ),
        s if s.starts_with("codigo:podar-gastadas:") || s.starts_with("podar-gastadas:") => (
            "No pude comprobar qué salidas ya se gastaron en el nodo. Tocá Actualizar saldo; si sigue, probá otro daemon (is_key_image_spent).",
            "Could not check which outputs are spent on the node. Tap Refresh balance; if it persists, try another daemon (is_key_image_spent).",
        ),
        "esa partida no está en fondeo" => (
            "Esa partida no está en fondeo.",
            "That stage is not in funding.",
        ),
        other => {
            let low = other.to_ascii_lowercase();
            if low.contains("timeout")
                || low.contains("elapsed")
                || low.contains("connection refused")
                || low.contains("unreachable")
                || low.contains("interface error")
                || low.contains("rejected")
                || low.contains("key image")
                || low.contains("double spend")
                || other.trim() == "()"
                || other.starts_with("cadena:")
                || other.starts_with("Chain:")
            {
                return humanizar_error_cadena(other, es);
            }
            return other.to_string();
        },
    };
    if es { esp.into() } else { ing.into() }
}

/// Confirmación corta cuando un respaldo se guardó o ya estaba.
pub fn listo_humano(code: &str, es: bool) -> String {
    let (esp, ing) = match code {
        "codigo:semilla-nueva" => (
            "Recuperé la billetera personal. El scan parte de la altura guardada en el respaldo. La caja de la obra no está en esas palabras: hace falta el share.",
            "Restored the personal wallet. Scan starts at the height stored in the backup. Those words do not hold the job's box: the share is still required.",
        ),
        "codigo:semilla-nueva-sin-altura" => (
            "Recuperé la billetera (respaldo sin altura). El scan usa la ventana reciente; si el faucet es viejo, pedí mirar más atrás. La caja pide el share aparte.",
            "Restored the wallet (backup without height). Scan uses the recent window; if the faucet is old, look further back. The box still needs the share.",
        ),
        "codigo:semilla-igual" => (
            "Esas palabras ya son las de esta billetera.",
            "Those words are already this wallet.",
        ),
        "codigo:share-nuevo" => (
            "Recuperé la caja de esa obra. Si el fondeo es viejo, pedí mirar más bloques.",
            "Restored that job's box. If the funding is old, scan further back.",
        ),
        "codigo:share-igual" => (
            "Ese share ya estaba en este equipo.",
            "That share was already on this machine.",
        ),
        _ => return aviso_humano(code, es),
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


/// Misma regla en Dioxus y Compose: reinicio solo con partida Encerrando y freno del motor.
pub fn puede_empezar_fondeo_de_nuevo(encerrando: bool, frenado: bool) -> bool {
    encerrando && frenado
}

/// Qué está haciendo el motor con una partida, en una palabra. Sale de las
/// líneas de la vista y de los txid que ya vio el dominio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnCurso {
    Nada,
    /// Pedido de fondeo andando: se arma o se firma, todavía no está en la red.
    FondeoArmando,
    /// La tx de fondeo ya se publicó y espera bloque. No se cancela.
    FondeoEnRed,
    /// Aceptaron el porcentaje y se está firmando el pago 2-de-2.
    PagoFirmando,
    /// La tx de pago ya se publicó y espera bloque.
    PagoEnRed,
}

impl Texto {
    /// Clasifica la línea del motor. Los frenos no cuentan como "en curso".
    pub fn en_curso(&self) -> EnCurso {
        match self {
            Texto::EsperandoPago(_) => EnCurso::PagoEnRed,
            Texto::Gastando | Texto::BuscandoCaja => EnCurso::PagoFirmando,
            Texto::EsperandoFondeo(_) => EnCurso::FondeoEnRed,
            Texto::Fondeando
            | Texto::BuscandoMonedas
            | Texto::EsperandoPropuesta
            | Texto::EsperandoContratista
            | Texto::BuscandoOtro => EnCurso::FondeoArmando,
            _ => EnCurso::Nada,
        }
    }
}

impl CajaVista {
    /// Todas las líneas de una partida (puede haber una de fondeo y otra de pago).
    pub fn lineas_de(&self, obra: &str, partida: usize) -> Vec<Texto> {
        self.lineas
            .iter()
            .filter(|l| l.obra == obra && l.partida == Some(partida))
            .map(|l| l.texto.clone())
            .collect()
    }
}

/// Botones válidos para una partida. Una sola regla para Dioxus y Compose:
/// la pantalla muestra solo lo que está en `true`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccionesPartida {
    pub en_curso: EnCurso,
    /// El motor paró con un error conocido (sin saldo, rechazo del nodo…).
    pub frenado: bool,
    pub editar_texto: bool,
    pub proponer_encierre: bool,
    pub cancelar_propuesta: bool,
    pub confirmar_fondeo: bool,
    pub empezar_fondeo_de_nuevo: bool,
    pub no_encerrar: bool,
    pub avisar_termino: bool,
    /// En trato, me toca responder y no hay un pago ya andando.
    pub aceptar_pago: bool,
    pub contraofertar: bool,
    /// "Abandonar partida (solo este equipo)": limpia un fondeo local.
    pub salir_local: bool,
    /// Me toca responder el porcentaje (aunque el pago ya esté en curso, para textos).
    pub me_toca: bool,
}

pub fn acciones_partida(obra: &Obra, i: usize, mid: &str, lineas: &[Texto]) -> AccionesPartida {
    let mut a = AccionesPartida {
        en_curso: EnCurso::Nada,
        frenado: false,
        editar_texto: false,
        proponer_encierre: false,
        cancelar_propuesta: false,
        confirmar_fondeo: false,
        empezar_fondeo_de_nuevo: false,
        no_encerrar: false,
        avisar_termino: false,
        aceptar_pago: false,
        contraofertar: false,
        salir_local: false,
        me_toca: false,
    };
    let Some(p) = obra.partidas.get(i) else {
        return a;
    };
    let soy_m = obra.mandante.id == mid;
    let soy_c = obra.contratista.id == mid;
    if !soy_m && !soy_c {
        return a;
    }
    a.frenado = lineas.iter().any(es_freno);
    // Lo más avanzado gana: un pago en la red tapa cualquier otra línea.
    let rango = |e: EnCurso| match e {
        EnCurso::Nada => 0,
        EnCurso::FondeoArmando => 1,
        EnCurso::FondeoEnRed => 2,
        EnCurso::PagoFirmando => 3,
        EnCurso::PagoEnRed => 4,
    };
    for l in lineas {
        let e = l.en_curso();
        if rango(e) > rango(a.en_curso) {
            a.en_curso = e;
        }
    }
    if p.pago_txid.is_some() && p.estado != PartidaEstado::Pagada {
        a.en_curso = EnCurso::PagoEnRed;
    }
    let cortada = matches!(
        obra.estado,
        EstadoObra::Abandonada | EstadoObra::Cerrada | EstadoObra::Rechazada
    );
    if cortada {
        return a;
    }
    let activa = obra.activa() == Some(i);
    let contra = obra.estado == EstadoObra::Contra;
    let en_red = matches!(a.en_curso, EnCurso::FondeoEnRed | EnCurso::PagoEnRed);
    let hay_motor = a.en_curso != EnCurso::Nada;
    match p.estado {
        PartidaEstado::Pendiente => {
            a.editar_texto = true;
            a.proponer_encierre = !contra && activa;
            a.salir_local = a.frenado;
        }
        PartidaEstado::Encerrando => {
            let soy_prop = p.encerrado_por.as_ref().is_some_and(|q| q.id == mid);
            if soy_prop && !a.frenado {
                a.cancelar_propuesta = !en_red;
            } else if puede_empezar_fondeo_de_nuevo(true, a.frenado) {
                a.empezar_fondeo_de_nuevo = true;
                a.no_encerrar = true;
            } else if !hay_motor {
                a.confirmar_fondeo = true;
                a.no_encerrar = true;
            } else {
                a.no_encerrar = !en_red;
            }
            a.salir_local = !en_red;
        }
        PartidaEstado::Encerrada => {
            a.avisar_termino = soy_c;
        }
        PartidaEstado::EnTrato => {
            a.me_toca = match p.turno {
                Some(Rol::Mandante) => soy_m,
                Some(Rol::Contratista) => soy_c,
                None => false,
            };
            let pagando = matches!(a.en_curso, EnCurso::PagoFirmando | EnCurso::PagoEnRed);
            a.aceptar_pago = a.me_toca && !pagando;
            a.contraofertar = a.me_toca && !pagando;
        }
        PartidaEstado::Pagada => {}
    }
    a
}

/// Texto corto del estado en curso, para la lista de partidas y el encabezado.
pub fn en_curso_corto(e: EnCurso, es: bool) -> Option<&'static str> {
    Some(match (e, es) {
        (EnCurso::Nada, _) => return None,
        (EnCurso::FondeoArmando, true) => "Fondeando…",
        (EnCurso::FondeoArmando, false) => "Funding…",
        (EnCurso::FondeoEnRed, true) => "Fondeo esperando bloque",
        (EnCurso::FondeoEnRed, false) => "Funding waiting for a block",
        (EnCurso::PagoFirmando, true) => "Firmando el pago…",
        (EnCurso::PagoFirmando, false) => "Signing the payment…",
        (EnCurso::PagoEnRed, true) => "Pago esperando bloque",
        (EnCurso::PagoEnRed, false) => "Payment waiting for a block",
    })
}

fn otro_id_obra(obra: &Obra, yo: &str) -> String {
    if yo == obra.mandante.id {
        obra.contratista.id.clone()
    } else {
        obra.mandante.id.clone()
    }
}

/// Sella notas y extras en claro antes de republicar. Escritorio y Android llaman esto.
pub fn sellar_obras_guardadas(nodo: &Nodo, yo: &Persona, sec: &str) {
    for obra in nodo.obras() {
        if !obra.participa(&yo.id) {
            continue;
        }
        let mut sealed = obra.clone();
        if sealed.preparar_para_red(&yo.id, &yo.clave_pub, sec).is_ok() && sealed != obra {
            nodo.publicar_obra(sealed);
        }
    }
}

/// Encerrada/Pagada solo cuando el motor vio la tx. Un solo camino para ambos clientes.
pub fn aplicar_hechos_monero(nodo: &Nodo, yo: &Persona, sec: &str, hechos: &[Hecho]) {
    for h in hechos {
        let mut obras = nodo.obras();
        let Some(obra) = obras.iter_mut().find(|o| o.id == h.obra) else {
            continue;
        };
        let otro = otro_id_obra(obra, &yo.id);
        if !nodo.trato_alineado(&yo.id, &otro) {
            continue;
        }
        let mut publico = false;
        if let Some(txid) = &h.fondeo {
            let confirma = obra.partidas.get(h.partida).is_some_and(|p| {
                p.estado == PartidaEstado::Encerrando
                    && p.encerrado_por.as_ref().is_some_and(|q| q.id != yo.id)
            });
            if confirma && obra.encerrar_confirmar(h.partida, yo).is_ok() {
                obra.partidas[h.partida].fondeo_txid = Some(txid.clone());
                publico = true;
            }
        }
        if let Some(txid) = &h.pago {
            if obra.aceptar_pago(h.partida, yo).is_ok() {
                obra.partidas[h.partida].pago_txid = Some(txid.clone());
                publico = true;
            }
        }
        if publico && obra.preparar_para_red(&yo.id, &yo.clave_pub, sec).is_ok() {
            nodo.publicar_obra(obra.clone());
        }
    }
}

/// Resultado de «Probar RPC del nodo» (escritorio y Android).
#[derive(Clone, Debug)]
pub struct PruebaDaemon {
    pub ok: bool,
    pub url: String,
    pub tip: Option<u64>,
    pub ms: u64,
    pub mensaje: String,
    /// El nodo es de la red local / Tailscale (192.168.x, 10.x, 100.64/10…).
    pub local: bool,
    /// Ruta legible por la que salió la conexión.
    pub ruta: String,
}

/// Qué sabe la UI de la VPN del sistema para esta app (solo Android la detecta).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VpnApp {
    /// No se sabe (escritorio, o Android sin dato).
    Desconocida,
    /// La red por defecto de la app no es una VPN.
    Ninguna,
    /// Una VPN de todo el teléfono (p. ej. Orbot en modo VPN) captura el tráfico de la app.
    Captura,
}

/// Ruta del RPC del daemon. El transporte (`monero-simple-request-rpc`) no usa
/// SOCKS ni proxies del entorno: siempre abre TCP directo. Lo único que lo desvía
/// es una VPN del sistema que capture a la app (Orbot en modo VPN → Tor).
pub fn ruta_daemon(url: &str, vpn: VpnApp, es: bool) -> String {
    let local = url_es_local(url);
    let (e, i) = match (local, vpn) {
        (true, VpnApp::Captura) => (
            "red local, pero una VPN del teléfono (Orbot) captura la app",
            "local network, but a phone VPN (Orbot) captures the app",
        ),
        (true, _) => ("directo por la red local, sin Tor", "direct over the local network, no Tor"),
        (false, VpnApp::Captura) => (
            "por la VPN del teléfono (con Orbot, por Tor)",
            "through the phone VPN (with Orbot, over Tor)",
        ),
        (false, _) => (
            "directo por internet, sin Tor (el nodo ve tu IP)",
            "direct over the internet, no Tor (the node sees your IP)",
        ),
    };
    if es { e.into() } else { i.into() }
}

/// Pista cuando el nodo local falla con una VPN capturando la app.
pub fn pista_vpn_local(es: bool) -> &'static str {
    if es {
        "Orbot en modo VPN está capturando la conexión a la red local: la manda por Tor y Tor no llega a IPs privadas como esta. Android no deja que una app se salte la VPN de Orbot. Arreglo: en Orbot → «Elegir aplicaciones» marcá al menos otra app y dejá Konstruado sin marcar (si no hay ninguna marcada, Orbot vuelve a «VPN de dispositivo completo»), o apagá la VPN con «Modo de usuarie avanzado» en los ajustes de Orbot. La sala sigue yendo por Tor a través del SOCKS 127.0.0.1:9050."
    } else {
        "Orbot's VPN mode is capturing the connection to your local network: it sends it through Tor and Tor cannot reach private IPs like this one. Android does not let an app bypass Orbot's VPN. Fix: in Orbot → \"Choose apps\" select at least one other app and leave Konstruado unselected (with none selected Orbot goes back to \"Full Device VPN\"), or turn the VPN off with Orbot's \"Power User Mode\". The room still goes over Tor through SOCKS 127.0.0.1:9050."
    }
}

/// Pide la punta al daemon activo. No gasta monedas.
pub async fn probar_daemon(es: bool) -> PruebaDaemon {
    probar_daemon_con_vpn(es, VpnApp::Desconocida).await
}

/// Igual que [`probar_daemon`], con lo que la UI sabe de la VPN del sistema.
pub async fn probar_daemon_con_vpn(es: bool, vpn: VpnApp) -> PruebaDaemon {
    let url = daemon_url();
    let local = url_es_local(&url);
    let ruta = ruta_daemon(&url, vpn, es);
    let t0 = Instant::now();
    let res = match chain::connect(&url).await {
        Ok(rpc) => match chain::tip(&rpc).await {
            Ok(n) => Ok(n as u64),
            Err(e) => Err(e.to_string()),
        },
        Err(e) => Err(e.to_string()),
    };
    let ms = t0.elapsed().as_millis() as u64;
    match res {
        Ok(tip) => PruebaDaemon {
            ok: true,
            url: url.clone(),
            tip: Some(tip),
            ms,
            mensaje: if es {
                format!("RPC OK: el nodo {url} respondió la punta en el bloque {tip} ({ms} ms, {ruta}).")
            } else {
                format!("RPC OK: node {url} answered tip at block {tip} ({ms} ms, {ruta}).")
            },
            local,
            ruta,
        },
        Err(e) => {
            let base = if local && vpn == VpnApp::Captura {
                pista_vpn_local(es).to_string()
            } else {
                humanizar_error_cadena(&e, es)
            };
            PruebaDaemon {
                ok: false,
                url: url.clone(),
                tip: None,
                ms,
                mensaje: if es {
                    format!("{base} URL: {url}. Tardó {ms} ms antes de fallar.")
                } else {
                    format!("{base} URL: {url}. Failed after {ms} ms.")
                },
                local,
                ruta,
            }
        }
    }
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

    /// View key compartida de la caja, en hex. No es la parte de gasto.
    pub fn view_de(&self, obra: &str) -> Option<String> {
        let m = self.inner.lock().unwrap();
        let cuenta = m.cuentas.get(obra)?;
        Some(hex::encode(cuenta.view_private_bytes()))
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

    /// Copia el share de esta obra a la ruta que eligió el usuario. No pisa el archivo interno.
    pub fn guardar_share(&self, obra: &str, path: &Path) -> Result<(), String> {
        self.inner.lock().unwrap().exportar_share(obra, path)
    }

    /// Recupera la billetera personal. No trae la caja. No pisa una semilla distinta.
    pub fn restaurar_semilla(&self, path: &Path) -> Result<&'static str, String> {
        let mut m = self.inner.lock().unwrap();
        let code = m.restaurar_semilla(path)?;
        m.armar_vista(&[]);
        Ok(code)
    }

    /// Recupera el share de una obra que ya está en este perfil, si el rol coincide.
    pub fn restaurar_share(&self, path: &Path, yo: &Persona, obras: &[Obra]) -> Result<&'static str, String> {
        let mut m = self.inner.lock().unwrap();
        let code = m.restaurar_share(path, yo, obras)?;
        m.armar_vista(obras);
        Ok(code)
    }

    /// Suma hasta 200 bloques a la historia de esta caja. El scan va de a 8.
    pub fn pedir_atras_caja(&self, obra: &str) -> Result<(), String> {
        let mut m = self.inner.lock().unwrap();
        m.pedir_atras_caja(obra)?;
        m.armar_vista(&[]);
        Ok(())
    }

    pub fn pedir_fondeo(&self, obra: &Obra, partida: usize, yo: &Persona) -> Result<(), String> {
        self.inner.lock().unwrap().pedir_fondeo(obra, partida, yo)
    }

    /// Vuelve a armar el fondeo sin borrar el pedido a mitad de una transacción ya publicada.
    pub fn reintentar_fondeo(
        &self,
        obra: &Obra,
        partida: usize,
        yo: &Persona,
        nodo: &Nodo,
    ) -> Result<(), String> {
        // Mismo camino que «Empezar de nuevo»: decoys frescos en los dos lados.
        self.empezar_fondeo_de_nuevo(obra, partida, yo, nodo)
    }

    /// Borra decoys/propuesta locales, avisa al peer con `fund-abort` y pide fondeo fresco.
    pub fn empezar_fondeo_de_nuevo(
        &self,
        obra: &Obra,
        partida: usize,
        yo: &Persona,
        nodo: &Nodo,
    ) -> Result<(), String> {
        self.inner
            .lock()
            .unwrap()
            .empezar_fondeo_de_nuevo(obra, partida, yo, nodo)
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
    podar_en: Option<Instant>,
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
    cajas_libros: HashMap<String, LibroCaja>,
    caja_pausa: Option<Instant>,
    caja_aviso: Option<String>,
    caja_aviso_obra: Option<String>,
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
    /// Salidas personales (tx, índice) que este lado metió al fondeo. Se sacan del libro al publicar.
    gastadas: Option<Vec<(String, u64)>>,
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
        /// Salidas del libro que el daemon marca como gastadas (key image).
        podar: Vec<(String, u64)>,
        /// Si el daemon no pudo decir cuáles están gastadas (RPC/ruta).
        podar_err: Option<String>,
    },
    CajaScan {
        obra: String,
        desde: usize,
        hasta: usize,
        tip: usize,
        retro: bool,
        entradas: Vec<EntradaNueva>,
        error: Option<String>,
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
            podar_en: None,
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
            cajas_libros: HashMap::new(),
            caja_pausa: None,
            caja_aviso: None,
            caja_aviso_obra: None,
            envio_aviso: None,
            ultimo_envio: None,
            ultimo_fee: None,
            ultimo_cambio: None,
            scan_pausa: None,
        };
        m.cargar_libro();
        m.cargar_shares();
        m.cargar_libros_caja();
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
            height: self.tip.map(|t| t as u64),
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
        let mut parsed = SeedBackup::parse(&text).map_err(|e| e.to_string())?;
        // Al exportar, grabamos la punta actual para que el restore no arranque desde génesis.
        if let Some(tip) = self.tip {
            parsed.height = Some(tip as u64);
        }
        let body = parsed.to_text();
        // Actualiza también el archivo interno, así el height no se pierde.
        let _ = std::fs::remove_file(semilla_path());
        let _ = backup::write_secret_file(&semilla_path(), &body);
        backup::write_secret_file(path, &body).map_err(|e| e.to_string())
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
                gastadas: None,
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
                    gastadas: None,
                },
            );
            return Ok(());
        }
        self.pedir_fondeo(obra, partida, yo)
    }

    fn cancelar_fondeo(&mut self, obra: &str, partida: usize) {
        self.limpiar_sesion_fondeo(obra, partida, false);
    }

    /// Borra propuesta/esqueleto/decoys/txid locales. Si `solo_si_no_visto`, no toca un fondeo ya visto en cadena.
    fn limpiar_sesion_fondeo(&mut self, obra: &str, partida: usize, forzar: bool) {
        let key = (obra.to_string(), partida);
        if let Some(f) = self.fondeos.get(&key) {
            if f.visto && !forzar {
                return;
            }
        }
        self.fondeos.remove(&key);
        let _ = std::fs::remove_file(espera_path(obra, partida, false));
        // Quita frenos viejos de esta partida; el caller puede poner uno nuevo.
        self.vista
            .lineas
            .retain(|l| !(l.obra == obra && l.partida == Some(partida)));
    }

    /// Avisa al peer y deja la partida Encerrando lista para un fondeo nuevo (decoys frescos).
    fn reiniciar_fondeo_tras_rechazo(
        &mut self,
        obra: &str,
        partida: usize,
        nodo: &Nodo,
        yo: &Persona,
    ) {
        let peer = self
            .fondeos
            .get(&(obra.to_string(), partida))
            .map(|f| f.peer.clone())
            .unwrap_or_default();
        if !peer.is_empty() {
            let cuerpo = partida.to_string();
            let _ = nodo.enviar_caja(obra, &peer, &yo.id, "fund-abort", cuerpo.as_bytes());
        }
        self.limpiar_sesion_fondeo(obra, partida, false);
        self.nota(
            obra,
            Some(partida),
            Texto::Falla("fondeo-rechazado".into()),
        );
    }

    /// Empezar el fondeo de cero: aborta la sesión en los dos lados y vuelve a pedir.
    fn empezar_fondeo_de_nuevo(
        &mut self,
        obra: &Obra,
        partida: usize,
        yo: &Persona,
        nodo: &Nodo,
    ) -> Result<(), String> {
        let p = obra.partidas.get(partida).ok_or("no está esa partida")?;
        if p.estado != PartidaEstado::Encerrando {
            return Err("esa partida no está en fondeo".into());
        }
        if rol_en(obra, &yo.id).is_none() {
            return Err("no estás en esta obra".into());
        }
        let peer = self
            .fondeos
            .get(&(obra.id.clone(), partida))
            .map(|f| f.peer.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| otro_id(obra, &yo.id).to_string());
        if !peer.is_empty() {
            let cuerpo = partida.to_string();
            let _ = nodo.enviar_caja(&obra.id, &peer, &yo.id, "fund-abort", cuerpo.as_bytes());
        }
        self.limpiar_sesion_fondeo(&obra.id, partida, false);
        self.nota(
            &obra.id,
            Some(partida),
            Texto::Falla("fondeo-reinicio".into()),
        );
        // Arranca sesión fresca (sin decoys viejos).
        self.pedir_fondeo(obra, partida, yo)?;
        // Quita el aviso de reinicio cuando ya hay sesión limpia pidiendo monedas.
        self.vista
            .lineas
            .retain(|l| !(l.obra == obra.id && l.partida == Some(partida) && matches!(l.texto, Texto::Falla(ref s) if s == "fondeo-reinicio")));
        Ok(())
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
                        self.limpiar_sesion_fondeo(&m.obra, i, false);
                        self.nota(
                            &m.obra,
                            Some(i),
                            Texto::Falla("fondeo-abortado-par".into()),
                        );
                    }
                }
                "obra-salida" => {
                    self.nota(&m.obra, None, Texto::Falla("par-salio-obra".into()));
                }
                "partida-salida" => {
                    if let Ok(i) = parse_idx(&m.cuerpo) {
                        self.limpiar_sesion_fondeo(&m.obra, i, false);
                        self.nota(
                            &m.obra,
                            Some(i),
                            Texto::Falla("par-salio-partida".into()),
                        );
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
        } else {
            let key = self
                .fondeos
                .iter()
                .find(|(k, f)| k.0 == m.obra && f.peer == m.de)
                .map(|(k, _)| k.clone());
            if let Some(key) = key {
                if let Some(f) = self.fondeos.get_mut(&key) {
                    f.txid = Some(txid);
                    f.error = None;
                }
                self.consumir_fondeo_personal(&key.0, key.1);
            }
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
        let id = account.obra_id().to_string();
        let addr = account.address().to_string();
        self.cuentas.insert(id.clone(), account);
        self.cajas_libros.entry(id).or_insert_with(|| LibroCaja::nueva(&addr));
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
                    // No marcar gastadas hasta que el nodo acepte el broadcast.
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
            let res = match chain::connect(&daemon_url()).await {
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
            if pago {
                self.spawn_ver(obra, i, true);
                return;
            }
            let txid = self
                .fondeos
                .get(&(obra.clone(), i))
                .and_then(|f| f.txid.clone());
            if let Some(txid) = txid {
                if self.caja_tiene_tx(&obra, &txid) {
                    if let Some(f) = self.fondeos.get_mut(&(obra.clone(), i)) {
                        f.visto = true;
                        f.error = None;
                    }
                } else if !self.caja_cubierta(&obra) {
                    if self.lanzar_trozo_caja(&obra) {
                        return;
                    }
                } else if let Some(f) = self.fondeos.get_mut(&(obra, i)) {
                    f.visto = false;
                    f.ultimo = Some(Instant::now());
                }
            }
        }
        self.empujar_historia();
        self.cerrar_busqueda_vacia();
        self.emitir_bloqueos(nodo, yo);
        if let Some((obra, i)) = self.busca_entradas(false) {
            self.spawn_entradas(obra, i, false, nodo, yo);
            return;
        }
        if let Some((obra, i)) = self.busca_entradas(true) {
            let amount = self
                .gastos
                .get(&(obra.clone(), i))
                .map(|g| g.capital)
                .unwrap_or(0);
            match self.cobertura_de(&obra, amount, 2) {
                CoberturaCaja::Libre => {
                    self.spawn_entradas(obra, i, true, nodo, yo);
                    return;
                }
                CoberturaCaja::Buscando => {
                    if self.lanzar_trozo_caja(&obra) {
                        return;
                    }
                }
                CoberturaCaja::SinSaldo => {
                    self.fallar(&obra, i, true, "sin-saldo-caja".into(), nodo, yo);
                }
                CoberturaCaja::Trabadas => {
                    self.fallar(&obra, i, true, "trabadas-caja".into(), nodo, yo);
                }
            }
        }
        if self.lanzar_envio() {
            return;
        }
        self.lanzar_saldo();
        if self.ocupado.is_none() {
            self.lanzar_caja_ociosa();
        }
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
        // Si el respaldo traía una altura mayor al tip actual, no miramos el futuro.
        if self.libro.listo && self.libro.desde > tip {
            self.libro.desde = tip;
            self.libro.hasta = tip.saturating_sub(1);
            self.guardar_libro();
        }
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
            return;
        }
        let _ = view;
        // Al día: podar salidas gastadas. Tras restaurar seed el scan las revive
        // y sin esta pasada el saldo (p.ej. en Android) se infla.
        let hace = self
            .podar_en
            .map(|t| t.elapsed())
            .unwrap_or(Duration::from_secs(9_999));
        if !self.libro.entradas.is_empty() && hace >= Duration::from_secs(20) {
            self.spawn_podar_gastadas();
        }
    }

    fn spawn_saldo(&mut self, view: xmr_joint::ViewPair, desde: usize, hasta: usize, retro: bool) {
        self.buscando = true;
        let spend = self.wallet.as_ref().map(|w| w.spend_key().clone());
        let crudas: Vec<(String, u64, Vec<u8>)> = self
            .libro
            .entradas
            .iter()
            .map(|e| (e.tx.clone(), e.indice, e.raw.clone()))
            .collect();
        let celda = self.ocupar();
        tokio::spawn(async move {
            let listo = match chain::connect(&daemon_url()).await {
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
                        let (podar, podar_err) = match podar_gastadas_en_cadena(&rpc, spend.as_ref(), &crudas).await {
                            Ok(p) => (p, None),
                            Err(e) => (Vec::new(), Some(e)),
                        };
                        Listo::Saldo {
                            desde,
                            hasta,
                            tip,
                            retro,
                            entradas,
                            podar,
                            podar_err,
                        }
                    }
                    Err(e) => Listo::Aviso(e.to_string()),
                },
                Err(e) => Listo::Aviso(e.to_string()),
            };
            *celda.lock().unwrap() = Some(listo);
        });
    }

    fn spawn_podar_gastadas(&mut self) {
        if self.ocupado.is_some() || self.wallet.is_none() || self.libro.entradas.is_empty() {
            return;
        }
        self.podar_en = Some(Instant::now());
        let spend = self.wallet.as_ref().map(|w| w.spend_key().clone());
        let crudas: Vec<(String, u64, Vec<u8>)> = self
            .libro
            .entradas
            .iter()
            .map(|e| (e.tx.clone(), e.indice, e.raw.clone()))
            .collect();
        let tip0 = self.tip.unwrap_or(0);
        let celda = self.ocupar();
        tokio::spawn(async move {
            let listo = match chain::connect(&daemon_url()).await {
                Ok(rpc) => {
                    let tip = chain::tip(&rpc).await.unwrap_or(tip0);
                    match podar_gastadas_en_cadena(&rpc, spend.as_ref(), &crudas).await {
                        Ok(podar) => Listo::Saldo {
                            desde: tip,
                            hasta: tip,
                            tip,
                            retro: false,
                            entradas: Vec::new(),
                            podar,
                            podar_err: None,
                        },
                        Err(e) => Listo::Saldo {
                            desde: tip,
                            hasta: tip,
                            tip,
                            retro: false,
                            entradas: Vec::new(),
                            podar: Vec::new(),
                            podar_err: Some(e),
                        },
                    }
                }
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
            let listo = match chain::connect(&daemon_url()).await {
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
            let Some((raw, tx, indice)) = self.salida_libre_id(minimo) else {
                return;
            };
            if let Some(f) = self.fondeos.get_mut(&(obra.clone(), partida)) {
                f.gastadas = Some(vec![(tx, indice)]);
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
        if !self.cuentas.contains_key(&obra) {
            self.fallar(&obra, partida, true, "sin-caja".into(), nodo, yo);
            return;
        }
        let amount = self
            .gastos
            .get(&(obra.clone(), partida))
            .map(|g| g.capital)
            .unwrap_or(0);
        let raws = self.raws_exactos(&obra, amount);
        if raws.len() != 2 {
            self.fallar(&obra, partida, true, "sin-saldo-caja".into(), nodo, yo);
            return;
        }
        if let Some(g) = self.gastos.get_mut(&(obra.clone(), partida)) {
            g.ultimo = Some(Instant::now());
        }
        let celda = self.ocupar();
        tokio::spawn(async move {
            let listo = chain::anillar_varias(raws)
                .await
                .map(|(decoys, fee)| Listo::Entradas {
                    obra: obra.clone(),
                    partida,
                    pago,
                    decoys,
                    fee,
                })
                .unwrap_or_else(|e| Listo::Fallo {
                    obra,
                    partida: Some(partida),
                    pago: Some(true),
                    msg: e.to_string(),
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
                        if !pago {
                            self.consumir_fondeo_personal(&obra, partida);
                        }
                        if let Some(peer) = peer {
                            let paso = if pago { "spend-tx" } else { "fund-tx" };
                            let _ = nodo.enviar_caja(&obra, &peer, &yo.id, paso, txid.as_bytes());
                        }
                    }
                    Err(e) => {
                        let rechazo = !pago && es_rechazo_fondeo(&e);
                        self.fallar(&obra, partida, pago, e.clone(), nodo, yo);
                        if rechazo {
                            self.reiniciar_fondeo_tras_rechazo(&obra, partida, nodo, yo);
                        }
                    }
                }
            }
            Listo::Saldo {
                desde,
                hasta,
                tip,
                retro,
                entradas,
                podar,
                podar_err,
            } => {
                self.buscando = false;
                self.scan_pausa = None;
                self.tip = Some(tip);
                let hubo_scan = !entradas.is_empty() || desde != hasta || retro;
                self.fundir_entradas(entradas);
                if !podar.is_empty() {
                    self.libro.marcar_gastadas(&podar);
                    self.podar_en = Some(Instant::now());
                }
                if let Some(e) = podar_err {
                    self.scan_aviso = Some(format!("codigo:podar-gastadas:{e}"));
                } else if self
                    .scan_aviso
                    .as_deref()
                    .is_some_and(|a| a.starts_with("codigo:podar-gastadas:"))
                {
                    self.scan_aviso = None;
                } else if hubo_scan {
                    self.scan_aviso = None;
                }
                if retro {
                    self.libro.desde = desde;
                    let n = hasta.saturating_sub(desde).saturating_add(1);
                    self.retro = self.retro.saturating_sub(n);
                } else if hubo_scan {
                    self.libro.hasta = hasta;
                }
                self.guardar_libro();
            }
            Listo::CajaScan {
                obra,
                desde,
                hasta,
                tip,
                retro,
                entradas,
                error,
            } => {
                self.tip = Some(tip);
                if let Some(error) = error {
                    self.caja_pausa = Some(Instant::now());
                    self.caja_aviso = Some(error);
                    self.caja_aviso_obra = Some(obra);
                } else {
                    self.caja_pausa = None;
                    self.caja_aviso = None;
                    self.caja_aviso_obra = None;
                    self.fundir_caja(&obra, entradas);
                    let era_nuevo = self.cajas_libros.get(&obra).is_some_and(|l| !l.listo);
                    if let Some(libro) = self.cajas_libros.get_mut(&obra) {
                        if retro {
                            libro.desde = desde;
                            let n = hasta.saturating_sub(desde).saturating_add(1);
                            libro.retro = libro.retro.saturating_sub(n);
                        } else if era_nuevo {
                            libro.desde = desde;
                            libro.hasta = hasta;
                            libro.listo = true;
                        } else {
                            libro.hasta = hasta.max(libro.hasta);
                        }
                    }
                    self.guardar_libro_caja(&obra);
                }
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
                        self.libro.marcar_gastadas(&usadas);
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
            if self.libro.ya_gastada(&nueva.tx, nueva.indice) {
                continue;
            }
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
        self.salida_libre_id(minimo).map(|(raw, _, _)| raw)
    }

    fn salida_libre_id(&self, minimo: u64) -> Option<(Vec<u8>, String, u64)> {
        let tip = self.tip?;
        let pares: Vec<(u64, usize)> = self
            .libro
            .entradas
            .iter()
            .map(|e| (e.monto, e.altura))
            .collect();
        let i = indice_salida_libre(&pares, tip, minimo)?;
        let e = &self.libro.entradas[i];
        Some((e.raw.clone(), e.tx.clone(), e.indice))
    }

    /// Saca del saldo personal las salidas que este lado ya metió al fondeo publicado.
    fn consumir_fondeo_personal(&mut self, obra: &str, partida: usize) {
        let Some(usadas) = self
            .fondeos
            .get(&(obra.to_string(), partida))
            .and_then(|f| f.gastadas.clone())
        else {
            return;
        };
        if usadas.is_empty() {
            return;
        }
        self.libro.marcar_gastadas(&usadas);
        self.guardar_libro();
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

    fn texto_gasto(&self, obra: &str, g: &Gasto) -> Texto {
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
        if g.soy_mandante && g.propuesta.is_none() {
            return match self.cobertura_de(obra, g.capital, 2) {
                CoberturaCaja::Buscando => Texto::BuscandoCaja,
                CoberturaCaja::SinSaldo => Texto::SinSaldoCaja,
                CoberturaCaja::Trabadas => Texto::TrabadasCaja,
                CoberturaCaja::Libre => Texto::Gastando,
            };
        }
        Texto::Gastando
    }

    fn armar_vista(&mut self, obras: &[Obra]) {
        let mut v = CajaVista::vacia();
        v.daemon = daemon_url();
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
            let texto = self.texto_gasto(obra, g);
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
        v.miradas = self.miradas();
        self.vista = v;
    }

    fn miradas(&self) -> Vec<MiradaCaja> {
        let mut out = Vec::new();
        for id in self.cuentas.keys() {
            let libro = self.cajas_libros.get(id);
            let (retro, bloques) = match libro {
                Some(l) => (l.retro, bloques_mirados(self.tip, l)),
                None => (0, LOOKBACK),
            };
            let aviso = (self.caja_aviso_obra.as_deref() == Some(id.as_str()))
                .then(|| self.caja_aviso.clone())
                .flatten();
            out.push(MiradaCaja {
                obra: id.clone(),
                retro,
                bloques,
                aviso,
            });
        }
        out
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
        let gastadas: Vec<(String, u64)> = disco
            .gastadas
            .into_iter()
            .map(|g| (g.tx, g.indice))
            .collect();
        let entradas = disco
            .entradas
            .into_iter()
            .filter_map(|e| {
                if gastadas.iter().any(|(tx, i)| tx == &e.tx && *i == e.indice) {
                    return None;
                }
                let raw = hex::decode(e.raw).ok()?;
                Some(Entrada {
                    altura: e.altura as usize,
                    monto: e.monto,
                    tx: e.tx,
                    indice: e.indice,
                    raw,
                })
            })
            .collect();
        self.libro = Libro {
            direccion: disco.direccion,
            desde: disco.desde as usize,
            hasta: disco.hasta as usize,
            listo: disco.listo,
            entradas,
            gastadas,
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
            gastadas: self
                .libro
                .gastadas
                .iter()
                .map(|(tx, indice)| EntradaRefDisco {
                    tx: tx.clone(),
                    indice: *indice,
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
                gastadas: None,
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

    fn exportar_share(&self, obra: &str, path: &Path) -> Result<(), String> {
        let cuenta = self.cuentas.get(obra).ok_or("codigo:share-no")?;
        let mut text = cuenta
            .backup()
            .map_err(|_| "codigo:share-archivo".to_string())?
            .to_text();
        if let Ok(disk) = backup::read_secret_file(&share_path(obra)) {
            if let Ok(parsed) = ShareBackup::parse(&disk) {
                if parsed.obra_id == obra {
                    text = disk;
                }
            }
        }
        if path.exists() {
            return Err("codigo:archivo-existe".into());
        }
        backup::write_secret_file(path, &text).map_err(|e| e.to_string())
    }

    fn restaurar_semilla(&mut self, path: &Path) -> Result<&'static str, String> {
        let text = backup::read_secret_file(path).map_err(|_| "codigo:semilla-archivo".to_string())?;
        let altura = SeedBackup::parse(&text).ok().and_then(|b| b.height);
        let actual = self.wallet.as_ref().map(|w| w.address().to_string());
        let (addr, igual) = restaurar_semilla_en(&xmr_dir(), &text, actual.as_deref())?;
        if igual {
            if self.wallet.is_none() {
                self.wallet = leer_semilla();
            }
            return Ok("codigo:semilla-igual");
        }
        self.wallet = leer_semilla();
        if self.wallet.as_ref().map(|w| w.address()) != Some(addr.as_str()) {
            return Err("codigo:semilla-archivo".into());
        }
        if self.libro.direccion != addr {
            self.libro = match altura {
                Some(h) => Libro::desde_altura(&addr, h as usize),
                None => Libro::nueva(&addr),
            };
            self.retro = 0;
            self.forzar = false;
            self.buscando = false;
            self.scan_aviso = if altura.is_none() {
                Some("codigo:semilla-sin-altura".into())
            } else {
                None
            };
            self.scan_pausa = None;
            self.envio_aviso = None;
            self.ultimo_envio = None;
            self.ultimo_fee = None;
            self.ultimo_cambio = None;
            self.guardar_libro();
            // Que el próximo tick pode key images (salidas gastadas del respaldo).
            self.podar_en = None;
            self.forzar = true;
        }
        Ok(if altura.is_some() {
            "codigo:semilla-nueva"
        } else {
            "codigo:semilla-nueva-sin-altura"
        })
    }

    fn restaurar_share(&mut self, path: &Path, yo: &Persona, obras: &[Obra]) -> Result<&'static str, String> {
        let text = backup::read_secret_file(path).map_err(|_| "codigo:share-archivo".to_string())?;
        let share = ShareBackup::parse(&text).map_err(|_| "codigo:share-archivo".to_string())?;
        if !id_sano(&share.obra_id) {
            return Err("codigo:share-obra".into());
        }
        let obra = obras.iter().find(|o| o.id == share.obra_id);
        let rol_mio = obra.and_then(|o| rol_en(o, &yo.id)).map(|p| p.label());
        validar_share_meta(obra.is_some(), rol_mio, &share.role, share.net == Net::Stagenet)
            .map_err(|e| e.to_string())?;
        let account = JointAccount::from_backup(&share).map_err(|_| "codigo:share-archivo".to_string())?;
        let id = account.obra_id().to_string();
        let dest = share_path(&id);
        let hay = self.cuentas.contains_key(&id);
        let iguales = self
            .cuentas
            .get(&id)
            .is_some_and(|prev| misma_cuenta(prev, &account));
        if hay {
            validar_share_existente(true, iguales).map_err(|e| e.to_string())?;
            if !dest.exists() {
                let body = account.backup().map_err(|_| "codigo:share-archivo".to_string())?.to_text();
                backup::write_secret_file(&dest, &body).map_err(|e| e.to_string())?;
            }
            return Ok("codigo:share-igual");
        }
        if dest.exists() {
            let disk_txt = backup::read_secret_file(&dest).map_err(|_| "codigo:share-distinto".to_string())?;
            let disk = ShareBackup::parse(&disk_txt).map_err(|_| "codigo:share-distinto".to_string())?;
            let disk_acc = JointAccount::from_backup(&disk).map_err(|_| "codigo:share-distinto".to_string())?;
            if !misma_cuenta(&disk_acc, &account) {
                return Err("codigo:share-distinto".into());
            }
            self.meter_cuenta(id, account);
            return Ok("codigo:share-igual");
        }
        let body = account.backup().map_err(|_| "codigo:share-archivo".to_string())?.to_text();
        backup::write_secret_file(&dest, &body).map_err(|e| e.to_string())?;
        self.meter_cuenta(id, account);
        Ok("codigo:share-nuevo")
    }

    fn meter_cuenta(&mut self, id: String, account: JointAccount) {
        let addr = account.address().to_string();
        self.cuentas.insert(id.clone(), account);
        match self.cajas_libros.get(&id) {
            Some(l) if l.direccion == addr => {}
            _ => {
                self.cajas_libros.insert(id.clone(), LibroCaja::nueva(&addr));
            }
        }
        self.dkg.remove(&id);
        self.dkg_inbox.remove(&id);
    }

    fn pedir_atras_caja(&mut self, obra: &str) -> Result<(), String> {
        let addr = self
            .cuentas
            .get(obra)
            .map(|c| c.address().to_string())
            .ok_or("codigo:share-no")?;
        let (desde, listo, retro) = {
            let libro = self
                .cajas_libros
                .entry(obra.to_string())
                .or_insert_with(|| LibroCaja::nueva(&addr));
            if libro.direccion != addr {
                *libro = LibroCaja::nueva(&addr);
            }
            (libro.desde, libro.listo, libro.retro)
        };
        let n = sumar_atras(self.tip.unwrap_or(0), desde, listo, retro);
        if n == 0 {
            return Err("codigo:caja-tope".into());
        }
        if let Some(libro) = self.cajas_libros.get_mut(obra) {
            libro.retro = libro.retro.saturating_add(n);
        }
        for ((o, _), g) in self.gastos.iter_mut() {
            if o == obra && g.error.as_deref() == Some("sin-saldo-caja") {
                g.error = None;
                g.ultimo = None;
            }
        }
        for ((o, _), f) in self.fondeos.iter_mut() {
            if o == obra && f.txid.is_some() && !f.visto {
                f.ultimo = None;
            }
        }
        self.caja_pausa = None;
        self.caja_aviso = None;
        self.caja_aviso_obra = None;
        self.guardar_libro_caja(obra);
        Ok(())
    }

    fn lanzar_caja_ociosa(&mut self) {
        if self.ocupado.is_some() {
            return;
        }
        let ids: Vec<String> = self.cuentas.keys().cloned().collect();
        for id in ids {
            if self.lanzar_trozo_caja(&id) {
                return;
            }
        }
    }

    fn lanzar_trozo_caja(&mut self, obra: &str) -> bool {
        if self.ocupado.is_some() || !frio(self.caja_pausa) {
            return false;
        }
        let Some(tip) = self.tip else {
            return false;
        };
        let (addr, view) = {
            let Some(cuenta) = self.cuentas.get(obra) else {
                return false;
            };
            let addr = cuenta.address().to_string();
            let Ok(view) = cuenta.view_pair() else {
                return false;
            };
            (addr, view)
        };
        let libro = self
            .cajas_libros
            .entry(obra.to_string())
            .or_insert_with(|| LibroCaja::nueva(&addr));
        if libro.direccion != addr {
            *libro = LibroCaja::nueva(&addr);
        }
        let Some((desde, hasta, retro)) =
            siguiente_trozo(tip, libro.desde, libro.hasta, libro.listo, libro.retro)
        else {
            return false;
        };
        if hasta < desde {
            return false;
        }
        let obra = obra.to_string();
        let celda = self.ocupar();
        tokio::spawn(async move {
            let listo = match chain::connect(&daemon_url()).await {
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
                        let tip_ahora = chain::tip(&rpc).await.unwrap_or(hasta);
                        Listo::CajaScan {
                            obra,
                            desde,
                            hasta,
                            tip: tip_ahora,
                            retro,
                            entradas,
                            error: None,
                        }
                    }
                    Err(e) => Listo::CajaScan {
                        obra,
                        desde,
                        hasta,
                        tip,
                        retro,
                        entradas: Vec::new(),
                        error: Some(e.to_string()),
                    },
                },
                Err(e) => Listo::CajaScan {
                    obra,
                    desde,
                    hasta,
                    tip,
                    retro,
                    entradas: Vec::new(),
                    error: Some(e.to_string()),
                },
            };
            *celda.lock().unwrap() = Some(listo);
        });
        true
    }

    fn caja_tiene_tx(&self, obra: &str, txid: &str) -> bool {
        self.cajas_libros
            .get(obra)
            .is_some_and(|l| l.entradas.iter().any(|e| e.tx == txid))
    }

    fn caja_cubierta(&self, obra: &str) -> bool {
        let Some(tip) = self.tip else {
            return false;
        };
        let Some(libro) = self.cajas_libros.get(obra) else {
            return false;
        };
        libro.listo && libro.hasta >= tip && libro.retro == 0
    }

    fn cobertura_de(&self, obra: &str, amount: u64, cuantos: usize) -> CoberturaCaja {
        let (listo, desde, hasta, retro, entradas) = match self.cajas_libros.get(obra) {
            Some(l) => (
                l.listo,
                l.desde,
                l.hasta,
                l.retro,
                l.entradas.iter().map(|e| (e.monto, e.altura)).collect::<Vec<_>>(),
            ),
            None => (false, 0, 0, 0, Vec::new()),
        };
        cobertura_caja(&entradas, self.tip, listo, desde, hasta, retro, amount, cuantos)
    }

    fn raws_exactos(&self, obra: &str, amount: u64) -> Vec<Vec<u8>> {
        let Some(tip) = self.tip else {
            return Vec::new();
        };
        let Some(libro) = self.cajas_libros.get(obra) else {
            return Vec::new();
        };
        let mut exactas: Vec<&Entrada> = libro.entradas.iter().filter(|e| e.monto == amount).collect();
        exactas.sort_by_key(|e| (e.altura, e.indice));
        let Some(nuevas) = exactas.get(exactas.len().saturating_sub(2)..) else {
            return Vec::new();
        };
        if nuevas.len() != 2 || nuevas.iter().any(|e| tip < e.altura.saturating_add(10)) {
            return Vec::new();
        }
        nuevas.iter().map(|e| e.raw.clone()).collect()
    }

    fn fundir_caja(&mut self, obra: &str, entradas: Vec<EntradaNueva>) {
        let Some(libro) = self.cajas_libros.get_mut(obra) else {
            return;
        };
        for nueva in entradas {
            let ya = libro
                .entradas
                .iter()
                .any(|e| e.tx == nueva.tx && e.indice == nueva.indice);
            if ya {
                continue;
            }
            libro.entradas.push(Entrada {
                altura: nueva.altura,
                monto: nueva.monto,
                tx: nueva.tx,
                indice: nueva.indice,
                raw: nueva.raw,
            });
        }
    }

    fn cargar_libros_caja(&mut self) {
        let ids: Vec<String> = self.cuentas.keys().cloned().collect();
        for id in ids {
            let Some(addr) = self.cuentas.get(&id).map(|c| c.address().to_string()) else {
                continue;
            };
            if !id_sano(&id) {
                continue;
            }
            let libro = match backup::read_secret_file(&libro_caja_path(&id))
                .ok()
                .and_then(|t| serde_json::from_str::<LibroCajaDisco>(&t).ok())
            {
                Some(d) if d.direccion == addr => LibroCaja::de_disco(d),
                _ => LibroCaja::nueva(&addr),
            };
            self.cajas_libros.insert(id, libro);
        }
    }

    fn guardar_libro_caja(&self, obra: &str) {
        if !id_sano(obra) {
            return;
        }
        let Some(libro) = self.cajas_libros.get(obra) else {
            return;
        };
        if libro.direccion.is_empty() {
            return;
        }
        let disco = LibroCajaDisco {
            direccion: libro.direccion.clone(),
            desde: libro.desde as u64,
            hasta: libro.hasta as u64,
            listo: libro.listo,
            retro: libro.retro as u64,
            entradas: libro
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
        let _ = escribir_0600(&libro_caja_path(obra), &text);
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
        gastadas: None,
    }
}


async fn podar_gastadas_en_cadena(
    rpc: &xmr_joint::chain::Daemon,
    spend: Option<&xmr_joint::LlaveGasto>,
    crudas: &[(String, u64, Vec<u8>)],
) -> Result<Vec<(String, u64)>, String> {
    let Some(spend) = spend else {
        return Ok(Vec::new());
    };
    if crudas.is_empty() {
        return Ok(Vec::new());
    }
    let mut images = Vec::with_capacity(crudas.len());
    let mut refs = Vec::with_capacity(crudas.len());
    for (tx, indice, raw) in crudas {
        let Some(ki) = xmr_joint::wallet::key_image_from_raw(spend, raw) else {
            continue;
        };
        images.push(ki);
        refs.push((tx.clone(), *indice));
    }
    if images.is_empty() {
        return Ok(Vec::new());
    }
    let flags = xmr_joint::chain::key_images_spent(rpc, &images)
        .await
        .map_err(|e| e.to_string())?;
    Ok(refs
        .into_iter()
        .zip(flags)
        .filter_map(|((tx, i), gastada)| gastada.then_some((tx, i)))
        .collect())
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
    /// Salidas ya gastadas (fondeo o envío). El scan no las vuelve a sumar.
    gastadas: Vec<(String, u64)>,
}

struct LibroCaja {
    direccion: String,
    desde: usize,
    hasta: usize,
    listo: bool,
    retro: usize,
    entradas: Vec<Entrada>,
}

impl LibroCaja {
    fn nueva(direccion: &str) -> Self {
        Self {
            direccion: direccion.to_string(),
            desde: 0,
            hasta: 0,
            listo: false,
            retro: 0,
            entradas: Vec::new(),
        }
    }

    fn de_disco(d: LibroCajaDisco) -> Self {
        Self {
            direccion: d.direccion,
            desde: d.desde as usize,
            hasta: d.hasta as usize,
            listo: d.listo,
            retro: d.retro as usize,
            entradas: d
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
        }
    }
}

#[derive(Serialize, Deserialize)]
struct LibroCajaDisco {
    direccion: String,
    desde: u64,
    hasta: u64,
    listo: bool,
    retro: u64,
    entradas: Vec<EntradaDisco>,
}

impl Libro {
    fn vacio() -> Self {
        Self {
            direccion: String::new(),
            desde: 0,
            hasta: 0,
            listo: false,
            entradas: Vec::new(),
            gastadas: Vec::new(),
        }
    }

    fn nueva(direccion: &str) -> Self {
        Self {
            direccion: direccion.to_string(),
            desde: 0,
            hasta: 0,
            listo: false,
            entradas: Vec::new(),
            gastadas: Vec::new(),
        }
    }

    /// Arranca el scan en `altura` (inclusive) hacia la punta. No desde génesis.
    fn desde_altura(direccion: &str, altura: usize) -> Self {
        Self {
            direccion: direccion.to_string(),
            desde: altura,
            hasta: altura.saturating_sub(1),
            listo: true,
            entradas: Vec::new(),
            gastadas: Vec::new(),
        }
    }

    fn ya_gastada(&self, tx: &str, indice: u64) -> bool {
        self.gastadas.iter().any(|(t, i)| t == tx && *i == indice)
    }

    fn marcar_gastadas(&mut self, usadas: &[(String, u64)]) {
        for (tx, indice) in usadas {
            if !self.ya_gastada(tx, *indice) {
                self.gastadas.push((tx.clone(), *indice));
            }
        }
        self.entradas
            .retain(|e| !usadas.iter().any(|(tx, i)| e.tx == *tx && e.indice == *i));
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
    #[serde(default)]
    gastadas: Vec<EntradaRefDisco>,
}

#[derive(Serialize, Deserialize)]
struct EntradaDisco {
    altura: u64,
    monto: u64,
    tx: String,
    indice: u64,
    raw: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct EntradaRefDisco {
    tx: String,
    indice: u64,
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

fn libro_caja_path(obra: &str) -> PathBuf {
    xmr_dir().join(format!("caja-{obra}.json"))
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

/// Próximo rango inclusive. `true` si camina hacia atrás.
///
/// El primer vistazo es el lookback de siempre, de una vez. Lo que el usuario
/// pide después sale de a [`PASO_SCAN`], para no trabar el nodo con miles de bloques.
fn siguiente_trozo(
    tip: usize,
    desde: usize,
    hasta: usize,
    listo: bool,
    retro: usize,
) -> Option<(usize, usize, bool)> {
    if !listo {
        let inicio = tip.saturating_sub(LOOKBACK);
        return Some((inicio, tip, false));
    }
    if hasta < tip {
        let d = hasta.saturating_add(1);
        let h = d.saturating_add(PASO_SCAN - 1).min(tip);
        return Some((d, h, false));
    }
    if retro > 0 && desde > 0 {
        let n = retro.min(PASO_SCAN).min(desde);
        let h = desde - 1;
        let d = h + 1 - n;
        return Some((d, h, true));
    }
    None
}

/// Cuántos bloques suma un click, sin pasar [`MAX_HISTORIA`].
fn sumar_atras(tip: usize, desde: usize, listo: bool, retro: usize) -> usize {
    if listo && desde == 0 {
        return 0;
    }
    let profundidad = if listo {
        tip.saturating_sub(desde).saturating_add(retro)
    } else {
        LOOKBACK.saturating_add(retro)
    };
    MAX_HISTORIA.saturating_sub(profundidad).min(PASO_ATRAS_CAJA)
}

fn bloques_mirados(tip: Option<usize>, libro: &LibroCaja) -> usize {
    if libro.listo {
        let punta = tip.unwrap_or(libro.hasta);
        punta.saturating_sub(libro.desde).saturating_add(libro.retro)
    } else {
        LOOKBACK.saturating_add(libro.retro)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CoberturaCaja {
    Libre,
    Trabadas,
    Buscando,
    SinSaldo,
}

/// Dos salidas del monto exacto, con lo que la caja ya guardó. No llama al nodo.
fn cobertura_caja(
    entradas: &[(u64, usize)],
    tip: Option<usize>,
    listo: bool,
    _desde: usize,
    hasta: usize,
    retro: usize,
    amount: u64,
    cuantos: usize,
) -> CoberturaCaja {
    let Some(tip) = tip else {
        return CoberturaCaja::Buscando;
    };
    let mut alturas: Vec<usize> = entradas
        .iter()
        .filter(|(monto, _)| *monto == amount)
        .map(|(_, altura)| *altura)
        .collect();
    alturas.sort_unstable();
    if alturas.len() >= cuantos {
        let nuevas = &alturas[alturas.len() - cuantos..];
        let sueltas = nuevas.iter().all(|altura| tip >= altura.saturating_add(10));
        return if sueltas {
            CoberturaCaja::Libre
        } else {
            CoberturaCaja::Trabadas
        };
    }
    let cubierto = listo && hasta >= tip && retro == 0;
    if cubierto {
        CoberturaCaja::SinSaldo
    } else {
        CoberturaCaja::Buscando
    }
}

fn validar_share_meta(
    obra_en_perfil: bool,
    rol_mio: Option<&str>,
    rol_archivo: &str,
    red_ok: bool,
) -> Result<(), &'static str> {
    if !red_ok {
        return Err("codigo:share-red");
    }
    if !obra_en_perfil {
        return Err("codigo:share-obra");
    }
    match rol_mio {
        Some(rol) if rol == rol_archivo => Ok(()),
        _ => Err("codigo:share-rol"),
    }
}

fn misma_cuenta(a: &JointAccount, b: &JointAccount) -> bool {
    if a.role() != b.role() || a.obra_id() != b.obra_id() || a.address() != b.address() {
        return false;
    }
    if a.view_private_bytes() != b.view_private_bytes() {
        return false;
    }
    match (a.backup(), b.backup()) {
        (Ok(x), Ok(y)) => {
            x.context == y.context && x.threshold_keys.as_slice() == y.threshold_keys.as_slice()
        }
        _ => false,
    }
}

fn validar_share_existente(ya: bool, iguales: bool) -> Result<&'static str, &'static str> {
    if ya && !iguales {
        return Err("codigo:share-distinto");
    }
    if ya {
        Ok("codigo:share-igual")
    } else {
        Ok("codigo:share-nuevo")
    }
}

enum PlanSemilla {
    Nueva,
    Igual,
}

fn decidir_semilla(actual: Option<&str>, nueva: &str) -> Result<PlanSemilla, &'static str> {
    match actual {
        Some(a) if a == nueva => Ok(PlanSemilla::Igual),
        Some(_) => Err("codigo:semilla-otra"),
        None => Ok(PlanSemilla::Nueva),
    }
}

/// Escribe la semilla en `dir` si no hay otra. No toca el libro ni el perfil.
fn restaurar_semilla_en(dir: &Path, text: &str, actual: Option<&str>) -> Result<(String, bool), String> {
    let backup = SeedBackup::parse(text).map_err(|_| "codigo:semilla-archivo".to_string())?;
    if backup.net != Net::Stagenet {
        return Err("codigo:semilla-red".into());
    }
    let wallet = SingleWallet::restore(Net::Stagenet, backup.words.as_str())
        .map_err(|_| "codigo:semilla-archivo".to_string())?;
    if wallet.address() != backup.address {
        return Err("codigo:semilla-direccion".into());
    }
    let addr = wallet.address().to_string();
    let plan = decidir_semilla(actual, &addr).map_err(|e| e.to_string())?;
    let path = dir.join("semilla.txt");
    if path.exists() {
        match backup::read_secret_file(&path)
            .ok()
            .and_then(|t| SeedBackup::parse(&t).ok())
        {
            Some(prev) if prev.address == addr && prev.net == Net::Stagenet => return Ok((addr, true)),
            Some(_) => return Err("codigo:semilla-otra".into()),
            None => return Err("codigo:semilla-rota".into()),
        }
    }
    backup::write_secret_file(&path, &backup.to_text()).map_err(|e| e.to_string())?;
    Ok((addr, matches!(plan, PlanSemilla::Igual)))
}

async fn ver_txid(view: xmr_joint::ViewPair, txid: &str) -> Result<bool, String> {
    let rpc = chain::connect(&daemon_url()).await.map_err(|e| e.to_string())?;
    let tip = chain::tip(&rpc).await.map_err(|e| e.to_string())?;
    let from = tip.saturating_sub(LOOKBACK);
    let outs = chain::scan(&rpc, view, from, tip).await.map_err(|e| e.to_string())?;
    Ok(outs.iter().any(|o| hex::encode(o.transaction()) == txid))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obra_en_trato() -> (Obra, Persona, Persona) {
        use konstruado_core::{Aceptacion, Oferta};
        let m = Persona::nueva("felipe").unwrap();
        let c = Persona::nueva("caco").unwrap();
        let o = Oferta::publicar(m.clone(), "Super casa", 1_000, 500, vec![]).unwrap();
        let a = Aceptacion::de(&o, c.clone(), 500).unwrap();
        let mut obra = Obra::desde_oferta(o, a).unwrap();
        obra.estado = EstadoObra::EnMarcha;
        let p = &mut obra.partidas[0];
        p.estado = PartidaEstado::EnTrato;
        p.propuesto = Some(100);
        p.turno = Some(Rol::Mandante);
        p.encerrado_por = Some(m.clone());
        p.fondeo_txid = Some("4b2c".into());
        (obra, m, c)
    }

    /// La captura de 0.2.5: el pago ya salió y esperaba bloque, pero la ficha
    /// seguía ofreciendo "Aceptar 100% y pagar" y "Otro porcentaje".
    #[test]
    fn con_pago_en_curso_no_se_ofrece_aceptar_de_nuevo() {
        let (obra, m, c) = obra_en_trato();
        let libre = acciones_partida(&obra, 0, &m.id, &[]);
        assert!(libre.me_toca && libre.aceptar_pago && libre.contraofertar);
        assert!(!libre.salir_local);
        for linea in [Texto::Gastando, Texto::EsperandoPago("f15f".into()), Texto::BuscandoCaja] {
            let a = acciones_partida(&obra, 0, &m.id, &[linea.clone()]);
            assert!(a.me_toca, "{linea:?}");
            assert!(!a.aceptar_pago && !a.contraofertar, "{linea:?}");
        }
        let a = acciones_partida(&obra, 0, &m.id, &[Texto::EsperandoPago("f15f".into())]);
        assert_eq!(a.en_curso, EnCurso::PagoEnRed);
        // Si el pago se frenó, se puede volver a intentar.
        let a = acciones_partida(&obra, 0, &m.id, &[Texto::TrabadasCaja]);
        assert!(a.frenado && a.aceptar_pago);
        // Al contratista nunca le toca mientras el turno es del mandante.
        let a = acciones_partida(&obra, 0, &c.id, &[]);
        assert!(!a.me_toca && !a.aceptar_pago);
        // Un txid de pago visto sin Pagada todavía también cuenta como en curso.
        let mut o2 = obra.clone();
        o2.partidas[0].pago_txid = Some("f15f".into());
        let a = acciones_partida(&o2, 0, &m.id, &[]);
        assert_eq!(a.en_curso, EnCurso::PagoEnRed);
        assert!(!a.aceptar_pago);
    }

    #[test]
    fn fondeo_publicado_no_se_cancela() {
        let (mut obra, m, c) = obra_en_trato();
        let p = &mut obra.partidas[0];
        p.estado = PartidaEstado::Encerrando;
        p.turno = None;
        p.propuesto = None;
        p.fondeo_txid = None;
        // Propuso m; c confirma.
        let a = acciones_partida(&obra, 0, &c.id, &[]);
        assert!(a.confirmar_fondeo && a.no_encerrar && a.salir_local);
        let a = acciones_partida(&obra, 0, &c.id, &[Texto::Fondeando]);
        assert!(!a.confirmar_fondeo && a.no_encerrar);
        let a = acciones_partida(&obra, 0, &c.id, &[Texto::EsperandoFondeo("4b2c".into())]);
        assert!(!a.confirmar_fondeo && !a.no_encerrar && !a.salir_local);
        let a = acciones_partida(&obra, 0, &m.id, &[]);
        assert!(a.cancelar_propuesta && !a.confirmar_fondeo);
        let a = acciones_partida(&obra, 0, &m.id, &[Texto::EsperandoFondeo("4b2c".into())]);
        assert!(!a.cancelar_propuesta);
        let a = acciones_partida(&obra, 0, &m.id, &[Texto::Falla("rejected".into())]);
        assert!(a.empezar_fondeo_de_nuevo && a.no_encerrar);
    }

    #[test]
    fn pagada_o_cortada_no_tiene_acciones() {
        let (mut obra, m, _c) = obra_en_trato();
        obra.partidas[0].estado = PartidaEstado::Pagada;
        let a = acciones_partida(&obra, 0, &m.id, &[]);
        assert!(!a.aceptar_pago && !a.salir_local && !a.editar_texto);
        let (mut obra, m, _c) = obra_en_trato();
        obra.estado = EstadoObra::Abandonada;
        let a = acciones_partida(&obra, 0, &m.id, &[]);
        assert!(!a.aceptar_pago && !a.contraofertar && !a.salir_local);
        let extraño = Persona::nueva("otro").unwrap();
        let (obra, _m, _c) = obra_en_trato();
        assert!(!acciones_partida(&obra, 0, &extraño.id, &[]).me_toca);
    }

    #[test]
    fn pendiente_solo_la_activa_se_encierra() {
        let (mut obra, m, _c) = obra_en_trato();
        obra.partidas[0].estado = PartidaEstado::Pendiente;
        obra.partidas[0].fondeo_txid = None;
        let a0 = acciones_partida(&obra, 0, &m.id, &[]);
        assert!(a0.editar_texto && a0.proponer_encierre);
        let a1 = acciones_partida(&obra, 1, &m.id, &[]);
        assert!(a1.editar_texto && !a1.proponer_encierre);
    }


    #[test]
    fn puede_empezar_solo_con_freno() {
        assert!(puede_empezar_fondeo_de_nuevo(true, true));
        assert!(!puede_empezar_fondeo_de_nuevo(true, false));
        assert!(!puede_empezar_fondeo_de_nuevo(false, true));
        assert!(!puede_empezar_fondeo_de_nuevo(false, false));
    }

    #[test]
    fn humaniza_timeout_del_nodo() {
        let m = humanizar_error_cadena(
            "cadena: interface error (timeout reached: Elapsed(()))",
            true,
        );
        assert!(m.contains("timeout"), "{m}");
        assert!(!m.contains("Elapsed(())"), "{m}");
    }

    #[test]
    fn humaniza_reset_del_nodo() {
        let m = humanizar_error_cadena(
            "cadena: interface error (Hyper(hyper::Error(Io, Os { code: 104, kind: ConnectionReset, message: \"Connection reset by peer\" })))",
            true,
        );
        assert!(m.contains("Orbot"), "{m}");
        assert!(!m.contains("hyper::Error"), "{m}");
    }

    #[test]
    fn ruta_del_daemon_local_y_publica() {
        let lan = "http://192.168.1.83:38081";
        assert!(ruta_daemon(lan, VpnApp::Ninguna, true).contains("red local"));
        assert!(ruta_daemon(lan, VpnApp::Desconocida, true).contains("sin Tor"));
        assert!(ruta_daemon(lan, VpnApp::Captura, true).contains("VPN"));
        let publico = xmr_joint::STAGENET_DAEMON;
        assert!(ruta_daemon(publico, VpnApp::Captura, true).contains("Tor"));
        assert!(ruta_daemon(publico, VpnApp::Ninguna, true).contains("ve tu IP"));
        assert!(pista_vpn_local(true).contains("Elegir aplicaciones"));
    }

    #[test]
    fn humaniza_tx_rechazada_vacia() {
        let m = aviso_humano("transaction was rejected ()", true);
        assert!(m.contains("rechazó") || m.contains("rechazo"), "{m}");
        assert!(!m.contains("()"), "{m}");
    }

    #[test]
    fn libro_no_rescata_salidas_gastadas() {
        let mut libro = Libro::nueva("5test");
        libro.entradas.push(Entrada {
            altura: 10,
            monto: 500_000_000_000,
            tx: "aa".into(),
            indice: 0,
            raw: vec![1],
        });
        libro.entradas.push(Entrada {
            altura: 20,
            monto: 495_000_000_000,
            tx: "bb".into(),
            indice: 0,
            raw: vec![2],
        });
        libro.marcar_gastadas(&[("aa".into(), 0)]);
        assert_eq!(libro.entradas.len(), 1);
        assert_eq!(libro.entradas[0].tx, "bb");
        assert!(libro.ya_gastada("aa", 0));
        // Re-fundir la misma salida gastada no la revive.
        let motor_libro = libro;
        // simulate fundir skip via ya_gastada
        assert!(motor_libro.ya_gastada("aa", 0));
        assert!(!motor_libro.ya_gastada("bb", 0));
        let total: u64 = motor_libro.entradas.iter().map(|e| e.monto).sum();
        assert_eq!(total, 495_000_000_000);
        assert_eq!(fmt_xmr(total), "0.4950");
    }

    #[test]
    fn dos_mil_son_cero_cero_cuatro_xmr() {
        let pico = a_piconero(2_000).unwrap();
        assert_eq!(pico, 40_000_000_000);
        assert_eq!(fmt_xmr(pico), "0.0400");
        assert!(a_piconero(u64::MAX).is_none());
        assert_eq!(maximo_envio(pico).as_deref(), Some("0.0390"));
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

    #[test]
    fn la_partida_muestra_el_total_y_lo_de_cada_lado() {
        let s = saldo_partida(true, PartidaEstado::Encerrada, 50, true, "felipe", "Don").unwrap();
        assert!(s.estado.contains("Fondeada"));
        assert!(s.detalle.contains("Total en la caja: 0.0020 XMR"));
        assert!(s.detalle.contains("felipe aportó 0.0010 XMR"));
        assert!(s.detalle.contains("Don aportó 0.0010 XMR"));
        assert!(s.candado.is_some());

        let trato = saldo_partida(false, PartidaEstado::EnTrato, 50, true, "felipe", "Don").unwrap();
        assert!(trato.estado.contains("In deal"));
        assert!(trato.detalle.contains("Total in the box: 0.0020 XMR"));

        let pagada = saldo_partida(true, PartidaEstado::Pagada, 50, true, "felipe", "Don").unwrap();
        assert!(pagada.estado.contains("ya no tiene saldo"));
        assert!(pagada.detalle.contains("0.0020 XMR"));
        assert!(pagada.candado.is_none());

        let espera = saldo_partida(true, PartidaEstado::Encerrando, 50, false, "felipe", "Don").unwrap();
        assert!(espera.estado.contains("Todavía no hay saldo"));
        assert_eq!(
            saldo_corto(true, PartidaEstado::Encerrada, 50, true).as_deref(),
            Some("0.0020 XMR en la caja")
        );
        assert!(saldo_partida(true, PartidaEstado::Pendiente, 50, false, "a", "b").is_none());
    }

    #[test]
    fn la_semilla_no_pisa_otra_direccion() {
        let dir = std::env::temp_dir().join(format!("konstruado-semilla-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mal = restaurar_semilla_en(&dir, "no es una semilla", None).unwrap_err();
        assert_eq!(mal, "codigo:semilla-archivo");

        let (w1, words1) = SingleWallet::generate(&mut OsRng, Net::Stagenet).unwrap();
        let (w2, words2) = SingleWallet::generate(&mut OsRng, Net::Stagenet).unwrap();
        let una = SeedBackup {
            net: Net::Stagenet,
            address: w1.address().to_string(),
            height: Some(100),
            words: words1,
        };
        let otra = SeedBackup {
            net: Net::Stagenet,
            address: w2.address().to_string(),
            height: None,
            words: words2,
        };
        let (addr, igual) = restaurar_semilla_en(&dir, &una.to_text(), None).unwrap();
        assert_eq!(addr, w1.address());
        assert!(!igual);
        let (misma, ya) = restaurar_semilla_en(&dir, &una.to_text(), Some(w1.address())).unwrap();
        assert_eq!(misma, w1.address());
        assert!(ya);
        std::fs::remove_file(dir.join("semilla.txt")).unwrap();
        let (repuesta, igual) = restaurar_semilla_en(&dir, &una.to_text(), Some(w1.address())).unwrap();
        assert_eq!(repuesta, w1.address());
        assert!(igual);
        assert!(std::fs::read_to_string(dir.join("semilla.txt")).unwrap().contains(w1.address()));
        let pisar = restaurar_semilla_en(&dir, &otra.to_text(), Some(w1.address())).unwrap_err();
        assert_eq!(pisar, "codigo:semilla-otra");
        let otra_en_disco = restaurar_semilla_en(&dir, &otra.to_text(), None).unwrap_err();
        assert_eq!(otra_en_disco, "codigo:semilla-otra");
        let guardado = std::fs::read_to_string(dir.join("semilla.txt")).unwrap();
        assert!(guardado.contains(w1.address()));
        assert!(!guardado.contains(w2.address()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn el_share_ajeno_no_entra() {
        assert_eq!(
            validar_share_meta(false, Some("mandante"), "mandante", true),
            Err("codigo:share-obra")
        );
        assert_eq!(
            validar_share_meta(true, Some("mandante"), "contratista", true),
            Err("codigo:share-rol")
        );
        assert_eq!(
            validar_share_meta(true, None, "mandante", true),
            Err("codigo:share-rol")
        );
        assert_eq!(
            validar_share_meta(true, Some("mandante"), "mandante", false),
            Err("codigo:share-red")
        );
        assert!(validar_share_meta(true, Some("contratista"), "contratista", true).is_ok());
        assert_eq!(validar_share_existente(true, false), Err("codigo:share-distinto"));
        assert_eq!(validar_share_existente(true, true), Ok("codigo:share-igual"));
        assert_eq!(validar_share_existente(false, false), Ok("codigo:share-nuevo"));
        assert!(listo_humano("codigo:semilla-nueva", true).contains("no está en esas palabras"));
        assert!(aviso_humano("codigo:share-rol", false).contains("other side"));
    }

    #[test]
    fn la_caja_no_camina_todo_de_una() {
        let (desde, hasta, retro) = siguiente_trozo(1_000, 0, 0, false, 5_000).unwrap();
        assert!(!retro);
        assert_eq!(desde, 1_000 - LOOKBACK);
        assert_eq!(hasta, 1_000);

        let (desde, hasta, retro) = siguiente_trozo(1_000, 960, 1_000, true, 200).unwrap();
        assert!(retro);
        assert_eq!(hasta - desde + 1, PASO_SCAN);
        assert!(PASO_SCAN < 200);

        assert_eq!(siguiente_trozo(1_000, 960, 1_000, true, 0), None);
        assert_eq!(sumar_atras(10_000, 10_000 - LOOKBACK, true, 0), PASO_ATRAS_CAJA);
        assert_eq!(sumar_atras(10_000, 0, true, 0), 0);
        assert_eq!(sumar_atras(50_000, 50_000 - MAX_HISTORIA, true, 0), 0);

        assert_eq!(
            cobertura_caja(&[(5, 10), (5, 11)], Some(100), true, 60, 100, 0, 5, 2),
            CoberturaCaja::Libre
        );
        assert_eq!(
            cobertura_caja(&[(5, 95), (5, 96)], Some(100), true, 60, 100, 200, 5, 2),
            CoberturaCaja::Trabadas
        );
        assert_eq!(
            cobertura_caja(&[], Some(100), true, 60, 100, 0, 5, 2),
            CoberturaCaja::SinSaldo
        );
        assert_eq!(
            cobertura_caja(&[(5, 10)], Some(100), true, 60, 100, 50, 5, 2),
            CoberturaCaja::Buscando
        );
        assert_eq!(
            cobertura_caja(
                &[(5, 10), (5, 11), (5, 40), (5, 41)],
                Some(100),
                true,
                60,
                100,
                0,
                5,
                2
            ),
            CoberturaCaja::Libre
        );
        assert_eq!(
            cobertura_caja(
                &[(5, 10), (5, 11), (5, 95), (5, 96)],
                Some(100),
                true,
                60,
                100,
                0,
                5,
                2
            ),
            CoberturaCaja::Trabadas
        );
        assert!(!es_freno(&Texto::BuscandoCaja));
        assert!(es_freno(&Texto::SinSaldoCaja));
    }
}
