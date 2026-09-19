mod export;
mod persist;

use std::time::Duration;

use dioxus::prelude::*;
use konstruado_core::{
    monto, monto_pct, n_partidas, titulo_partida, Aceptacion, EstadoObra, Oferta, Obra, Partida,
    PartidaEstado, Persona, Rol, MAX_NOTA,
};
use konstruado_net::{EstadoTor, Nodo, RED};

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
enum Screen {
    Bienvenida,
    Tablero,
    Nueva,
    Oferta,
    Detalle,
    VerPartida,
}

fn persistir(yo: Option<Persona>, rol: Option<Rol>, n: &Nodo) {
    persist::guardar(&persist::EstadoDisco {
        yo,
        rol,
        ofertas: n.tablero(),
        obras: n.obras(),
        presentes: n.presentes(),
    });
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
                    persistir(yo(), rol(), &n);
                    n.esperar(Duration::from_secs(1)).await;
                }
            }
            Err(e) => err.set(Some(format!("Red: {e}"))),
        }
        }
    });

    let adentro = screen() != Screen::Bienvenida;
    let quien = yo().map(|p| p.nombre).unwrap_or_default();
    let rol_txt = rol().map(Rol::etiqueta).unwrap_or("");
    let mid = yo().map(|p| p.id).unwrap_or_default();
    let mis_obras: Vec<Obra> = obras()
        .into_iter()
        .filter(|o| o.estado != EstadoObra::Rechazada)
        .filter(|o| o.mandante.id == mid || o.contratista.id == mid)
        .collect();

    rsx! {
        style { {CSS} }
        div { class: "app",
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
                if adentro {
                    span { class: "quien", "{quien} · {rol_txt}" }
                }
            }
            div { class: "shell",
                if adentro {
                    aside { class: "side",
                        h2 { "Obras" }
                        for o in mis_obras {
                            button {
                                class: "side-item",
                                onclick: move |_| {
                                    sel_obra.set(Some(o.id.clone()));
                                    screen.set(Screen::Detalle);
                                },
                                strong { "{o.nombre}" }
                                span { class: chip_estado(o.estado), "{label_estado(o.estado)}" }
                            }
                        }
                        if rol() == Some(Rol::Mandante) {
                            button {
                                class: "btn btn-primary",
                                onclick: move |_| screen.set(Screen::Nueva),
                                "Publicar obra"
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

fn label_estado(e: EstadoObra) -> &'static str {
    match e {
        EstadoObra::Publicada => "Publicada",
        EstadoObra::Contra => "Contra",
        EstadoObra::Rechazada => "Rechazada",
        EstadoObra::Acordada => "Acordada",
        EstadoObra::EnMarcha => "En marcha",
        EstadoObra::Abandonada => "Abandonada",
        EstadoObra::Cerrada => "Cerrada",
    }
}

fn chip_partida(e: PartidaEstado) -> &'static str {
    match e {
        PartidaEstado::Pendiente => "chip chip-off",
        PartidaEstado::Encerrada | PartidaEstado::EnTrato => "chip chip-wait",
        PartidaEstado::Pagada => "chip chip-ok",
    }
}

fn label_partida(p: &Partida) -> String {
    match p.estado {
        PartidaEstado::Pendiente => "Pendiente".into(),
        PartidaEstado::Encerrada => "En obra".into(),
        PartidaEstado::EnTrato => match p.propuesto {
            Some(n) => format!("Trato {n}%"),
            None => "En trato".into(),
        },
        PartidaEstado::Pagada => match p.pago {
            Some(n) => format!("Pagada {n}%"),
            None => "Pagada".into(),
        },
    }
}

fn parse_pct(s: &str) -> u32 {
    s.chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

pub(crate) fn fmt_cuando(ts: i64) -> String {
    if ts <= 0 {
        return String::new();
    }
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|d| d.format("%d/%m/%Y %H:%M").to_string())
        .unwrap_or_default()
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

fn avisos_para(mid: &str, soy_m: bool, obras: &[Obra], ofertas: &[Oferta]) -> Vec<Aviso> {
    let mut out = Vec::new();
    if !soy_m {
        for o in ofertas {
            if o.mandante.id != mid {
                out.push(Aviso {
                    texto: format!("{}: {} publicó, podés aceptar", o.nombre, o.mandante.nombre),
                    obra_id: o.id.clone(),
                    es_oferta: true,
                    partida: None,
                });
            }
        }
    }
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
                texto: format!(
                    "{}: {} propone garantía {}",
                    obra.nombre,
                    obra.contratista.nombre,
                    monto(obra.garantia)
                ),
                obra_id: obra.id.clone(),
                es_oferta: false,
                partida: None,
            });
        }
        let mi_rol = if obra.mandante.id == mid {
            Rol::Mandante
        } else {
            Rol::Contratista
        };
        for (i, p) in obra.partidas.iter().enumerate() {
            let titulo = titulo_partida(i, &p.detalle);
            if p.estado == PartidaEstado::EnTrato && p.turno == Some(mi_rol) {
                let pct = p.propuesto.unwrap_or(0);
                out.push(Aviso {
                    texto: format!("{} · {}: te toca responder ({pct}%)", obra.nombre, titulo),
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
                    .unwrap_or("el mandante");
                out.push(Aviso {
                    texto: format!(
                        "{} · {}: {quien} encerró, avisá cuando termines",
                        obra.nombre, titulo
                    ),
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

fn placeholder_partida(i: usize, n: u32) -> &'static str {
    const CINCO: [&str; 5] = [
        "Cimientos",
        "Muros",
        "Techumbre",
        "Instalaciones",
        "Terminaciones",
    ];
    if n == 5 && i < 5 {
        CINCO[i]
    } else {
        "Qué se hace en esta partida"
    }
}

fn linea_red(tor: EstadoTor, peers: usize, otros: &[String]) -> String {
    let tor_txt = match tor {
        EstadoTor::Listo { onion } => {
            let corto = onion.get(..8).unwrap_or(onion.as_str());
            format!("Tor {corto}…")
        }
        EstadoTor::Arrancando { paso } => format!("Tor {paso}"),
        EstadoTor::Fallo(s) => format!("Tor: {s}"),
        EstadoTor::Ausente => "Red local".into(),
    };
    let gente = if otros.is_empty() {
        if peers == 0 {
            "nadie más en la red".into()
        } else {
            format!("{peers} par(es), todavía sin nombre")
        }
    } else {
        otros.join(", ")
    };
    format!("{tor_txt} · {gente}")
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
    rsx! {
        div { class: "pane narrow",
            h1 { "La obra, con el dinero encerrado." }
            p { class: "lead",
                "No te ves con la otra persona como en un chat. El mandante publica una obra. El contratista la ve en el tablero y acepta (o propone otra garantía)."
            }
            div { class: "paso", b { "1" } "Tu nombre" }
            input {
                r#type: "text",
                placeholder: "Cómo te llamás",
                value: "{nombre}",
                oninput: move |e| nombre.set(e.value()),
            }
            div { class: "paso", b { "2" } "¿Qué vas a hacer?" }
            div { class: "roles",
                button {
                    class: if rol() == Some(Rol::Mandante) { "rol on" } else { "rol" },
                    onclick: move |_| rol.set(Some(Rol::Mandante)),
                    strong { "Pago la obra" }
                    span { "Mandante. Publicás el trabajo y la garantía. El otro la ve." }
                }
                button {
                    class: if rol() == Some(Rol::Contratista) { "rol on" } else { "rol" },
                    onclick: move |_| rol.set(Some(Rol::Contratista)),
                    strong { "La construyo" }
                    span { "Contratista. Buscás lo publicado y aceptás, o proponés otra garantía." }
                }
            }
            button {
                class: "btn btn-primary",
                onclick: move |_| {
                    let Some(_) = rol() else {
                        err.set(Some("Elegí si pagás la obra o la construís.".into()));
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
                        Err(e) => err.set(Some(e.to_string())),
                    }
                },
                "Entrar"
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
    let mut buscando = use_signal(|| false);
    let mid = yo().map(|p| p.id).unwrap_or_default();
    let ocupadas: Vec<String> = obras()
        .into_iter()
        .filter(|o| o.estado != EstadoObra::Rechazada)
        .map(|o| o.id)
        .collect();
    let mias: Vec<Oferta> = ofertas()
        .into_iter()
        .filter(|o| o.mandante.id == mid && !ocupadas.contains(&o.id))
        .collect();
    let ajenas: Vec<Oferta> = ofertas()
        .into_iter()
        .filter(|o| o.mandante.id != mid && !ocupadas.contains(&o.id))
        .collect();
    let mis_obras: Vec<Obra> = obras()
        .into_iter()
        .filter(|o| o.estado != EstadoObra::Rechazada)
        .filter(|o| o.mandante.id == mid || o.contratista.id == mid)
        .collect();
    let otros = otros_nombres(yo(), presentes(), peers());
    let status = linea_red(tor(), peers(), &otros);
    let soy_m = rol() == Some(Rol::Mandante);
    let sin_ajenas = ajenas.is_empty();
    let sin_mias = mias.is_empty() && mis_obras.is_empty();
    let avisos = avisos_para(&mid, soy_m, &mis_obras, &ajenas);
    let hint_contratista = if otros.is_empty() {
        "No hay avisos. Don Dinero tiene que publicar, y vos podés tocar Buscar ofertas.".to_string()
    } else {
        format!(
            "{} está en la red. Si no ves el aviso, tocá Buscar ofertas.",
            otros.join(", ")
        )
    };
    rsx! {
        div { class: "pane",
            h1 { if soy_m { "Tus obras" } else { "Tablero" } }
            p { class: "status",
                "Red " code { "{RED}" } " · {status}"
            }
            if peers() == 0 {
                p { class: "hint",
                    match tor() {
                        EstadoTor::Arrancando { .. } => "Tor está subiendo. El mandante abre la sala; el contratista solo busca.",
                        _ => "Nadie más todavía. En la misma PC, un segundo cargo run se engancha solo. En otra máquina, Don Dinero abre la sala y Chasquilla busca.",
                    }
                }
            }

            if !avisos.is_empty() {
                p { class: "lead", "Te toca" }
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
                                span { class: "chip chip-wait", "Te toca" }
                            }
                        }
                    }
                }
                div { style: "height: 24px;" }
            }

            if soy_m {
                p { class: "lead",
                    "Publicá. El contratista no te ve a vos: ve el aviso en su tablero."
                }
                if sin_mias {
                    p { class: "hint", "Todavía no publicaste nada." }
                }
                div { class: "stack",
                    for o in mias {
                        div { class: "card static",
                            div { class: "card-h",
                                strong { "{o.nombre}" }
                                span { class: "chip chip-off", "Esperando contratista" }
                            }
                            p { class: "meta",
                                "Trabajo {monto(o.trabajo)} · garantía {monto(o.garantia_sugerida)} · {o.n_partidas_sugeridas} partidas"
                            }
                            if let Some(r) = resumen_detalles(&o.detalles) {
                                p { class: "meta", "{r}" }
                            }
                        }
                    }
                    for o in mis_obras {
                        button {
                            class: "card",
                            onclick: move |_| {
                                sel_obra.set(Some(o.id.clone()));
                                screen.set(Screen::Detalle);
                            },
                            div { class: "card-h",
                                strong { "{o.nombre}" }
                                span { class: chip_estado(o.estado), "{label_estado(o.estado)}" }
                            }
                            p { class: "meta",
                                "Contratista {o.contratista.nombre} · {o.n_partidas} partidas · {monto(o.garantia)} por lado"
                            }
                            if o.estado == EstadoObra::Contra {
                                p { class: "meta", "Te toca: contra de garantía" }
                            }
                        }
                    }
                }
                div { style: "height: 24px;" }
                button {
                    class: "btn btn-primary",
                    onclick: move |_| screen.set(Screen::Nueva),
                    "Publicar obra"
                }
            } else {
                p { class: "lead",
                    "Ofertas del mandante. Aceptás las condiciones o proponés otra garantía."
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
                    if buscando() { "Buscando…" } else { "Buscar ofertas" }
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
                                span { class: "chip chip-off", "{o.n_partidas_sugeridas} partidas" }
                            }
                            p { class: "meta", "Mandante: {o.mandante.nombre}" }
                            p { class: "meta",
                                "Trabajo {monto(o.trabajo)} · garantía sugerida {monto(o.garantia_sugerida)}"
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
                if !mis_obras.is_empty() {
                    div { style: "height: 24px;" }
                    h1 { "Mis obras" }
                    div { class: "stack",
                        for o in mis_obras {
                            button {
                                class: "card",
                                onclick: move |_| {
                                    sel_obra.set(Some(o.id.clone()));
                                    screen.set(Screen::Detalle);
                                },
                                div { class: "card-h",
                                    strong { "{o.nombre}" }
                                    span { class: chip_estado(o.estado), "{label_estado(o.estado)}" }
                                }
                                p { class: "meta",
                                    "Mandante {o.mandante.nombre} · {o.n_partidas} partidas · {monto(o.garantia)} por lado"
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
    rsx! {
        div { class: "pane narrow",
            h1 { "Publicar obra" }
            p { class: "lead", "Vos sos el mandante. El contratista va a ver esto en el tablero." }
            label { class: "et", "NOMBRE" }
            input {
                r#type: "text",
                value: "{obra_nom}",
                oninput: move |e| obra_nom.set(e.value()),
            }
            label { class: "et", "TRABAJO" }
            input {
                r#type: "text",
                value: "{trabajo}",
                oninput: move |e| trabajo.set(e.value()),
            }
            label { class: "et", "GARANTÍA SUGERIDA" }
            input {
                r#type: "text",
                value: "{garantia}",
                oninput: move |e| garantia.set(e.value()),
            }
            p { class: "hint",
                match preview.clone() {
                    Ok(n) => format!("{n} partidas. En cada una los dos encierran {g}."),
                    Err(e) => e.to_string(),
                }
            }
            if let Ok(n) = preview {
                div { class: "paso", b { "3" } "Qué entra en cada partida" }
                p { class: "hint", "Como en un presupuesto: cimientos, muros, techumbre. El texto es opcional." }
                for (i, d) in detalles().into_iter().enumerate() {
                    label { class: "et", "PARTIDA {i + 1}" }
                    input {
                        r#type: "text",
                        placeholder: "{placeholder_partida(i, n)}",
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
                        err.set(Some("La red todavía no arrancó.".into()));
                        return;
                    };
                    match Oferta::publicar(m, obra_nom(), t, g, detalles()) {
                        Ok(o) => {
                            err.set(None);
                            nodo.publicar(o);
                            screen.set(Screen::Tablero);
                        }
                        Err(e) => err.set(Some(e.to_string())),
                    }
                },
                "Publicar en la red"
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
        return rsx! { p { "No hay oferta." } };
    };
    let g: u64 = garantia_acc().chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0);
    let preview = n_partidas(o.trabajo, g);
    let contra = g != o.garantia_sugerida;
    rsx! {
        div { class: "pane narrow",
            h1 { "{o.nombre}" }
            p { class: "lead",
                "{o.mandante.nombre} ofrece trabajo por {monto(o.trabajo)}. Garantía sugerida {monto(o.garantia_sugerida)} ({o.n_partidas_sugeridas} partidas)."
            }
            label { class: "et", "TU GARANTÍA" }
            input {
                r#type: "text",
                value: "{garantia_acc}",
                oninput: move |e| garantia_acc.set(e.value()),
            }
            p { class: "hint",
                match preview.clone() {
                    Ok(n) if contra => format!("Contra: {n} partidas de {g}. El mandante tiene que confirmar."),
                    Ok(n) => format!("Aceptás {n} partidas. Los dos encierran {g} en cada una."),
                    Err(e) => e.to_string(),
                }
            }
            if let Ok(n) = preview {
                label { class: "et", "PARTIDAS" }
                if contra {
                    p { class: "hint", "Al cambiar la garantía, el número de partidas cambia. Completá o ajustá los textos." }
                    for (i, d) in detalles().into_iter().enumerate() {
                        label { class: "et", "PARTIDA {i + 1}" }
                        input {
                            r#type: "text",
                            placeholder: "{placeholder_partida(i, n)}",
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
                                span { "{titulo_partida(i, d)}" }
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
                            Err(e) => err.set(Some(e.to_string())),
                        },
                        Err(e) => err.set(Some(e.to_string())),
                    }
                },
                if contra { "Proponer esta garantía" } else { "Aceptar condiciones" }
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
    let id = sel_obra().unwrap_or_default();
    let Some(obra) = obras().into_iter().find(|o| o.id == id) else {
        return rsx! { p { "La obra todavía no llegó. Si la acabás de publicar, esperá al contratista." } };
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
    rsx! {
        div { class: "pane",
            div { class: "card-h",
                h1 { "{nom}" }
                span { class: chip_estado(estado), "{label_estado(estado)}" }
            }
            p { class: "lead",
                "Mandante {mnom} · contratista {cnom} · {n_part} partidas de {monto(garantia)}"
            }
            if let Some(m) = export_msg() {
                p { class: "hint", "{m}" }
            }
            button {
                class: "btn btn-ghost",
                onclick: {
                    let obra = obra.clone();
                    move |_| {
                        match export::guardar(&obra) {
                            Ok(p) => export_msg.set(Some(format!("Guardado en {}", p.display()))),
                            Err(e) => export_msg.set(Some(e)),
                        }
                    }
                },
                "Exportar constancia"
            }
            if contra {
                p { class: "hint",
                    "El contratista propone garantía {monto(garantia)} ({n_part} partidas)."
                }
                if soy_m {
                    button {
                        class: "btn btn-primary",
                        onclick: {
                            let mut obra = obra.clone();
                            let mid = mid.clone();
                            move |_| {
                                let Some(nodo) = red() else { return };
                                match obra.confirmar_contra(&mid) {
                                    Ok(()) => {
                                        nodo.publicar_obra(obra.clone());
                                    }
                                    Err(e) => err.set(Some(e.to_string())),
                                }
                            }
                        },
                        "Confirmar contra"
                    }
                    button {
                        class: "btn btn-ghost",
                        onclick: {
                            let mut obra = obra.clone();
                            let mid = mid.clone();
                            move |_| {
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
                                        Err(e) => err.set(Some(e.to_string())),
                                    }
                                    }
                                    Err(e) => err.set(Some(e.to_string())),
                                }
                            }
                        },
                        "No aceptar esta garantía"
                    }
                }
            }
            if estado == EstadoObra::Abandonada {
                p { class: "hint", "Esta obra se abandonó. El trato quedó cortado." }
            }
            if abierta && se_puede_abandonar {
                if confirma_abandono() {
                    p { class: "hint", "¿Abandonar? Se corta el trato y no se puede deshacer." }
                    button {
                        class: "btn btn-primary",
                        onclick: {
                            let mut obra = obra.clone();
                            move |_| {
                                let Some(quien) = yo() else { return };
                                let Some(nodo) = red() else { return };
                                match obra.abandonar(&quien) {
                                    Ok(()) => {
                                        err.set(None);
                                        confirma_abandono.set(false);
                                        nodo.publicar_obra(obra.clone());
                                        screen.set(Screen::Tablero);
                                    }
                                    Err(e) => err.set(Some(e.to_string())),
                                }
                            }
                        },
                        "Sí, abandonar"
                    }
                    button {
                        class: "btn btn-ghost",
                        onclick: move |_| confirma_abandono.set(false),
                        "No"
                    }
                } else {
                    button {
                        class: "btn btn-ghost",
                        onclick: move |_| confirma_abandono.set(true),
                        "Abandonar esta obra"
                    }
                }
            }
            p { class: "hint", "Entrá a cada partida para avisar que terminó, tratar el porcentaje y ver el hilo." }
            div { style: "height: 16px;" }
            for (i, p) in partidas.iter().enumerate() {
                {
                    let on = activa == Some(i);
                    let titulo = titulo_partida(i, &p.detalle);
                    let label = label_partida(p);
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
                                span { "{monto(garantia)} por lado" }
                            }
                            span { class: kind, "{label}" }
                        }
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
    use_effect(move || {
        let _ = sel_partida();
        pct.set("100".into());
        nota.set(String::new());
    });
    let id = sel_obra().unwrap_or_default();
    let Some(obra) = obras().into_iter().find(|o| o.id == id) else {
        return rsx! { p { "No está esa obra." } };
    };
    let Some(i) = sel_partida() else {
        return rsx! { p { "Elegí una partida." } };
    };
    let Some(p) = obra.partidas.get(i).cloned() else {
        return rsx! { p { "No está esa partida." } };
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
    let titulo = titulo_partida(i, &p.detalle);
    let label = label_partida(&p);
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
    rsx! {
        div { class: "pane narrow",
            button {
                class: "btn btn-ghost",
                onclick: move |_| screen.set(Screen::Detalle),
                "Volver a la obra"
            }
            div { class: "card-h",
                h1 { "{i + 1}  {titulo}" }
                span { class: chip_partida(p.estado), "{label}" }
            }
            p { class: "lead",
                "{monto(garantia)} por lado. Mandante {obra.mandante.nombre} · contratista {obra.contratista.nombre}"
            }
            if cerrado {
                if let Some(r) = p.recibo.as_ref() {
                    div { class: "recibo",
                        strong { "Recibo · {r.titulo}" }
                        p { "Pagó {r.porcentaje}% · {monto(r.monto)} · aceptó {r.acepto_nombre} · {fmt_cuando(r.cuando)}" }
                    }
                } else {
                    p { class: "hint",
                        "Cerró al {p.pago.unwrap_or(0)}% ({monto(monto_pct(garantia, p.pago.unwrap_or(0)))}). El hilo quedó guardado."
                    }
                }
            }
            if let Some(q) = p.encerrado_por.as_ref() {
                p { class: "meta", "Encerró {q.nombre} · {fmt_cuando(p.encerrado_cuando)}" }
            }
            if !p.notas.is_empty() {
                div { class: "notas",
                    for n in p.notas.iter() {
                        div { class: "nota",
                            strong { "{n.autor_nombre} · {n.porcentaje}% · {fmt_cuando(n.cuando)}" }
                            if !n.texto.is_empty() {
                                p { "{n.texto}" }
                            }
                        }
                    }
                }
            }
            if !cortada && p.estado == PartidaEstado::Pendiente {
                if contra {
                    p { class: "hint", "Primero hay que confirmar la contra de la obra." }
                } else if !activa {
                    p { class: "hint", "Todavía no toca. Cerrá la partida que está en curso." }
                } else if confirma_encerrar() {
                    p { class: "hint", "¿Encerrar esta partida? Queda registro de quién lo hizo." }
                    button {
                        class: "btn btn-primary",
                        onclick: {
                            let mut obra = obra.clone();
                            move |_| {
                                let Some(quien) = yo() else { return };
                                let Some(nodo) = red() else { return };
                                match obra.encerrar_partida(i, &quien) {
                                    Ok(()) => {
                                        err.set(None);
                                        confirma_encerrar.set(false);
                                        nodo.publicar_obra(obra.clone());
                                    }
                                    Err(e) => err.set(Some(e.to_string())),
                                }
                            }
                        },
                        "Sí, encerrar"
                    }
                    button {
                        class: "btn btn-ghost",
                        onclick: move |_| confirma_encerrar.set(false),
                        "No"
                    }
                } else {
                    div { style: "height: 16px;" }
                    button {
                        class: "btn btn-primary",
                        onclick: move |_| confirma_encerrar.set(true),
                        "Encerrar esta partida (stub XMR)"
                    }
                }
            }
            if !cortada && p.estado == PartidaEstado::Encerrada && soy_c {
                label { class: "et", "PORCENTAJE A COBRAR" }
                input {
                    r#type: "text",
                    value: "{pct}",
                    oninput: move |e| pct.set(e.value()),
                }
                label { class: "et", "NOTA ({n_nota}/{MAX_NOTA})" }
                input {
                    r#type: "text",
                    placeholder: "Terminé las fundaciones",
                    value: "{nota}",
                    oninput: move |e| nota.set(recorta_nota(e.value())),
                }
                div { style: "height: 16px;" }
                button {
                    class: "btn btn-primary",
                    onclick: {
                        let mut obra = obra.clone();
                        move |_| {
                            let Some(quien) = yo() else { return };
                            let Some(nodo) = red() else { return };
                            match obra.avisar_termino(i, &quien, parse_pct(&pct()), nota()) {
                                Ok(()) => {
                                    err.set(None);
                                    nodo.publicar_obra(obra.clone());
                                }
                                Err(e) => err.set(Some(e.to_string())),
                            }
                        }
                    },
                    "Avisar que terminé"
                }
            }
            if !cortada && p.estado == PartidaEstado::Encerrada && soy_m {
                p { class: "hint", "El contratista avisa cuando termina y propone cuánto se paga." }
            }
            if !cortada && p.estado == PartidaEstado::EnTrato {
                if let Some(n) = propuesto {
                    p { class: "lead", "Sobre la mesa: {n}% ({monto(monto_pct(garantia, n))})." }
                }
                if mi_turno == Some(false) {
                    p { class: "hint", "Esperando a {espera_nom}." }
                }
                if mi_turno == Some(true) {
                    div { style: "height: 12px;" }
                    button {
                        class: "btn btn-primary",
                        onclick: {
                            let mut obra = obra.clone();
                            move |_| {
                                let Some(quien) = yo() else { return };
                                let Some(nodo) = red() else { return };
                                match obra.aceptar_pago(i, &quien) {
                                    Ok(()) => {
                                        err.set(None);
                                        nodo.publicar_obra(obra.clone());
                                    }
                                    Err(e) => err.set(Some(e.to_string())),
                                }
                            }
                        },
                        "Aceptar {propuesto.unwrap_or(0)}%"
                    }
                    label { class: "et", "OTRO PORCENTAJE" }
                    input {
                        r#type: "text",
                        value: "{pct}",
                        oninput: move |e| pct.set(e.value()),
                    }
                    label { class: "et", "NOTA ({n_nota}/{MAX_NOTA})" }
                    input {
                        r#type: "text",
                        placeholder: "Falta la entrada de auto",
                        value: "{nota}",
                        oninput: move |e| nota.set(recorta_nota(e.value())),
                    }
                    button {
                        class: "btn btn-ghost",
                        onclick: {
                            let mut obra = obra.clone();
                            move |_| {
                                let Some(quien) = yo() else { return };
                                let Some(nodo) = red() else { return };
                                match obra.contra_pago(i, &quien, parse_pct(&pct()), nota()) {
                                    Ok(()) => {
                                        err.set(None);
                                        nodo.publicar_obra(obra.clone());
                                    }
                                    Err(e) => err.set(Some(e.to_string())),
                                }
                            }
                        },
                        "Proponer este porcentaje"
                    }
                }
            }
        }
    }
}
