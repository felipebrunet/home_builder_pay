use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use konstruado_core::{Obra, Oferta, Persona, RetiroOferta, Rol};

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
    /// La persona eligió el idioma (ES/EN). Si no, Android usa el del teléfono.
    #[serde(default)]
    pub idioma_fijo: bool,
    /// X25519 secret, base64. Stays in this data dir. Never gossiped.
    #[serde(default)]
    pub clave_sec: String,
    /// Spend key of the stagenet hot wallet, hex. Never gossiped.
    #[serde(default)]
    pub spend_sec: String,
    /// Obras archivadas solo en este equipo (ocultas del tablero; el almacén las conserva).
    #[serde(default)]
    pub obras_salidas: Vec<String>,
    /// Lápidas de ofertas retiradas (propias y de otros). Sin esto, una oferta
    /// quitada vuelve con el gossip del otro par.
    #[serde(default)]
    pub retiradas: Vec<RetiroOferta>,
}

/// Respaldo portable de obras/ofertas (sin seed, share ni claves).
/// Sirve para recuperar el perfil tras reinstalar; chain+share mandan para el dinero.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RespaldoPerfil {
    pub formato: String,
    #[serde(default)]
    pub obras: Vec<Obra>,
    #[serde(default)]
    pub ofertas: Vec<Oferta>,
}

pub const FORMATO_RESPALDO_OBRAS: &str = "konstruado-obras-v1";

/// Formato viejo (0.2.7): ya no se exporta; queda para pruebas de la importación.
#[allow(dead_code)]
pub fn exportar_perfil_obras(obras: &[Obra], ofertas: &[Oferta]) -> Result<String, String> {
    let r = RespaldoPerfil {
        formato: FORMATO_RESPALDO_OBRAS.into(),
        obras: obras.to_vec(),
        ofertas: ofertas.to_vec(),
    };
    serde_json::to_string_pretty(&r).map_err(|e| format!("No pude armar el respaldo: {e}"))
}

pub fn importar_perfil_obras(texto: &str) -> Result<RespaldoPerfil, String> {
    let r: RespaldoPerfil = serde_json::from_str(texto.trim())
        .map_err(|e| format!("Ese archivo no es un respaldo de obras de Konstruado: {e}"))?;
    if r.formato != FORMATO_RESPALDO_OBRAS && !r.formato.starts_with("konstruado-obras-") {
        return Err(format!("Formato de respaldo desconocido: {}", r.formato));
    }
    if r.obras.is_empty() && r.ofertas.is_empty() {
        return Err("El respaldo no trae obras ni ofertas.".into());
    }
    Ok(r)
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
        let _g = datos_test_lock();
        let dir = std::env::temp_dir().join(format!("konstruado-persist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let prev = std::env::var_os("KONSTRUADO_DATOS");
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
            idioma_fijo: true,
            clave_sec: String::new(),
            spend_sec: String::new(),
            obras_salidas: vec![],
            retiradas: vec![RetiroOferta {
                oferta_id: "o1".into(),
                autor_id: yo.id.clone(),
                cuando: 7,
                prueba: "ab".into(),
            }],
        };
        guardar(&e);
        let b = cargar();
        assert_eq!(b.yo.unwrap().nombre, "Don Dinero");
        assert_eq!(b.rol, Some(Rol::Mandante));
        assert_eq!(b.tema, "vivo");
        assert_eq!(b.idioma, "en");
        assert_eq!(b.retiradas.len(), 1);
        assert_eq!(b.retiradas[0].oferta_id, "o1");
        match prev {
            Some(v) => unsafe { std::env::set_var("KONSTRUADO_DATOS", v) },
            None => unsafe { std::env::remove_var("KONSTRUADO_DATOS") },
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Un estado.json de 0.2.5 (sin `retiradas`) se sigue leyendo.
    #[test]
    fn estado_viejo_sin_retiradas_se_lee() {
        let raw = r#"{"yo":null,"rol":null,"ofertas":[],"obras":[],"presentes":[],"tema":"vivo","idioma":"es","clave_sec":"","spend_sec":"","obras_salidas":[]}"#;
        let e: EstadoDisco = serde_json::from_str(raw).unwrap();
        assert!(e.retiradas.is_empty());
    }

    #[test]
    fn respaldo_obras_json_vacio_falla() {
        assert!(importar_perfil_obras(r#"{"formato":"konstruado-obras-v1"}"#).is_err());
        let ok = exportar_perfil_obras(&[], &[]).unwrap();
        // export allows empty vecs but import rejects empty
        assert!(importar_perfil_obras(&ok).is_err());
        let r = RespaldoPerfil {
            formato: FORMATO_RESPALDO_OBRAS.into(),
            obras: vec![],
            ofertas: vec![],
        };
        let raw = serde_json::to_string(&r).unwrap();
        assert!(importar_perfil_obras(&raw).is_err());
    }
}

/// Archivo con la URL propia del daemon stagenet (`daemon.url` en el dir de datos).
#[cfg(test)]
pub(crate) fn datos_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

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

    #[test]
    fn daemon_persistido_y_fallback() {
        let _g = datos_test_lock();
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
