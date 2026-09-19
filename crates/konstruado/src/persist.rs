use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use konstruado_core::{Oferta, Obra, Persona, Rol};

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
}

fn tema_vivo() -> String {
    "vivo".into()
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
    serde_json::from_str(&raw).unwrap_or_default()
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
        };
        guardar(&e);
        let b = cargar();
        assert_eq!(b.yo.unwrap().nombre, "Don Dinero");
        assert_eq!(b.rol, Some(Rol::Mandante));
        assert_eq!(b.tema, "vivo");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
