//! Partida-1 payment path: quote, fund bond+P1, coop-close. One partida at a time.

use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use bitcoin::consensus::{deserialize, encode::serialize_hex};
use bitcoin::psbt::Psbt;
use bitcoin::{Address, Amount, OutPoint, Transaction};
use hbp_bitcoin::{
    apply_key_spend_sig, attach_prev_tx, bond_address, bond_escrow_from_body, build_funding_psbt,
    build_key_spend_tx, build_partial_funding_psbt, combine_partials, combine_psbts,
    complete_partial_funding_psbt, encode_partial, encode_pubnonce, extract_signed_funding_tx,
    funding_share, key_spend_sighash, keys_from_body, new_nonce_seed, our_partial_signature,
    parse_partial, parse_pubnonce, partida_address, partida_escrow_from_body,
    psbt_signed_input_count, sign_quote, signer_index, start_round, to_btc_network,
    tweaked_key_agg, validate_funding_tx, verify_quote, CoopFile, ExpectedFunding, FundingRequest,
    Identity, OfferedCoin, WatchScan, WatchedUtxo,
};
use hbp_core::{
    bond_minor, btc_price_to_minor, fiat_minor_to_sats, format_major_amount, minor_from_major,
    BondStatus, NonceJournal, PartidaQuote, PartidaStatus, Project, ProjectStatus, Quote, Role,
    SignedContract, Unit, PRODUCT_NETWORK,
};
use serde::{Deserialize, Serialize};

pub const ART_COIN: &str = "05-coin.json";
pub const ART_PARTIAL: &str = "06-funding.partial.json";
pub const ART_PSBT: &str = "06-funding.unsigned.json";
pub const ART_SIGNED: &str = "07-signed-psbt.json";
pub const ART_ONESIG: &str = "07-onesig.psbt.json";
pub const ART_COOP: &str = "08-coop.json";
pub const ART_COOP_TX: &str = "09-coop-tx.json";
pub const ART_TX: &str = "09-funding-tx.json";

const FIRST_PARTIDA: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayStage {
    NeedContract,
    NeedQuote,
    NeedQuoteSig,
    FundBondP1,
    PartidaInCourse,
    UnlockNext,
    AllClosed,
}

/// Staged funding handshake (one primary action each).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FundHandshakeStep {
    NeedWatch,
    ScanCoins,
    PickCoin,
    SendPartial,
    WaitPeerComplete,
    CompleteIncoming,
    RetrySend,
    ExportAndSign,
    SendOneSig,
    WaitChain,
}

impl FundHandshakeStep {
    pub fn title_es(self) -> &'static str {
        match self {
            Self::NeedWatch => "Guarda tu billetera",
            Self::ScanCoins => "Buscar mi plata",
            Self::PickCoin => "Elige qué plata usar",
            Self::SendPartial => "Armar y enviar mi parte",
            Self::WaitPeerComplete => "Esperando al otro",
            Self::CompleteIncoming => "Completar con mi parte",
            Self::RetrySend => "Reenviar",
            Self::ExportAndSign => "Exportar para firmar",
            Self::SendOneSig => "Enviar lo firmado",
            Self::WaitChain => "Comprobar en la red",
        }
    }
}

/// PSBT export / import / reenviar stay on the main path only while still funding.
pub fn show_main_fund_ui(stage: PayStage) -> bool {
    matches!(stage, PayStage::FundBondP1)
}

pub const KIND_PARTIDA: &str = "partida";
pub const KIND_BOND: &str = "bond";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoopAction {
    Propose,
    WaitPeer,
    Sign,
    Finish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StopStep {
    #[default]
    Confirm,
    PayP1,
    DestBond,
    ReturnBond,
}

/// Forward-only progress. Never walk this backwards except "Empezar de nuevo".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FundMark {
    #[default]
    Start,
    CoinPicked,
    PartialReady,
    PartialSent,
    CompleteReady,
    CompleteSent,
    OneSigReady,
    OneSigSent,
    OneSigFromPeer,
}

