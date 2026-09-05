use std::fs;
use std::path::{Path, PathBuf};

use crate::pay::{PayCoins, PayUiDraft};
use anyhow::{bail, Context, Result};
use hbp_bitcoin::{import_watch, CoopFile, Identity, OfferedCoin, WatchAccount};
use hbp_core::{
    bond_minor, suggest_equal_stage_minors, vault_decrypt, vault_encrypt, ContractBody,
    DisputePolicy, Network, NonceJournal, Offer, PartidaSpec, Project, Quote, Role, SignedContract,
    Unit, DEFAULT_BOND_BPS, PRODUCT_NETWORK,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkEntry {
    pub name: String,
    pub slug: String,
    pub role: Role,
    pub network: Network,
    /// Other party's display name (e.g. mandante “Don José”). Never a folder slug.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub peer_name: String,
    /// Mandante display name frozen at publish (“Felipe”). Not the obra folder.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub publisher_name: String,
    /// Mandante already published this obra (name board / DHT).
    #[serde(default)]
    pub published: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkIndex {
    pub works: Vec<WorkEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkBackup {
    pub version: u32,
    pub entry: WorkEntry,
    pub identity: Identity,
    pub draft: Option<ContractBody>,
    pub offer: Option<Offer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiPrefs {
    #[serde(default = "default_dark")]
    pub dark: bool,
    /// Who is using this computer. Contratista never sees "Crear obra".
    #[serde(default)]
    pub role: Role,
    #[serde(default)]
    pub mandante_name: String,
    #[serde(default)]
    pub contratista_name: String,
    /// Notes panel starts collapsed so it does not eat the obra view.
    #[serde(default)]
    pub log_open: bool,
}

fn default_dark() -> bool {
    true
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            dark: true,
            role: Role::Mandante,
            mandante_name: String::new(),
            contratista_name: String::new(),
            log_open: false,
        }
    }
}

impl UiPrefs {
    pub fn display_name(&self) -> &str {
        match self.role {
            Role::Mandante => self.mandante_name.trim(),
            Role::Contratista => self.contratista_name.trim(),
        }
    }

    pub fn set_display_name(&mut self, name: impl Into<String>) {
        let name = name.into().trim().to_string();
        match self.role {
            Role::Mandante => self.mandante_name = name,
            Role::Contratista => self.contratista_name = name,
        }
    }

    pub fn first_run(&self) -> bool {
        self.mandante_name.trim().is_empty() && self.contratista_name.trim().is_empty()
    }

    pub fn needs_name(&self) -> bool {
        self.display_name().is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PeerBook {
    pub onions: Vec<String>,
}

pub struct WorkStore {
    pub root: PathBuf,
    pub index: WorkIndex,
}

pub fn default_works_root() -> PathBuf {
    if let Ok(p) = std::env::var("HBP_WORKS") {
        return PathBuf::from(p);
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home)
        .join("Documents")
        .join("home_builder_pay")
}

pub fn slugify(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "work".into()
    } else {
        s
    }
}

impl WorkStore {
    pub fn open(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)?;
        let idx = root.join("works.json");
        let index = if idx.exists() {
            serde_json::from_str(&fs::read_to_string(&idx)?)?
        } else {
            WorkIndex::default()
        };
        Ok(Self { root, index })
    }

    pub fn save_index(&self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        fs::write(
            self.root.join("works.json"),
            serde_json::to_string_pretty(&self.index)?,
        )?;
        Ok(())
    }

    pub fn work_dir(&self, slug: &str) -> PathBuf {
        self.root.join(slug)
    }

    pub fn prefs_path(&self) -> PathBuf {
        self.root.join("ui.json")
    }

    pub fn load_prefs(&self) -> UiPrefs {
        let p = self.prefs_path();
        if !p.exists() {
            return UiPrefs::default();
        }
        fs::read_to_string(&p)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save_prefs(&self, prefs: &UiPrefs) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        fs::write(self.prefs_path(), serde_json::to_string_pretty(prefs)?)?;
        Ok(())
    }

    pub fn create_work(
        &mut self,
        name: &str,
        role: Role,
        network: Network,
        secret_hex: Option<&str>,
    ) -> Result<WorkEntry> {
        let name = name.trim();
        if name.is_empty() {
            bail!("work name is required");
        }
        let slug = slugify(name);
        if network == Network::Bitcoin {
            bail!("mainnet is not offered; the product network is Signet");
        }
        if self.index.works.iter().any(|w| w.slug == slug) {
            bail!("a work named '{name}' already exists");
        }
        let dir = self.work_dir(&slug);
        fs::create_dir_all(&dir)?;
        fs::create_dir_all(dir.join("contracts"))?;
        let mut id = if let Some(hex) = secret_hex {
            hbp_bitcoin::identity_from_secret(network, hex)?
        } else {
            hbp_bitcoin::generate_identity(network)?
        };
        id.role = Some(role);
        fs::write(
            dir.join("identity.json"),
            serde_json::to_string_pretty(&id)?,
        )?;
        let entry = WorkEntry {
            name: name.to_string(),
            slug,
            role,
            network,
            peer_name: String::new(),
            publisher_name: String::new(),
            published: false,
        };
        self.index.works.push(entry.clone());
        self.save_index()?;
        Ok(entry)
    }

    /// Product GUI: always Signet. No network picker.
    pub fn create_product_work(
        &mut self,
        name: &str,
        role: Role,
        secret_hex: Option<&str>,
    ) -> Result<WorkEntry> {
        self.create_work(name, role, PRODUCT_NETWORK, secret_hex)
    }

    pub fn load_identity(&self, slug: &str) -> Result<Identity> {
        Ok(serde_json::from_str(&fs::read_to_string(
            self.work_dir(slug).join("identity.json"),
        )?)?)
    }

    pub fn load_draft(&self, slug: &str) -> Result<Option<ContractBody>> {
        let p = self.work_dir(slug).join("draft.json");
        if !p.exists() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_str(&fs::read_to_string(p)?)?))
    }

    pub fn save_draft(&self, slug: &str, body: &ContractBody) -> Result<()> {
        fs::write(
            self.work_dir(slug).join("draft.json"),
            serde_json::to_string_pretty(body)?,
        )?;
        Ok(())
    }

    pub fn save_offer(&self, slug: &str, offer: &Offer) -> Result<PathBuf> {
        let path = self.work_dir(slug).join("00-offer.json");
        fs::write(&path, serde_json::to_string_pretty(offer)?)?;
        Ok(path)
    }

    pub fn load_offer(&self, slug: &str) -> Result<Option<Offer>> {
        let p = self.work_dir(slug).join("00-offer.json");
        if !p.exists() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_str(&fs::read_to_string(p)?)?))
    }

    pub fn save_pending(&self, slug: &str, signed: &SignedContract) -> Result<PathBuf> {
        let path = self.work_dir(slug).join("01-accepted.pending.json");
        fs::write(&path, serde_json::to_string_pretty(signed)?)?;
        Ok(path)
    }

    pub fn load_pending(&self, slug: &str) -> Result<Option<SignedContract>> {
        read_json_opt(&self.work_dir(slug).join("01-accepted.pending.json"))
    }

    pub fn save_signed(&self, slug: &str, signed: &SignedContract) -> Result<PathBuf> {
        let dir = self.work_dir(slug).join("contracts");
        fs::create_dir_all(&dir)?;
        let path = dir.join("01-accepted.json");
        fs::write(&path, serde_json::to_string_pretty(signed)?)?;
        Ok(path)
    }

    pub fn load_signed(&self, slug: &str) -> Result<Option<SignedContract>> {
        read_json_opt(
            &self
                .work_dir(slug)
                .join("contracts")
                .join("01-accepted.json"),
        )
    }

    pub fn save_peer_onion(&self, slug: &str, onion: &str) -> Result<()> {
        let onion = onion.trim();
        if onion.is_empty() {
            return Ok(());
        }
        fs::write(
            self.work_dir(slug).join("peer.json"),
            serde_json::to_string_pretty(&serde_json::json!({ "onion": onion }))?,
        )?;
        let mut book = self.load_peer_book();
        if !book.onions.iter().any(|o| o == onion) {
            book.onions.push(onion.to_string());
            if book.onions.len() > 32 {
                book.onions.remove(0);
            }
            let _ = fs::write(
                self.root.join("net-peers.json"),
                serde_json::to_string_pretty(&book)?,
            );
        }
        Ok(())
    }

    pub fn load_peer_onion(&self, slug: &str) -> Option<String> {
        let p = self.work_dir(slug).join("peer.json");
        let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(p).ok()?).ok()?;
        v.get("onion")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
    }

    pub fn set_publisher_name(&mut self, slug: &str, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Ok(());
        }
        if let Some(w) = self.index.works.iter_mut().find(|w| w.slug == slug) {
            if w.publisher_name != name {
                w.publisher_name = name.to_string();
                self.save_index()?;
            }
        }
        Ok(())
    }

    pub fn mark_published(&mut self, slug: &str) -> Result<()> {
        let Some(w) = self.index.works.iter_mut().find(|w| w.slug == slug) else {
            return Ok(());
        };
        if w.published {
            return Ok(());
        }
        w.published = true;
        self.save_index()
    }

    pub fn remember_peer(
        &mut self,
        slug: &str,
        onion: &str,
        peer_name: Option<&str>,
    ) -> Result<()> {
        self.save_peer_onion(slug, onion)?;
        if let Some(n) = peer_name.map(str::trim).filter(|s| !s.is_empty()) {
            if let Some(w) = self.index.works.iter_mut().find(|w| w.slug == slug) {
                if w.peer_name != n {
                    w.peer_name = n.to_string();
                    self.save_index()?;
                }
            }
        }
        Ok(())
    }

    pub fn find_by_work_name(&self, name: &str) -> Option<WorkEntry> {
        let slug = slugify(name);
        let trimmed = name.trim();
        self.index
            .works
            .iter()
            .find(|w| w.slug == slug || w.name.eq_ignore_ascii_case(trimmed))
            .cloned()
    }

    pub fn load_peer_book(&self) -> PeerBook {
        let p = self.root.join("net-peers.json");
        fs::read_to_string(p)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Local watch-only xpub. Never sent to the peer.
    pub fn save_watch(
        &self,
        slug: &str,
        account: &WatchAccount,
        passphrase: Option<&str>,
    ) -> Result<PathBuf> {
        let path = self.work_dir(slug).join("watch.json");
        let json = serde_json::to_vec_pretty(account)?;
        if let Some(pw) = passphrase.map(str::trim).filter(|s| !s.is_empty()) {
            fs::write(&path, vault_encrypt(&json, pw)?)?;
        } else {
            fs::write(&path, json)?;
        }
        Ok(path)
    }

    pub fn load_watch(&self, slug: &str, passphrase: Option<&str>) -> Result<Option<WatchAccount>> {
        let p = self.work_dir(slug).join("watch.json");
        if !p.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&p)?;
        if hbp_core::vault_is_encrypted(&raw) {
            let pw = passphrase
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .context("esta billetera está cifrada: escribe la frase")?;
            let pt = vault_decrypt(&raw, pw)?;
            return Ok(Some(serde_json::from_slice(&pt)?));
        }
        Ok(Some(serde_json::from_str(&raw)?))
    }

    pub fn import_xpub_local(
        &self,
        slug: &str,
        raw: &str,
        passphrase: Option<&str>,
    ) -> Result<WatchAccount> {
        let acc = import_watch(raw, None, PRODUCT_NETWORK, 20)?;
        self.save_watch(slug, &acc, passphrase)?;
        Ok(acc)
    }

    pub fn pay_dir(&self, slug: &str) -> PathBuf {
        self.work_dir(slug).join("pay")
    }

    fn ensure_pay_dir(&self, slug: &str) -> Result<PathBuf> {
        let dir = self.pay_dir(slug);
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    pub fn save_pay_project(&self, slug: &str, project: &Project) -> Result<()> {
        let dir = self.ensure_pay_dir(slug)?;
        fs::write(
            dir.join("state.json"),
            serde_json::to_string_pretty(project)?,
        )?;
        Ok(())
    }

    pub fn load_pay_project(&self, slug: &str) -> Result<Option<Project>> {
        read_json_opt(&self.pay_dir(slug).join("state.json"))
    }

    pub fn save_pay_quote(&self, slug: &str, quote: &Quote) -> Result<()> {
        let dir = self.ensure_pay_dir(slug)?;
        fs::write(
            dir.join("02-quote.json"),
            serde_json::to_string_pretty(quote)?,
        )?;
        Ok(())
    }

    pub fn load_pay_quote(&self, slug: &str) -> Result<Option<Quote>> {
        read_json_opt(&self.pay_dir(slug).join("02-quote.json"))
    }

    pub fn save_pay_coins(&self, slug: &str, coins: &PayCoins) -> Result<()> {
        let dir = self.ensure_pay_dir(slug)?;
        fs::write(dir.join("coins.json"), serde_json::to_string_pretty(coins)?)?;
        Ok(())
    }

    pub fn load_pay_coins(&self, slug: &str) -> Result<PayCoins> {
        Ok(read_json_opt(&self.pay_dir(slug).join("coins.json"))?.unwrap_or_default())
    }

    pub fn save_pay_draft(&self, slug: &str, draft: &PayUiDraft) -> Result<()> {
        let dir = self.ensure_pay_dir(slug)?;
        fs::write(
            dir.join("session.json"),
            serde_json::to_string_pretty(draft)?,
        )?;
        Ok(())
    }

    pub fn load_pay_draft(&self, slug: &str) -> Result<PayUiDraft> {
        Ok(read_json_opt(&self.pay_dir(slug).join("session.json"))?.unwrap_or_default())
    }

    pub fn save_pay_coop(&self, slug: &str, coop: &CoopFile) -> Result<()> {
        let dir = self.ensure_pay_dir(slug)?;
        fs::write(
            dir.join("08-coop.json"),
            serde_json::to_string_pretty(coop)?,
        )?;
        Ok(())
    }

    pub fn load_pay_coop(&self, slug: &str) -> Result<Option<CoopFile>> {
        read_json_opt(&self.pay_dir(slug).join("08-coop.json"))
    }

    pub fn save_pay_nonces(&self, slug: &str, journal: &NonceJournal) -> Result<()> {
        let dir = self.ensure_pay_dir(slug)?;
        fs::write(
            dir.join("nonces.json"),
            serde_json::to_string_pretty(journal)?,
        )?;
        Ok(())
    }

    pub fn load_pay_nonces(&self, slug: &str) -> Result<NonceJournal> {
        Ok(read_json_opt(&self.pay_dir(slug).join("nonces.json"))?.unwrap_or_default())
    }

    pub fn save_offered_coin(&self, slug: &str, coin: &OfferedCoin) -> Result<()> {
        let mut coins = self.load_pay_coins(slug)?;
        match coin.role {
            Role::Mandante => coins.mandante = Some(coin.clone()),
            Role::Contratista => coins.contratista = Some(coin.clone()),
        }
        self.save_pay_coins(slug, &coins)
    }

    /// Load or create `Project` from the signed trato. Re-applies a fully signed quote.
    pub fn ensure_pay_project(&self, slug: &str) -> Result<Project> {
        if let Some(mut p) = self.load_pay_project(slug)? {
            if p.quote.is_none() {
                if let Some(q) = self.load_pay_quote(slug)? {
                    if q.mandante_sig.is_some() && q.contratista_sig.is_some() {
                        let _ = p.set_quote(q);
                        self.save_pay_project(slug, &p)?;
                    }
                }
            }
            return Ok(p);
        }
        let signed = self
            .load_signed(slug)?
            .context("falta el trato firmado (01-accepted.json)")?;
        let mut p = Project::from_signed(signed)?;
        if let Some(q) = self.load_pay_quote(slug)? {
            if q.mandante_sig.is_some() && q.contratista_sig.is_some() {
                let _ = p.set_quote(q);
            }
        }
        self.save_pay_project(slug, &p)?;
        Ok(p)
    }

    /// Contratista: open or create a local folder for a work found on the net.
    pub fn ensure_contratista_work(
        &mut self,
        name: &str,
        peer_name: Option<&str>,
    ) -> Result<WorkEntry> {
        let slug = slugify(name);
        let peer = peer_name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        if let Some(pos) = self.index.works.iter().position(|w| w.slug == slug) {
            if let Some(p) = peer {
                self.index.works[pos].peer_name = p;
                self.save_index()?;
            }
            return Ok(self.index.works[pos].clone());
        }
        let mut e = self.create_product_work(name, Role::Contratista, None)?;
        if let Some(p) = peer {
            if let Some(w) = self.index.works.iter_mut().find(|w| w.slug == e.slug) {
                w.peer_name = p.clone();
                e.peer_name = p;
                self.save_index()?;
            }
        }
        Ok(e)
    }
}

