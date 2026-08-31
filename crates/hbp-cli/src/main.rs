use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{bail, Context, Result};
use base64::Engine;
use bitcoin::consensus::encode::{deserialize, serialize_hex};
use bitcoin::{Address, Amount, OutPoint, TxOut};
use clap::{Parser, Subcommand};
use hbp_bitcoin::{
    apply_key_spend_sig, bond_address, bond_escrow_from_body, build_funding_psbt,
    build_key_spend_tx, build_script_path_tx, build_split_key_spend_tx, build_split_script_path_tx,
    build_unwind_tx, combine_partials, encode_partial, encode_pubnonce, finish_coop_signature,
    generate_identity, identity_from_secret, key_spend_sighash, keys_from_body, mad_address,
    mad_escrow_from_body, new_nonce_seed, our_partial_signature, parse_partial, parse_pubnonce,
    partida_address, partida_escrow_from_body, sign_arbiter, sign_arbiter_leaf, sign_body,
    sign_quote, sign_unwind, start_round, tweaked_key_agg, validate_funding_tx, verify_arbiter,
    verify_body, verify_quote, ArbiterWith, CoopFile, ExpectedFunding, FundingCoin, FundingRequest,
    Identity,
};
use hbp_core::{
    bond_minor, bond_warnings, fiat_minor_to_sats, minor_from_major, ArbiterNomination,
    ContractBody, DisputePolicy, Network, Offer, PartidaQuote, PartidaSpec, Quote, Role,
    SignedContract, Unit, DEFAULT_ARBITER_WINDOW_SECS,
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
    /// Unlock / encrypt identity.json. No minimum length (toy). Also HBP_PASSPHRASE.
    #[arg(long, global = true, env = "HBP_PASSPHRASE", hide_env_values = true)]
    passphrase: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create a local identity. Use --passphrase to encrypt identity.json.
    Init {
        #[arg(long, default_value = "regtest")]
        network: String,
        #[arg(long)]
        role: Option<String>,
        /// Restore from a 32-byte secret hex (paper backup). Never share this.
        #[arg(long)]
        secret: Option<String>,
    },
    /// Show the shareable half of this identity (compressed pubkey, not an xpub).
    Identity {
        /// Also print the secret. Paper backup only — not for the other party.
        #[arg(long, default_value_t = false)]
        backup: bool,
        /// Re-save identity.json encrypted with --passphrase / HBP_PASSPHRASE.
        #[arg(long, default_value_t = false)]
        encrypt: bool,
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
        /// unwind (default) | mad | arbiter — offeror proposes the *policy*;
        /// the person (arbiter pubkey) is named later by both, not in the offer.
        #[arg(long, default_value = "unwind")]
        dispute: String,
        /// With --dispute mad: bps of partida 1, each party (100 = 1%).
        #[arg(long)]
        mad_bps: Option<u16>,
        /// With --dispute arbiter: seconds after plazo before last-resort unwind (default 7 days).
        #[arg(long, default_value_t = DEFAULT_ARBITER_WINDOW_SECS)]
        arbiter_window: u32,
    },
    /// Either party proposes an arbiter pubkey (after accept; both must sign before funding).
    ProposeArbiter {
        #[arg(long)]
        pubkey: String,
    },
    /// Counterparty co-signs the same arbiter pubkey (or import a fully signed nomination).
    AcceptArbiter {
        file: PathBuf,
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
    /// Unsigned PSBT: escrow outputs exact; fee from change. Sign with Core/Sparrow.
    Fund {
        #[arg(long, default_value_t = 1)]
        partida: u32,
        #[arg(long, default_value_t = false)]
        partida_only: bool,
        #[arg(long, default_value_t = 2000)]
        fee: u64,
        #[arg(long)]
        m_outpoint: String,
        #[arg(long)]
        m_sats: u64,
        /// Address of the mandante coin being spent (witness UTXO).
        #[arg(long)]
        m_prev: String,
        #[arg(long)]
        m_change: String,
        #[arg(long)]
        c_outpoint: Option<String>,
        #[arg(long)]
        c_sats: Option<u64>,
        #[arg(long)]
        c_prev: Option<String>,
        #[arg(long)]
        c_change: Option<String>,
    },
    /// Start a file MuSig2 close (writes 04-coop.json with our pubnonce).
    CoopPropose {
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
        #[arg(long, default_value_t = false)]
        refund: bool,
        #[arg(long)]
        pay_sats: Option<u64>,
        #[arg(long)]
        refund_dest: Option<String>,
    },
    /// Counterparty: add pubnonce + partial signature to a coop file.
    CoopSign {
        file: PathBuf,
    },
    /// Originator: aggregate partials and print the signed tx hex.
    CoopFinish {
        file: PathBuf,
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
        /// Pay this many sats to `--dest` (e.g. 80% of the partida). Remainder minus fee goes to `--refund-dest`.
        #[arg(long)]
        pay_sats: Option<u64>,
        #[arg(long)]
        refund_dest: Option<String>,
    },
    /// Script-path A+M or A+C (after T). `--dir` is the party; `--arbiter-dir` is A.
    ArbiterClose {
        #[arg(long)]
        kind: String,
        /// am = arbiter+mandante; ac = arbiter+contratista
        #[arg(long)]
        with: String,
        #[arg(long)]
        arbiter_dir: PathBuf,
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
        #[arg(long)]
        pay_sats: Option<u64>,
        #[arg(long)]
        refund_dest: Option<String>,
        #[arg(long)]
        peer_dir: Option<PathBuf>,
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
    let store = Store::with_passphrase(cli.dir, cli.passphrase);
    match cli.cmd {
        Cmd::Init {
            network,
            role,
            secret,
        } => cmd_init(&store, &network, role.as_deref(), secret.as_deref()),
        Cmd::Identity { backup, encrypt } => cmd_identity(&store, backup, encrypt),
        Cmd::New {
            unit,
            bond_bps,
            t_project,
            dispute,
            mad_bps,
            arbiter_window,
        } => cmd_new(
            &store,
            &unit,
            bond_bps,
            t_project,
            &dispute,
            mad_bps,
            arbiter_window,
        ),
        Cmd::ProposeArbiter { pubkey } => cmd_propose_arbiter(&store, &pubkey),
        Cmd::AcceptArbiter { file } => cmd_accept_arbiter(&store, file),
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
        Cmd::Fund {
            partida,
            partida_only,
            fee,
            m_outpoint,
            m_sats,
            m_prev,
            m_change,
            c_outpoint,
            c_sats,
            c_prev,
            c_change,
        } => cmd_fund(
            &store,
            partida,
            partida_only,
            fee,
            &m_outpoint,
            m_sats,
            &m_prev,
            &m_change,
            c_outpoint.as_deref(),
            c_sats,
            c_prev.as_deref(),
            c_change.as_deref(),
        ),
        Cmd::CoopPropose {
            kind,
            outpoint,
            sats,
            dest,
            fee,
            partida,
            refund,
            pay_sats,
            refund_dest,
        } => cmd_coop_propose(
            &store,
            &kind,
            &outpoint,
            sats,
            &dest,
            fee,
            partida,
            refund,
            pay_sats,
            refund_dest.as_deref(),
        ),
        Cmd::CoopSign { file } => cmd_coop_sign(&store, file),
        Cmd::CoopFinish { file } => cmd_coop_finish(&store, file),
        Cmd::CoopClose {
            kind,
            outpoint,
            sats,
            dest,
            fee,
            partida,
            peer_dir,
            refund,
            pay_sats,
            refund_dest,
        } => cmd_coop_close(
            &store,
            &kind,
            &outpoint,
            sats,
            &dest,
            fee,
            partida,
            peer_dir,
            refund,
            pay_sats,
            refund_dest.as_deref(),
        ),
        Cmd::ArbiterClose {
            kind,
            with,
            arbiter_dir,
            outpoint,
            sats,
            dest,
            fee,
            partida,
            pay_sats,
            refund_dest,
            peer_dir,
        } => cmd_arbiter_close(
            &store,
            &kind,
            &with,
            arbiter_dir,
            &outpoint,
            sats,
            &dest,
            fee,
            partida,
            pay_sats,
            refund_dest.as_deref(),
            peer_dir,
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

fn cmd_init(store: &Store, network: &str, role: Option<&str>, secret: Option<&str>) -> Result<()> {
    if store.identity_path().exists() {
        bail!(
            "identity already exists at {}",
            store.identity_path().display()
        );
    }
    let network = Network::from_str(network)?;
    let mut id = if let Some(s) = secret {
        identity_from_secret(network, s)?
    } else {
        generate_identity(network)?
    };
    if let Some(r) = role {
        id.role = Some(Role::from_str(r)?);
    }
    store.save_identity(&id)?;
    println!("network    {}", network_name(network));
    println!("public_key {}", id.public_key);
    println!("stored     {}", store.identity_path().display());
    println!(
        "share      public_key only (the offer already carries it). not an xpub; one key, not HD"
    );
    if store.has_passphrase() {
        println!("encrypted  identity.json (passphrase; no strength check — toy)");
        println!(
            "backup     hbp --dir {} --passphrase … identity --backup",
            store.root.display()
        );
    } else {
        println!(
            "backup     hbp --dir {} identity --backup",
            store.root.display()
        );
        println!(
            "warning    secret is plaintext; encrypt with: hbp --passphrase … identity --encrypt"
        );
        println!("warning    never send identity.json to the other party");
    }
    Ok(())
}

fn cmd_identity(store: &Store, backup: bool, encrypt: bool) -> Result<()> {
    if encrypt {
        if !store.has_passphrase() {
            bail!("--encrypt needs --passphrase or HBP_PASSPHRASE");
        }
        let id = store.load_identity()?;
        store.save_identity(&id)?;
        println!("encrypted  {}", store.identity_path().display());
        if !backup {
            return Ok(());
        }
    }
    let id = store.load_identity()?;
    println!("network    {}", network_name(id.network));
    if let Some(r) = id.role {
        let role = match r {
            Role::Mandante => "mandante",
            Role::Contratista => "contratista",
        };
        println!("role       {role}");
    }
    println!("public_key {}", id.public_key);
    println!("share      this pubkey (or the offer file). equivalent of an xpub for a single key");
    if backup {
        println!("secret_key {}", id.secret_key);
        println!("warning    secret_key is YOUR half of the 2-of-2. the other party never sees it");
    }
    Ok(())
}

fn parse_dispute(kind: &str, mad_bps: Option<u16>, window: u32) -> Result<DisputePolicy> {
    match kind.to_ascii_lowercase().as_str() {
        "unwind" => Ok(DisputePolicy::Unwind),
        "mad" => {
            let mad_bps = mad_bps.context("--mad-bps required with --dispute mad")?;
            let d = DisputePolicy::Mad { mad_bps };
            d.validate()?;
            Ok(d)
        }
        "arbiter" => {
            let d = DisputePolicy::Arbiter {
                window_secs: window,
            };
            d.validate()?;
            Ok(d)
        }
        other => bail!("dispute must be unwind|mad|arbiter, got {other}"),
    }
}

fn cmd_new(
    store: &Store,
    unit: &str,
    bond_bps: u16,
    t_project: u32,
    dispute: &str,
    mad_bps: Option<u16>,
    arbiter_window: u32,
) -> Result<()> {
    let id = store.load_identity()?;
    let dispute = parse_dispute(dispute, mad_bps, arbiter_window)?;
    let body = ContractBody {
        network: id.network,
        unit: Unit::from_str(unit)?,
        bond_bps,
        t_project,
        partidas: vec![],
        mandante_pubkey: id.public_key,
        contratista_pubkey: None,
        dispute: dispute.clone(),
    };
    store.save_draft(&body)?;
    println!("draft {}", store.draft_path().display());
    println!("dispute {}", serde_json::to_string(&dispute)?);
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

fn cmd_propose_arbiter(store: &Store, pubkey: &str) -> Result<()> {
    let id = store.load_identity()?;
    let project = store.load_project()?;
    let pubkey = pubkey.trim().to_string();
    if let Some(existing) = project.named_arbiter_pubkey()? {
        if existing == pubkey {
            println!("arbiter already locked {existing}");
            return Ok(());
        }
        bail!("arbiter already locked to {existing}");
    }
    if project.funding_started() {
        bail!("too late to name an arbiter: UTXOs already funded");
    }
    let cid = project.contract.id()?;
    let mut nom = ArbiterNomination {
        contract_id: cid.clone(),
        pubkey,
        mandante_sig: None,
        contratista_sig: None,
    };
    nom.validate_against(&project.contract.body)?;
    let sig = sign_arbiter(&id.secret()?, &cid, &nom.pubkey)?;
    match party_role(&id, &project.contract.body)? {
        Role::Mandante => nom.mandante_sig = Some(sig),
        Role::Contratista => nom.contratista_sig = Some(sig),
    }
    let path = store.save_arbiter(&nom)?;
    println!("{}", path.display());
    println!(
        "pass to the other party: hbp accept-arbiter {}",
        path.display()
    );
    Ok(())
}

fn cmd_accept_arbiter(store: &Store, file: PathBuf) -> Result<()> {
    let id = store.load_identity()?;
    let mut project = store.load_project()?;
    let mut nom: ArbiterNomination = read_json(&file)?;
    if nom.contract_id != project.contract.id()? {
        bail!("arbiter nomination is for a different contract");
    }
    nom.validate_against(&project.contract.body)?;
    if let Some(existing) = project.named_arbiter_pubkey()? {
        if existing != nom.pubkey {
            bail!("arbiter already locked to {existing}");
        }
    } else if project.funding_started() {
        bail!("too late to name an arbiter: UTXOs already funded");
    }
    let cid = project.contract.id()?;
    let role = party_role(&id, &project.contract.body)?;
    let already = match role {
        Role::Mandante => nom.mandante_sig.is_some(),
        Role::Contratista => nom.contratista_sig.is_some(),
    };
    if !already {
        let sig = sign_arbiter(&id.secret()?, &cid, &nom.pubkey)?;
        match role {
            Role::Mandante => nom.mandante_sig = Some(sig),
            Role::Contratista => nom.contratista_sig = Some(sig),
        }
    }
    if let Some(s) = &nom.mandante_sig {
        verify_arbiter(&project.contract.body.mandante_pubkey, s, &cid, &nom.pubkey)?;
    }
    if let (Some(s), Some(cpk)) = (
        &nom.contratista_sig,
        &project.contract.body.contratista_pubkey,
    ) {
        verify_arbiter(cpk, s, &cid, &nom.pubkey)?;
    }
    if nom.fully_signed() {
        project.set_arbiter(nom.clone())?;
        store.save_project(&project)?;
    }
    let path = store.save_arbiter(&nom)?;
    if nom.fully_signed() {
        println!("arbiter locked {}", nom.pubkey);
        println!("{}", path.display());
        println!(
            "other party: hbp accept-arbiter {}  (import)",
            path.display()
        );
    } else {
        println!("partial nomination {}", path.display());
    }
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
    let mad_sats = match &body.dispute {
        hbp_core::DisputePolicy::Mad { mad_bps } => {
            let p1 = partidas
                .iter()
                .find(|p| p.id == body.partidas[0].id)
                .map(|p| p.sats)
                .unwrap_or(0);
            Some(
                p1.checked_mul(u64::from(*mad_bps))
                    .and_then(|v| v.checked_div(10_000))
                    .context("mad_sats overflow")?,
            )
        }
        _ => None,
    };
    let mut quote = Quote {
        contract_id: project.contract.id()?,
        bond_sats,
        partidas,
        fx_note: fx_note.to_string(),
        quoted_at_unix: now_unix(),
        mandante_sig: None,
        contratista_sig: None,
        mad_sats,
    };
    let sig = sign_quote(&id.secret()?, &quote)?;
    match party_role(&id, &project.contract.body)? {
        Role::Mandante => quote.mandante_sig = Some(sig),
        Role::Contratista => quote.contratista_sig = Some(sig),
    }
    verify_present_quote_sigs(&project.contract.body, &quote)?;
    if quote.mandante_sig.is_some() && quote.contratista_sig.is_some() {
        project.set_quote(quote.clone())?;
        store.save_project(&project)?;
    }
    let path = store.save_quote(&quote)?;
    println!("{}", path.display());
    println!("bond_sats {bond_sats}");
    if let Some(m) = mad_sats {
        println!(
            "mad_sats_each {m} (on-chain output {})",
            m.saturating_mul(2)
        );
    }
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
        let sig = sign_quote(&id.secret()?, &quote)?;
        match role {
            Role::Mandante => quote.mandante_sig = Some(sig),
            Role::Contratista => quote.contratista_sig = Some(sig),
        }
    }
    verify_present_quote_sigs(&project.contract.body, &quote)?;
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
            verify_present_quote_sigs(&project.contract.body, &q)?;
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
    println!("dispute {}", serde_json::to_string(&body.dispute)?);
    println!("mandante_pubkey {}", body.mandante_pubkey);
    if let Some(c) = &body.contratista_pubkey {
        println!("contratista_pubkey {c}");
    }
    let named = project.named_arbiter_pubkey()?;
    if matches!(body.dispute, DisputePolicy::Arbiter { .. }) && named.is_none() {
        println!(
            "arbiter (unnamed — both must hbp propose-arbiter / accept-arbiter before funding)"
        );
        return Ok(());
    }
    if let Some(a) = named {
        println!("arbiter {a}");
    }
    let bond = bond_address(body, named)?;
    println!("bond {}", bond);
    for p in &body.partidas {
        let a = partida_address(body, p.id, named)?;
        println!("partida {} {}", p.id, a);
    }
    if matches!(body.dispute, DisputePolicy::Mad { .. }) {
        println!("mad {}", mad_address(body)?);
    }
    match quote {
        Some(q) if q.mandante_sig.is_some() && q.contratista_sig.is_some() => {
            println!("bond_sats {}", q.bond_sats);
            if let Some(m) = q.mad_sats {
                println!("mad_sats_each {m}");
            }
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
    let named = project.named_arbiter_pubkey()?;
    let _ = keys_from_body(body)?;
    let bond = bond_escrow_from_body(body, named)?;
    let part = partida_escrow_from_body(body, partida, named)?;
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
    if let (DisputePolicy::Mad { .. }, Some(each)) = (&body.dispute, quote.mad_sats) {
        let mad = mad_escrow_from_body(body)?;
        let want = each.saturating_mul(2);
        tx.output
            .iter()
            .find(|o| o.script_pubkey == mad.script_pubkey() && o.value.to_sat() == want)
            .context("missing MAD output with 2*mad_sats")?;
    }
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

fn cmd_fund(
    store: &Store,
    partida: u32,
    partida_only: bool,
    fee: u64,
    m_outpoint: &str,
    m_sats: u64,
    m_prev: &str,
    m_change: &str,
    c_outpoint: Option<&str>,
    c_sats: Option<u64>,
    c_prev: Option<&str>,
    c_change: Option<&str>,
) -> Result<()> {
    let mut project = store.load_project()?;
    let quote = load_project_quote(store, &mut project)?
        .ok_or_else(|| anyhow::anyhow!("need a signed quote first"))?;
    if quote.mandante_sig.is_none() || quote.contratista_sig.is_none() {
        bail!("quote needs both signatures");
    }
    let body = &project.contract.body;
    let named = project.named_arbiter_pubkey()?;
    let net = hbp_bitcoin::to_btc_network(body.network);
    let parse_addr = |s: &str| -> Result<Address> {
        Ok(Address::from_str(s)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .require_network(net)?)
    };
    let part = partida_escrow_from_body(body, partida, named)?;
    let part_sats = quote.partida_sats(partida)?;
    let bond = if partida_only {
        None
    } else {
        Some((
            bond_escrow_from_body(body, named)?.script_pubkey(),
            quote.bond_sats,
        ))
    };
    let mad = if !partida_only && matches!(body.dispute, DisputePolicy::Mad { .. }) {
        let each = quote
            .mad_sats
            .ok_or_else(|| anyhow::anyhow!("mad policy needs quote.mad_sats"))?;
        Some((
            mad_escrow_from_body(body)?.script_pubkey(),
            each.saturating_mul(2),
        ))
    } else {
        None
    };
    let contratista = if partida_only {
        None
    } else {
        Some(FundingCoin {
            outpoint: OutPoint::from_str(c_outpoint.context("--c-outpoint required")?)
                .map_err(|e| anyhow::anyhow!("{e}"))?,
            sats: c_sats.context("--c-sats required")?,
            script_pubkey: parse_addr(c_prev.context("--c-prev required")?)?.script_pubkey(),
        })
    };
    let req = FundingRequest {
        bond,
        partida: (part.script_pubkey(), part_sats),
        mad,
        fee,
        mandante: FundingCoin {
            outpoint: OutPoint::from_str(m_outpoint).map_err(|e| anyhow::anyhow!("{e}"))?,
            sats: m_sats,
            script_pubkey: parse_addr(m_prev)?.script_pubkey(),
        },
        mandante_change: parse_addr(m_change)?,
        contratista,
        contratista_change: c_change.map(parse_addr).transpose()?,
    };
    let psbt = build_funding_psbt(&req)?;
    if !partida_only {
        validate_funding_tx(
            &psbt.unsigned_tx,
            &ExpectedFunding {
                bond_script: req.bond.as_ref().unwrap().0.clone(),
                bond_sats: quote.bond_sats,
                partida_script: part.script_pubkey(),
                partida_sats: part_sats,
                change: vec![],
                allow_other_outputs: true,
            },
        )?;
    }
    eprintln!(
        "partida {partida} {} sats exact; fee {fee} from change (not escrow)",
        part_sats
    );
    println!(
        "{}",
        base64::engine::general_purpose::STANDARD.encode(psbt.serialize())
    );
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
    pay_sats: Option<u64>,
    refund_dest: Option<&str>,
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
            partida_escrow_from_body(body, pid, project.named_arbiter_pubkey()?)?
        }
        "bond" => bond_escrow_from_body(body, project.named_arbiter_pubkey()?)?,
        "mad" => mad_escrow_from_body(body)?,
        other => bail!("kind must be partida|bond|mad, got {other}"),
    };
    let net = hbp_bitcoin::to_btc_network(body.network);
    let dest = Address::from_str(dest)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .require_network(net)?;
    let outpoint = OutPoint::from_str(outpoint).map_err(|e| anyhow::anyhow!("{e}"))?;
    if refund && pay_sats.is_some() {
        bail!("use either --refund or --pay-sats, not both");
    }
    let unsigned = if let Some(pay) = pay_sats {
        let refund_addr = refund_dest.context("--refund-dest required with --pay-sats")?;
        let refund_addr = Address::from_str(refund_addr)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .require_network(net)?;
        let tx = build_split_key_spend_tx(
            outpoint,
            Amount::from_sat(sats),
            &dest,
            Amount::from_sat(pay),
            &refund_addr,
            Amount::from_sat(fee),
        )?;
        eprintln!(
            "split pay {pay} sats to {dest}; refund {} sats to {refund_addr}",
            sats - pay - fee
        );
        tx
    } else {
        build_key_spend_tx(
            outpoint,
            Amount::from_sat(sats),
            &dest,
            Amount::from_sat(fee),
        )?
    };
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
            if let Some(pay) = pay_sats {
                eprintln!("partida {pid} closed at {pay}/{sats} sats (agreed split)");
            }
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

fn coop_unsigned(
    store: &Store,
    kind: &str,
    outpoint: &str,
    sats: u64,
    dest: &str,
    fee: u64,
    partida: Option<u32>,
    refund: bool,
    pay_sats: Option<u64>,
    refund_dest: Option<&str>,
) -> Result<(
    hbp_core::Project,
    hbp_bitcoin::Escrow,
    bitcoin::secp256k1::PublicKey,
    bitcoin::secp256k1::PublicKey,
    bitcoin::Transaction,
    [u8; 32],
)> {
    let project = store.load_project()?;
    let body = &project.contract.body;
    let (m_pk, c_pk) = keys_from_body(body)?;
    let named = project.named_arbiter_pubkey()?;
    let escrow = match kind {
        "partida" => {
            let pid = partida.context("--partida required")?;
            partida_escrow_from_body(body, pid, named)?
        }
        "bond" => bond_escrow_from_body(body, named)?,
        "mad" => mad_escrow_from_body(body)?,
        other => bail!("kind must be partida|bond|mad, got {other}"),
    };
    let net = hbp_bitcoin::to_btc_network(body.network);
    let dest = Address::from_str(dest)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .require_network(net)?;
    let outpoint = OutPoint::from_str(outpoint).map_err(|e| anyhow::anyhow!("{e}"))?;
    if refund && pay_sats.is_some() {
        bail!("use either --refund or --pay-sats, not both");
    }
    let unsigned = if let Some(pay) = pay_sats {
        let refund_addr = refund_dest.context("--refund-dest required with --pay-sats")?;
        let refund_addr = Address::from_str(refund_addr)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .require_network(net)?;
        build_split_key_spend_tx(
            outpoint,
            Amount::from_sat(sats),
            &dest,
            Amount::from_sat(pay),
            &refund_addr,
            Amount::from_sat(fee),
        )?
    } else {
        build_key_spend_tx(
            outpoint,
            Amount::from_sat(sats),
            &dest,
            Amount::from_sat(fee),
        )?
    };
    let prev = TxOut {
        value: Amount::from_sat(sats),
        script_pubkey: escrow.script_pubkey(),
    };
    let sighash = key_spend_sighash(&unsigned, &prev)?;
    Ok((project, escrow, m_pk, c_pk, unsigned, sighash))
}

fn cmd_coop_propose(
    store: &Store,
    kind: &str,
    outpoint: &str,
    sats: u64,
    dest: &str,
    fee: u64,
    partida: Option<u32>,
    refund: bool,
    pay_sats: Option<u64>,
    refund_dest: Option<&str>,
) -> Result<()> {
    let id = store.load_identity()?;
    let (project, escrow, m_pk, c_pk, unsigned, sighash) = coop_unsigned(
        store,
        kind,
        outpoint,
        sats,
        dest,
        fee,
        partida,
        refund,
        pay_sats,
        refund_dest,
    )?;
    let role = party_role(&id, &project.contract.body)?;
    let idx = hbp_bitcoin::signer_index(role);
    let mut j = store.load_nonces()?;
    let seed = new_nonce_seed(&mut j)?;
    let sh = hex::encode(sighash);
    j.stash_pending(&sh, &seed);
    store.save_nonces(&j)?;
    let ctx = tweaked_key_agg(&escrow, &m_pk, &c_pk)?;
    let (_, pubn) = start_round(ctx, &id.secret()?, idx, seed, &sighash)?;
    let pn = encode_pubnonce(&pubn);
    let mut coop = CoopFile {
        contract_id: project.contract.id()?,
        kind: kind.to_string(),
        partida_id: partida,
        outpoint: outpoint.to_string(),
        sats,
        dest: dest.to_string(),
        fee,
        refund,
        pay_sats,
        refund_dest: refund_dest.map(|s| s.to_string()),
        tx_hex: serialize_hex(&unsigned),
        sighash: sh,
        mandante_pubnonce: None,
        contratista_pubnonce: None,
        mandante_partial: None,
        contratista_partial: None,
    };
    match role {
        Role::Mandante => coop.mandante_pubnonce = Some(pn),
        Role::Contratista => coop.contratista_pubnonce = Some(pn),
    }
    let path = store.root.join("04-coop.json");
    store::write_json(&path, &coop)?;
    println!("{}", path.display());
    eprintln!("pass to the other party: hbp coop-sign {}", path.display());
    Ok(())
}

fn cmd_coop_sign(store: &Store, file: PathBuf) -> Result<()> {
    let id = store.load_identity()?;
    let mut coop: CoopFile = read_json(&file)?;
    let (project, escrow, m_pk, c_pk, _tx, sighash) = coop_unsigned(
        store,
        &coop.kind,
        &coop.outpoint,
        coop.sats,
        &coop.dest,
        coop.fee,
        coop.partida_id,
        coop.refund,
        coop.pay_sats,
        coop.refund_dest.as_deref(),
    )?;
    if hex::encode(sighash) != coop.sighash {
        bail!("coop file sighash does not match rebuilt tx");
    }
    if project.contract.id()? != coop.contract_id {
        bail!("coop file is for a different contract");
    }
    let role = party_role(&id, &project.contract.body)?;
    let idx = hbp_bitcoin::signer_index(role);
    let peer_idx = 1 - idx;
    let peer_n = match role {
        Role::Mandante => coop.contratista_pubnonce.as_deref(),
        Role::Contratista => coop.mandante_pubnonce.as_deref(),
    }
    .ok_or_else(|| anyhow::anyhow!("peer pubnonce missing; they must coop-propose first"))?;
    let mut j = store.load_nonces()?;
    let seed = new_nonce_seed(&mut j)?;
    j.stash_pending(&coop.sighash, &seed);
    store.save_nonces(&j)?;
    let ctx = tweaked_key_agg(&escrow, &m_pk, &c_pk)?;
    let (_, pubn) = start_round(ctx, &id.secret()?, idx, seed, &sighash)?;
    let peer = parse_pubnonce(peer_n)?;
    let partial = our_partial_signature(
        &m_pk,
        &c_pk,
        &escrow,
        &id.secret()?,
        idx,
        seed,
        peer_idx,
        &peer,
        &sighash,
    )?;
    let pn = encode_pubnonce(&pubn);
    let ps = encode_partial(&partial);
    match role {
        Role::Mandante => {
            coop.mandante_pubnonce = Some(pn);
            coop.mandante_partial = Some(ps);
        }
        Role::Contratista => {
            coop.contratista_pubnonce = Some(pn);
            coop.contratista_partial = Some(ps);
        }
    }
    let path = store.root.join("04-coop.json");
    store::write_json(&path, &coop)?;
    println!("{}", path.display());
    eprintln!("pass back: hbp coop-finish {}", path.display());
    Ok(())
}

fn cmd_coop_finish(store: &Store, file: PathBuf) -> Result<()> {
    let id = store.load_identity()?;
    let coop: CoopFile = read_json(&file)?;
    let (mut project, escrow, m_pk, c_pk, unsigned, sighash) = coop_unsigned(
        store,
        &coop.kind,
        &coop.outpoint,
        coop.sats,
        &coop.dest,
        coop.fee,
        coop.partida_id,
        coop.refund,
        coop.pay_sats,
        coop.refund_dest.as_deref(),
    )?;
    if hex::encode(sighash) != coop.sighash {
        bail!("coop file sighash does not match rebuilt tx");
    }
    let role = party_role(&id, &project.contract.body)?;
    let idx = hbp_bitcoin::signer_index(role);
    let peer_idx = 1 - idx;
    let (peer_n, peer_p) = match role {
        Role::Mandante => (
            coop.contratista_pubnonce
                .as_deref()
                .context("missing C nonce")?,
            coop.contratista_partial
                .as_deref()
                .context("missing C partial")?,
        ),
        Role::Contratista => (
            coop.mandante_pubnonce
                .as_deref()
                .context("missing M nonce")?,
            coop.mandante_partial
                .as_deref()
                .context("missing M partial")?,
        ),
    };
    let mut j = store.load_nonces()?;
    let seed = j.peek_pending(&coop.sighash)?;
    let sig = combine_partials(
        &m_pk,
        &c_pk,
        &escrow,
        &id.secret()?,
        idx,
        seed,
        peer_idx,
        &parse_pubnonce(peer_n)?,
        parse_partial(peer_p)?,
        &sighash,
    )?;
    let _ = j.take_pending(&coop.sighash);
    store.save_nonces(&j)?;
    let signed = apply_key_spend_sig(unsigned, &sig);
    let hex = serialize_hex(&signed);
    let txid = signed.compute_txid().to_string();
    println!("{hex}");
    eprintln!("coop-finish {} txid {txid}", coop.kind);
    let state_err = (|| -> Result<()> {
        match (coop.kind.as_str(), coop.refund) {
            ("partida", false) => {
                let pid = coop.partida_id.unwrap();
                let _ = project.propose_reception(pid);
                project.mark_paid(pid, txid.clone())?;
            }
            ("partida", true) => {
                let pid = coop.partida_id.unwrap();
                project.mark_partida_unwound(pid, txid.clone())?;
            }
            ("bond", false) => project.mark_bond_released(txid.clone())?,
            ("bond", true) => project.mark_bond_unwound(txid.clone())?,
            _ => {}
        }
        store.save_project(&project)?;
        Ok(())
    })();
    if let Err(e) = state_err {
        eprintln!("signed tx printed; local state not updated ({e:#})");
    }
    Ok(())
}

fn cmd_arbiter_close(
    store: &Store,
    kind: &str,
    with: &str,
    arbiter_dir: PathBuf,
    outpoint: &str,
    sats: u64,
    dest: &str,
    fee: u64,
    partida: Option<u32>,
    pay_sats: Option<u64>,
    refund_dest: Option<&str>,
    peer_dir: Option<PathBuf>,
) -> Result<()> {
    let id = store.load_identity()?;
    let mut project = store.load_project()?;
    let named = project
        .named_arbiter_pubkey()?
        .ok_or_else(|| anyhow::anyhow!("arbiter not named yet"))?
        .to_string();
    let a_store = Store::new(arbiter_dir);
    let a_id = a_store.load_identity()?;
    if a_id.public_key != named {
        bail!(
            "arbiter-dir pubkey {} != nominated {}",
            a_id.public_key,
            named
        );
    }
    let role = party_role(&id, &project.contract.body)?;
    let with = match with.to_ascii_lowercase().as_str() {
        "am" | "a+m" | "mandante" => ArbiterWith::Mandante,
        "ac" | "a+c" | "contratista" => ArbiterWith::Contratista,
        other => bail!("--with must be am|ac, got {other}"),
    };
    match (with, role) {
        (ArbiterWith::Mandante, Role::Mandante) => {}
        (ArbiterWith::Contratista, Role::Contratista) => {}
        _ => bail!("--dir must be the party in --with (am → mandante, ac → contratista)"),
    }
    let named_ref = Some(named.as_str());
    let escrow = match kind {
        "partida" => {
            let pid = partida.context("--partida required")?;
            partida_escrow_from_body(&project.contract.body, pid, named_ref)?
        }
        "bond" => bond_escrow_from_body(&project.contract.body, named_ref)?,
        other => bail!("kind must be partida|bond, got {other}"),
    };
    let net = hbp_bitcoin::to_btc_network(project.contract.body.network);
    let dest_addr = Address::from_str(dest)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .require_network(net)?;
    let outpoint = OutPoint::from_str(outpoint).map_err(|e| anyhow::anyhow!("{e}"))?;
    let unsigned = if let Some(pay) = pay_sats {
        let refund_addr = refund_dest.context("--refund-dest required with --pay-sats")?;
        let refund_addr = Address::from_str(refund_addr)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .require_network(net)?;
        build_split_script_path_tx(
            escrow.dispute_locktime,
            outpoint,
            Amount::from_sat(sats),
            &dest_addr,
            Amount::from_sat(pay),
            &refund_addr,
            Amount::from_sat(fee),
        )?
    } else {
        build_script_path_tx(
            escrow.dispute_locktime,
            outpoint,
            Amount::from_sat(sats),
            &dest_addr,
            Amount::from_sat(fee),
        )?
    };
    let prev = TxOut {
        value: Amount::from_sat(sats),
        script_pubkey: escrow.script_pubkey(),
    };
    let signed = sign_arbiter_leaf(
        &escrow,
        with,
        unsigned,
        &prev,
        &a_id.secret()?,
        &id.secret()?,
    )?;
    let hex = serialize_hex(&signed);
    let txid = signed.compute_txid().to_string();
    match (kind, with) {
        ("partida", ArbiterWith::Contratista) if pay_sats.is_none() => {
            let pid = partida.unwrap();
            let _ = project.propose_reception(pid);
            let _ = project.mark_paid(pid, txid.clone());
        }
        ("partida", _) => {
            let pid = partida.unwrap();
            let _ = project.mark_partida_unwound(pid, txid.clone());
        }
        ("bond", ArbiterWith::Contratista) => {
            let _ = project.mark_bond_released(txid.clone());
        }
        ("bond", ArbiterWith::Mandante) => {
            let _ = project.mark_bond_unwound(txid.clone());
        }
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
    eprintln!(
        "arbiter-close {kind} {:?} txid {txid} locktime {}",
        with, signed.lock_time
    );
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
    let role = party_role(&id, body)?;
    let escrow = match kind {
        "partida" => {
            if role != Role::Mandante {
                bail!("only the mandante can unwind a partida");
            }
            let pid = partida.context("--partida required")?;
            partida_escrow_from_body(body, pid, project.named_arbiter_pubkey()?)?
        }
        "bond" => {
            if role != Role::Contratista {
                bail!("only the contratista can unwind the bond; timeout is not a bank boleta");
            }
            bond_escrow_from_body(body, project.named_arbiter_pubkey()?)?
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

fn verify_present_quote_sigs(body: &ContractBody, quote: &Quote) -> Result<()> {
    if let Some(s) = &quote.mandante_sig {
        verify_quote(&body.mandante_pubkey, s, quote)?;
    }
    if let (Some(s), Some(c)) = (&quote.contratista_sig, &body.contratista_pubkey) {
        verify_quote(c, s, quote)?;
    }
    Ok(())
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