impl FundMark {
    pub fn raise(&mut self, to: Self) {
        if to > *self {
            *self = to;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FundingSendKind {
    Partial,
    Complete,
    OneSig,
}

#[derive(Debug, Clone)]
pub struct FundView {
    pub has_watch: bool,
    pub has_scan: bool,
    pub has_our_coin: bool,
    pub inputs: Option<usize>,
    pub sigs: usize,
    pub we_own_partial_input: bool,
    pub mark: FundMark,
    pub send_failed: bool,
    pub pending_send: Option<FundingSendKind>,
    pub onesig_from_peer: bool,
}

/// Decide the single funding step. A later mark wins over stale UI flags.
pub fn fund_handshake_step(v: &FundView) -> FundHandshakeStep {
    if v.send_failed && v.pending_send.is_some() {
        return FundHandshakeStep::RetrySend;
    }
    if v.onesig_from_peer || v.mark >= FundMark::OneSigSent || v.awaiting_like() {
        return FundHandshakeStep::WaitChain;
    }
    if v.sigs >= 1 || v.mark == FundMark::OneSigReady {
        return FundHandshakeStep::SendOneSig;
    }
    let inputs = effective_inputs(v);
    if inputs.unwrap_or(0) >= 2 || v.mark >= FundMark::CompleteReady {
        return FundHandshakeStep::ExportAndSign;
    }
    if !v.has_watch && v.mark <= FundMark::CoinPicked && inputs.is_none() {
        return FundHandshakeStep::NeedWatch;
    }
    match inputs {
        Some(1) if v.we_own_partial_input || v.mark >= FundMark::PartialSent => {
            FundHandshakeStep::WaitPeerComplete
        }
        Some(1) if v.has_our_coin => FundHandshakeStep::CompleteIncoming,
        Some(1) => FundHandshakeStep::PickCoin,
        _ if v.has_our_coin => FundHandshakeStep::SendPartial,
        _ if v.has_scan => FundHandshakeStep::PickCoin,
        _ => FundHandshakeStep::ScanCoins,
    }
}

impl FundView {
    fn awaiting_like(&self) -> bool {
        self.mark >= FundMark::OneSigSent || self.onesig_from_peer
    }
}

fn effective_inputs(v: &FundView) -> Option<usize> {
    match v.mark {
        FundMark::CompleteReady
        | FundMark::CompleteSent
        | FundMark::OneSigReady
        | FundMark::OneSigSent
        | FundMark::OneSigFromPeer => Some(v.inputs.unwrap_or(2).max(2)),
        FundMark::PartialReady | FundMark::PartialSent => Some(v.inputs.unwrap_or(1).max(1)),
        _ => v.inputs,
    }
}

/// Keep the more advanced PSBT (more sigs, then more inputs). Never downgrade.
pub fn prefer_funding_psbt(current: &str, incoming: &str) -> String {
    let inc = incoming.trim();
    if inc.is_empty() {
        return current.to_string();
    }
    let cur = current.trim();
    if cur.is_empty() {
        return inc.to_string();
    }
    let a = classify_funding_psbt(cur).unwrap_or((0, 0));
    let b = classify_funding_psbt(inc).unwrap_or((0, 0));
    if (b.1, b.0) > (a.1, a.0) {
        inc.to_string()
    } else {
        cur.to_string()
    }
}

pub fn psbt_to_base64(psbt: &Psbt) -> String {
    STANDARD.encode(psbt.serialize())
}

pub fn psbt_display_text(raw: &str) -> Result<String> {
    Ok(psbt_to_base64(&parse_psbt(raw)?))
}

pub fn psbt_file_bytes(raw: &str) -> Result<Vec<u8>> {
    Ok(parse_psbt(raw)?.serialize())
}

pub fn parse_psbt_bytes(bytes: &[u8]) -> Result<Psbt> {
    if let Ok(p) = Psbt::deserialize(bytes) {
        return Ok(p);
    }
    let text = std::str::from_utf8(bytes).context("PSBT archivo (binario, hex o base64)")?;
    parse_psbt(text)
}

pub fn spanish_chain_status(txid: Option<&str>, confirmed: Option<bool>) -> String {
    match (txid, confirmed) {
        (Some(_), Some(true)) => "Confirmada en Signet.".into(),
        (Some(_), Some(false)) => "Vista en mempool (sin confirmar).".into(),
        (Some(_), None) => "Vista en Signet.".into(),
        _ => "Aún no aparece en Signet.".into(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayUiDraft {
    #[serde(default)]
    pub price_major: String,
    #[serde(default = "default_fee")]
    pub fee_sats: String,
    #[serde(default)]
    pub m_outpoint: String,
    #[serde(default)]
    pub m_sats: String,
    #[serde(default)]
    pub m_addr: String,
    #[serde(default)]
    pub m_change: String,
    #[serde(default)]
    pub c_outpoint: String,
    #[serde(default)]
    pub c_sats: String,
    #[serde(default)]
    pub c_addr: String,
    #[serde(default)]
    pub c_change: String,
    #[serde(default)]
    pub signed_psbt_a: String,
    #[serde(default)]
    pub signed_psbt_b: String,
    #[serde(default)]
    pub dest: String,
    #[serde(default)]
    pub unsigned_psbt_hex: String,
    #[serde(default)]
    pub funding_tx_hex: String,
    #[serde(default)]
    pub coop_paste: String,
    #[serde(default)]
    pub selected_outpoint: String,
    #[serde(default)]
    pub onesig_hex: String,
    #[serde(default)]
    pub awaiting_chain: bool,
    #[serde(default)]
    pub send_failed: bool,
    #[serde(default)]
    pub pending_send: Option<FundingSendKind>,
    #[serde(default)]
    pub fund_mark: FundMark,
    #[serde(default)]
    pub onesig_from_peer: bool,
    #[serde(default)]
    pub show_psbt_text: bool,
    #[serde(default)]
    pub stop_open: bool,
    #[serde(default)]
    pub stop_step: StopStep,
    #[serde(default)]
    pub bond_dest: String,
    /// Fully signed MuSig2 spend (P1 cobro or boleta return). Not a funding PSBT.
    #[serde(default)]
    pub coop_tx_hex: String,
    #[serde(default)]
    pub coop_tx_kind: String,
    #[serde(default)]
    pub coop_tx_published: bool,
}

fn default_fee() -> String {
    "250".into()
}

impl Default for PayUiDraft {
    fn default() -> Self {
        Self {
            price_major: String::new(),
            fee_sats: default_fee(),
            m_outpoint: String::new(),
            m_sats: String::new(),
            m_addr: String::new(),
            m_change: String::new(),
            c_outpoint: String::new(),
            c_sats: String::new(),
            c_addr: String::new(),
            c_change: String::new(),
            signed_psbt_a: String::new(),
            signed_psbt_b: String::new(),
            dest: String::new(),
            unsigned_psbt_hex: String::new(),
            funding_tx_hex: String::new(),
            coop_paste: String::new(),
            selected_outpoint: String::new(),
            onesig_hex: String::new(),
            awaiting_chain: false,
            send_failed: false,
            pending_send: None,
            fund_mark: FundMark::Start,
            onesig_from_peer: false,
            show_psbt_text: false,
            stop_open: false,
            stop_step: StopStep::Confirm,
            bond_dest: String::new(),
            coop_tx_hex: String::new(),
            coop_tx_kind: String::new(),
            coop_tx_published: false,
        }
    }
}

impl PayUiDraft {
    pub fn reset_funding_handshake(&mut self) {
        self.unsigned_psbt_hex.clear();
        self.onesig_hex.clear();
        self.funding_tx_hex.clear();
        self.awaiting_chain = false;
        self.send_failed = false;
        self.pending_send = None;
        self.fund_mark = FundMark::Start;
        self.onesig_from_peer = false;
        self.show_psbt_text = false;
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PayCoins {
    #[serde(default)]
    pub mandante: Option<OfferedCoin>,
    #[serde(default)]
    pub contratista: Option<OfferedCoin>,
}

pub fn party_role(id: &Identity, body: &hbp_core::ContractBody) -> Result<Role> {
    if id.public_key == body.mandante_pubkey {
        Ok(Role::Mandante)
    } else if body.contratista_pubkey.as_ref() == Some(&id.public_key) {
        Ok(Role::Contratista)
    } else {
        bail!("esta llave no es parte de este trato")
    }
}

pub fn verify_present_quote_sigs(body: &hbp_core::ContractBody, quote: &Quote) -> Result<()> {
    if let Some(s) = &quote.mandante_sig {
        verify_quote(&body.mandante_pubkey, s, quote)?;
    }
    if let (Some(s), Some(c)) = (&quote.contratista_sig, &body.contratista_pubkey) {
        verify_quote(c, s, quote)?;
    }
    Ok(())
}

pub fn quote_fully_signed(q: &Quote) -> bool {
    q.mandante_sig.is_some() && q.contratista_sig.is_some()
}

pub fn pay_stage(project: Option<&Project>, pending_quote: Option<&Quote>) -> PayStage {
    let Some(project) = project else {
        return PayStage::NeedContract;
    };
    if project.is_stopped() {
        return PayStage::AllClosed;
    }
    let locked = project
        .quote
        .as_ref()
        .or(pending_quote.filter(|q| quote_fully_signed(q)));
    if locked.is_none() {
        return if pending_quote.is_some() {
            PayStage::NeedQuoteSig
        } else {
            PayStage::NeedQuote
        };
    }
    let p1 = project.partida(FIRST_PARTIDA).ok();
    let p1_state = p1.map(|p| &p.state);
    if matches!(project.bond, hbp_core::BondStatus::Unfunded)
        || matches!(
            p1_state,
            Some(PartidaStatus::Scheduled | PartidaStatus::AmountAgreed { .. })
        )
    {
        return PayStage::FundBondP1;
    }
    match p1_state {
        Some(PartidaStatus::Funding { .. })
        | Some(PartidaStatus::Locked { .. })
        | Some(PartidaStatus::ReceptionProposed { .. })
        | Some(PartidaStatus::FeeBurnT1 { .. }) => PayStage::PartidaInCourse,
        Some(s) if p1.is_some_and(|p| p.is_terminal()) => {
            if project.active_partida_id().is_some() {
                let _ = s;
                PayStage::UnlockNext
            } else {
                PayStage::AllClosed
            }
        }
        _ => PayStage::FundBondP1,
    }
}

pub fn spanish_now(project: Option<&Project>, pending_quote: Option<&Quote>) -> String {
    match pay_stage(project, pending_quote) {
        PayStage::NeedContract => "Cierra el trato primero (Aceptar / confirmar).".into(),
        PayStage::NeedQuote => "Ahora: acordar la plata de la boleta y de la partida 1.".into(),
        PayStage::NeedQuoteSig => "Ahora: firmar la plata de la boleta y de la partida 1.".into(),
        PayStage::FundBondP1 => "Ahora: juntar la plata de la boleta y de la partida 1".into(),
        PayStage::PartidaInCourse => "Partida 1 en curso".into(),
        PayStage::UnlockNext => {
            let next = project.and_then(|p| p.active_partida_id()).unwrap_or(2);
            format!("Partida 1 cerrada. Ahora puedes ver la partida {next}.")
        }
        PayStage::AllClosed => {
            if project.is_some_and(|p| {
                matches!(p.status, ProjectStatus::Cancelled)
                    || matches!(p.bond, BondStatus::Released { .. })
            }) {
                "Obra detenida. La boleta volvió al contratista.".into()
            } else {
                "Todas las partidas de este trato están cerradas.".into()
            }
        }
    }
}

/// Contract-currency amount, e.g. `5000.00 CLP`.
pub fn format_obra_money(minor: u64, unit: Unit) -> String {
    format!("{} {unit}", format_major_amount(minor, unit))
}

/// Quoted FX is a locked snapshot, not a live ticker that replaces the partida list.
pub fn agreed_fx_line(quote: &Quote, unit: Unit) -> String {
    let pair = format!("{unit}/BTC");
    let note = quote.fx_note.trim();
    if note.is_empty() {
        format!("tipo de cambio acordado: {pair}")
    } else if note.contains('/') {
        format!("tipo de cambio acordado: {note}")
    } else {
        format!("tipo de cambio acordado: {pair} ({note})")
    }
}

/// Primary number stays in the contract unit; sats are optional small print.
pub fn obra_amount_pair(minor: u64, unit: Unit, sats: Option<u64>) -> (String, Option<String>) {
    let main = format_obra_money(minor, unit);
    if unit.is_bitcoin_denom() {
        return (main, None);
    }
    (main, sats.map(|s| format!("{s} sats")))
}

pub fn contract_bond_minor(body: &hbp_core::ContractBody) -> u64 {
    bond_minor(body.total_minor(), body.bond_bps).unwrap_or(0)
}

pub fn partida_spec_minor(body: &hbp_core::ContractBody, id: u32) -> Option<u64> {
    body.partidas
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.amount_minor)
}

/// Later partidas stay grey until the previous one is Paid / Unwound / FeeBurnT2.
pub fn partida_ui_enabled(project: &Project, id: u32) -> bool {
    if project.is_stopped() {
        return false;
    }
    project.active_partida_id() == Some(id)
}

pub fn can_open_stop_wizard(project: &Project) -> bool {
    project.bond_is_funded()
}

pub fn p1_blocks_bond_return(project: &Project) -> bool {
    project.has_open_onchain_partida()
}

pub fn coop_filename(kind: &str) -> &'static str {
    if kind == KIND_BOND {
        "08-coop-bond.json"
    } else {
        "08-coop.json"
    }
}

pub fn price_minor_from_major(major: &str) -> Result<u64> {
    Ok(minor_from_major(major.trim())?)
}

/// Manual major or FX quote → price in the **contract** unit. USD FX never applies to CLP.
pub fn quote_price_minor(
    contract_unit: Unit,
    manual_major: &str,
    fx: Option<&hbp_net::FxQuote>,
) -> Result<Option<u64>> {
    if contract_unit.is_bitcoin_denom() {
        return Ok(None);
    }
    let manual = manual_major.trim();
    if !manual.is_empty() {
        let major: f64 = manual
            .replace('_', "")
            .parse()
            .context("precio BTC (número)")?;
        hbp_net::require_plausible_pair(contract_unit, major)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        return Ok(Some(btc_price_to_minor(major, contract_unit)?));
    }
    if let Some(q) = fx {
        if q.unit != contract_unit {
            bail!(
                "el precio es {}/BTC; este trato es {}/BTC. Consulta de nuevo.",
                q.unit,
                contract_unit
            );
        }
        return Ok(Some(
            hbp_net::fx_price_minor(q, contract_unit).context("precio FX inválido")?,
        ));
    }
    Ok(None)
}

pub fn can_recotizar(project: &Project) -> bool {
    !project.funding_started()
}

pub fn preview_quote_sats(signed: &SignedContract, price_minor: Option<u64>) -> Result<(u64, u64)> {
    let body = &signed.body;
    let bond = bond_minor(body.total_minor(), body.bond_bps)?;
    let p1 = body
        .partidas
        .iter()
        .find(|p| p.id == FIRST_PARTIDA)
        .context("el trato no tiene partida 1")?;
    Ok((
        amount_to_sats(bond, body.unit, price_minor)?,
        amount_to_sats(p1.amount_minor, body.unit, price_minor)?,
    ))
}

fn amount_to_sats(amount_minor: u64, unit: Unit, price_minor: Option<u64>) -> Result<u64> {
    match unit {
        Unit::Sats => Ok(amount_minor),
        Unit::Btc => amount_minor
            .checked_mul(1_000_000)
            .context("desborde al pasar BTC a sats"),
        _ => {
            let price = price_minor.context("falta el precio de BTC (FX o manual)")?;
            Ok(fiat_minor_to_sats(amount_minor, price)?)
        }
    }
}

pub fn draft_quote(
    signed: &SignedContract,
    price_minor: Option<u64>,
    fx_note: &str,
) -> Result<Quote> {
    if signed.body.network != PRODUCT_NETWORK {
        bail!("Esta app solo cotiza Signet");
    }
    if matches!(signed.body.dispute, hbp_core::DisputePolicy::Mad { .. }) {
        bail!("MAD no entra en esta pantalla");
    }
    let body = &signed.body;
    if !body.unit.is_bitcoin_denom() {
        let price = price_minor.context("falta el precio BTC en la moneda de la obra")?;
        let major = price as f64 / body.unit.minor_per_major() as f64;
        hbp_net::require_plausible_pair(body.unit, major).map_err(|e| anyhow::anyhow!("{e}"))?;
    }
    let bond_sats = amount_to_sats(
        bond_minor(body.total_minor(), body.bond_bps)?,
        body.unit,
        price_minor,
    )?;
    let mut partidas = Vec::new();
    for p in &body.partidas {
        partidas.push(PartidaQuote {
            id: p.id,
            sats: amount_to_sats(p.amount_minor, body.unit, price_minor)?,
        });
    }
    Ok(Quote {
        contract_id: signed.id()?,
        bond_sats,
        partidas,
        fx_note: fx_note.to_string(),
        quoted_at_unix: now_unix(),
        mandante_sig: None,
        contratista_sig: None,
        mad_sats: None,
    })
}

pub fn sign_our_quote(id: &Identity, signed: &SignedContract, mut quote: Quote) -> Result<Quote> {
    if quote.contract_id != signed.id()? {
        bail!("la cotización es de otro trato");
    }
    quote.validate_against(&signed.body)?;
    let role = party_role(id, &signed.body)?;
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
    verify_present_quote_sigs(&signed.body, &quote)?;
    Ok(quote)
}

/// Merge an incoming quote. Amounts must match if we already have one.
pub fn apply_incoming_quote(local: Option<Quote>, incoming: Quote) -> Result<Quote> {
    let Some(local) = local else {
        return Ok(incoming);
    };
    if local.contract_id != incoming.contract_id
        || local.bond_sats != incoming.bond_sats
        || local.partidas != incoming.partidas
        || local.mad_sats != incoming.mad_sats
    {
        // Same numbers, keep whichever sigs exist.
        let same_amounts = local.contract_id == incoming.contract_id
            && local.bond_sats == incoming.bond_sats
            && local.partidas == incoming.partidas
            && local.mad_sats == incoming.mad_sats;
        if !same_amounts {
            bail!("llegó otra cotización con montos distintos; pónganse de acuerdo en uno");
        }
    }
    let mut out = local;
    if out.mandante_sig.is_none() {
        out.mandante_sig = incoming.mandante_sig;
    }
    if out.contratista_sig.is_none() {
        out.contratista_sig = incoming.contratista_sig;
    }
    Ok(out)
}

pub fn recotizar_if_unfunded(project: &mut Project) -> Result<()> {
    if !can_recotizar(project) {
        bail!("el fondeo ya empezó; no se puede recotizar");
    }
    project.clear_quote()?;
    Ok(())
}

pub fn lock_quote_if_ready(project: &mut Project, quote: &Quote) -> Result<bool> {
    if !quote_fully_signed(quote) {
        return Ok(false);
    }
    if project.quote.is_some() {
        return Ok(true);
    }
    verify_present_quote_sigs(&project.contract.body, quote)?;
    project.set_quote(quote.clone())?;
    Ok(true)
}

pub fn coin_from_fields(
    role: Role,
    outpoint: &str,
    sats: &str,
    address: &str,
    change: &str,
) -> Result<OfferedCoin> {
    let outpoint = normalize_outpoint(outpoint)?;
    let sats: u64 = sats
        .trim()
        .parse()
        .context("sats tiene que ser un número")?;
    if sats == 0 {
        bail!("sats tiene que ser > 0");
    }
    let address = address.trim().to_string();
    let change = change.trim().to_string();
    if address.is_empty() || change.is_empty() {
        bail!("pega dirección y cambio (Sparrow / Electrum Signet)");
    }
    let coin = OfferedCoin {
        role,
        outpoint,
        sats,
        address,
        change,
        prev_tx_hex: None,
    };
    let _ = coin.funding_coin(PRODUCT_NETWORK)?;
    let _ = coin.change_address(PRODUCT_NETWORK)?;
    Ok(coin)
}

fn normalize_outpoint(raw: &str) -> Result<String> {
    let s = raw.trim().replace(',', ":").replace([' ', '\t'], "");
    let op = OutPoint::from_str(&s).map_err(|e| anyhow::anyhow!("outpoint '{raw}': {e}"))?;
    Ok(op.to_string())
}

pub fn build_p1_funding_psbt(
    project: &Project,
    mandante: &OfferedCoin,
    contratista: &OfferedCoin,
    fee: u64,
) -> Result<Psbt> {
    if fee == 0 {
        bail!("la comisión tiene que ser > 0");
    }
    if project.active_partida_id() != Some(FIRST_PARTIDA) {
        bail!("solo se fondea la partida activa; ahora no es la 1");
    }
    let quote = project
        .quote
        .as_ref()
        .context("falta la cotización firmada por los dos")?;
    if !quote_fully_signed(quote) {
        bail!("la cotización necesita las dos firmas");
    }
    if mandante.role != Role::Mandante || contratista.role != Role::Contratista {
        bail!("las monedas no coinciden con mandante / contratista");
    }
    let body = &project.contract.body;
    if matches!(body.dispute, hbp_core::DisputePolicy::Mad { .. }) {
        bail!("MAD no entra en esta pantalla");
    }
    let named = project.named_arbiter_pubkey()?;
    let part = partida_escrow_from_body(body, FIRST_PARTIDA, named)?;
    let part_sats = quote.partida_sats(FIRST_PARTIDA)?;
    let bond = bond_escrow_from_body(body, named)?;
    let req = FundingRequest {
        bond: Some((bond.script_pubkey(), quote.bond_sats)),
        partida: (part.script_pubkey(), part_sats),
        mad: None,
        fee,
        mandante: mandante.funding_coin(body.network)?,
        mandante_change: mandante.change_address(body.network)?,
        contratista: Some(contratista.funding_coin(body.network)?),
        contratista_change: Some(contratista.change_address(body.network)?),
    };
    let mut psbt = build_funding_psbt(&req)?;
    if let Some(tx) = mandante.prev_tx()? {
        attach_prev_tx(&mut psbt, req.mandante.outpoint, tx)?;
    }
    if let (Some(c), Some(tx)) = (&req.contratista, contratista.prev_tx()?) {
        attach_prev_tx(&mut psbt, c.outpoint, tx)?;
    }
    validate_funding_tx(
        &psbt.unsigned_tx,
        &ExpectedFunding {
            bond_script: bond.script_pubkey(),
            bond_sats: quote.bond_sats,
            partida_script: part.script_pubkey(),
            partida_sats: part_sats,
            change: vec![],
            allow_other_outputs: true,
        },
    )?;
    Ok(psbt)
}

pub fn psbt_to_hex(psbt: &Psbt) -> String {
    hex::encode(psbt.serialize())
}

pub fn parse_psbt(raw: &str) -> Result<Psbt> {
    let s = raw.trim();
    if s.is_empty() {
        bail!("pega el PSBT (hex o base64)");
    }
    if s.chars().all(|c| c.is_ascii_hexdigit()) && s.len() % 2 == 0 {
        let bytes = hex::decode(s).context("PSBT hex")?;
        return Psbt::deserialize(&bytes).context("PSBT");
    }
    let bytes = STANDARD.decode(s).context("PSBT hex o base64")?;
    Psbt::deserialize(&bytes).context("PSBT")
}

pub fn combine_signed_funding(parts: &[&str]) -> Result<String> {
    let mut psbts = Vec::new();
    for p in parts {
        let t = p.trim();
        if t.is_empty() {
            continue;
        }
        psbts.push(parse_psbt(t)?);
    }
    if psbts.is_empty() {
        bail!("pega los PSBT firmados (cada Blue / Sparrow firma su input)");
    }
    let combined = combine_psbts(&psbts)?;
    let tx = extract_signed_funding_tx(combined)?;
    Ok(serialize_hex(&tx))
}

pub fn apply_verified_p1_funding(project: &mut Project, tx_hex: &str) -> Result<String> {
    if project.active_partida_id() != Some(FIRST_PARTIDA) {
        bail!("no se puede anotar este fondeo: la partida 1 no es la activa");
    }
    let quote = project
        .quote
        .as_ref()
        .context("falta la cotización firmada")?
        .clone();
    if !quote_fully_signed(&quote) {
        bail!("la cotización necesita las dos firmas");
    }
    let body = &project.contract.body;
    let named = project.named_arbiter_pubkey()?;
    let bond = bond_escrow_from_body(body, named)?;
    let part = partida_escrow_from_body(body, FIRST_PARTIDA, named)?;
    let raw = hex::decode(tx_hex.trim()).context("tx hex")?;
    let tx: Transaction = deserialize(&raw).context("tx hex")?;
    let txid = tx.compute_txid().to_string();
    let part_sats = quote.partida_sats(FIRST_PARTIDA)?;
    let part_vout = tx
        .output
        .iter()
        .position(|o| o.script_pubkey == part.script_pubkey() && o.value.to_sat() == part_sats)
        .context("falta la salida de partida 1 con el monto cotizado")?;
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
        .position(|o| {
            o.script_pubkey == bond.script_pubkey() && o.value.to_sat() == quote.bond_sats
        })
        .context("falta la salida de la boleta")?;
    match &project.bond {
        hbp_core::BondStatus::Unfunded => {
            project.note_bond_funding(txid.clone(), bond_vout as u32, quote.bond_sats, 1)?;
        }
        hbp_core::BondStatus::Funded { sats, .. } if *sats == quote.bond_sats => {
            // Already noted (Esplora poll / second verify). Treat as success.
        }
        hbp_core::BondStatus::Funded { sats, .. } => {
            bail!(
                "la boleta ya está fondeada con otro monto ({sats} ≠ {})",
                quote.bond_sats
            );
        }
        _ => return Ok(txid),
    }
    project.note_partida_funding(
        FIRST_PARTIDA,
        txid.clone(),
        part_vout as u32,
        part_sats,
        1,
        1,
    )?;
    Ok(txid)
}

pub fn escrow_addrs(project: &Project) -> Result<(String, String)> {
    let named = project.named_arbiter_pubkey()?;
    let bond = bond_address(&project.contract.body, named)?;
    let p1 = partida_address(&project.contract.body, FIRST_PARTIDA, named)?;
    if bond.to_string() == p1.to_string() {
        bail!("boleta y partida 1 no pueden ser la misma dirección");
    }
    Ok((bond.to_string(), p1.to_string()))
}

pub fn our_funding_need(project: &Project, role: Role, fee: u64) -> Result<u64> {
    let quote = project
        .quote
        .as_ref()
        .context("falta la cotización firmada")?;
    Ok(funding_share(
        quote.bond_sats,
        quote.partida_sats(FIRST_PARTIDA)?,
        fee,
        role,
    )?)
}

pub fn suggest_watched(scan: &WatchScan, need: u64) -> Option<&WatchedUtxo> {
    let mut ok: Vec<&WatchedUtxo> = scan.utxos.iter().filter(|u| u.sats >= need).collect();
    ok.sort_by_key(|u| (u.confirmed, u.sats));
    // Prefer confirmed, then smallest that still covers the share.
    ok.into_iter()
        .filter(|u| u.confirmed)
        .min_by_key(|u| u.sats)
        .or_else(|| {
            scan.utxos
                .iter()
                .filter(|u| u.sats >= need)
                .min_by_key(|u| u.sats)
        })
}

pub fn coin_from_watched(role: Role, utxo: &WatchedUtxo, change: &str) -> Result<OfferedCoin> {
    coin_from_fields(
        role,
        &utxo.outpoint,
        &utxo.sats.to_string(),
        &utxo.address,
        change,
    )
}

fn p1_escrow_pair(
    project: &Project,
) -> Result<((bitcoin::ScriptBuf, u64), (bitcoin::ScriptBuf, u64))> {
    let quote = project
        .quote
        .as_ref()
        .context("falta la cotización firmada")?;
    if !quote_fully_signed(quote) {
        bail!("la cotización necesita las dos firmas");
    }
    let named = project.named_arbiter_pubkey()?;
    let body = &project.contract.body;
    let bond = bond_escrow_from_body(body, named)?;
    let part = partida_escrow_from_body(body, FIRST_PARTIDA, named)?;
    if bond.script_pubkey() == part.script_pubkey() {
        bail!("boleta y partida 1 salieron iguales; eso es un error de protocolo");
    }
    Ok((
        (bond.script_pubkey(), quote.bond_sats),
        (part.script_pubkey(), quote.partida_sats(FIRST_PARTIDA)?),
    ))
}

pub fn build_our_partial(
    project: &Project,
    role: Role,
    coin: &OfferedCoin,
    fee: u64,
) -> Result<Psbt> {
    if project.active_partida_id() != Some(FIRST_PARTIDA) {
        bail!("solo se fondea la partida activa; ahora no es la 1");
    }
    if coin.role != role {
        bail!("esa moneda no es de tu rol");
    }
    let (bond, part) = p1_escrow_pair(project)?;
    let mut psbt = build_partial_funding_psbt(
        bond,
        part,
        fee,
        role,
        &coin.funding_coin(PRODUCT_NETWORK)?,
        &coin.change_address(PRODUCT_NETWORK)?,
    )?;
    if let Some(tx) = coin.prev_tx()? {
        attach_prev_tx(&mut psbt, coin.outpoint()?, tx)?;
    }
    Ok(psbt)
}

pub fn complete_incoming_partial(
    project: &Project,
    role: Role,
    coin: &OfferedCoin,
    fee: u64,
    incoming: &str,
) -> Result<Psbt> {
    if coin.role != role {
        bail!("esa moneda no es de tu rol");
    }
    let (bond, part) = p1_escrow_pair(project)?;
    let mut psbt = parse_psbt(incoming)?;
    complete_partial_funding_psbt(
        &mut psbt,
        bond.clone(),
        part.clone(),
        fee,
        role,
        &coin.funding_coin(PRODUCT_NETWORK)?,
        &coin.change_address(PRODUCT_NETWORK)?,
    )?;
    if let Some(tx) = coin.prev_tx()? {
        attach_prev_tx(&mut psbt, coin.outpoint()?, tx)?;
    }
    validate_funding_tx(
        &psbt.unsigned_tx,
        &ExpectedFunding {
            bond_script: bond.0,
            bond_sats: bond.1,
            partida_script: part.0,
            partida_sats: part.1,
            change: vec![],
            allow_other_outputs: true,
        },
    )?;
    Ok(psbt)
}

pub fn classify_funding_psbt(raw: &str) -> Result<(usize, usize)> {
    let psbt = parse_psbt(raw)?;
    Ok((psbt.unsigned_tx.input.len(), psbt_signed_input_count(&psbt)))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingWire {
    pub hex: String,
    #[serde(default)]
    pub role: Option<Role>,
    #[serde(default)]
    pub fee: Option<u64>,
}

pub fn funding_wire(hex: &str, role: Role, fee: u64) -> serde_json::Value {
    serde_json::to_value(FundingWire {
        hex: hex.to_string(),
        role: Some(role),
        fee: Some(fee),
    })
    .unwrap_or_else(|_| hex_artifact(hex))
}

pub fn funding_wire_hex(json: &serde_json::Value) -> Option<String> {
    if let Ok(w) = serde_json::from_value::<FundingWire>(json.clone()) {
        if !w.hex.trim().is_empty() {
            return Some(w.hex);
        }
    }
    hex_from_artifact(json)
}

pub fn parse_fee(raw: &str) -> Result<u64> {
    let fee: u64 = raw.trim().parse().context("comisión en sats")?;
    if fee == 0 {
        bail!("la comisión tiene que ser > 0");
    }
    Ok(fee)
}

fn coop_unsigned(
    project: &Project,
    dest: &str,
    fee: u64,
    kind: &str,
) -> Result<(
    hbp_bitcoin::Escrow,
    bitcoin::secp256k1::PublicKey,
    bitcoin::secp256k1::PublicKey,
    Transaction,
    [u8; 32],
    String,
    u64,
)> {
    let body = &project.contract.body;
    let (txid, vout, sats, escrow) = match kind {
        KIND_BOND => {
            let (txid, vout, sats) = project
                .bond_utxo()
                .context("la boleta aún no está fondeada")?;
            let named = project.named_arbiter_pubkey()?;
            let escrow = bond_escrow_from_body(body, named)?;
            (txid.to_string(), vout, sats, escrow)
        }
        KIND_PARTIDA => {
            let p1 = project.partida(FIRST_PARTIDA)?;
            let (txid, vout, sats) = p1
                .locked_utxo()
                .context("la partida 1 aún no está fondeada / locked")?;
            let named = project.named_arbiter_pubkey()?;
            let escrow = partida_escrow_from_body(body, FIRST_PARTIDA, named)?;
            (txid.to_string(), vout, sats, escrow)
        }
        other => bail!("cierre desconocido: {other}"),
    };
    let outpoint = format!("{txid}:{vout}");
    let (m_pk, c_pk) = keys_from_body(body)?;
    let net = to_btc_network(body.network);
    let dest = Address::from_str(dest.trim())
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .require_network(net)?;
    let op = OutPoint::from_str(&outpoint).map_err(|e| anyhow::anyhow!("{e}"))?;
    let unsigned = build_key_spend_tx(op, Amount::from_sat(sats), &dest, Amount::from_sat(fee))?;
    let prev = bitcoin::TxOut {
        value: Amount::from_sat(sats),
        script_pubkey: escrow.script_pubkey(),
    };
    let sighash = key_spend_sighash(&unsigned, &prev)?;
    Ok((escrow, m_pk, c_pk, unsigned, sighash, outpoint, sats))
}

fn session_seed(journal: &mut NonceJournal, sighash: &str) -> Result<[u8; 32]> {
    if let Some(seed) = journal.pending_seed(sighash) {
        return Ok(seed);
    }
    let seed = new_nonce_seed(journal)?;
    journal.stash_pending(sighash, &seed);
    Ok(seed)
}

fn our_nonce<'a>(coop: &'a CoopFile, role: Role) -> Option<&'a str> {
    match role {
        Role::Mandante => coop.mandante_pubnonce.as_deref(),
        Role::Contratista => coop.contratista_pubnonce.as_deref(),
    }
}

fn peer_nonce<'a>(coop: &'a CoopFile, role: Role) -> Option<&'a str> {
    match role {
        Role::Mandante => coop.contratista_pubnonce.as_deref(),
        Role::Contratista => coop.mandante_pubnonce.as_deref(),
    }
}

fn our_partial<'a>(coop: &'a CoopFile, role: Role) -> Option<&'a str> {
    match role {
        Role::Mandante => coop.mandante_partial.as_deref(),
        Role::Contratista => coop.contratista_partial.as_deref(),
    }
}

fn peer_partial<'a>(coop: &'a CoopFile, role: Role) -> Option<&'a str> {
    match role {
        Role::Mandante => coop.contratista_partial.as_deref(),
        Role::Contratista => coop.mandante_partial.as_deref(),
    }
}

