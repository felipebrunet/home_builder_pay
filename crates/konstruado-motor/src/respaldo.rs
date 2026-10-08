//! Respaldo completo (desde 0.2.8): un solo archivo cifrado con todo lo que hace
//! falta para volver a esta cuenta en otro equipo. Reemplaza los respaldos sueltos
//! de semilla, share y obras (que siguen entrando como importación avanzada).
//!
//! Contenido (JSON, dentro de [`xmr_joint::sobre`]: argon2id + XChaCha20-Poly1305):
//! - el perfil entero de `estado.json` (nombre, rol, obras, ofertas, retiradas,
//!   claves del perfil, tema e idioma),
//! - la semilla con su altura de restauración,
//! - el share FROST de cada caja, con el primer bloque que miró esa caja,
//! - el nodo propio (`daemon.url`), si hay.
//!
//! Restaurar es atómico: se descifra y valida todo en memoria, se arma la carpeta
//! nueva en `restaurar.tmp/` y recién al final se renombra a `restaurar.listo/`.
//! El cambio de carpetas lo hace [`aplicar_pendiente`] al arrancar, antes de leer
//! nada; lo que había queda entero en `previo-<fecha>/`. Nunca se mezcla a medias.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use xmr_joint::sobre;

use crate::caja::{self, MaterialCaja, ShareRespaldo, Tono};
use crate::i18n::Idioma;
use crate::persist::EstadoDisco;
use konstruado_core::{asegurar_clave, Obra, Rol};

pub const FORMATO: &str = "konstruado-respaldo";
pub const VERSION_CONTENIDO: u32 = 1;
/// Nombre sugerido: `konstruado-respaldo-AAAA-MM-DD.kbak`.
pub const EXTENSION: &str = "kbak";
pub const CLAVE_MINIMA: usize = 8;

const TEMPORAL: &str = "restaurar.tmp";
const LISTO: &str = "restaurar.listo";
const FASE_A: &str = ".fase-a";
const ULTIMO: &str = "ultimo-respaldo.json";
/// Lo que el respaldo reemplaza en la carpeta de datos. Lo demás (estado de Tor,
/// temporales) queda donde está.
const VIVOS: &[&str] = &["estado.json", "estado.json.tmp", "xmr", "daemon.url", ULTIMO];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Contenido {
    pub formato: String,
    pub version: u32,
    /// Unix, segundos.
    pub creado: i64,
    /// Versión de la app que lo armó.
    pub app: String,
    pub perfil: EstadoDisco,
    #[serde(default)]
    pub semilla: Option<String>,
    #[serde(default)]
    pub altura: Option<u64>,
    #[serde(default)]
    pub shares: Vec<ShareRespaldo>,
    #[serde(default)]
    pub daemon_url: Option<String>,
}

/// Lo que se muestra antes de restaurar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resumen {
    pub nombre: String,
    pub rol: Rol,
    pub creado: i64,
    pub app: String,
    pub n_obras: usize,
    pub n_ofertas: usize,
    pub n_shares: usize,
    pub direccion: Option<String>,
    pub altura: Option<u64>,
    pub daemon_url: Option<String>,
    /// En este equipo ya hay una cuenta, semilla o shares: restaurar los reemplaza.
    pub hay_datos: bool,
}

fn mias<'a>(obras: &'a [Obra], yo: &'a str) -> impl Iterator<Item = &'a Obra> + 'a {
    obras.iter().filter(move |o| o.mandante.id == yo || o.contratista.id == yo)
}

/// Arma el contenido. Exige una cuenta (nombre y rol).
pub fn armar(
    perfil: EstadoDisco,
    material: MaterialCaja,
    daemon_url: Option<String>,
    ahora: i64,
) -> Result<Contenido, String> {
    if !perfil.adentro() {
        return Err("codigo:respaldo-sin-cuenta".into());
    }
    Ok(Contenido {
        formato: FORMATO.into(),
        version: VERSION_CONTENIDO,
        creado: ahora,
        app: env!("CARGO_PKG_VERSION").into(),
        perfil,
        semilla: material.semilla,
        altura: material.altura,
        shares: material.shares,
        daemon_url,
    })
}

pub fn cifrar(c: &Contenido, clave: &str) -> Result<Vec<u8>, String> {
    cifrar_con(c, clave, sobre::ParamsKdf::default())
}

pub fn cifrar_con(c: &Contenido, clave: &str, p: sobre::ParamsKdf) -> Result<Vec<u8>, String> {
    if clave.chars().count() < CLAVE_MINIMA {
        return Err("codigo:respaldo-clave-corta".into());
    }
    let plano = zeroize::Zeroizing::new(serde_json::to_vec(c).map_err(|e| e.to_string())?);
    sobre::sellar_con(&plano, clave, p).map_err(|e| e.to_string())
}

pub fn descifrar(datos: &[u8], clave: &str) -> Result<Contenido, String> {
    let plano = sobre::abrir(datos, clave).map_err(|e| e.to_string())?;
    let c: Contenido = serde_json::from_slice(&plano).map_err(|_| "codigo:respaldo-danado".to_string())?;
    if c.formato != FORMATO {
        return Err("codigo:respaldo-no-es".into());
    }
    if c.version > VERSION_CONTENIDO {
        return Err(format!("codigo:respaldo-version:{}", c.version));
    }
    Ok(c)
}

