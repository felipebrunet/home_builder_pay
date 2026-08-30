use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use hbp_bitcoin::Identity;
use hbp_core::{ArbiterNomination, NonceJournal, Offer, Project, Quote, SignedContract};

#[derive(Clone)]
pub struct Store {
    pub root: PathBuf,
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn init_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        fs::create_dir_all(self.root.join("contracts"))?;
        Ok(())
    }

    pub fn identity_path(&self) -> PathBuf {
        self.root.join("identity.json")
    }

    pub fn load_identity(&self) -> Result<Identity> {
        let raw = fs::read_to_string(self.identity_path())
            .context("identity.json missing; run hbp init")?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn save_identity(&self, id: &Identity) -> Result<()> {
        self.init_dir()?;
        write_json(&self.identity_path(), id)
    }

    pub fn nonce_path(&self) -> PathBuf {
        self.root.join("nonces.json")
    }

    pub fn load_nonces(&self) -> Result<NonceJournal> {
        let p = self.nonce_path();
        if !p.exists() {
            return Ok(NonceJournal::default());
        }
        Ok(serde_json::from_str(&fs::read_to_string(p)?)?)
    }

    pub fn save_nonces(&self, j: &NonceJournal) -> Result<()> {
        write_json(&self.nonce_path(), j)
    }

    pub fn draft_path(&self) -> PathBuf {
        self.root.join("draft.json")
    }

    pub fn load_draft(&self) -> Result<hbp_core::ContractBody> {
        Ok(serde_json::from_str(
            &fs::read_to_string(self.draft_path()).context("no draft; run hbp new")?,
        )?)
    }

    pub fn save_draft(&self, body: &hbp_core::ContractBody) -> Result<()> {
        write_json(&self.draft_path(), body)
    }

    pub fn contract_dir(&self, id: &str) -> PathBuf {
        self.root.join("contracts").join(id)
    }

    pub fn save_offer(&self, offer: &Offer) -> Result<PathBuf> {
        let path = self.root.join("00-offer.json");
        write_json(&path, offer)?;
        Ok(path)
    }

    pub fn save_signed(&self, signed: &SignedContract) -> Result<PathBuf> {
        let id = signed.id()?;
        let dir = self.contract_dir(&id);
        fs::create_dir_all(&dir)?;
        let path = dir.join("01-accepted.json");
        write_json(&path, signed)?;
        let project = Project::from_signed(signed.clone())?;
        self.save_project(&project)?;
        Ok(path)
    }

    pub fn save_project(&self, project: &Project) -> Result<()> {
        let id = project.contract.id()?;
        let dir = self.contract_dir(&id);
        fs::create_dir_all(&dir)?;
        write_json(&dir.join("state.json"), project)?;
        // pointer to current contract
        fs::write(self.root.join("CURRENT"), id.as_bytes())?;
        Ok(())
    }

    pub fn current_id(&self) -> Result<String> {
        let p = self.root.join("CURRENT");
        Ok(fs::read_to_string(p)
            .context("no current contract")?
            .trim()
            .to_string())
    }

    pub fn load_project(&self) -> Result<Project> {
        let id = self.current_id()?;
        let path = self.contract_dir(&id).join("state.json");
        Ok(serde_json::from_str(
            &fs::read_to_string(path).context("state.json missing")?,
        )?)
    }

    pub fn quote_path(&self, contract_id: &str) -> PathBuf {
        self.contract_dir(contract_id).join("02-quote.json")
    }

    pub fn load_quote(&self, contract_id: &str) -> Result<Option<Quote>> {
        let path = self.quote_path(contract_id);
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(read_json(&path)?))
    }

    pub fn arbiter_path(&self, contract_id: &str) -> PathBuf {
        self.contract_dir(contract_id).join("03-arbiter.json")
    }

    pub fn save_arbiter(&self, nom: &ArbiterNomination) -> Result<PathBuf> {
        let dir = self.contract_dir(&nom.contract_id);
        fs::create_dir_all(&dir)?;
        let path = self.arbiter_path(&nom.contract_id);
        write_json(&path, nom)?;
        Ok(path)
    }

    pub fn save_quote(&self, quote: &Quote) -> Result<PathBuf> {
        let dir = self.contract_dir(&quote.contract_id);
        fs::create_dir_all(&dir)?;
        let path = dir.join("02-quote.json");
        write_json(&path, quote)?;
        Ok(path)
    }
}

pub fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let s = serde_json::to_string_pretty(value)?;
    fs::write(path, s)?;
    Ok(())
}

pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}
