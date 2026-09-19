mod export;
mod help;
mod i18n;
mod persist;

use std::time::Duration;

use dioxus::prelude::*;
use konstruado_core::{
    monto, monto_pct, n_partidas, Aceptacion, EstadoObra, Oferta, Obra, PartidaEstado, Persona, Rol,
    MAX_NOTA,
};
use konstruado_net::{EstadoTor, Nodo, RED};
use i18n::Idioma;

const CSS: &str = include_str!("ui.css");

fn main() {
    preparar_grafica();
    let window = dioxus::desktop::WindowBuilder::new()
        .with_title("Konstruado")
        .with_inner_size(dioxus::desktop::LogicalSize::new(1100.0, 760.0))
        .with_min_inner_size(dioxus::desktop::LogicalSize::new(420.0, 560.0));
    let cfg = dioxus::desktop::Config::new().with_window(window);
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
    Help,
}

fn persistir(yo: Option<Persona>, rol: Option<Rol>, tema: String, idioma: String, n: &Nodo) {
    persist::guardar(&persist::EstadoDisco {
        yo,
        rol,
        ofertas: n.tablero(),
        obras: n.obras(),
        presentes: n.presentes(),
        tema,
        idioma,
    });
}

fn lang_now() -> Idioma {
    consume_context::<Signal<Idioma>>()()
}

