//! Prueba en una sola máquina, con secretos nuevos.
//!
//! Las dos partes viven en este proceso. En la ventana, los mismos bytes van a
//! viajar por el canal de la obra. Sin `--broadcast` no se manda nada al daemon.

use std::path::{Path, PathBuf};

use monero_wallet::address::MoneroAddress;
use rand_core::{OsRng, RngCore};

use xmr_joint::backup::{self, SeedBackup};
use xmr_joint::chain;
use xmr_joint::dkg::{DkgParty, JointAccount, Party};
use xmr_joint::fund;
use xmr_joint::network::{Net, FEE_CUSHION, STAGENET_DAEMON};
use xmr_joint::spend::{self, SpendSession};
use xmr_joint::wallet::SingleWallet;

const LOOKBACK: usize = 30;

struct Flags {
    dir: Option<PathBuf>,
    obra: Option<String>,
    daemon: Option<String>,
    capital: Option<u64>,
    pct: Option<u32>,
    partida: Option<u32>,
    from_height: Option<usize>,
    lookback: Option<usize>,
    broadcast: bool,
}

fn main() {
    if let Err(e) = dispatch() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn dispatch() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let cmd = match args.next() {
        Some(cmd) if cmd != "-h" && cmd != "--help" && cmd != "ayuda" => cmd,
        _ => {
            print_usage();
            return Ok(());
        }
    };
    let flags = parse_flags(args)?;
    match cmd.as_str() {
        "init" => init(&flags),
        "dkg" => dkg(&flags),
        "check" => block_on(check(&flags)),
        "fund" => block_on(fund_cmd(&flags)),
        "spend" => block_on(spend_cmd(&flags)),
        other => Err(format!("comando {other:?} no existe\n\n{}", usage_text())),
    }
}

fn print_usage() {
    print!("{}", usage_text());
}

fn usage_text() -> String {
    format!(
        "\
stagenet — esqueleto de xmr-joint. Secretos nuevos, red stagenet.

  stagenet init --dir DIR
  stagenet dkg --dir DIR --obra ID
  stagenet check [--daemon URL]
  stagenet fund --dir DIR --obra ID --partida N --capital PICONERO
                [--from-height H | --lookback N] [--daemon URL] [--broadcast]
  stagenet spend --dir DIR --obra ID --capital PICONERO --pct 0-100
                 [--from-height H | --lookback N] [--daemon URL] [--broadcast]

El directorio queda con mandante.seed, contratista.seed, mandante.share
y contratista.share, modo 0600. Si el archivo ya existe, no se pisa.
--capital va en piconero (0,04 XMR = 40000000000).
Sin --broadcast la transacción se guarda en el directorio y no se publica.
Daemon por defecto: {STAGENET_DAEMON}
"
    )
}

fn parse_flags(args: impl Iterator<Item = String>) -> Result<Flags, String> {
    let mut flags = Flags {
        dir: None,
        obra: None,
        daemon: None,
        capital: None,
        pct: None,
        partida: None,
        from_height: None,
        lookback: None,
        broadcast: false,
    };
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dir" => flags.dir = Some(PathBuf::from(need(&mut args, "--dir")?)),
            "--obra" => flags.obra = Some(need(&mut args, "--obra")?),
            "--daemon" => flags.daemon = Some(need(&mut args, "--daemon")?),
            "--capital" => flags.capital = Some(parse_num(&need(&mut args, "--capital")?)?),
            "--pct" => {
                let n: u32 = parse_num(&need(&mut args, "--pct")?)?;
                flags.pct = Some(n);
            }
            "--partida" => flags.partida = Some(parse_num(&need(&mut args, "--partida")?)?),
            "--from-height" => flags.from_height = Some(parse_num(&need(&mut args, "--from-height")?)?),
            "--lookback" => flags.lookback = Some(parse_num(&need(&mut args, "--lookback")?)?),
            "--broadcast" => flags.broadcast = true,
            other => return Err(format!("no entiendo {other}")),
        }
    }
    if flags.from_height.is_some() && flags.lookback.is_some() {
        return Err("usá --from-height o --lookback, no los dos".into());
    }
    Ok(flags)
}

fn need(args: &mut std::iter::Peekable<impl Iterator<Item = String>>, flag: &str) -> Result<String, String> {
    args.next().ok_or_else(|| format!("{flag} necesita un valor"))
}

fn parse_num<T: std::str::FromStr>(s: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("número inválido {s:?}"))
}

fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio")
        .block_on(fut)
}

fn dir_of(flags: &Flags) -> Result<&Path, String> {
    flags.dir.as_deref().ok_or_else(|| "falta --dir".into())
}

