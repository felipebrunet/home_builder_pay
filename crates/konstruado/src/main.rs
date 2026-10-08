use konstruado_motor::{caja, i18n, persist, respaldo};
mod export;
mod help;
mod portapapeles;

use std::time::Duration;

use dioxus::prelude::*;
use konstruado_core::{
    asegurar_clave, fmt_monto, leer_usd, monto_pct, n_partidas, oferta_en_tablero, retirar_oferta,
    usd_editable, Aceptacion, EstadoObra, Moneda, Oferta, Obra, PartidaEstado, Persona, Rol,
    TextoLeido, MAX_NOTA,
};
use konstruado_motor::cotizacion;
use konstruado_net::{EstadoTor, Nodo, RED};
use i18n::Idioma;
const CSS: &str = include_str!("ui.css");

/// Precio USD/XMR (hora del último y error). Leerlo en un componente lo redibuja al cambiar.
static PRECIO: GlobalSignal<(i64, Option<String>)> = Signal::global(|| (0, None));

/// Un monto escrito en un campo, según la moneda de la obra u oferta.
fn leer_monto(moneda: Moneda, s: &str) -> u64 {
    match moneda {
        Moneda::Usd => leer_usd(s).unwrap_or(0),
        Moneda::Unidades => s.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0),
    }
}

/// Un monto para un campo editable.
fn monto_editable(moneda: Moneda, n: u64) -> String {
    match moneda {
        Moneda::Usd => usd_editable(n),
        Moneda::Unidades => n.to_string(),
    }
}

/// Un monto para mostrar: `USD 1.500` o las unidades de las obras viejas.
fn mm(moneda: Moneda, n: u64, lang: Idioma) -> String {
    fmt_monto(moneda, n, lang == Idioma::Es)
}

fn main() {
    preparar_grafica();
    // Un respaldo completo restaurado se aplica acá, antes de leer nada del disco.
    match respaldo::aplicar_pendiente(&persist::dir()) {
        Ok(true) => eprintln!("respaldo: restaurado"),
        Ok(false) => {}
        Err(e) => eprintln!("respaldo: {e}"),
    }
    if let Err(e) = persist::cargar_daemon_al_arrancar() {
        eprintln!("daemon: {e}");
    }
    let window = dioxus::desktop::WindowBuilder::new()
        .with_title(concat!("Konstruado ", env!("CARGO_PKG_VERSION")))
        .with_inner_size(dioxus::desktop::LogicalSize::new(1240.0, 800.0))
        .with_min_inner_size(dioxus::desktop::LogicalSize::new(420.0, 560.0));
    let mut cfg = dioxus::desktop::Config::new()
        .with_window(window)
        .with_menu(help::menu());
    // Ícono de ventana: pala y picota (assets/icon/konstruado.svg → generar.py).
    match dioxus::desktop::icon_from_memory(include_bytes!("../assets/konstruado-256.png")) {
        Ok(icono) => cfg = cfg.with_icon(icono),
        Err(e) => eprintln!("ícono: {e}"),
    }
    dioxus::LaunchBuilder::desktop().with_cfg(cfg).launch(App);
}