#[component]
fn App() -> Element {
    let guardado = use_hook(persist::cargar);
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

    use_future(move || {
        let ofertas0 = guardado.ofertas.clone();
        let obras0 = guardado.obras.clone();
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
                red.set(Some(n.clone()));
                loop {
                    tor.set(n.estado_tor());
                    if let Some(p) = yo() {
                        n.anunciar(p);
                    }
                    if let Some(r) = rol() {
                        n.entrar_en_sala(r == Rol::Mandante);
                    }
                    peers.set(n.n_peers());
                    presentes.set(n.presentes());
                    ofertas.set(n.tablero());
                    obras.set(n.obras());
                    persistir(yo(), rol(), tema(), idioma().codigo().into(), &n);
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
        .filter(|o| o.mandante.id == mid || o.contratista.id == mid)
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
                button {
                    class: "quien",
                    onclick: move |_| screen.set(Screen::Help),
                    "Help"
                }
                if adentro {
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
                            Detalle { yo, red, obras, sel_obra, sel_partida, screen, err }
                        },
                        Screen::VerPartida => rsx! {
                            VerPartida { yo, red, obras, sel_obra, sel_partida, screen, err }
                        },
                        Screen::Cuenta => rsx! {
                            Cuenta { nombre, rol, yo, red, screen, err, tema }
                        },
                        Screen::Help => rsx! {
                            help::Help { yo, screen }
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

fn avisos_para(mid: &str, _soy_m: bool, obras: &[Obra], _ofertas: &[Oferta], lang: Idioma) -> Vec<Aviso> {
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
                out.push(Aviso {
                    texto: match lang {
                        Idioma::Es => format!(
                            "{}: {} propone extra {} ({})",
                            obra.nombre, ex.por.nombre, ex.detalle, monto(ex.monto)
                        ),
                        Idioma::En => format!(
                            "{}: {} proposes extra {} ({})",
                            obra.nombre, ex.por.nombre, ex.detalle, monto(ex.monto)
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
                        Ok(p) => {
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
    screen: Signal<Screen>,
    err: Signal<Option<String>>,
    tema: Signal<String>,
) -> Element {
    let mut nom = use_signal(|| nombre());
    let mut rlocal = use_signal(|| rol());
    let mut tlocal = use_signal(|| tema());
    let mut idioma = use_context::<Signal<Idioma>>();
    let mut ilocal = use_signal(|| idioma());
    let lang = idioma();
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
                                persistir(Some(p.clone()), Some(r), tlocal(), ilocal().codigo().into(), &nodo);
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
    let mut buscando = use_signal(|| false);
    let mid = yo().map(|p| p.id).unwrap_or_default();
    let ocupadas: Vec<String> = obras()
        .into_iter()
        .filter(|o| o.estado != EstadoObra::Rechazada)
        .map(|o| o.id)
        .collect();
    let mut mias: Vec<Oferta> = ofertas()
        .into_iter()
        .filter(|o| o.mandante.id == mid && !ocupadas.contains(&o.id))
        .collect();
    mias.sort_by(|a, b| b.actualizado.cmp(&a.actualizado).then(b.id.cmp(&a.id)));
    let mut ajenas: Vec<Oferta> = ofertas()
        .into_iter()
        .filter(|o| o.mandante.id != mid && !ocupadas.contains(&o.id))
        .collect();
    ajenas.sort_by(|a, b| b.actualizado.cmp(&a.actualizado).then(b.id.cmp(&a.id)));
    let mut mis_obras: Vec<Obra> = obras()
        .into_iter()
        .filter(|o| o.estado != EstadoObra::Rechazada)
        .filter(|o| o.mandante.id == mid || o.contratista.id == mid)
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
    let avisos = avisos_para(&mid, soy_m, &mis_obras, &ajenas, lang);
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
                                err.set(None);
                                nodo.quitar(&obra.id);
                                nodo.publicar_obra(obra);
                                screen.set(Screen::Tablero);
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
fn Detalle(
    yo: Signal<Option<Persona>>,
    red: Signal<Option<Nodo>>,
    obras: Signal<Vec<Obra>>,
    sel_obra: Signal<Option<String>>,
    sel_partida: Signal<Option<usize>>,
    screen: Signal<Screen>,
    err: Signal<Option<String>>,
) -> Element {
    let mut confirma_abandono = use_signal(|| false);
    let mut export_msg = use_signal(|| None::<String>);
    let mut extra_nom = use_signal(String::new);
    let mut extra_monto = use_signal(String::new);
    let lang = use_context::<Signal<Idioma>>()();
    let id = sel_obra().unwrap_or_default();
    let Some(obra) = obras().into_iter().find(|o| o.id == id) else {
        return rsx! { p { {lang.t("La obra todavía no llegó. Si la acabás de publicar, esperá al contratista.", "The job has not arrived yet. If you just posted it, wait for the contractor.")} } };
    };
    let mid = yo().map(|p| p.id).unwrap_or_default();
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
    rsx! {
        div { class: "pane",
            div { class: "card-h",
                h1 { "{nom}" }
                span { class: chip_estado(estado), "{lang.label_estado(estado)}" }
            }
            p { class: "lead",
                {match lang { Idioma::Es => format!("Mandante {mnom} · contratista {cnom} · {n_part} partidas · trabajo {}", monto(obra.trabajo)), Idioma::En => format!("Client {mnom} · contractor {cnom} · {n_part} stages · job {}", monto(obra.trabajo)) }}
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
                    move |_| {
                        match export::guardar_txt(&obra, lang_now()) {
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
                    move |_| {
                        match export::guardar_pdf(&obra, lang_now()) {
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
                                        nodo.publicar_obra(obra.clone());
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
                                            nodo.publicar_obra(obra.clone());
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
                                        nodo.publicar_obra(obra.clone());
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
                                        nodo.publicar_obra(obra.clone());
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
                                        nodo.publicar_obra(obra.clone());
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
                        p { class: "hint", {match lang { Idioma::Es => format!("Esperando extra: {} ({} por lado)", ex.detalle, monto(ex.monto)), Idioma::En => format!("Waiting on extra: {} ({} per side)", ex.detalle, monto(ex.monto)) }} }
                    } else {
                        p { class: "hint", {match lang { Idioma::Es => format!("{} propone extra: {} (+{} por lado)", ex.por.nombre, ex.detalle, monto(ex.monto)), Idioma::En => format!("{} proposes extra: {} (+{} per side)", ex.por.nombre, ex.detalle, monto(ex.monto)) }} }
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
                                    match obra.aceptar_extra(&quien) {
                                        Ok(()) => {
                                            err.set(None);
                                            nodo.publicar_obra(obra.clone());
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
                                            nodo.publicar_obra(obra.clone());
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
                                        nodo.publicar_obra(obra.clone());
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
) -> Element {
    let mut pct = use_signal(|| "100".to_string());
    let mut nota = use_signal(String::new);
    let mut confirma_encerrar = use_signal(|| false);
    let mut detalle_edit = use_signal(String::new);
    let lang = use_context::<Signal<Idioma>>()();
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
    let sincronizando = !cortada && sincronizando_trato(red, yo, &obra);
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
                                    nodo.publicar_obra(obra.clone());
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
            if !p.notas.is_empty() {
                div { class: "notas",
                    for n in p.notas.iter() {
                        div { class: "nota",
                            strong { "{n.autor_nombre} · {n.porcentaje}% · {lang.fmt_cuando(n.cuando)}" }
                            if !n.texto.is_empty() {
                                p { "{n.texto}" }
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
                                        nodo.publicar_obra(obra.clone());
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
                        {lang.t("Encerrar esta partida (stub XMR)", "Lock this stage (XMR stub)")}
                    }
                }
            }
            if !cortada && p.estado == PartidaEstado::Encerrando {
                if soy_prop_enc {
                    p { class: "hint", {lang.t("Esperando que el otro confirme el encierre.", "Waiting for the other person to confirm the lock.")} }
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
                                        nodo.publicar_obra(obra.clone());
                                    }
                                    Err(e) => err.set(Some(lang_now().error(&e))),
                                }
                            }
                        },
                        {lang.t("Cancelar propuesta", "Cancel proposal")}
                    }
                } else {
                    p { class: "lead", {lang.t("El otro quiere encerrar esta partida. Los dos tienen que confirmar.", "The other person wants to lock this stage. Both have to confirm.")} }
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
                                match obra.encerrar_confirmar(i, &quien) {
                                    Ok(()) => {
                                        err.set(None);
                                        nodo.publicar_obra(obra.clone());
                                    }
                                    Err(e) => err.set(Some(lang_now().error(&e))),
                                }
                            }
                        },
                        {lang.t("Confirmar encierre", "Confirm lock")}
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
                                        nodo.publicar_obra(obra.clone());
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
                                    nodo.publicar_obra(obra.clone());
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
                            let mut obra = obra.clone();
                            move |_| {
                                if !exigir_sesion(red, yo, &obra, err) {
                                    return;
                                }
                                let Some(quien) = yo() else { return };
                                let Some(nodo) = red() else { return };
                                match obra.aceptar_pago(i, &quien) {
                                    Ok(()) => {
                                        err.set(None);
                                        nodo.publicar_obra(obra.clone());
                                    }
                                    Err(e) => err.set(Some(lang_now().error(&e))),
                                }
                            }
                        },
                        {match lang { Idioma::Es => format!("Aceptar {}%", propuesto.unwrap_or(0)), Idioma::En => format!("Accept {}%", propuesto.unwrap_or(0)) }}
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
                                        nodo.publicar_obra(obra.clone());
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