/// Valida el contenido entero sin tocar el disco.
pub fn validar(c: &Contenido) -> Result<caja::ResumenCaja, String> {
    let p = &c.perfil;
    let yo = p.yo.as_ref().ok_or("codigo:respaldo-sin-cuenta")?;
    if p.rol.is_none() || yo.nombre.trim().is_empty() || yo.id.is_empty() {
        return Err("codigo:respaldo-sin-cuenta".into());
    }
    if !p.clave_sec.is_empty() {
        let (sec, pubk) = asegurar_clave(&p.clave_sec, &yo.clave_pub);
        if sec != p.clave_sec || (!yo.clave_pub.is_empty() && pubk != yo.clave_pub) {
            return Err("codigo:respaldo-perfil".into());
        }
    }
    if let Some(u) = &c.daemon_url {
        xmr_joint::validar_daemon_url(u).map_err(|_| "codigo:respaldo-nodo".to_string())?;
    }
    caja::validar_material(yo, &p.obras, c.semilla.as_deref(), &c.shares)
}

/// ¿Hay algo en esta carpeta que un respaldo reemplazaría?
pub fn hay_datos(dir: &Path) -> bool {
    let cuenta = std::fs::read_to_string(dir.join("estado.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<EstadoDisco>(&t).ok())
        .is_some_and(|e| e.yo.is_some());
    let xmr = dir.join("xmr");
    let semilla = xmr.join("semilla.txt").exists();
    let shares = std::fs::read_dir(&xmr).is_ok_and(|rd| {
        rd.flatten()
            .any(|e| e.file_name().to_string_lossy().ends_with(".share"))
    });
    cuenta || semilla || shares
}

fn resumen(c: &Contenido, m: caja::ResumenCaja, hay: bool) -> Resumen {
    let yo = c.perfil.yo.as_ref().map(|p| p.id.clone()).unwrap_or_default();
    Resumen {
        nombre: c.perfil.yo.as_ref().map(|p| p.nombre.clone()).unwrap_or_default(),
        rol: c.perfil.rol.unwrap_or(Rol::Mandante),
        creado: c.creado,
        app: c.app.clone(),
        n_obras: mias(&c.perfil.obras, &yo).count(),
        n_ofertas: c.perfil.ofertas.len(),
        n_shares: m.n_shares,
        direccion: m.direccion,
        altura: m.altura,
        daemon_url: c.daemon_url.clone(),
        hay_datos: hay,
    }
}

/// Descifra y valida, sin escribir nada. Para mostrar qué trae antes de restaurar.
pub fn revisar(dir: &Path, datos: &[u8], clave: &str) -> Result<Resumen, String> {
    let c = descifrar(datos, clave)?;
    let m = validar(&c)?;
    Ok(resumen(&c, m, hay_datos(dir)))
}

fn escribir(path: &Path, texto: &str, secreto: bool) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(if secreto { 0o600 } else { 0o644 })
        .open(path)?;
    f.write_all(texto.as_bytes())?;
    f.sync_all()
}

/// Paso 1 de restaurar: valida todo y deja la carpeta nueva lista en
/// `restaurar.listo/`. No toca nada de lo que está en uso. Si ya hay datos en este
/// equipo, hace falta `reemplazar = true` (la confirmación de peligro de la UI).
/// Después hay que reiniciar la app: [`aplicar_pendiente`] hace el cambio.
pub fn preparar(dir: &Path, datos: &[u8], clave: &str, reemplazar: bool) -> Result<Resumen, String> {
    let c = descifrar(datos, clave)?;
    let m = validar(&c)?;
    let hay = hay_datos(dir);
    if hay && !reemplazar {
        return Err("codigo:respaldo-hay-datos".into());
    }
    let archivos = caja::archivos_restauracion(c.semilla.as_deref(), c.altura, &c.shares)?;
    let tmp = dir.join(TEMPORAL);
    let _ = std::fs::remove_dir_all(&tmp);
    let armar = || -> Result<(), String> {
        std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
        let mut perfil = c.perfil.clone();
        // Las archivadas "solo en este equipo" son del equipo viejo: igual se traen.
        perfil.obras_salidas.retain(|id| perfil.obras.iter().any(|o| &o.id == id));
        let estado = serde_json::to_string_pretty(&perfil).map_err(|e| e.to_string())?;
        escribir(&tmp.join("estado.json"), &estado, true).map_err(|e| e.to_string())?;
        for (rel, texto) in &archivos {
            escribir(&tmp.join(rel), texto, true).map_err(|e| e.to_string())?;
        }
        if let Some(u) = &c.daemon_url {
            escribir(&tmp.join("daemon.url"), &format!("{}\n", u.trim()), false).map_err(|e| e.to_string())?;
        }
        // Lo restaurado cuenta como respaldado: el archivo existe y es este.
        let ult = Ultimo {
            cuando: c.creado,
            huella: huella_de(&perfil, c.semilla.as_deref(), &c.shares),
        };
        escribir(
            &tmp.join(ULTIMO),
            &serde_json::to_string(&ult).map_err(|e| e.to_string())?,
            false,
        )
        .map_err(|e| e.to_string())?;
        escribir(&tmp.join(FASE_A), "", false).map_err(|e| e.to_string())?;
        Ok(())
    };
    if let Err(e) = armar() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }
    // Punto de no retorno: un solo rename. Antes de esto no cambió nada en uso.
    let listo = dir.join(LISTO);
    let _ = std::fs::remove_dir_all(&listo);
    if let Err(e) = std::fs::rename(&tmp, &listo) {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e.to_string());
    }
    Ok(resumen(&c, m, hay))
}