fn set_our_nonce(coop: &mut CoopFile, role: Role, pn: String) {
    match role {
        Role::Mandante => coop.mandante_pubnonce = Some(pn),
        Role::Contratista => coop.contratista_pubnonce = Some(pn),
    }
}

fn set_our_partial(coop: &mut CoopFile, role: Role, ps: String) {
    match role {
        Role::Mandante => coop.mandante_partial = Some(ps),
        Role::Contratista => coop.contratista_partial = Some(ps),
    }
}

/// Keep both sides' nonces and partials. A later dest/fee (new sighash) replaces.
pub fn merge_coop_file(local: Option<CoopFile>, incoming: CoopFile) -> CoopFile {
    let Some(mut a) = local else {
        return incoming;
    };
    if a.contract_id != incoming.contract_id || a.kind != incoming.kind {
        return incoming;
    }
    if a.sighash != incoming.sighash {
        return incoming;
    }
    if a.mandante_pubnonce.is_none() {
        a.mandante_pubnonce = incoming.mandante_pubnonce;
    }
    if a.contratista_pubnonce.is_none() {
        a.contratista_pubnonce = incoming.contratista_pubnonce;
    }
    if a.mandante_partial.is_none() {
        a.mandante_partial = incoming.mandante_partial;
    }
    if a.contratista_partial.is_none() {
        a.contratista_partial = incoming.contratista_partial;
    }
    a
}