/// WSL has no real GPU. Mesa tries Zink, prints EGL noise, the window
/// still opens. Force software GL before WebKit starts.
fn preparar_grafica() {
    let wsl = std::fs::read_to_string("/proc/version")
        .map(|v| v.to_ascii_lowercase().contains("microsoft"))
        .unwrap_or(false);
    if !wsl {
        return;
    }
    for (k, v) in [
        ("WEBKIT_DISABLE_DMABUF_RENDERER", "1"),
        ("LIBGL_ALWAYS_SOFTWARE", "1"),
        ("GALLIUM_DRIVER", "llvmpipe"),
        ("EGL_LOG_LEVEL", "fatal"),
    ] {
        if std::env::var_os(k).is_none() {
            // Called from main before any other thread exists.
            unsafe { std::env::set_var(k, v) };
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Screen {
    Bienvenida,
    Tablero,
    Nueva,
    Oferta,
    Detalle,
    VerPartida,
    Cuenta,
    Billetera,
    Help,
}

#[derive(Clone)]
struct ClaveSec(String);

#[derive(Clone)]
struct SpendSec(String);

fn persistir(
    yo: Option<Persona>,
    rol: Option<Rol>,
    tema: String,
    idioma: String,
    clave_sec: String,
    spend_sec: String,
    n: &Nodo,
) {
    persist::guardar(&persist::EstadoDisco {
        yo,
        rol,
        ofertas: n.tablero(),
        obras: n.obras_todas(),
        presentes: n.presentes(),
        tema,
        idioma,
        clave_sec,
        spend_sec,
        obras_salidas: n.obras_salidas(),
        retiradas: n.retiradas(),
    });
}

fn publicar_trato(
    nodo: &Nodo,
    mut obra: Obra,
    quien: Option<Persona>,
    mut err: Signal<Option<String>>,
) -> bool {
    let sec = consume_context::<Signal<ClaveSec>>()().0;
    let Some(q) = quien else {
        err.set(Some(
            lang_now()
                .t(
                    "Entrá con tu nombre para cifrar la nota.",
                    "Sign in with your name to seal the note.",
                )
                .into(),
        ));
        return false;
    };
    match obra.preparar_para_red(&q.id, &q.clave_pub, &sec) {
        Ok(()) => {
            nodo.publicar_obra(obra);
            err.set(None);
            true
        }
        Err(e) => {
            err.set(Some(lang_now().error(&e)));
            false
        }
    }
}

fn lang_now() -> Idioma {
    consume_context::<Signal<Idioma>>()()
}

#[component]
fn App() -> Element {
    let guardado = use_hook(|| {
        let mut g = persist::cargar();
        if let Some(yo) = g.yo.as_mut() {
            let (sec, pubk) = asegurar_clave(&g.clave_sec, &yo.clave_pub);
            let changed = g.clave_sec != sec || yo.clave_pub != pubk;
            yo.clave_pub = pubk;
            g.clave_sec = sec;
            if changed {
                persist::guardar(&g);
            }
        }
        g
    });
    let clave_sec = use_context_provider(|| Signal::new(ClaveSec(guardado.clave_sec.clone())));
    let spend_sec = use_context_provider(|| Signal::new(SpendSec(guardado.spend_sec.clone())));
    let mut screen = use_signal(|| {
        if guardado.adentro() {
            Screen::Tablero
        } else {
            Screen::Bienvenida
        }
    });
    let nombre = use_signal(|| {
        guardado
            .yo
            .as_ref()
            .map(|p| p.nombre.clone())
            .unwrap_or_default()
    });
    let rol = use_signal(|| guardado.rol);
    let yo = use_signal(|| guardado.yo.clone());
    let mut red = use_signal(|| None::<Nodo>);
    let mut tor = use_signal(|| EstadoTor::Ausente);
    let mut peers = use_signal(|| 0usize);
    let mut presentes = use_signal(Vec::<Persona>::new);
    let mut ofertas = use_signal(Vec::<Oferta>::new);
    let mut obras = use_signal(Vec::<Obra>::new);
    let sel_oferta = use_signal(|| None::<Oferta>);
    let mut sel_obra = use_signal(|| None::<String>);
    let sel_partida = use_signal(|| None::<usize>);
    let mut err = use_signal(|| None::<String>);
    // Obras nuevas en dólares: USD 1000 de trabajo, USD 200 por partida.
    let trabajo = use_signal(|| "1000".to_string());
    let garantia = use_signal(|| "200".to_string());
    let obra_nom = use_signal(|| "Casa El Quisco".to_string());
    let garantia_acc = use_signal(|| "2000".to_string());
    let tema = use_signal(|| {
        if guardado.tema.is_empty() {
            "vivo".into()
        } else {
            guardado.tema.clone()
        }
    });
    let idioma = use_context_provider(|| Signal::new(Idioma::parse(&guardado.idioma)));
    let mut help_vista = use_signal(|| help::Vista::About);
    let caja_motor = use_hook(caja::Caja::nueva);
    let caja_ui = caja_motor.clone();
    let mut vista_caja = use_signal(caja::CajaVista::vacia);
    dioxus::desktop::use_muda_event_handler(move |evt| match evt.id().0.as_str() {
        help::ID_ABOUT => {
            help_vista.set(help::Vista::About);
            screen.set(Screen::Help);
        }
        help::ID_README => {
            help_vista.set(help::Vista::Readme);
            screen.set(Screen::Help);
        }
        _ => {}
    });

    use_future(move || {
        let caja_motor = caja_motor.clone();
        let ofertas0 = guardado.ofertas.clone();
        let mut obras0 = guardado.obras.clone();
        let salidas0 = guardado.obras_salidas.clone();
        let retiradas0 = guardado.retiradas.clone();
        let sec0 = guardado.clave_sec.clone();
        if let Some(p) = guardado.yo.clone() {
            for o in &mut obras0 {
                if o.participa(&p.id) {
                    let _ = o.preparar_para_red(&p.id, &p.clave_pub, &sec0);
                }
            }
        }
        let yo_id = guardado.yo.as_ref().map(|p| p.id.clone());
        let presentes0: Vec<Persona> = guardado
            .presentes
            .iter()
            .filter(|p| Some(&p.id) == yo_id.as_ref())
            .cloned()
            .collect();
        async move {
        match Nodo::arrancar().await {
            Ok(n) => {
                n.fijar_obras_salidas(salidas0);
                n.fijar_retiradas(retiradas0);
                n.hidratar(ofertas0, obras0, presentes0);
                if let Some(p) = yo() {
                    n.actualizar_yo(p);
                }
                red.set(Some(n.clone()));
                loop {
                    tor.set(n.estado_tor());
                    // Precio USD/XMR por el tor propio. Si tor no está (o falló), directo.
                    match (n.socks(), n.estado_tor()) {
                        (Some(s), _) => cotizacion::refrescar_en_fondo(Some(s)),
                        (None, EstadoTor::Ausente | EstadoTor::Fallo(_)) => cotizacion::refrescar_en_fondo(None),
                        _ => {}
                    }
                    let pq = (
                        cotizacion::ultima().map(|q| q.cuando).unwrap_or(0),
                        cotizacion::ultimo_error(),
                    );
                    if *PRECIO.peek() != pq {
                        *PRECIO.write() = pq;
                    }
                    if let Some(p) = yo() {
                        n.fijar_persona(&p.id);
                        n.anunciar(p.clone());
                        n.anunciar_persona();
                        let sec = clave_sec().0;
                        sellar_guardadas(&n, &p, &sec);
                        let hechos = caja_motor.tick(&n, &p, &n.obras());
                        aplicar_monero(&n, &p, &sec, &hechos);
                        vista_caja.set(caja_motor.vista());
                    }
                    if let Some(r) = rol() {
                        n.entrar_en_sala(r == Rol::Mandante);
                    }
                    peers.set(n.n_peers());
                    presentes.set(n.presentes());
                    ofertas.set(n.tablero());
                    obras.set(n.obras());
                    persistir(
                        yo(),
                        rol(),
                        tema(),
                        idioma().codigo().into(),
                        clave_sec().0,
                        spend_sec().0,
                        &n,
                    );
                    n.esperar(Duration::from_secs(1)).await;
                }
            }
            Err(e) => err.set(Some(format!("Red: {e}"))),
        }
        }
    });

    let adentro = yo().is_some();
    let quien = yo().map(|p| p.nombre).unwrap_or_default();
    let rol_txt = rol().map(|r| idioma().rol(r)).unwrap_or("");
    let lang = idioma();
    let mid = yo().map(|p| p.id).unwrap_or_default();
    let mut mis_obras: Vec<Obra> = obras()
        .into_iter()
        .filter(|o| obra_en_lista_activa(o.estado))
        .filter(|o| o.participa(&mid))
        .collect();
    mis_obras.sort_by(|a, b| b.actualizado.cmp(&a.actualizado).then(b.id.cmp(&a.id)));
    let en_billetera = screen() == Screen::Billetera;
    // Recordatorio de re-exportar tras crear/unirse a una obra o armar una caja.
    let (respaldo_falta, respaldo_linea) = if adentro {
        let est = estado_respaldo(&caja_ui, yo(), &obras());
        let falta = est.falta && est.hay_algo && (est.ultimo.is_some() || !mis_obras.is_empty());
        (falta, respaldo::texto_estado(&est, lang).1)
    } else {
        (false, String::new())
    };
    let en_obra = if matches!(screen(), Screen::Detalle | Screen::VerPartida) {
        sel_obra()
    } else {
        None
    };

    rsx! {
        style { {CSS} }
        div { class: "app", "data-theme": "{tema()}",
            header { class: "top",
                button {
                    class: "logo",
                    onclick: move |_| {
                        if adentro {
                            screen.set(Screen::Tablero);
                        }
                    },
                    "Konstruado"
                }
                LangSwitch {}
                if adentro {
                    button {
                        class: if en_billetera { "top-billetera on" } else { "top-billetera" },
                        onclick: move |_| screen.set(Screen::Billetera),
                        {lang.t("Billetera", "Wallet")}
                    }
                    button {
                        class: "quien",
                        onclick: move |_| screen.set(Screen::Cuenta),
                        "{quien} · {rol_txt}"
                    }
                }
            }
            div { class: "shell",
                if adentro {
                    aside { class: "side",
                        button {
                            class: if en_billetera { "side-wallet on" } else { "side-wallet" },
                            onclick: move |_| screen.set(Screen::Billetera),
                            {lang.t("Billetera", "Wallet")}
                        }
                        h2 { {lang.t("Mis obras", "My jobs")} }
                        div { class: "side-list",
                            for o in mis_obras {
                                button {
                                    class: if en_obra.as_deref() == Some(o.id.as_str()) { "side-item on" } else { "side-item" },
                                    onclick: move |_| {
                                        sel_obra.set(Some(o.id.clone()));
                                        screen.set(Screen::Detalle);
                                    },
                                    strong { "{o.nombre}" }
                                    span { class: chip_estado(o.estado), "{lang.label_estado(o.estado)}" }
                                }
                            }
                        }
                        if rol() == Some(Rol::Mandante) {
                            button {
                                class: "btn btn-primary",
                                onclick: move |_| screen.set(Screen::Nueva),
                                {lang.t("Publicar obra", "Post a job")}
                            }
                        }
                    }
                }
                main { class: "main",
                    if let Some(e) = err() {
                        div { class: "toast", role: "alert",
                            span { "{e}" }
                            button {
                                title: lang.t("Cerrar", "Close"),
                                onclick: move |_| err.set(None),
                                "×"
                            }
                        }
                    }
                    if adentro && matches!(screen(), Screen::Tablero | Screen::Detalle) && respaldo_falta {
                        div { class: "recordatorio",
                            p { class: "estado wait", "{respaldo_linea}" }
                            button {
                                class: "btn btn-ghost",
                                onclick: move |_| screen.set(Screen::Billetera),
                                {lang.t("Ir a Respaldos", "Go to Backups")}
                            }
                        }
                    }
                    match screen() {
                        Screen::Bienvenida => rsx! {
                            Bienvenida { nombre, rol, yo, red, screen, err }
                        },
                        Screen::Tablero => rsx! {
                            Tablero {
                                yo, rol, red, ofertas, obras, presentes, screen, sel_oferta, sel_obra,
                                sel_partida, tor, peers, garantia_acc, err
                            }
                        },
                        Screen::Nueva => rsx! {
                            Nueva {
                                yo, red, screen, err, trabajo, garantia, obra_nom
                            }
                        },
                        Screen::Oferta => rsx! {
                            VerOferta {
                                yo, red, sel_oferta, garantia_acc, screen, err
                            }
                        },
                        Screen::Detalle => rsx! {
                            Detalle { yo, red, obras, sel_obra, sel_partida, screen, err, caja: caja_ui.clone(), vista: vista_caja }
                        },
                        Screen::VerPartida => rsx! {
                            VerPartida { yo, red, obras, sel_obra, sel_partida, screen, err, caja: caja_ui.clone(), vista: vista_caja }
                        },
                        Screen::Cuenta => rsx! {
                            Cuenta { nombre, rol, yo, red, obras, screen, err, tema, caja: caja_ui.clone(), vista: vista_caja }
                        },
                        Screen::Billetera => rsx! {
                            Billetera { yo, obras, red, screen, err, caja: caja_ui.clone(), vista: vista_caja }
                        },
                        Screen::Help => rsx! {
                            help::Help { yo, screen, vista: help_vista() }
                        },
                    }
                }
            }
        }
    }
}

fn chip_estado(e: EstadoObra) -> &'static str {
    match e {
        EstadoObra::Publicada => "chip chip-off",
        EstadoObra::Contra => "chip chip-wait",
        EstadoObra::Rechazada => "chip chip-off",
        EstadoObra::Acordada => "chip chip-off",
        EstadoObra::EnMarcha => "chip chip-wait",
        EstadoObra::Abandonada => "chip chip-off",
        EstadoObra::Cerrada => "chip chip-ok",
    }
}

fn obra_en_curso(e: EstadoObra) -> bool {
    matches!(
        e,
        EstadoObra::Contra | EstadoObra::Acordada | EstadoObra::EnMarcha
    )
}

/// Mis obras / tablero activos: no terminales ni archivadas (archivadas ya salen de `obras()`).
fn obra_en_lista_activa(e: EstadoObra) -> bool {
    !matches!(
        e,
        EstadoObra::Rechazada | EstadoObra::Abandonada | EstadoObra::Cerrada
    )
}


fn chip_partida(e: PartidaEstado) -> &'static str {
    match e {
        PartidaEstado::Pendiente => "chip chip-off",
        PartidaEstado::Encerrando | PartidaEstado::Encerrada | PartidaEstado::EnTrato => {
            "chip chip-wait"
        }
        PartidaEstado::Pagada => "chip chip-ok",
    }
}


fn parse_pct(s: &str) -> u32 {
    s.chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}


fn exigir_sesion(
    red: Signal<Option<Nodo>>,
    yo: Signal<Option<Persona>>,
    obra: &Obra,
    mut err: Signal<Option<String>>,
) -> bool {
    let Some(nodo) = red() else {
        err.set(Some(lang_now().t("La red todavía no arrancó.", "The network has not started yet.").into()));
        return false;
    };
    let Some(p) = yo() else {
        err.set(Some(
            lang_now()
                .t(
                    "Falta tu nombre en esta ventana.",
                    "This window has no name yet.",
                )
                .into(),
        ));
        return false;
    };
    let otro = if p.id == obra.mandante.id {
        &obra.contratista.id
    } else {
        &obra.mandante.id
    };
    if matches!(nodo.estado_tor(), EstadoTor::Arrancando { .. }) && nodo.n_peers() == 0 {
        err.set(Some(
            lang_now().t(
                "Sincronizando el trato… esperá a que baje el estado del otro.",
                "Syncing the deal… wait for the other side's state.",
            ).into(),
        ));
        return false;
    }
    if nodo.trato_alineado(&p.id, otro) {
        true
    } else if nodo.sesion_viva(&p.id, otro) && !nodo.sync_reciente() {
        err.set(Some(
            lang_now().t(
                "Sincronizando el trato… todavía no bajó lo último del otro.",
                "Syncing the deal… the latest from the other side has not arrived yet.",
            ).into(),
        ));
        false
    } else {
        err.set(Some(
            lang_now().t(
                "El otro no está en línea. Tiene que tener Konstruado abierto.",
                "The other person is offline. They need Konstruado open.",
            ).into(),
        ));
        false
    }
}

fn sellar_guardadas(nodo: &Nodo, yo: &Persona, sec: &str) {
    caja::sellar_obras_guardadas(nodo, yo, sec);
}

fn aplicar_monero(nodo: &Nodo, yo: &Persona, sec: &str, hechos: &[caja::Hecho]) {
    caja::aplicar_hechos_monero(nodo, yo, sec, hechos);
}

/// True while waiting for a dump; not when the other is simply offline.
fn sincronizando_trato(
    red: Signal<Option<Nodo>>,
    yo: Signal<Option<Persona>>,
    obra: &Obra,
) -> bool {
    let Some(nodo) = red() else {
        return false;
    };
    let Some(p) = yo() else {
        return false;
    };
    let otro = if p.id == obra.mandante.id {
        &obra.contratista.id
    } else {
        &obra.mandante.id
    };
    if nodo.trato_alineado(&p.id, otro) {
        return false;
    }
    (matches!(nodo.estado_tor(), EstadoTor::Arrancando { .. }) && nodo.n_peers() == 0)
        || (nodo.sesion_viva(&p.id, otro) && !nodo.sync_reciente())
}

fn recorta_nota(s: String) -> String {
    if s.chars().count() <= MAX_NOTA {
        s
    } else {
        s.chars().take(MAX_NOTA).collect()
    }
}

#[derive(Clone)]
struct Aviso {
    texto: String,
    obra_id: String,
    es_oferta: bool,
    partida: Option<usize>,
}

fn avisos_para(
    mid: &str,
    _soy_m: bool,
    obras: &[Obra],
    _ofertas: &[Oferta],
    lang: Idioma,
    sec: &str,
) -> Vec<Aviso> {
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
            out.push(Aviso {
                texto: match lang {
                    Idioma::Es => format!(
                        "{}: {} propone garantía {}",
                        obra.nombre,
                        obra.contratista.nombre,
                        mm(obra.moneda, obra.garantia, lang)
                    ),
                    Idioma::En => format!(
                        "{}: {} proposes guarantee {}",
                        obra.nombre,
                        obra.contratista.nombre,
                        mm(obra.moneda, obra.garantia, lang)
                    ),
                },
                obra_id: obra.id.clone(),
                es_oferta: false,
                partida: None,
            });
        }
        if let Some(ex) = obra.extra.as_ref() {
            if ex.por.id != mid {
                let detalle = match obra.leer_extra(sec) {
                    TextoLeido::Plano(t) => t,
                    TextoLeido::Cerrado => lang.t("Texto cifrado", "Encrypted text").into(),
                };
                out.push(Aviso {
                    texto: match lang {
                        Idioma::Es => format!(
                            "{}: {} propone extra {} ({})",
                            obra.nombre, ex.por.nombre, detalle, mm(obra.moneda, ex.monto, lang)
                        ),
                        Idioma::En => format!(
                            "{}: {} proposes extra {} ({})",
                            obra.nombre, ex.por.nombre, detalle, mm(obra.moneda, ex.monto, lang)
                        ),
                    },
                    obra_id: obra.id.clone(),
                    es_oferta: false,
                    partida: None,
                });
            }
        }
        if let Some(cl) = obra.cierre.as_ref() {
            if cl.id != mid {
                out.push(Aviso {
                    texto: match lang {
                        Idioma::Es => format!("{}: {} quiere cortar el trato", obra.nombre, cl.nombre),
                        Idioma::En => format!("{}: {} wants to end the deal", obra.nombre, cl.nombre),
                    },
                    obra_id: obra.id.clone(),
                    es_oferta: false,
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
            let titulo = lang.titulo_partida(i, &p.detalle);
            if p.estado == PartidaEstado::Encerrando {
                if p.encerrado_por.as_ref().map(|q| q.id.as_str()) != Some(mid) {
                    out.push(Aviso {
                        texto: match lang {
                            Idioma::Es => format!(
                                "{} · {}: te toca confirmar el encierre",
                                obra.nombre, titulo
                            ),
                            Idioma::En => format!(
                                "{} · {}: your turn to confirm the lock",
                                obra.nombre, titulo
                            ),
                        },
                        obra_id: obra.id.clone(),
                        es_oferta: false,
                        partida: Some(i),
                    });
                }
            }
            if p.estado == PartidaEstado::EnTrato && p.turno == Some(mi_rol) {
                let pct = p.propuesto.unwrap_or(0);
                out.push(Aviso {
                    texto: match lang {
                    Idioma::Es => format!("{} · {}: te toca responder ({pct}%)", obra.nombre, titulo),
                    Idioma::En => format!("{} · {}: your turn to reply ({pct}%)", obra.nombre, titulo),
                },
                    obra_id: obra.id.clone(),
                    es_oferta: false,
                    partida: Some(i),
                });
            }
            if p.estado == PartidaEstado::Encerrada && mi_rol == Rol::Contratista {
                let quien = p
                    .encerrado_por
                    .as_ref()
                    .map(|q| q.nombre.as_str())
                    .unwrap_or(lang.t("el mandante", "the client"));
                out.push(Aviso {
                    texto: match lang {
                        Idioma::Es => format!(
                            "{} · {}: {quien} encerró, avisá cuando termines",
                            obra.nombre, titulo
                        ),
                        Idioma::En => format!(
                            "{} · {}: {quien} locked it, report when you finish",
                            obra.nombre, titulo
                        ),
                    },
                    obra_id: obra.id.clone(),
                    es_oferta: false,
                    partida: Some(i),
                });
            }
        }
    }
    out
}

fn otros_nombres(yo: Option<Persona>, presentes: Vec<Persona>, peers: usize) -> Vec<String> {
    if peers == 0 {
        return Vec::new();
    }
    let mid = yo.map(|p| p.id).unwrap_or_default();
    let mut names: Vec<String> = presentes
        .into_iter()
        .filter(|p| p.id != mid)
        .map(|p| p.nombre)
        .collect();
    names.sort();
    names.dedup();
    names
}

fn resumen_detalles(detalles: &[String]) -> Option<String> {
    let bits: Vec<&str> = detalles
        .iter()
        .filter(|d| !d.is_empty())
        .map(|d| d.as_str())
        .collect();
    if bits.is_empty() {
        None
    } else {
        Some(bits.join(" · "))
    }
}

#[component]
fn LangSwitch() -> Element {
    let mut idioma = use_context::<Signal<Idioma>>();
    rsx! {
        div { class: "lang",
            button {
                class: if idioma() == Idioma::Es { "lang-opt on" } else { "lang-opt" },
                onclick: move |_| idioma.set(Idioma::Es),
                "ES"
            }
            button {
                class: if idioma() == Idioma::En { "lang-opt on" } else { "lang-opt" },
                onclick: move |_| idioma.set(Idioma::En),
                "EN"
            }
        }
    }
}

#[component]
fn Bienvenida(
    nombre: Signal<String>,
    rol: Signal<Option<Rol>>,
    yo: Signal<Option<Persona>>,
    red: Signal<Option<Nodo>>,
    screen: Signal<Screen>,
    err: Signal<Option<String>>,
) -> Element {
    let lang = use_context::<Signal<Idioma>>()();
    // Primera vez sin cuenta: elegir entre crear una o traerla de un respaldo.
    let mut modo = use_signal(|| 0u8);
    if modo() == 0 {
        return rsx! {
            div { class: "pane narrow",
                h1 { {lang.t("La obra, con el dinero encerrado.", "The job, with the money locked.")} }
                p { class: "lead",
                    {lang.t("No te ves con la otra persona como en un chat. El mandante publica una obra. El contratista la ve en el tablero y acepta (o propone otra garantía).", "You do not see the other person like a chat. The client posts a job. The contractor sees it on the board and accepts (or proposes another guarantee).")}
                }
                div { class: "roles",
                    button {
                        class: "rol",
                        onclick: move |_| modo.set(1),
                        strong { {lang.t("Crear cuenta nueva", "Create a new account")} }
                        span { {lang.t("Elegís tu nombre y si pagás la obra o la construís.", "Choose your name and whether you pay for the job or build it.")} }
                    }
                    button {
                        class: "rol",
                        onclick: move |_| modo.set(2),
                        strong { {lang.t("Restaurar desde respaldo", "Restore from backup")} }
                        span { {lang.t("Traés todo del archivo cifrado: semilla, obras, cajas, nombre y rol.", "Bring everything back from the encrypted file: seed, jobs, escrows, name and role.")} }
                    }
                }
            }
        };
    }
    if modo() == 2 {
        return rsx! {
            div { class: "pane narrow",
                div { class: "migas",
                    button { onclick: move |_| modo.set(0), {lang.t("← Volver", "← Back")} }
                }
                h1 { {lang.t("Restaurar desde respaldo", "Restore from backup")} }
                p { class: "lead",
                    {lang.t(
                        "Después de restaurar, lo más nuevo del trato (notas, porcentajes, pagos) baja del otro por la sala cuando los dos están en línea.",
                        "After a restore, the newest deal progress (notes, percentages, payments) syncs from the other party through the room when both are online.",
                    )}
                }
                section { class: "panel",
                    RestaurarRespaldo { err }
                }
            }
        };
    }
    rsx! {
        div { class: "pane narrow",
            div { class: "migas",
                button { onclick: move |_| modo.set(0), {lang.t("← Volver", "← Back")} }
            }
            h1 { {lang.t("Crear cuenta nueva", "Create a new account")} }
            div { class: "paso", b { "1" } {lang.t("Tu nombre", "Your name")} }
            input {
                r#type: "text",
                placeholder: lang.t("Cómo te llamás", "Your name"),
                value: "{nombre}",
                oninput: move |e| nombre.set(e.value()),
            }
            div { class: "paso", b { "2" } {lang.t("¿Qué vas a hacer?", "What will you do?")} }
            div { class: "roles",
                button {
                    class: if rol() == Some(Rol::Mandante) { "rol on" } else { "rol" },
                    onclick: move |_| rol.set(Some(Rol::Mandante)),
                    strong { {lang.t("Pago la obra", "I pay for the job")} }
                    span { {lang.t("Mandante. Publicás el trabajo y la garantía. El otro la ve.", "Client. You post the job and the guarantee. The other person sees it.")} }
                }
                button {
                    class: if rol() == Some(Rol::Contratista) { "rol on" } else { "rol" },
                    onclick: move |_| rol.set(Some(Rol::Contratista)),
                    strong { {lang.t("La construyo", "I build it")} }
                    span { {lang.t("Contratista. Buscás lo publicado y aceptás, o proponés otra garantía.", "Contractor. You look at posted jobs and accept, or propose another guarantee.")} }
                }
            }
            button {
                class: "btn btn-primary",
                onclick: move |_| {
                    let Some(_) = rol() else {
                        err.set(Some(lang_now().t("Elegí si pagás la obra o la construís.", "Choose whether you pay for the job or build it.").into()));
                        return;
                    };
                    match Persona::nueva(nombre()) {
                        Ok(mut p) => {
                            let (sec, pubk) = konstruado_core::generar_clave();
                            p.clave_pub = pubk;
                            consume_context::<Signal<ClaveSec>>().set(ClaveSec(sec));
                            if let Some(nodo) = red() {
                                nodo.entrar_en_sala(rol() == Some(Rol::Mandante));
                            }
                            yo.set(Some(p));
                            err.set(None);
                            screen.set(Screen::Tablero);
                        }
                        Err(e) => err.set(Some(lang_now().error(&e))),
                    }
                },
                {lang.t("Entrar", "Enter")}
            }
        }
    }
}

#[component]
fn Cuenta(
    nombre: Signal<String>,
    rol: Signal<Option<Rol>>,
    yo: Signal<Option<Persona>>,
    red: Signal<Option<Nodo>>,
    obras: Signal<Vec<Obra>>,
    screen: Signal<Screen>,
    err: Signal<Option<String>>,
    tema: Signal<String>,
    caja: caja::Caja,
    vista: Signal<caja::CajaVista>,
) -> Element {
    let mut nom = use_signal(|| nombre());
    let mut rlocal = use_signal(|| rol());
    let mut tlocal = use_signal(|| tema());
    let mut idioma = use_context::<Signal<Idioma>>();
    let mut ilocal = use_signal(|| idioma());
    let lang = idioma();
    let caja_crear = caja.clone();
    let caja_daemon = caja.clone();
    let mut daemon_url = use_signal(|| {
        if xmr_joint::daemon_es_defecto() {
            String::new()
        } else {
            xmr_joint::daemon_url()
        }
    });
    let mut daemon_aviso = use_signal(|| Option::<String>::None);
    let mut daemon_probando = use_signal(|| false);
    rsx! {
        div { class: "pane",
            h1 { {lang.t("Tu cuenta", "Your account")} }
            p { class: "sub",
                {lang.t("El nombre y el rol se pueden cambiar. Las obras no se borran. El mandante abre la sala; el contratista solo busca.", "Name and role can be changed. Jobs are not deleted. The client opens the room; the contractor only looks.")}
            }
            div { class: "cols parejas",
            div { class: "col",
            section { class: "panel",
            h2 { {lang.t("Perfil", "Profile")} }
            label { class: "et", {lang.t("NOMBRE", "NAME")} }
            input {
                r#type: "text",
                value: "{nom}",
                oninput: move |e| nom.set(e.value()),
            }
            label { class: "et", {lang.t("¿QUÉ VAS A HACER?", "WHAT WILL YOU DO?")} }
            div { class: "roles lado",
                button {
                    class: if rlocal() == Some(Rol::Mandante) { "rol on" } else { "rol" },
                    onclick: move |_| rlocal.set(Some(Rol::Mandante)),
                    strong { {lang.t("Pago la obra", "I pay for the job")} }
                    span { {lang.t("Mandante. Publicás y abrís la sala.", "Client. You post and open the room.")} }
                }
                button {
                    class: if rlocal() == Some(Rol::Contratista) { "rol on" } else { "rol" },
                    onclick: move |_| rlocal.set(Some(Rol::Contratista)),
                    strong { {lang.t("La construyo", "I build it")} }
                    span { {lang.t("Contratista. Buscás lo publicado. No abrís sala.", "Contractor. You look at posted jobs. You do not open a room.")} }
                }
            }
            }
            section { class: "panel",
            h2 { {lang.t("Apariencia e idioma", "Look and language")} }
            label { class: "et", {lang.t("TEMA", "THEME")} }
            div { class: "roles lado",
                button {
                    class: if tlocal() == "vivo" { "rol on" } else { "rol" },
                    onclick: move |_| tlocal.set("vivo".into()),
                    strong { {lang.t("Vivo", "Vivid")} }
                    span { {lang.t("Arcilla, crema y contraste. El de siempre más color.", "Clay, cream and contrast. The usual, more color.")} }
                }
                button {
                    class: if tlocal() == "calma" { "rol on" } else { "rol" },
                    onclick: move |_| tlocal.set("calma".into()),
                    strong { {lang.t("Calma", "Calm")} }
                    span { {lang.t("Gris claro, menos tinta. El anterior.", "Light gray, less ink. The previous look.")} }
                }
            }
            label { class: "et", {lang.t("IDIOMA", "LANGUAGE")} }
            div { class: "roles lado",
                button {
                    class: if ilocal() == Idioma::Es { "rol on" } else { "rol" },
                    onclick: move |_| ilocal.set(Idioma::Es),
                    strong { "Español" }
                    span { {lang.t("El idioma de la ventana. El trato es el mismo.", "The language of this window. The deal is the same.")} }
                }
                button {
                    class: if ilocal() == Idioma::En { "rol on" } else { "rol" },
                    onclick: move |_| ilocal.set(Idioma::En),
                    strong { "English" }
                    span { {lang.t("English. The deal does not change.", "English. The deal does not change.")} }
                }
            }
            }
            button {
                class: "btn btn-primary",
                onclick: move |_| {
                    let Some(mut p) = yo() else { return };
                    let Some(r) = rlocal() else {
                        err.set(Some(lang_now().t("Elegí si pagás la obra o la construís.", "Choose whether you pay for the job or build it.").into()));
                        return;
                    };
                    match p.renombrar(nom()) {
                        Ok(()) => {
                            tema.set(tlocal());
                            idioma.set(ilocal());
                            if let Some(nodo) = red() {
                                nodo.actualizar_yo(p.clone());
                                nodo.entrar_en_sala(r == Rol::Mandante);
                                persistir(
                                    Some(p.clone()),
                                    Some(r),
                                    tlocal(),
                                    ilocal().codigo().into(),
                                    consume_context::<Signal<ClaveSec>>()().0,
                                    consume_context::<Signal<SpendSec>>()().0,
                                    &nodo,
                                );
                            }
                            nombre.set(nom());
                            rol.set(Some(r));
                            yo.set(Some(p));
                            err.set(None);
                            screen.set(Screen::Tablero);
                        }
                        Err(e) => err.set(Some(lang_now().error(&e))),
                    }
                },
                {lang.t("Guardar perfil y apariencia", "Save profile and look")}
            }
            }
            div { class: "col",
            section { class: "panel",
            h2 { {lang.t("Nodo de stagenet", "Stagenet node")} }
            p { class: "estado info",
                {
                    let d = vista().daemon.clone();
                    if xmr_joint::daemon_es_defecto() {
                        match lang {
                            Idioma::Es => format!("Activo (público): {d}"),
                            Idioma::En => format!("Active (public): {d}"),
                        }
                    } else {
                        match lang {
                            Idioma::Es => format!("Activo (propio): {d}"),
                            Idioma::En => format!("Active (custom): {d}"),
                        }
                    }
                }
            }
            p { class: "help",
                {lang.t(
                    "Guardar fija el nodo para scan, saldo, fondeo y pago. «Usar por defecto» vuelve al público.",
                    "Save sets the node for scan, balance, funding and payout. Use default goes back to the public daemon.",
                )}
            }
            label { class: "et", {lang.t("URL DEL NODO", "NODE URL")} }
            input {
                r#type: "text",
                value: "{daemon_url}",
                placeholder: "{xmr_joint::STAGENET_DAEMON}",
                oninput: move |e| daemon_url.set(e.value()),
            }
            div { class: "acciones",
                button {
                    class: "btn btn-primary btn-sm",
                    onclick: {
                        let caja_daemon = caja_daemon.clone();
                        move |_| {
                            match persist::fijar_daemon_persistido(Some(&daemon_url())) {
                                Ok(u) => {
                                    daemon_url.set(if xmr_joint::daemon_es_defecto() {
                                        String::new()
                                    } else {
                                        u.clone()
                                    });
                                    daemon_aviso.set(Some(match lang_now() {
                                        Idioma::Es => format!("Nodo guardado: {u}"),
                                        Idioma::En => format!("Node saved: {u}"),
                                    }));
                                    caja_daemon.pedir_actualizacion();
                                    err.set(None);
                                }
                                Err(e) => {
                                    daemon_aviso.set(None);
                                    err.set(Some(e));
                                }
                            }
                        }
                    },
                    {lang.t("Guardar nodo", "Save node")}
                }
                button {
                    class: "btn btn-ghost btn-sm",
                    onclick: {
                        let caja_daemon = caja_daemon.clone();
                        move |_| {
                            match persist::fijar_daemon_persistido(None) {
                                Ok(u) => {
                                    daemon_url.set(String::new());
                                    daemon_aviso.set(Some(match lang_now() {
                                        Idioma::Es => format!("Volví al nodo público: {u}"),
                                        Idioma::En => format!("Back to public node: {u}"),
                                    }));
                                    caja_daemon.pedir_actualizacion();
                                    err.set(None);
                                }
                                Err(e) => err.set(Some(e)),
                            }
                        }
                    },
                    {lang.t("Usar por defecto", "Use default")}
                }
                button {
                    class: "btn btn-ghost btn-sm",
                    disabled: daemon_probando(),
                    onclick: move |_| {
                        if daemon_probando() {
                            return;
                        }
                        daemon_probando.set(true);
                        let es = lang_now() == Idioma::Es;
                        spawn(async move {
                            let r = caja::probar_daemon(es).await;
                            daemon_aviso.set(Some(r.mensaje));
                            daemon_probando.set(false);
                        });
                    },
                    if daemon_probando() {
                        {lang.t("Probando RPC…", "Testing RPC…")}
                    } else {
                        {lang.t("Probar RPC del nodo", "Test node RPC")}
                    }
                }
            }
            if let Some(a) = daemon_aviso() {
                p { class: "estado info", "{a}" }
            }
            if let Some(tip) = vista().tip {
                dl { class: "datos",
                    dt { {lang.t("Punta del nodo", "Node tip")} }
                    dd { "{tip}" }
                }
            }
            }
            section { class: "panel",
            h2 { {lang.t("Billetera", "Wallet")} }
            p { class: "help", "{caja::estado_cotizacion(matches!(lang, Idioma::Es))}" }
            if let Some(addr) = vista().personal.clone() {
                span { class: "mono caja", "{addr}" }
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| screen.set(Screen::Billetera),
                    {lang.t("Abrir billetera", "Open wallet")}
                }
            } else {
                button {
                    class: "btn btn-primary",
                    onclick: move |_| {
                        match caja_crear.crear_semilla() {
                            Ok(_) => err.set(None),
                            Err(e) => err.set(Some(e)),
                        }
                    },
                    {lang.t("Crear billetera de stagenet", "Create stagenet wallet")}
                }
            }
            }
            details { class: "plegable", ontoggle: move |_| ocultar_semilla(),
                summary { {lang.t("Respaldos y recuperación", "Backups and recovery")} }
                div { class: "cuerpo",
                    SeccionRespaldos { caja: caja.clone(), yo, obras, red, vista, err }
                }
            }
            }
            }
        }
    }
}

#[component]
fn Billetera(
    yo: Signal<Option<Persona>>,
    obras: Signal<Vec<Obra>>,
    red: Signal<Option<Nodo>>,
    screen: Signal<Screen>,
    mut err: Signal<Option<String>>,
    caja: caja::Caja,
    vista: Signal<caja::CajaVista>,
) -> Element {
    let mut destino = use_signal(String::new);
    let mut monto = use_signal(String::new);
    let lang = use_context::<Signal<Idioma>>()();
    let es = matches!(lang, Idioma::Es);
    let v = vista();
    let b = v.billetera.clone();
    let addr = v.personal.clone();
    let caja_envio = caja.clone();
    let caja_act = caja.clone();
    let caja_act2 = caja.clone();
    let caja_atras = caja.clone();
    let caja_crear = caja.clone();
    // Estado de la billetera en una sola línea de alto fijo: no corre el contenido.
    let (punto, linea) = barra_billetera(&b, addr.is_some(), v.tip, lang);
    rsx! {
        div { class: "pane",
            div { class: "cabeza",
                h1 { {lang.t("Billetera", "Wallet")} }
                div { class: "barra-estado derecha", style: "flex: 1 1 360px; max-width: 620px;",
                    span { class: "{punto}" }
                    span { class: "txt", title: "{linea}", "{linea}" }
                    if addr.is_some() {
                        button {
                            class: "btn btn-ghost btn-sm",
                            style: "width: auto; min-height: 28px; padding: 2px 10px; font-size: 13px;",
                            disabled: b.buscando,
                            onclick: move |_| caja_act2.pedir_actualizacion(),
                            {lang.t("Actualizar", "Refresh")}
                        }
                    }
                }
            }
            p { class: "sub",
                {lang.t(
                    "Tu Monero personal de stagenet. La caja de una obra es otra dirección, de las dos personas.",
                    "Your personal stagenet Monero. A job's box is a different address, shared by both people.",
                )}
            }
            if let Some(addr) = addr {
                div { class: "cols",
                    div { class: "col",
                        section { class: "panel",
                            h2 { {lang.t("Saldo", "Balance")} }
                            p { class: "saldo",
                                "{caja::fmt_xmr(b.total)}"
                                small { "XMR" }
                            }
                            dl { class: "datos",
                                dt { {lang.t("Libre", "Unlocked")} }
                                dd { "{caja::fmt_xmr(b.libre)} XMR" }
                                dt { {lang.t("Trabado", "Locked")} }
                                dd { "{caja::fmt_xmr(b.trabado)} XMR" }
                                dt { {lang.t("Mirado", "Scanned")} }
                                dd {
                                    {match (b.desde, b.hasta) {
                                        (Some(d), Some(h)) => match lang {
                                            Idioma::Es => format!("bloques {d} – {h}"),
                                            Idioma::En => format!("blocks {d} – {h}"),
                                        },
                                        _ => lang.t("todavía nada", "nothing yet").to_string(),
                                    }}
                                }
                                dt { {lang.t("Punta del nodo", "Node tip")} }
                                dd { {v.tip.map(|t| t.to_string()).unwrap_or_else(|| "—".into())} }
                                dt { {lang.t("Nodo", "Node")} }
                                dd { span { class: "mono", "{v.daemon}" } }
                            }
                            if let Some(aviso) = b.aviso.clone() {
                                p { class: "estado err", "{caja::aviso_humano(&aviso, es)}" }
                            }
                            if let Some(tx) = b.ultimo.clone() {
                                p { class: "estado info",
                                    span {
                                        {match lang {
                                            Idioma::Es => format!(
                                                "Último envío: fee {} XMR, cambio {} XMR (vuelve en el próximo bloque). ",
                                                caja::fmt_xmr(b.ultimo_fee.unwrap_or(0)),
                                                caja::fmt_xmr(b.ultimo_cambio.unwrap_or(0)),
                                            ),
                                            Idioma::En => format!(
                                                "Last send: fee {} XMR, change {} XMR (returns in the next block). ",
                                                caja::fmt_xmr(b.ultimo_fee.unwrap_or(0)),
                                                caja::fmt_xmr(b.ultimo_cambio.unwrap_or(0)),
                                            ),
                                        }}
                                        span { class: "mono", "{tx}" }
                                    }
                                }
                            }
                            p { class: "help",
                                {lang.t(
                                    "El scan arranca 40 bloques atrás. Si el faucet es más viejo, mirá más atrás. El candado de ~10 bloques es aparte.",
                                    "The scan starts 40 blocks back. If the faucet is older, scan further back. The ~10 block lock is separate.",
                                )}
                            }
                            div { class: "acciones",
                                button {
                                    class: "btn btn-ghost btn-sm",
                                    onclick: move |_| caja_act.pedir_actualizacion(),
                                    {lang.t("Actualizar saldo", "Refresh balance")}
                                }
                                button {
                                    class: "btn btn-ghost btn-sm",
                                    onclick: move |_| caja_atras.pedir_atras(),
                                    {lang.t("Mirar 200 bloques más atrás", "Scan 200 blocks further back")}
                                }
                            }
                        }
                        section { class: "panel",
                            h2 { {lang.t("Entradas", "Outputs")} }
                            if b.movs.is_empty() {
                                p { class: "help", {lang.t("Todavía no hay entradas en los bloques mirados.", "No outputs in the scanned blocks yet.")} }
                            } else {
                                div { class: "movs",
                                    for mov in b.movs.iter() {
                                        div { class: "mov",
                                            b { "{caja::fmt_xmr(mov.monto)} XMR" }
                                            span { class: if mov.libre { "chip chip-ok" } else { "chip chip-wait" },
                                                {match lang {
                                                    Idioma::Es => format!("bloque {} · {}", mov.altura, if mov.libre { "libre" } else { "trabado" }),
                                                    Idioma::En => format!("block {} · {}", mov.altura, if mov.libre { "unlocked" } else { "locked" }),
                                                }}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    div { class: "col",
                        section { class: "panel",
                            h2 { {lang.t("Recibir", "Receive")} }
                            input {
                                class: "addr",
                                r#type: "text",
                                readonly: true,
                                value: "{addr}",
                            }
                            p { class: "help",
                                {lang.t(
                                    "Seleccioná la dirección y copiala. El scan no ve monedas más viejas que lo ya mirado.",
                                    "Select the address and copy it. The scan misses coins older than what was already scanned.",
                                )}
                            }
                        }
                        section { class: "panel",
                            h2 { {lang.t("Enviar", "Send")} }
                            label { class: "et", {lang.t("DESTINO", "DESTINATION")} }
                            input {
                                r#type: "text",
                                value: "{destino}",
                                placeholder: lang.t("Dirección de stagenet", "Stagenet address"),
                                oninput: move |e| destino.set(e.value()),
                            }
                            label { class: "et", {lang.t("MONTO EN XMR", "AMOUNT IN XMR")} }
                            input {
                                r#type: "text",
                                value: "{monto}",
                                placeholder: "0.04",
                                oninput: move |e| monto.set(e.value()),
                            }
                            p { class: "help",
                                {caja::ayuda_envio(lang == Idioma::Es)}
                            }
                            div { class: "acciones",
                                button {
                                    class: "btn btn-ghost",
                                    onclick: move |_| {
                                        match caja::maximo_envio(vista().billetera.libre) {
                                            Some(texto) => {
                                                monto.set(texto);
                                                err.set(None);
                                            }
                                            None => err.set(Some(lang_now().t(
                                                "Todavía no hay saldo libre para enviar.",
                                                "There is no unlocked balance to send yet.",
                                            ).into())),
                                        }
                                    },
                                    {lang.t("Usar el máximo", "Use the maximum")}
                                }
                                button {
                                    class: "btn btn-primary",
                                    disabled: b.enviando,
                                    onclick: move |_| {
                                        match caja_envio.pedir_envio(&destino(), &monto()) {
                                            Ok(()) => err.set(None),
                                            Err(e) => err.set(Some(caja::aviso_humano(&e, lang_now() == Idioma::Es))),
                                        }
                                    },
                                    if b.enviando { {lang.t("Enviando…", "Sending…")} } else { {lang.t("Enviar", "Send")} }
                                }
                            }
                        }
                        details { class: "plegable", ontoggle: move |_| ocultar_semilla(), open: true,
                            summary { {lang.t("Respaldos y recuperación", "Backups and recovery")} }
                            div { class: "cuerpo",
                                SeccionRespaldos { caja: caja.clone(), yo, obras, red, vista, err }
                            }
                        }
                    }
                }
            } else {
                div { class: "cols",
                    div { class: "col",
                        section { class: "panel",
                            h2 { {lang.t("Crear billetera", "Create wallet")} }
                            p { class: "help", "{caja::nota_precio_stagenet(es)}" }
                            p { class: "help",
                                {lang.t(
                                    "Todavía no hay semilla en este equipo. Se crean 25 palabras nuevas y quedan en la carpeta de datos.",
                                    "This machine has no seed yet. This creates 25 new words and keeps them in the data folder.",
                                )}
                            }
                            button {
                                class: "btn btn-primary",
                                onclick: move |_| {
                                    match caja_crear.crear_semilla() {
                                        Ok(_) => {
                                            err.set(None);
                                            vista.set(caja_crear.vista());
                                        }
                                        Err(e) => err.set(Some(e)),
                                    }
                                },
                                {lang.t("Crear billetera de stagenet", "Create stagenet wallet")}
                            }
                        }
                    }
                    div { class: "col",
                        details { class: "plegable", ontoggle: move |_| ocultar_semilla(), open: true,
                            summary { {lang.t("Respaldos y recuperación", "Backups and recovery")} }
                            div { class: "cuerpo",
                                SeccionRespaldos { caja: caja.clone(), yo, obras, red, vista, err }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Punto y texto de la barra de estado de la billetera (regla compartida en caja.rs).
fn barra_billetera(
    b: &caja::BilleteraVista,
    hay: bool,
    tip: Option<usize>,
    lang: Idioma,
) -> (&'static str, String) {
    let (tono, texto) = caja::estado_billetera(b, hay, tip, matches!(lang, Idioma::Es));
    let punto = match tono {
        caja::Tono::Ok => "punto",
        caja::Tono::Espera => "punto wait",
        caja::Tono::Error => "punto err",
        caja::Tono::Apagado => "punto off",
    };
    (punto, texto)
}

#[component]
fn Tablero(
    yo: Signal<Option<Persona>>,
    rol: Signal<Option<Rol>>,
    red: Signal<Option<Nodo>>,
    ofertas: Signal<Vec<Oferta>>,
    obras: Signal<Vec<Obra>>,
    presentes: Signal<Vec<Persona>>,
    screen: Signal<Screen>,
    sel_oferta: Signal<Option<Oferta>>,
    sel_obra: Signal<Option<String>>,
    sel_partida: Signal<Option<usize>>,
    tor: Signal<EstadoTor>,
    peers: Signal<usize>,
    garantia_acc: Signal<String>,
    err: Signal<Option<String>>,
) -> Element {
    let lang = use_context::<Signal<Idioma>>()();
    let sec = use_context::<Signal<ClaveSec>>()().0;
    let mut buscando = use_signal(|| false);
    // Qué oferta propia está pidiendo confirmación para quitarse.
    let mut confirma_quitar = use_signal(|| None::<String>);
    let mid = yo().map(|p| p.id).unwrap_or_default();
    // Incluye archivadas para que no reaparezca la oferta al archivar la obra.
    let todas = red()
        .as_ref()
        .map(|n| n.obras_todas())
        .unwrap_or_else(|| obras());
    let mut mias: Vec<Oferta> = ofertas()
        .into_iter()
        .filter(|o| o.mandante.id == mid && oferta_en_tablero(&o.id, &todas))
        .collect();
    mias.sort_by(|a, b| b.actualizado.cmp(&a.actualizado).then(b.id.cmp(&a.id)));
    let mut ajenas: Vec<Oferta> = ofertas()
        .into_iter()
        .filter(|o| o.mandante.id != mid && oferta_en_tablero(&o.id, &todas))
        .collect();
    ajenas.sort_by(|a, b| b.actualizado.cmp(&a.actualizado).then(b.id.cmp(&a.id)));
    let mut mis_obras: Vec<Obra> = obras()
        .into_iter()
        .filter(|o| obra_en_lista_activa(o.estado))
        .filter(|o| o.participa(&mid))
        .collect();
    mis_obras.sort_by(|a, b| b.actualizado.cmp(&a.actualizado).then(b.id.cmp(&a.id)));
    let en_curso: Vec<Obra> = mis_obras
        .iter()
        .filter(|o| obra_en_curso(o.estado))
        .cloned()
        .collect();
    let otros = otros_nombres(yo(), presentes(), peers());
    let status = lang.linea_red(tor(), peers(), &otros);
    let punto_red = match tor() {
        EstadoTor::Arrancando { .. } => "punto wait",
        _ if peers() == 0 => "punto off",
        _ => "punto",
    };
    let soy_m = rol() == Some(Rol::Mandante);
    let sin_ajenas = ajenas.is_empty();
    let sin_mias = mias.is_empty();
    let avisos = avisos_para(&mid, soy_m, &mis_obras, &ajenas, lang, &sec);
    let hint_contratista = if otros.is_empty() {
        lang.t(
            "No hay avisos. Don Dinero tiene que publicar, y vos podés tocar Buscar ofertas.",
            "No notices. The client has to post, and you can tap Search jobs.",
        ).to_string()
    } else {
        match lang {
            Idioma::Es => format!(
                "{} está en la red. Si no ves el aviso, tocá Buscar ofertas.",
                otros.join(", ")
            ),
            Idioma::En => format!(
                "{} is on the network. If you do not see the notice, tap Search jobs.",
                otros.join(", ")
            ),
        }
    };
    rsx! {
        div { class: "pane",
            div { class: "cabeza",
                h1 { {lang.t("Tablero", "Board")} }
                div { class: "barra-estado derecha", style: "flex: 1 1 360px; max-width: 620px;",
                    span { class: "{punto_red}" }
                    span { class: "txt", title: "{status}",
                        {lang.t("Red", "Net")} " " span { class: "mono", "{RED}" } " · {status}"
                    }
                }
            }
            if peers() == 0 {
                p { class: "help",
                    match tor() {
                        EstadoTor::Arrancando { .. } => lang.t("Tor está subiendo. El mandante abre la sala; el contratista solo busca.", "Tor is coming up. The client opens the room; the contractor only looks."),
                        _ => lang.t("Nadie más todavía. En la misma PC, un segundo cargo run se engancha solo. En otra máquina, Don Dinero abre la sala y Chasquilla busca.", "Nobody else yet. On the same PC, a second cargo run joins on its own. On another machine, the client opens the room and the contractor searches."),
                    }
                }
            }
            div { class: "cols",
                div { class: "col",
                    if !avisos.is_empty() {
                        section { class: "panel foco",
                            h2 { {lang.t("Te toca", "Your turn")} }
                            div { class: "stack",
                                for a in avisos {
                                    button {
                                        class: "card aviso",
                                        onclick: move |_| {
                                            if a.es_oferta {
                                                if let Some(o) = ofertas().into_iter().find(|o| o.id == a.obra_id) {
                                                    garantia_acc.set(monto_editable(o.moneda, o.garantia_sugerida));
                                                    sel_oferta.set(Some(o));
                                                    screen.set(Screen::Oferta);
                                                }
                                            } else if let Some(i) = a.partida {
                                                sel_obra.set(Some(a.obra_id.clone()));
                                                sel_partida.set(Some(i));
                                                screen.set(Screen::VerPartida);
                                            } else {
                                                sel_obra.set(Some(a.obra_id.clone()));
                                                screen.set(Screen::Detalle);
                                            }
                                        },
                                        div { class: "card-h",
                                            strong { "{a.texto}" }
                                            span { class: "chip chip-info", {lang.t("Te toca", "Your turn")} }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if soy_m {
                        section { class: "panel",
                            h2 { {lang.t("Mis ofertas publicadas", "My posted offers")} }
                            if sin_mias {
                                p { class: "help", {lang.t("No hay ofertas tuyas esperando contratista.", "None of your offers is waiting for a contractor.")} }
                            } else {
                                p { class: "help", {lang.t("Mientras nadie la tome, podés quitarla. Desaparece también del tablero del contratista.", "While nobody takes it, you can remove it. It also disappears from the contractor's board.")} }
                            }
                            div { class: "stack",
                                for o in mias {
                                    {
                                        let pidiendo = confirma_quitar().as_deref() == Some(o.id.as_str());
                                        let oid_pedir = o.id.clone();
                                        let oid = o.id.clone();
                                        rsx! {
                                            div { class: "card static",
                                                div { class: "card-h",
                                                    strong { "{o.nombre}" }
                                                    span { class: "chip chip-off", {lang.t("Esperando contratista", "Waiting for contractor")} }
                                                }
                                                p { class: "meta",
                                                    {match lang { Idioma::Es => format!("Trabajo {} · garantía {} · {} partidas", mm(o.moneda, o.trabajo, lang), mm(o.moneda, o.garantia_sugerida, lang), o.n_partidas_sugeridas), Idioma::En => format!("Job {} · guarantee {} · {} stages", mm(o.moneda, o.trabajo, lang), mm(o.moneda, o.garantia_sugerida, lang), o.n_partidas_sugeridas) }}
                                                }
                                                if let Some(r) = resumen_detalles(&o.detalles) {
                                                    p { class: "meta", "{r}" }
                                                }
                                                if pidiendo {
                                                    p { class: "estado err", {lang.t("¿Quitar esta oferta? Se retira para todos y no se puede deshacer.", "Remove this offer? It is withdrawn for everyone and cannot be undone.")} }
                                                    div { class: "acciones",
                                                        button {
                                                            class: "btn btn-danger btn-sm",
                                                            onclick: move |_| {
                                                                confirma_quitar.set(None);
                                                                let Some(nodo) = red() else { return };
                                                                let Some(yo_p) = yo() else { return };
                                                                let Some(o) = nodo.tablero().into_iter().find(|o| o.id == oid) else {
                                                                    ofertas.set(nodo.tablero());
                                                                    return;
                                                                };
                                                                let sec = consume_context::<Signal<ClaveSec>>()().0;
                                                                match retirar_oferta(&o, &yo_p.id, &sec, &nodo.obras_todas()) {
                                                                    Ok(r) => {
                                                                        nodo.retirar(r);
                                                                        ofertas.set(nodo.tablero());
                                                                        err.set(None);
                                                                    }
                                                                    Err(e) => err.set(Some(lang_now().error(&e))),
                                                                }
                                                            },
                                                            {lang.t("Sí, quitar", "Yes, remove")}
                                                        }
                                                        button {
                                                            class: "btn btn-ghost btn-sm",
                                                            onclick: move |_| confirma_quitar.set(None),
                                                            {lang.t("No", "No")}
                                                        }
                                                    }
                                                } else {
                                                    div { class: "acciones",
                                                        button {
                                                            class: "btn btn-danger btn-sm",
                                                            onclick: move |_| confirma_quitar.set(Some(oid_pedir.clone())),
                                                            {lang.t("Quitar oferta", "Remove offer")}
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        section { class: "panel",
                            div { class: "panel-h",
                                h2 { {lang.t("Ofertas del mandante", "Jobs from the client")} }
                                button {
                                    class: "btn btn-primary btn-sm",
                                    style: "width: auto;",
                                    disabled: buscando(),
                                    onclick: move |_| {
                                        let Some(nodo) = red() else { return };
                                        buscando.set(true);
                                        nodo.buscar();
                                        spawn(async move {
                                            nodo.esperar(Duration::from_secs(4)).await;
                                            buscando.set(false);
                                        });
                                    },
                                    if buscando() { {lang.t("Buscando…", "Searching…")} } else { {lang.t("Buscar ofertas", "Search jobs")} }
                                }
                            }
                            p { class: "help", {lang.t("Aceptás las condiciones o proponés otra garantía.", "Accept the terms or propose another guarantee.")} }
                            div { class: "stack",
                                for o in ajenas {
                                    button {
                                        class: "card",
                                        onclick: move |_| {
                                            garantia_acc.set(monto_editable(o.moneda, o.garantia_sugerida));
                                            sel_oferta.set(Some(o.clone()));
                                            screen.set(Screen::Oferta);
                                        },
                                        div { class: "card-h",
                                            strong { "{o.nombre}" }
                                            span { class: "chip chip-off", {match lang { Idioma::Es => format!("{} partidas", o.n_partidas_sugeridas), Idioma::En => format!("{} stages", o.n_partidas_sugeridas) }} }
                                        }
                                        p { class: "meta", "{lang.t(\"Mandante\", \"Client\")}: {o.mandante.nombre}" }
                                        p { class: "meta",
                                            {match lang { Idioma::Es => format!("Trabajo {} · garantía sugerida {}", mm(o.moneda, o.trabajo, lang), mm(o.moneda, o.garantia_sugerida, lang)), Idioma::En => format!("Job {} · suggested guarantee {}", mm(o.moneda, o.trabajo, lang), mm(o.moneda, o.garantia_sugerida, lang)) }}
                                        }
                                        if let Some(r) = resumen_detalles(&o.detalles) {
                                            p { class: "meta", "{r}" }
                                        }
                                    }
                                }
                            }
                            if sin_ajenas {
                                p { class: "estado info", "{hint_contratista}" }
                            }
                        }
                    }
                }
                div { class: "col",
                    if soy_m {
                        section { class: "panel",
                            h2 { {lang.t("Publicar", "Post")} }
                            p { class: "help",
                                {lang.t("El contratista no te ve a vos: ve el aviso en su tablero.", "The contractor does not see you: they see the notice on their board.")}
                            }
                            button {
                                class: "btn btn-primary",
                                onclick: move |_| screen.set(Screen::Nueva),
                                {lang.t("Publicar obra", "Post a job")}
                            }
                        }
                    }
                    section { class: "panel",
                        h2 { {lang.t("Obras en curso", "Jobs in progress")} }
                        if en_curso.is_empty() {
                            p { class: "help", {lang.t("Ninguna todavía. Cuando un contratista toma una oferta, aparece acá.", "None yet. When a contractor takes an offer, it shows up here.")} }
                        }
                        div { class: "stack",
                            for o in en_curso {
                                button {
                                    class: "card",
                                    onclick: move |_| {
                                        sel_obra.set(Some(o.id.clone()));
                                        screen.set(Screen::Detalle);
                                    },
                                    div { class: "card-h",
                                        strong { "{o.nombre}" }
                                        span { class: chip_estado(o.estado), "{lang.label_estado(o.estado)}" }
                                    }
                                    p { class: "meta",
                                        if soy_m {
                                            {match lang { Idioma::Es => format!("Contratista {} · {} partidas · {} por lado", o.contratista.nombre, o.n_partidas, mm(o.moneda, o.garantia, lang)), Idioma::En => format!("Contractor {} · {} stages · {} per side", o.contratista.nombre, o.n_partidas, mm(o.moneda, o.garantia, lang)) }}
                                        } else {
                                            {match lang { Idioma::Es => format!("Mandante {} · {} partidas · {} por lado", o.mandante.nombre, o.n_partidas, mm(o.moneda, o.garantia, lang)), Idioma::En => format!("Client {} · {} stages · {} per side", o.mandante.nombre, o.n_partidas, mm(o.moneda, o.garantia, lang)) }}
                                        }
                                    }
                                    if o.estado == EstadoObra::Contra && soy_m {
                                        p { class: "meta", {lang.t("Te toca: contra de garantía", "Your turn: guarantee counter")} }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn Nueva(
    yo: Signal<Option<Persona>>,
    red: Signal<Option<Nodo>>,
    screen: Signal<Screen>,
    err: Signal<Option<String>>,
    trabajo: Signal<String>,
    garantia: Signal<String>,
    obra_nom: Signal<String>,
) -> Element {
    // Montos en dólares (centavos).
    let t: u64 = leer_monto(Moneda::Usd, &trabajo());
    let g: u64 = leer_monto(Moneda::Usd, &garantia());
    let preview = n_partidas(t, g);
    let _ = PRECIO();
    let mut detalles = use_signal(|| vec![String::new(); 5]);
    use_effect(move || {
        if let Ok(n) = n_partidas(t, g) {
            let n = n as usize;
            let mut d = detalles();
            if d.len() != n {
                d.resize(n, String::new());
                detalles.set(d);
            }
        }
    });
    let lang = use_context::<Signal<Idioma>>()();
    rsx! {
        div { class: "pane",
            div { class: "migas",
                button { onclick: move |_| screen.set(Screen::Tablero), {lang.t("← Tablero", "← Board")} }
            }
            h1 { {lang.t("Publicar obra", "Post a job")} }
            p { class: "sub", {lang.t("Vos sos el mandante. El contratista va a ver esto en el tablero.", "You are the client. The contractor will see this on the board.")} }
            div { class: "cols parejas",
            div { class: "col",
            section { class: "panel",
            h2 { {lang.t("Condiciones", "Terms")} }
            label { class: "et", {lang.t("NOMBRE", "NAME")} }
            input {
                r#type: "text",
                value: "{obra_nom}",
                oninput: move |e| obra_nom.set(e.value()),
            }
            label { class: "et", {lang.t("TRABAJO (USD)", "JOB AMOUNT (USD)")} }
            input {
                r#type: "text",
                value: "{trabajo}",
                oninput: move |e| trabajo.set(e.value()),
            }
            label { class: "et", {lang.t("GARANTÍA SUGERIDA POR PARTIDA (USD)", "SUGGESTED GUARANTEE PER STAGE (USD)")} }
            input {
                r#type: "text",
                value: "{garantia}",
                oninput: move |e| garantia.set(e.value()),
            }
            p { class: if preview.is_ok() { "estado info" } else { "estado err" },
                match preview.clone() {
                    Ok(n) => match lang { Idioma::Es => format!("{n} partidas. En cada una los dos encierran {}.", caja::texto_usd_aprox(true, g)), Idioma::En => format!("{n} stages. In each one both lock {}.", caja::texto_usd_aprox(false, g)) },
                    Err(e) => lang.error(&e),
                }
            }
            p { class: "help", "{caja::estado_cotizacion(lang == Idioma::Es)}" }
            p { class: "help", "{caja::nota_precio_stagenet(lang == Idioma::Es)}" }
            p { class: "help", {lang.t("Mientras nadie la tome, la podés quitar del tablero. Se retira también para el contratista.", "While nobody takes it, you can remove it from the board. It is withdrawn for the contractor too.")} }
            button {
                class: "btn btn-primary",
                onclick: move |_| {
                    let Some(m) = yo() else { return };
                    let Some(nodo) = red() else {
                        err.set(Some(lang_now().t("La red todavía no arrancó.", "The network has not started yet.").into()));
                        return;
                    };
                    match Oferta::publicar_usd(m, obra_nom(), t, g, detalles()) {
                        Ok(mut o) => {
                            o.sellar_retiro(&consume_context::<Signal<ClaveSec>>()().0);
                            err.set(None);
                            nodo.publicar(o);
                            screen.set(Screen::Tablero);
                        }
                        Err(e) => err.set(Some(lang_now().error(&e))),
                    }
                },
                {lang.t("Publicar en la red", "Post to the network")}
            }
            }
            }
            div { class: "col",
            section { class: "panel",
            h2 { {lang.t("Qué entra en cada partida", "What each stage covers")} }
            if let Ok(n) = preview {
                p { class: "help", {lang.t("Como en un presupuesto: cimientos, muros, techumbre. El texto es opcional.", "Like a quote: foundations, walls, roof. The text is optional.")} }
                for (i, d) in detalles().into_iter().enumerate() {
                    label { class: "et", "{lang.t(\"PARTIDA\", \"STAGE\")} {i + 1}" }
                    input {
                        r#type: "text",
                        placeholder: "{lang.placeholder_partida(i, n)}",
                        value: "{d}",
                        oninput: move |e| {
                            let mut v = detalles();
                            if i < v.len() {
                                v[i] = e.value();
                                detalles.set(v);
                            }
                        },
                    }
                }
            } else {
                p { class: "help", {lang.t("Con un trabajo y una garantía válidos aparecen las partidas.", "With a valid job amount and guarantee, the stages show up.")} }
            }
            }
            }
            }
        }
    }
}

#[component]
fn VerOferta(
    yo: Signal<Option<Persona>>,
    red: Signal<Option<Nodo>>,
    sel_oferta: Signal<Option<Oferta>>,
    garantia_acc: Signal<String>,
    screen: Signal<Screen>,
    err: Signal<Option<String>>,
) -> Element {
    let lang = use_context::<Signal<Idioma>>()();
    let mut detalles = use_signal(|| {
        sel_oferta()
            .map(|o| o.detalles.clone())
            .unwrap_or_default()
    });
    use_effect(move || {
        let Some(of) = sel_oferta() else { return };
        let g: u64 = leer_monto(of.moneda, &garantia_acc());
        if let Ok(n) = n_partidas(of.trabajo, g) {
            let n = n as usize;
            let mut d = detalles();
            if d.len() != n {
                d.resize(n, String::new());
                detalles.set(d);
            }
        }
    });
    let Some(o) = sel_oferta() else {
        return rsx! { p { {lang.t("No hay oferta.", "There is no offer.")} } };
    };
    let g: u64 = leer_monto(o.moneda, &garantia_acc());
    let preview = n_partidas(o.trabajo, g);
    let contra = g != o.garantia_sugerida;
    let gtxt = mm(o.moneda, g, lang);
    let _ = PRECIO();
    rsx! {
        div { class: "pane",
            div { class: "migas",
                button { onclick: move |_| screen.set(Screen::Tablero), {lang.t("← Tablero", "← Board")} }
            }
            h1 { "{o.nombre}" }
            p { class: "sub",
                {match lang { Idioma::Es => format!("{} ofrece trabajo por {}. Garantía sugerida {} ({} partidas).", o.mandante.nombre, mm(o.moneda, o.trabajo, lang), mm(o.moneda, o.garantia_sugerida, lang), o.n_partidas_sugeridas), Idioma::En => format!("{} offers a job for {}. Suggested guarantee {} ({} stages).", o.mandante.nombre, mm(o.moneda, o.trabajo, lang), mm(o.moneda, o.garantia_sugerida, lang), o.n_partidas_sugeridas) }}
            }
            div { class: "cols parejas",
            div { class: "col",
            section { class: "panel",
            h2 { {lang.t("Tu respuesta", "Your answer")} }
            label { class: "et", {if o.moneda == Moneda::Usd { lang.t("TU GARANTÍA POR PARTIDA (USD)", "YOUR GUARANTEE PER STAGE (USD)") } else { lang.t("TU GARANTÍA", "YOUR GUARANTEE") }} }
            input {
                r#type: "text",
                value: "{garantia_acc}",
                oninput: move |e| garantia_acc.set(e.value()),
            }
            p { class: if preview.is_err() { "estado err" } else if contra { "estado wait" } else { "estado info" },
                match preview.clone() {
                    Ok(n) if contra => match lang { Idioma::Es => format!("Contra: {n} partidas de {gtxt}. El mandante tiene que confirmar."), Idioma::En => format!("Counter: {n} stages of {gtxt}. The client has to confirm.") },
                    Ok(n) => match lang { Idioma::Es => format!("Aceptás {n} partidas. Los dos encierran {gtxt} en cada una."), Idioma::En => format!("You accept {n} stages. Both lock {gtxt} in each one.") },
                    Err(e) => lang.error(&e),
                }
            }
            p { class: "help", {lang.t("Si cambiás la garantía, mandás una contra: el mandante la confirma o la rechaza.", "If you change the guarantee, you send a counter: the client confirms or rejects it.")} }
            if o.moneda == Moneda::Usd {
                p { class: "help", {match lang { Idioma::Es => format!("Por partida: {}. El XMR queda fijo cuando se encierra cada partida.", caja::texto_usd_aprox(true, g)), Idioma::En => format!("Per stage: {}. The XMR is fixed when each stage is locked.", caja::texto_usd_aprox(false, g)) }} }
                p { class: "help", "{caja::estado_cotizacion(lang == Idioma::Es)}" }
            }
            button {
                class: "btn btn-primary",
                onclick: move |_| {
                    let Some(c) = yo() else { return };
                    let Some(nodo) = red() else { return };
                    if !nodo.tablero().iter().any(|x| x.id == o.id) {
                        err.set(Some(lang_now().t(
                            "El mandante quitó esta oferta. Ya no se puede aceptar.",
                            "The client removed this offer. It can no longer be accepted.",
                        ).into()));
                        return;
                    }
                    let oferta = o.clone();
                    let dets = if contra { detalles() } else { oferta.detalles.clone() };
                    match Aceptacion::de_con(&oferta, c, g, dets) {
                        Ok(acc) => match Obra::desde_oferta(oferta, acc) {
                            Ok(obra) => {
                                let id = obra.id.clone();
                                if publicar_trato(&nodo, obra, yo(), err) {
                                    nodo.quitar(&id);
                                    screen.set(Screen::Tablero);
                                }
                            }
                            Err(e) => err.set(Some(lang_now().error(&e))),
                        },
                        Err(e) => err.set(Some(lang_now().error(&e))),
                    }
                },
                if contra { {lang.t("Proponer esta garantía", "Propose this guarantee")} } else { {lang.t("Aceptar condiciones", "Accept terms")} }
            }
            }
            }
            div { class: "col",
            section { class: "panel",
            h2 { {lang.t("Partidas", "Stages")} }
            if let Ok(n) = preview {
                if contra {
                    p { class: "help", {lang.t("Al cambiar la garantía, el número de partidas cambia. Completá o ajustá los textos.", "Changing the guarantee changes the number of stages. Fill in or adjust the texts.")} }
                    for (i, d) in detalles().into_iter().enumerate() {
                        label { class: "et", "{lang.t(\"PARTIDA\", \"STAGE\")} {i + 1}" }
                        input {
                            r#type: "text",
                            placeholder: "{lang.placeholder_partida(i, n)}",
                            value: "{d}",
                            oninput: move |e| {
                                let mut v = detalles();
                                if i < v.len() {
                                    v[i] = e.value();
                                    detalles.set(v);
                                }
                            },
                        }
                    }
                } else {
                    div { class: "stack",
                        for (i, d) in o.detalles.iter().enumerate() {
                            div { class: "partida static",
                                span { class: "num", "{i + 1}" }
                                div { class: "txt", strong { "{lang.titulo_partida(i, d)}" } }
                            }
                        }
                    }
                }
            }
            }
            }
            }
        }
    }
}

#[component]
fn RestaurarLlaves(
    caja: caja::Caja,
    yo: Signal<Option<Persona>>,
    obras: Signal<Vec<Obra>>,
    red: Signal<Option<Nodo>>,
    vista: Signal<caja::CajaVista>,
    mut err: Signal<Option<String>>,
) -> Element {
    let mut ok = use_signal(|| None::<String>);
    let lang = use_context::<Signal<Idioma>>()();
    let caja_semilla = caja.clone();
    let caja_share = caja.clone();
    let caja_vista = caja.clone();
    rsx! {
        div { class: "grupo",
        h3 { {lang.t("Importar las 25 palabras (.txt)", "Import the 25 words (.txt)")} }
        p { class: "help",
            {lang.t(
                "Recuperar las 25 palabras trae tu dirección personal. Si el archivo trae altura de bloque, el scan parte de ahí; si es un respaldo viejo sin altura, usa la ventana reciente (podés mirar más atrás). No trae la caja ni tu nombre en el trato.",
                "Restoring the 25 words brings back your personal address. If the file has a block height, scan starts there; old backups without height use the recent window (you can look further back). It does not bring the box or your name on the deal.",
            )}
        }
        button {
            class: "btn btn-ghost",
            onclick: move |_| {
                let Some(path) = rfd::FileDialog::new().pick_file() else { return };
                ok.set(None);
                match caja_semilla.restaurar_semilla(&path) {
                    Ok(code) => {
                        vista.set(caja_semilla.vista());
                        err.set(None);
                        ok.set(Some(caja::listo_humano(code, lang_now() == Idioma::Es)));
                    }
                    Err(e) => err.set(Some(caja::aviso_humano(&e, lang_now() == Idioma::Es))),
                }
            },
            {lang.t("Elegir el archivo de palabras", "Pick the words file")}
        }
        }
        div { class: "grupo",
        h3 { {lang.t("Importar un share (.share)", "Import a share (.share)")} }
        p { class: "help",
            {lang.t(
                "Recuperar un share trae la caja de una obra que ya está en este equipo. Tiene que ser el tuyo: el del otro lado no sirve. Si perdiste el perfil entero, esto no te vuelve a unir.",
                "Restoring a share brings back the box of a job already on this machine. It has to be yours: the other side's file will not work. If the whole profile is gone, this does not rejoin the deal.",
            )}
        }
        button {
            class: "btn btn-ghost",
            onclick: move |_| {
                let Some(path) = rfd::FileDialog::new().pick_file() else { return };
                let Some(quien) = yo() else {
                    err.set(Some(lang_now().t(
                        "Falta tu nombre en este equipo.",
                        "This machine does not have your name yet.",
                    ).into()));
                    return;
                };
                ok.set(None);
                match caja_share.restaurar_share(&path, &quien, &obras()) {
                    Ok(code) => {
                        vista.set(caja_vista.vista());
                        err.set(None);
                        ok.set(Some(caja::listo_humano(code, lang_now() == Idioma::Es)));
                    }
                    Err(e) => err.set(Some(caja::aviso_humano(&e, lang_now() == Idioma::Es))),
                }
            },
            {lang.t("Elegir el archivo del share", "Pick the share file")}
        }
        }
        div { class: "grupo",
        h3 { {lang.t("Respaldo de obras (JSON)", "Job backup (JSON)")} }
        p { class: "help",
            {lang.t(
                "Importa obras y ofertas de un konstruado-obras.json viejo. Puede estar desfasado respecto al otro; la cadena y el share mandan para el dinero.",
                "Imports jobs and offers from an old konstruado-obras.json. It may be behind the peer; chain and share rule the money.",
            )}
        }
        button {
            class: "btn btn-ghost",
            onclick: move |_| {
                let Some(nodo) = red() else {
                    err.set(Some(lang_now().t("La red todavía no arrancó.", "The network is not up yet.").into()));
                    return;
                };
                let Some(path) = rfd::FileDialog::new().pick_file() else { return };
                let Ok(raw) = std::fs::read_to_string(&path) else {
                    err.set(Some(lang_now().t("No pude leer ese archivo.", "Could not read that file.").into()));
                    return;
                };
                match persist::importar_perfil_obras(&raw) {
                    Ok(r) => {
                        let n_obras = r.obras.len();
                        let n_ofertas = r.ofertas.len();
                        for o in r.ofertas {
                            nodo.publicar(o);
                        }
                        let sec = consume_context::<Signal<ClaveSec>>()().0;
                        for mut obra in r.obras {
                            nodo.olvidar_salida_obra(&obra.id);
                            if let Some(q) = yo() {
                                if obra.participa(&q.id) {
                                    let _ = obra.preparar_para_red(&q.id, &q.clave_pub, &sec);
                                }
                            }
                            nodo.publicar_obra(obra);
                        }
                        obras.set(nodo.obras());
                        err.set(None);
                        ok.set(Some(match lang_now() {
                            Idioma::Es => format!(
                                "Importé {n_obras} obra(s) y {n_ofertas} oferta(s). Puede estar desfasado vs el otro; recuperá el share si lo tenés."
                            ),
                            Idioma::En => format!(
                                "Imported {n_obras} job(s) and {n_ofertas} offer(s). It may be behind the peer; restore the share if you have it."
                            ),
                        }));
                    }
                    Err(e) => err.set(Some(e)),
                }
            },
            {lang.t("Importar obras (.json)", "Import jobs (.json)")}
        }
        }
        if let Some(m) = ok() {
            p { class: "ok-msg", "{m}" }
        }
    }
}

/// Reinicia la app (después de restaurar un respaldo completo: el cambio de
/// carpetas lo hace `respaldo::aplicar_pendiente` al arrancar).
fn reiniciar_app() {
    if let Ok(exe) = std::env::current_exe() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let _ = std::process::Command::new(exe).args(args).spawn();
    }
    std::process::exit(0);
}

fn clase_tono(t: caja::Tono) -> &'static str {
    match t {
        caja::Tono::Ok => "estado ok",
        caja::Tono::Espera => "estado wait",
        caja::Tono::Error => "estado err",
        caja::Tono::Apagado => "estado info",
    }
}

/// Estado del último respaldo completo (regla compartida en respaldo.rs).
fn estado_respaldo(caja: &caja::Caja, yo: Option<Persona>, obras: &[Obra]) -> respaldo::EstadoRespaldo {
    let mid = yo.map(|p| p.id).unwrap_or_default();
    let (sem, cajas) = caja.claves_respaldo();
    respaldo::estado(&persist::dir(), &respaldo::huella_de_partes(&mid, obras, sem, cajas))
}

/// «Respaldos y recuperación»: respaldo completo, restaurar, y lo viejo en Avanzado.
/// Sube cada vez que se abre o cierra un plegable de «Respaldos y recuperación»:
/// las 25 palabras se ocultan solas.
static OCULTAR_SEMILLA: GlobalSignal<u64> = Signal::global(|| 0);

fn ocultar_semilla() {
    *OCULTAR_SEMILLA.write() += 1;
}

#[component]
fn SeccionRespaldos(
    caja: caja::Caja,
    yo: Signal<Option<Persona>>,
    obras: Signal<Vec<Obra>>,
    red: Signal<Option<Nodo>>,
    vista: Signal<caja::CajaVista>,
    err: Signal<Option<String>>,
) -> Element {
    let lang = use_context::<Signal<Idioma>>()();
    rsx! {
        if yo().is_some() {
            ExportarRespaldo { caja: caja.clone(), yo, obras, err }
        }
        if vista().tiene_semilla {
            VerSemilla { caja: caja.clone() }
        }
        RestaurarRespaldo { err }
        details { class: "plegable",
            summary { {lang.t("Avanzado: importar respaldos sueltos (0.2.7 o antes)", "Advanced: import standalone backups (0.2.7 or older)")} }
            div { class: "cuerpo",
                RestaurarLlaves { caja: caja.clone(), yo, obras, red, vista, err }
            }
        }
    }
}

/// «Ver las 25 palabras» de la billetera personal y su view key. Las palabras
/// solo se leen tras la advertencia y se sueltan (con zeroize) al ocultar,
/// al cerrar el plegable o al cambiar de vista.
#[component]
fn VerSemilla(caja: caja::Caja) -> Element {
    let lang = use_context::<Signal<Idioma>>()();
    let es = lang == Idioma::Es;
    let mut paso = use_signal(|| 0u8);
    let mut semilla = use_signal(|| None::<caja::SemillaVista>);
    let mut nota = use_signal(|| None::<String>);
    let mut ver_vk = use_signal(|| false);
    // Solo reacciona a un cierre/cambio posterior: el primer valor (0) o el
    // toogle de apertura del plegable no deben ocultar lo que el usuario acaba de abrir.
    let mut visto = use_signal(|| None::<u64>);
    use_effect(move || {
        let g = OCULTAR_SEMILLA();
        let prev = *visto.peek();
        visto.set(Some(g));
        if let Some(p) = prev {
            if g != p {
                paso.set(0);
                semilla.set(None);
                nota.set(None);
                ver_vk.set(false);
            }
        }
    });
    let llaves = caja.llaves_billetera();
    let caja_ver = caja.clone();
    let mostrar = paso() == 2 && semilla.read().is_some();
    rsx! {
        div { class: "grupo",
            h3 { {lang.t("Las 25 palabras y la view key", "The 25 words and the view key")} }
            p { class: "help",
                {lang.t(
                    "Para abrir esta billetera personal en Feather o monero-wallet-cli (stagenet).",
                    "To open this personal wallet in Feather or monero-wallet-cli (stagenet).",
                )}
            }
            if paso() == 0 {
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| paso.set(1),
                    {lang.t("Ver las 25 palabras", "Show the 25 words")}
                }
            }
            if paso() == 1 {
                div { class: "estado err aviso-semilla",
                    div {
                        for t in caja::aviso_ver_semilla(es) {
                            p { "{t}" }
                        }
                    }
                }
                div { class: "fila-btn",
                    button {
                        class: "btn btn-danger",
                        onclick: move |_| {
                            match caja_ver.ver_semilla() {
                                Ok(v) => {
                                    semilla.set(Some(v));
                                    paso.set(2);
                                }
                                Err(e) => {
                                    nota.set(Some(caja::aviso_humano(&e, lang_now() == Idioma::Es)));
                                    paso.set(0);
                                }
                            }
                        },
                        {lang.t("Mostrar", "Show")}
                    }
                    button {
                        class: "btn btn-ghost",
                        onclick: move |_| paso.set(0),
                        {lang.t("Cancelar", "Cancel")}
                    }
                }
            }
            if mostrar {
                if let Some(v) = semilla.read().as_ref() {
                    ol { class: "semilla",
                        for (i, w) in v.numeradas() {
                            li { span { class: "n", "{i}" } span { class: "w", "{w}" } }
                        }
                    }
                    label { class: "et", {lang.t("ALTURA DE RESTAURACIÓN (BLOQUE)", "RESTORE HEIGHT (BLOCK)")} }
                    span { class: "mono", {v.altura.map(|h| h.to_string()).unwrap_or_else(|| "—".into())} }
                    p { class: "help",
                        {lang.t(
                            "En la otra billetera elegí «restaurar desde semilla», red stagenet, y poné este bloque.",
                            "In the other wallet choose «restore from seed», stagenet, and enter this block.",
                        )}
                    }
                }
                div { class: "fila-btn",
                    button {
                        class: "btn btn-ghost",
                        onclick: move |_| {
                            let es = lang_now() == Idioma::Es;
                            let ok = semilla
                                .read()
                                .as_ref()
                                .map(|v| portapapeles::copiar_secreto(v.palabras.as_str(), caja::SEMILLA_PORTAPAPELES_SEG))
                                .unwrap_or(false);
                            nota.set(Some(if ok {
                                caja::aviso_copia_semilla(es)
                            } else if es {
                                "No pude usar el portapapeles: seleccioná y copiá a mano.".into()
                            } else {
                                "Could not use the clipboard: select and copy by hand.".into()
                            }));
                        },
                        {lang.t("Copiar las 25 palabras", "Copy the 25 words")}
                    }
                    button {
                        class: "btn btn-primary",
                        onclick: move |_| {
                            semilla.set(None);
                            paso.set(0);
                        },
                        {lang.t("Ocultar", "Hide")}
                    }
                }
            }
            if let Some(n) = nota() {
                p { class: "help", "{n}" }
            }
            if let Some(l) = llaves {
                label { class: "et", {lang.t("DIRECCIÓN DE TU BILLETERA (STAGENET)", "YOUR WALLET ADDRESS (STAGENET)")} }
                span { class: "mono caja", "{l.direccion}" }
                div { class: "fila-btn",
                    button {
                        class: "btn btn-ghost btn-sm",
                        onclick: {
                            let d = l.direccion.clone();
                            move |_| {
                                let es = lang_now() == Idioma::Es;
                                let ok = portapapeles::copiar(&d);
                                nota.set(Some(if ok { if es { "Dirección copiada." } else { "Address copied." } } else if es { "No pude usar el portapapeles." } else { "Could not use the clipboard." }.into()));
                            }
                        },
                        {lang.t("Copiar dirección", "Copy address")}
                    }
                    button {
                        class: "btn btn-ghost btn-sm",
                        onclick: move |_| ver_vk.set(!ver_vk()),
                        {if ver_vk() { lang.t("Ocultar view key", "Hide view key") } else { lang.t("Mostrar view key", "Show view key") }}
                    }
                }
                if ver_vk() {
                    p { class: "clave", "{l.view_key}" }
                    button {
                        class: "btn btn-ghost btn-sm",
                        onclick: {
                            let k = l.view_key.clone();
                            move |_| {
                                let es = lang_now() == Idioma::Es;
                                let ok = portapapeles::copiar(&k);
                                nota.set(Some(if ok { if es { "View key copiada." } else { "View key copied." } } else if es { "No pude usar el portapapeles." } else { "Could not use the clipboard." }.into()));
                            }
                        },
                        {lang.t("Copiar view key", "Copy view key")}
                    }
                    p { class: "help", {caja::ayuda_view_key_billetera(es)} }
                }
            }
        }
    }
}

#[component]
fn ExportarRespaldo(
    caja: caja::Caja,
    yo: Signal<Option<Persona>>,
    obras: Signal<Vec<Obra>>,
    mut err: Signal<Option<String>>,
) -> Element {
    let lang = use_context::<Signal<Idioma>>()();
    let es = lang == Idioma::Es;
    let mut clave = use_signal(String::new);
    let mut clave2 = use_signal(String::new);
    let mut ok = use_signal(|| None::<String>);
    let est = estado_respaldo(&caja, yo(), &obras());
    let (tono, linea) = respaldo::texto_estado(&est, lang);
    let caja_exp = caja.clone();
    rsx! {
        div { class: "grupo",
            h3 { {lang.t("Respaldo completo", "Full backup")} }
            p { class: clase_tono(tono), "{linea}" }
            for t in respaldo::ayuda(es) {
                p { class: "help", "{t}" }
            }
            label { class: "et", {lang.t("CONTRASEÑA DEL RESPALDO", "BACKUP PASSWORD")} }
            input {
                r#type: "password",
                placeholder: lang.t("Al menos 8 caracteres", "At least 8 characters"),
                value: "{clave}",
                oninput: move |e| clave.set(e.value()),
            }
            input {
                r#type: "password",
                placeholder: lang.t("Repetila", "Repeat it"),
                value: "{clave2}",
                oninput: move |e| clave2.set(e.value()),
            }
            button {
                class: "btn btn-primary",
                onclick: move |_| {
                    let es = lang_now() == Idioma::Es;
                    ok.set(None);
                    if clave() != clave2() {
                        err.set(Some(if es { "Las dos contraseñas no coinciden." } else { "The two passwords do not match." }.into()));
                        return;
                    }
                    if clave().chars().count() < respaldo::CLAVE_MINIMA {
                        err.set(Some(respaldo::aviso("codigo:respaldo-clave-corta", es)));
                        return;
                    }
                    let Some(path) = rfd::FileDialog::new()
                        .set_file_name(respaldo::nombre_archivo())
                        .add_filter("Konstruado", &[respaldo::EXTENSION])
                        .save_file()
                    else {
                        return;
                    };
                    let dir = persist::dir();
                    let ahora = chrono::Utc::now().timestamp();
                    let hecho = respaldo::exportar(&dir, persist::cargar(), caja_exp.material_respaldo(), &clave(), ahora)
                        .and_then(|(bytes, h)| {
                            respaldo::guardar_archivo(&path, &bytes)?;
                            respaldo::marcar_hecho(&dir, h, ahora)
                        });
                    match hecho {
                        Ok(()) => {
                            err.set(None);
                            clave.set(String::new());
                            clave2.set(String::new());
                            ok.set(Some(match lang_now() {
                                Idioma::Es => format!("Respaldo guardado en {}.", path.display()),
                                Idioma::En => format!("Backup saved to {}.", path.display()),
                            }));
                        }
                        Err(e) => err.set(Some(respaldo::aviso(&e, es))),
                    }
                },
                {lang.t("Exportar respaldo completo", "Export full backup")}
            }
            if let Some(m) = ok() {
                p { class: "ok-msg", "{m}" }
            }
        }
    }
}

/// Restaurar desde el respaldo completo: revisar, confirmar (peligro si ya hay
/// datos) y reiniciar. Se usa en la bienvenida y en Billetera.
#[component]
fn RestaurarRespaldo(mut err: Signal<Option<String>>) -> Element {
    let lang = use_context::<Signal<Idioma>>()();
    let mut archivo = use_signal(|| None::<(String, Vec<u8>)>);
    let mut clave = use_signal(String::new);
    let mut resumen = use_signal(|| None::<respaldo::Resumen>);
    let mut confirma = use_signal(|| false);
    let mut hecho = use_signal(|| None::<String>);
    let restaurar = move |_| {
        let es = lang_now() == Idioma::Es;
        let Some((_, bytes)) = archivo() else { return };
        let reemplazar = resumen().is_some_and(|r| r.hay_datos);
        match respaldo::preparar(&persist::dir(), &bytes, &clave(), reemplazar) {
            Ok(_) => {
                err.set(None);
                clave.set(String::new());
                hecho.set(Some(if es { "Respaldo restaurado. Konstruado se reinicia…" } else { "Backup restored. Konstruado is restarting…" }.into()));
                std::thread::spawn(|| {
                    std::thread::sleep(Duration::from_millis(1500));
                    reiniciar_app();
                });
            }
            Err(e) => err.set(Some(respaldo::aviso(&e, es))),
        }
    };
    rsx! {
        div { class: "grupo",
            h3 { {lang.t("Restaurar desde respaldo", "Restore from backup")} }
            p { class: "help",
                {lang.t(
                    "Elegí el archivo .kbak y escribí su contraseña. Primero se revisa todo (semilla, cada share contra su obra y tu rol); si algo no cuadra no se escribe nada.",
                    "Pick the .kbak file and type its password. Everything is checked first (seed, each share against its job and your role); if anything is off nothing is written.",
                )}
            }
            button {
                class: "btn btn-ghost",
                onclick: move |_| {
                    let Some(path) = rfd::FileDialog::new()
                        .add_filter("Konstruado", &[respaldo::EXTENSION])
                        .add_filter("*", &["*"])
                        .pick_file()
                    else {
                        return;
                    };
                    resumen.set(None);
                    confirma.set(false);
                    match std::fs::read(&path) {
                        Ok(b) if xmr_joint::sobre::es_sobre(&b) => {
                            let n = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                            archivo.set(Some((n, b)));
                            err.set(None);
                        }
                        Ok(_) => err.set(Some(respaldo::aviso("codigo:respaldo-no-es", lang_now() == Idioma::Es))),
                        Err(e) => err.set(Some(e.to_string())),
                    }
                },
                {lang.t("Elegir el archivo del respaldo", "Pick the backup file")}
            }
            if let Some((n, _)) = archivo() {
                span { class: "mono", "{n}" }
                label { class: "et", {lang.t("CONTRASEÑA", "PASSWORD")} }
                input {
                    r#type: "password",
                    value: "{clave}",
                    oninput: move |e| {
                        clave.set(e.value());
                        resumen.set(None);
                        confirma.set(false);
                    },
                }
                if resumen().is_none() {
                    button {
                        class: "btn btn-primary",
                        disabled: clave().is_empty(),
                        onclick: move |_| {
                            let Some((_, bytes)) = archivo() else { return };
                            match respaldo::revisar(&persist::dir(), &bytes, &clave()) {
                                Ok(r) => {
                                    err.set(None);
                                    resumen.set(Some(r));
                                }
                                Err(e) => err.set(Some(respaldo::aviso(&e, lang_now() == Idioma::Es))),
                            }
                        },
                        {lang.t("Abrir y revisar", "Open and check")}
                    }
                }
            }
            if let Some(r) = resumen() {
                dl { class: "datos",
                    dt { {lang.t("Cuenta", "Account")} }
                    dd { "{r.nombre} · {lang.rol(r.rol)}" }
                    dt { {lang.t("Obras · ofertas", "Jobs · offers")} }
                    dd { "{r.n_obras} · {r.n_ofertas}" }
                    dt { {lang.t("Cajas (shares)", "Escrows (shares)")} }
                    dd { "{r.n_shares}" }
                    dt { {lang.t("Billetera", "Wallet")} }
                    dd { class: "mono", {r.direccion.clone().map(|d| format!("{}…", &d[..12.min(d.len())])).unwrap_or_else(|| "—".into())} }
                    dt { {lang.t("Mirar desde el bloque", "Scan from block")} }
                    dd { {r.altura.map(|h| h.to_string()).unwrap_or_else(|| "—".into())} }
                    dt { {lang.t("Hecho", "Made")} }
                    dd { "{lang.fmt_cuando(r.creado)} · v{r.app}" }
                }
                if r.hay_datos {
                    p { class: "estado err",
                        {lang.t(
                            "Este equipo ya tiene una cuenta, una billetera o cajas. Restaurar las reemplaza enteras (no se mezclan): lo de ahora queda guardado en la carpeta previo-… de los datos.",
                            "This device already has an account, a wallet or escrows. Restoring replaces them entirely (nothing is merged): what is here now is kept in the previo-… folder inside the data folder.",
                        )}
                    }
                    if confirma() {
                        button { class: "btn btn-danger", onclick: restaurar, {lang.t("Sí, reemplazar todo y reiniciar", "Yes, replace everything and restart")} }
                        button { class: "btn btn-ghost", onclick: move |_| confirma.set(false), {lang.t("No", "No")} }
                    } else {
                        button { class: "btn btn-danger", onclick: move |_| confirma.set(true), {lang.t("Reemplazar lo de este equipo", "Replace what is on this device")} }
                    }
                } else {
                    button { class: "btn btn-primary", onclick: restaurar, {lang.t("Restaurar y reiniciar", "Restore and restart")} }
                }
            }
            if let Some(m) = hecho() {
                p { class: "ok-msg", "{m}" }
            }
        }
    }
}

/// Respaldos de la caja de una obra: guardar el share propio y recuperarlo.
#[component]
fn CajaRespaldo(
    obra_id: String,
    caja: caja::Caja,
    vista: Signal<caja::CajaVista>,
    yo: Signal<Option<Persona>>,
    obras: Signal<Vec<Obra>>,
    mut err: Signal<Option<String>>,
) -> Element {
    let mut ok = use_signal(|| None::<String>);
    let lang = use_context::<Signal<Idioma>>()();
    let caja_share = caja.clone();
    let caja_vista = caja.clone();
    rsx! {
        div { class: "grupo",
            h3 { {lang.t("Importar un share suelto", "Import a standalone share")} }
            p { class: "help",
                {lang.t(
                    "Para archivos .share de 0.2.7 o antes. Desde 0.2.8 el share va en el respaldo completo (Billetera → Respaldos). Si perdiste el share de esta obra, recuperalo desde el archivo que guardaste. Tiene que ser el tuyo y la obra tiene que seguir en este equipo.",
                    "If you lost this job's share, restore it from the file you saved. It has to be yours, and the job has to still be on this machine.",
                )}
            }
            button {
                class: "btn btn-ghost",
                onclick: move |_| {
                    let Some(path) = rfd::FileDialog::new().pick_file() else { return };
                    let Some(quien) = yo() else {
                        err.set(Some(lang_now().t(
                            "Falta tu nombre en este equipo.",
                            "This machine does not have your name yet.",
                        ).into()));
                        return;
                    };
                    ok.set(None);
                    match caja_share.restaurar_share(&path, &quien, &obras()) {
                        Ok(code) => {
                            vista.set(caja_vista.vista());
                            err.set(None);
                            ok.set(Some(caja::listo_humano(code, lang_now() == Idioma::Es)));
                        }
                        Err(e) => err.set(Some(caja::aviso_humano(&e, lang_now() == Idioma::Es))),
                    }
                },
                {lang.t("Importar un share suelto", "Import a standalone share")}
            }
        }
        if let Some(m) = ok() {
            p { class: "ok-msg", "{m}" }
        }
    }
}

/// Qué tan atrás mira la caja de una obra, y el botón para mirar más.
#[component]
fn CajaMirar(
    obra_id: String,
    caja: caja::Caja,
    vista: Signal<caja::CajaVista>,
    mut err: Signal<Option<String>>,
) -> Element {
    let lang = use_context::<Signal<Idioma>>()();
    let v = vista();
    let hay = v.caja_de(&obra_id).is_some();
    let mirada = v.miradas.into_iter().find(|m| m.obra == obra_id);
    let caja_atras = caja.clone();
    let obra_atras = obra_id.clone();
    if !hay {
        return rsx! {
            p { class: "help", {lang.t("La caja todavía no está armada en este equipo.", "The box is not built on this machine yet.")} }
        };
    }
    rsx! {
        dl { class: "datos",
            dt { {lang.t("La caja mira", "The box scans")} }
            dd {
                {match (&mirada, lang) {
                    (Some(m), Idioma::Es) => format!("{} bloques hacia atrás", m.bloques),
                    (Some(m), Idioma::En) => format!("{} blocks back", m.bloques),
                    (None, Idioma::Es) => "los últimos 40 bloques".to_string(),
                    (None, Idioma::En) => "the last 40 blocks".to_string(),
                }}
            }
            if let Some(m) = mirada.as_ref().filter(|m| m.retro > 0) {
                dt { {lang.t("Falta mirar", "Left to scan")} }
                dd {
                    {match lang {
                        Idioma::Es => format!("{} bloques", m.retro),
                        Idioma::En => format!("{} blocks", m.retro),
                    }}
                }
            }
        }
        if let Some(aviso) = mirada.as_ref().and_then(|m| m.aviso.clone()) {
            p { class: "estado err", "{caja::aviso_humano(&aviso, matches!(lang, Idioma::Es))}" }
        }
        p { class: "help",
            {lang.t(
                "Sirve si el fondeo es más viejo que lo que la caja ya miró.",
                "Useful if the funding is older than what the box already scanned.",
            )}
        }
        button {
            class: "btn btn-ghost",
            onclick: move |_| {
                match caja_atras.pedir_atras_caja(&obra_atras) {
                    Ok(()) => {
                        vista.set(caja_atras.vista());
                        err.set(None);
                    }
                    Err(e) => err.set(Some(caja::aviso_humano(&e, lang_now() == Idioma::Es))),
                }
            },
            {lang.t("Mirar 200 bloques más atrás en la caja", "Scan 200 more blocks back in the box")}
        }
    }
}

#[component]
fn CajaLlave(obra_id: String, caja: caja::Caja, vista: Signal<caja::CajaVista>) -> Element {
    let mut mostrar = use_signal(|| false);
    let lang = use_context::<Signal<Idioma>>()();
    let addr = vista().caja_de(&obra_id).map(|s| s.to_string());
    let Some(addr) = addr else {
        return rsx! {};
    };
    let clave = if mostrar() { caja.view_de(&obra_id) } else { None };
    rsx! {
        label { class: "et", {lang.t("DIRECCIÓN DE LA CAJA (STAGENET)", "BOX ADDRESS (STAGENET)")} }
        span { class: "mono caja", "{addr}" }
        button {
            class: "btn btn-ghost btn-sm",
            onclick: move |_| mostrar.set(!mostrar()),
            {if mostrar() { lang.t("Ocultar view key", "Hide view key") } else { lang.t("Mostrar view key de la caja", "Show the box view key") }}
        }
        if let Some(clave) = clave {
            p { class: "clave", "{clave}" }
            p { class: "help",
                {lang.t(
                    "Junto con la dirección, esta view key muestra los movimientos de la caja. No alcanza para gastar.",
                    "With the address, this view key shows the box movements. It cannot spend.",
                )}
            }
        }
    }
}

#[component]
fn Detalle(
    yo: Signal<Option<Persona>>,
    red: Signal<Option<Nodo>>,
    obras: Signal<Vec<Obra>>,
    sel_obra: Signal<Option<String>>,
    sel_partida: Signal<Option<usize>>,
    screen: Signal<Screen>,
    err: Signal<Option<String>>,
    caja: caja::Caja,
    vista: Signal<caja::CajaVista>,
) -> Element {
    let mut confirma_abandono = use_signal(|| false);
    let mut confirma_salida_local = use_signal(|| false);
    let mut export_msg = use_signal(|| None::<String>);
    let mut extra_nom = use_signal(String::new);
    let mut extra_monto = use_signal(String::new);
    let lang = use_context::<Signal<Idioma>>()();
    let es = matches!(lang, Idioma::Es);
    let sec = use_context::<Signal<ClaveSec>>()().0;
    let id = sel_obra().unwrap_or_default();
    let Some(obra) = obras().into_iter().find(|o| o.id == id) else {
        return rsx! { p { {lang.t("La obra todavía no llegó. Si la acabás de publicar, esperá al contratista.", "The job has not arrived yet. If you just posted it, wait for the contractor.")} } };
    };
    let mid = yo().map(|p| p.id).unwrap_or_default();
    if !obra.participa(&mid) {
        return rsx! { p { {lang.t("Esta obra es de otras dos personas.", "This job belongs to two other people.")} } };
    }
    let soy_m = obra.mandante.id == mid;
    let estado = obra.estado;
    let activa = obra.activa();
    let garantia = obra.garantia;
    let n_part = obra.n_partidas;
    let nom = obra.nombre.clone();
    let mnom = obra.mandante.nombre.clone();
    let cnom = obra.contratista.nombre.clone();
    let partidas = obra.partidas.clone();
    let contra = estado == EstadoObra::Contra;
    let soy_c = obra.contratista.id == mid;
    let se_puede_abandonar = soy_m || soy_c;
    let abierta = !matches!(
        estado,
        EstadoObra::Cerrada | EstadoObra::Rechazada | EstadoObra::Abandonada
    );
    let sincronizando = abierta && sincronizando_trato(red, yo, &obra);
    let extra_label = match obra.leer_extra(&sec) {
        TextoLeido::Plano(t) => t,
        TextoLeido::Cerrado => lang.t("Texto cifrado", "Encrypted text").into(),
    };
    let hay_caja = vista().caja_de(&obra.id).is_some();
    let obra_caja = obra.id.clone();
    let v = vista();
    // Estado en curso de cada partida, con la misma regla que la pantalla de partida.
    let en_curso: Vec<(Option<&'static str>, bool, bool, Option<String>)> = (0..partidas.len())
        .map(|i| {
            let a = caja::acciones_partida_con(&obra, i, &mid, &v.lineas_de(&obra.id, i), v.traba(&obra, i));
            let toca = a.aceptar_pago || a.avisar_termino || a.confirmar_fondeo || a.empezar_fondeo_de_nuevo;
            (caja::en_curso_corto(a.en_curso, es), a.frenado, toca, caja::traba_corta(a.traba, es))
        })
        .collect();
    let pagadas = partidas.iter().filter(|p| p.estado == PartidaEstado::Pagada).count();
    let hay_foco = contra || obra.cierre.is_some() || (abierta && obra.extra.is_some()) || estado == EstadoObra::Abandonada;
    rsx! {
        div { class: "pane",
            div { class: "cabeza",
                h1 { "{nom}" }
                span { class: chip_estado(estado), "{lang.label_estado(estado)}" }
                if sincronizando {
                    span { class: "chip chip-wait", {lang.t("Sincronizando…", "Syncing…")} }
                }
            }
            p { class: "lead",
                {match lang { Idioma::Es => format!("Mandante {mnom} · contratista {cnom} · trabajo {}", mm(obra.moneda, obra.trabajo, lang)), Idioma::En => format!("Client {mnom} · contractor {cnom} · job {}", mm(obra.moneda, obra.trabajo, lang)) }}
            }
            div { class: "cols",
                div { class: "col",
                    if hay_foco {
                        section { class: "panel foco",
                            h2 { {lang.t("Para resolver", "To resolve")} }
                            if contra {
                                p { class: "estado info",
                                    {match lang { Idioma::Es => format!("El contratista propone garantía {} ({} partidas).", mm(obra.moneda, garantia, lang), n_part), Idioma::En => format!("The contractor proposes guarantee {} ({} stages).", mm(obra.moneda, garantia, lang), n_part) }}
                                }
                                if soy_m {
                                    div { class: "acciones",
                                        button {
                                            class: "btn btn-primary",
                                            onclick: {
                                                let obra = obra.clone();
                                                let mid = mid.clone();
                                                move |_| {
                                                    let mut obra = obra.clone();
                                                    if !exigir_sesion(red, yo, &obra, err) {
                                                        return;
                                                    }
                                                    let Some(nodo) = red() else { return };
                                                    match obra.confirmar_contra(&mid) {
                                                        Ok(()) => {
                                                            publicar_trato(&nodo, obra.clone(), yo(), err);
                                                        }
                                                        Err(e) => err.set(Some(lang_now().error(&e))),
                                                    }
                                                }
                                            },
                                            {lang.t("Confirmar contra", "Confirm counter")}
                                        }
                                        button {
                                            class: "btn btn-ghost",
                                            onclick: {
                                                let obra = obra.clone();
                                                let mid = mid.clone();
                                                move |_| {
                                                    let mut obra = obra.clone();
                                                    if !exigir_sesion(red, yo, &obra, err) {
                                                        return;
                                                    }
                                                    let Some(nodo) = red() else { return };
                                                    match obra.rechazar_contra(&mid) {
                                                        Ok(()) => {
                                                            let gpub = if obra.garantia_publicada > 0 {
                                                                obra.garantia_publicada
                                                            } else {
                                                                obra.garantia
                                                            };
                                                            let dets: Vec<String> =
                                                                obra.partidas.iter().map(|p| p.detalle.clone()).collect();
                                                            match Oferta::publicar(
                                                                obra.mandante.clone(),
                                                                obra.nombre.clone(),
                                                                obra.trabajo,
                                                                gpub,
                                                                dets,
                                                            ) {
                                                                Ok(mut oferta) => {
                                                                    oferta.id = obra.id.clone();
                                                                    oferta.sellar_retiro(&consume_context::<Signal<ClaveSec>>()().0);
                                                                    err.set(None);
                                                                    publicar_trato(&nodo, obra.clone(), yo(), err);
                                                                    nodo.publicar(oferta);
                                                                    screen.set(Screen::Tablero);
                                                                }
                                                                Err(e) => err.set(Some(lang_now().error(&e))),
                                                            }
                                                        }
                                                        Err(e) => err.set(Some(lang_now().error(&e))),
                                                    }
                                                }
                                            },
                                            {lang.t("No aceptar esta garantía", "Do not accept this guarantee")}
                                        }
                                    }
                                } else {
                                    p { class: "estado wait", {lang.t("Esperando que el mandante responda la contra.", "Waiting for the client to answer the counter.")} }
                                }
                            }
                            if estado == EstadoObra::Abandonada {
                                p { class: "estado err", {lang.t("Esta obra se abandonó. El trato quedó cortado.", "This job was abandoned. The deal is cut.")} }
                            }
                            if let Some(cl) = obra.cierre.clone() {
                                if cl.id == mid {
                                    p { class: "estado wait", {lang.t("Esperando que acepten cortar el trato.", "Waiting for them to accept ending the deal.")} }
                                } else if se_puede_abandonar {
                                    p { class: "estado err", {match lang { Idioma::Es => format!("{} quiere cortar el trato.", cl.nombre), Idioma::En => format!("{} wants to end the deal.", cl.nombre) }} }
                                    div { class: "acciones",
                                        button {
                                            class: "btn btn-danger",
                                            onclick: {
                                                let obra = obra.clone();
                                                move |_| {
                                                    let mut obra = obra.clone();
                                                    if !exigir_sesion(red, yo, &obra, err) {
                                                        return;
                                                    }
                                                    let Some(quien) = yo() else { return };
                                                    let Some(nodo) = red() else { return };
                                                    match obra.aceptar_cierre(&quien) {
                                                        Ok(()) => {
                                                            err.set(None);
                                                            publicar_trato(&nodo, obra.clone(), yo(), err);
                                                            screen.set(Screen::Tablero);
                                                        }
                                                        Err(e) => err.set(Some(lang_now().error(&e))),
                                                    }
                                                }
                                            },
                                            {lang.t("Aceptar cierre", "Accept close")}
                                        }
                                        button {
                                            class: "btn btn-primary",
                                            onclick: {
                                                let obra = obra.clone();
                                                move |_| {
                                                    let mut obra = obra.clone();
                                                    if !exigir_sesion(red, yo, &obra, err) {
                                                        return;
                                                    }
                                                    let Some(quien) = yo() else { return };
                                                    let Some(nodo) = red() else { return };
                                                    match obra.rechazar_cierre(&quien) {
                                                        Ok(()) => {
                                                            err.set(None);
                                                            publicar_trato(&nodo, obra.clone(), yo(), err);
                                                        }
                                                        Err(e) => err.set(Some(lang_now().error(&e))),
                                                    }
                                                }
                                            },
                                            {lang.t("Seguir con la obra", "Keep going")}
                                        }
                                    }
                                }
                            }
                            if abierta {
                                if let Some(ex) = obra.extra.clone() {
                                    if ex.por.id == mid {
                                        p { class: "estado wait", {match lang { Idioma::Es => format!("Esperando respuesta a la extra: {} ({} por lado)", extra_label, mm(obra.moneda, ex.monto, lang)), Idioma::En => format!("Waiting for an answer on the extra: {} ({} per side)", extra_label, mm(obra.moneda, ex.monto, lang)) }} }
                                    } else {
                                        p { class: "estado info", {match lang { Idioma::Es => format!("{} propone extra: {} (+{} por lado)", ex.por.nombre, extra_label, mm(obra.moneda, ex.monto, lang)), Idioma::En => format!("{} proposes extra: {} (+{} per side)", ex.por.nombre, extra_label, mm(obra.moneda, ex.monto, lang)) }} }
                                        div { class: "acciones",
                                            button {
                                                class: "btn btn-primary",
                                                onclick: {
                                                    let obra = obra.clone();
                                                    move |_| {
                                                        let mut obra = obra.clone();
                                                        if !exigir_sesion(red, yo, &obra, err) {
                                                            return;
                                                        }
                                                        let Some(quien) = yo() else { return };
                                                        let Some(nodo) = red() else { return };
                                                        let sec = consume_context::<Signal<ClaveSec>>()().0;
                                                        if let Err(e) = obra.abrir_extra(&sec) {
                                                            err.set(Some(lang_now().error(&e)));
                                                            return;
                                                        }
                                                        match obra.aceptar_extra(&quien) {
                                                            Ok(()) => {
                                                                err.set(None);
                                                                publicar_trato(&nodo, obra.clone(), yo(), err);
                                                            }
                                                            Err(e) => err.set(Some(lang_now().error(&e))),
                                                        }
                                                    }
                                                },
                                                {lang.t("Aceptar extra", "Accept extra")}
                                            }
                                            button {
                                                class: "btn btn-ghost",
                                                onclick: {
                                                    let obra = obra.clone();
                                                    move |_| {
                                                        let mut obra = obra.clone();
                                                        if !exigir_sesion(red, yo, &obra, err) {
                                                            return;
                                                        }
                                                        let Some(quien) = yo() else { return };
                                                        let Some(nodo) = red() else { return };
                                                        match obra.rechazar_extra(&quien) {
                                                            Ok(()) => {
                                                                err.set(None);
                                                                publicar_trato(&nodo, obra.clone(), yo(), err);
                                                            }
                                                            Err(e) => err.set(Some(lang_now().error(&e))),
                                                        }
                                                    }
                                                },
                                                {lang.t("No agregar", "Do not add")}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    section { class: "panel",
                        div { class: "panel-h",
                            h2 { {lang.t("Partidas", "Stages")} }
                            span { class: "chip chip-info",
                                {match lang { Idioma::Es => format!("{pagadas} de {} pagadas", partidas.len()), Idioma::En => format!("{pagadas} of {} paid", partidas.len()) }}
                            }
                        }
                        p { class: "help", {lang.t("Entrá a cada partida para encerrar, avisar que terminó, tratar el porcentaje y ver el hilo.", "Open each stage to lock it, report finish, deal the percentage and see the thread.")} }
                        div { class: "stack",
                            for (i, p) in partidas.iter().enumerate() {
                                {
                                    let on = activa == Some(i);
                                    let titulo = lang.titulo_partida(i, &p.detalle);
                                    let label = lang.label_partida(p);
                                    let kind = chip_partida(p.estado);
                                    let (curso, frenado, toca, traba) = en_curso.get(i).cloned().unwrap_or((None, false, false, None));
                                    let corto = caja::saldo_corto(es, p.estado, obra.piconero_partida(i), p.fondeo_txid.is_some());
                                    rsx! {
                                        button {
                                            class: if on { "partida on" } else { "partida" },
                                            onclick: move |_| {
                                                sel_partida.set(Some(i));
                                                screen.set(Screen::VerPartida);
                                            },
                                            span { class: "num", "{i + 1}" }
                                            div { class: "txt",
                                                strong { "{titulo}" }
                                                span { "{mm(obra.moneda, p.capital(garantia), lang)} {lang.t(\"por lado\", \"per side\")}" }
                                                if let Some(corto) = corto {
                                                    span { "{corto}" }
                                                }
                                            }
                                            div { class: "chips",
                                                span { class: kind, "{label}" }
                                                if frenado {
                                                    span { class: "chip chip-err", {lang.t("Frenado", "Stopped")} }
                                                } else if let Some(c) = curso {
                                                    span { class: "chip chip-wait", "{c}" }
                                                } else if let Some(t) = traba {
                                                    span { class: "chip chip-wait", "{t}" }
                                                } else if toca {
                                                    span { class: "chip chip-info", {lang.t("Te toca", "Your turn")} }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if abierta && se_puede_abandonar && estado != EstadoObra::Contra && obra.extra.is_none() {
                        details { class: "plegable",
                            summary { {lang.t("Proponer partida extra", "Propose an extra stage")} }
                            div { class: "cuerpo",
                                p { class: "help", {lang.t("Una extra suma una partida al final. El otro tiene que aceptarla.", "An extra adds a stage at the end. The other person has to accept it.")} }
                                label { class: "et", {lang.t("NOMBRE", "NAME")} }
                                input {
                                    r#type: "text",
                                    placeholder: lang.t("P. ej. Techumbre extra", "E.g. Extra roof"),
                                    value: "{extra_nom}",
                                    oninput: move |e| extra_nom.set(e.value()),
                                }
                                label { class: "et", {if obra.moneda == Moneda::Usd { lang.t("MONTO POR LADO (USD)", "AMOUNT PER SIDE (USD)") } else { lang.t("MONTO POR LADO", "AMOUNT PER SIDE") }} }
                                input {
                                    r#type: "text",
                                    placeholder: if obra.moneda == Moneda::Usd { lang.t("P. ej. 150", "E.g. 150") } else { lang.t("P. ej. 3000", "E.g. 3000") },
                                    value: "{extra_monto}",
                                    oninput: move |e| extra_monto.set(e.value()),
                                }
                                button {
                                    class: "btn btn-ghost",
                                    onclick: {
                                        let obra = obra.clone();
                                        move |_| {
                                            let mut obra = obra.clone();
                                            if extra_nom().trim().is_empty() {
                                                return;
                                            }
                                            let m = leer_monto(obra.moneda, &extra_monto());
                                            if m == 0 {
                                                err.set(Some(lang_now().t("La extra lleva un monto mayor a cero.", "The extra needs an amount greater than zero.").into()));
                                                return;
                                            }
                                            if !exigir_sesion(red, yo, &obra, err) {
                                                return;
                                            }
                                            let Some(quien) = yo() else { return };
                                            let Some(nodo) = red() else { return };
                                            match obra.proponer_extra(&quien, extra_nom(), m) {
                                                Ok(()) => {
                                                    err.set(None);
                                                    extra_nom.set(String::new());
                                                    extra_monto.set(String::new());
                                                    publicar_trato(&nodo, obra.clone(), yo(), err);
                                                }
                                                Err(e) => err.set(Some(lang_now().error(&e))),
                                            }
                                        }
                                    },
                                    {lang.t("Proponer extra", "Propose extra")}
                                }
                            }
                        }
                    }
                }
                div { class: "col",
                    section { class: "panel",
                        h2 { {lang.t("Caja 2-de-2", "2-of-2 box")} }
                        if hay_caja {
                            CajaLlave { obra_id: obra_caja, caja: caja.clone(), vista }
                        } else if matches!(estado, EstadoObra::Acordada | EstadoObra::EnMarcha) && (soy_m || soy_c) {
                            p { class: "estado wait", {lang.t("Armando la caja 2-de-2. Los dos tienen que seguir en línea.", "Building the 2-of-2 box. Both have to stay online.")} }
                        } else {
                            p { class: "help", {lang.t("La caja se arma cuando los dos acuerdan la obra.", "The box is built once both agree on the job.")} }
                        }
                        dl { class: "datos",
                            dt { {lang.t("Partidas", "Stages")} }
                            dd { "{n_part}" }
                            dt { {lang.t("Garantía", "Guarantee")} }
                            dd { "{mm(obra.moneda, garantia, lang)}" }
                            dt { {lang.t("Trabajo", "Job")} }
                            dd { "{mm(obra.moneda, obra.trabajo, lang)}" }
                        }
                    }
                    section { class: "panel",
                        h2 { {lang.t("Constancia", "Record")} }
                        p { class: "help", {lang.t("Un resumen de la obra, sus partidas y el hilo, para guardar o imprimir.", "A summary of the job, its stages and the thread, to keep or print.")} }
                        div { class: "acciones",
                            button {
                                class: "btn btn-ghost btn-sm",
                                onclick: {
                                    let obra = obra.clone();
                                    let sec = sec.clone();
                                    move |_| {
                                        match export::guardar_txt(&obra, lang_now(), &sec) {
                                            Ok(p) => export_msg.set(Some(format!("{} {}", lang_now().t("Guardado en", "Saved to"), p.display()))),
                                            Err(e) => export_msg.set(Some(e)),
                                        }
                                    }
                                },
                                {lang.t("Exportar texto", "Export text")}
                            }
                            button {
                                class: "btn btn-ghost btn-sm",
                                onclick: {
                                    let obra = obra.clone();
                                    let sec = sec.clone();
                                    move |_| {
                                        match export::guardar_pdf(&obra, lang_now(), &sec) {
                                            Ok(p) => export_msg.set(Some(format!("{} {}", lang_now().t("Guardado en", "Saved to"), p.display()))),
                                            Err(e) => export_msg.set(Some(e)),
                                        }
                                    }
                                },
                                {lang.t("Exportar PDF", "Export PDF")}
                            }
                        }
                        if let Some(m) = export_msg() {
                            p { class: "ok-msg", "{m}" }
                        }
                    }
                    if abierta && (soy_m || soy_c) {
                        details { class: "plegable",
                            summary { {lang.t("Avanzado", "Advanced")} }
                            div { class: "cuerpo",
                                CajaRespaldo { obra_id: obra.id.clone(), caja: caja.clone(), vista, yo, obras, err }
                                CajaMirar { obra_id: obra.id.clone(), caja: caja.clone(), vista, err }
                            }
                        }
                    }
                    if se_puede_abandonar && (obra.cierre.is_none() || !abierta) {
                        details { class: "plegable cuidado",
                            summary { {lang.t("Cortar o archivar", "End or archive")} }
                            div { class: "cuerpo",
                                if abierta && obra.cierre.is_none() {
                                    div { class: "grupo",
                                        h3 { {lang.t("Abandonar la obra", "Abandon the job")} }
                                        if confirma_abandono() {
                                            p { class: "estado err",
                                                if obra.hay_riesgo() {
                                                    {lang.t("Hay partidas encerradas. El otro tiene que aceptar el cierre.", "There are locked stages. The other person has to accept the close.")}
                                                } else {
                                                    {lang.t("¿Abandonar? Se corta el trato y no se puede deshacer.", "Abandon? The deal is cut and cannot be undone.")}
                                                }
                                            }
                                            div { class: "acciones",
                                                button {
                                                    class: "btn btn-danger",
                                                    onclick: {
                                                        let obra = obra.clone();
                                                        move |_| {
                                                            let mut obra = obra.clone();
                                                            if obra.hay_riesgo() && !exigir_sesion(red, yo, &obra, err) {
                                                                return;
                                                            }
                                                            let Some(quien) = yo() else { return };
                                                            let Some(nodo) = red() else { return };
                                                            match obra.abandonar(&quien) {
                                                                Ok(()) => {
                                                                    err.set(None);
                                                                    confirma_abandono.set(false);
                                                                    publicar_trato(&nodo, obra.clone(), yo(), err);
                                                                    if !obra.hay_riesgo() || obra.estado == EstadoObra::Abandonada {
                                                                        screen.set(Screen::Tablero);
                                                                    }
                                                                }
                                                                Err(e) => err.set(Some(lang_now().error(&e))),
                                                            }
                                                        }
                                                    },
                                                    if obra.hay_riesgo() { {lang.t("Proponer cierre", "Propose close")} } else { {lang.t("Sí, abandonar", "Yes, abandon")} }
                                                }
                                                button {
                                                    class: "btn btn-ghost",
                                                    onclick: move |_| confirma_abandono.set(false),
                                                    {lang.t("No", "No")}
                                                }
                                            }
                                        } else {
                                            p { class: "help", {lang.t("Corta el trato con el otro. Si hay partidas encerradas, el otro tiene que aceptar el cierre.", "Ends the deal with the other person. If stages are locked, they have to accept the close.")} }
                                            button {
                                                class: "btn btn-danger btn-sm",
                                                onclick: move |_| confirma_abandono.set(true),
                                                {lang.t("Abandonar esta obra", "Abandon this job")}
                                            }
                                        }
                                    }
                                }
                                div { class: "grupo",
                                    h3 { {lang.t("Archivar en este equipo", "Archive on this device")} }
                                    p { class: "help",
                                        {lang.t(
                                            "Sale del tablero y de Mis obras. No mueve fondos ni corta el trato del otro. El share y el contexto quedan en disco.",
                                            "It leaves the board and My jobs. Funds are not moved and the other side is not cut off. Share and context stay on disk.",
                                        )}
                                    }
                                    if confirma_salida_local() {
                                        div { class: "acciones",
                                            button {
                                                class: "btn btn-danger",
                                                onclick: {
                                                    let obra = obra.clone();
                                                    let caja = caja.clone();
                                                    move |_| {
                                                        let Some(nodo) = red() else { return };
                                                        for i in 0..obra.partidas.len() {
                                                            caja.cancelar_fondeo(&obra.id, i);
                                                        }
                                                        nodo.quitar(&obra.id);
                                                        nodo.archivar_obra_local(&obra.id);
                                                        obras.set(nodo.obras());
                                                        confirma_salida_local.set(false);
                                                        err.set(None);
                                                        screen.set(Screen::Tablero);
                                                    }
                                                },
                                                {lang.t("Sí, archivar aquí", "Yes, archive here")}
                                            }
                                            button {
                                                class: "btn btn-ghost",
                                                onclick: move |_| confirma_salida_local.set(false),
                                                {lang.t("No", "No")}
                                            }
                                        }
                                    } else {
                                        button {
                                            class: "btn btn-danger btn-sm",
                                            onclick: move |_| confirma_salida_local.set(true),
                                            {lang.t("Archivar esta obra", "Archive this job")}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Clase, texto y txid (aparte, en monoespaciado) de una línea del motor.
fn linea_estado(t: &caja::Texto, es: bool) -> (&'static str, String, Option<String>) {
    let clase = if caja::es_freno(t) {
        "estado err"
    } else if t.en_curso() != caja::EnCurso::Nada {
        "estado wait"
    } else {
        "estado info"
    };
    match t {
        caja::Texto::EsperandoFondeo(tx) => (
            clase,
            if es { "Fondeo publicado, esperando un bloque." } else { "Funding published, waiting for a block." }.into(),
            Some(tx.clone()),
        ),
        caja::Texto::EsperandoPago(tx) => (
            clase,
            if es { "Pago publicado, esperando un bloque." } else { "Payment published, waiting for a block." }.into(),
            Some(tx.clone()),
        ),
        _ => (clase, t.mostrar(es), None),
    }
}

#[component]
fn VerPartida(
    yo: Signal<Option<Persona>>,
    red: Signal<Option<Nodo>>,
    obras: Signal<Vec<Obra>>,
    sel_obra: Signal<Option<String>>,
    sel_partida: Signal<Option<usize>>,
    screen: Signal<Screen>,
    err: Signal<Option<String>>,
    caja: caja::Caja,
    vista: Signal<caja::CajaVista>,
) -> Element {
    let mut pct = use_signal(|| "100".to_string());
    let mut nota = use_signal(String::new);
    let mut confirma_encerrar = use_signal(|| false);
    let mut detalle_edit = use_signal(String::new);
    let lang = use_context::<Signal<Idioma>>()();
    let es = matches!(lang, Idioma::Es);
    let sec = use_context::<Signal<ClaveSec>>()().0;
    use_effect(move || {
        let id = sel_obra();
        let idx = sel_partida();
        pct.set("100".into());
        nota.set(String::new());
        if let (Some(id), Some(i)) = (id, idx) {
            if let Some(o) = obras.peek().iter().find(|o| o.id == id) {
                if let Some(p) = o.partidas.get(i) {
                    detalle_edit.set(p.detalle.clone());
                }
            }
        }
    });
    let id = sel_obra().unwrap_or_default();
    let Some(obra) = obras().into_iter().find(|o| o.id == id) else {
        return rsx! { p { {lang.t("No está esa obra.", "That job is not here.")} } };
    };
    let Some(i) = sel_partida() else {
        return rsx! { p { {lang.t("Elegí una partida.", "Pick a stage.")} } };
    };
    let Some(p) = obra.partidas.get(i).cloned() else {
        return rsx! { p { {lang.t("No está esa partida.", "That stage is not here.")} } };
    };
    let mid = yo().map(|x| x.id).unwrap_or_default();
    if !obra.participa(&mid) {
        return rsx! { p { {lang.t("Esta obra es de otras dos personas.", "This job belongs to two other people.")} } };
    }
    let soy_m = obra.mandante.id == mid;
    let soy_c = obra.contratista.id == mid;
    let espera_nom = match p.turno {
        Some(Rol::Mandante) => obra.mandante.nombre.clone(),
        Some(Rol::Contratista) => obra.contratista.nombre.clone(),
        None => String::new(),
    };
    let titulo = lang.titulo_partida(i, &p.detalle);
    let label = lang.label_partida(&p);
    let activa = obra.activa() == Some(i);
    let contra = obra.estado == EstadoObra::Contra;
    let garantia = obra.garantia;
    let propuesto = p.propuesto;
    let cerrado = p.estado == PartidaEstado::Pagada;
    let _ = PRECIO();
    let usd = obra.moneda == Moneda::Usd;
    let xmr_linea = if usd { caja::xmr_partida(es, &obra, i) } else { None };
    let aviso_precio = p.precio.as_ref().and_then(|pr| caja::aviso_diferencia_precio(es, pr));
    let xmr_pagado = p.precio.as_ref().zip(p.pago).map(|(pr, n)| caja::fmt_xmr(pr.piconero_pct(n)));
    let cortada = matches!(
        obra.estado,
        EstadoObra::Abandonada | EstadoObra::Cerrada | EstadoObra::Rechazada
    );
    let n_nota = nota().chars().count();
    let soy_prop_enc = p
        .encerrado_por
        .as_ref()
        .map(|q| q.id == mid)
        .unwrap_or(false);
    // Una sola regla (caja.rs) decide qué botones existen; Android usa la misma.
    let lineas = vista().lineas_de(&obra.id, i);
    let acc = caja::acciones_partida_con(&obra, i, &mid, &lineas, vista().traba(&obra, i));
    let traba_txt = caja::texto_traba(acc.traba, obra.contratista.id == mid, es);
    let pagando = matches!(acc.en_curso, caja::EnCurso::PagoFirmando | caja::EnCurso::PagoEnRed);
    let en_curso_txt = caja::en_curso_corto(acc.en_curso, es);
    let lineas_vis: Vec<(&'static str, String, Option<String>)> =
        lineas.iter().map(|t| linea_estado(t, es)).collect();
    let sincronizando = !cortada && sincronizando_trato(red, yo, &obra);
    let notas_vis: Vec<(String, String, bool)> = p
        .notas
        .iter()
        .map(|n| {
            let cabeza = format!(
                "{} · {}% · {}",
                n.autor_nombre,
                n.porcentaje,
                lang.fmt_cuando(n.cuando)
            );
            match obra.leer_nota(n, &sec) {
                TextoLeido::Plano(t) => (cabeza, t, false),
                TextoLeido::Cerrado => (
                    cabeza,
                    lang.t("Nota cifrada", "Encrypted note").into(),
                    true,
                ),
            }
        })
        .collect();
    let nom_obra = obra.nombre.clone();
    let saldo = caja::saldo_partida(es, p.estado, obra.piconero_partida(i), p.fondeo_txid.is_some(), &obra.mandante.nombre, &obra.contratista.nombre);
    // Botón "solo este equipo": a la vista si el motor se frenó, si no en Avanzado.
    let salir_btn = {
        let obra = obra.clone();
        let caja = caja.clone();
        let clase = if acc.frenado { "btn btn-danger" } else { "btn btn-danger btn-sm" };
        rsx! {
            button {
                class: clase,
                onclick: move |_| {
                    let mut obra = obra.clone();
                    let Some(quien) = yo() else { return };
                    let Some(nodo) = red() else { return };
                    caja.cancelar_fondeo(&obra.id, i);
                    if obra.partidas.get(i).map(|p| p.estado == PartidaEstado::Encerrando).unwrap_or(false) {
                        match obra.encerrar_cancelar(i, &quien) {
                            Ok(()) => {
                                publicar_trato(&nodo, obra.clone(), yo(), err);
                            }
                            Err(e) => {
                                err.set(Some(lang_now().error(&e)));
                                return;
                            }
                        }
                    }
                    let peer = if obra.mandante.id == quien.id {
                        obra.contratista.id.clone()
                    } else {
                        obra.mandante.id.clone()
                    };
                    let cuerpo = i.to_string();
                    let _ = nodo.enviar_caja(&obra.id, &peer, &quien.id, "partida-salida", cuerpo.as_bytes());
                    err.set(None);
                },
                {lang.t("Abandonar partida (solo este equipo)", "Leave stage (this device only)")}
            }
            p { class: "help",
                {lang.t(
                    "Cancela fondeo o propuesta locales. No mueve monedas ni firma por el otro. Si ya está Encerrada en cadena, la caja sigue.",
                    "Cancels local funding or proposal. Does not move coins or sign for the other side. If already Locked on-chain, the box stays.",
                )}
            }
        }
    };
    let cancelar_enc = {
        let obra = obra.clone();
        move |_: Event<MouseData>| {
            let mut obra = obra.clone();
            if !exigir_sesion(red, yo, &obra, err) {
                return;
            }
            let Some(quien) = yo() else { return };
            let Some(nodo) = red() else { return };
            match obra.encerrar_cancelar(i, &quien) {
                Ok(()) => {
                    err.set(None);
                    publicar_trato(&nodo, obra.clone(), yo(), err);
                }
                Err(e) => err.set(Some(lang_now().error(&e))),
            }
        }
    };
    rsx! {
        div { class: "pane",
            div { class: "migas",
                button {
                    onclick: move |_| screen.set(Screen::Detalle),
                    "← {nom_obra}"
                }
                span { "/" }
                span { {lang.t("Partida", "Stage")} " {i + 1}" }
            }
            div { class: "cabeza",
                h1 { "{i + 1}  {titulo}" }
                span { class: chip_partida(p.estado), "{label}" }
                if let Some(t) = en_curso_txt {
                    span { class: "chip chip-wait", "{t}" }
                } else if let Some(t) = caja::traba_corta(acc.traba, es) {
                    span { class: "chip chip-wait", "{t}" }
                } else if acc.me_toca {
                    span { class: "chip chip-info", {lang.t("Te toca", "Your turn")} }
                }
            }
            p { class: "lead",
                {match lang { Idioma::Es => format!("{} por lado · mandante {} · contratista {}", mm(obra.moneda, p.capital(garantia), lang), obra.mandante.nombre, obra.contratista.nombre), Idioma::En => format!("{} per side · client {} · contractor {}", mm(obra.moneda, p.capital(garantia), lang), obra.mandante.nombre, obra.contratista.nombre) }}
            }
            if let Some(x) = xmr_linea.clone() {
                p { class: if p.precio.is_some() { "lead xmr fijo" } else { "lead xmr" }, "{x}" }
            }
            div { class: "cols",
                div { class: "col",
                    section { class: "panel foco",
                        div { class: "panel-h",
                            h2 { {lang.t("Ahora", "Now")} }
                        }
                        for (clase, texto, tx) in lineas_vis {
                            div { class: "{clase}",
                                span { "{texto}" }
                                if let Some(tx) = tx {
                                    span { class: "mono", "{tx}" }
                                }
                            }
                        }
                        if sincronizando {
                            p { class: "estado wait", {lang.t("Sincronizando el trato… las acciones esperan a bajar el estado del otro.", "Syncing the deal… actions wait until the other side's state arrives.")} }
                        }
                        if cortada {
                            p { class: "estado info", {lang.t("La obra está cortada. Esta partida ya no tiene acciones.", "The job is cut. This stage has no actions left.")} }
                        }
                        // ── Pagada ──
                        if cerrado {
                            if let Some(r) = p.recibo.as_ref() {
                                div { class: "recibo",
                                    strong { "{lang.t(\"Recibo\", \"Receipt\")} · {r.titulo}" }
                                    p { {match lang { Idioma::Es => format!("Pagó {}% · {} · aceptó {} · {}", r.porcentaje, mm(obra.moneda, r.monto, lang), r.acepto_nombre, lang.fmt_cuando(r.cuando)), Idioma::En => format!("Paid {}% · {} · accepted by {} · {}", r.porcentaje, mm(obra.moneda, r.monto, lang), r.acepto_nombre, lang.fmt_cuando(r.cuando)) }} }
                                    if let Some(x) = xmr_pagado.clone() {
                                        p { class: "help", {match lang { Idioma::Es => format!("Al contratista: {x} XMR del monto fijo de la partida."), Idioma::En => format!("To the contractor: {x} XMR of the stage's fixed amount.") }} }
                                    }
                                }
                            } else {
                                p { class: "estado ok",
                                    {match lang { Idioma::Es => format!("Cerró al {}% ({}). El hilo quedó guardado.", p.pago.unwrap_or(0), mm(obra.moneda, monto_pct(garantia, p.pago.unwrap_or(0)), lang)), Idioma::En => format!("Closed at {}% ({}). The thread was saved.", p.pago.unwrap_or(0), mm(obra.moneda, monto_pct(garantia, p.pago.unwrap_or(0)), lang)) }}
                                }
                            }
                        }
                        // ── Pendiente ──
                        if !cortada && p.estado == PartidaEstado::Pendiente {
                            if contra {
                                p { class: "estado info", {lang.t("Primero hay que confirmar la contra de la obra.", "The job counter has to be confirmed first.")} }
                            } else if !activa {
                                p { class: "estado info", {lang.t("Todavía no toca. Cerrá la partida que está en curso.", "Not this one yet. Close the stage that is underway.")} }
                            } else if acc.proponer_encierre {
                                p { class: "help", {lang.t("Encerrar pone la garantía de los dos en la caja 2-de-2. Los dos tienen que confirmar y estar en línea.", "Locking puts both guarantees in the 2-of-2 box. Both have to confirm and be online.")} }
                                if usd {
                                    p { class: "help", {lang.t("Al proponer, el XMR de esta partida queda fijo con el precio de ahora. El otro lo ve antes de confirmar.", "When you propose, this stage's XMR is fixed at the current price. The other person sees it before confirming.")} }
                                    p { class: "help", "{caja::estado_cotizacion(es)}" }
                                }
                                if confirma_encerrar() {
                                    div { class: "acciones",
                                        button {
                                            class: "btn btn-primary",
                                            onclick: {
                                                let obra = obra.clone();
                                                move |_| {
                                                    let mut obra = obra.clone();
                                                    if !exigir_sesion(red, yo, &obra, err) {
                                                        return;
                                                    }
                                                    let Some(quien) = yo() else { return };
                                                    let Some(nodo) = red() else { return };
                                                    // Obras en USD: el XMR queda fijo con el precio de ahora.
                                                    let precio = match caja::precio_para_encerrar(&obra, i) {
                                                        Ok(pr) => pr,
                                                        Err(e) => {
                                                            err.set(Some(caja::aviso_humano(&e, lang_now() == Idioma::Es)));
                                                            return;
                                                        }
                                                    };
                                                    match obra.encerrar_proponer_con(i, &quien, precio) {
                                                        Ok(()) => {
                                                            err.set(None);
                                                            confirma_encerrar.set(false);
                                                            publicar_trato(&nodo, obra.clone(), yo(), err);
                                                        }
                                                        Err(e) => err.set(Some(lang_now().error(&e))),
                                                    }
                                                }
                                            },
                                            {lang.t("Sí, proponer encerrar", "Yes, propose lock")}
                                        }
                                        button {
                                            class: "btn btn-ghost",
                                            onclick: move |_| confirma_encerrar.set(false),
                                            {lang.t("No", "No")}
                                        }
                                    }
                                } else {
                                    button {
                                        class: "btn btn-primary",
                                        onclick: move |_| confirma_encerrar.set(true),
                                        {lang.t("Encerrar esta partida", "Lock this stage")}
                                    }
                                }
                            }
                        }
                        // ── Encerrando ──
                        if !cortada && p.estado == PartidaEstado::Encerrando {
                            if soy_prop_enc && !acc.frenado && acc.en_curso == caja::EnCurso::Nada {
                                p { class: "estado wait", {lang.t("Esperando que el otro confirme el encierre.", "Waiting for the other person to confirm the lock.")} }
                            }
                            if acc.confirmar_fondeo {
                                p { class: "estado info", {lang.t("El otro quiere encerrar esta partida. Confirmar arma una sola transacción con los dos.", "The other person wants to lock this stage. Confirm builds one transaction from both wallets.")} }
                                if let Some(pr) = p.precio.as_ref() {
                                    p { class: "estado info", {match lang { Idioma::Es => format!("Precio que propone: cada lado pone {}. Confirmar acepta ese precio.", caja::texto_precio_fijado(true, pr)), Idioma::En => format!("Proposed price: each side puts in {}. Confirming accepts that price.", caja::texto_precio_fijado(false, pr)) }} }
                                }
                                if let Some(a) = aviso_precio.clone() {
                                    p { class: "estado wait", "{a}" }
                                }
                            }
                            div { class: "acciones",
                                if acc.confirmar_fondeo {
                                    button {
                                        class: "btn btn-primary",
                                        onclick: {
                                            let obra = obra.clone();
                                            let caja = caja.clone();
                                            move |_| {
                                                if !exigir_sesion(red, yo, &obra, err) {
                                                    return;
                                                }
                                                let Some(quien) = yo() else { return };
                                                match caja.pedir_fondeo(&obra, i, &quien) {
                                                    Ok(()) => err.set(None),
                                                    Err(e) => err.set(Some(caja::aviso_humano(&e, lang_now() == Idioma::Es))),
                                                }
                                            }
                                        },
                                        {lang.t("Confirmar y fondear", "Confirm and fund")}
                                    }
                                }
                                if acc.empezar_fondeo_de_nuevo {
                                    button {
                                        class: "btn btn-primary",
                                        onclick: {
                                            let obra = obra.clone();
                                            let caja = caja.clone();
                                            move |_| {
                                                if !exigir_sesion(red, yo, &obra, err) {
                                                    return;
                                                }
                                                let Some(quien) = yo() else { return };
                                                let Some(nodo) = red() else { return };
                                                match caja.empezar_fondeo_de_nuevo(&obra, i, &quien, &nodo) {
                                                    Ok(()) => err.set(None),
                                                    Err(e) => err.set(Some(caja::aviso_humano(&e, lang_now() == Idioma::Es))),
                                                }
                                            }
                                        },
                                        {lang.t("Empezar el fondeo de nuevo", "Start funding again")}
                                    }
                                }
                                if acc.cancelar_propuesta {
                                    button {
                                        class: "btn btn-ghost",
                                        onclick: cancelar_enc.clone(),
                                        {lang.t("Cancelar propuesta", "Cancel proposal")}
                                    }
                                }
                                if acc.no_encerrar {
                                    button {
                                        class: "btn btn-ghost",
                                        onclick: cancelar_enc.clone(),
                                        {lang.t("No encerrar", "Do not lock")}
                                    }
                                }
                            }
                            if acc.en_curso == caja::EnCurso::FondeoEnRed {
                                p { class: "help", {lang.t("El fondeo ya está en la red. No se puede cancelar; se encierra solo cuando entra en un bloque.", "The funding is already on the network. It cannot be cancelled; it locks by itself once it is in a block.")} }
                            }
                        }
                        // ── Encerrada ──
                        if !cortada && p.estado == PartidaEstado::Encerrada {
                            if acc.avisar_termino {
                                p { class: "help", {lang.t("Cuando termines, avisá y proponé cuánto se paga. El mandante acepta o contraoferta.", "When you finish, report it and propose how much is paid. The client accepts or counters.")} }
                                div { class: "parejas-in",
                                    label { class: "et", {lang.t("PORCENTAJE A COBRAR", "PERCENT TO CHARGE")} }
                                    input {
                                        r#type: "text",
                                        value: "{pct}",
                                        oninput: move |e| pct.set(e.value()),
                                    }
                                    label { class: "et", "{lang.t(\"NOTA\", \"NOTE\")} ({n_nota}/{MAX_NOTA})" }
                                    input {
                                        r#type: "text",
                                        placeholder: lang.t("Terminé las fundaciones", "Foundations are done"),
                                        value: "{nota}",
                                        oninput: move |e| nota.set(recorta_nota(e.value())),
                                    }
                                }
                                button {
                                    class: "btn btn-primary",
                                    onclick: {
                                        let obra = obra.clone();
                                        move |_| {
                                            let mut obra = obra.clone();
                                            if !exigir_sesion(red, yo, &obra, err) {
                                                return;
                                            }
                                            let Some(quien) = yo() else { return };
                                            let Some(nodo) = red() else { return };
                                            match obra.avisar_termino(i, &quien, parse_pct(&pct()), nota()) {
                                                Ok(()) => {
                                                    err.set(None);
                                                    publicar_trato(&nodo, obra.clone(), yo(), err);
                                                }
                                                Err(e) => err.set(Some(lang_now().error(&e))),
                                            }
                                        }
                                    },
                                    {lang.t("Avisar que terminé", "Report that I finished")}
                                }
                            } else if let Some(t) = traba_txt.clone() {
                                p { class: "estado wait", "{t}" }
                                if !soy_m {
                                    button {
                                        class: "btn btn-primary",
                                        disabled: true,
                                        title: "{t}",
                                        {lang.t("Avisar que terminé", "Report that I finished")}
                                    }
                                }
                                p { class: "help", {lang.t("El fondeo tiene que juntar 10 confirmaciones antes de que la caja pueda pagar. El aviso se suelta solo.", "The funding needs 10 confirmations before the escrow can pay. This clears by itself.")} }
                            } else if soy_m {
                                p { class: "estado info", {lang.t("Encerrada. El contratista avisa cuando termina y propone cuánto se paga.", "Locked. The contractor reports when they finish and proposes how much is paid.")} }
                            }
                        }
                        // ── En trato ──
                        if !cortada && p.estado == PartidaEstado::EnTrato {
                            if let Some(n) = propuesto {
                                dl { class: "datos",
                                    dt { {lang.t("Sobre la mesa", "On the table")} }
                                    dd { strong { "{n}%" } " · {mm(obra.moneda, monto_pct(garantia, n), lang)}" }
                                    if !espera_nom.is_empty() {
                                        dt { {lang.t("Responde", "Answers")} }
                                        dd { if acc.me_toca { {lang.t("vos", "you")} } else { "{espera_nom}" } }
                                    }
                                }
                            }
                            if pagando {
                                p { class: "estado wait",
                                    {match lang {
                                        Idioma::Es => format!("Pago del {}% en curso. No hace falta volver a aceptar; se cierra cuando la transacción entra en un bloque.", propuesto.unwrap_or(0)),
                                        Idioma::En => format!("Payment of {}% in progress. No need to accept again; it closes once the transaction is in a block.", propuesto.unwrap_or(0)),
                                    }}
                                }
                            } else if !acc.me_toca {
                                p { class: "estado wait", {match lang { Idioma::Es => format!("Esperando a {espera_nom}."), Idioma::En => format!("Waiting for {espera_nom}.") }} }
                            }
                            if let Some(t) = traba_txt.clone() {
                                if !pagando {
                                    p { class: "estado wait", "{t}" }
                                    if acc.me_toca {
                                        button {
                                            class: "btn btn-primary",
                                            disabled: true,
                                            title: "{t}",
                                            {match lang { Idioma::Es => format!("Aceptar {}% y pagar", propuesto.unwrap_or(0)), Idioma::En => format!("Accept {}% and pay", propuesto.unwrap_or(0)) }}
                                        }
                                    }
                                }
                            }
                            if acc.aceptar_pago {
                                button {
                                    class: "btn btn-primary",
                                    onclick: {
                                        let obra = obra.clone();
                                        let caja = caja.clone();
                                        move |_| {
                                            if !exigir_sesion(red, yo, &obra, err) {
                                                return;
                                            }
                                            let Some(quien) = yo() else { return };
                                            match caja.pedir_gasto(&obra, i, &quien) {
                                                Ok(()) => err.set(None),
                                                Err(e) => err.set(Some(caja::aviso_humano(&e, lang_now() == Idioma::Es))),
                                            }
                                        }
                                    },
                                    {match lang { Idioma::Es => format!("Aceptar {}% y pagar", propuesto.unwrap_or(0)), Idioma::En => format!("Accept {}% and pay", propuesto.unwrap_or(0)) }}
                                }
                            }
                            if acc.contraofertar {
                                details { class: "plegable",
                                    summary { {lang.t("Proponer otro porcentaje", "Propose another percentage")} }
                                    div { class: "cuerpo",
                                        label { class: "et", {lang.t("OTRO PORCENTAJE", "OTHER PERCENT")} }
                                        input {
                                            r#type: "text",
                                            value: "{pct}",
                                            oninput: move |e| pct.set(e.value()),
                                        }
                                        label { class: "et", "{lang.t(\"NOTA\", \"NOTE\")} ({n_nota}/{MAX_NOTA})" }
                                        input {
                                            r#type: "text",
                                            placeholder: lang.t("Falta la entrada de auto", "The driveway is missing"),
                                            value: "{nota}",
                                            oninput: move |e| nota.set(recorta_nota(e.value())),
                                        }
                                        button {
                                            class: "btn btn-ghost",
                                            onclick: {
                                                let obra = obra.clone();
                                                move |_| {
                                                    let mut obra = obra.clone();
                                                    if !exigir_sesion(red, yo, &obra, err) {
                                                        return;
                                                    }
                                                    let Some(quien) = yo() else { return };
                                                    let Some(nodo) = red() else { return };
                                                    match obra.contra_pago(i, &quien, parse_pct(&pct()), nota()) {
                                                        Ok(()) => {
                                                            err.set(None);
                                                            publicar_trato(&nodo, obra.clone(), yo(), err);
                                                        }
                                                        Err(e) => err.set(Some(lang_now().error(&e))),
                                                    }
                                                }
                                            },
                                            {lang.t("Proponer este porcentaje", "Propose this percentage")}
                                        }
                                    }
                                }
                            }
                        }
                        if acc.salir_local && acc.frenado {
                            {salir_btn.clone()}
                        }
                    }
                    section { class: "panel",
                        h2 { {lang.t("Hilo", "Thread")} }
                        if notas_vis.is_empty() {
                            p { class: "help", {lang.t("Todavía no hay notas. Cada aviso y contraoferta deja una acá.", "No notes yet. Each report and counteroffer leaves one here.")} }
                        } else {
                            div { class: "notas",
                                for (cabeza, cuerpo, cifrada) in notas_vis {
                                    div { class: "nota",
                                        strong { "{cabeza}" }
                                        if cifrada {
                                            p { class: "cifrada", "{cuerpo}" }
                                        } else if !cuerpo.is_empty() {
                                            p { "{cuerpo}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if acc.editar_texto {
                        details { class: "plegable",
                            summary { {lang.t("Texto de la partida", "Stage text")} }
                            div { class: "cuerpo",
                                p { class: "help", {lang.t("Se puede cambiar mientras la partida está pendiente. El otro ve el cambio.", "It can change while the stage is pending. The other person sees the change.")} }
                                input {
                                    r#type: "text",
                                    value: "{detalle_edit}",
                                    oninput: move |e| detalle_edit.set(e.value()),
                                }
                                button {
                                    class: "btn btn-ghost",
                                    onclick: {
                                        let obra = obra.clone();
                                        move |_| {
                                            let mut obra = obra.clone();
                                            let Some(quien) = yo() else { return };
                                            let Some(nodo) = red() else { return };
                                            match obra.editar_detalle(i, &quien, detalle_edit()) {
                                                Ok(()) => {
                                                    err.set(None);
                                                    publicar_trato(&nodo, obra.clone(), yo(), err);
                                                }
                                                Err(e) => err.set(Some(lang_now().error(&e))),
                                            }
                                        }
                                    },
                                    {lang.t("Guardar texto", "Save text")}
                                }
                            }
                        }
                    }
                }
                div { class: "col",
                    section { class: "panel",
                        h2 { {lang.t("Caja y transacciones", "Box and transactions")} }
                        if let Some(s) = saldo {
                            p { strong { "{s.estado}" } }
                            p { "{s.detalle}" }
                            if let Some(c) = s.candado {
                                p { class: "help", "{c}" }
                            }
                        } else if let Some(x) = caja::xmr_partida(es, &obra, i) {
                            p { "{x}" }
                        }
                        CajaLlave { obra_id: obra.id.clone(), caja: caja.clone(), vista }
                        if p.fondeo_txid.is_some() || p.pago_txid.is_some() || p.encerrado_por.is_some() {
                            dl { class: "datos",
                                if let Some(tx) = p.fondeo_txid.as_ref() {
                                    dt { {lang.t("Fondeo", "Funding")} }
                                    dd { span { class: "mono", "{tx}" } }
                                }
                                if let Some(tx) = p.pago_txid.as_ref() {
                                    dt { {lang.t("Pago", "Payment")} }
                                    dd { span { class: "mono", "{tx}" } }
                                }
                                if let Some(q) = p.encerrado_por.as_ref() {
                                    dt { {lang.t("Encerró", "Locked by")} }
                                    dd { "{q.nombre} · {lang.fmt_cuando(p.encerrado_cuando)}" }
                                }
                            }
                        }
                    }
                    if !cortada && (soy_m || soy_c) {
                        details { class: "plegable",
                            summary { {lang.t("Avanzado", "Advanced")} }
                            div { class: "cuerpo",
                                CajaRespaldo { obra_id: obra.id.clone(), caja: caja.clone(), vista, yo, obras, err }
                                div { class: "grupo",
                                    CajaMirar { obra_id: obra.id.clone(), caja: caja.clone(), vista, err }
                                }
                                if acc.salir_local && !acc.frenado {
                                    div { class: "grupo", {salir_btn} }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use konstruado_core::NotaPartida;

    fn par(nombre: &str) -> (Persona, String) {
        let mut p = Persona::nueva(nombre).unwrap();
        let (sec, pubk) = konstruado_core::generar_clave();
        p.clave_pub = pubk;
        (p, sec)
    }

    fn obra_con_nota(m: &Persona, c: &Persona, texto: &str) -> Obra {
        let o = Oferta::publicar(m.clone(), "Casa", 10_000, 2_000, vec!["Muro".into()]).unwrap();
        let a = Aceptacion::de(&o, c.clone(), 2_000).unwrap();
        let mut obra = Obra::desde_oferta(o, a).unwrap();
        obra.partidas[0].notas.push(NotaPartida {
            autor_id: c.id.clone(),
            autor_nombre: c.nombre.clone(),
            porcentaje: 100,
            texto: texto.into(),
            cuando: 0,
            caja: String::new(),
        });
        obra
    }

    #[test]
    fn el_fondeo_no_republica_una_nota_en_claro() {
        let (m, _) = par("Alice");
        let (c, cs) = par("Bob");
        let mut obra = obra_con_nota(&m, &c, "Terminé el muro");
        assert!(obra.preparar_para_red(&c.id, &c.clave_pub, &cs).is_ok());
        assert!(obra.partidas[0].notas[0].texto.is_empty());
        assert!(!obra.partidas[0].notas[0].caja.is_empty());
        assert!(obra.preparar_para_red(&c.id, &c.clave_pub, &cs).is_ok());

        let mut m = m;
        m.clave_pub.clear();
        let mut clara = obra_con_nota(&m, &c, "Terminé el muro");
        assert!(!clara.preparar_para_red(&c.id, &c.clave_pub, &cs).is_ok());
        assert_eq!(clara.partidas[0].notas[0].texto, "Terminé el muro");
        assert!(clara.partidas[0].notas[0].caja.is_empty());
    }
}