fn read_json_opt<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&fs::read_to_string(path)?)?))
}

/// Build a fee-burn draft whose stages each equal the 10% bond.
pub fn draft_equal_stages(
    identity: &Identity,
    work_name: &str,
    unit: Unit,
    total_minor: u64,
    t1: u32,
    t2: u32,
    descriptions: &[String],
) -> Result<ContractBody> {
    if identity.role != Some(Role::Mandante) {
        bail!("only the mandante creates an offer");
    }
    let bond_bps = DEFAULT_BOND_BPS;
    let amounts = suggest_equal_stage_minors(total_minor, bond_bps)?;
    let n = amounts.len();
    if !descriptions.is_empty() && descriptions.len() != n {
        bail!(
            "need {n} stage descriptions (or none — defaults will be used); got {}",
            descriptions.len()
        );
    }
    let partidas = amounts
        .iter()
        .enumerate()
        .map(|(i, amt)| PartidaSpec {
            id: (i as u32) + 1,
            description: descriptions
                .get(i)
                .cloned()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| format!("Partida {}", i + 1)),
            amount_minor: *amt,
            plazo_unix: t1,
        })
        .collect();
    let body = ContractBody {
        network: identity.network,
        unit,
        work_name: work_name.to_string(),
        bond_bps,
        t_project: t2,
        partidas,
        mandante_pubkey: identity.public_key.clone(),
        contratista_pubkey: None,
        dispute: DisputePolicy::fee_burn(t1, t2),
    };
    body.validate()?;
    let bond = bond_minor(body.total_minor(), bond_bps)?;
    if !hbp_core::stages_equal_bond(
        body.total_minor(),
        bond_bps,
        &body
            .partidas
            .iter()
            .map(|p| p.amount_minor)
            .collect::<Vec<_>>(),
    ) {
        bail!(
            "could not split total so each stage equals bond {bond}; pick a total that is N × (total × 10%)"
        );
    }
    Ok(body)
}

