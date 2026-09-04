use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use hbp_bitcoin::Identity;
use hbp_core::{
    bond_minor, suggest_equal_stage_minors, ContractBody, DisputePolicy, Network, Offer,
    PartidaSpec, Role, Unit, DEFAULT_BOND_BPS, PRODUCT_NETWORK,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkEntry {
    pub name: String,
    pub slug: String,
    pub role: Role,
    pub network: Network,
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
}

fn default_dark() -> bool {
    true
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self { dark: true }
    }
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
        store.save_prefs(&UiPrefs { dark: false }).unwrap();
        assert!(!store.load_prefs().dark);
        let _ = fs::remove_dir_all(&tmp);
    }
}
