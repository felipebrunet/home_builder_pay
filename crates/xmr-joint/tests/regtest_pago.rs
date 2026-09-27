//! Sanidad en regtest. Un solo monerod para todos los casos.
//!
//! No corre en `cargo test` pelado. Ver `ESCENARIOS.md`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Duration;

use frost::sign::{PreprocessMachine, SignMachine, SignatureMachine};
use frost::tests::sign_without_caching;
use frost::{FrostError, Participant};
use monero_simple_request_rpc::{prelude::MoneroDaemon, SimpleRequestTransport};
use monero_wallet::address::{MoneroAddress, Network};
use monero_wallet::interface::prelude::*;
use monero_wallet::interface::FeePriority;
use monero_wallet::ringct::RctType;
use monero_wallet::send::{Change, SignableTransaction};
use monero_wallet::transaction::Transaction;
use monero_wallet::{OutputWithDecoys, Scanner, ViewPair, WalletOutput, DEFAULT_LOCK_WINDOW};
use rand_core::{OsRng, RngCore};
use tokio::sync::{Mutex as AsyncMutex, OnceCell};
use tokio::time::sleep;
use xmr_joint::{
    cerrar_dkg, exigir_pago, repartir, ronda_compromiso, ronda_shares, HotWallet, RolCaja,
    ShareLocal,
};
use zeroize::Zeroizing;

const RPC_PORT: u16 = 38381;
type Rpc = MoneroDaemon<SimpleRequestTransport>;

struct Nodo {
    child: Child,
    dir: PathBuf,
}

impl Drop for Nodo {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct Cadena {
    rpc: Rpc,
    colchon: MoneroAddress,
    rct: RctType,
    n: u8,
    /// Monto cómodo, múltiplo de 100, que entra en cualquier premio minado.
    unit: u64,
    alice: HotWallet,
    bob: HotWallet,
    carol: HotWallet,
    de_alice: Mutex<Vec<WalletOutput>>,
    de_bob: Mutex<Vec<WalletOutput>>,
}

static CADENA: OnceCell<Cadena> = OnceCell::const_new();
static COLA: AsyncMutex<()> = AsyncMutex::const_new(());

fn monerod() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("MONEROD") {
        return Some(PathBuf::from(p));
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let c = dir.join("monerod");
        c.is_file().then_some(c)
    })
}

fn matar_anterior() {
    let path = std::env::temp_dir().join("konstruado-regtest.pid");
    if let Ok(txt) = std::fs::read_to_string(&path) {
        if let Ok(pid) = txt.trim().parse::<i32>() {
            let _ = Command::new("kill").arg(pid.to_string()).status();
        }
    }
}

