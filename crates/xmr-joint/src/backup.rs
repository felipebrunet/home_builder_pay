//! Respaldos en texto. Modo 0600. La semilla personal no reconstruye el share.

use std::fs::OpenOptions;
use std::io::{Write, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use zeroize::{Zeroize, Zeroizing};

use crate::network::Net;
use crate::{Error, Result};

const SEED_MAGIC: &str = "konstruado single-sig v1";
const SHARE_MAGIC: &str = "konstruado multisig-share v1";

/// 25 palabras y la dirección, para comprobar al restaurar.
///
/// `height` es la altura de cadena al crear/exportar el respaldo. Al restaurar,
/// el scan parte de ahí hacia adelante. `None` = respaldo viejo sin altura
/// (fallback: ventana reciente + aviso de mirar más atrás).
pub struct SeedBackup {
    pub net: Net,
    pub address: String,
    /// Altura del tip al respaldar. Opcional por compatibilidad con v1 sin campo.
    pub height: Option<u64>,
    pub words: Zeroizing<String>,
}

impl Drop for SeedBackup {
    fn drop(&mut self) {
        self.address.zeroize();
        self.words.zeroize();
    }
}

impl SeedBackup {
    pub fn to_text(&self) -> String {
        let mut out = format!(
            "{SEED_MAGIC}\nnetwork {}\naddress {}\nlanguage english\n",
            self.net.label(),
            self.address,
        );
        if let Some(h) = self.height {
            out.push_str(&format!("height {h}\n"));
        }
        out.push_str(self.words.as_str());
        out.push('\n');
        out
    }

    pub fn parse(text: &str) -> Result<Self> {
        let mut lines = text.lines();
        let magic = lines.next().unwrap_or("");
        if magic != SEED_MAGIC {
            return Err(Error::Backup("no es un respaldo de semilla v1".into()));
        }
        let net = expect_field(&mut lines, "network")?;
        let address = expect_field(&mut lines, "address")?;
        let language = expect_field(&mut lines, "language")?;
        if language != "english" {
            return Err(Error::Backup("solo inglés en este esqueleto".into()));
        }
        // Opcional: `height N` (respaldos nuevos). Si falta, es un v1 viejo.
        let next = lines.next().unwrap_or("").trim();
        let (height, words) = if let Some(rest) = next.strip_prefix("height ") {
            let h = rest
                .trim()
                .parse::<u64>()
                .map_err(|_| Error::Backup("altura de semilla inválida".into()))?;
            let w = lines.next().unwrap_or("").trim().to_string();
            (Some(h), w)
        } else {
            (None, next.to_string())
        };
        if words.split_whitespace().count() != 25 {
            return Err(Error::Backup("la semilla tiene que tener 25 palabras".into()));
        }
        Ok(Self {
            net: Net::parse(&net).map_err(Error::Backup)?,
            address,
            height,
            words: Zeroizing::new(words),
        })
    }
}

/// Share FROST de una obra, más la view privada compartida.
///
/// Con la semilla personal no se arma esto. Hace falta este archivo.
pub struct ShareBackup {
    pub role: String,
    pub obra_id: String,
    pub net: Net,
    pub address: String,
    pub context: [u8; 32],
    pub view_private: Zeroizing<[u8; 32]>,
    pub threshold_keys: Zeroizing<Vec<u8>>,
}

impl Drop for ShareBackup {
    fn drop(&mut self) {
        self.address.zeroize();
        self.view_private.zeroize();
        self.threshold_keys.zeroize();
    }
}

impl ShareBackup {
    pub fn to_text(&self) -> String {
        format!(
            "{SHARE_MAGIC}\nrole {}\nobra {}\nnetwork {}\naddress {}\ncontext {}\nview_private {}\nthreshold_keys {}\n",
            self.role,
            self.obra_id,
            self.net.label(),
            self.address,
            hex::encode(self.context),
            hex::encode(self.view_private.as_slice()),
            hex::encode(self.threshold_keys.as_slice()),
        )
    }

    pub fn parse(text: &str) -> Result<Self> {
        let mut lines = text.lines();
        let magic = lines.next().unwrap_or("");
        if magic != SHARE_MAGIC {
            return Err(Error::Backup("no es un respaldo de share v1".into()));
        }
        let role = expect_field(&mut lines, "role")?;
        if role != "mandante" && role != "contratista" {
            return Err(Error::Backup("rol del share desconocido".into()));
        }
        let obra_id = expect_field(&mut lines, "obra")?;
        let net = Net::parse(&expect_field(&mut lines, "network")?).map_err(Error::Backup)?;
        let address = expect_field(&mut lines, "address")?;
        let context = decode_32(&expect_field(&mut lines, "context")?)?;
        let view_private = Zeroizing::new(decode_32(&expect_field(&mut lines, "view_private")?)?);
        let keys_hex = expect_field(&mut lines, "threshold_keys")?;
        let threshold_keys = Zeroizing::new(
            hex::decode(keys_hex.trim()).map_err(|e| Error::Backup(e.to_string()))?,
        );
        if threshold_keys.is_empty() {
            return Err(Error::Backup("threshold_keys vacío".into()));
        }
        Ok(Self {
            role,
            obra_id,
            net,
            address,
            context,
            view_private,
            threshold_keys,
        })
    }
}

fn expect_field<'a>(lines: &mut impl Iterator<Item = &'a str>, key: &str) -> Result<String> {
    let line = lines.next().unwrap_or("");
    let mut parts = line.splitn(2, ' ');
    let got = parts.next().unwrap_or("");
    let value = parts.next().unwrap_or("").trim();
    if got != key || value.is_empty() {
        return Err(Error::Backup(format!("falta el campo {key}")));
    }
    Ok(value.to_string())
}

