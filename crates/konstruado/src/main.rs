use std::time::Duration;

use dioxus::prelude::*;
use konstruado_core::{
    monto, n_partidas, Aceptacion, EstadoObra, Oferta, Obra, PartidaEstado, Persona,
};
use konstruado_net::{EstadoTor, Nodo, RED};

const CSS: &str = include_str!("ui.css");

fn main() {
    let window = dioxus::desktop::WindowBuilder::new()
        .with_title("Konstruado")
        .with_inner_size(dioxus::desktop::LogicalSize::new(1100.0, 760.0))
        .with_min_inner_size(dioxus::desktop::LogicalSize::new(420.0, 560.0));
    let cfg = dioxus::desktop::Config::new().with_window(window);
    dioxus::LaunchBuilder::desktop().with_cfg(cfg).launch(App);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Bienvenida,
    Tablero,
    Nueva,
    Oferta,
    Detalle,
}

#[component]
fn App() -> Element {
    let mut screen = use_signal(|| Screen::Bienvenida);
    let mut nombre = use_signal(String::new);
    let mut yo = use_signal(|| None::<Persona>);
    let mut red = use_signal(|| None::<Nodo>);
    let mut tor = use_signal(|| EstadoTor::Ausente);
    let mut peers = use_signal(|| 0usize);
    let mut ofertas = use_signal(Vec::<Oferta>::new);
    let mut obras = use_signal(Vec::<Obra>::new);
    let mut sel_oferta = use_signal(|| None::<Oferta>);
    let mut sel_obra = use_signal(|| None::<String>);
    let mut err = use_signal(|| None::<String>);
    let mut trabajo = use_signal(|| "10000".to_string());
    let mut garantia = use_signal(|| "2000".to_string());
    let mut obra_nom = use_signal(|| "Casa El Quisco".to_string());
    let mut garantia_acc = use_signal(|| "2000".to_string());

    use_future(move || async move {
        match Nodo::arrancar().await {
            Ok(n) => {
                tor.set(n.estado_tor().await);
                red.set(Some(n.clone()));
                loop {
                    peers.set(n.n_peers().await);
                    ofertas.set(n.tablero().await);
                    obras.set(n.obras().await);
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
            Err(e) => err.set(Some(format!("Red: {e}"))),
        }
    });

    let adentro = screen() != Screen::Bienvenida;
    let quien = yo().map(|p| p.nombre).unwrap_or_default();

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
                    span { class: "quien", "{quien}" }
                }
            }
            div { class: "shell",
                if adentro {
                    aside { class: "side",
                        h2 { "Obras" }
                        for o in obras() {
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
                        button {
                            class: "btn btn-primary",
                            onclick: move |_| screen.set(Screen::Nueva),
                            "Publicar obra"
                        }
                    }
                }
                main { class: "main",
                    if let Some(e) = err() {
                        div { class: "err", "{e}" }
                    }
                    match screen() {
                        Screen::Bienvenida => rsx! {
                            Bienvenida { nombre, yo, screen, err }
                        },
                        Screen::Tablero => rsx! {
                            Tablero {
                                yo, ofertas, obras, screen, sel_oferta, sel_obra,
                                tor, peers, garantia_acc
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
                            Detalle { yo, red, obras, sel_obra, screen, err }
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
        EstadoObra::Acordada => "chip chip-off",
        EstadoObra::EnMarcha => "chip chip-wait",
        EstadoObra::Cerrada => "chip chip-ok",
    }
}

fn label_estado(e: EstadoObra) -> &'static str {
    match e {
        EstadoObra::Publicada => "Publicada",
        EstadoObra::Contra => "Contra",
        EstadoObra::Acordada => "Acordada",
        EstadoObra::EnMarcha => "En marcha",
        EstadoObra::Cerrada => "Cerrada",
    }
}

#[component]
fn Bienvenida(
    nombre: Signal<String>,
    yo: Signal<Option<Persona>>,
    screen: Signal<Screen>,
    err: Signal<Option<String>>,
) -> Element {
    rsx! {
        div { class: "pane narrow",
            h1 { "La obra, con el dinero encerrado." }
            p { class: "lead",
                "El mandante publica. El contratista acepta (o propone otra garantía). El capital en juego es siempre igual."
            }
            div { class: "paso", b { "1" } "Tu nombre" }
            input {
                r#type: "text",
                placeholder: "Cómo te llamás",
                value: "{nombre}",
                oninput: move |e| nombre.set(e.value()),
            }
            div { style: "height: 20px;" }
            button {
                class: "btn btn-primary",
                onclick: move |_| {
                    match Persona::nueva(nombre()) {
                        Ok(p) => {
                            yo.set(Some(p));
                            err.set(None);
                            screen.set(Screen::Tablero);
                        }
                        Err(e) => err.set(Some(e.to_string())),
                    }
                },
                "Entrar a la red"
            }
        }
    }
}

#[component]
fn Tablero(
    yo: Signal<Option<Persona>>,
    ofertas: Signal<Vec<Oferta>>,
    obras: Signal<Vec<Obra>>,
    screen: Signal<Screen>,
    sel_oferta: Signal<Option<Oferta>>,
    sel_obra: Signal<Option<String>>,
    tor: Signal<EstadoTor>,
    peers: Signal<usize>,
    garantia_acc: Signal<String>,
) -> Element {
    let mid = yo().map(|p| p.id).unwrap_or_default();
    let ocupadas: Vec<String> = obras().into_iter().map(|o| o.id).collect();
    let abiertas: Vec<Oferta> = ofertas()
        .into_iter()
        .filter(|o| !ocupadas.contains(&o.id))
        .collect();
    let sin_ofertas = abiertas.is_empty();
    let tor_txt = match tor() {
        EstadoTor::Listo { socks } => format!("Tor listo ({socks})"),
        EstadoTor::Ausente => "Tor no encontrado (red local)".into(),
    };
    rsx! {
        div { class: "pane",
            h1 { "Tablero" }
            p { class: "status",
                "Red " code { "{RED}" } " · {tor_txt} · {peers} pares"
            }
            p { class: "lead", "Ofertas publicadas. El contratista entra sin que le pasen un archivo." }
            div { class: "stack",
                for o in abiertas {
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
                    }
                }
            }
            if sin_ofertas {
                p { class: "hint", "Nadie publicó todavía. Si sos mandante, publicá una obra." }
            }
            div { style: "height: 24px;" }
            h1 { "Mis obras" }
            div { class: "stack",
                for o in obras() {
                    if o.mandante.id == mid || o.contratista.id == mid {
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
                                "{o.n_partidas} partidas · {monto(o.garantia)} por lado"
                            }
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
                match preview {
                    Ok(n) => format!("{n} partidas. En cada una los dos encierran {g}."),
                    Err(e) => e.to_string(),
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
                    match Oferta::publicar(m, obra_nom(), t, g) {
                        Ok(o) => {
                            err.set(None);
                            spawn(async move {
                                nodo.publicar(o).await;
                            });
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
                match preview {
                    Ok(n) if contra => format!("Contra: {n} partidas de {g}. El mandante tiene que confirmar."),
                    Ok(n) => format!("Aceptás {n} partidas. Los dos encierran {g} en cada una."),
                    Err(e) => e.to_string(),
                }
            }
            div { style: "height: 24px;" }
            button {
                class: "btn btn-primary",
                onclick: move |_| {
                    let Some(c) = yo() else { return };
                    let Some(nodo) = red() else { return };
                    let oferta = o.clone();
                    match Aceptacion::de(&oferta, c, g) {
                        Ok(acc) => match Obra::desde_oferta(oferta, acc) {
                            Ok(obra) => {
                                err.set(None);
                                spawn(async move {
                                    nodo.quitar(&obra.id).await;
                                    nodo.publicar_obra(obra).await;
                                });
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
    screen: Signal<Screen>,
    err: Signal<Option<String>>,
) -> Element {
    let id = sel_obra().unwrap_or_default();
    let Some(obra) = obras().into_iter().find(|o| o.id == id) else {
        return rsx! { p { "No está esa obra." } };
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
    rsx! {
        div { class: "pane",
            div { class: "card-h",
                h1 { "{nom}" }
                span { class: chip_estado(estado), "{label_estado(estado)}" }
            }
            p { class: "lead",
                "Mandante {mnom} · contratista {cnom} · {n_part} partidas de {monto(garantia)}"
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
                                        let o = obra.clone();
                                        spawn(async move { nodo.publicar_obra(o).await; });
                                    }
                                    Err(e) => err.set(Some(e.to_string())),
                                }
                            }
                        },
                        "Confirmar contra"
                    }
                }
            }
            div { style: "height: 16px;" }
            for (i, st) in partidas.iter().enumerate() {
                {
                    let on = activa == Some(i);
                    let label = match st {
                        PartidaEstado::Pendiente => "Pendiente",
                        PartidaEstado::Encerrada => "Encerrada",
                        PartidaEstado::Pagada => "Pagada",
                    };
                    let kind = match st {
                        PartidaEstado::Pendiente => "chip chip-off",
                        PartidaEstado::Encerrada => "chip chip-wait",
                        PartidaEstado::Pagada => "chip chip-ok",
                    };
                    rsx! {
                        div { class: if on { "partida on" } else { "partida" },
                            div { class: "txt",
                                strong { "{i + 1}  Partida" }
                                span { "{monto(garantia)} por lado" }
                            }
                            span { class: kind, "{label}" }
                        }
                    }
                }
            }
            div { style: "height: 24px;" }
            if let Some(i) = activa {
                if partidas[i] == PartidaEstado::Pendiente && !contra {
                    button {
                        class: "btn btn-primary",
                        onclick: {
                            let mut obra = obra.clone();
                            move |_| {
                                let Some(nodo) = red() else { return };
                                match obra.encerrar_partida(i) {
                                    Ok(()) => {
                                        let o = obra.clone();
                                        spawn(async move { nodo.publicar_obra(o).await; });
                                    }
                                    Err(e) => err.set(Some(e.to_string())),
                                }
                            }
                        },
                        "Encerrar partida {i + 1} (stub XMR)"
                    }
                }
                if partidas[i] == PartidaEstado::Encerrada && soy_m {
                    button {
                        class: "btn btn-primary",
                        onclick: {
                            let mut obra = obra.clone();
                            move |_| {
                                let Some(nodo) = red() else { return };
                                match obra.pagar_partida(i) {
                                    Ok(()) => {
                                        let o = obra.clone();
                                        spawn(async move { nodo.publicar_obra(o).await; });
                                    }
                                    Err(e) => err.set(Some(e.to_string())),
                                }
                            }
                        },
                        "Pagar partida {i + 1}"
                    }
                }
            }
        }
    }
}
