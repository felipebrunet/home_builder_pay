//! Coordinator: xpubs in, PSBTs out. No seeds.

use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{bail, Context, Result};
use base64::Engine;
use bitcoin::psbt::Psbt;
use bitcoin::{Address, OutPoint};
use clap::{Parser, Subcommand};
use hbp_bitcoin::{
    attach_prev_tx, build_burn_psbt, build_coop_psbt, build_funding_psbt, combine_psbts, escrow_at,
    extract_signed_funding_tx, extract_wsh_tx, import_watch, normalize_cosigner_key, scan_watch,
    to_btc_network, CoopOutput, FundingCoin, FundingRequest, OfferedCoin, WatchKind,
};
use hbp_core::{Mode, Network, Offer, Project, Role, Terms};

mod esplora;
mod psbt_io;
mod store;

use esplora::Esplora;
use store::{read_json, Store, Session};

#[derive(Parser)]
#[command(name = "hbp", about = "home_builder_pay — P2WSH 2-of-2 coordinator (no keys)")]
struct Cli {
    #[arg(long)]
    dir: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Network + role only. No secret is created.
    Init {
        #[arg(long)]
        network: Network,
        #[arg(long)]
        role: Role,
    },
    /// BIP48 cosigner xpub (Zpub/Vpub/tpub). Shared with the peer.
    Cosigner {
        xpub: String,
    },
    /// Local singlesig watch-only (m/84). Never sent to the peer.
    WatchImport {
        #[arg(long)]
        xpub: String,
        #[arg(long)]
        kind: Option<WatchKind>,
    },
    /// Mandante: create the offer (hold | burn).
    New {
        #[arg(long)]
        mode: String,
        #[arg(long)]
        sats: u64,
        #[arg(long, default_value_t = 500)]
        fee: u64,
        #[arg(long)]
        t_unix: Option<u32>,
        #[arg(long, default_value_t = 0)]
        index: u32,
    },
    Offer,
    /// Contratista: attach your m/48 xpub and write 01-accepted.json.
    Accept {
        file: PathBuf,
    },
    /// Mandante: import 01-accepted.json.
    Import {
        file: PathBuf,
    },
    Addresses,
    Status,
    Coins {
        #[arg(long, env = "HBP_ESPLORA")]
        esplora: Option<String>,
    },
    OfferCoin {
        #[arg(long)]
        outpoint: String,
        #[arg(long)]
        sats: Option<u64>,
        #[arg(long)]
        address: Option<String>,
        #[arg(long)]
        change: Option<String>,
        #[arg(long, env = "HBP_ESPLORA")]
        esplora: Option<String>,
    },
    /// Build funding PSBT (and burn PSBT in burn mode). Sign burn first.
    Fund {
        #[arg(long)]
        mine: PathBuf,
        #[arg(long)]
        peer: PathBuf,
        #[arg(long)]
        fee: Option<u64>,
        #[arg(long, env = "HBP_ESPLORA")]
        esplora: Option<String>,
    },
    CombineBurn {
        files: Vec<PathBuf>,
    },
    CombineFund {
        files: Vec<PathBuf>,
        #[arg(long, env = "HBP_ESPLORA")]
        esplora: Option<String>,
    },
    Coop {
        #[arg(long)]
        dest: String,
        #[arg(long)]
        sats: Option<u64>,
        #[arg(long, default_value_t = 200)]
        fee: u64,
    },
    CombineCoop {
        files: Vec<PathBuf>,
        #[arg(long, env = "HBP_ESPLORA")]
        esplora: Option<String>,
    },
    /// Broadcast the stored fully-signed burn (after T).
    PublishBurn {
        #[arg(long, env = "HBP_ESPLORA")]
        esplora: Option<String>,
    },
    Sync {
        #[arg(long, env = "HBP_ESPLORA")]
        esplora: Option<String>,
    },
    /// Save a signed PSBT (path or base64) as <kind>.signed.psbt in --dir.
    PutPsbt {
        #[arg(long)]
        kind: String,
        input: String,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let store = Store::new(cli.dir);
    match cli.cmd {
        Cmd::Init { network, role } => cmd_init(&store, network, role),
        Cmd::Cosigner { xpub } => cmd_cosigner(&store, &xpub),
        Cmd::WatchImport { xpub, kind } => cmd_watch(&store, &xpub, kind),
        Cmd::New {
            mode,
            sats,
            fee,
            t_unix,
            index,
        } => cmd_new(&store, &mode, sats, fee, t_unix, index),
        Cmd::Offer => cmd_offer(&store),
        Cmd::Accept { file } => cmd_accept(&store, file),
        Cmd::Import { file } => cmd_import(&store, file),
        Cmd::Addresses => cmd_addresses(&store),
        Cmd::Status => cmd_status(&store),
        Cmd::Coins { esplora } => cmd_coins(&store, esplora.as_deref()),
        Cmd::OfferCoin {
            outpoint,
            sats,
            address,
            change,
            esplora,
        } => cmd_offer_coin(
            &store,
            &outpoint,
            sats,
            address.as_deref(),
            change.as_deref(),
            esplora.as_deref(),
        ),
        Cmd::Fund {
            mine,
            peer,
            fee,
            esplora,
        } => cmd_fund(&store, mine, peer, fee, esplora.as_deref()),
        Cmd::CombineBurn { files } => cmd_combine_burn(&store, files),
        Cmd::CombineFund { files, esplora } => cmd_combine_fund(&store, files, esplora.as_deref()),
        Cmd::Coop { dest, sats, fee } => cmd_coop(&store, &dest, sats, fee),
        Cmd::CombineCoop { files, esplora } => cmd_combine_coop(&store, files, esplora.as_deref()),
        Cmd::PublishBurn { esplora } => cmd_publish_burn(&store, esplora.as_deref()),
        Cmd::Sync { esplora } => cmd_sync(&store, esplora.as_deref()),
        Cmd::PutPsbt { kind, input } => cmd_put_psbt(&store, &kind, &input),
    }
}

fn cmd_init(store: &Store, network: Network, role: Role) -> Result<()> {
    store.save_session(&Session {
        network,
        role,
        cosigner_xpub: None,
    })?;
    eprintln!("session {} {:?}", store.root.display(), role);
    Ok(())
}

fn cmd_cosigner(store: &Store, xpub: &str) -> Result<()> {
    let mut s = store.load_session()?;
    let key = normalize_cosigner_key(xpub)?;
    s.cosigner_xpub = Some(key.clone());
    store.save_session(&s)?;
    eprintln!("cosigner {key}");
    println!("{key}");
    Ok(())
}

fn cmd_watch(store: &Store, xpub: &str, kind: Option<WatchKind>) -> Result<()> {
    let s = store.load_session()?;
    let acc = import_watch(xpub, kind, s.network, 20)?;
    store.save_watch(&acc)?;
    let addr = hbp_bitcoin::address_at(&acc.receive_descriptor, 0, s.network)?;
    eprintln!("watch-only {:?}  receive0 {addr}", acc.kind);
    println!("{addr}");
    Ok(())
}

fn parse_mode(mode: &str, t_unix: Option<u32>) -> Result<Mode> {
    match mode.to_ascii_lowercase().as_str() {
        "hold" => Ok(Mode::Hold),
        "burn" => {
            let t = t_unix.context("--t-unix required for burn (unix time)")?;
            Ok(Mode::Burn { t_unix: t })
        }
        other => bail!("mode must be hold|burn, got {other}"),
    }
}

fn cmd_new(
    store: &Store,
    mode: &str,
    sats: u64,
    fee: u64,
    t_unix: Option<u32>,
    index: u32,
) -> Result<()> {
    let s = store.load_session()?;
    if s.role != Role::Mandante {
        bail!("only the mandante creates the offer");
    }
    let xpub = s
        .cosigner_xpub
        .clone()
        .context("set your m/48 xpub first: hbp cosigner XPUB")?;
    let terms = Terms {
        network: s.network,
        mode: parse_mode(mode, t_unix)?,
        sats,
        fee,
        index,
        mandante_xpub: xpub,
        contratista_xpub: None,
    };
    let p = Project::from_offer(Offer {
        terms: terms.clone(),
    });
    store.save_project(&p)?;
    let path = store.save_offer_terms(&terms)?;
    eprintln!("{}", path.display());
    println!("{}", serde_json::to_string_pretty(&terms)?);
    Ok(())
}

fn cmd_offer(store: &Store) -> Result<()> {
    let p = store.load_project()?;
    let path = store.save_offer_terms(&p.terms)?;
    println!("{}", path.display());
    Ok(())
}

fn cmd_accept(store: &Store, file: PathBuf) -> Result<()> {
    let s = store.load_session()?;
    if s.role != Role::Contratista {
        bail!("accept is for the contratista");
    }
    let xpub = s
        .cosigner_xpub
        .clone()
        .context("set your m/48 xpub first: hbp cosigner XPUB")?;
    let terms: Terms = read_json(&file)?;
    if terms.network != s.network {
        bail!("offer network {:?} != session {:?}", terms.network, s.network);
    }
    let mut p = Project::from_offer(Offer { terms });
    p.accept(xpub)?;
    store.save_project(&p)?;
    let path = store.save_accepted(&p.terms)?;
    eprintln!("{}", path.display());
    println!("{}", serde_json::to_string_pretty(&p.terms)?);
    Ok(())
}

fn cmd_import(store: &Store, file: PathBuf) -> Result<()> {
    let mut p = store.load_project()?;
    let terms: Terms = read_json(&file)?;
    p.import_accepted(terms)?;
    store.save_project(&p)?;
    store.save_accepted(&p.terms)?;
    println!("{}", p.id()?);
    Ok(())
}

fn escrow_of(p: &Project) -> Result<hbp_bitcoin::Escrow> {
    let (a, b) = p.terms.require_complete()?;
    Ok(escrow_at(a, b, p.terms.index, p.terms.network)?)
}

fn cmd_addresses(store: &Store) -> Result<()> {
    let p = store.load_project()?;
    let e = escrow_of(&p)?;
    println!("{}", e.address);
    eprintln!("descriptor {}", e.descriptor);
    if let Mode::Burn { t_unix } = p.terms.mode {
        eprintln!("burn after {t_unix}");
    }
    Ok(())
}

fn cmd_status(store: &Store) -> Result<()> {
    let p = store.load_project()?;
    println!("{}", serde_json::to_string_pretty(&p)?);
    Ok(())
}

fn resolve_esplora(store: &Store, explicit: Option<&str>) -> Result<Esplora> {
    let s = store.load_session()?;
    let candidates: Vec<String> = if let Some(u) = explicit {
        vec![u.to_string()]
    } else {
        let urls = hbp_bitcoin::default_esplora_urls(s.network);
        if urls.is_empty() {
            bail!("no default Esplora for {:?}; pass --esplora", s.network);
        }
        urls.iter().map(|x| (*x).to_string()).collect()
    };
    let c = Esplora::connect(&candidates)?;
    eprintln!("esplora {}", c.base);
    Ok(c)
}

fn hbp_err(e: anyhow::Error) -> hbp_bitcoin::Error {
    hbp_bitcoin::Error::msg(e.to_string())
}

fn cmd_coins(store: &Store, esplora: Option<&str>) -> Result<()> {
    let acc = store.load_watch()?;
    let client = resolve_esplora(store, esplora)?;
    let scan = scan_watch(&acc, |addr| client.address_utxos(addr).map_err(hbp_err))?;
    eprintln!(
        "{} UTXO(s); receive {} ; change {}",
        scan.utxos.len(),
        scan.receive,
        scan.change
    );
    println!("{}", serde_json::to_string_pretty(&scan)?);
    Ok(())
}

fn parse_outpoint(raw: &str) -> Result<OutPoint> {
    let s = raw.trim().replace(',', ":").replace([' ', '\t'], "");
    OutPoint::from_str(&s).map_err(|e| anyhow::anyhow!("outpoint '{raw}': {e}"))
}

fn cmd_offer_coin(
    store: &Store,
    outpoint: &str,
    sats: Option<u64>,
    address: Option<&str>,
    change: Option<&str>,
    esplora: Option<&str>,
) -> Result<()> {
    let s = store.load_session()?;
    let want = parse_outpoint(outpoint)?;
    let (sats, address, change, prev_tx_hex) = if let (Some(sats), Some(address), Some(change)) =
        (sats, address, change)
    {
        (sats, address.to_string(), change.to_string(), None)
    } else {
        let acc = store.load_watch()?;
        let client = resolve_esplora(store, esplora)?;
        let scan = scan_watch(&acc, |addr| client.address_utxos(addr).map_err(hbp_err))?;
        let found = scan
            .utxos
            .iter()
            .find(|u| u.outpoint == want.to_string())
            .ok_or_else(|| anyhow::anyhow!("outpoint {want} is not on this watch-only"))?;
        let change = change.unwrap_or(&scan.change).to_string();
        let prev_tx_hex = match client.tx_hex(&want.txid.to_string()) {
            Ok(h) => Some(h),
            Err(e) => {
                eprintln!("warning: prev tx ({e:#})");
                None
            }
        };
        (found.sats, found.address.clone(), change, prev_tx_hex)
    };
    let coin = OfferedCoin {
        role: s.role,
        outpoint: want.to_string(),
        sats,
        address,
        change,
        prev_tx_hex,
    };
    let path = store.coin_path();
    store::write_json(&path, &coin)?;
    eprintln!("{}", path.display());
    println!("{}", serde_json::to_string_pretty(&coin)?);
    Ok(())
}

fn psbt_b64(p: &Psbt) -> String {
    base64::engine::general_purpose::STANDARD.encode(p.serialize())
}

fn load_coin(path: &PathBuf) -> Result<OfferedCoin> {
    read_json(path)
}

fn cmd_fund(
    store: &Store,
    mine: PathBuf,
    peer: PathBuf,
    fee: Option<u64>,
    esplora: Option<&str>,
) -> Result<()> {
    let p = store.load_project()?;
    p.terms.require_complete()?;
    let s = store.load_session()?;
    let mine_c = load_coin(&mine)?;
    let peer_c = load_coin(&peer)?;
    if mine_c.role == peer_c.role {
        bail!("mine and peer coins have the same role");
    }
    let (m_coin, c_coin) = if mine_c.role == Role::Mandante {
        (mine_c, peer_c)
    } else {
        (peer_c, mine_c)
    };
    let fee = fee.unwrap_or(p.terms.fee);
    let escrow = escrow_of(&p)?;
    let req = FundingRequest {
        escrow: escrow.script_pubkey.clone(),
        escrow_sats: p.terms.escrow_sats(),
        fee,
        mandante: FundingCoin {
            outpoint: m_coin.outpoint()?,
            sats: m_coin.sats,
            script_pubkey: Address::from_str(&m_coin.address)?
                .require_network(to_btc_network(s.network))?
                .script_pubkey(),
        },
        mandante_change: m_coin.change_address(s.network)?,
        contratista: FundingCoin {
            outpoint: c_coin.outpoint()?,
            sats: c_coin.sats,
            script_pubkey: Address::from_str(&c_coin.address)?
                .require_network(to_btc_network(s.network))?
                .script_pubkey(),
        },
        contratista_change: c_coin.change_address(s.network)?,
    };
    let mut funding = build_funding_psbt(&req)?;
    if let Some(prev) = m_coin.prev_tx()? {
        attach_prev_tx(&mut funding, m_coin.outpoint()?, prev)?;
    }
    if let Some(prev) = c_coin.prev_tx()? {
        attach_prev_tx(&mut funding, c_coin.outpoint()?, prev)?;
    }
    psbt_io::write_psbt_binary(&store.funding_psbt_path(), &funding)?;

    let mut out = serde_json::json!({
        "address": escrow.address.to_string(),
        "escrow_sats": p.terms.escrow_sats(),
        "funding_psbt": psbt_b64(&funding),
        "funding_path": store.funding_psbt_path().display().to_string(),
        "mode": if p.terms.mode.is_burn() { "burn" } else { "hold" },
    });

    if let Mode::Burn { t_unix } = p.terms.mode {
        let txid = funding.unsigned_tx.compute_txid();
        let burn = build_burn_psbt(
            &escrow,
            OutPoint { txid, vout: 0 },
            p.terms.escrow_sats(),
            t_unix,
        )?;
        psbt_io::write_psbt_binary(&store.burn_psbt_path(), &burn)?;
        out["burn_psbt"] = serde_json::Value::String(psbt_b64(&burn));
        out["burn_path"] = serde_json::Value::String(store.burn_psbt_path().display().to_string());
        out["t_unix"] = serde_json::Value::from(t_unix);
        eprintln!("burn mode: sign the BURN psbt first (m/48), then funding (m/84)");
    } else {
        eprintln!("hold mode: sign the funding psbt (m/84)");
    }
    let _ = esplora;
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

fn cmd_combine_burn(store: &Store, files: Vec<PathBuf>) -> Result<()> {
    let mut p = store.load_project()?;
    if !p.terms.mode.is_burn() {
        bail!("hold mode has no burn");
    }
    let parts: Vec<Psbt> = files.iter().map(|f| psbt_io::load_psbt(f)).collect::<Result<_>>()?;
    let comb = combine_psbts(&parts)?;
    let tx = extract_wsh_tx(comb.clone()).context("burn PSBT not fully signed (need both m/48)")?;
    psbt_io::write_psbt_binary(&store.burn_psbt_path(), &comb)?;
    p.set_burn_psbt(psbt_b64(&comb))?;
    store.save_project(&p)?;
    eprintln!("burn ready locktime {} txid {}", tx.lock_time, tx.compute_txid());
    println!("{}", psbt_b64(&comb));
    Ok(())
}

fn try_broadcast(store: &Store, hex: &str, esplora: Option<&str>) -> Result<Option<String>> {
    match resolve_esplora(store, esplora) {
        Ok(c) => {
            let txid = c.broadcast(hex)?;
            eprintln!("broadcast {txid}");
            Ok(Some(txid))
        }
        Err(_) => {
            eprintln!("no Esplora; broadcast the hex yourself");
            Ok(None)
        }
    }
}

fn cmd_combine_fund(store: &Store, files: Vec<PathBuf>, esplora: Option<&str>) -> Result<()> {
    let mut p = store.load_project()?;
    if p.terms.mode.is_burn() && p.burn_psbt.is_none() {
        bail!("sign and combine-burn before combining funding");
    }
    let parts: Vec<Psbt> = files.iter().map(|f| psbt_io::load_psbt(f)).collect::<Result<_>>()?;
    let comb = combine_psbts(&parts)?;
    let tx = extract_signed_funding_tx(comb).context("funding not fully signed")?;
    let hex = hex::encode(bitcoin::consensus::serialize(&tx));
    let txid = tx.compute_txid().to_string();
    let _ = try_broadcast(store, &hex, esplora)?;
    p.mark_funded(txid.clone(), 0, p.terms.escrow_sats())?;
    store.save_project(&p)?;
    println!("{hex}");
    eprintln!("funded {txid} vout 0");
    Ok(())
}

fn cmd_coop(store: &Store, dest: &str, sats: Option<u64>, fee: u64) -> Result<()> {
    let p = store.load_project()?;
    let (txid, vout, escrow_sats) = p
        .funded_utxo()
        .ok_or_else(|| anyhow::anyhow!("not funded yet"))?;
    let s = store.load_session()?;
    let dest = Address::from_str(dest.trim())?
        .require_network(to_btc_network(s.network))?;
    let pay = sats.unwrap_or(escrow_sats.saturating_sub(fee));
    let escrow = escrow_of(&p)?;
    let psbt = build_coop_psbt(
        &escrow,
        OutPoint::from_str(&format!("{txid}:{vout}"))?,
        escrow_sats,
        &[CoopOutput {
            address: dest,
            sats: pay,
        }],
        fee,
    )?;
    psbt_io::write_psbt_binary(&store.coop_psbt_path(), &psbt)?;
    let out = serde_json::json!({
        "coop_psbt": psbt_b64(&psbt),
        "coop_path": store.coop_psbt_path().display().to_string(),
        "pay": pay,
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

fn cmd_combine_coop(store: &Store, files: Vec<PathBuf>, esplora: Option<&str>) -> Result<()> {
    let mut p = store.load_project()?;
    let parts: Vec<Psbt> = files.iter().map(|f| psbt_io::load_psbt(f)).collect::<Result<_>>()?;
    let comb = combine_psbts(&parts)?;
    let tx = extract_wsh_tx(comb).context("coop PSBT not fully signed")?;
    let hex = hex::encode(bitcoin::consensus::serialize(&tx));
    let txid = tx.compute_txid().to_string();
    let _ = try_broadcast(store, &hex, esplora)?;
    p.mark_closed(txid.clone())?;
    store.save_project(&p)?;
    println!("{hex}");
    eprintln!("closed {txid}");
    Ok(())
}

fn cmd_publish_burn(store: &Store, esplora: Option<&str>) -> Result<()> {
    let mut p = store.load_project()?;
    let b64 = p
        .burn_psbt
        .as_ref()
        .context("no complete burn PSBT (combine-burn first)")?;
    let raw = base64::engine::general_purpose::STANDARD.decode(b64)?;
    let psbt = Psbt::deserialize(&raw)?;
    let tx = extract_wsh_tx(psbt)?;
    let hex = hex::encode(bitcoin::consensus::serialize(&tx));
    match try_broadcast(store, &hex, esplora) {
        Ok(Some(txid)) => {
            p.mark_burned(txid.clone())?;
            store.save_project(&p)?;
            println!("{hex}");
            eprintln!("burned {txid}");
        }
        Ok(None) => {
            println!("{hex}");
        }
        Err(e) => {
            println!("{hex}");
            bail!("broadcast failed: {e:#}");
        }
    }
    Ok(())
}

fn cmd_put_psbt(store: &Store, kind: &str, input: &str) -> Result<()> {
    let kind = kind.to_ascii_lowercase();
    if !matches!(kind.as_str(), "funding" | "burn" | "coop") {
        bail!("kind must be funding|burn|coop");
    }
    let psbt = if PathBuf::from(input).exists() {
        psbt_io::load_psbt(&PathBuf::from(input))?
    } else {
        let raw = base64::engine::general_purpose::STANDARD
            .decode(input.trim())
            .or_else(|_| hex::decode(input.trim()))
            .context("not a file, base64 PSBT, or hex PSBT")?;
        Psbt::deserialize(&raw).context("PSBT deserialize")?
    };
    let path = store.root.join(format!("{kind}.signed.psbt"));
    psbt_io::write_psbt_binary(&path, &psbt)?;
    eprintln!("{}", path.display());
    println!("{}", path.display());
    Ok(())
}

fn cmd_sync(store: &Store, esplora: Option<&str>) -> Result<()> {
    let mut p = store.load_project()?;
    let Some((txid, vout, _)) = p.funded_utxo().map(|(t, v, s)| (t.to_string(), v, s)) else {
        println!("{}", serde_json::to_string_pretty(&p)?);
        return Ok(());
    };
    let client = resolve_esplora(store, esplora)?;
    let spends = client.outspends(&txid)?;
    if let Some(sp) = spends.get(vout as usize) {
        if sp.spent {
            if let Some(spend_txid) = &sp.txid {
                let hex = client.tx_hex(spend_txid)?;
                let raw = hex::decode(hex.trim())?;
                let tx: bitcoin::Transaction = bitcoin::consensus::deserialize(&raw)?;
                if tx.lock_time.to_consensus_u32() >= 500_000_000
                    || tx.output.iter().any(|o| o.script_pubkey.is_op_return())
                {
                    p.mark_burned(spend_txid.clone())?;
                    eprintln!("burned {spend_txid}");
                } else {
                    p.mark_closed(spend_txid.clone())?;
                    eprintln!("closed {spend_txid}");
                }
                store.save_project(&p)?;
            }
        }
    }
    println!("{}", serde_json::to_string_pretty(&p)?);
    Ok(())
}