fn decode_32(hex_str: &str) -> Result<[u8; 32]> {
    let bytes = hex::decode(hex_str.trim()).map_err(|e| Error::Backup(e.to_string()))?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| Error::Backup("se esperaban 32 bytes".into()))?;
    Ok(arr)
}

/// Crea el archivo y falla si ya existe. Queda en 0600.
pub fn write_secret_file(path: &Path, text: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(text.as_bytes())?;
    Ok(())
}

pub fn read_secret_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_semilla_en_texto_vuelve() {
        let words = "uno dos tres cuatro cinco seis siete ocho nueve diez once doce trece catorce quince dieciseis diecisiete dieciocho diecinueve veinte veintiuno veintidos veintitres veinticuatro veinticinco";
        let seed = SeedBackup {
            net: Net::Stagenet,
            address: "5abc".into(),
            height: Some(1_234_567),
            words: Zeroizing::new(words.into()),
        };
        let parsed = SeedBackup::parse(&seed.to_text()).unwrap();
        assert_eq!(parsed.net, Net::Stagenet);
        assert_eq!(parsed.address, "5abc");
        assert_eq!(parsed.height, Some(1_234_567));
        assert_eq!(parsed.words.as_str(), words);

        // Respaldo viejo sin height: las palabras vienen justo después de language.
        let viejo = format!(
            "{SEED_MAGIC}\nnetwork stagenet\naddress 5abc\nlanguage english\n{words}\n"
        );
        let p2 = SeedBackup::parse(&viejo).unwrap();
        assert_eq!(p2.height, None);
        assert_eq!(p2.words.as_str(), words);
    }

    #[test]
    fn el_share_redondo_no_pierde_el_rol() {
        let share = ShareBackup {
            role: "mandante".into(),
            obra_id: "obra-1".into(),
            net: Net::Stagenet,
            address: "5addr".into(),
            context: [7; 32],
            view_private: Zeroizing::new([9; 32]),
            threshold_keys: Zeroizing::new(vec![1, 2, 3, 4]),
        };
        let parsed = ShareBackup::parse(&share.to_text()).unwrap();
        assert_eq!(parsed.role, "mandante");
        assert_eq!(parsed.obra_id, "obra-1");
        assert_eq!(parsed.context, [7; 32]);
        assert_eq!(parsed.view_private.as_slice(), &[9; 32]);
        assert_eq!(parsed.threshold_keys.as_slice(), &[1, 2, 3, 4]);
    }
}
