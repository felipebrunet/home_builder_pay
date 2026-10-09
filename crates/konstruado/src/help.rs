use dioxus::desktop::muda::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use dioxus::prelude::*;

use konstruado_core::Persona;

use crate::Screen;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const REPO: &str = env!("CARGO_PKG_REPOSITORY");
pub const LICENSE: &str = env!("CARGO_PKG_LICENSE");
pub const DESCRIPCION: &str = env!("CARGO_PKG_DESCRIPTION");
pub const ID_ABOUT: &str = "konstruado-about";
pub const ID_README: &str = "konstruado-readme";
const README: &str = include_str!("../HELP.md");

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Vista {
    About,
    Readme,
}

/// Native Window / Edit / Help. Help is always present, not only in debug.
pub fn menu() -> Menu {
    let menu = Menu::new();
    let window_menu = Submenu::new("Window", true);
    window_menu
        .append_items(&[
            &PredefinedMenuItem::fullscreen(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::maximize(None),
            &PredefinedMenuItem::minimize(None),
            &PredefinedMenuItem::close_window(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::quit(None),
        ])
        .unwrap();

    let edit_menu = Submenu::new("Edit", true);
    edit_menu
        .append_items(&[
            &PredefinedMenuItem::undo(None),
            &PredefinedMenuItem::redo(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::cut(None),
            &PredefinedMenuItem::copy(None),
            &PredefinedMenuItem::paste(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::select_all(None),
        ])
        .unwrap();

    let help_menu = Submenu::new("Help", true);
    help_menu
        .append_items(&[
            &MenuItem::with_id(ID_ABOUT, "About Konstruado", true, None),
            &MenuItem::with_id(ID_README, "README", true, None),
        ])
        .unwrap();
    if cfg!(debug_assertions) {
        help_menu
            .append_items(&[
                &PredefinedMenuItem::separator(),
                &MenuItem::with_id(
                    "dioxus-toggle-dev-tools",
                    "Toggle Developer Tools",
                    true,
                    None,
                ),
                &MenuItem::with_id(
                    "dioxus-float-top",
                    "Float on Top (dev mode only)",
                    true,
                    None,
                ),
            ])
            .unwrap();
    }

    menu.append_items(&[&window_menu, &edit_menu, &help_menu])
        .unwrap();
    #[cfg(target_os = "macos")]
    {
        help_menu.set_as_help_menu_for_nsapp();
        window_menu.set_as_windows_menu_for_nsapp();
    }
    menu
}

/// Octicon `mark-github` (primer/octicons, MIT), 16×16. Toma el color del texto.
pub const GITHUB_MARK: &str = include_str!("../../../assets/icon/mark-github.svg");

/// «Código en GitHub» / «Source on GitHub»: abre el repo en el navegador del sistema.
#[component]
pub fn EnlaceGithub(etiqueta: String) -> Element {
    let svg = GITHUB_MARK.replacen("<svg ", "<svg aria-hidden=\"true\" fill=\"currentColor\" ", 1);
    rsx! {
        button {
            class: "gh-link",
            title: "{REPO}",
            onclick: move |_| abrir_url(REPO),
            span { class: "gh-mark", dangerous_inner_html: "{svg}" }
            span { "{etiqueta}" }
        }
    }
}

pub fn abrir_url(url: &str) {
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
pub fn Help(yo: Signal<Option<Persona>>, screen: Signal<Screen>, vista: Vista) -> Element {
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
            match vista {
                Vista::About => rsx! {
                    h1 { "About" }
                    div { class: "about",
                        p { strong { "Konstruado" } " {VERSION}" }
                        p { "{DESCRIPCION}" }
                        p { "{LICENSE}" }
                        EnlaceGithub { etiqueta: "Source on GitHub".to_string() }
                        p { class: "help", "{REPO}" }
                    }
                },
                Vista::Readme => rsx! {
                    h1 { "README" }
                    pre { class: "readme", "{README}" }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn about_trae_repo_y_readme() {
        // Version comes from [workspace.package]; HELP.md must not hardcode it.
        assert!(!VERSION.is_empty() && !VERSION.ends_with("-dev"));
        assert!(!README.contains(VERSION));
        assert!(REPO.contains("felipebrunet/konstruado"));
        assert!(README.contains("Not a chat"));
        assert!(README.contains("ES / EN"));
        assert!(!README.contains("cargo run"));
        assert!(!README.contains("CONTEXTO"));
        assert!(README.contains("job backup → seed → share"));
        assert!(README.contains("Restore from backup"));
        assert!(README.contains("KSTRBAK"));
        assert!(README.contains("Show the 25 words"));
        let _ = menu();
    }
}