/// Paso 2, al arrancar y antes de leer nada: si hay un `restaurar.listo/`, mueve
/// lo viejo a `previo-<fecha>/` y pone lo nuevo en su lugar. Se puede cortar en
/// cualquier punto y retomar: cada paso mira qué falta.
pub fn aplicar_pendiente(dir: &Path) -> Result<bool, String> {
    let listo = dir.join(LISTO);
    if !listo.is_dir() {
        let _ = std::fs::remove_dir_all(dir.join(TEMPORAL));
        return Ok(false);
    }
    let previo = || -> PathBuf {
        let base = format!("previo-{}", chrono::Local::now().format("%Y%m%d-%H%M%S"));
        let mut p = dir.join(&base);
        let mut n = 1;
        while p.exists() {
            p = dir.join(format!("{base}-{n}"));
            n += 1;
        }
        p
    };
    let mut destino: Option<PathBuf> = None;
    let mut mover_a_previo = |nombre: &std::ffi::OsStr| -> Result<(), String> {
        let src = dir.join(nombre);
        if src.symlink_metadata().is_err() {
            return Ok(());
        }
        let d = destino.get_or_insert_with(previo);
        std::fs::create_dir_all(&*d).map_err(|e| e.to_string())?;
        std::fs::rename(&src, d.join(nombre)).map_err(|e| e.to_string())
    };
    if listo.join(FASE_A).exists() {
        for n in VIVOS {
            mover_a_previo(std::ffi::OsStr::new(n))?;
        }
        std::fs::remove_file(listo.join(FASE_A)).map_err(|e| e.to_string())?;
    }
    let entradas: Vec<_> = std::fs::read_dir(&listo)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.file_name())
        .collect();
    for n in entradas {
        // Algo escrito entre el paso 1 y el reinicio (el bucle guarda cada segundo).
        mover_a_previo(&n)?;
        std::fs::rename(listo.join(&n), dir.join(&n)).map_err(|e| e.to_string())?;
    }
    std::fs::remove_dir(&listo).map_err(|e| e.to_string())?;
    Ok(true)
}

