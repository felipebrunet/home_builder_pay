//! Respaldo completo de punta a punta con la API que usa Android: exportar en un
//! equipo, restaurar en otro y "reiniciar". Cada fase corre en su propio proceso
//! (la carpeta de datos es global al proceso, igual que en el teléfono).

use std::path::PathBuf;
use std::process::Command;

use konstruado_ffi::KonstruadoApp;

const CLAVE: &str = "clave de prueba e2e";

fn dir(nombre: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("konstruado-e2e-{nombre}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn fase(nombre: &str, envs: &[(&str, String)]) -> String {
    let out = Command::new(std::env::current_exe().unwrap())
        .args([nombre, "--exact", "--ignored", "--nocapture", "--test-threads=1"])
        .envs(envs.iter().map(|(k, v)| (*k, v.as_str())))
        .output()
        .unwrap();
    let txt = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "fase {nombre} falló:\n{txt}");
    txt
}

fn valor(txt: &str, clave: &str) -> String {
    txt.lines()
        .find_map(|l| l.split_once(&format!("E2E {clave}=")).map(|(_, v)| v))
        .unwrap_or_else(|| panic!("falta {clave} en:\n{txt}"))
        .trim()
        .to_string()
}

#[test]
fn exportar_restaurar_y_reiniciar() {
    let a = dir("a");
    let b = dir("b");
    let archivo = a.join("respaldo.kbak");
    let t = fase(
        "fase_exportar",
        &[("E2E_DIR", a.display().to_string()), ("E2E_OUT", archivo.display().to_string())],
    );
    let (id, addr) = (valor(&t, "ID"), valor(&t, "ADDR"));
    let t = fase(
        "fase_restaurar",
        &[("E2E_DIR", b.display().to_string()), ("E2E_OUT", archivo.display().to_string())],
    );
    assert_eq!(valor(&t, "HAY_DATOS"), "false");
    // Nada en uso cambió todavía.
    assert!(!b.join("estado.json").exists() || !std::fs::read_to_string(b.join("estado.json")).unwrap().contains(&id));
    let t = fase("fase_reiniciar", &[("E2E_DIR", b.display().to_string())]);
    assert_eq!(valor(&t, "ID"), id);
    assert_eq!(valor(&t, "ADDR"), addr);
    assert_eq!(valor(&t, "NOMBRE"), "Felipe E2E");
    assert_eq!(valor(&t, "ROL"), "mandante");
    let _ = std::fs::remove_dir_all(&a);
    let _ = std::fs::remove_dir_all(&b);
}

fn app(dir: &str) -> std::sync::Arc<KonstruadoApp> {
    KonstruadoApp::nuevo(dir.into(), None, None, vec![], false).unwrap()
}

#[test]
#[ignore]
fn fase_exportar() {
    let Ok(d) = std::env::var("E2E_DIR") else { return };
    let a = app(&d);
    let p = a.crear_cuenta("Felipe E2E".into(), "mandante".into()).unwrap();
    let addr = a.crear_semilla().unwrap();
    assert!(a.estado_respaldo().falta);
    // Contraseña corta: no arma nada.
    assert!(a.exportar_respaldo("corta".into()).is_err());
    let bytes = a.exportar_respaldo(CLAVE.into()).unwrap();
    std::fs::write(std::env::var("E2E_OUT").unwrap(), &bytes).unwrap();
    a.respaldo_guardado().unwrap();
    let e = a.estado_respaldo();
    assert!(!e.falta && e.ultimo.is_some(), "{e:?}");
    println!("E2E ID={}", p.id);
    println!("E2E ADDR={addr}");
}

#[test]
#[ignore]
fn fase_restaurar() {
    let Ok(d) = std::env::var("E2E_DIR") else { return };
    let a = app(&d);
    assert!(!a.perfil().tiene_cuenta);
    let bytes = std::fs::read(std::env::var("E2E_OUT").unwrap()).unwrap();
    assert!(a.revisar_respaldo(bytes.clone(), "otra clave larga".into()).is_err());
    let r = a.revisar_respaldo(bytes.clone(), CLAVE.into()).unwrap();
    assert_eq!(r.nombre, "Felipe E2E");
    assert!(r.direccion.is_some());
    a.restaurar_respaldo(bytes, CLAVE.into(), r.hay_datos).unwrap();
    println!("E2E HAY_DATOS={}", r.hay_datos);
}

#[test]
#[ignore]
fn fase_reiniciar() {
    let Ok(d) = std::env::var("E2E_DIR") else { return };
    let a = app(&d);
    let p = a.perfil();
    assert!(p.tiene_cuenta);
    let b = a.billetera();
    println!("E2E ID={}", p.id);
    println!("E2E NOMBRE={}", p.nombre);
    println!("E2E ROL={}", p.rol);
    println!("E2E ADDR={}", b.direccion.unwrap_or_default());
    // Restaurado = respaldado.
    assert!(!a.estado_respaldo().falta);
}
