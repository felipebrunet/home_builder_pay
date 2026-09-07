use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use hbp_bitcoin::WatchAccount;
use hbp_core::{Network, Project, Role, Terms};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct Store {
    pub root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub network: Network,
    pub role: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cosigner_xpub: Option<String>,
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn init_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        Ok(())
    }

    pub fn session_path(&self) -> PathBuf {
        self.root.join("session.json")
    }

    pub fn save_session(&self, s: &Session) -> Result<()> {
        self.init_dir()?;
        write_json(&self.session_path(), s)
    }

    pub fn load_session(&self) -> Result<Session> {
        read_json(&self.session_path()).context("session.json missing; run hbp init")
    }

    pub fn watch_path(&self) -> PathBuf {
        self.root.join("watch.json")
    }

    pub fn save_watch(&self, w: &WatchAccount) -> Result<()> {
        write_json(&self.watch_path(), w)?;
        set_owner_secret(&self.watch_path())?;
        Ok(())
    }

    pub fn load_watch(&self) -> Result<WatchAccount> {
        read_json(&self.watch_path()).context("watch.json missing; hbp watch-import --xpub …")
    }

    pub fn offer_path(&self) -> PathBuf {
        self.root.join("00-offer.json")
    }

    pub fn accepted_path(&self) -> PathBuf {
        self.root.join("01-accepted.json")
    }

    pub fn coin_path(&self) -> PathBuf {
        self.root.join("05-coin.json")
    }

    pub fn state_path(&self) -> PathBuf {
        self.root.join("state.json")
    }

    pub fn funding_psbt_path(&self) -> PathBuf {
        self.root.join("funding.psbt")
    }

    pub fn burn_psbt_path(&self) -> PathBuf {
        self.root.join("burn.psbt")
    }

    pub fn coop_psbt_path(&self) -> PathBuf {
        self.root.join("coop.psbt")
    }

    pub fn save_offer_terms(&self, terms: &Terms) -> Result<PathBuf> {
        let p = self.offer_path();
        write_json(&p, terms)?;
        Ok(p)
    }

    pub fn save_accepted(&self, terms: &Terms) -> Result<PathBuf> {
        let p = self.accepted_path();
        write_json(&p, terms)?;
        Ok(p)
    }

    pub fn save_project(&self, project: &Project) -> Result<()> {
        write_json(&self.state_path(), project)
    }

    pub fn load_project(&self) -> Result<Project> {
        read_json(&self.state_path()).context("state.json missing")
    }
}

pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn write_json<T: serde::Serialize>(path: &Path, v: &T) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let s = serde_json::to_string_pretty(v)?;
    fs::write(path, s + "\n")?;
    Ok(())
}

fn set_owner_secret(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut p = fs::metadata(path)?.permissions();
        p.set_mode(0o600);
        fs::set_permissions(path, p)?;
    }
    Ok(())
}