pub fn coop_action(coop: Option<&CoopFile>, role: Role) -> CoopAction {
    let Some(c) = coop else {
        return CoopAction::Propose;
    };
    if peer_partial(c, role).is_some() && peer_nonce(c, role).is_some() {
        return CoopAction::Finish;
    }
    if peer_nonce(c, role).is_some() && our_partial(c, role).is_none() {
        return CoopAction::Sign;
    }
    if our_nonce(c, role).is_some() && peer_nonce(c, role).is_none() {
        return CoopAction::WaitPeer;
    }
    if our_nonce(c, role).is_none() {
        return CoopAction::Propose;
    }
    CoopAction::WaitPeer
}

fn blank_coop(
    project: &Project,
    kind: &str,
    dest: &str,
    fee: u64,
    outpoint: String,
    sats: u64,
    tx_hex: String,
    sighash: String,
) -> Result<CoopFile> {
    Ok(CoopFile {
        contract_id: project.contract.id()?,
        kind: kind.to_string(),
        partida_id: if kind == KIND_PARTIDA {
            Some(FIRST_PARTIDA)
        } else {
            None
        },
        outpoint,
        sats,
        dest: dest.trim().to_string(),
        fee,
        refund: false,
        pay_sats: None,
        refund_dest: None,
        tx_hex,
        sighash,
        mandante_pubnonce: None,
        contratista_pubnonce: None,
        mandante_partial: None,
        contratista_partial: None,
    })
}

