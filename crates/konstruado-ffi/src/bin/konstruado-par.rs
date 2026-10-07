//! Par sin ventana para el smoke de interop. Usa exactamente la fachada FFI
//! (`KonstruadoApp`) que llama Android, con su propia carpeta de datos.
//!
//! mandante:    publica una oferta, espera la caja 2-de-2, propone encerrar la partida 1.
//! contratista: acepta la oferta, espera la caja, "Confirmar y fondear".
//!
//! Imprime líneas `PASO …` para que un script verifique el flujo.

use std::time::{Duration, Instant};

use konstruado_ffi::KonstruadoApp;

fn main() {
    let mut datos = String::new();
    let mut nombre = String::new();
    let mut rol = String::new();
    let mut destinos = Vec::new();
    let mut escritorio = false;
    let mut socks: Option<(String, u16)> = None;
    let mut segundos = 90u64;
    let mut preparar = false;
    let mut a = std::env::args().skip(1);
    while let Some(x) = a.next() {
        match x.as_str() {
            "--datos" => datos = a.next().unwrap_or_default(),
            "--nombre" => nombre = a.next().unwrap_or_default(),
            "--rol" => rol = a.next().unwrap_or_default(),
            "--destino" => destinos.push(a.next().unwrap_or_default()),
            "--escritorio" => escritorio = true,
            // Solo deja el perfil listo (cuenta, semilla, oferta) y sale: para abrirlo con el escritorio real.
            "--preparar" => preparar = true,
            "--segundos" => segundos = a.next().and_then(|s| s.parse().ok()).unwrap_or(90),
            "--socks" => {
                let s = a.next().unwrap_or_default();
                if let Some((h, p)) = s.rsplit_once(':') {
                    socks = Some((h.into(), p.parse().unwrap_or(9050)));
                }
            }
            otro => eprintln!("ignoro {otro}"),
        }
    }
    let t0 = Instant::now();
    let log = |s: String| println!("[{:>5.1}s {}] {s}", t0.elapsed().as_secs_f32(), nombre);
    let app = KonstruadoApp::nuevo(
        datos,
        socks.as_ref().map(|s| s.0.clone()),
        socks.as_ref().map(|s| s.1),
        destinos,
        escritorio,
    )
    .expect("arranque");
    log(format!("PASO arranque {}", app.version()));
    if !app.perfil().tiene_cuenta {
        app.crear_cuenta(nombre.clone(), rol.clone()).expect("cuenta");
    }
    log(format!("PASO cuenta {:?}", app.perfil().rol));
    if !app.billetera().tiene_semilla {
        match app.crear_semilla() {
            Ok(addr) => log(format!("PASO semilla {}", &addr[..16])),
            Err(e) => log(format!("PASO semilla-error {e}")),
        }
    }
    let soy_m = rol == "mandante";
    if preparar {
        if soy_m {
            let o = app
                .publicar_oferta("Casa Escritorio".into(), "10000".into(), "2000".into(), vec![])
                .expect("oferta");
            log(format!("PASO oferta-preparada {}", o.id));
        }
        std::thread::sleep(Duration::from_millis(1500));
        app.guardar();
        log("PASO perfil-listo".into());
        return;
    }
    let mut publicada = false;
    let mut aceptada = false;
    let mut obra_id: Option<String> = None;
    let mut caja_vista = false;
    let mut encerrar = false;
    let mut fondear = false;
    let mut ultima_linea = String::new();
    let mut ultima_red = String::new();
    let mut ultima_billetera = String::new();
    let mut ultimo_estado = String::new();
    while t0.elapsed() < Duration::from_secs(segundos) {
        let red = app.red();
        if red.linea != ultima_red {
            log(format!("RED {} (vivas={} pares={})", red.linea, red.sesiones_vivas, red.pares));
            ultima_red = red.linea.clone();
        }
        let b = app.billetera();
        let bl = format!(
            "tip={:?} total={} libre={} visto='{}' aviso={:?}",
            b.tip, b.total, b.libre, b.visto, b.aviso
        );
        if bl != ultima_billetera {
            log(format!("BILLETERA {bl}"));
            ultima_billetera = bl;
        }
        let tab = app.tablero();
        if soy_m && !publicada && red.pares > 0 {
            match app.publicar_oferta("Casa Smoke".into(), "10000".into(), "2000".into(), vec![]) {
                Ok(o) => {
                    log(format!("PASO oferta-publicada {} ({} partidas)", o.id, o.n_partidas));
                    publicada = true;
                }
                Err(e) => log(format!("oferta-error {e}")),
            }
        }
        if !soy_m && !aceptada {
            if let Some(o) = tab.ofertas.first() {
                log(format!("PASO oferta-vista {} de {}", o.nombre, o.mandante));
                match app.aceptar_oferta(o.id.clone(), o.garantia_sugerida.to_string(), vec![]) {
                    Ok(id) => {
                        log(format!("PASO aceptada obra={id}"));
                        aceptada = true;
                    }
                    Err(e) => log(format!("aceptar-error {e}")),
                }
            }
        }
        if obra_id.is_none() {
            obra_id = tab.obras.first().map(|o| o.id.clone());
        }
        if let Some(id) = obra_id.clone() {
            if let Ok(v) = app.obra_vista(id.clone()) {
                let est = format!("{} sincronizando={}", v.estado_label, v.sincronizando);
                if est != ultimo_estado {
                    log(format!("OBRA {est}"));
                    ultimo_estado = est;
                }
                if let (Some(addr), false) = (&v.caja_direccion, caja_vista) {
                    caja_vista = true;
                    log(format!("PASO CAJA {addr}"));
                    let vk = app.view_key_caja(id.clone()).unwrap_or_default();
                    log(format!("PASO viewkey {}…", &vk[..vk.len().min(12)]));
                    match app.exportar_share(id.clone()) {
                        Ok(txt) => {
                            log(format!("PASO share-exportado {} bytes", txt.len()));
                            match app.restaurar_share(txt) {
                                Ok(m) => log(format!("PASO share-restaurado {m}")),
                                Err(e) => log(format!("share-restaurar-error {e}")),
                            }
                        }
                        Err(e) => log(format!("share-error {e}")),
                    }
                }
                if caja_vista && soy_m && !encerrar {
                    match app.proponer_encerrar(id.clone(), 0) {
                        Ok(()) => {
                            encerrar = true;
                            log("PASO encerrar-propuesto".into());
                        }
                        Err(e) => log(format!("encerrar-espera {e}")),
                    }
                }
                if let Ok(p) = app.partida_vista(id.clone(), 0) {
                    if !soy_m && !fondear && p.puede_confirmar_fondear {
                        match app.confirmar_y_fondear(id.clone(), 0) {
                            Ok(()) => {
                                fondear = true;
                                log("PASO confirmar-y-fondear pedido".into());
                            }
                            Err(e) => log(format!("PASO fondear-error {e}")),
                        }
                    }
                    let l = format!("{} · {:?} freno={}", p.label, p.linea, p.linea_freno);
                    if l != ultima_linea {
                        log(format!("PARTIDA {l}"));
                        ultima_linea = l;
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(1000));
    }
    app.guardar();
    log(format!(
        "FIN caja={caja_vista} encerrar={encerrar} fondear={fondear} red='{}'",
        app.red().linea
    ));
}
