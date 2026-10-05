mod caja;
mod export;
mod help;
mod i18n;
mod persist;

use std::time::Duration;

use dioxus::prelude::*;
use konstruado_core::{
    asegurar_clave, monto, monto_pct, n_partidas, oferta_en_tablero, Aceptacion, EstadoObra, Oferta,
    Obra, PartidaEstado, Persona, Rol, TextoLeido, MAX_NOTA,
};
use konstruado_net::{EstadoTor, Nodo, RED};
use i18n::Idioma;
const CSS: &str = include_str!("ui.css");

fn main() {
    preparar_grafica();
    let window = dioxus::desktop::WindowBuilder::new()
        .with_title(concat!("Konstruado ", env!("CARGO_PKG_VERSION")))
        .with_inner_size(dioxus::desktop::LogicalSize::new(1100.0, 760.0))
        .with_min_inner_size(dioxus::desktop::LogicalSize::new(420.0, 560.0));
    let cfg = dioxus::desktop::Config::new()
        .with_window(window)
        .with_menu(help::menu());
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
        obras: n.obras(),
        presentes: n.presentes(),
        tema,
        idioma,
        clave_sec,
        spend_sec,
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
    let trabajo = use_signal(|| "10000".to_string());
    let garantia = use_signal(|| "2000".to_string());
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
                n.hidratar(ofertas0, obras0, presentes0);
                if let Some(p) = yo() {
                    n.actualizar_yo(p);
                }
                red.set(Some(n.clone()));
                loop {
                    tor.set(n.estado_tor());
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
        .filter(|o| o.estado != EstadoObra::Rechazada)
        .filter(|o| o.participa(&mid))
        .collect();
    mis_obras.sort_by(|a, b| b.actualizado.cmp(&a.actualizado).then(b.id.cmp(&a.id)));

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
                        class: "top-billetera",
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
                            class: "side-wallet",
                            onclick: move |_| screen.set(Screen::Billetera),
                            {lang.t("Billetera", "Wallet")}
                        }
                        h2 { {lang.t("Mis obras", "My jobs")} }
                        div { class: "side-list",
                            for o in mis_obras {
                                button {
                                    class: "side-item",
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
                        div { class: "err", "{e}" }
                    }
                    match screen() {
                        Screen::Bienvenida => rsx! {
                            Bienvenida { nombre, rol, yo, red, screen, err }
                        },
                        Screen::Tablero => rsx! {
                            Tablero {
                                yo, rol, red, ofertas, obras, presentes, screen, sel_oferta, sel_obra,
                                sel_partida, tor, peers, garantia_acc
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
                            Billetera { yo, obras, screen, err, caja: caja_ui.clone(), vista: vista_caja }
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

/// Sella notas y extras que quedaron en claro. Si falta la clave del otro, la obra no se toca.
fn sellar_guardadas(nodo: &Nodo, yo: &Persona, sec: &str) {
    for obra in nodo.obras() {
        if !obra.participa(&yo.id) {
            continue;
        }
        let mut sealed = obra.clone();
        if lista_para_publicar(&mut sealed, yo, sec) && sealed != obra {
            nodo.publicar_obra(sealed);
        }
    }
}

/// True cuando la obra ya no tiene texto en claro. Si no se puede sellar, queda como estaba.
fn lista_para_publicar(obra: &mut Obra, yo: &Persona, sec: &str) -> bool {
    obra.preparar_para_red(&yo.id, &yo.clave_pub, sec).is_ok()
}

fn aplicar_monero(nodo: &Nodo, yo: &Persona, sec: &str, hechos: &[caja::Hecho]) {
    for h in hechos {
        let mut obras = nodo.obras();
        let Some(obra) = obras.iter_mut().find(|o| o.id == h.obra) else {
            continue;
        };
        let otro = if yo.id == obra.mandante.id {
            obra.contratista.id.clone()
        } else {
            obra.mandante.id.clone()
        };
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
        if publico && lista_para_publicar(obra, yo, sec) {
            nodo.publicar_obra(obra.clone());
        }
    }
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
                        monto(obra.garantia)
                    ),
                    Idioma::En => format!(
                        "{}: {} proposes guarantee {}",
                        obra.nombre,
                        obra.contratista.nombre,
                        monto(obra.garantia)
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
                            obra.nombre, ex.por.nombre, detalle, monto(ex.monto)
                        ),
                        Idioma::En => format!(
                            "{}: {} proposes extra {} ({})",
                            obra.nombre, ex.por.nombre, detalle, monto(ex.monto)
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
    rsx! {
        div { class: "pane narrow",
            h1 { {lang.t("La obra, con el dinero encerrado.", "The job, with the money locked.")} }
            p { class: "lead",
                {lang.t("No te ves con la otra persona como en un chat. El mandante publica una obra. El contratista la ve en el tablero y acepta (o propone otra garantía).", "You do not see the other person like a chat. The client posts a job. The contractor sees it on the board and accepts (or proposes another guarantee).")}
            }
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
    let caja_palabras = caja.clone();
    let caja_crear = caja.clone();
    rsx! {
        div { class: "pane narrow",
            h1 { {lang.t("Tu cuenta", "Your account")} }
            p { class: "lead",
                {lang.t("El nombre y el rol se pueden cambiar. Las obras no se borran. El mandante abre la sala; el contratista solo busca.", "Name and role can be changed. Jobs are not deleted. The client opens the room; the contractor only looks.")}
            }
            label { class: "et", {lang.t("NOMBRE", "NAME")} }
            input {
                r#type: "text",
                value: "{nom}",
                oninput: move |e| nom.set(e.value()),
            }
            div { class: "paso", b { "2" } {lang.t("¿Qué vas a hacer?", "What will you do?")} }
            div { class: "roles",
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
            div { class: "paso", b { "3" } {lang.t("Apariencia", "Look")} }
            div { class: "roles",
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
            div { class: "paso", b { "4" } {lang.t("Idioma", "Language")} }
            div { class: "roles",
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
            div { class: "paso", b { "5" } "Stagenet" }
            p { class: "hint", "{vista().daemon}" }
            button {
                class: "btn btn-ghost",
                onclick: move |_| screen.set(Screen::Billetera),
                {lang.t("Abrir billetera", "Open wallet")}
            }
            if let Some(tip) = vista().tip {
                p { class: "meta", {match lang { Idioma::Es => format!("Punta del nodo: {tip}"), Idioma::En => format!("Node tip: {tip}") }} }
            }
            p { class: "hint", "{caja::escala(matches!(lang, Idioma::Es))}" }
            if let Some(addr) = vista().personal.clone() {
                p { class: "meta", "{addr}" }
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| {
                        let Some(path) = rfd::FileDialog::new()
                            .set_file_name("konstruado-semilla.txt")
                            .save_file()
                        else {
                            return;
                        };
                        match caja_palabras.guardar_palabras(&path) {
                            Ok(()) => err.set(None),
                            Err(e) => err.set(Some(e)),
                        }
                    },
                    {lang.t("Guardar las 25 palabras", "Save the 25 words")}
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
            RestaurarLlaves { caja: caja.clone(), yo, obras, vista, err }
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
                {lang.t("Guardar", "Save")}
            }
            button {
                class: "btn btn-ghost",
                onclick: move |_| screen.set(Screen::Tablero),
                {lang.t("Volver", "Back")}
            }
        }
    }
}

#[component]
fn Billetera(
    yo: Signal<Option<Persona>>,
    obras: Signal<Vec<Obra>>,
    screen: Signal<Screen>,
    mut err: Signal<Option<String>>,
    caja: caja::Caja,
    vista: Signal<caja::CajaVista>,
) -> Element {
    let mut destino = use_signal(String::new);
    let mut monto = use_signal(String::new);
    let lang = use_context::<Signal<Idioma>>()();
    let b = vista().billetera.clone();
    let addr = vista().personal.clone();
    let caja_palabras = caja.clone();
    let caja_envio = caja.clone();
    let caja_act = caja.clone();
    let caja_atras = caja.clone();
    let caja_crear = caja.clone();
    rsx! {
        div { class: "pane narrow",
            h1 { {lang.t("Billetera", "Wallet")} }
            p { class: "lead",
                {lang.t(
                    "Tu Monero personal de stagenet. La caja de una obra es otra dirección, de las dos personas.",
                    "Your personal stagenet Monero. A job's box is a different address, shared by both people.",
                )}
            }
            p { class: "hint", "{vista().daemon}" }
            if let Some(addr) = addr {
                p { class: "saldo",
                    "{caja::fmt_xmr(b.total)}"
                    small { "XMR" }
                }
                p { class: "meta",
                    {match lang {
                        Idioma::Es => format!(
                            "Libre {} · trabado {} (10 bloques)",
                            caja::fmt_xmr(b.libre),
                            caja::fmt_xmr(b.trabado),
                        ),
                        Idioma::En => format!(
                            "Unlocked {} · locked {} (10 blocks)",
                            caja::fmt_xmr(b.libre),
                            caja::fmt_xmr(b.trabado),
                        ),
                    }}
                }
                if let Some(tip) = vista().tip {
                    p { class: "meta",
                        {match lang {
                            Idioma::Es => format!("Punta del nodo: {tip}"),
                            Idioma::En => format!("Node tip: {tip}"),
                        }}
                    }
                }
                if let (Some(desde), Some(hasta)) = (b.desde, b.hasta) {
                    p { class: "hint",
                        {match lang {
                            Idioma::Es => format!("Visto desde el bloque {desde} hasta el {hasta}."),
                            Idioma::En => format!("Scanned from block {desde} through {hasta}."),
                        }}
                    }
                } else {
                    p { class: "hint",
                        {lang.t(
                            "Todavía no miré la cadena. Arranco por los últimos 40 bloques.",
                            "I have not scanned the chain yet. I start with the last 40 blocks.",
                        )}
                    }
                }
                if b.buscando {
                    p { class: "hint", {lang.t("Mirando la cadena…", "Scanning the chain…")} }
                }
                if b.enviando {
                    p { class: "hint", {lang.t("Firmando y publicando…", "Signing and publishing…")} }
                }
                if b.retro > 0 {
                    p { class: "hint",
                        {match lang {
                            Idioma::Es => format!("Quedan {} bloques por mirar hacia atrás.", b.retro),
                            Idioma::En => format!("{} blocks left to scan backward.", b.retro),
                        }}
                    }
                }
                if let Some(aviso) = b.aviso.clone() {
                    p { class: "err", "{caja::aviso_humano(&aviso, matches!(lang, Idioma::Es))}" }
                }
                if let Some(tx) = b.ultimo.clone() {
                    p { class: "meta",
                        {match lang {
                            Idioma::Es => format!(
                                "Último envío {tx}. Fee {} XMR. Cambio {} XMR, vuelve en el próximo bloque.",
                                caja::fmt_xmr(b.ultimo_fee.unwrap_or(0)),
                                caja::fmt_xmr(b.ultimo_cambio.unwrap_or(0)),
                            ),
                            Idioma::En => format!(
                                "Last send {tx}. Fee {} XMR. Change {} XMR, it returns in the next block.",
                                caja::fmt_xmr(b.ultimo_fee.unwrap_or(0)),
                                caja::fmt_xmr(b.ultimo_cambio.unwrap_or(0)),
                            ),
                        }}
                    }
                }
                if !b.movs.is_empty() {
                    div { class: "paso", b { "·" } {lang.t("Entradas", "Outputs")} }
                    for mov in b.movs.iter() {
                        div { class: "mov",
                            span { "{caja::fmt_xmr(mov.monto)} XMR" }
                            span { class: "hint",
                                {match lang {
                                    Idioma::Es => format!(
                                        "bloque {} · {}",
                                        mov.altura,
                                        if mov.libre { "libre" } else { "trabado" }
                                    ),
                                    Idioma::En => format!(
                                        "block {} · {}",
                                        mov.altura,
                                        if mov.libre { "unlocked" } else { "locked" }
                                    ),
                                }}
                            }
                        }
                    }
                }
                div { class: "paso", b { "1" } {lang.t("Recibir", "Receive")} }
                input {
                    class: "addr",
                    r#type: "text",
                    readonly: true,
                    value: "{addr}",
                }
                p { class: "hint",
                    {lang.t(
                        "Seleccioná la dirección y copiala. El scan no ve monedas que tengan más de lo que ya miramos: si el faucet es viejo, pedí mirar más atrás.",
                        "Select the address and copy it. The scan misses coins older than what we already looked at: if the faucet is old, scan further back.",
                    )}
                }
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| {
                        let Some(path) = rfd::FileDialog::new()
                            .set_file_name("konstruado-semilla.txt")
                            .save_file()
                        else {
                            return;
                        };
                        match caja_palabras.guardar_palabras(&path) {
                            Ok(()) => err.set(None),
                            Err(e) => err.set(Some(e)),
                        }
                    },
                    {lang.t("Guardar las 25 palabras", "Save the 25 words")}
                }
                RestaurarLlaves { caja: caja.clone(), yo, obras, vista, err }
                div { class: "paso", b { "2" } {lang.t("Enviar", "Send")} }
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
                p { class: "hint",
                    {lang.t(
                        "El cambio vuelve a esta billetera. Se reserva 0,001 XMR para el fee. Hace falta al menos 1 piconero de cambio.",
                        "Change comes back to this wallet. 0.001 XMR is set aside for the fee. At least 1 piconero of change is required.",
                    )}
                }
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| {
                        match caja::maximo_envio(vista().billetera.libre) {
                            Some(texto) => {
                                monto.set(texto);
                                err.set(None);
                            }
                            None => err.set(Some(lang_now().t(
                                "No hay saldo libre suficiente para el fee.",
                                "There is not enough unlocked balance for the fee.",
                            ).into())),
                        }
                    },
                    {lang.t("Usar el máximo", "Use the maximum")}
                }
                button {
                    class: "btn btn-primary",
                    onclick: move |_| {
                        match caja_envio.pedir_envio(&destino(), &monto()) {
                            Ok(()) => err.set(None),
                            Err(e) => err.set(Some(caja::aviso_humano(&e, lang_now() == Idioma::Es))),
                        }
                    },
                    {lang.t("Enviar", "Send")}
                }
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| caja_act.pedir_actualizacion(),
                    {lang.t("Actualizar saldo", "Refresh balance")}
                }
                button {
                    class: "btn btn-ghost",
                    onclick: move |_| caja_atras.pedir_atras(),
                    {lang.t("Mirar 200 bloques más atrás", "Scan 200 blocks further back")}
                }
            } else {
                p { class: "hint", "{caja::escala(matches!(lang, Idioma::Es))}" }
                p { class: "hint",
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
                RestaurarLlaves { caja: caja.clone(), yo, obras, vista, err }
            }
            button {
                class: "btn btn-ghost",
                onclick: move |_| screen.set(Screen::Tablero),
                {lang.t("Volver", "Back")}
            }
        }
    }
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
) -> Element {
    let lang = use_context::<Signal<Idioma>>()();
    let sec = use_context::<Signal<ClaveSec>>()().0;
    let mut buscando = use_signal(|| false);
    let mid = yo().map(|p| p.id).unwrap_or_default();
    let todas = obras();
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
    let mut mis_obras: Vec<Obra> = todas
        .iter()
        .cloned()
        .filter(|o| o.estado != EstadoObra::Rechazada)
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
    let soy_m = rol() == Some(Rol::Mandante);
    let sin_ajenas = ajenas.is_empty();
    let sin_mias = mias.is_empty() && en_curso.is_empty();
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
            h1 { {lang.t("Tablero", "Board")} }
            p { class: "status",
                {lang.t("Red", "Net")} " " code { "{RED}" } " · {status}"
            }
            if peers() == 0 {
                p { class: "hint",
                    match tor() {
                        EstadoTor::Arrancando { .. } => lang.t("Tor está subiendo. El mandante abre la sala; el contratista solo busca.", "Tor is coming up. The client opens the room; the contractor only looks."),
                        _ => lang.t("Nadie más todavía. En la misma PC, un segundo cargo run se engancha solo. En otra máquina, Don Dinero abre la sala y Chasquilla busca.", "Nobody else yet. On the same PC, a second cargo run joins on its own. On another machine, the client opens the room and the contractor searches."),
                    }
                }
            }

            if !avisos.is_empty() {
                p { class: "lead", {lang.t("Te toca", "Your turn")} }
                div { class: "stack",
                    for a in avisos {
                        button {
                            class: "card aviso",
                            onclick: move |_| {
                                if a.es_oferta {
                                    if let Some(o) = ofertas().into_iter().find(|o| o.id == a.obra_id) {
                                        garantia_acc.set(o.garantia_sugerida.to_string());
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
                                span { class: "chip chip-wait", {lang.t("Te toca", "Your turn")} }
                            }
                        }
                    }
                }
                div { style: "height: 24px;" }
            }

            if soy_m {
                p { class: "lead",
                    {lang.t("Publicá. El contratista no te ve a vos: ve el aviso en su tablero.", "Post a job. The contractor does not see you: they see the notice on their board.")}
                }
                if sin_mias {
                    p { class: "hint", {lang.t("Todavía no publicaste nada.", "You have not posted anything yet.")} }
                }
                div { class: "stack",
                    for o in mias {
                        div { class: "card static",
                            div { class: "card-h",
                                strong { "{o.nombre}" }
                                span { class: "chip chip-off", {lang.t("Esperando contratista", "Waiting for contractor")} }
                            }
                            p { class: "meta",
                                {match lang { Idioma::Es => format!("Trabajo {} · garantía {} · {} partidas", monto(o.trabajo), monto(o.garantia_sugerida), o.n_partidas_sugeridas), Idioma::En => format!("Job {} · guarantee {} · {} stages", monto(o.trabajo), monto(o.garantia_sugerida), o.n_partidas_sugeridas) }}
                            }
                            if let Some(r) = resumen_detalles(&o.detalles) {
                                p { class: "meta", "{r}" }
                            }
                        }
                    }
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
                                {match lang { Idioma::Es => format!("Contratista {} · {} partidas · {} por lado", o.contratista.nombre, o.n_partidas, monto(o.garantia)), Idioma::En => format!("Contractor {} · {} stages · {} per side", o.contratista.nombre, o.n_partidas, monto(o.garantia)) }}
                            }
                            if o.estado == EstadoObra::Contra {
                                p { class: "meta", {lang.t("Te toca: contra de garantía", "Your turn: guarantee counter")} }
                            }
                        }
                    }
                }
                div { style: "height: 24px;" }
                button {
                    class: "btn btn-primary",
                    onclick: move |_| screen.set(Screen::Nueva),
                    {lang.t("Publicar obra", "Post a job")}
                }
            } else {
                p { class: "lead",
                    {lang.t("Ofertas del mandante. Aceptás las condiciones o proponés otra garantía.", "Jobs from the client. Accept the terms or propose another guarantee.")}
                }
                button {
                    class: "btn btn-primary",
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
                div { style: "height: 16px;" }
                div { class: "stack",
                    for o in ajenas {
                        button {
                            class: "card",
                            onclick: move |_| {
                                garantia_acc.set(o.garantia_sugerida.to_string());
                                sel_oferta.set(Some(o.clone()));
                                screen.set(Screen::Oferta);
                            },
                            div { class: "card-h",
                                strong { "{o.nombre}" }
                                span { class: "chip chip-off", {match lang { Idioma::Es => format!("{} partidas", o.n_partidas_sugeridas), Idioma::En => format!("{} stages", o.n_partidas_sugeridas) }} }
                            }
                            p { class: "meta", "{lang.t(\"Mandante\", \"Client\")}: {o.mandante.nombre}" }
                            p { class: "meta",
                                {match lang { Idioma::Es => format!("Trabajo {} · garantía sugerida {}", monto(o.trabajo), monto(o.garantia_sugerida)), Idioma::En => format!("Job {} · suggested guarantee {}", monto(o.trabajo), monto(o.garantia_sugerida)) }}
                            }
                            if let Some(r) = resumen_detalles(&o.detalles) {
                                p { class: "meta", "{r}" }
                            }
                        }
                    }
                }
                if sin_ajenas {
                    p { class: "hint", "{hint_contratista}" }
                }
                if !en_curso.is_empty() {
                    div { style: "height: 24px;" }
                    p { class: "lead", {lang.t("En curso", "In progress")} }
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
                                    {match lang { Idioma::Es => format!("Mandante {} · {} partidas · {} por lado", o.mandante.nombre, o.n_partidas, monto(o.garantia)), Idioma::En => format!("Client {} · {} stages · {} per side", o.mandante.nombre, o.n_partidas, monto(o.garantia)) }}
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
    let t: u64 = trabajo().chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0);
    let g: u64 = garantia().chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0);
    let preview = n_partidas(t, g);
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
        div { class: "pane narrow",
            h1 { {lang.t("Publicar obra", "Post a job")} }
            p { class: "lead", {lang.t("Vos sos el mandante. El contratista va a ver esto en el tablero.", "You are the client. The contractor will see this on the board.")} }
            label { class: "et", {lang.t("NOMBRE", "NAME")} }
            input {
                r#type: "text",
                value: "{obra_nom}",
                oninput: move |e| obra_nom.set(e.value()),
            }
            label { class: "et", {lang.t("TRABAJO", "JOB AMOUNT")} }
            input {
                r#type: "text",
                value: "{trabajo}",
                oninput: move |e| trabajo.set(e.value()),
            }
            label { class: "et", {lang.t("GARANTÍA SUGERIDA", "SUGGESTED GUARANTEE")} }
            input {
                r#type: "text",
                value: "{garantia}",
                oninput: move |e| garantia.set(e.value()),
            }
            p { class: "hint",
                match preview.clone() {
                    Ok(n) => match lang { Idioma::Es => format!("{n} partidas. En cada una los dos encierran {g}."), Idioma::En => format!("{n} stages. In each one both lock {g}.") },
                    Err(e) => lang.error(&e),
                }
            }
            if let Ok(n) = preview {
                div { class: "paso", b { "3" } {lang.t("Qué entra en cada partida", "What each stage covers")} }
                p { class: "hint", {lang.t("Como en un presupuesto: cimientos, muros, techumbre. El texto es opcional.", "Like a quote: foundations, walls, roof. The text is optional.")} }
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
            }
            div { style: "height: 24px;" }
            button {
                class: "btn btn-primary",
                onclick: move |_| {
                    let Some(m) = yo() else { return };
                    let Some(nodo) = red() else {
                        err.set(Some(lang_now().t("La red todavía no arrancó.", "The network has not started yet.").into()));
                        return;
                    };
                    match Oferta::publicar(m, obra_nom(), t, g, detalles()) {
                        Ok(o) => {
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
        let g: u64 = garantia_acc()
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse()
            .unwrap_or(0);
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
    let g: u64 = garantia_acc().chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0);
    let preview = n_partidas(o.trabajo, g);
    let contra = g != o.garantia_sugerida;
    rsx! {
        div { class: "pane narrow",
            h1 { "{o.nombre}" }
            p { class: "lead",
                {match lang { Idioma::Es => format!("{} ofrece trabajo por {}. Garantía sugerida {} ({} partidas).", o.mandante.nombre, monto(o.trabajo), monto(o.garantia_sugerida), o.n_partidas_sugeridas), Idioma::En => format!("{} offers a job for {}. Suggested guarantee {} ({} stages).", o.mandante.nombre, monto(o.trabajo), monto(o.garantia_sugerida), o.n_partidas_sugeridas) }}
            }
            label { class: "et", {lang.t("TU GARANTÍA", "YOUR GUARANTEE")} }
            input {
                r#type: "text",
                value: "{garantia_acc}",
                oninput: move |e| garantia_acc.set(e.value()),
            }
            p { class: "hint",
                match preview.clone() {
                    Ok(n) if contra => match lang { Idioma::Es => format!("Contra: {n} partidas de {g}. El mandante tiene que confirmar."), Idioma::En => format!("Counter: {n} stages of {g}. The client has to confirm.") },
                    Ok(n) => match lang { Idioma::Es => format!("Aceptás {n} partidas. Los dos encierran {g} en cada una."), Idioma::En => format!("You accept {n} stages. Both lock {g} in each one.") },
                    Err(e) => lang.error(&e),
                }
            }
            if let Ok(n) = preview {
                label { class: "et", {lang.t("PARTIDAS", "STAGES")} }
                if contra {
                    p { class: "hint", {lang.t("Al cambiar la garantía, el número de partidas cambia. Completá o ajustá los textos.", "Changing the guarantee changes the number of stages. Fill in or adjust the texts.")} }
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
                    div { class: "lista-part",
                        for (i, d) in o.detalles.iter().enumerate() {
                            div { class: "lista-part-item",
                                b { "{i + 1}" }
                                span { "{lang.titulo_partida(i, d)}" }
                            }
                        }
                    }
                }
            }
            div { style: "height: 24px;" }
            button {
                class: "btn btn-primary",
                onclick: move |_| {
                    let Some(c) = yo() else { return };
                    let Some(nodo) = red() else { return };
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
}

#[component]
fn RestaurarLlaves(
    caja: caja::Caja,
    yo: Signal<Option<Persona>>,
    obras: Signal<Vec<Obra>>,
    vista: Signal<caja::CajaVista>,
    mut err: Signal<Option<String>>,
) -> Element {
    let mut ok = use_signal(|| None::<String>);
    let lang = use_context::<Signal<Idioma>>()();
    let caja_semilla = caja.clone();
    let caja_share = caja.clone();
    let caja_vista = caja.clone();
    rsx! {
        p { class: "hint",
            {lang.t(
                "Recuperar las 25 palabras trae tu dirección personal. No trae la caja de la obra ni tu nombre en el trato.",
                "Restoring the 25 words brings back your personal address. It does not bring the job's box or your name on the deal.",
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
            {lang.t("Recuperar las 25 palabras", "Restore the 25 words")}
        }
        p { class: "hint",
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
            {lang.t("Recuperar un share", "Restore a share")}
        }
        if let Some(m) = ok() {
            p { class: "hint", "{m}" }
        }
    }
}

#[component]
fn CajaProfundidad(
    obra_id: String,
    caja: caja::Caja,
    vista: Signal<caja::CajaVista>,
    yo: Signal<Option<Persona>>,
    obras: Signal<Vec<Obra>>,
    mut err: Signal<Option<String>>,
) -> Element {
    let mut ok = use_signal(|| None::<String>);
    let lang = use_context::<Signal<Idioma>>()();
    let v = vista();
    let hay = v.caja_de(&obra_id).is_some();
    let mirada = v.miradas.into_iter().find(|m| m.obra == obra_id);
    let (linea_bloques, linea_retro, linea_aviso) = match &mirada {
        Some(m) => (
            match lang {
                Idioma::Es => format!("La caja mira {} bloques hacia atrás.", m.bloques),
                Idioma::En => format!("The box looks {} blocks back.", m.bloques),
            },
            if m.retro > 0 {
                Some(match lang {
                    Idioma::Es => format!("Quedan {} bloques por mirar en la caja.", m.retro),
                    Idioma::En => format!("{} box blocks left to scan backward.", m.retro),
                })
            } else {
                None
            },
            m.aviso.clone(),
        ),
        None => (
            lang.t(
                "La caja arranca por los últimos 40 bloques.",
                "The box starts with the last 40 blocks.",
            ).to_string(),
            None,
            None,
        ),
    };
    let caja_guardar = caja.clone();
    let caja_atras = caja.clone();
    let caja_share = caja.clone();
    let caja_vista = caja.clone();
    let obra_guardar = obra_id.clone();
    let obra_atras = obra_id;
    rsx! {
        if hay {
            button {
                class: "btn btn-ghost",
                onclick: move |_| {
                    let Some(path) = rfd::FileDialog::new()
                        .set_file_name(format!("konstruado-{obra_guardar}.share"))
                        .save_file()
                    else {
                        return;
                    };
                    ok.set(None);
                    match caja_guardar.guardar_share(&obra_guardar, &path) {
                        Ok(()) => {
                            err.set(None);
                            ok.set(Some(lang_now().t(
                                "Guardé el share. Esa copia puede gastar, junto con la del otro.",
                                "Saved the share. That copy can spend, together with the other person's.",
                            ).into()));
                        }
                        Err(e) => err.set(Some(caja::aviso_humano(&e, lang_now() == Idioma::Es))),
                    }
                },
                {lang.t("Guardar el share de la caja", "Save the box share")}
            }
            p { class: "hint",
                {lang.t(
                    "Esta copia puede gastar, junto con el share del otro. Guardala aparte y no la pegues en un chat.",
                    "This copy can spend, together with the other person's share. Keep it aside and do not paste it into a chat.",
                )}
            }
            p { class: "hint", "{linea_bloques}" }
            if let Some(retro) = linea_retro {
                p { class: "hint", "{retro}" }
            }
            if let Some(aviso) = linea_aviso {
                p { class: "err", "{caja::aviso_humano(&aviso, matches!(lang, Idioma::Es))}" }
            }
            button {
                class: "btn btn-ghost",
                onclick: move |_| {
                    ok.set(None);
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
        p { class: "hint",
            {lang.t(
                "Si perdiste el share de esta obra, recuperalo desde el archivo que guardaste. Tiene que ser el tuyo y la obra tiene que seguir en este equipo.",
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
            {lang.t("Recuperar un share", "Restore a share")}
        }
        if let Some(m) = ok() {
            p { class: "hint", "{m}" }
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
        p { class: "meta", {match lang { Idioma::Es => format!("Caja stagenet {addr}"), Idioma::En => format!("Stagenet box {addr}") }} }
        button {
            class: "btn btn-ghost",
            onclick: move |_| mostrar.set(!mostrar()),
            {if mostrar() { lang.t("Ocultar view key", "Hide view key") } else { lang.t("Mostrar view key de la caja", "Show the box view key") }}
        }
        if let Some(clave) = clave {
            p { class: "clave", "{clave}" }
            p { class: "hint",
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
    let mut export_msg = use_signal(|| None::<String>);
    let mut extra_nom = use_signal(String::new);
    let mut extra_monto = use_signal(String::new);
    let lang = use_context::<Signal<Idioma>>()();
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
    rsx! {
        div { class: "pane",
            div { class: "card-h",
                h1 { "{nom}" }
                span { class: chip_estado(estado), "{lang.label_estado(estado)}" }
            }
            p { class: "lead",
                {match lang { Idioma::Es => format!("Mandante {mnom} · contratista {cnom} · {n_part} partidas · trabajo {}", monto(obra.trabajo)), Idioma::En => format!("Client {mnom} · contractor {cnom} · {n_part} stages · job {}", monto(obra.trabajo)) }}
            }
            if hay_caja {
                CajaLlave { obra_id: obra_caja, caja: caja.clone(), vista }
            } else if matches!(estado, EstadoObra::Acordada | EstadoObra::EnMarcha) && (soy_m || soy_c) {
                p { class: "hint", {lang.t("Armando la caja 2-de-2. Los dos tienen que seguir en línea.", "Building the 2-of-2 box. Both have to stay online.")} }
            }
            if abierta && (soy_m || soy_c) {
                CajaProfundidad { obra_id: obra.id.clone(), caja: caja.clone(), vista, yo, obras, err }
            }
            if sincronizando {
                p { class: "hint", {lang.t("Sincronizando el trato… las acciones esperan a bajar el estado del otro.", "Syncing the deal… actions wait until the other side's state arrives.")} }
            }
            if let Some(m) = export_msg() {
                p { class: "hint", "{m}" }
            }
            button {
                class: "btn btn-ghost",
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
                class: "btn btn-ghost",
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
            if contra {
                p { class: "hint",
                    {match lang { Idioma::Es => format!("El contratista propone garantía {} ({} partidas).", monto(garantia), n_part), Idioma::En => format!("The contractor proposes guarantee {} ({} stages).", monto(garantia), n_part) }}
                }
                if soy_m {
                    button {
                        class: "btn btn-primary",
                        onclick: {
                            let mut obra = obra.clone();
                            let mid = mid.clone();
                            move |_| {
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
                            let mut obra = obra.clone();
                            let mid = mid.clone();
                            move |_| {
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
            }
            if estado == EstadoObra::Abandonada {
                p { class: "hint", {lang.t("Esta obra se abandonó. El trato quedó cortado.", "This job was abandoned. The deal is cut.")} }
            }
            if let Some(cl) = obra.cierre.clone() {
                if cl.id == mid {
                    p { class: "hint", {lang.t("Esperando que acepten cortar el trato.", "Waiting for them to accept ending the deal.")} }
                } else if se_puede_abandonar {
                    p { class: "lead", {match lang { Idioma::Es => format!("{} quiere cortar el trato.", cl.nombre), Idioma::En => format!("{} wants to end the deal.", cl.nombre) }} }
                    button {
                        class: "btn btn-primary",
                        onclick: {
                            let mut obra = obra.clone();
                            move |_| {
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
                        class: "btn btn-ghost",
                        onclick: {
                            let mut obra = obra.clone();
                            move |_| {
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
            } else if abierta && se_puede_abandonar {
                if confirma_abandono() {
                    p { class: "hint",
                        if obra.hay_riesgo() {
                            {lang.t("Hay partidas encerradas. El otro tiene que aceptar el cierre.", "There are locked stages. The other person has to accept the close.")}
                        } else {
                            {lang.t("¿Abandonar? Se corta el trato y no se puede deshacer.", "Abandon? The deal is cut and cannot be undone.")}
                        }
                    }
                    button {
                        class: "btn btn-primary",
                        onclick: {
                            let mut obra = obra.clone();
                            move |_| {
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
                } else {
                    button {
                        class: "btn btn-ghost",
                        onclick: move |_| confirma_abandono.set(true),
                        {lang.t("Abandonar esta obra", "Abandon this job")}
                    }
                }
            }
            p { class: "hint", {lang.t("Entrá a cada partida para avisar que terminó, tratar el porcentaje y ver el hilo.", "Open each stage to report finish, deal the percentage and see the thread.")} }
            div { style: "height: 16px;" }
            for (i, p) in partidas.iter().enumerate() {
                {
                    let on = activa == Some(i);
                    let titulo = lang.titulo_partida(i, &p.detalle);
                    let label = lang.label_partida(p);
                    let kind = chip_partida(p.estado);
                    rsx! {
                        button {
                            class: if on { "partida on" } else { "partida" },
                            onclick: move |_| {
                                sel_partida.set(Some(i));
                                screen.set(Screen::VerPartida);
                            },
                            div { class: "txt",
                                strong { "{i + 1}  {titulo}" }
                                span { "{monto(p.capital(garantia))} {lang.t(\"por lado\", \"per side\")}" }
                                if let Some(corto) = caja::saldo_corto(matches!(lang, Idioma::Es), p.estado, p.capital(garantia), p.fondeo_txid.is_some()) {
                                    span { "{corto}" }
                                }
                            }
                            span { class: kind, "{label}" }
                        }
                    }
                }
            }
            div { class: "extra-box",
                if !abierta {}
                else if let Some(ex) = obra.extra.clone() {
                    if ex.por.id == mid {
                        p { class: "hint", {match lang { Idioma::Es => format!("Esperando extra: {} ({} por lado)", extra_label, monto(ex.monto)), Idioma::En => format!("Waiting on extra: {} ({} per side)", extra_label, monto(ex.monto)) }} }
                    } else {
                        p { class: "hint", {match lang { Idioma::Es => format!("{} propone extra: {} (+{} por lado)", ex.por.nombre, extra_label, monto(ex.monto)), Idioma::En => format!("{} proposes extra: {} (+{} per side)", ex.por.nombre, extra_label, monto(ex.monto)) }} }
                        button {
                            class: "btn btn-primary",
                            onclick: {
                                let mut obra = obra.clone();
                                move |_| {
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
                                let mut obra = obra.clone();
                                move |_| {
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
                } else if abierta && se_puede_abandonar && estado != EstadoObra::Contra {
                    label { class: "et", {lang.t("PARTIDA EXTRA (OPCIONAL)", "EXTRA STAGE (OPTIONAL)")} }
                    input {
                        r#type: "text",
                        placeholder: lang.t("P. ej. Techumbre extra", "E.g. Extra roof"),
                        value: "{extra_nom}",
                        oninput: move |e| extra_nom.set(e.value()),
                    }
                    label { class: "et", {lang.t("MONTO POR LADO", "AMOUNT PER SIDE")} }
                    input {
                        r#type: "text",
                        placeholder: lang.t("P. ej. 3000", "E.g. 3000"),
                        value: "{extra_monto}",
                        oninput: move |e| extra_monto.set(e.value()),
                    }
                    button {
                        class: "btn btn-ghost",
                        onclick: {
                            let mut obra = obra.clone();
                            move |_| {
                                if extra_nom().trim().is_empty() {
                                    return;
                                }
                                let m = extra_monto()
                                    .chars()
                                    .filter(|c| c.is_ascii_digit())
                                    .collect::<String>()
                                    .parse()
                                    .unwrap_or(0);
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
    let mi_turno = p.turno.map(|r| match r {
        Rol::Mandante => soy_m,
        Rol::Contratista => soy_c,
    });
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
    let fondeo_curso = vista().linea(&obra.id, i).cloned();
    let frenado = fondeo_curso.as_ref().is_some_and(caja::es_freno);
    let clase_linea = if frenado { "err" } else { "hint" };
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
    rsx! {
        div { class: "pane narrow",
            button {
                class: "btn btn-ghost",
                onclick: move |_| screen.set(Screen::Detalle),
                {lang.t("Volver a la obra", "Back to the job")}
            }
            div { class: "card-h",
                h1 { "{i + 1}  {titulo}" }
                span { class: chip_partida(p.estado), "{label}" }
            }
            p { class: "lead",
                {match lang { Idioma::Es => format!("{} por lado. Mandante {} · contratista {}", monto(p.capital(garantia)), obra.mandante.nombre, obra.contratista.nombre), Idioma::En => format!("{} per side. Client {} · contractor {}", monto(p.capital(garantia)), obra.mandante.nombre, obra.contratista.nombre) }}
            }
            if let Some(s) = caja::saldo_partida(matches!(lang, Idioma::Es), p.estado, p.capital(garantia), p.fondeo_txid.is_some(), &obra.mandante.nombre, &obra.contratista.nombre) {
                p { class: "lead", "{s.estado}" }
                p { class: "lead", "{s.detalle}" }
                if let Some(c) = s.candado {
                    p { class: "hint", "{c}" }
                }
                CajaLlave { obra_id: obra.id.clone(), caja: caja.clone(), vista }
            } else if let Some(pico) = caja::a_piconero(p.capital(garantia)) {
                p { class: "hint", {match lang { Idioma::Es => format!("{} XMR por lado en stagenet.", caja::fmt_xmr(pico)), Idioma::En => format!("{} XMR per side on stagenet.", caja::fmt_xmr(pico)) }} }
            }
            if !cortada && (soy_m || soy_c) {
                CajaProfundidad { obra_id: obra.id.clone(), caja: caja.clone(), vista, yo, obras, err }
            }
            if let Some(tx) = p.fondeo_txid.as_ref() {
                p { class: "meta", {match lang { Idioma::Es => format!("Fondeo {tx}"), Idioma::En => format!("Funding {tx}") }} }
            }
            if let Some(tx) = p.pago_txid.as_ref() {
                p { class: "meta", {match lang { Idioma::Es => format!("Pago {tx}"), Idioma::En => format!("Payment {tx}") }} }
            }
            if let Some(txt) = vista().linea(&obra.id, i).cloned() {
                p { class: "{clase_linea}", "{txt.mostrar(matches!(lang, Idioma::Es))}" }
            }
            if sincronizando {
                p { class: "hint", {lang.t("Sincronizando el trato… las acciones esperan a bajar el estado del otro.", "Syncing the deal… actions wait until the other side's state arrives.")} }
            }
            if !cortada && p.estado == PartidaEstado::Pendiente && (soy_m || soy_c) {
                label { class: "et", {lang.t("TEXTO", "TEXT")} }
                input {
                    r#type: "text",
                    value: "{detalle_edit}",
                    oninput: move |e| detalle_edit.set(e.value()),
                }
                button {
                    class: "btn btn-ghost",
                    onclick: {
                        let mut obra = obra.clone();
                        move |_| {
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
            if cerrado {
                if let Some(r) = p.recibo.as_ref() {
                    div { class: "recibo",
                        strong { "{lang.t(\"Recibo\", \"Receipt\")} · {r.titulo}" }
                        p { {match lang { Idioma::Es => format!("Pagó {}% · {} · aceptó {} · {}", r.porcentaje, monto(r.monto), r.acepto_nombre, lang.fmt_cuando(r.cuando)), Idioma::En => format!("Paid {}% · {} · accepted by {} · {}", r.porcentaje, monto(r.monto), r.acepto_nombre, lang.fmt_cuando(r.cuando)) }} }
                    }
                } else {
                    p { class: "hint",
                        {match lang { Idioma::Es => format!("Cerró al {}% ({}). El hilo quedó guardado.", p.pago.unwrap_or(0), monto(monto_pct(garantia, p.pago.unwrap_or(0)))), Idioma::En => format!("Closed at {}% ({}). The thread was saved.", p.pago.unwrap_or(0), monto(monto_pct(garantia, p.pago.unwrap_or(0)))) }}
                    }
                }
            }
            if let Some(q) = p.encerrado_por.as_ref() {
                p { class: "meta", {match lang { Idioma::Es => format!("Encerró {} · {}", q.nombre, lang.fmt_cuando(p.encerrado_cuando)), Idioma::En => format!("Locked by {} · {}", q.nombre, lang.fmt_cuando(p.encerrado_cuando)) }} }
            }
            if !notas_vis.is_empty() {
                div { class: "notas",
                    for (cabeza, cuerpo, cifrada) in notas_vis {
                        div { class: "nota",
                            strong { "{cabeza}" }
                            if cifrada {
                                p { class: "hint", "{cuerpo}" }
                            } else if !cuerpo.is_empty() {
                                p { "{cuerpo}" }
                            }
                        }
                    }
                }
            }
            if !cortada && p.estado == PartidaEstado::Pendiente {
                if contra {
                    p { class: "hint", {lang.t("Primero hay que confirmar la contra de la obra.", "The job counter has to be confirmed first.")} }
                } else if !activa {
                    p { class: "hint", {lang.t("Todavía no toca. Cerrá la partida que está en curso.", "Not this one yet. Close the stage that is underway.")} }
                } else if confirma_encerrar() {
                    p { class: "hint", {lang.t("Los dos tienen que confirmar el encierre. El otro tiene que estar en línea.", "Both have to confirm the lock. The other person has to be online.")} }
                    button {
                        class: "btn btn-primary",
                        onclick: {
                            let mut obra = obra.clone();
                            move |_| {
                                if !exigir_sesion(red, yo, &obra, err) {
                                    return;
                                }
                                let Some(quien) = yo() else { return };
                                let Some(nodo) = red() else { return };
                                match obra.encerrar_proponer(i, &quien) {
                                    Ok(()) => {
                                        err.set(None);
                                        confirma_encerrar.set(false);
                                        publicar_trato(&nodo, obra.clone(), yo(), err);
                                    }
                                    Err(e) => err.set(Some(lang_now().error(&e))),
                                }
                            }
                        },
                        {lang.t("Proponer encerrar", "Propose lock")}
                    }
                    button {
                        class: "btn btn-ghost",
                        onclick: move |_| confirma_encerrar.set(false),
                        {lang.t("No", "No")}
                    }
                } else {
                    div { style: "height: 16px;" }
                    button {
                        class: "btn btn-primary",
                        onclick: move |_| confirma_encerrar.set(true),
                        {lang.t("Encerrar esta partida", "Lock this stage")}
                    }
                }
            }
            if !cortada && p.estado == PartidaEstado::Encerrando {
                if soy_prop_enc && !frenado {
                    if fondeo_curso.is_none() {
                        p { class: "hint", {lang.t("Esperando que el otro confirme el encierre.", "Waiting for the other person to confirm the lock.")} }
                    }
                    button {
                        class: "btn btn-ghost",
                        onclick: {
                            let mut obra = obra.clone();
                            move |_| {
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
                        },
                        {lang.t("Cancelar propuesta", "Cancel proposal")}
                    }
                } else if frenado {
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
                                match caja.reintentar_fondeo(&obra, i, &quien) {
                                    Ok(()) => err.set(None),
                                    Err(e) => err.set(Some(caja::aviso_humano(&e, lang_now() == Idioma::Es))),
                                }
                            }
                        },
                        {lang.t("Reintentar el fondeo", "Try the funding again")}
                    }
                    button {
                        class: "btn btn-ghost",
                        onclick: {
                            let mut obra = obra.clone();
                            move |_| {
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
                        },
                        {lang.t("No encerrar", "Do not lock")}
                    }
                } else if fondeo_curso.is_none() {
                    p { class: "lead", {lang.t("El otro quiere encerrar esta partida. Confirmar arma una sola transacción con los dos.", "The other person wants to lock this stage. Confirm builds one transaction from both wallets.")} }
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
                    button {
                        class: "btn btn-ghost",
                        onclick: {
                            let mut obra = obra.clone();
                            move |_| {
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
                        },
                        {lang.t("No encerrar", "Do not lock")}
                    }
                } else {
                    button {
                        class: "btn btn-ghost",
                        onclick: {
                            let mut obra = obra.clone();
                            move |_| {
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
                        },
                        {lang.t("No encerrar", "Do not lock")}
                    }
                }
            }
            if !cortada && p.estado == PartidaEstado::Encerrada && soy_c {
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
                div { style: "height: 16px;" }
                button {
                    class: "btn btn-primary",
                    onclick: {
                        let mut obra = obra.clone();
                        move |_| {
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
            }
            if !cortada && p.estado == PartidaEstado::Encerrada && soy_m {
                p { class: "hint", {lang.t("El contratista avisa cuando termina y propone cuánto se paga.", "The contractor reports when they finish and proposes how much is paid.")} }
            }
            if !cortada && p.estado == PartidaEstado::EnTrato {
                if let Some(n) = propuesto {
                    p { class: "lead", {match lang { Idioma::Es => format!("Sobre la mesa: {n}% ({}).", monto(monto_pct(garantia, n))), Idioma::En => format!("On the table: {n}% ({}).", monto(monto_pct(garantia, n))) }} }
                }
                if mi_turno == Some(false) {
                    p { class: "hint", {match lang { Idioma::Es => format!("Esperando a {espera_nom}."), Idioma::En => format!("Waiting for {espera_nom}.") }} }
                }
                if mi_turno == Some(true) {
                    div { style: "height: 12px;" }
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
                            let mut obra = obra.clone();
                            move |_| {
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
        assert!(lista_para_publicar(&mut obra, &c, &cs));
        assert!(obra.partidas[0].notas[0].texto.is_empty());
        assert!(!obra.partidas[0].notas[0].caja.is_empty());
        assert!(lista_para_publicar(&mut obra, &c, &cs));

        let mut m = m;
        m.clave_pub.clear();
        let mut clara = obra_con_nota(&m, &c, "Terminé el muro");
        assert!(!lista_para_publicar(&mut clara, &c, &cs));
        assert_eq!(clara.partidas[0].notas[0].texto, "Terminé el muro");
        assert!(clara.partidas[0].notas[0].caja.is_empty());
    }
}