fn require_kind(coop: &CoopFile, kind: &str) -> Result<()> {
    if coop.kind != kind {
        bail!("este archivo es de {} , no de {kind}", coop.kind);
    }
    if kind == KIND_PARTIDA && coop.partida_id != Some(FIRST_PARTIDA) {
        bail!("esta ronda no es el cierre de la partida 1");
    }
    Ok(())
}

/// Add our nonce. If the peer already sent theirs, also add our partial.
/// Reuses the pending seed so a second click does not invalidate the peer.
pub fn coop_contribute(
    id: &Identity,
    project: &Project,
    dest: &str,
    fee: u64,
    journal: &mut NonceJournal,
    kind: &str,
    existing: Option<CoopFile>,
) -> Result<CoopFile> {
    let role = party_role(id, &project.contract.body)?;
    let dest_use = existing
        .as_ref()
        .map(|c| c.dest.clone())
        .filter(|d| !d.trim().is_empty())
        .unwrap_or_else(|| dest.trim().to_string());
    let fee_use = existing.as_ref().map(|c| c.fee).unwrap_or(fee);
    let (escrow, m_pk, c_pk, unsigned, sighash, outpoint, sats) =
        coop_unsigned(project, &dest_use, fee_use, kind)?;
    let sh = hex::encode(sighash);
    let mut coop = match existing {
        Some(c) if c.sighash == sh && c.kind == kind => c,
        Some(_) | None => blank_coop(
            project,
            kind,
            &dest_use,
            fee_use,
            outpoint,
            sats,
            serialize_hex(&unsigned),
            sh.clone(),
        )?,
    };
    if project.contract.id()? != coop.contract_id {
        bail!("el archivo de cierre es de otro trato");
    }
    require_kind(&coop, kind)?;
    let idx = signer_index(role);
    let seed = session_seed(journal, &sh)?;
    let ctx = tweaked_key_agg(&escrow, &m_pk, &c_pk)?;
    let (_, pubn) = start_round(ctx, &id.secret()?, idx, seed, &sighash)?;
    let pn = encode_pubnonce(&pubn);
    if let Some(prev) = our_nonce(&coop, role) {
        if prev != pn {
            bail!("tu parte ya estaba armada. No empieces de nuevo; espera al otro y continúa.");
        }
    }
    set_our_nonce(&mut coop, role, pn);
    if let Some(peer_n) = peer_nonce(&coop, role) {
        let peer_idx = 1 - idx;
        let partial = our_partial_signature(
            &m_pk,
            &c_pk,
            &escrow,
            &id.secret()?,
            idx,
            seed,
            peer_idx,
            &parse_pubnonce(peer_n)?,
            &sighash,
        )?;
        set_our_partial(&mut coop, role, encode_partial(&partial));
    }
    Ok(coop)
}

pub fn coop_propose(
    id: &Identity,
    project: &Project,
    dest: &str,
    fee: u64,
    journal: &mut NonceJournal,
) -> Result<CoopFile> {
    coop_contribute(id, project, dest, fee, journal, KIND_PARTIDA, None)
}

pub fn coop_propose_on(
    id: &Identity,
    project: &Project,
    dest: &str,
    fee: u64,
    journal: &mut NonceJournal,
    kind: &str,
    existing: Option<CoopFile>,
) -> Result<CoopFile> {
    coop_contribute(id, project, dest, fee, journal, kind, existing)
}

pub fn coop_sign(
    id: &Identity,
    project: &Project,
    coop: CoopFile,
    journal: &mut NonceJournal,
) -> Result<CoopFile> {
    let dest = coop.dest.clone();
    let fee = coop.fee;
    let kind = coop.kind.clone();
    coop_contribute(id, project, &dest, fee, journal, &kind, Some(coop))
}

pub fn coop_finish(
    id: &Identity,
    project: &mut Project,
    coop: &CoopFile,
    journal: &mut NonceJournal,
) -> Result<String> {
    let role = party_role(id, &project.contract.body)?;
    require_kind(coop, &coop.kind)?;
    if project.contract.id()? != coop.contract_id {
        bail!("el archivo de cierre es de otro trato");
    }
    let (escrow, m_pk, c_pk, unsigned, sighash, _, _) =
        coop_unsigned(project, &coop.dest, coop.fee, &coop.kind)?;
    if hex::encode(sighash) != coop.sighash {
        bail!("el sighash del cierre no coincide");
    }
    let idx = signer_index(role);
    let peer_idx = 1 - idx;
    let peer_n = peer_nonce(coop, role).context("falta la parte del otro")?;
    let peer_p = peer_partial(coop, role).context("falta la firma del otro")?;
    let seed = journal
        .pending_seed(&coop.sighash)
        .context("falta tu parte guardada; pulsa Proponer o Firmar primero")?;
    if let Some(ours) = our_nonce(coop, role) {
        let ctx = tweaked_key_agg(&escrow, &m_pk, &c_pk)?;
        let (_, pubn) = start_round(ctx, &id.secret()?, idx, seed, &sighash)?;
        if encode_pubnonce(&pubn) != ours {
            bail!(
                "tu parte no coincide con este archivo. No armes otra: usa el mismo envío del otro."
            );
        }
    }
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
    let signed = apply_key_spend_sig(unsigned, &sig);
    Ok(serialize_hex(&signed))
}

pub fn hex_artifact(hex: &str) -> serde_json::Value {
    serde_json::json!({ "hex": hex })
}

pub fn hex_from_artifact(json: &serde_json::Value) -> Option<String> {
    json.get("hex")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            json.get("base64")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
}

pub fn coop_tx_wire(hex: &str, kind: &str) -> serde_json::Value {
    serde_json::json!({
        "hex": hex.trim(),
        "kind": kind,
        "txid": txid_from_tx_hex(hex).unwrap_or_default(),
    })
}

pub fn coop_tx_kind_from_artifact(json: &serde_json::Value) -> Option<String> {
    json.get("kind")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| s == KIND_PARTIDA || s == KIND_BOND)
}

pub fn txid_from_tx_hex(hex: &str) -> Result<String> {
    let raw = hex::decode(hex.trim()).context("tx hex")?;
    let tx: Transaction = deserialize(&raw).context("tx hex")?;
    Ok(tx.compute_txid().to_string())
}