pub fn export_backup(store: &WorkStore, slug: &str) -> Result<WorkBackup> {
    let entry = store
        .index
        .works
        .iter()
        .find(|w| w.slug == slug)
        .cloned()
        .with_context(|| format!("unknown work {slug}"))?;
    Ok(WorkBackup {
        version: 1,
        entry,
        identity: store.load_identity(slug)?,
        draft: store.load_draft(slug)?,
        offer: store.load_offer(slug)?,
    })
}

pub fn import_backup(store: &mut WorkStore, backup: &WorkBackup) -> Result<WorkEntry> {
    if backup.version != 1 {
        bail!("unsupported backup version");
    }
    if backup.entry.network == Network::Bitcoin || backup.identity.network == Network::Bitcoin {
        bail!("mainnet backups are not accepted");
    }
    if backup.entry.network != PRODUCT_NETWORK || backup.identity.network != PRODUCT_NETWORK {
        bail!(
            "product GUI is Signet-only (got {:?})",
            backup.entry.network
        );
    }
    let slug = backup.entry.slug.clone();
    if store.index.works.iter().any(|w| w.slug == slug) {
        bail!("work '{}' already exists", backup.entry.name);
    }
    let dir = store.work_dir(&slug);
    fs::create_dir_all(dir.join("contracts"))?;
    fs::write(
        dir.join("identity.json"),
        serde_json::to_string_pretty(&backup.identity)?,
    )?;
    if let Some(d) = &backup.draft {
        fs::write(dir.join("draft.json"), serde_json::to_string_pretty(d)?)?;
    }
    if let Some(o) = &backup.offer {
        fs::write(dir.join("00-offer.json"), serde_json::to_string_pretty(o)?)?;
    }
    store.index.works.push(backup.entry.clone());
    store.save_index()?;
    Ok(backup.entry.clone())
}

