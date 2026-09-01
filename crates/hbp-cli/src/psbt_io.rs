use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use base64::Engine;
use bitcoin::psbt::Psbt;

/// Load a PSBT from binary, base64, or hex of the PSBT serialization.
pub fn load_psbt(path: &Path) -> Result<Psbt> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    if let Ok(p) = Psbt::deserialize(&bytes) {
        return Ok(p);
    }
    let s = std::str::from_utf8(&bytes)
        .context("PSBT file is neither binary PSBT nor UTF-8 base64/hex")?
        .trim();
    if let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(s) {
        if let Ok(p) = Psbt::deserialize(&raw) {
            return Ok(p);
        }
    }
    let raw = hex::decode(s).context("PSBT as hex")?;
    Psbt::deserialize(&raw).context("PSBT deserialize")
}

pub fn write_psbt_binary(path: &Path, psbt: &Psbt) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, psbt.serialize())?;
    Ok(())
}