pub fn needs_coop_publish(draft: &PayUiDraft) -> bool {
    !draft.coop_tx_hex.trim().is_empty() && !draft.coop_tx_published
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagoCoopGate {
    ShowPublish,
    ShowRedoBond,
    ShowStopped,
}

/// What Pago must show after a coop finish. Testable without egui.
pub fn pago_coop_gate(
    project: Option<&Project>,
    draft: &PayUiDraft,
    bond_coop: Option<&CoopFile>,
) -> Option<PagoCoopGate> {
    if needs_coop_publish(draft) {
        return Some(PagoCoopGate::ShowPublish);
    }
    let Some(project) = project else {
        return None;
    };
    if draft.coop_tx_published && project.is_stopped() {
        return Some(PagoCoopGate::ShowStopped);
    }
    if can_redo_bond_return(project, draft, bond_coop.is_some()) {
        return Some(PagoCoopGate::ShowRedoBond);
    }
    None
}

pub fn can_redo_bond_return(project: &Project, draft: &PayUiDraft, has_bond_coop: bool) -> bool {
    if needs_coop_publish(draft) || draft.coop_tx_published {
        return false;
    }
    if p1_blocks_bond_return(project) {
        return false;
    }
    let spendable = project.bond_is_funded() || matches!(project.bond, BondStatus::Released { .. });
    if !spendable {
        return false;
    }
    project.is_stopped() || has_bond_coop
}

pub fn can_open_stop_or_redo(project: &Project, draft: &PayUiDraft) -> bool {
    project.bond_is_funded()
        || (matches!(project.bond, BondStatus::Released { .. }) && !draft.coop_tx_published)
}

pub fn looks_like_signed_coop_hex(hex: &str) -> bool {
    let Ok(raw) = hex::decode(hex.trim()) else {
        return false;
    };
    let Ok(tx) = deserialize::<Transaction>(&raw) else {
        return false;
    };
    tx.input.len() == 1 && tx.output.len() == 1
}

pub fn infer_coop_kind(project: &Project, draft: &PayUiDraft) -> String {
    if draft.coop_tx_kind == KIND_PARTIDA || draft.coop_tx_kind == KIND_BOND {
        return draft.coop_tx_kind.clone();
    }
    if p1_blocks_bond_return(project) {
        KIND_PARTIDA.into()
    } else {
        KIND_BOND.into()
    }
}

/// Pull a leftover signed coop tx back into the draft (old finish stored it in funding hex).
pub fn recover_coop_tx_into_draft(
    draft: &mut PayUiDraft,
    project: &Project,
    disk: Option<&(String, String)>,
) -> bool {
    if !draft.coop_tx_hex.trim().is_empty() {
        if draft.coop_tx_kind.trim().is_empty() {
            draft.coop_tx_kind = infer_coop_kind(project, draft);
        }
        return true;
    }
    if let Some((hex, kind)) = disk {
        if !hex.trim().is_empty() {
            draft.coop_tx_hex = hex.trim().to_string();
            draft.coop_tx_kind = if kind.trim().is_empty() {
                infer_coop_kind(project, draft)
            } else {
                kind.clone()
            };
            return true;
        }
    }
    if looks_like_signed_coop_hex(&draft.funding_tx_hex) {
        draft.coop_tx_hex = draft.funding_tx_hex.trim().to_string();
        draft.coop_tx_kind = infer_coop_kind(project, draft);
        return true;
    }
    false
}

pub fn stash_finished_coop_hex(draft: &mut PayUiDraft, hex: &str, kind: &str) {
    draft.coop_tx_hex = hex.trim().to_string();
    draft.coop_tx_kind = kind.to_string();
    draft.coop_tx_published = false;
}

pub fn parse_coop_outpoint(outpoint: &str) -> Result<(String, u32)> {
    let (txid, vout) = outpoint.split_once(':').context("outpoint")?;
    Ok((txid.to_string(), vout.parse().context("vout")?))
}

/// Undo a local mark_bond_released that happened before the tx hit Signet.
pub fn restore_unconfirmed_bond(project: &mut Project, coop: Option<&CoopFile>) -> Result<bool> {
    if project.bond_is_funded() {
        return Ok(false);
    }
    let Some(coop) = coop.filter(|c| c.kind == KIND_BOND && !c.outpoint.is_empty() && c.sats > 0)
    else {
        bail!("no hay datos para rehacer la devolución");
    };
    let (txid, vout) = parse_coop_outpoint(&coop.outpoint)?;
    project.bond = BondStatus::Funded {
        txid,
        vout,
        sats: coop.sats,
        confirmations: 1,
    };
    if matches!(
        project.status,
        ProjectStatus::Closed | ProjectStatus::Cancelled
    ) {
        project.status = ProjectStatus::Active;
    }
    Ok(true)
}

pub fn spanish_now_pay(
    project: Option<&Project>,
    pending_quote: Option<&Quote>,
    draft: &PayUiDraft,
    bond_coop: Option<&CoopFile>,
) -> String {
    match pago_coop_gate(project, draft, bond_coop) {
        Some(PagoCoopGate::ShowPublish) => "Firmado — falta publicar".into(),
        Some(PagoCoopGate::ShowRedoBond) => {
            "La devolución quedó a medias. Vuelve a hacerla.".into()
        }
        _ => spanish_now(project, pending_quote),
    }
}

/// Apply a finished coop spend on the peer who did not press Terminar.
pub fn apply_finished_coop_hex(project: &mut Project, hex: &str, kind: &str) -> Result<String> {
    let txid = txid_from_tx_hex(hex)?;
    match kind {
        KIND_PARTIDA => {
            if matches!(
                project.partida(FIRST_PARTIDA).map(|p| &p.state),
                Ok(PartidaStatus::Paid { .. })
            ) {
                return Ok(txid);
            }
            let _ = project.propose_reception(FIRST_PARTIDA);
            project.mark_paid(FIRST_PARTIDA, txid.clone())?;
        }
        KIND_BOND => {
            if matches!(project.bond, BondStatus::Released { .. }) {
                return Ok(txid);
            }
            project.mark_bond_released(txid.clone())?;
        }
        other => bail!("cierre desconocido: {other}"),
    }
    Ok(txid)
}

fn now_unix() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{contratista_accept, mandante_commit};
    use hbp_bitcoin::generate_identity;
    use hbp_core::{
        BondStatus, ContractBody, DisputePolicy, Network, Offer, PartidaSpec, SignedContract,
        DEFAULT_BOND_BPS,
    };

    fn pair_ids() -> (Identity, Identity) {
        let mut m = generate_identity(Network::Signet).unwrap();
        m.role = Some(Role::Mandante);
        let mut c = generate_identity(Network::Signet).unwrap();
        c.role = Some(Role::Contratista);
        (m, c)
    }

    fn signed_two_partidas(m: &Identity, c: &Identity) -> SignedContract {
        let draft = ContractBody {
            network: Network::Signet,
            unit: Unit::Usd,
            work_name: "casa2".into(),
            bond_bps: DEFAULT_BOND_BPS,
            t_project: 1_800_000_000,
            partidas: vec![
                PartidaSpec {
                    id: 1,
                    description: "Radier".into(),
                    amount_minor: 100_000,
                    plazo_unix: 1_700_000_000,
                },
                PartidaSpec {
                    id: 2,
                    description: "Muros".into(),
                    amount_minor: 100_000,
                    plazo_unix: 1_710_000_000,
                },
            ],
            mandante_pubkey: m.public_key.clone(),
            contratista_pubkey: None,
            dispute: DisputePolicy::fee_burn(1_700_000_000, 1_800_000_000),
        };
        let offer = Offer {
            mandante_sig: hbp_bitcoin::sign_body(&m.secret().unwrap(), &draft).unwrap(),
            body: draft,
        };
        let pending = contratista_accept(offer.clone(), c).unwrap();
        mandante_commit(&offer, pending, m).unwrap()
    }

    fn dummy_coin(role: Role, n: u8, sats: u64) -> OfferedCoin {
        let (_ms, mp, _cs, cp) = {
            use bitcoin::secp256k1::{rand::rngs::OsRng, PublicKey, Secp256k1, SecretKey};
            let secp = Secp256k1::new();
            let m_sk = SecretKey::new(&mut OsRng);
            let c_sk = SecretKey::new(&mut OsRng);
            (
                m_sk,
                PublicKey::from_secret_key(&secp, &m_sk),
                c_sk,
                PublicKey::from_secret_key(&secp, &c_sk),
            )
        };
        let esc = hbp_bitcoin::partida_descriptor(&mp, &cp, 1_700_000_000 + u32::from(n)).unwrap();
        let addr = bitcoin::Address::p2tr_tweaked(esc.output_key(), bitcoin::Network::Signet);
        use bitcoin::hashes::Hash;
        OfferedCoin {
            role,
            outpoint: format!("{}:{n}", bitcoin::Txid::from_byte_array([n; 32])),
            sats,
            address: addr.to_string(),
            change: addr.to_string(),
            prev_tx_hex: None,
        }
    }

    #[test]
    fn one_partida_at_a_time_ui_and_state() {
        let (m, c) = pair_ids();
        let signed = signed_two_partidas(&m, &c);
        let mut project = Project::from_signed(signed.clone()).unwrap();
        assert_eq!(
            spanish_now(Some(&project), None),
            "Ahora: acordar la plata de la boleta y de la partida 1."
        );
        assert!(partida_ui_enabled(&project, 1));
        assert!(!partida_ui_enabled(&project, 2));

        let q = draft_quote(&signed, Some(8_000_000), "test 80k USD/BTC").unwrap();
        let q = sign_our_quote(&m, &signed, q).unwrap();
        let q = sign_our_quote(&c, &signed, q).unwrap();
        assert!(lock_quote_if_ready(&mut project, &q).unwrap());
        let (boleta, p1addr) = escrow_addrs(&project).unwrap();
        assert_ne!(boleta, p1addr, "boleta and partida 1 must be distinct");
        assert_eq!(
            spanish_now(Some(&project), Some(&q)),
            "Ahora: juntar la plata de la boleta y de la partida 1"
        );
        assert_eq!(project.active_partida_id(), Some(1));

        project
            .note_bond_funding("bond".into(), 0, q.bond_sats, 1)
            .unwrap();
        let err = project
            .note_partida_funding(2, "p2".into(), 1, q.partida_sats(2).unwrap(), 1, 1)
            .unwrap_err();
        assert!(err.to_string().contains("partida 1"));
        project
            .note_partida_funding(1, "p1".into(), 1, q.partida_sats(1).unwrap(), 1, 1)
            .unwrap();
        assert_eq!(spanish_now(Some(&project), Some(&q)), "Partida 1 en curso");
        assert!(!partida_ui_enabled(&project, 2));

        project.mark_paid(1, "pay1".into()).unwrap();
        assert!(partida_ui_enabled(&project, 2));
        assert!(spanish_now(Some(&project), Some(&q)).contains("partida 2"));
        assert!(!show_main_fund_ui(pay_stage(Some(&project), Some(&q))));
    }

    #[test]
    fn both_must_sign_quote_before_lock() {
        let (m, c) = pair_ids();
        let signed = signed_two_partidas(&m, &c);
        let mut project = Project::from_signed(signed.clone()).unwrap();
        let q = draft_quote(&signed, Some(8_000_000), "fx").unwrap();
        let q = sign_our_quote(&m, &signed, q).unwrap();
        assert!(q.mandante_sig.is_some());
        assert!(q.contratista_sig.is_none());
        assert!(!lock_quote_if_ready(&mut project, &q).unwrap());
        let incoming = sign_our_quote(&c, &signed, q.clone()).unwrap();
        let merged = apply_incoming_quote(Some(q), incoming).unwrap();
        assert!(lock_quote_if_ready(&mut project, &merged).unwrap());
        assert!(project.quote.is_some());
    }

    #[test]
    fn fund_verify_bond_plus_p1_then_p2_still_blocked() {
        let (m, c) = pair_ids();
        let signed = signed_two_partidas(&m, &c);
        let mut project = Project::from_signed(signed.clone()).unwrap();
        let q = sign_our_quote(
            &c,
            &signed,
            sign_our_quote(
                &m,
                &signed,
                draft_quote(&signed, Some(8_000_000), "fx").unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        lock_quote_if_ready(&mut project, &q).unwrap();

        let m_coin = dummy_coin(Role::Mandante, 1, 5_000_000);
        let c_coin = dummy_coin(Role::Contratista, 2, 5_000_000);
        let psbt = build_p1_funding_psbt(&project, &m_coin, &c_coin, 400).unwrap();
        let tx_hex = serialize_hex(&psbt.unsigned_tx);
        let txid = apply_verified_p1_funding(&mut project, &tx_hex).unwrap();
        assert!(!txid.is_empty());
        assert!(matches!(project.bond, BondStatus::Funded { .. }));
        assert_eq!(spanish_now(Some(&project), Some(&q)), "Partida 1 en curso");
        let again = apply_verified_p1_funding(&mut project, &tx_hex).unwrap();
        assert_eq!(again, txid);
        assert!(!show_main_fund_ui(pay_stage(Some(&project), Some(&q))));
        assert!(!partida_ui_enabled(&project, 2));
        let err = project
            .note_partida_funding(2, "no".into(), 0, q.partida_sats(2).unwrap(), 1, 1)
            .unwrap_err();
        assert!(err.to_string().contains("partida 1") || err.to_string().contains("must be paid"));
    }

    #[test]
    fn preview_sats_at_80k() {
        let (m, c) = pair_ids();
        let signed = signed_two_partidas(&m, &c);
        let (bond, p1) = preview_quote_sats(&signed, Some(8_000_000)).unwrap();
        // 10% of $2000 = $200; $200 / $80k * 1e8 = 250_000 sats. P1 $1000 → 1_250_000.
        assert_eq!(bond, 250_000);
        assert_eq!(p1, 1_250_000);
    }

    fn signed_clp_partida(m: &Identity, c: &Identity, p1_minor: u64) -> SignedContract {
        let draft = ContractBody {
            network: Network::Signet,
            unit: Unit::Clp,
            work_name: "casa-clp".into(),
            bond_bps: DEFAULT_BOND_BPS,
            t_project: 1_800_000_000,
            partidas: vec![
                PartidaSpec {
                    id: 1,
                    description: "Radier".into(),
                    amount_minor: p1_minor,
                    plazo_unix: 1_700_000_000,
                },
                PartidaSpec {
                    id: 2,
                    description: "Muros".into(),
                    amount_minor: p1_minor,
                    plazo_unix: 1_710_000_000,
                },
            ],
            mandante_pubkey: m.public_key.clone(),
            contratista_pubkey: None,
            dispute: DisputePolicy::fee_burn(1_700_000_000, 1_800_000_000),
        };
        let offer = Offer {
            mandante_sig: hbp_bitcoin::sign_body(&m.secret().unwrap(), &draft).unwrap(),
            body: draft,
        };
        let pending = contratista_accept(offer.clone(), c).unwrap();
        mandante_commit(&offer, pending, m).unwrap()
    }

    #[test]
    fn clp_quote_uses_clp_btc_not_usd() {
        use hbp_core::{btc_price_to_minor, parse_major_amount};
        let (m, c) = pair_ids();
        let p1 = parse_major_amount("5000", Unit::Clp).unwrap();
        assert_eq!(p1, 500_000);
        let signed = signed_clp_partida(&m, &c, p1);
        let clp_price = btc_price_to_minor(74_492_748.0, Unit::Clp).unwrap();
        let (bond, p1_sats) = preview_quote_sats(&signed, Some(clp_price)).unwrap();
        assert_eq!(p1_sats, 6_712);
        // 10% boleta of 10_000 CLP total (two 5000 partidas) = 1000 CLP → 1_342 sats.
        assert_eq!(bond, 1_342);

        let usd_fx = hbp_net::FxQuote {
            unit: Unit::Usd,
            btc_price_major: 79_600.0,
            source: "test",
        };
        assert!(quote_price_minor(Unit::Clp, "", Some(&usd_fx)).is_err());
        let from_manual = quote_price_minor(Unit::Clp, "74492748", None)
            .unwrap()
            .unwrap();
        assert_eq!(from_manual, clp_price);
        assert!(quote_price_minor(Unit::Clp, "79600", None).is_err());

        let q = draft_quote(&signed, Some(clp_price), "Yadio 74492748 CLP/BTC").unwrap();
        assert_eq!(q.partida_sats(1).unwrap(), 6_712);
        let usd_price = btc_price_to_minor(79_600.0, Unit::Usd).unwrap();
        assert!(
            draft_quote(&signed, Some(usd_price), "wrong USD").is_err(),
            "USD/BTC must not quote a CLP contract"
        );
    }

    #[test]
    fn recotizar_clears_unfunded_quote() {
        let (m, c) = pair_ids();
        let signed = signed_two_partidas(&m, &c);
        let mut project = Project::from_signed(signed.clone()).unwrap();
        let q = sign_our_quote(
            &c,
            &signed,
            sign_our_quote(
                &m,
                &signed,
                draft_quote(&signed, Some(8_000_000), "fx").unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        lock_quote_if_ready(&mut project, &q).unwrap();
        assert!(can_recotizar(&project));
        recotizar_if_unfunded(&mut project).unwrap();
        assert!(project.quote.is_none());
        assert_eq!(
            spanish_now(Some(&project), None),
            "Ahora: acordar la plata de la boleta y de la partida 1."
        );
    }

    #[test]
    fn partial_psbt_handshake_then_p2_blocked() {
        let (m, c) = pair_ids();
        let signed = signed_two_partidas(&m, &c);
        let mut project = Project::from_signed(signed.clone()).unwrap();
        let q = sign_our_quote(
            &c,
            &signed,
            sign_our_quote(
                &m,
                &signed,
                draft_quote(&signed, Some(8_000_000), "fx").unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        lock_quote_if_ready(&mut project, &q).unwrap();
        let m_coin = dummy_coin(Role::Mandante, 1, 5_000_000);
        let c_coin = dummy_coin(Role::Contratista, 2, 5_000_000);
        let partial = build_our_partial(&project, Role::Mandante, &m_coin, 400).unwrap();
        assert_eq!(partial.unsigned_tx.input.len(), 1);
        let complete = complete_incoming_partial(
            &project,
            Role::Contratista,
            &c_coin,
            400,
            &psbt_to_hex(&partial),
        )
        .unwrap();
        assert_eq!(complete.unsigned_tx.input.len(), 2);
        let txid =
            apply_verified_p1_funding(&mut project, &serialize_hex(&complete.unsigned_tx)).unwrap();
        assert!(!txid.is_empty());
        assert!(!partida_ui_enabled(&project, 2));
    }

    fn view(
        has_watch: bool,
        has_scan: bool,
        has_our_coin: bool,
        inputs: Option<usize>,
        sigs: usize,
        we_own: bool,
        mark: FundMark,
        send_failed: bool,
        pending: Option<FundingSendKind>,
        from_peer: bool,
    ) -> FundView {
        FundView {
            has_watch,
            has_scan,
            has_our_coin,
            inputs,
            sigs,
            we_own_partial_input: we_own,
            mark,
            send_failed,
            pending_send: pending,
            onesig_from_peer: from_peer,
        }
    }

    #[test]
    fn fund_handshake_steps_are_sequential() {
        use FundHandshakeStep::*;
        use FundMark::*;
        assert_eq!(
            fund_handshake_step(&view(
                false, false, false, None, 0, false, Start, false, None, false
            )),
            NeedWatch
        );
        assert_eq!(
            fund_handshake_step(&view(
                true, false, false, None, 0, false, Start, false, None, false
            )),
            ScanCoins
        );
        assert_eq!(
            fund_handshake_step(&view(
                true, true, false, None, 0, false, Start, false, None, false
            )),
            PickCoin
        );
        assert_eq!(
            fund_handshake_step(&view(
                true, true, true, None, 0, false, CoinPicked, false, None, false
            )),
            SendPartial
        );
        assert_eq!(
            fund_handshake_step(&view(
                true,
                true,
                true,
                Some(1),
                0,
                true,
                PartialSent,
                false,
                None,
                false
            )),
            WaitPeerComplete
        );
        assert_eq!(
            fund_handshake_step(&view(
                true,
                true,
                true,
                Some(1),
                0,
                false,
                PartialReady,
                false,
                None,
                false
            )),
            CompleteIncoming
        );
        assert_eq!(
            fund_handshake_step(&view(
                true,
                true,
                true,
                Some(2),
                0,
                false,
                CompleteReady,
                false,
                None,
                false
            )),
            ExportAndSign
        );
        assert_eq!(
            fund_handshake_step(&view(
                true,
                true,
                true,
                Some(2),
                1,
                false,
                OneSigReady,
                false,
                None,
                false
            )),
            SendOneSig
        );
        assert_eq!(
            fund_handshake_step(&view(
                true,
                true,
                true,
                Some(2),
                1,
                false,
                OneSigSent,
                false,
                None,
                false
            )),
            WaitChain
        );
        assert_eq!(
            fund_handshake_step(&view(
                true,
                true,
                true,
                Some(2),
                1,
                false,
                CompleteReady,
                false,
                None,
                true
            )),
            WaitChain
        );
    }

    #[test]
    fn complete_mark_locks_out_armar_and_picker() {
        use FundHandshakeStep::*;
        use FundMark::*;
        // Stale 1-in hex or missing classify must not reopen "Armar parcial".
        assert_eq!(
            fund_handshake_step(&view(
                true,
                true,
                true,
                Some(1),
                0,
                false,
                CompleteReady,
                false,
                None,
                false
            )),
            ExportAndSign
        );
        assert_eq!(
            fund_handshake_step(&view(
                true,
                true,
                true,
                None,
                0,
                false,
                CompleteSent,
                false,
                None,
                false
            )),
            ExportAndSign
        );
        assert_eq!(
            fund_handshake_step(&view(
                true,
                true,
                true,
                Some(2),
                0,
                true,
                CompleteReady,
                true,
                Some(FundingSendKind::Complete),
                false
            )),
            RetrySend
        );
    }

    #[test]
    fn prefer_funding_psbt_never_downgrades() {
        let (m, c) = pair_ids();
        let signed = signed_two_partidas(&m, &c);
        let mut project = Project::from_signed(signed.clone()).unwrap();
        let q = sign_our_quote(
            &c,
            &signed,
            sign_our_quote(
                &m,
                &signed,
                draft_quote(&signed, Some(8_000_000), "fx").unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        lock_quote_if_ready(&mut project, &q).unwrap();
        let m_coin = dummy_coin(Role::Mandante, 1, 5_000_000);
        let c_coin = dummy_coin(Role::Contratista, 2, 5_000_000);
        let partial =
            psbt_to_hex(&build_our_partial(&project, Role::Mandante, &m_coin, 400).unwrap());
        let complete = psbt_to_hex(
            &complete_incoming_partial(&project, Role::Contratista, &c_coin, 400, &partial)
                .unwrap(),
        );
        assert_eq!(prefer_funding_psbt(&complete, &partial), complete);
        assert_eq!(prefer_funding_psbt("", &partial), partial);
        assert_eq!(prefer_funding_psbt(&partial, &complete), complete);
    }

    #[test]
    fn spanish_chain_status_is_plain() {
        assert!(spanish_chain_status(None, None).contains("Aún no"));
        let s = spanish_chain_status(Some("abcd"), Some(false));
        assert!(s.contains("mempool"));
        assert!(!s.contains("abcd"), "txid stays off the primary line");
        assert!(spanish_chain_status(Some("abcd"), Some(true)).contains("Confirmada"));
        assert!(!spanish_chain_status(Some("abcd"), Some(true)).contains("abcd"));
    }

    #[test]
    fn quote_display_keeps_clp_primary() {
        let (m, c) = pair_ids();
        let p1 = 500_000; // 5_000.00 CLP
        let signed = signed_clp_partida(&m, &c, p1);
        let body = &signed.body;
        let (boleta, _) = obra_amount_pair(contract_bond_minor(body), body.unit, None);
        let (partida, none_sats) = obra_amount_pair(p1, body.unit, None);
        assert!(boleta.contains("CLP"));
        assert!(boleta.contains("1000.00"));
        assert_eq!(partida, "5000.00 CLP");
        assert!(none_sats.is_none());

        let clp_price = hbp_core::btc_price_to_minor(74_492_748.0, Unit::Clp).unwrap();
        let q = draft_quote(&signed, Some(clp_price), "Yadio 74492748 CLP/BTC").unwrap();
        let (p1_main, p1_sats) = obra_amount_pair(p1, body.unit, Some(q.partida_sats(1).unwrap()));
        assert_eq!(p1_main, "5000.00 CLP");
        assert_eq!(p1_sats.as_deref(), Some("6712 sats"));
        let fx = agreed_fx_line(&q, body.unit);
        assert!(fx.starts_with("tipo de cambio acordado:"));
        assert!(fx.contains("CLP/BTC"));
        assert!(!fx.contains("USD/BTC"));
        assert!(show_main_fund_ui(PayStage::FundBondP1));
        assert!(!show_main_fund_ui(PayStage::PartidaInCourse));
    }

    fn funded_p1(m: &Identity, c: &Identity) -> (Project, Quote) {
        let signed = signed_two_partidas(m, c);
        let mut project = Project::from_signed(signed.clone()).unwrap();
        let q = sign_our_quote(
            c,
            &signed,
            sign_our_quote(
                m,
                &signed,
                draft_quote(&signed, Some(8_000_000), "fx").unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        lock_quote_if_ready(&mut project, &q).unwrap();
        let m_coin = dummy_coin(Role::Mandante, 1, 5_000_000);
        let c_coin = dummy_coin(Role::Contratista, 2, 5_000_000);
        let psbt = build_p1_funding_psbt(&project, &m_coin, &c_coin, 400).unwrap();
        apply_verified_p1_funding(&mut project, &serialize_hex(&psbt.unsigned_tx)).unwrap();
        (project, q)
    }

    #[test]
    fn merge_coop_keeps_both_nonces() {
        let a = CoopFile {
            contract_id: "x".into(),
            kind: KIND_PARTIDA.into(),
            partida_id: Some(1),
            outpoint: "aa:0".into(),
            sats: 1,
            dest: "d".into(),
            fee: 250,
            refund: false,
            pay_sats: None,
            refund_dest: None,
            tx_hex: "00".into(),
            sighash: "sh".into(),
            mandante_pubnonce: Some("m".into()),
            contratista_pubnonce: None,
            mandante_partial: None,
            contratista_partial: None,
        };
        let mut b = a.clone();
        b.mandante_pubnonce = None;
        b.contratista_pubnonce = Some("c".into());
        let m = merge_coop_file(Some(a), b);
        assert_eq!(m.mandante_pubnonce.as_deref(), Some("m"));
        assert_eq!(m.contratista_pubnonce.as_deref(), Some("c"));
    }

    #[test]
    fn coop_both_propose_then_sign_reuses_seed_and_finishes() {
        let (m, c) = pair_ids();
        let (mut project, _q) = funded_p1(&m, &c);
        let dest = dummy_coin(Role::Contratista, 9, 1_000).address;
        let mut jm = NonceJournal::default();
        let mut jc = NonceJournal::default();
        let a = coop_propose_on(&m, &project, &dest, 250, &mut jm, KIND_PARTIDA, None).unwrap();
        let b = coop_propose_on(&c, &project, &dest, 250, &mut jc, KIND_PARTIDA, None).unwrap();
        assert_eq!(a.sighash, b.sighash);
        let m_nonce = a.mandante_pubnonce.clone();
        let merged = merge_coop_file(Some(a), b);
        assert!(merged.mandante_pubnonce.is_some());
        assert!(merged.contratista_pubnonce.is_some());
        let signed_c = coop_sign(&c, &project, merged.clone(), &mut jc).unwrap();
        assert_eq!(signed_c.mandante_pubnonce, m_nonce);
        let signed_m = coop_sign(&m, &project, signed_c, &mut jm).unwrap();
        assert_eq!(signed_m.mandante_pubnonce, m_nonce);
        assert!(signed_m.contratista_partial.is_some());
        assert!(signed_m.mandante_partial.is_some());
        let hex = coop_finish(&m, &mut project, &signed_m, &mut jm).unwrap();
        assert!(!hex.is_empty());
        assert!(txid_from_tx_hex(&hex).is_ok());
        assert!(looks_like_signed_coop_hex(&hex));
        let wire = coop_tx_wire(&hex, KIND_PARTIDA);
        assert_eq!(coop_tx_kind_from_artifact(&wire).as_deref(), Some(KIND_PARTIDA));
        assert!(hex_from_artifact(&wire).is_some());
        let mut draft = PayUiDraft::default();
        stash_finished_coop_hex(&mut draft, &hex, KIND_PARTIDA);
        assert!(needs_coop_publish(&draft));
        assert_eq!(
            pago_coop_gate(Some(&project), &draft, None),
            Some(PagoCoopGate::ShowPublish)
        );
        assert_eq!(
            spanish_now_pay(Some(&project), None, &draft, None),
            "Firmado — falta publicar"
        );
        assert!(
            !matches!(
                project.partida(1).unwrap().state,
                PartidaStatus::Paid { .. }
            ),
            "finish must not mark paid before Signet"
        );
        apply_finished_coop_hex(&mut project, &hex, KIND_PARTIDA).unwrap();
        draft.coop_tx_published = true;
        assert!(!needs_coop_publish(&draft));
        assert!(!show_main_fund_ui(pay_stage(Some(&project), None)));
        assert_eq!(
            coop_action(Some(&signed_m), Role::Mandante),
            CoopAction::Finish
        );
    }

    #[test]
    fn bond_coop_after_p1_paid_closes_later_partidas() {
        let (m, c) = pair_ids();
        let (mut project, _q) = funded_p1(&m, &c);
        let dest = dummy_coin(Role::Contratista, 9, 1_000).address;
        let mut jm = NonceJournal::default();
        let mut jc = NonceJournal::default();
        let p1 = coop_propose_on(&m, &project, &dest, 250, &mut jm, KIND_PARTIDA, None).unwrap();
        let p1 = coop_sign(&c, &project, p1, &mut jc).unwrap();
        let p1_hex = coop_finish(&m, &mut project, &p1, &mut jm).unwrap();
        apply_finished_coop_hex(&mut project, &p1_hex, KIND_PARTIDA).unwrap();
        assert!(p1_blocks_bond_return(&project) == false);
        assert!(can_open_stop_wizard(&project));

        let mut jm2 = NonceJournal::default();
        let mut jc2 = NonceJournal::default();
        let bond = coop_propose_on(&c, &project, &dest, 250, &mut jc2, KIND_BOND, None).unwrap();
        assert_eq!(bond.kind, KIND_BOND);
        let bond = coop_sign(&m, &project, bond, &mut jm2).unwrap();
        let hex = coop_finish(&c, &mut project, &bond, &mut jc2).unwrap();
        assert!(!hex.is_empty());
        assert!(txid_from_tx_hex(&hex).is_ok());
        assert!(!project.is_stopped(), "finish must not stop before Signet");
        let mut draft = PayUiDraft::default();
        stash_finished_coop_hex(&mut draft, &hex, KIND_BOND);
        assert!(needs_coop_publish(&draft));
        assert_eq!(
            pago_coop_gate(Some(&project), &draft, Some(&bond)),
            Some(PagoCoopGate::ShowPublish)
        );
        apply_finished_coop_hex(&mut project, &hex, KIND_BOND).unwrap();
        draft.coop_tx_published = true;
        assert!(matches!(project.bond, BondStatus::Released { .. }));
        assert!(matches!(
            project.status,
            hbp_core::ProjectStatus::Closed | hbp_core::ProjectStatus::Cancelled
        ));
        assert_eq!(pay_stage(Some(&project), None), PayStage::AllClosed);
        assert!(!partida_ui_enabled(&project, 2));
        assert!(spanish_now(Some(&project), None).contains("detenida"));
        assert_eq!(
            pago_coop_gate(Some(&project), &draft, None),
            Some(PagoCoopGate::ShowStopped)
        );
    }

    #[test]
    fn gate_exposes_publish_or_redo_if_stopped_without_signet() {
        let (m, c) = pair_ids();
        let (mut project, _q) = funded_p1(&m, &c);
        let dest = dummy_coin(Role::Contratista, 9, 1_000).address;
        let mut jm = NonceJournal::default();
        let mut jc = NonceJournal::default();
        let p1 = coop_propose_on(&m, &project, &dest, 250, &mut jm, KIND_PARTIDA, None).unwrap();
        let p1 = coop_sign(&c, &project, p1, &mut jc).unwrap();
        let p1_hex = coop_finish(&m, &mut project, &p1, &mut jm).unwrap();
        apply_finished_coop_hex(&mut project, &p1_hex, KIND_PARTIDA).unwrap();
        let mut jm2 = NonceJournal::default();
        let mut jc2 = NonceJournal::default();
        let bond = coop_propose_on(&c, &project, &dest, 250, &mut jc2, KIND_BOND, None).unwrap();
        let bond = coop_sign(&m, &project, bond, &mut jm2).unwrap();
        let hex = coop_finish(&c, &mut project, &bond, &mut jc2).unwrap();

        let mut draft = PayUiDraft::default();
        stash_finished_coop_hex(&mut draft, &hex, KIND_BOND);
        assert_eq!(
            pago_coop_gate(Some(&project), &draft, Some(&bond)),
            Some(PagoCoopGate::ShowPublish)
        );

        // Old bug: local state says stopped, hex still in the funding field.
        apply_finished_coop_hex(&mut project, &hex, KIND_BOND).unwrap();
        assert!(project.is_stopped());
        draft.coop_tx_hex.clear();
        draft.funding_tx_hex = hex.clone();
        assert!(recover_coop_tx_into_draft(&mut draft, &project, None));
        assert!(needs_coop_publish(&draft));
        assert_eq!(
            pago_coop_gate(Some(&project), &draft, Some(&bond)),
            Some(PagoCoopGate::ShowPublish)
        );

        // Hex lost entirely: must still expose Rehacer, not a silent "detenida".
        draft.coop_tx_hex.clear();
        draft.funding_tx_hex.clear();
        draft.coop_tx_published = false;
        assert_eq!(
            pago_coop_gate(Some(&project), &draft, Some(&bond)),
            Some(PagoCoopGate::ShowRedoBond)
        );
        assert!(restore_unconfirmed_bond(&mut project, Some(&bond)).unwrap());
        assert!(project.bond_is_funded());
        assert!(!project.is_stopped());
    }

    #[test]
    fn rotating_seed_is_rejected_on_finish() {
        let (m, c) = pair_ids();
        let (mut project, _q) = funded_p1(&m, &c);
        let dest = dummy_coin(Role::Contratista, 9, 1_000).address;
        let mut jm = NonceJournal::default();
        let mut jc = NonceJournal::default();
        let proposed =
            coop_propose_on(&m, &project, &dest, 250, &mut jm, KIND_PARTIDA, None).unwrap();
        let signed = coop_sign(&c, &project, proposed.clone(), &mut jc).unwrap();
        // Simulate the old bug: mandante generates a new seed for the same sighash.
        let rogue = new_nonce_seed(&mut jm).unwrap();
        jm.stash_pending(&proposed.sighash, &rogue);
        let err = coop_finish(&m, &mut project, &signed, &mut jm).unwrap_err();
        assert!(
            err.to_string().contains("no coincide") || err.to_string().contains("no armes"),
            "{err}"
        );
    }
}
