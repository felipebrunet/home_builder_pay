use dioxus::prelude::*;

use konstruado_core::Persona;

use crate::Screen;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const REPO: &str = env!("CARGO_PKG_REPOSITORY");
pub const LICENSE: &str = env!("CARGO_PKG_LICENSE");
pub const DESCRIPCION: &str = env!("CARGO_PKG_DESCRIPTION");
const README: &str = include_str!("../../../README.md");

fn abrir_url(url: &str) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

#[component]
pub fn Help(yo: Signal<Option<Persona>>, screen: Signal<Screen>) -> Element {
    rsx! {
        div { class: "pane",
            button {
                class: "btn btn-ghost",
                onclick: move |_| {
                    if yo().is_some() {
                        screen.set(Screen::Tablero);
                    } else {
                        screen.set(Screen::Bienvenida);
                    }
                },
                "Back"
            }
            h1 { "Help" }
            div { class: "about",
                p { class: "lead", "About" }
                p { strong { "Konstruado" } " {VERSION}" }
                p { "{DESCRIPCION}" }
                p { "{LICENSE}" }
                button {
                    class: "help-link",
                    onclick: move |_| abrir_url(REPO),
                    "{REPO}"
                }
            }
            pre { class: "readme", "{README}" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn about_trae_repo_y_readme() {
        assert_eq!(VERSION, "0.1.0");
        assert!(REPO.contains("felipebrunet/home_builder_pay"));
        assert!(README.contains("Konstruado"));
        assert!(README.contains("ES / EN"));
    }
}
