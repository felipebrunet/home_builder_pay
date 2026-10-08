//! Portapapeles del escritorio. En Linux usa el de GTK (el mismo bucle que
//! la ventana de Dioxus). La semilla se copia sin pasarla al gestor del
//! portapapeles y se borra a los [`crate::caja::SEMILLA_PORTAPAPELES_SEG`]
//! segundos si el portapapeles todavía la tiene.

/// Copia texto común (dirección, view key). `false` si no hay portapapeles.
pub fn copiar(texto: &str) -> bool {
    imp::copiar(texto, true)
}

/// Copia un secreto y programa el borrado. `false` si no hay portapapeles.
pub fn copiar_secreto(texto: &str, segundos: u64) -> bool {
    if !imp::copiar(texto, false) {
        return false;
    }
    imp::borrar_si_sigue(zeroize::Zeroizing::new(texto.to_string()), segundos);
    true
}

#[cfg(target_os = "linux")]
mod imp {

    fn portapapeles() -> Option<gtk::Clipboard> {
        if !gtk::is_initialized_main_thread() {
            return None;
        }
        Some(gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD))
    }

    pub fn copiar(texto: &str, guardar: bool) -> bool {
        let Some(c) = portapapeles() else { return false };
        c.set_text(texto);
        if guardar {
            // Que sobreviva al cerrar la app (gestor del portapapeles), solo lo público.
            c.store();
        }
        true
    }

    pub fn borrar_si_sigue(secreto: zeroize::Zeroizing<String>, segundos: u64) {
        let seg = u32::try_from(segundos).unwrap_or(u32::MAX);
        gtk::glib::timeout_add_seconds_local_once(seg, move || {
            let Some(c) = portapapeles() else { return };
            let sigue = c.wait_for_text().is_some_and(|t| t.as_str() == secreto.as_str());
            if sigue {
                c.set_text("");
                c.clear();
            }
        });
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    pub fn copiar(_texto: &str, _guardar: bool) -> bool {
        false
    }
    pub fn borrar_si_sigue(_secreto: zeroize::Zeroizing<String>, _segundos: u64) {}
}