pub fn write_backup_file(path: &Path, backup: &WorkBackup) -> Result<()> {
    fs::write(path, serde_json::to_string_pretty(backup)?)?;
    Ok(())
}

pub fn read_backup_file(path: &Path) -> Result<WorkBackup> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_stages_match_ten_percent_bond() {
        let mut id = hbp_bitcoin::generate_identity(Network::Regtest).unwrap();
        id.role = Some(Role::Mandante);
        let body = draft_equal_stages(
            &id,
            "Casa",
            Unit::Usd,
            1_000_000,
            1_700_000_000,
            1_800_000_000,
            &[],
        )
        .unwrap();
        assert_eq!(body.partidas.len(), 10);
        assert!(body.partidas.iter().all(|p| p.amount_minor == 100_000));
        assert_eq!(
            bond_minor(body.total_minor(), body.bond_bps).unwrap(),
            100_000
        );
        assert!(matches!(body.dispute, DisputePolicy::FeeBurn { .. }));
        assert!(!hbp_core::ARBITER_ENABLED);
    }

    #[test]
    fn sats_draft_stores_integer_satoshis() {
        let mut id = hbp_bitcoin::generate_identity(Network::Regtest).unwrap();
        id.role = Some(Role::Mandante);
        let total = hbp_core::parse_major_amount("100000", Unit::Sats).unwrap();
        let body = draft_equal_stages(
            &id,
            "Casa",
            Unit::Sats,
            total,
            1_700_000_000,
            1_800_000_000,
            &[],
        )
        .unwrap();
        assert_eq!(body.unit, Unit::Sats);
        assert_eq!(body.total_minor(), 100_000);
        assert!(body.partidas.iter().all(|p| p.amount_minor == 10_000));
        let json = serde_json::to_string(&body).unwrap();
        assert!(json.contains("\"unit\":\"SATS\""));
        let back: ContractBody = serde_json::from_str(&json).unwrap();
        assert_eq!(back.unit, Unit::Sats);
        assert_eq!(back.total_minor(), 100_000);
    }

    #[test]
    fn product_rejects_mainnet() {
        let tmp = std::env::temp_dir().join(format!("hbp-app-mainnet-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let mut store = WorkStore::open(tmp.join("a")).unwrap();
        let err = store
            .create_work("X", Role::Mandante, Network::Bitcoin, None)
            .unwrap_err();
        assert!(err.to_string().contains("mainnet"));
        let reg = store
            .create_work("Reg", Role::Mandante, Network::Regtest, None)
            .unwrap();
        assert_eq!(reg.network, Network::Regtest);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn import_rejects_mainnet_and_regtest_backups() {
        let tmp = std::env::temp_dir().join(format!("hbp-app-bak-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let mut store = WorkStore::open(tmp.join("a")).unwrap();
        let mut id = hbp_bitcoin::generate_identity(Network::Bitcoin).unwrap();
        id.role = Some(Role::Mandante);
        let bak = WorkBackup {
            version: 1,
            entry: WorkEntry {
                name: "X".into(),
                slug: "x".into(),
                role: Role::Mandante,
                network: Network::Bitcoin,
                peer_name: String::new(),
                publisher_name: String::new(),
                published: false,
            },
            identity: id,
            draft: None,
            offer: None,
        };
        let err = import_backup(&mut store, &bak).unwrap_err();
        assert!(err.to_string().contains("mainnet"));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn work_store_create_export_import() {
        let tmp = std::env::temp_dir().join(format!("hbp-app-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let mut store = WorkStore::open(tmp.join("a")).unwrap();
        let entry = store
            .create_product_work("Obra Norte", Role::Mandante, None)
            .unwrap();
        assert_eq!(entry.network, PRODUCT_NETWORK);
        assert_eq!(entry.slug, "obra-norte");
        let id = store.load_identity(&entry.slug).unwrap();
        let draft = draft_equal_stages(
            &id,
            &entry.name,
            Unit::Usd,
            1_000_000,
            1_700_000_000,
            1_800_000_000,
            &[],
        )
        .unwrap();
        store.save_draft(&entry.slug, &draft).unwrap();
        let bak = export_backup(&store, &entry.slug).unwrap();
        let mut other = WorkStore::open(tmp.join("b")).unwrap();
        import_backup(&mut other, &bak).unwrap();
        assert_eq!(other.index.works.len(), 1);
        assert_eq!(
            other.load_identity(&entry.slug).unwrap().public_key,
            id.public_key
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn ui_prefs_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("hbp-app-prefs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let store = WorkStore::open(tmp.join("a")).unwrap();
        assert!(store.load_prefs().dark);
        store
            .save_prefs(&UiPrefs {
                dark: false,
                role: Role::Contratista,
                contratista_name: "Don José".into(),
                ..UiPrefs::default()
            })
            .unwrap();
        let back = store.load_prefs();
        assert!(!back.dark);
        assert_eq!(back.role, Role::Contratista);
        assert_eq!(back.display_name(), "Don José");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn contratista_work_and_peer_book() {
        let tmp = std::env::temp_dir().join(format!("hbp-app-ct-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let mut store = WorkStore::open(tmp.join("a")).unwrap();
        let e = store
            .ensure_contratista_work("Casa Norte", Some("Doña Ana"))
            .unwrap();
        assert_eq!(e.role, Role::Contratista);
        assert_eq!(e.peer_name, "Doña Ana");
        assert_eq!(
            store
                .ensure_contratista_work("Casa Norte", None)
                .unwrap()
                .slug,
            e.slug
        );
        store.save_peer_onion(&e.slug, "abc.onion").unwrap();
        assert_eq!(store.load_peer_onion(&e.slug).as_deref(), Some("abc.onion"));
        assert!(store
            .load_peer_book()
            .onions
            .iter()
            .any(|o| o == "abc.onion"));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn publish_and_hello_peer_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("hbp-app-hello-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let mut store = WorkStore::open(tmp.join("a")).unwrap();
        let e = store
            .create_product_work("casa2", Role::Mandante, None)
            .unwrap();
        assert!(!e.published);
        store.mark_published(&e.slug).unwrap();
        store.set_publisher_name(&e.slug, "Felipe").unwrap();
        store
            .remember_peer(&e.slug, "jose.onion", Some("Felipe"))
            .unwrap();
        assert_eq!(
            store.find_by_work_name("casa2").unwrap().publisher_name,
            "Felipe"
        );
        let back = store.find_by_work_name("Casa2").unwrap();
        assert!(back.published);
        assert_eq!(back.peer_name, "Felipe");
        assert_eq!(
            store.load_peer_onion(&e.slug).as_deref(),
            Some("jose.onion")
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn pay_state_roundtrip() {
        use crate::pay::{draft_quote, lock_quote_if_ready, sign_our_quote};
        use crate::protocol::{contratista_accept, mandante_commit};
        use hbp_core::Offer;

        let tmp = std::env::temp_dir().join(format!("hbp-app-pay-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let mut store = WorkStore::open(tmp.join("a")).unwrap();
        let e = store
            .create_product_work("casa2", Role::Mandante, None)
            .unwrap();
        let mut m = store.load_identity(&e.slug).unwrap();
        m.role = Some(Role::Mandante);
        let mut c = hbp_bitcoin::generate_identity(PRODUCT_NETWORK).unwrap();
        c.role = Some(Role::Contratista);
        let draft = draft_equal_stages(
            &m,
            "casa2",
            Unit::Usd,
            1_000_000,
            1_700_000_000,
            1_800_000_000,
            &[],
        )
        .unwrap();
        let offer = Offer {
            mandante_sig: hbp_bitcoin::sign_body(&m.secret().unwrap(), &draft).unwrap(),
            body: draft,
        };
        let pending = contratista_accept(offer.clone(), &c).unwrap();
        let signed = mandante_commit(&offer, pending, &m).unwrap();
        store.save_signed(&e.slug, &signed).unwrap();
        let mut project = store.ensure_pay_project(&e.slug).unwrap();
        let q = sign_our_quote(
            &c,
            &signed,
            sign_our_quote(
                &m,
                &signed,
                draft_quote(&signed, Some(8_000_000), "test").unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        store.save_pay_quote(&e.slug, &q).unwrap();
        assert!(lock_quote_if_ready(&mut project, &q).unwrap());
        store.save_pay_project(&e.slug, &project).unwrap();
        let back = store.ensure_pay_project(&e.slug).unwrap();
        assert!(back.quote.is_some());
        assert_eq!(back.active_partida_id(), Some(1));
        let _ = fs::remove_dir_all(&tmp);
    }
}