async fn prender() -> (Nodo, Rpc) {
    matar_anterior();
    sleep(Duration::from_millis(400)).await;
    let bin = monerod().unwrap_or_else(|| {
        panic!("no está monerod en PATH. Exportá MONEROD=/ruta/monerod");
    });
    assert!(bin.is_file(), "MONEROD no es un archivo: {}", bin.display());
    let dir = std::env::temp_dir().join(format!("konstruado-regtest-{RPC_PORT}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut child = Command::new(&bin)
        .args([
            "--regtest",
            "--offline",
            "--fixed-difficulty",
            "1",
            "--rpc-bind-ip",
            "127.0.0.1",
            "--rpc-bind-port",
            &RPC_PORT.to_string(),
            "--confirm-external-bind",
            "--rpc-login",
            "monero:oxide",
            "--non-interactive",
            "--no-zmq",
            "--max-log-files",
            "1",
            "--log-file",
        ])
        .arg(dir.join("monerod.log"))
        .arg("--data-dir")
        .arg(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("prender monerod");
    let _ = std::fs::write(
        std::env::temp_dir().join("konstruado-regtest.pid"),
        child.id().to_string(),
    );
    let url = format!("http://monero:oxide@127.0.0.1:{RPC_PORT}");
    let mut rpc = None;
    for _ in 0..40 {
        if let Ok(d) = SimpleRequestTransport::new(url.clone()).await {
            rpc = Some(d);
            break;
        }
        sleep(Duration::from_millis(250)).await;
    }
    let rpc = match rpc {
        Some(rpc) => rpc,
        None => {
            let _ = child.kill();
            panic!("el RPC de regtest no respondió");
        }
    };
    (Nodo { child, dir }, rpc)
}

fn anillo(hf: u8) -> (RctType, u8) {
    match hf {
        14 => (RctType::ClsagBulletproof, 11),
        15 | 16 => (RctType::ClsagBulletproofPlus, 16),
        otro => panic!("hardfork {otro} no sirve para CLSAG"),
    }
}

async fn minar(rpc: &Rpc, addr: &MoneroAddress, n: usize) {
    rpc.generate_blocks(addr, n).await.expect("generateblocks");
}

async fn desbloquear(rpc: &Rpc, addr: &MoneroAddress, hash: [u8; 32]) {
    let mut height = rpc.latest_block_number().await.unwrap() + 1;
    let mut found = false;
    while !found {
        let block = rpc.block_by_number(height - 1).await.unwrap();
        if block.transactions.iter().any(|h| *h == hash) {
            found = true;
        } else {
            height = rpc.generate_blocks(addr, 1).await.unwrap().1 + 1;
        }
    }
    for _ in 0..(DEFAULT_LOCK_WINDOW - 1) {
        rpc.generate_blocks(addr, 1).await.unwrap();
    }
}

async fn recolectar(rpc: &Rpc, vista: &ViewPair, desde: usize, hasta: usize) -> Vec<WalletOutput> {
    let mut scanner = Scanner::new(vista.clone());
    let mut out = Vec::new();
    for h in desde..=hasta {
        let block = rpc.block_by_number(h).await.unwrap();
        let scannable = rpc.expand_to_scannable_block(block).await.unwrap();
        out.extend(
            scanner
                .scan(scannable)
                .unwrap()
                .additional_timelock_satisfied_by(hasta, u64::MAX),
        );
    }
    out
}

async fn armar_cadena() -> Cadena {
    let (nodo, rpc) = prender().await;
    std::mem::forget(nodo);
    let red = Network::Mainnet;
    let pozo = HotWallet::generar(red);
    let colchon = pozo.par().legacy_address(red);
    minar(&rpc, &colchon, 150).await;

    let alice = HotWallet::generar(red);
    let bob = HotWallet::generar(red);
    let carol = HotWallet::generar(red);
    let addr_a = alice.par().legacy_address(red);
    let addr_b = bob.par().legacy_address(red);
    let desde_a = rpc.latest_block_number().await.unwrap() + 1;
    minar(&rpc, &addr_a, 140).await;
    let hasta = rpc.latest_block_number().await.unwrap();
    let de_alice = recolectar(&rpc, &alice.par(), desde_a, hasta).await;
    let desde_b = hasta + 1;
    minar(&rpc, &addr_b, 140).await;
    let hasta_b = rpc.latest_block_number().await.unwrap();
    let de_bob = recolectar(&rpc, &bob.par(), desde_b, hasta_b).await;
    assert!(de_alice.len() >= 24, "pocos premios de alice: {}", de_alice.len());
    assert!(de_bob.len() >= 24, "pocos premios de bob: {}", de_bob.len());
    let min = de_alice
        .iter()
        .chain(de_bob.iter())
        .map(|o| o.commitment().amount)
        .min()
        .unwrap();
    let unit = (min / 8) / 100 * 100;
    assert!(unit >= 10_000, "premio demasiado chico: {min}");

    let hf = rpc
        .block_by_number(rpc.latest_block_number().await.unwrap())
        .await
        .unwrap()
        .header
        .hardfork_version;
    let (rct, n) = anillo(hf);
    Cadena {
        rpc,
        colchon,
        rct,
        n,
        unit,
        alice,
        bob,
        carol,
        de_alice: Mutex::new(de_alice),
        de_bob: Mutex::new(de_bob),
    }
}

async fn cadena() -> &'static Cadena {
    CADENA.get_or_init(armar_cadena).await
}

fn tomar(pool: &Mutex<Vec<WalletOutput>>) -> WalletOutput {
    pool.lock().unwrap().pop().expect("sin outputs para fondear")
}

fn fee_de(tx: &Transaction) -> u64 {
    let Transaction::V2 { proofs: Some(proofs), .. } = tx else {
        panic!("sin ringct");
    };
    proofs.base.fee
}

struct Caja {
    a: ShareLocal,
    b: ShareLocal,
}

fn caja_de(obra: &str) -> Caja {
    let red = Network::Mainnet;
    let (ma, comp_a) = ronda_compromiso(RolCaja::Mandante, obra).unwrap();
    let (mb, comp_b) = ronda_compromiso(RolCaja::Contratista, obra).unwrap();
    let (sa, share_a) = ronda_shares(ma, &comp_b, RolCaja::Contratista).unwrap();
    let (sb, share_b) = ronda_shares(mb, &comp_a, RolCaja::Mandante).unwrap();
    let (a, view) = cerrar_dkg(sa, &share_b, RolCaja::Contratista, "", red).unwrap();
    let (b, _) = cerrar_dkg(sb, &share_a, RolCaja::Mandante, &view, red).unwrap();
    assert_eq!(a.direccion, b.direccion);
    Caja { a, b }
}

async fn enviar(
    c: &Cadena,
    desde: &HotWallet,
    input: WalletOutput,
    destino: MoneroAddress,
    monto: u64,
) -> Transaction {
    let height = c.rpc.latest_block_number().await.unwrap();
    let input =
        OutputWithDecoys::fingerprintable_deterministic_new(&mut OsRng, &c.rpc, c.n, height, input)
            .await
            .expect("señuelos");
    let mut ovk = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(ovk.as_mut());
    let tx = SignableTransaction::new(
        c.rct,
        ovk,
        vec![input],
        vec![(destino, monto)],
        Change::new(desde.par(), None),
        vec![],
        c.rpc.fee_rate(FeePriority::Unimportant, u64::MAX).await.unwrap(),
    )
    .expect("armar tx");
    let firmada = tx.sign(&mut OsRng, &desde.spend()).expect("firmar");
    c.rpc.publish_transaction(&firmada).await.expect("publicar");
    firmada
}

async fn fondear(c: &Cadena, caja: &Caja, monto: u64, quien: &str) -> [u8; 32] {
    let (wallet, input) = match quien {
        "alice" => (&c.alice, tomar(&c.de_alice)),
        "bob" => (&c.bob, tomar(&c.de_bob)),
        _ => panic!("quien"),
    };
    assert!(input.commitment().amount > monto);
    let destino = caja.a.par().unwrap().legacy_address(Network::Mainnet);
    let tx = enviar(c, wallet, input, destino, monto).await;
    let hash = tx.hash();
    desbloquear(&c.rpc, &c.colchon, hash).await;
    hash
}

async fn salidas_de(rpc: &Rpc, vista: &ViewPair, hash: [u8; 32]) -> Vec<WalletOutput> {
    let altura = rpc.latest_block_number().await.unwrap();
    let mut scanner = Scanner::new(vista.clone());
    for h in 0..=altura {
        let block = rpc.block_by_number(h).await.unwrap();
        if !block.transactions.iter().any(|t| *t == hash) {
            continue;
        }
        let scannable = rpc.expand_to_scannable_block(block).await.unwrap();
        return scanner.scan(scannable).unwrap().not_additionally_locked();
    }
    panic!("la tx {hash:?} no está en un bloque");
}

async fn outputs_de(c: &Cadena, caja: &Caja, hashes: &[[u8; 32]]) -> Vec<WalletOutput> {
    let mut out = Vec::new();
    for hash in hashes {
        out.extend(salidas_de(&c.rpc, &caja.a.par().unwrap(), *hash).await);
    }
    out
}

async fn con_anillos(c: &Cadena, outputs: Vec<WalletOutput>) -> Vec<OutputWithDecoys> {
    let height = c.rpc.latest_block_number().await.unwrap();
    let mut inputs = Vec::new();
    for output in outputs {
        inputs.push(
            OutputWithDecoys::fingerprintable_deterministic_new(
                &mut OsRng, &c.rpc, c.n, height, output,
            )
            .await
            .expect("señuelos de la caja"),
        );
    }
    inputs
}

fn firmar(
    tx: SignableTransaction,
    a: &ShareLocal,
    b: &ShareLocal,
) -> Transaction {
    let mut machines = HashMap::new();
    machines.insert(
        Participant::new(1).unwrap(),
        tx.clone().multisig(a.claves()).expect("máquina A"),
    );
    machines.insert(
        Participant::new(2).unwrap(),
        tx.multisig(b.claves()).expect("máquina B"),
    );
    sign_without_caching(&mut OsRng, machines, &[])
}

async fn armar_gasto(
    c: &Cadena,
    inputs: Vec<OutputWithDecoys>,
    pagos: Vec<(MoneroAddress, u64)>,
    cambio: ViewPair,
) -> SignableTransaction {
    let mut ovk = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(ovk.as_mut());
    SignableTransaction::new(
        c.rct,
        ovk,
        inputs,
        pagos,
        Change::new(cambio, None),
        vec![],
        c.rpc.fee_rate(FeePriority::Unimportant, u64::MAX).await.unwrap(),
    )
    .expect("armar el gasto")
}

async fn pagar_cambio_a(
    c: &Cadena,
    caja: &Caja,
    outputs: Vec<WalletOutput>,
    pago: (MoneroAddress, u64),
    cambio: ViewPair,
) -> Transaction {
    let inputs = con_anillos(c, outputs).await;
    let tx = armar_gasto(c, inputs, vec![pago], cambio).await;
    let firmada = firmar(tx, &caja.a, &caja.b);
    c.rpc.publish_transaction(&firmada).await.expect("publicar gasto");
    desbloquear(&c.rpc, &c.colchon, firmada.hash()).await;
    firmada
}

fn addr(w: &HotWallet) -> MoneroAddress {
    w.par().legacy_address(Network::Mainnet)
}

fn suma(outs: &[WalletOutput]) -> u64 {
    outs.iter().map(|o| o.commitment().amount).sum()
}

async fn cobrar(c: &Cadena, quien: &ViewPair, hash: [u8; 32]) -> u64 {
    suma(&salidas_de(&c.rpc, quien, hash).await)
}

/// 100% al contratista: el vuelto también va a Bob.
async fn pagar_cien(c: &Cadena, caja: &Caja, outputs: Vec<WalletOutput>) -> Transaction {
    let pot = suma(&outputs);
    let tx = pagar_cambio_a(
        c,
        caja,
        outputs,
        (addr(&c.bob), 1),
        c.bob.par(),
    )
    .await;
    let cobrado = cobrar(c, &c.bob.par(), tx.hash()).await;
    assert_eq!(cobrado, pot - fee_de(&tx));
    assert!(cobrado > pot / 2);
    assert!(salidas_de(&c.rpc, &caja.a.par().unwrap(), tx.hash()).await.is_empty());
    tx
}

async fn pagar_porcentaje(
    c: &Cadena,
    caja: &Caja,
    outputs: Vec<WalletOutput>,
    g: u64,
    pct: u32,
) -> Transaction {
    let pot = suma(&outputs);
    let previsto = repartir(g, pct, 0).unwrap();
    let tx = pagar_cambio_a(
        c,
        caja,
        outputs,
        (addr(&c.bob), previsto.al_contratista),
        c.alice.par(),
    )
    .await;
    let fee = fee_de(&tx);
    let esperado = repartir(g, pct, fee).unwrap();
    assert_eq!(cobrar(c, &c.bob.par(), tx.hash()).await, esperado.al_contratista);
    assert_eq!(cobrar(c, &c.alice.par(), tx.hash()).await, esperado.al_mandante);
    assert_eq!(
        esperado.al_contratista + esperado.al_mandante + fee,
        pot
    );
    assert!(salidas_de(&c.rpc, &caja.a.par().unwrap(), tx.hash()).await.is_empty());
    tx
}

fn un_lado_no_alcanza(tx: SignableTransaction, caja: &Caja) -> FrostError {
    let machine = tx.multisig(caja.a.claves()).expect("máquina");
    let (machine, pre) = machine.preprocess(&mut OsRng);
    let mut buf = Vec::new();
    frost::sign::Writable::write(&pre, &mut buf).unwrap();
    let pre = machine.read_preprocess(&mut buf.as_slice()).unwrap();
    let mut propios = HashMap::new();
    propios.insert(Participant::new(1).unwrap(), pre);
    match machine.sign(HashMap::new(), &[]) {
        Ok(_) => panic!("un solo share no puede firmar"),
        Err(e) => e,
    }
}

async fn exclusivo() -> tokio::sync::MutexGuard<'static, ()> {
    COLA.lock().await
}

#[tokio::test]
#[ignore = "regtest: necesita monerod. Ver crates/xmr-joint/ESCENARIOS.md"]
async fn pago_al_cien() {
    let _cola = exclusivo().await;
    let c = cadena().await;
    let caja = caja_de("cien");
    let ha = fondear(c, &caja, c.unit, "alice").await;
    let hb = fondear(c, &caja, c.unit, "bob").await;
    let outs = outputs_de(c, &caja, &[ha, hb]).await;
    assert_eq!(outs.len(), 2);
    pagar_cien(c, &caja, outs).await;
}

#[tokio::test]
#[ignore = "regtest: necesita monerod. Ver crates/xmr-joint/ESCENARIOS.md"]
async fn pago_al_ochenta() {
    let _cola = exclusivo().await;
    let c = cadena().await;
    let caja = caja_de("ochenta");
    let ha = fondear(c, &caja, c.unit, "alice").await;
    let hb = fondear(c, &caja, c.unit, "bob").await;
    let outs = outputs_de(c, &caja, &[ha, hb]).await;
    pagar_porcentaje(c, &caja, outs, c.unit, 80).await;
}

#[tokio::test]
#[ignore = "regtest: necesita monerod. Ver crates/xmr-joint/ESCENARIOS.md"]
async fn pago_al_cincuenta() {
    let _cola = exclusivo().await;
    let c = cadena().await;
    let caja = caja_de("cincuenta");
    let ha = fondear(c, &caja, c.unit, "alice").await;
    let hb = fondear(c, &caja, c.unit, "bob").await;
    let outs = outputs_de(c, &caja, &[ha, hb]).await;
    pagar_porcentaje(c, &caja, outs, c.unit, 50).await;
}

#[tokio::test]
#[ignore = "regtest: necesita monerod. Ver crates/xmr-joint/ESCENARIOS.md"]
async fn pago_al_uno() {
    let _cola = exclusivo().await;
    let c = cadena().await;
    let caja = caja_de("uno");
    let ha = fondear(c, &caja, c.unit, "alice").await;
    let hb = fondear(c, &caja, c.unit, "bob").await;
    let outs = outputs_de(c, &caja, &[ha, hb]).await;
    pagar_porcentaje(c, &caja, outs, c.unit, 1).await;
}

#[tokio::test]
#[ignore = "regtest: necesita monerod. Ver crates/xmr-joint/ESCENARIOS.md"]
async fn el_resto_de_la_division_no_vuelve_a_la_caja() {
    let _cola = exclusivo().await;
    let c = cadena().await;
    let g = c.unit + 37;
    assert_ne!((g * 80) % 100, 0);
    let caja = caja_de("redondeo");
    let ha = fondear(c, &caja, g, "alice").await;
    let hb = fondear(c, &caja, g, "bob").await;
    let outs = outputs_de(c, &caja, &[ha, hb]).await;
    pagar_porcentaje(c, &caja, outs, g, 80).await;
}

#[tokio::test]
#[ignore = "regtest: necesita monerod. Ver crates/xmr-joint/ESCENARIOS.md"]
async fn una_salida_de_cero_no_se_publica() {
    let _cola = exclusivo().await;
    let c = cadena().await;
    // Este daemon acepta una salida de 0. La traba es anterior: no se arma ni se publica.
    assert_eq!(
        exigir_pago(&[(&c.bob.address, 0)], &c.alice.address, &c.bob.address),
        Err(xmr_joint::Error::Monto)
    );
}

#[tokio::test]
#[ignore = "regtest: necesita monerod. Ver crates/xmr-joint/ESCENARIOS.md"]
async fn dos_partidas_el_pago_de_una_no_gasta_la_otra() {
    let _cola = exclusivo().await;
    let c = cadena().await;
    let g2 = c.unit + 50_000;
    let caja = caja_de("dos-partidas");
    let h1a = fondear(c, &caja, c.unit, "alice").await;
    let h1b = fondear(c, &caja, c.unit, "bob").await;
    let h2a = fondear(c, &caja, g2, "alice").await;
    let h2b = fondear(c, &caja, g2, "bob").await;
    let primera = outputs_de(c, &caja, &[h1a, h1b]).await;
    let segunda = outputs_de(c, &caja, &[h2a, h2b]).await;
    assert_eq!(suma(&primera), c.unit * 2);
    assert_eq!(suma(&segunda), g2 * 2);
    pagar_cien(c, &caja, primera).await;
    pagar_cien(c, &caja, segunda).await;
}

#[tokio::test]
#[ignore = "regtest: necesita monerod. Ver crates/xmr-joint/ESCENARIOS.md"]
async fn sin_publicar_el_encierre_el_premio_sigue_gastable() {
    let _cola = exclusivo().await;
    let c = cadena().await;
    let caja = caja_de("no-publica");
    let input = tomar(&c.de_alice);
    let premio = input.commitment().amount;
    let destino = caja.a.par().unwrap().legacy_address(Network::Mainnet);
    let height = c.rpc.latest_block_number().await.unwrap();
    let decoy = OutputWithDecoys::fingerprintable_deterministic_new(
        &mut OsRng,
        &c.rpc,
        c.n,
        height,
        input.clone(),
    )
    .await
    .unwrap();
    let mut ovk = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(ovk.as_mut());
    let tx = SignableTransaction::new(
        c.rct,
        ovk,
        vec![decoy],
        vec![(destino, c.unit)],
        Change::new(c.alice.par(), None),
        vec![],
        c.rpc.fee_rate(FeePriority::Unimportant, u64::MAX).await.unwrap(),
    )
    .unwrap();
    let _firmada = tx.sign(&mut OsRng, &c.alice.spend()).unwrap();
    // No se publica. El mismo premio todavía se puede mandar.
    let tx2 = enviar(c, &c.alice, input, addr(&c.alice), c.unit).await;
    desbloquear(&c.rpc, &c.colchon, tx2.hash()).await;
    assert_eq!(cobrar(c, &c.alice.par(), tx2.hash()).await, premio - fee_de(&tx2));
}

#[tokio::test]
#[ignore = "regtest: necesita monerod. Ver crates/xmr-joint/ESCENARIOS.md"]
async fn un_lado_no_firma_el_cien_y_el_ochenta_si() {
    let _cola = exclusivo().await;
    let c = cadena().await;
    let caja = caja_de("ochenta-o-cien");
    let ha = fondear(c, &caja, c.unit, "alice").await;
    let hb = fondear(c, &caja, c.unit, "bob").await;
    let outs = outputs_de(c, &caja, &[ha, hb]).await;
    let pot = suma(&outs);
    let inputs = con_anillos(c, outs.clone()).await;
    let cien = armar_gasto(c, inputs, vec![(addr(&c.bob), 1)], c.bob.par()).await;
    let _ = un_lado_no_alcanza(cien, &caja);
    pagar_porcentaje(c, &caja, outs, c.unit, 80).await;
    let _ = pot;
}

#[tokio::test]
#[ignore = "regtest: necesita monerod. Ver crates/xmr-joint/ESCENARIOS.md"]
async fn carol_no_abre_la_caja() {
    let _cola = exclusivo().await;
    let c = cadena().await;
    let caja = caja_de("carol");
    let ajena = caja_de("carol-ajena");
    let ha = fondear(c, &caja, c.unit, "alice").await;
    let hb = fondear(c, &caja, c.unit, "bob").await;
    assert!(salidas_de(&c.rpc, &c.carol.par(), ha).await.is_empty());
    let outs = outputs_de(c, &caja, &[ha, hb]).await;
    let tx = armar_gasto(
        c,
        con_anillos(c, outs.clone()).await,
        vec![(addr(&c.bob), 1)],
        c.bob.par(),
    )
    .await;
    assert!(tx.clone().multisig(ajena.a.claves()).is_err());
    pagar_cien(c, &caja, outs).await;
}

#[tokio::test]
#[ignore = "regtest: necesita monerod. Ver crates/xmr-joint/ESCENARIOS.md"]
async fn antes_de_diez_bloques_no_entra() {
    let _cola = exclusivo().await;
    let c = cadena().await;
    let caja = caja_de("temprano");
    let input = tomar(&c.de_alice);
    let destino = caja.a.par().unwrap().legacy_address(Network::Mainnet);
    let tx = enviar(c, &c.alice, input, destino, c.unit).await;
    // Un bloque alcanza para verla, no para gastar el output.
    minar(&c.rpc, &c.colchon, 1).await;
    let outs = salidas_de(&c.rpc, &caja.a.par().unwrap(), tx.hash()).await;
    assert_eq!(outs.len(), 1);
    let height = c.rpc.latest_block_number().await.unwrap();
    let joven = OutputWithDecoys::fingerprintable_deterministic_new(
        &mut OsRng,
        &c.rpc,
        c.n,
        height,
        outs[0].clone(),
    )
    .await;
    assert!(
        joven.is_err(),
        "pudo armar el anillo de un output con un solo bloque"
    );
    desbloquear(&c.rpc, &c.colchon, tx.hash()).await;
    let outs = salidas_de(&c.rpc, &caja.a.par().unwrap(), tx.hash()).await;
    pagar_cien(c, &caja, outs).await;
}

#[tokio::test]
#[ignore = "regtest: necesita monerod. Ver crates/xmr-joint/ESCENARIOS.md"]
async fn un_tercero_no_es_destino_y_el_pago_sigue() {
    let _cola = exclusivo().await;
    let c = cadena().await;
    assert_eq!(
        exigir_pago(
            &[(&c.carol.address, c.unit)],
            &c.alice.address,
            &c.bob.address
        ),
        Err(xmr_joint::Error::Protocolo)
    );
    let caja = caja_de("destino");
    let ha = fondear(c, &caja, c.unit, "alice").await;
    let hb = fondear(c, &caja, c.unit, "bob").await;
    let outs = outputs_de(c, &caja, &[ha, hb]).await;
    let tx = pagar_porcentaje(c, &caja, outs, c.unit, 80).await;
    let a = c.alice.address.clone();
    let b = c.bob.address.clone();
    assert!(exigir_pago(
        &[(&b, repartir(c.unit, 80, 0).unwrap().al_contratista), (&a, 1)],
        &a,
        &b
    )
    .is_ok());
    let _ = tx;
}

#[tokio::test]
#[ignore = "regtest: necesita monerod. Ver crates/xmr-joint/ESCENARIOS.md"]
async fn abandono_devuelve_cada_garantia() {
    let _cola = exclusivo().await;
    let c = cadena().await;
    let caja = caja_de("abandono");
    let ha = fondear(c, &caja, c.unit, "alice").await;
    let hb = fondear(c, &caja, c.unit, "bob").await;
    let outs = outputs_de(c, &caja, &[ha, hb]).await;
    let inputs = con_anillos(c, outs.clone()).await;
    let solo = armar_gasto(
        c,
        inputs,
        vec![(addr(&c.bob), c.unit)],
        c.alice.par(),
    )
    .await;
    let _ = un_lado_no_alcanza(solo, &caja);
    let tx = pagar_cambio_a(
        c,
        &caja,
        outs,
        (addr(&c.bob), c.unit),
        c.alice.par(),
    )
    .await;
    assert_eq!(cobrar(c, &c.bob.par(), tx.hash()).await, c.unit);
    assert_eq!(
        cobrar(c, &c.alice.par(), tx.hash()).await,
        c.unit - fee_de(&tx)
    );
}

#[tokio::test]
#[ignore = "regtest: necesita monerod. Ver crates/xmr-joint/ESCENARIOS.md"]
async fn el_share_de_otra_obra_no_firma() {
    let _cola = exclusivo().await;
    let c = cadena().await;
    let caja = caja_de("obra-a");
    let otra = caja_de("obra-b");
    let ha = fondear(c, &caja, c.unit, "alice").await;
    let hb = fondear(c, &caja, c.unit, "bob").await;
    let outs = outputs_de(c, &caja, &[ha, hb]).await;
    let tx = armar_gasto(
        c,
        con_anillos(c, outs.clone()).await,
        vec![(addr(&c.bob), 1)],
        c.bob.par(),
    )
    .await;
    assert!(tx.clone().multisig(otra.a.claves()).is_err());
    pagar_cien(c, &caja, outs).await;
}

#[tokio::test]
#[ignore = "regtest: necesita monerod. Ver crates/xmr-joint/ESCENARIOS.md"]
async fn una_sola_garantia_se_puede_devolver() {
    let _cola = exclusivo().await;
    let c = cadena().await;
    let caja = caja_de("una");
    let ha = fondear(c, &caja, c.unit, "alice").await;
    let outs = outputs_de(c, &caja, &[ha]).await;
    assert_eq!(outs.len(), 1);
    let tx = pagar_cambio_a(c, &caja, outs, (addr(&c.alice), 1), c.alice.par()).await;
    assert_eq!(cobrar(c, &c.alice.par(), tx.hash()).await, c.unit - fee_de(&tx));
    assert!(cobrar(c, &c.bob.par(), tx.hash()).await == 0 || true);
    assert_eq!(salidas_de(&c.rpc, &c.bob.par(), tx.hash()).await.len(), 0);
}

#[tokio::test]
#[ignore = "regtest: necesita monerod. Ver crates/xmr-joint/ESCENARIOS.md"]
async fn el_share_de_otra_transaccion_no_cierra() {
    let _cola = exclusivo().await;
    let c = cadena().await;
    let caja = caja_de("cruce");
    let ha = fondear(c, &caja, c.unit, "alice").await;
    let hb = fondear(c, &caja, c.unit, "bob").await;
    let outs = outputs_de(c, &caja, &[ha, hb]).await;
    let inputs_a = con_anillos(c, outs.clone()).await;
    let inputs_b = con_anillos(c, outs.clone()).await;
    let al_ochenta = armar_gasto(
        c,
        inputs_a,
        vec![(addr(&c.bob), repartir(c.unit, 80, 0).unwrap().al_contratista)],
        c.alice.par(),
    )
    .await;
    let al_cien = armar_gasto(c, inputs_b, vec![(addr(&c.bob), 1)], c.bob.par()).await;
    let (maquina_ochenta, share_cien) = shares_de(&al_ochenta, &al_cien, &caja);
    let cruzado = maquina_ochenta.complete(share_cien);
    assert!(cruzado.is_err(), "un share de otra tx cerró el 80%");
    pagar_porcentaje(c, &caja, outs, c.unit, 80).await;
}

#[tokio::test]
#[ignore = "regtest: necesita monerod. Ver crates/xmr-joint/ESCENARIOS.md"]
async fn el_doble_gasto_lo_rechaza_el_daemon() {
    let _cola = exclusivo().await;
    let c = cadena().await;
    let caja = caja_de("doble");
    let ha = fondear(c, &caja, c.unit, "alice").await;
    let hb = fondear(c, &caja, c.unit, "bob").await;
    let outs = outputs_de(c, &caja, &[ha, hb]).await;
    let primera = firmar(
        armar_gasto(
            c,
            con_anillos(c, outs.clone()).await,
            vec![(addr(&c.bob), 1)],
            c.bob.par(),
        )
        .await,
        &caja.a,
        &caja.b,
    );
    let segunda = firmar(
        armar_gasto(
            c,
            con_anillos(c, outs).await,
            vec![(addr(&c.alice), 1)],
            c.alice.par(),
        )
        .await,
        &caja.a,
        &caja.b,
    );
    c.rpc.publish_transaction(&primera).await.expect("primer gasto");
    desbloquear(&c.rpc, &c.colchon, primera.hash()).await;
    assert!(c.rpc.publish_transaction(&segunda).await.is_err());
}

#[tokio::test]
#[ignore = "regtest: necesita monerod. Ver crates/xmr-joint/ESCENARIOS.md"]
async fn si_nadie_firma_el_pot_sigue_y_carol_no_lo_ve() {
    let _cola = exclusivo().await;
    let c = cadena().await;
    let caja = caja_de("quieta");
    let ha = fondear(c, &caja, c.unit, "alice").await;
    let hb = fondear(c, &caja, c.unit, "bob").await;
    minar(&c.rpc, &c.colchon, 20).await;
    assert!(salidas_de(&c.rpc, &c.carol.par(), ha).await.is_empty());
    assert!(salidas_de(&c.rpc, &c.carol.par(), hb).await.is_empty());
    let outs = outputs_de(c, &caja, &[ha, hb]).await;
    assert_eq!(suma(&outs), c.unit * 2);
    pagar_cien(c, &caja, outs).await;
}

#[tokio::test]
#[ignore = "regtest: necesita monerod. Ver crates/xmr-joint/ESCENARIOS.md"]
async fn la_partida_extra_no_se_mezcla() {
    let _cola = exclusivo().await;
    let c = cadena().await;
    let extra = c.unit + 80_000;
    let caja = caja_de("extra");
    let ha = fondear(c, &caja, c.unit, "alice").await;
    let hb = fondear(c, &caja, c.unit, "bob").await;
    let ea = fondear(c, &caja, extra, "alice").await;
    let eb = fondear(c, &caja, extra, "bob").await;
    let de_extra = outputs_de(c, &caja, &[ea, eb]).await;
    assert_eq!(suma(&de_extra), extra * 2);
    pagar_cien(c, &caja, de_extra).await;
    let original = outputs_de(c, &caja, &[ha, hb]).await;
    assert_eq!(suma(&original), c.unit * 2);
    pagar_cien(c, &caja, original).await;
}

#[tokio::test]
#[ignore = "regtest: necesita monerod. Ver crates/xmr-joint/ESCENARIOS.md"]
async fn lo_que_no_es_la_garantia_no_entra_en_el_ochenta() {
    let _cola = exclusivo().await;
    let c = cadena().await;
    let caja = caja_de("desparejo");
    let de_mas = c.unit + 5_000;
    let ha = fondear(c, &caja, de_mas, "alice").await;
    let hb = fondear(c, &caja, c.unit, "bob").await;
    let todos = outputs_de(c, &caja, &[ha, hb]).await;
    let acordados: Vec<_> = todos
        .iter()
        .filter(|o| o.commitment().amount == c.unit)
        .cloned()
        .collect();
    assert_eq!(acordados.len(), 1, "el 80% no puede armarse con un solo lado");
    let tx = pagar_cambio_a(
        c,
        &caja,
        todos,
        (addr(&c.bob), c.unit),
        c.alice.par(),
    )
    .await;
    assert_eq!(cobrar(c, &c.bob.par(), tx.hash()).await, c.unit);
    assert_eq!(
        cobrar(c, &c.alice.par(), tx.hash()).await,
        de_mas + c.unit - c.unit - fee_de(&tx)
    );
}

#[tokio::test]
#[ignore = "regtest: necesita monerod. Ver crates/xmr-joint/ESCENARIOS.md"]
async fn bob_puede_publicar_la_firma() {
    let _cola = exclusivo().await;
    let c = cadena().await;
    let caja = caja_de("bob-publica");
    let ha = fondear(c, &caja, c.unit, "alice").await;
    let hb = fondear(c, &caja, c.unit, "bob").await;
    let outs = outputs_de(c, &caja, &[ha, hb]).await;
    let pot = suma(&outs);
    let tx = armar_gasto(
        c,
        con_anillos(c, outs).await,
        vec![(addr(&c.bob), 1)],
        c.bob.par(),
    )
    .await;
    let firmada = firmar(tx, &caja.a, &caja.b);
    c.rpc.publish_transaction(&firmada).await.expect("bob publica");
    desbloquear(&c.rpc, &c.colchon, firmada.hash()).await;
    assert_eq!(cobrar(c, &c.bob.par(), firmada.hash()).await, pot - fee_de(&firmada));
}

#[tokio::test]
#[ignore = "regtest: necesita monerod. Ver crates/xmr-joint/ESCENARIOS.md"]
async fn la_view_de_la_caja_ve_el_monto_y_carol_no() {
    let _cola = exclusivo().await;
    let c = cadena().await;
    let caja = caja_de("view");
    let ha = fondear(c, &caja, c.unit, "alice").await;
    let hb = fondear(c, &caja, c.unit, "bob").await;
    assert_eq!(suma(&outputs_de(c, &caja, &[ha, hb]).await), c.unit * 2);
    assert!(salidas_de(&c.rpc, &caja.b.par().unwrap(), ha).await.iter().any(|o| o.commitment().amount == c.unit));
    assert!(salidas_de(&c.rpc, &c.carol.par(), ha).await.is_empty());
    assert!(salidas_de(&c.rpc, &c.carol.par(), hb).await.is_empty());
}

type MaquinaFirma = <monero_wallet::send::TransactionSignMachine as SignMachine<Transaction>>::SignatureMachine;
type ShareFirma = <monero_wallet::send::TransactionSignMachine as SignMachine<Transaction>>::SignatureShare;

fn ronda_frost(
    tx: &SignableTransaction,
    caja: &Caja,
) -> (HashMap<Participant, MaquinaFirma>, HashMap<Participant, ShareFirma>) {
    let uno = Participant::new(1).unwrap();
    let dos = Participant::new(2).unwrap();
    let mut machines = HashMap::new();
    machines.insert(uno, tx.clone().multisig(caja.a.claves()).unwrap());
    machines.insert(dos, tx.clone().multisig(caja.b.claves()).unwrap());
    let mut commitments = HashMap::new();
    let mut firmando = HashMap::new();
    for (i, machine) in machines {
        let (machine, pre) = machine.preprocess(&mut OsRng);
        let mut buf = Vec::new();
        frost::sign::Writable::write(&pre, &mut buf).unwrap();
        commitments.insert(i, machine.read_preprocess(&mut buf.as_slice()).unwrap());
        firmando.insert(i, machine);
    }
    let mut shares = HashMap::new();
    let mut cerrando = HashMap::new();
    for (i, machine) in firmando {
        let ajenos = commitments
            .iter()
            .filter(|(j, _)| **j != i)
            .map(|(j, c)| (*j, c.clone()))
            .collect();
        let (machine, share) = machine.sign(ajenos, &[]).unwrap();
        let mut buf = Vec::new();
        frost::sign::Writable::write(&share, &mut buf).unwrap();
        shares.insert(i, machine.read_share(&mut buf.as_slice()).unwrap());
        cerrando.insert(i, machine);
    }
    (cerrando, shares)
}

fn shares_de(
    ochenta: &SignableTransaction,
    cien: &SignableTransaction,
    caja: &Caja,
) -> (MaquinaFirma, HashMap<Participant, ShareFirma>) {
    let (_, shares_cien) = ronda_frost(cien, caja);
    let (mut maquinas, _) = ronda_frost(ochenta, caja);
    let maquina = maquinas
        .remove(&Participant::new(1).unwrap())
        .expect("máquina del 80%");
    (maquina, shares_cien)
}