// ---------------------------------------------------------------- último respaldo

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Huella {
    pub obras: BTreeSet<String>,
    pub cajas: BTreeSet<String>,
    pub semilla: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Ultimo {
    cuando: i64,
    huella: Huella,
}

fn huella_de(p: &EstadoDisco, semilla: Option<&str>, shares: &[ShareRespaldo]) -> Huella {
    let yo = p.yo.as_ref().map(|q| q.id.clone()).unwrap_or_default();
    huella_de_partes(
        &yo,
        &p.obras,
        semilla.and_then(|t| xmr_joint::SeedBackup::parse(t).ok()).map(|b| b.address.clone()),
        shares.iter().map(|s| s.obra_id.clone()),
    )
}

/// Huella sin leer secretos: mis obras, la dirección de la semilla y las cajas armadas.
pub fn huella_de_partes(
    yo: &str,
    obras: &[Obra],
    semilla: Option<String>,
    cajas: impl IntoIterator<Item = String>,
) -> Huella {
    Huella {
        obras: mias(obras, yo).map(|o| o.id.clone()).collect(),
        cajas: cajas.into_iter().collect(),
        semilla,
    }
}

/// Arma y cifra el respaldo completo. Devuelve los bytes y la huella para
/// [`marcar_hecho`] una vez guardado el archivo.
pub fn exportar(
    dir: &Path,
    perfil: EstadoDisco,
    material: MaterialCaja,
    clave: &str,
    ahora: i64,
) -> Result<(Vec<u8>, Huella), String> {
    let h = huella(&perfil, &material);
    let daemon = std::fs::read_to_string(dir.join("daemon.url"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let c = armar(perfil, material, daemon, ahora)?;
    Ok((cifrar(&c, clave)?, h))
}

/// Guarda los bytes en `path` sin dejar un archivo a medias (temporal + rename), en 0600.
pub fn guardar_archivo(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension(format!("{EXTENSION}.tmp"));
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .map_err(|e| e.to_string())?;
    f.write_all(bytes).and_then(|_| f.sync_all()).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

/// Lo que hoy entraría en un respaldo.
pub fn huella(perfil: &EstadoDisco, material: &MaterialCaja) -> Huella {
    huella_de(perfil, material.semilla.as_deref(), &material.shares)
}

pub fn marcar_hecho(dir: &Path, h: Huella, ahora: i64) -> Result<(), String> {
    let raw = serde_json::to_string(&Ultimo { cuando: ahora, huella: h }).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{ULTIMO}.tmp"));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dir.join(ULTIMO)).map_err(|e| e.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EstadoRespaldo {
    pub ultimo: Option<i64>,
    /// Hay obras, cajas o una semilla que el último respaldo no tiene.
    pub falta: bool,
    /// Hay algo que respaldar.
    pub hay_algo: bool,
}

pub fn estado(dir: &Path, actual: &Huella) -> EstadoRespaldo {
    let ult = std::fs::read_to_string(dir.join(ULTIMO))
        .ok()
        .and_then(|t| serde_json::from_str::<Ultimo>(&t).ok());
    let hay_algo = actual.semilla.is_some() || !actual.obras.is_empty() || !actual.cajas.is_empty();
    let falta = match &ult {
        None => hay_algo,
        Some(u) => {
            !actual.obras.is_subset(&u.huella.obras)
                || !actual.cajas.is_subset(&u.huella.cajas)
                || (actual.semilla.is_some() && actual.semilla != u.huella.semilla)
        }
    };
    EstadoRespaldo {
        ultimo: ult.map(|u| u.cuando),
        falta,
        hay_algo,
    }
}

/// Línea de estado del respaldo (Billetera y aviso del tablero).
pub fn texto_estado(e: &EstadoRespaldo, lang: Idioma) -> (Tono, String) {
    let es = lang == Idioma::Es;
    let cuando = e.ultimo.map(|t| lang.fmt_cuando(t));
    match (cuando, e.falta) {
        (None, true) => (
            Tono::Error,
            if es { "Todavía no hiciste un respaldo completo." } else { "You have not made a full backup yet." }.into(),
        ),
        (None, false) => (
            Tono::Apagado,
            if es { "Sin respaldo todavía: no hay nada que respaldar." } else { "No backup yet: nothing to back up." }.into(),
        ),
        (Some(c), true) => (
            Tono::Espera,
            if es {
                format!("Hay obras o cajas nuevas desde el último respaldo ({c}). Exportalo de nuevo.")
            } else {
                format!("There are new jobs or escrows since the last backup ({c}). Export it again.")
            },
        ),
        (Some(c), false) => (
            Tono::Ok,
            if es { format!("Último respaldo completo: {c}.") } else { format!("Last full backup: {c}.") },
        ),
    }
}

/// Ayuda de la sección de respaldo (mismo texto en las dos apps).
pub fn ayuda(es: bool) -> Vec<&'static str> {
    if es {
        vec![
            "Un solo archivo, cifrado con tu contraseña: la semilla con su altura, tus obras y ofertas, el share de cada caja, tu nombre, tu rol y el nodo.",
            "Sin la contraseña no se abre y no hay forma de recuperarla. Guardá el archivo fuera de este equipo.",
            "Exportalo de nuevo después de crear o unirte a una obra y cuando se arma una caja 2-de-2: ese share nuevo solo está en el respaldo nuevo.",
            "Después de restaurar, lo más nuevo del trato (notas, porcentajes, pagos) baja del otro por la sala cuando los dos están en línea.",
        ]
    } else {
        vec![
            "One file, encrypted with your password: the seed with its height, your jobs and offers, each escrow's share, your name, your role and the node.",
            "It does not open without the password, and the password cannot be recovered. Keep the file off this device.",
            "Export it again after you create or join a job and when a 2-of-2 escrow is set up: that new share is only in the new backup.",
            "After a restore, the newest deal progress (notes, percentages, payments) syncs from the other party through the room when both are online.",
        ]
    }
}

/// Nombre sugerido para el archivo.
pub fn nombre_archivo() -> String {
    format!("konstruado-respaldo-{}.{EXTENSION}", chrono::Local::now().format("%Y-%m-%d"))
}

/// Mensaje legible para los `codigo:respaldo-*`. Lo demás va a `caja::aviso_humano`.
pub fn aviso(code: &str, es: bool) -> String {
    let t = |a: &str, b: &str| if es { a.to_string() } else { b.to_string() };
    match code {
        "codigo:respaldo-no-es" => t("Ese archivo no es un respaldo completo de Konstruado.", "That file is not a Konstruado full backup."),
        "codigo:respaldo-danado" => t("El respaldo está dañado o incompleto.", "The backup is damaged or incomplete."),
        "codigo:respaldo-clave" => t("La contraseña no abre este respaldo (o el archivo fue modificado).", "The password does not open this backup (or the file was modified)."),
        "codigo:respaldo-clave-vacia" => t("Escribí la contraseña.", "Type the password."),
        "codigo:respaldo-clave-corta" => t("La contraseña tiene que tener al menos 8 caracteres.", "The password needs at least 8 characters."),
        "codigo:respaldo-sin-cuenta" => t("Falta la cuenta (nombre y rol) en el respaldo.", "The backup has no account (name and role)."),
        "codigo:respaldo-perfil" => t("Las claves del perfil no coinciden: el respaldo está dañado.", "The profile keys do not match: the backup is damaged."),
        "codigo:respaldo-nodo" => t("El nodo guardado en el respaldo no es válido.", "The node saved in the backup is not valid."),
        "codigo:respaldo-hay-datos" => t("Este equipo ya tiene una cuenta. Confirmá que querés reemplazarla.", "This device already has an account. Confirm you want to replace it."),
        "codigo:share-duplicado" => t("El respaldo trae dos shares para la misma obra.", "The backup has two shares for the same job."),
        c if c.starts_with("codigo:respaldo-version:") => t(
            "Este respaldo es de una versión más nueva de Konstruado. Actualizá la app.",
            "This backup is from a newer Konstruado version. Update the app.",
        ),
        c => caja::aviso_humano(c, es),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use konstruado_core::{Aceptacion, EstadoObra, Oferta, Persona};
    use rand_core::OsRng;
    use xmr_joint::dkg::{DkgParty, JointAccount, Party};
    use xmr_joint::wallet::SingleWallet;
    use xmr_joint::{Net, SeedBackup};

    const RAPIDO: sobre::ParamsKdf = sobre::ParamsKdf { memoria_kib: 8 * 1024, pasadas: 1, hilos: 1 };
    const CLAVE: &str = "una clave larga";

    fn cuentas(obra: &str) -> (JointAccount, JointAccount) {
        let mut rng = OsRng;
        let (mut m, c1) = DkgParty::start(Party::Mandante, obra, Net::Stagenet, &mut rng).unwrap();
        let (mut c, c2) = DkgParty::start(Party::Contratista, obra, Net::Stagenet, &mut rng).unwrap();
        let s1 = m.ingest_commit(&c2, &mut rng).unwrap();
        let s2 = c.ingest_commit(&c1, &mut rng).unwrap();
        c.ingest_share(&s1, &mut rng).unwrap();
        let hecho = m.ingest_share(&s2, &mut rng).unwrap();
        let cuenta_c = c.ingest_view(&hecho.view.unwrap()).unwrap();
        (hecho.account.unwrap(), cuenta_c)
    }

    struct Caso {
        contenido: Contenido,
        /// Share del contratista de la misma obra (para probar el rechazo).
        share_otro: String,
    }

    fn caso() -> Caso {
        let mut m = Persona::nueva("felipe").unwrap();
        let (sec, pubk) = asegurar_clave("", "");
        m.clave_pub = pubk;
        let c = Persona::nueva("caco").unwrap();
        let o = Oferta::publicar(m.clone(), "Super casa", 1_000, 500, vec![]).unwrap();
        let a = Aceptacion::de(&o, c.clone(), 500).unwrap();
        let mut obra = Obra::desde_oferta(o.clone(), a).unwrap();
        obra.estado = EstadoObra::EnMarcha;
        let (cm, cc) = cuentas(&obra.id);
        let (w, words) = SingleWallet::generate(&mut OsRng, Net::Stagenet).unwrap();
        let semilla = SeedBackup {
            net: Net::Stagenet,
            address: w.address().to_string(),
            height: Some(2_000_000),
            words,
        }
        .to_text();
        let perfil = EstadoDisco {
            yo: Some(m.clone()),
            rol: Some(Rol::Mandante),
            ofertas: vec![o],
            obras: vec![obra.clone()],
            presentes: vec![m.clone()],
            tema: "oscuro".into(),
            idioma: "en".into(),
            clave_sec: sec,
            spend_sec: String::new(),
            obras_salidas: vec![],
            retiradas: vec![konstruado_core::RetiroOferta {
                oferta_id: "o-vieja".into(),
                autor_id: m.id.clone(),
                cuando: 7,
                prueba: "ab".into(),
            }],
        };
        let material = MaterialCaja {
            semilla: Some(semilla),
            altura: Some(1_999_000),
            shares: vec![ShareRespaldo {
                obra_id: obra.id.clone(),
                texto: cm.backup().unwrap().to_text(),
                desde: Some(2_000_100),
            }],
        };
        let contenido = armar(perfil, material, Some("http://100.64.0.2:38081".into()), 1_791_000_000).unwrap();
        Caso {
            contenido,
            share_otro: cc.backup().unwrap().to_text(),
        }
    }

    fn carpeta(nombre: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("konstruado-resp-{nombre}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Foto de una carpeta: (ruta relativa, contenido) de cada archivo.
    fn foto(dir: &Path) -> Vec<(String, Vec<u8>)> {
        fn walk(base: &Path, d: &Path, out: &mut Vec<(String, Vec<u8>)>) {
            for e in std::fs::read_dir(d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(base, &p, out);
                } else {
                    out.push((p.strip_prefix(base).unwrap().display().to_string(), std::fs::read(&p).unwrap()));
                }
            }
        }
        let mut v = Vec::new();
        walk(dir, dir, &mut v);
        v.sort();
        v
    }

    #[test]
    fn ida_y_vuelta_restaura_todo() {
        let k = caso();
        let bytes = cifrar_con(&k.contenido, CLAVE, RAPIDO).unwrap();
        assert!(sobre::es_sobre(&bytes));
        let dir = carpeta("ida");
        let r = revisar(&dir, &bytes, CLAVE).unwrap();
        assert_eq!(r.nombre, "felipe");
        assert_eq!((r.n_obras, r.n_ofertas, r.n_shares), (1, 1, 1));
        assert_eq!(r.altura, Some(2_000_000));
        assert!(!r.hay_datos);
        preparar(&dir, &bytes, CLAVE, false).unwrap();
        // Nada en uso cambió todavía: todo espera en restaurar.listo/.
        assert!(!dir.join("estado.json").exists());
        assert!(aplicar_pendiente(&dir).unwrap());
        assert!(!dir.join(LISTO).exists());
        let e: EstadoDisco = serde_json::from_str(&std::fs::read_to_string(dir.join("estado.json")).unwrap()).unwrap();
        let p = &k.contenido.perfil;
        assert_eq!(e.yo.as_ref().unwrap().id, p.yo.as_ref().unwrap().id);
        assert_eq!(e.clave_sec, p.clave_sec);
        assert_eq!((e.tema.as_str(), e.idioma.as_str()), ("oscuro", "en"));
        assert_eq!(e.obras.len(), 1);
        assert_eq!(e.retiradas.len(), 1);
        // Semilla con la altura de restauración (la más baja de las dos) y libro desde ahí.
        let s = SeedBackup::parse(&std::fs::read_to_string(dir.join("xmr/semilla.txt")).unwrap()).unwrap();
        assert_eq!(s.height, Some(1_999_000));
        let libro: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("xmr/libro.json")).unwrap()).unwrap();
        assert_eq!(libro["desde"], 1_999_000);
        let id = &p.obras[0].id;
        assert!(dir.join(format!("xmr/{id}.share")).exists());
        let caja: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join(format!("xmr/caja-{id}.json"))).unwrap()).unwrap();
        assert_eq!(caja["desde"], 2_000_100);
        assert_eq!(std::fs::read_to_string(dir.join("daemon.url")).unwrap().trim(), "http://100.64.0.2:38081");
        // Secretos en 0600.
        use std::os::unix::fs::PermissionsExt;
        let modo = std::fs::metadata(dir.join("xmr/semilla.txt")).unwrap().permissions().mode() & 0o777;
        assert_eq!(modo, 0o600);
        // Lo restaurado cuenta como respaldado.
        let mat = MaterialCaja {
            semilla: k.contenido.semilla.clone(),
            altura: None,
            shares: k.contenido.shares.clone(),
        };
        let est = estado(&dir, &huella(&e, &mat));
        assert!(!est.falta && est.ultimo == Some(1_791_000_000));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clave_equivocada_no_abre_ni_escribe() {
        let k = caso();
        let bytes = cifrar_con(&k.contenido, CLAVE, RAPIDO).unwrap();
        let dir = carpeta("clave");
        assert_eq!(revisar(&dir, &bytes, "otra clave larga").unwrap_err(), "codigo:respaldo-clave");
        assert_eq!(preparar(&dir, &bytes, "otra clave larga", true).unwrap_err(), "codigo:respaldo-clave");
        assert!(foto(&dir).is_empty());
        assert!(cifrar_con(&k.contenido, "corta", RAPIDO).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn archivo_corrupto_o_ajeno_se_rechaza() {
        let k = caso();
        let bytes = cifrar_con(&k.contenido, CLAVE, RAPIDO).unwrap();
        let dir = carpeta("corrupto");
        let mut t = bytes.clone();
        let mitad = t.len() / 2;
        t[mitad] ^= 0x40;
        assert_eq!(revisar(&dir, &t, CLAVE).unwrap_err(), "codigo:respaldo-clave");
        assert_eq!(revisar(&dir, &bytes[..30], CLAVE).unwrap_err(), "codigo:respaldo-danado");
        assert_eq!(revisar(&dir, b"konstruado single-sig v1\n", CLAVE).unwrap_err(), "codigo:respaldo-no-es");
        // Cifrado bien pero sin JSON adentro.
        let raro = sobre::sellar_con(b"no soy json", CLAVE, RAPIDO).unwrap();
        assert_eq!(revisar(&dir, &raro, CLAVE).unwrap_err(), "codigo:respaldo-danado");
        // Un formato de contenido más nuevo pide actualizar.
        let mut nuevo = k.contenido.clone();
        nuevo.version = 9;
        let b = cifrar_con(&nuevo, CLAVE, RAPIDO).unwrap();
        assert_eq!(revisar(&dir, &b, CLAVE).unwrap_err(), "codigo:respaldo-version:9");
        assert!(foto(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn share_que_no_es_de_mi_rol_u_obra_se_rechaza() {
        let k = caso();
        let dir = carpeta("share");
        // El share del contratista en el respaldo del mandante.
        let mut c = k.contenido.clone();
        c.shares[0].texto = k.share_otro.clone();
        let b = cifrar_con(&c, CLAVE, RAPIDO).unwrap();
        assert_eq!(preparar(&dir, &b, CLAVE, false).unwrap_err(), "codigo:share-rol");
        // Un share de una obra que no está en el perfil.
        let (otra, _) = cuentas("obra-ajena");
        let mut c = k.contenido.clone();
        c.shares.push(ShareRespaldo {
            obra_id: "obra-ajena".into(),
            texto: otra.backup().unwrap().to_text(),
            desde: None,
        });
        let b = cifrar_con(&c, CLAVE, RAPIDO).unwrap();
        assert_eq!(preparar(&dir, &b, CLAVE, false).unwrap_err(), "codigo:share-obra");
        // Etiqueta de obra que no coincide con el share.
        let mut c = k.contenido.clone();
        c.shares[0].obra_id = "otra-cosa".into();
        let b = cifrar_con(&c, CLAVE, RAPIDO).unwrap();
        assert_eq!(preparar(&dir, &b, CLAVE, false).unwrap_err(), "codigo:share-obra");
        // Semilla con otra dirección.
        let mut c = k.contenido.clone();
        c.semilla = Some(c.semilla.unwrap().replacen("address 5", "address 7", 1));
        let b = cifrar_con(&c, CLAVE, RAPIDO).unwrap();
        assert!(preparar(&dir, &b, CLAVE, false).unwrap_err().starts_with("codigo:semilla"));
        assert!(foto(&dir).is_empty(), "nada escrito si algo no valida");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn con_datos_pide_confirmar_y_nunca_mezcla() {
        let k = caso();
        let bytes = cifrar_con(&k.contenido, CLAVE, RAPIDO).unwrap();
        let dir = carpeta("atomico");
        // Un equipo con otra cuenta, otra semilla y un share de otra obra.
        let viejo = EstadoDisco {
            yo: Some(Persona::nueva("otra").unwrap()),
            rol: Some(Rol::Contratista),
            ..Default::default()
        };
        std::fs::write(dir.join("estado.json"), serde_json::to_string(&viejo).unwrap()).unwrap();
        std::fs::create_dir_all(dir.join("xmr")).unwrap();
        std::fs::write(dir.join("xmr/semilla.txt"), "semilla vieja").unwrap();
        std::fs::write(dir.join("xmr/obra-vieja.share"), "share viejo").unwrap();
        std::fs::write(dir.join("daemon.url"), "http://10.0.0.1:38081\n").unwrap();
        std::fs::create_dir_all(dir.join("arti")).unwrap();
        std::fs::write(dir.join("arti/estado"), "tor").unwrap();
        let antes = foto(&dir);
        assert!(hay_datos(&dir));
        // Sin confirmar: error y nada cambia.
        assert_eq!(preparar(&dir, &bytes, CLAVE, false).unwrap_err(), "codigo:respaldo-hay-datos");
        assert_eq!(foto(&dir), antes);
        // Un fallo a mitad de armar la carpeta nueva no deja rastros.
        // (Un archivo donde va la carpeta temporal: remove_dir_all no lo borra y create_dir_all falla.)
        std::fs::write(dir.join(TEMPORAL), "estorbo: no es carpeta").unwrap();
        let r = preparar(&dir, &bytes, CLAVE, true);
        assert!(r.is_err());
        std::fs::remove_file(dir.join(TEMPORAL)).unwrap();
        assert_eq!(foto(&dir), antes);
        assert!(!dir.join(LISTO).exists());
        // Confirmado: prepara sin tocar lo vivo…
        let r = preparar(&dir, &bytes, CLAVE, true).unwrap();
        assert!(r.hay_datos);
        let vivos: Vec<_> = foto(&dir).into_iter().filter(|(p, _)| !p.starts_with(LISTO)).collect();
        assert_eq!(vivos, antes);
        // …el bucle de la app sigue guardando antes del reinicio…
        std::fs::write(dir.join("estado.json"), "escrito después").unwrap();
        // …y al arrancar cambia todo junto. Lo viejo queda entero en previo-*/.
        assert!(aplicar_pendiente(&dir).unwrap());
        assert!(!dir.join("xmr/obra-vieja.share").exists());
        assert_eq!(std::fs::read_to_string(dir.join("daemon.url")).unwrap().trim(), "http://100.64.0.2:38081");
        assert_eq!(std::fs::read_to_string(dir.join("arti/estado")).unwrap(), "tor");
        let previo = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .find(|p| p.file_name().unwrap().to_string_lossy().starts_with("previo-"))
            .unwrap();
        assert_eq!(std::fs::read_to_string(previo.join("xmr/obra-vieja.share")).unwrap(), "share viejo");
        assert_eq!(std::fs::read_to_string(previo.join("xmr/semilla.txt")).unwrap(), "semilla vieja");
        assert_eq!(std::fs::read_to_string(previo.join("estado.json")).unwrap(), "escrito después");
        let e: EstadoDisco = serde_json::from_str(&std::fs::read_to_string(dir.join("estado.json")).unwrap()).unwrap();
        assert_eq!(e.yo.unwrap().nombre, "felipe");
        // Sin nada pendiente, arrancar no hace nada.
        assert!(!aplicar_pendiente(&dir).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn aplicar_se_retoma_si_se_corta_a_mitad() {
        let k = caso();
        let bytes = cifrar_con(&k.contenido, CLAVE, RAPIDO).unwrap();
        let dir = carpeta("retoma");
        std::fs::write(dir.join("estado.json"), "viejo").unwrap();
        preparar(&dir, &bytes, CLAVE, true).unwrap();
        // Simula un corte: la fase A movió lo viejo y la B alcanzó a mover estado.json.
        let previo = dir.join("previo-manual");
        std::fs::create_dir_all(&previo).unwrap();
        std::fs::rename(dir.join("estado.json"), previo.join("estado.json")).unwrap();
        std::fs::remove_file(dir.join(LISTO).join(FASE_A)).unwrap();
        std::fs::rename(dir.join(LISTO).join("estado.json"), dir.join("estado.json")).unwrap();
        assert!(aplicar_pendiente(&dir).unwrap());
        assert!(dir.join("xmr/semilla.txt").exists());
        let e: EstadoDisco = serde_json::from_str(&std::fs::read_to_string(dir.join("estado.json")).unwrap()).unwrap();
        assert_eq!(e.yo.unwrap().nombre, "felipe");
        assert_eq!(std::fs::read_to_string(previo.join("estado.json")).unwrap(), "viejo");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recordatorio_tras_obra_o_caja_nueva() {
        let dir = carpeta("ultimo");
        let mut h = Huella {
            obras: ["o1".to_string()].into(),
            cajas: BTreeSet::new(),
            semilla: Some("5abc".into()),
        };
        let e = estado(&dir, &h);
        assert!(e.falta && e.ultimo.is_none());
        assert_eq!(texto_estado(&e, Idioma::Es).1, "Todavía no hiciste un respaldo completo.");
        marcar_hecho(&dir, h.clone(), 1_791_000_000).unwrap();
        let e = estado(&dir, &h);
        assert!(!e.falta && e.ultimo == Some(1_791_000_000));
        assert_eq!(texto_estado(&e, Idioma::Es).0, Tono::Ok);
        // DKG terminado: hay un share nuevo que el respaldo no tiene.
        h.cajas.insert("o1".into());
        assert!(estado(&dir, &h).falta);
        assert!(texto_estado(&estado(&dir, &h), Idioma::En).1.contains("Export it again"));
        marcar_hecho(&dir, h.clone(), 1_791_000_100).unwrap();
        // Obra nueva.
        h.obras.insert("o2".into());
        assert!(estado(&dir, &h).falta);
        // Una obra que ya no está no pide respaldo.
        h.obras.remove("o2");
        h.obras.remove("o1");
        assert!(!estado(&dir, &h).falta);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Herramienta de capturas: deja en `KONSTRUADO_DEMO` un share de mandante y un
    /// libro de caja con el fondeo de la partida 1 en el bloque `KONSTRUADO_DEMO_ALTURA`.
    #[test]
    #[ignore]
    fn genera_demo_capturas() {
        let (Ok(d), Ok(h)) = (std::env::var("KONSTRUADO_DEMO"), std::env::var("KONSTRUADO_DEMO_ALTURA")) else {
            return;
        };
        let dir = PathBuf::from(d);
        let e: EstadoDisco = serde_json::from_str(&std::fs::read_to_string(dir.join("estado.json")).unwrap()).unwrap();
        let obra = &e.obras[0];
        let txid = obra.partidas[0].fondeo_txid.clone().unwrap();
        let (cm, _) = cuentas(&obra.id);
        let h: u64 = h.parse().unwrap();
        std::fs::create_dir_all(dir.join("xmr")).unwrap();
        std::fs::write(dir.join(format!("xmr/{}.share", obra.id)), cm.backup().unwrap().to_text()).unwrap();
        let libro = serde_json::json!({
            "direccion": cm.address().to_string(), "desde": h - 5, "hasta": h + 2, "listo": true, "retro": 0,
            "entradas": [{"altura": h, "monto": 20_000_000_000u64, "tx": txid, "indice": 0, "raw": "00"}],
        });
        std::fs::write(dir.join(format!("xmr/caja-{}.json", obra.id)), libro.to_string()).unwrap();
    }

    /// Los respaldos sueltos de antes siguen entrando por la importación avanzada.
    #[test]
    fn respaldos_viejos_siguen_entrando() {
        let k = caso();
        let semilla = k.contenido.semilla.as_deref().unwrap();
        let s = SeedBackup::parse(semilla).unwrap();
        assert_eq!(s.height, Some(2_000_000));
        let share = xmr_joint::ShareBackup::parse(&k.contenido.shares[0].texto).unwrap();
        assert_eq!(share.role, "mandante");
        let p = &k.contenido.perfil;
        let yo = p.yo.as_ref().unwrap();
        caja::validar_material(yo, &p.obras, Some(semilla), &k.contenido.shares).unwrap();
        let json = crate::persist::exportar_perfil_obras(&p.obras, &p.ofertas).unwrap();
        let r = crate::persist::importar_perfil_obras(&json).unwrap();
        assert_eq!(r.obras.len(), 1);
        // Y no se confunden con un respaldo completo.
        assert!(!sobre::es_sobre(semilla.as_bytes()));
        let dir = carpeta("viejo");
        assert_eq!(revisar(&dir, semilla.as_bytes(), CLAVE).unwrap_err(), "codigo:respaldo-no-es");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
