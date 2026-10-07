use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use konstruado_core::{Obra, Oferta, Persona, Rol};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EstadoDisco {
    pub yo: Option<Persona>,
    pub rol: Option<Rol>,
    #[serde(default)]
    pub ofertas: Vec<Oferta>,
    #[serde(default)]
    pub obras: Vec<Obra>,
    #[serde(default)]
    pub presentes: Vec<Persona>,
    #[serde(default = "tema_vivo")]
    pub tema: String,
    #[serde(default = "idioma_es")]
    pub idioma: String,
    /// X25519 secret, base64. Stays in this data dir. Never gossiped.
    #[serde(default)]
    pub clave_sec: String,
    /// Spend key of the stagenet hot wallet, hex. Never gossiped.
    #[serde(default)]
    pub spend_sec: String,
}

fn tema_vivo() -> String {
    "vivo".into()
}

fn idioma_es() -> String {
    "es".into()
}

impl EstadoDisco {
    pub fn adentro(&self) -> bool {
        self.yo.is_some() && self.rol.is_some()
    }
}

pub fn dir() -> PathBuf {
    if let Ok(p) = std::env::var("KONSTRUADO_DATOS") {
        return PathBuf::from(p);
    }
    std::env::var("HOME")
        .map(|h| PathBuf::from(h).join(".konstruado"))
        .unwrap_or_else(|_| PathBuf::from(".konstruado"))
}

fn archivo() -> PathBuf {
    dir().join("estado.json")
}

pub fn cargar() -> EstadoDisco {
    let path = archivo();
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return EstadoDisco::default();
    };
    match serde_json::from_str(&raw) {
        Ok(e) => e,
        Err(_) => {
            let bak = path.with_extension("json.bak");
            let _ = std::fs::copy(&path, &bak);
            EstadoDisco::default()
        }
    }
}

pub fn guardar(e: &EstadoDisco) {
    if e.yo.is_none() {
        return;
    }
    let dir = dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = archivo();
    let tmp = dir.join("estado.json.tmp");
    let Ok(raw) = serde_json::to_string_pretty(e) else {
        return;
    };
    if std::fs::write(&tmp, raw).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let dir = std::env::temp_dir().join(format!("konstruado-persist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        unsafe { std::env::set_var("KONSTRUADO_DATOS", &dir) };
        let yo = Persona::nueva("Don Dinero").unwrap();
        let e = EstadoDisco {
            yo: Some(yo.clone()),
            rol: Some(Rol::Mandante),
            ofertas: vec![],
            obras: vec![],
            presentes: vec![yo.clone()],
            tema: "vivo".into(),
            idioma: "en".into(),
            clave_sec: String::new(),
            spend_sec: String::new(),
        };
        guardar(&e);
        let b = cargar();
        assert_eq!(b.yo.unwrap().nombre, "Don Dinero");
        assert_eq!(b.rol, Some(Rol::Mandante));
        assert_eq!(b.tema, "vivo");
        assert_eq!(b.idioma, "en");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Archivo con la URL propia del daemon stagenet (`daemon.url` en el dir de datos).
fn path_daemon() -> PathBuf {
    dir().join("daemon.url")
}

/// Al arrancar: carga `daemon.url`, si no `KONSTRUADO_DAEMON`, si no el público.
/// Misma semántica en escritorio y Android (vía FFI).
pub fn cargar_daemon_al_arrancar() -> Result<(), String> {
    let p = path_daemon();
    match std::fs::read_to_string(&p) {
        Ok(s) if !s.trim().is_empty() => {
            xmr_joint::fijar_daemon(Some(s.trim()))?;
        }
        Ok(_) | Err(_) => {
            if let Ok(env) = std::env::var("KONSTRUADO_DAEMON") {
                if !env.trim().is_empty() {
                    xmr_joint::fijar_daemon(Some(env.trim()))?;
                    return Ok(());
                }
            }
            let _ = xmr_joint::fijar_daemon(None);
        }
    }
    Ok(())
}

/// Activa y persiste un nodo propio. `None` / vacío = público (borra `daemon.url`).
/// Devuelve la URL activa.
pub fn fijar_daemon_persistido(url: Option<&str>) -> Result<String, String> {
    let p = path_daemon();
    let _ = std::fs::create_dir_all(dir());
    match url.map(str::trim).filter(|s| !s.is_empty()) {
        None => {
            xmr_joint::fijar_daemon(None)?;
            let _ = std::fs::remove_file(&p);
            Ok(xmr_joint::daemon_url())
        }
        Some(raw) => {
            let ok = xmr_joint::validar_daemon_url(raw)?;
            xmr_joint::fijar_daemon(Some(&ok))?;
            std::fs::write(&p, format!("{ok}\n"))
                .map_err(|e| format!("No pude guardar el nodo: {e}"))?;
            Ok(ok)
        }
    }
}

#[cfg(test)]
mod daemon_tests {
    use super::*;
    use std::sync::Mutex;

    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn daemon_persistido_y_fallback() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("konstruado-daemon-persist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let prev = std::env::var_os("KONSTRUADO_DATOS");
        unsafe { std::env::set_var("KONSTRUADO_DATOS", &dir) };
        let _ = xmr_joint::fijar_daemon(None);
        cargar_daemon_al_arrancar().unwrap();
        assert!(xmr_joint::daemon_es_defecto());
        assert_eq!(xmr_joint::daemon_url(), xmr_joint::STAGENET_DAEMON);
        let ok = fijar_daemon_persistido(Some("http://100.64.0.2:38081")).unwrap();
        assert_eq!(ok, "http://100.64.0.2:38081");
        let _ = xmr_joint::fijar_daemon(None);
        cargar_daemon_al_arrancar().unwrap();
        assert_eq!(xmr_joint::daemon_url(), "http://100.64.0.2:38081");
        fijar_daemon_persistido(None).unwrap();
        let _ = xmr_joint::fijar_daemon(None);
        cargar_daemon_al_arrancar().unwrap();
        assert!(xmr_joint::daemon_es_defecto());
        match prev {
            Some(v) => unsafe { std::env::set_var("KONSTRUADO_DATOS", v) },
            None => unsafe { std::env::remove_var("KONSTRUADO_DATOS") },
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