fn obra_of(flags: &Flags) -> Result<&str, String> {
    flags.obra.as_deref().filter(|s| !s.is_empty()).ok_or_else(|| "falta --obra".into())
}

fn capital_of(flags: &Flags) -> Result<u64, String> {
    match flags.capital {
        Some(0) | None => Err("falta --capital en piconero, mayor que cero".into()),
        Some(n) => Ok(n),
    }
}

fn daemon_of(flags: &Flags) -> &str {
    flags.daemon.as_deref().unwrap_or(STAGENET_DAEMON)
}

fn seed_path(dir: &Path, role: &str) -> PathBuf {
    dir.join(format!("{role}.seed"))
}

fn share_path(dir: &Path, role: &str) -> PathBuf {
    dir.join(format!("{role}.share"))
}

fn init(flags: &Flags) -> Result<(), String> {
    let dir = dir_of(flags)?;
    let mut rng = OsRng;
    let (mandante, words_m) = SingleWallet::generate(&mut rng, Net::Stagenet).map_err(|e| e.to_string())?;
    let (contratista, words_c) = SingleWallet::generate(&mut rng, Net::Stagenet).map_err(|e| e.to_string())?;
    write_seed(dir, "mandante", mandante.address(), &words_m)?;
    write_seed(dir, "contratista", contratista.address(), &words_c)?;
    println!("Semillas nuevas en {} (0600). No son las de ningún laboratorio.", dir.display());
    println!("mandante:    {}", mandante.address());
    println!("contratista: {}", contratista.address());
    println!("Siguiente: stagenet dkg --dir {} --obra ID", dir.display());
    Ok(())
}

fn write_seed(dir: &Path, role: &str, address: &str, words: &str) -> Result<(), String> {
    let backup = SeedBackup {
        net: Net::Stagenet,
        address: address.to_string(),
        words: zeroize::Zeroizing::new(words.to_string()),
    };
    backup::write_secret_file(&seed_path(dir, role), &backup.to_text()).map_err(|e| e.to_string())
}

