use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{bail, Context, Result};
use bitcoin::consensus::encode::{deserialize, serialize_hex};
use bitcoin::{Address, Amount, OutPoint, TxOut};
use clap::{Parser, Subcommand};
use hbp_bitcoin::{
    apply_key_spend_sig, bond_address, bond_descriptor, build_key_spend_tx, build_unwind_tx,
    finish_coop_signature, generate_identity, key_spend_sighash, keys_from_body, new_nonce_seed,
    partida_address, partida_descriptor, sign_body, sign_unwind, validate_funding_tx, verify_body,
    ExpectedFunding, Identity,
};
use hbp_core::{
    bond_minor, bond_warnings, fiat_minor_to_sats, minor_from_major, ContractBody, Network, Offer,
    PartidaQuote, PartidaSpec, Quote, Role, SignedContract, Unit,
};

mod store;
use store::{read_json, Store};

#[derive(Parser)]
#[command(
    name = "hbp",
    about = "home_builder_pay — 2-of-2 Taproot partidas + boleta"
)]
struct Cli {
    #[arg(long, global = true, default_value = ".hbp")]
    dir: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create a local identity (toy keys, plaintext on disk).
    Init {
        #[arg(long, default_value = "regtest")]
        network: String,
        #[arg(long)]
        role: Option<String>,
    },
    /// Start a contract draft as mandante.
    New {
        #[arg(long, default_value = "USD")]
        unit: String,
        #[arg(long, default_value_t = 1000)]
        bond_bps: u16,
        /// Unix time (CLTV) for boleta unwind. Must be >= last partida plazo.
        #[arg(long)]
        t_project: u32,
    },
    AddPartida {
        #[arg(long)]
        desc: String,
        /// Amount in contract unit, e.g. 1500.50
        #[arg(long)]
        amount: String,
        #[arg(long)]
        plazo: u32,
    },
    Offer,
    /// Contratista accepts an offer file.
    Accept {
        file: PathBuf,
    },
    /// Mandante countersigns the accepted contract.
    Commit {
        file: PathBuf,
    },
    /// Record sats for the boleta and every partida (both must sign).
    Quote {
        #[arg(long)]
        bond_sats: Option<u64>,
        /// BTC price in the contract unit, e.g. 80000 (used if --bond-sats omitted).
        #[arg(long)]
        btc_price: Option<String>,
        #[arg(long, default_value = "manual")]
        fx_note: String,
    },
    AcceptQuote {
        file: PathBuf,
    },
    Addresses,
    Status,
    /// Load a countersigned contract (contratista side).
    Import {
        file: PathBuf,
    },
    VerifyFunding {
        #[arg(long)]
        tx_hex: String,
        #[arg(long, default_value_t = 1)]
        partida: u32,
        /// Partida 2+: only the payment output (boleta already locked).
        #[arg(long, default_value_t = false)]
        partida_only: bool,
    },
    /// Same-machine MuSig2 close (loads both identities). Demo / local only.
    CoopClose {
        #[arg(long)]
        kind: String,
        #[arg(long)]
        outpoint: String,
        #[arg(long)]
        sats: u64,
        #[arg(long)]
        dest: String,
        #[arg(long, default_value_t = 200)]
        fee: u64,
        #[arg(long)]
        partida: Option<u32>,
        /// Directory of the other party (`identity.json` + nonce journal).
        #[arg(long)]
        peer_dir: PathBuf,
        /// Cooperative refund (partida → mandante) instead of payment to contratista.
        #[arg(long, default_value_t = false)]
        refund: bool,
    },
    /// Build+sign a script-path unwind (after T). Mandante: partida. Contratista: boleta.
    Unwind {
        #[arg(long)]
        kind: String,
        #[arg(long)]
        outpoint: String,
        #[arg(long)]
        sats: u64,
        #[arg(long)]
        dest: String,
        #[arg(long, default_value_t = 200)]
        fee: u64,
        #[arg(long)]
        partida: Option<u32>,
        /// Optional other party's dir, to copy state.json after the unwind.
        #[arg(long)]
        peer_dir: Option<PathBuf>,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let store = Store::new(cli.dir);
    match cli.cmd {
        Cmd::Init { network, role } => cmd_init(&store, &network, role.as_deref()),
        Cmd::New {
            unit,
            bond_bps,
            t_project,
        } => cmd_new(&store, &unit, bond_bps, t_project),
        Cmd::AddPartida {
            desc,
            amount,
            plazo,
        } => cmd_add_partida(&store, desc, &amount, plazo),
        Cmd::Offer => cmd_offer(&store),
        Cmd::Accept { file } => cmd_accept(&store, file),
        Cmd::Commit { file } => cmd_commit(&store, file),
        Cmd::Import { file } => cmd_import(&store, file),
        Cmd::Quote {
            bond_sats,
            btc_price,
            fx_note,
        } => cmd_quote(&store, bond_sats, btc_price.as_deref(), &fx_note),
        Cmd::AcceptQuote { file } => cmd_accept_quote(&store, file),
        Cmd::Addresses => cmd_addresses(&store),
        Cmd::Status => cmd_status(&store),
        Cmd::VerifyFunding {
            tx_hex,
            partida,
            partida_only,
        } => cmd_verify_funding(&store, &tx_hex, partida, partida_only),
        Cmd::CoopClose {
            kind,
            outpoint,
            sats,
            dest,
            fee,
            partida,
            peer_dir,
            refund,
        } => cmd_coop_close(
            &store, &kind, &outpoint, sats, &dest, fee, partida, peer_dir, refund,
        ),
        Cmd::Unwind {
            kind,
            outpoint,
            sats,
            dest,
            fee,
            partida,
            peer_dir,
        } => cmd_unwind(
            &store, &kind, &outpoint, sats, &dest, fee, partida, peer_dir,
        ),
    }
}

fn cmd_init(store: &Store, network: &str, role: Option<&str>) -> Result<()> {
    if store.identity_path().exists() {
        bail!(
            "identity already exists at {}",
            store.identity_path().display()
        );
    }
    let network = Network::from_str(network)?;
    let mut id = generate_identity(network)?;
    if let Some(r) = role {
        id.role = Some(Role::from_str(r)?);
    }
    store.save_identity(&id)?;
    println!("network    {}", network_name(network));
    println!("public_key {}", id.public_key);
    println!("stored     {}", store.identity_path().display());
    println!("warning    secret is plaintext; toy keys only");
    Ok(())
}

fn cmd_new(store: &Store, unit: &str, bond_bps: u16, t_project: u32) -> Result<()> {
    let id = store.load_identity()?;
    let body = ContractBody {
        network: id.network,
        unit: Unit::from_str(unit)?,
        bond_bps,
        t_project,
        partidas: vec![],
        mandante_pubkey: id.public_key,
        contratista_pubkey: None,
        arbiter_pubkey: None,
    };
    store.save_draft(&body)?;
    println!("draft {}", store.draft_path().display());
    Ok(())
}

fn cmd_add_partida(store: &Store, desc: String, amount: &str, plazo: u32) -> Result<()> {
    let mut body = store.load_draft()?;
    let id = body.partidas.last().map(|p| p.id + 1).unwrap_or(1);
    body.partidas.push(PartidaSpec {
        id,
        description: desc,
        amount_minor: minor_from_major(amount)?,
        plazo_unix: plazo,
    });
    if plazo > body.t_project {
        bail!("plazo {plazo} is after t_project {}", body.t_project);
    }
    store.save_draft(&body)?;
    let total = body.total_minor();
    let bond = bond_minor(total, body.bond_bps)?;
    println!("partida {id} added; total_minor={total} bond_minor={bond}");
    for w in bond_warnings(bond, total, body.partidas.last().unwrap().amount_minor) {
        println!("note: {w}");
    }
    Ok(())
}

fn cmd_offer(store: &Store) -> Result<()> {
    let id = store.load_identity()?;
    let body = store.load_draft()?;
    body.validate()?;
    if body.partidas.is_empty() {
        bail!("add at least one partida");
    }
    let sig = sign_body(&id.secret()?, &body)?;
    let offer = Offer {
        body,
        mandante_sig: sig,
    };
    let path = store.save_offer(&offer)?;
    println!("{}", path.display());
    Ok(())
}

fn cmd_accept(store: &Store, file: PathBuf) -> Result<()> {
    let id = store.load_identity()?;
    let offer: Offer = read_json(&file)?;
    verify_body(
        &offer.body.mandante_pubkey,
        &offer.mandante_sig,
        &offer.body,
    )?;
    let mut body = offer.body;
    body.contratista_pubkey = Some(id.public_key.clone());
    body.validate()?;
    let sig = sign_body(&id.secret()?, &body)?;
    let signed = SignedContract {
        body,
        mandante_sig: offer.mandante_sig,
        contratista_sig: sig,
    };
    // mandante_sig was over the offer body (no contratista key). Keep it as the
    // offer commitment; countersign (`commit`) binds the full body.
    let path = store.root.join("01-accepted.pending.json");
    store::write_json(&path, &signed)?;
    println!("{}", path.display());
    println!("pass this to the mandante for `hbp commit`");
    Ok(())
}

fn cmd_commit(store: &Store, file: PathBuf) -> Result<()> {
    let id = store.load_identity()?;
    let mut pending: SignedContract = read_json(&file)?;
    let offer: Offer = read_json(&store.root.join("00-offer.json"))
        .context("mandante needs the original 00-offer.json in --dir")?;
    if pending.body.terms() != offer.body.terms() {
        bail!("accepted terms do not match the offer");
    }
    if pending.body.mandante_pubkey != id.public_key {
        bail!("this identity is not the mandante");
    }
    verify_body(
        pending.body.contratista_pubkey.as_ref().unwrap(),
        &pending.contratista_sig,
        &pending.body,
    )?;
    pending.mandante_sig = sign_body(&id.secret()?, &pending.body)?;
    verify_body(
        &pending.body.mandante_pubkey,
        &pending.mandante_sig,
        &pending.body,
    )?;
    let path = store.save_signed(&pending)?;
    println!("contract {}", pending.id()?);
    println!("{}", path.display());
    Ok(())
}

fn cmd_quote(
    store: &Store,
    bond_sats: Option<u64>,
    btc_price: Option<&str>,
    fx_note: &str,
) -> Result<()> {
    let id = store.load_identity()?;
    let mut project = store.load_project()?;
    let body = &project.contract.body;
    let price_minor = btc_price.map(minor_from_major).transpose()?;
    let bond_sats = match (bond_sats, price_minor) {
        (Some(s), _) => s,
        (None, Some(price)) => {
            fiat_minor_to_sats(bond_minor(body.total_minor(), body.bond_bps)?, price)?
        }
        _ => bail!("provide --bond-sats or --btc-price"),
    };
    let mut partidas = Vec::new();
    for p in &body.partidas {
        let sats = match price_minor {
            Some(price) => fiat_minor_to_sats(p.amount_minor, price)?,
            None => bail!("--btc-price required to size partidas (or extend CLI later)"),
        };
        partidas.push(PartidaQuote { id: p.id, sats });
    }
    let mut quote = Quote {
        contract_id: project.contract.id()?,
        bond_sats,
        partidas,
        fx_note: fx_note.to_string(),
        quoted_at_unix: now_unix(),
        mandante_sig: None,
        contratista_sig: None,
    };
    let sig = sign_quote(&id, &quote)?;
    match party_role(&id, &project.contract.body)? {
        Role::Mandante => quote.mandante_sig = Some(sig),
        Role::Contratista => quote.contratista_sig = Some(sig),
    }
    // If both already present after this signature... unlikely. Save partial.
    if quote.mandante_sig.is_some() && quote.contratista_sig.is_some() {
        project.set_quote(quote.clone())?;
        store.save_project(&project)?;
    }
    let path = store.save_quote(&quote)?;
    println!("{}", path.display());
    println!("bond_sats {bond_sats}");
    Ok(())
}

fn cmd_accept_quote(store: &Store, file: PathBuf) -> Result<()> {
    let id = store.load_identity()?;
    let mut quote: Quote = read_json(&file)?;
    let mut project = store.load_project()?;
    if quote.contract_id != project.contract.id()? {
        bail!("quote is for a different contract");
    }
    quote.validate_against(&project.contract.body)?;
    let role = party_role(&id, &project.contract.body)?;
    let already = match role {
        Role::Mandante => quote.mandante_sig.is_some(),
        Role::Contratista => quote.contratista_sig.is_some(),
    };
    if !already {
        let sig = sign_quote(&id, &quote)?;
        match role {
            Role::Mandante => quote.mandante_sig = Some(sig),
            Role::Contratista => quote.contratista_sig = Some(sig),
        }
    }
    if quote.mandante_sig.is_none() || quote.contratista_sig.is_none() {
        let path = store.save_quote(&quote)?;
        println!("partial quote {}", path.display());
        println!("pass this file to the other party for `hbp accept-quote`");
        return Ok(());
    }
    project.set_quote(quote.clone())?;
    store.save_project(&project)?;
    let path = store.save_quote(&quote)?;
    println!("quote locked {}", path.display());
    println!("other party: hbp accept-quote {}", path.display());
    Ok(())
}

fn load_project_quote(store: &Store, project: &mut hbp_core::Project) -> Result<Option<Quote>> {
    if let Some(q) = project.quote.clone() {
        return Ok(Some(q));
    }
    let id = project.contract.id()?;
    if let Some(q) = store.load_quote(&id)? {
        if q.mandante_sig.is_some() && q.contratista_sig.is_some() {
            project.set_quote(q.clone())?;
            store.save_project(project)?;
            return Ok(Some(q));
        }
        return Ok(Some(q));
    }
    Ok(None)
}

fn cmd_addresses(store: &Store) -> Result<()> {
    let mut project = store.load_project()?;
    let quote = load_project_quote(store, &mut project)?;
    let body = &project.contract.body;
    let bond = bond_address(body)?;
    println!("bond {}", bond);
    for p in &body.partidas {
        let a = partida_address(body, p.id)?;
        println!("partida {} {}", p.id, a);
    }
    match quote {
        Some(q) if q.mandante_sig.is_some() && q.contratista_sig.is_some() => {
            println!("bond_sats {}", q.bond_sats);
            for p in &q.partidas {
                println!("partida {} sats {}", p.id, p.sats);
            }
        }
        Some(_) => println!("(quote not fully signed yet)"),
        None => println!("(no quote yet — amounts unknown)"),
    }
    Ok(())
}

fn cmd_status(store: &Store) -> Result<()> {
    let project = store.load_project()?;
    println!("{}", serde_json::to_string_pretty(&project)?);
    Ok(())
}

fn cmd_verify_funding(store: &Store, tx_hex: &str, partida: u32, partida_only: bool) -> Result<()> {
    let mut project = store.load_project()?;
    let quote = load_project_quote(store, &mut project)?
        .ok_or_else(|| anyhow::anyhow!("need a signed quote first"))?;
    if quote.mandante_sig.is_none() || quote.contratista_sig.is_none() {
        bail!("quote needs both signatures");
    }
    let body = &project.contract.body;
    let (m, c) = keys_from_body(body)?;
    let spec = body.partida(partida)?;
    let bond = bond_descriptor(&m, &c, body.t_project)?;
    let part = partida_descriptor(&m, &c, spec.plazo_unix)?;
    let raw = hex::decode(tx_hex.trim())?;
    let tx: bitcoin::Transaction = deserialize(&raw).context("tx hex")?;
    let txid = tx.compute_txid().to_string();
    let part_sats = quote.partida_sats(partida)?;
    let part_vout = tx
        .output
        .iter()
        .position(|o| o.script_pubkey == part.script_pubkey() && o.value.to_sat() == part_sats)
        .context("missing partida output with quoted amount")?;
    if partida_only {
        project.note_partida_funding(partida, txid.clone(), part_vout as u32, part_sats, 1, 1)?;
        store.save_project(&project)?;
        println!("partida {partida} funding ok; txid {txid} vout {part_vout}");
        return Ok(());
    }
    validate_funding_tx(
        &tx,
        &ExpectedFunding {
            bond_script: bond.script_pubkey(),
            bond_sats: quote.bond_sats,
            partida_script: part.script_pubkey(),
            partida_sats: part_sats,
            change: vec![],
            allow_other_outputs: true,
        },
    )?;
    let bond_vout = tx
        .output
        .iter()
        .position(|o| o.script_pubkey == bond.script_pubkey())
        .unwrap();
    project.note_bond_funding(txid.clone(), bond_vout as u32, quote.bond_sats, 1)?;
    project.note_partida_funding(partida, txid.clone(), part_vout as u32, part_sats, 1, 1)?;
    store.save_project(&project)?;
    println!("funding ok; txid {txid} bond_vout {bond_vout} partida_vout {part_vout}");
    Ok(())
}

fn cmd_coop_close(
    store: &Store,
    kind: &str,
    outpoint: &str,
    sats: u64,
    dest: &str,
    fee: u64,
    partida: Option<u32>,
    peer_dir: PathBuf,
    refund: bool,
) -> Result<()> {
    let us = store.load_identity()?;
    let peer_store = Store::new(peer_dir);
    let peer = peer_store.load_identity()?;
    let mut project = store.load_project()?;
    let body = &project.contract.body;
    let (m_pk, c_pk) = keys_from_body(body)?;
    let (m_sk, c_sk) = match party_role(&us, body)? {
        Role::Mandante => (us.secret()?, peer.secret()?),
        Role::Contratista => (peer.secret()?, us.secret()?),
    };
    let escrow = match kind {
        "partida" => {
            let pid = partida.context("--partida required")?;
            let spec = body.partida(pid)?;
            partida_descriptor(&m_pk, &c_pk, spec.plazo_unix)?
        }
        "bond" => bond_descriptor(&m_pk, &c_pk, body.t_project)?,
        other => bail!("kind must be partida|bond, got {other}"),
    };
    let dest = Address::from_str(dest)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .require_network(hbp_bitcoin::to_btc_network(body.network))?;
    let outpoint = OutPoint::from_str(outpoint).map_err(|e| anyhow::anyhow!("{e}"))?;
    let unsigned = build_key_spend_tx(
        outpoint,
        Amount::from_sat(sats),
        &dest,
        Amount::from_sat(fee),
    )?;
    let prev = TxOut {
        value: Amount::from_sat(sats),
        script_pubkey: escrow.script_pubkey(),
    };
    let sighash = key_spend_sighash(&unsigned, &prev)?;
    let mut j_us = store.load_nonces()?;
    let mut j_peer = peer_store.load_nonces()?;
    let (seed_m, seed_c) = match party_role(&us, body)? {
        Role::Mandante => (new_nonce_seed(&mut j_us)?, new_nonce_seed(&mut j_peer)?),
        Role::Contratista => (new_nonce_seed(&mut j_peer)?, new_nonce_seed(&mut j_us)?),
    };
    store.save_nonces(&j_us)?;
    peer_store.save_nonces(&j_peer)?;
    let sig = finish_coop_signature(
        &m_pk,
        &c_pk,
        &escrow,
        Some(&m_sk),
        Some(&c_sk),
        seed_m,
        seed_c,
        &sighash,
    )?;
    let signed = apply_key_spend_sig(unsigned, &sig);
    let hex = serialize_hex(&signed);
    let txid = signed.compute_txid().to_string();
    match (kind, refund) {
        ("partida", false) => {
            let pid = partida.unwrap();
            let _ = project.propose_reception(pid);
            project.mark_paid(pid, txid.clone())?;
        }
        ("partida", true) => {
            let pid = partida.unwrap();
            project.mark_partida_unwound(pid, txid.clone())?;
        }
        ("bond", false) => project.mark_bond_released(txid.clone())?,
        ("bond", true) => project.mark_bond_unwound(txid.clone())?,
        _ => {}
    }
    store.save_project(&project)?;
    if peer_store.root.join("CURRENT").exists() {
        let _ = peer_store.save_project(&project);
    }
    println!("{hex}");
    eprintln!("coop-close {kind} txid {txid}");
    Ok(())
}

fn cmd_unwind(
    store: &Store,
    kind: &str,
    outpoint: &str,
    sats: u64,
    dest: &str,
    fee: u64,
    partida: Option<u32>,
    peer_dir: Option<PathBuf>,
) -> Result<()> {
    let id = store.load_identity()?;
    let mut project = store.load_project()?;
    let body = &project.contract.body;
    let (m, c) = keys_from_body(body)?;
    let role = party_role(&id, body)?;
    let escrow = match kind {
        "partida" => {
            if role != Role::Mandante {
                bail!("only the mandante can unwind a partida");
            }
            let pid = partida.context("--partida required")?;
            let spec = body.partida(pid)?;
            partida_descriptor(&m, &c, spec.plazo_unix)?
        }
        "bond" => {
            if role != Role::Contratista {
                bail!("only the contratista can unwind the bond; timeout is not a bank boleta");
            }
            bond_descriptor(&m, &c, body.t_project)?
        }
        other => bail!("kind must be partida|bond, got {other}"),
    };
    let dest = Address::from_str(dest)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .require_network(hbp_bitcoin::to_btc_network(body.network))?;
    let outpoint = OutPoint::from_str(outpoint).map_err(|e| anyhow::anyhow!("{e}"))?;
    let tx = build_unwind_tx(
        &escrow,
        outpoint,
        Amount::from_sat(sats),
        &dest,
        Amount::from_sat(fee),
    )?;
    let prev = bitcoin::TxOut {
        value: Amount::from_sat(sats),
        script_pubkey: escrow.script_pubkey(),
    };
    let signed = sign_unwind(&escrow, tx, &prev, &id.secret()?)?;
    let hex = serialize_hex(&signed);
    let txid = signed.compute_txid().to_string();
    match kind {
        "partida" => {
            let pid = partida.unwrap();
            project.mark_partida_unwound(pid, txid.clone())?;
        }
        "bond" => project.mark_bond_unwound(txid.clone())?,
        _ => {}
    }
    store.save_project(&project)?;
    if let Some(peer) = peer_dir {
        let peer_store = Store::new(peer);
        if peer_store.root.join("CURRENT").exists() {
            let _ = peer_store.save_project(&project);
        }
    }
    println!("{hex}");
    eprintln!("unwind {kind} txid {txid} locktime {}", signed.lock_time);
    Ok(())
}

fn cmd_import(store: &Store, file: PathBuf) -> Result<()> {
    let signed: SignedContract = read_json(&file)?;
    verify_body(
        &signed.body.mandante_pubkey,
        &signed.mandante_sig,
        &signed.body,
    )?;
    verify_body(
        signed
            .body
            .contratista_pubkey
            .as_ref()
            .context("missing contratista key")?,
        &signed.contratista_sig,
        &signed.body,
    )?;
    let path = store.save_signed(&signed)?;
    println!("contract {}", signed.id()?);
    println!("{}", path.display());
    Ok(())
}

fn party_role(id: &Identity, body: &ContractBody) -> Result<Role> {
    if id.public_key == body.mandante_pubkey {
        Ok(Role::Mandante)
    } else if body.contratista_pubkey.as_ref() == Some(&id.public_key) {
        Ok(Role::Contratista)
    } else {
        bail!("this identity is not a party to the contract")
    }
}

fn sign_quote(id: &Identity, quote: &Quote) -> Result<String> {
    // Quotes are signed as canonical JSON of the unsigned subset.
    let unsigned = serde_json::json!({
        "contract_id": quote.contract_id,
        "bond_sats": quote.bond_sats,
        "partidas": quote.partidas,
        "fx_note": quote.fx_note,
        "quoted_at_unix": quote.quoted_at_unix,
    });
    let bytes = serde_json::to_vec(&unsigned)?;
    let secp = bitcoin::secp256k1::Secp256k1::new();
    let sk = id.secret()?;
    let keypair = bitcoin::key::Keypair::from_secret_key(&secp, &sk);
    use bitcoin::hashes::{sha256, Hash};
    let hash = sha256::Hash::hash(&bytes);
    let msg = bitcoin::secp256k1::Message::from_digest(hash.to_byte_array());
    let sig = secp.sign_schnorr_no_aux_rand(&msg, &keypair);
    Ok(hex::encode(sig.as_ref()))
}

fn now_unix() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as u32
}

fn network_name(n: Network) -> &'static str {
    match n {
        Network::Regtest => "regtest",
        Network::Signet => "signet",
        Network::Testnet => "testnet",
        Network::Bitcoin => "bitcoin",
    }
}
