//! Sala/relay sin ventana. Hospeda el onion de encuentro (con `tor` en el PATH)
//! o, con `--tcp PUERTO`, solo escucha por TCP (pruebas locales, emulador,
//! `adb reverse`, LAN con KONSTRUADO_ESCUCHAR=0.0.0.0).
//!
//! Solo puede haber UN anfitrión de la sala por vez: la clave del onion está
//! horneada. Si un mandante de escritorio ya hospeda, no corras esto con tor.

use std::time::Duration;

use konstruado_net::{Nodo, PUERTO_LOCAL};

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut tcp: Option<u16> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--tcp" => {
                let p = args
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(PUERTO_LOCAL);
                tcp = Some(p);
                i += 1;
            }
            "-h" | "--help" => {
                println!("uso: konstruado-sala [--tcp PUERTO]\n  sin --tcp: lanza tor y hospeda el onion de encuentro\n  --tcp: solo TCP local (KONSTRUADO_ESCUCHAR=0.0.0.0 para LAN)");
                return Ok(());
            }
            otro => eprintln!("argumento ignorado: {otro}"),
        }
        i += 1;
    }
    let nodo = match tcp {
        Some(p) => {
            let n = Nodo::arrancar_en(p).await?;
            println!("sala TCP en puerto {p} (sin tor)");
            n
        }
        None => {
            let n = Nodo::arrancar().await?;
            n.entrar_en_sala(true);
            println!("sala: lanzando tor y hospedando el onion de encuentro");
            n
        }
    };
    loop {
        let t = nodo.estado_tor();
        println!(
            "pares={} vivos={} ofertas={} obras={} presentes={} tor={:?}",
            nodo.n_peers(),
            nodo.n_vivos(),
            nodo.tablero().len(),
            nodo.obras().len(),
            nodo.presentes().len(),
            t
        );
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}