fn dkg(flags: &Flags) -> Result<(), String> {
    let dir = dir_of(flags)?;
    let obra = obra_of(flags)?;
    let _ = load_wallet(dir, "mandante")?;
    let _ = load_wallet(dir, "contratista")?;
    let mut rng = OsRng;
    let (mut mandante, c1) = DkgParty::start(Party::Mandante, obra, Net::Stagenet, &mut rng).map_err(|e| e.to_string())?;
    let (mut contratista, c2) =
        DkgParty::start(Party::Contratista, obra, Net::Stagenet, &mut rng).map_err(|e| e.to_string())?;
    let s_for_c = mandante.ingest_commit(&c2, &mut rng).map_err(|e| e.to_string())?;
    let s_for_m = contratista.ingest_commit(&c1, &mut rng).map_err(|e| e.to_string())?;
    let done_c = contratista.ingest_share(&s_for_c, &mut rng).map_err(|e| e.to_string())?;
    if done_c.account.is_some() || done_c.view.is_some() {
        return Err("el contratista no debería cerrar antes de la view".into());
    }
    let done_m = mandante.ingest_share(&s_for_m, &mut rng).map_err(|e| e.to_string())?;
    let account_m = done_m.account.ok_or("el mandante no cerró la caja")?;
    let view = done_m.view.ok_or("el mandante no armó la view")?;
    let account_c = contratista
        .ingest_view(&xmr_joint::dkg::ViewAnnounce::decode(&view.encode()).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if account_m.address() != account_c.address() {
        return Err("las dos partes no llegaron a la misma dirección".into());
    }
    write_share(dir, &account_m)?;
    write_share(dir, &account_c)?;
    println!("Caja 2-de-2");
    println!("obra: {}", account_m.obra_id());
    println!("dirección: {}", account_m.address());
    println!("shares: mandante.share y contratista.share");
    println!("La semilla personal no reconstruye estos shares.");
    Ok(())
}

fn write_share(dir: &Path, account: &JointAccount) -> Result<(), String> {
    let backup = account.backup().map_err(|e| e.to_string())?;
    backup::write_secret_file(&share_path(dir, account.role().label()), &backup.to_text()).map_err(|e| e.to_string())
}

fn load_wallet(dir: &Path, role: &str) -> Result<SingleWallet, String> {
    let text = backup::read_secret_file(&seed_path(dir, role)).map_err(|e| format!("{role}.seed: {e}"))?;
    let backup = SeedBackup::parse(&text).map_err(|e| e.to_string())?;
    if backup.net != Net::Stagenet {
        return Err(format!("{role}.seed no es stagenet"));
    }
    let wallet = SingleWallet::restore(backup.net, backup.words.as_str()).map_err(|e| e.to_string())?;
    if wallet.address() != backup.address {
        return Err(format!("la dirección de {role}.seed no sale de esas palabras"));
    }
    Ok(wallet)
}

fn load_share(dir: &Path, role: Party, obra: &str) -> Result<JointAccount, String> {
    let text = backup::read_secret_file(&share_path(dir, role.label())).map_err(|e| format!("{}.share: {e}", role.label()))?;
    let backup = xmr_joint::backup::ShareBackup::parse(&text).map_err(|e| e.to_string())?;
    if backup.net != Net::Stagenet {
        return Err(format!("{}.share no es stagenet", role.label()));
    }
    if backup.obra_id != obra {
        return Err(format!("{}.share es de la obra {}, no de {obra}", role.label(), backup.obra_id));
    }
    let account = JointAccount::from_backup(&backup).map_err(|e| e.to_string())?;
    if account.role() != role {
        return Err(format!("{}.share tiene otro rol", role.label()));
    }
    Ok(account)
}

async fn check(flags: &Flags) -> Result<(), String> {
    let url = daemon_of(flags);
    let rpc = chain::connect(url).await.map_err(|e| e.to_string())?;
    let tip = chain::tip(&rpc).await.map_err(|e| e.to_string())?;
    println!("daemon: {url}");
    println!("tip: {tip}");
    Ok(())
}

async fn height_range(rpc: &chain::Daemon, flags: &Flags) -> Result<(usize, usize), String> {
    let tip = chain::tip(rpc).await.map_err(|e| e.to_string())?;
    let from = match flags.from_height {
        Some(h) => h,
        None => tip.saturating_sub(flags.lookback.unwrap_or(LOOKBACK)),
    };
    if from > tip {
        return Err(format!("--from-height {from} queda después del tip {tip}"));
    }
    println!("escaneo {from}..={tip}");
    Ok((from, tip))
}

async fn fund_cmd(flags: &Flags) -> Result<(), String> {
    let dir = dir_of(flags)?;
    let obra = obra_of(flags)?;
    let capital = capital_of(flags)?;
    let partida = flags.partida.ok_or("falta --partida")?;
    let mandante = load_wallet(dir, "mandante")?;
    let contratista = load_wallet(dir, "contratista")?;
    let box_m = load_share(dir, Party::Mandante, obra)?;
    let box_c = load_share(dir, Party::Contratista, obra)?;
    if box_m.address() != box_c.address() {
        return Err("los dos shares no son de la misma caja".into());
    }
    let url = daemon_of(flags);
    let rpc = chain::connect(url).await.map_err(|e| e.to_string())?;
    let (from, tip) = height_range(&rpc, flags).await?;
    let rate = chain::fee_rate(&rpc).await.map_err(|e| e.to_string())?;
    let outs_m = chain::scan(&rpc, mandante.view_pair(), from, tip).await.map_err(|e| e.to_string())?;
    let outs_c = chain::scan(&rpc, contratista.view_pair(), from, tip).await.map_err(|e| e.to_string())?;
    let picked_m = fund::pick_output(outs_m, capital.saturating_add(FEE_CUSHION)).map_err(|e| {
        format!("{e}. El mandante necesita un output de al menos {} piconero (capital + 0,001 XMR de margen).", capital + FEE_CUSHION)
    })?;
    let picked_c = fund::pick_output(outs_c, capital).map_err(|e| format!("contratista: {e}"))?;
    println!("mandante aporta {} piconero", picked_m.commitment().amount);
    println!("contratista aporta {} piconero", picked_c.commitment().amount);
    let decoy_m = [chain::with_decoys(&rpc, picked_m, tip).await.map_err(|e| e.to_string())?];
    let decoy_c = [chain::with_decoys(&rpc, picked_c, tip).await.map_err(|e| e.to_string())?];
    let mut ovk = [0u8; 32];
    OsRng.fill_bytes(&mut ovk);
    let proposal = fund::mandante_proposal(
        Net::Stagenet,
        obra,
        partida,
        box_m.address(),
        capital,
        &decoy_m,
        mandante.spend_key(),
        fund::fee_parts(&rate),
        ovk,
    )
    .map_err(|e| e.to_string())?;
    let skeleton = fund::contratista_responde(
        Net::Stagenet,
        box_m.address(),
        capital,
        &proposal,
        mandante.view_pair(),
        &decoy_c,
        contratista.spend_key(),
        contratista.address(),
    )
    .map_err(|e| e.to_string())?;
    let tx = fund::mandante_cierra(Net::Stagenet, &proposal, &skeleton, &decoy_m, mandante.spend_key())
        .map_err(|e| e.to_string())?;
    finish_tx(dir, "last-fund.tx.hex", &tx, flags.broadcast, &rpc).await?;
    println!("destino: {}", box_m.address());
    println!("cada parte puso {capital} piconero. El pote de la caja queda en {}", capital * 2);
    Ok(())
}

async fn spend_cmd(flags: &Flags) -> Result<(), String> {
    let dir = dir_of(flags)?;
    let obra = obra_of(flags)?;
    let capital = capital_of(flags)?;
    let pct = flags.pct.ok_or("falta --pct")?;
    if pct > 100 {
        return Err("el porcentaje pasa de 100".into());
    }
    let mandante = load_wallet(dir, "mandante")?;
    let contratista = load_wallet(dir, "contratista")?;
    let box_m = load_share(dir, Party::Mandante, obra)?;
    let box_c = load_share(dir, Party::Contratista, obra)?;
    if box_m.address() != box_c.address() {
        return Err("los dos shares no son de la misma caja".into());
    }
    let url = daemon_of(flags);
    let rpc = chain::connect(url).await.map_err(|e| e.to_string())?;
    let (from, tip) = height_range(&rpc, flags).await?;
    let view = box_m.view_pair().map_err(|e| e.to_string())?;
    let outs = chain::scan(&rpc, view, from, tip).await.map_err(|e| e.to_string())?;
    let matching: Vec<_> = outs.into_iter().filter(|o| o.commitment().amount == capital).collect();
    if matching.len() != 2 {
        return Err(format!(
            "encontré {} salidas de {capital} piconero en la caja. Este esqueleto gasta exactamente las dos del fondeo.",
            matching.len()
        ));
    }
    let rate = chain::fee_rate(&rpc).await.map_err(|e| e.to_string())?;
    let mut decoys = Vec::with_capacity(2);
    for output in matching {
        decoys.push(chain::with_decoys(&rpc, output, tip).await.map_err(|e| e.to_string())?);
    }
    let c_addr = parse_addr(contratista.address())?;
    let m_addr = parse_addr(mandante.address())?;
    let (proposal, split) = spend::propose(&mut OsRng, obra, capital, pct, &c_addr, &m_addr, decoys, rate)
        .map_err(|e| e.to_string())?;
    println!(
        "reparto: contratista {}  mandante {}  fee {}",
        split.contratista, split.mandante, split.fee
    );
    let mut rng = OsRng;
    let (sess_m, pre_m) = SpendSession::open(&box_m, &proposal, &mut rng).map_err(|e| e.to_string())?;
    let (sess_c, pre_c) = SpendSession::open(&box_c, &proposal, &mut rng).map_err(|e| e.to_string())?;
    let (signed_m, share_m) = sess_m.sign(&pre_c).map_err(|e| e.to_string())?;
    let (signed_c, share_c) = sess_c.sign(&pre_m).map_err(|e| e.to_string())?;
    let tx_m = signed_m.complete(&share_c).map_err(|e| e.to_string())?;
    let tx_c = signed_c.complete(&share_m).map_err(|e| e.to_string())?;
    if tx_m.hash() != tx_c.hash() {
        return Err("las dos partes no armaron la misma transacción".into());
    }
    finish_tx(dir, "last-spend.tx.hex", &tx_m, flags.broadcast, &rpc).await?;
    Ok(())
}

fn parse_addr(text: &str) -> Result<MoneroAddress, String> {
    MoneroAddress::from_str(Net::Stagenet.oxide(), text).map_err(|e| format!("dirección: {e}"))
}

async fn finish_tx(
    dir: &Path,
    name: &str,
    tx: &monero_wallet::transaction::Transaction,
    broadcast: bool,
    rpc: &chain::Daemon,
) -> Result<(), String> {
    let txid = hex::encode(tx.hash());
    let path = dir.join(name);
    std::fs::write(&path, hex::encode(tx.serialize())).map_err(|e| e.to_string())?;
    println!("txid: {txid}");
    println!("blob: {}", path.display());
    if broadcast {
        chain::publish(rpc, tx).await.map_err(|e| e.to_string())?;
        println!("publicada. Todavía no está confirmada.");
    } else {
        println!("no se publicó. Para mandarla: repetí el comando con --broadcast, o el blob ya está en el archivo.");
    }
    Ok(())
}
