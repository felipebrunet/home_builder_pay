//! Fetch and unpack the official Tor Expert Bundle so one click can spawn
//! a Hidden Service. We do not vendor Tor in git; first connect downloads
//! from dist.torproject.org (BSD-licensed, © The Tor Project).

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Pinned stable Expert Bundle. Bump when Tor ships a newer 15.x/16.x stable.
pub const TOR_BUNDLE_VERSION: &str = "15.0.21";

pub fn expert_bundle_url_for(os: &str, arch: &str) -> String {
    format!(
        "https://dist.torproject.org/torbrowser/{ver}/tor-expert-bundle-{os}-{arch}-{ver}.tar.gz",
        ver = TOR_BUNDLE_VERSION
    )
}

pub fn expert_bundle_url() -> String {
    let (os, arch) = bundle_target();
    expert_bundle_url_for(os, arch)
}

pub fn bundle_target() -> (&'static str, &'static str) {
    let os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "x86") {
        "i686"
    } else {
        "x86_64"
    };
    (os, arch)
}

/// Where we keep the unpacked `tor` / `tor.exe` for this user.
pub fn tor_cache_dir() -> PathBuf {
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(local).join("home_builder_pay").join("tor");
    }
    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        #[cfg(target_os = "windows")]
        {
            return PathBuf::from(home)
                .join("AppData")
                .join("Local")
                .join("home_builder_pay")
                .join("tor");
        }
        #[cfg(not(target_os = "windows"))]
        {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("home_builder_pay")
                .join("tor");
        }
    }
    PathBuf::from("home_builder_pay_tor")
}

pub fn find_tor_in_dir(root: &Path) -> Option<PathBuf> {
    if !root.exists() {
        return None;
    }
    fn walk(dir: &Path, depth: u8) -> Option<PathBuf> {
        if depth > 6 {
            return None;
        }
        let rd = std::fs::read_dir(dir).ok()?;
        let mut dirs = Vec::new();
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name();
            let name = name.to_string_lossy();
            if p.is_file() && (name == "tor.exe" || name == "tor") {
                return Some(p);
            }
            if p.is_dir() && name != "." && name != ".." {
                dirs.push(p);
            }
        }
        for d in dirs {
            if let Some(hit) = walk(&d, depth + 1) {
                return Some(hit);
            }
        }
        None
    }
    walk(root, 0)
}

/// Unpack a `.tar.gz` Expert Bundle into `dest`. Rejects `..` path components.
pub fn extract_expert_bundle(tgz: &Path, dest: &Path) -> crate::Result<PathBuf> {
    fs::create_dir_all(dest)?;
    let f = File::open(tgz)?;
    let dec = flate2::read::GzDecoder::new(f);
    let mut ar = tar::Archive::new(dec);
    for entry in ar.entries()? {
        let mut entry = entry?;
        let rel = entry.path()?.into_owned();
        if path_has_parent(&rel) {
            return Err(crate::Error::msg("refusing tar path with .."));
        }
        let _ = entry.unpack_in(dest)?;
    }
    find_tor_in_dir(dest).ok_or_else(|| {
        crate::Error::msg("el paquete de Tor no trae tor.exe (¿archivo incompleto?)")
    })
}

fn path_has_parent(p: &Path) -> bool {
    p.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
}

pub fn download_expert_bundle(
    dest_dir: &Path,
    mut progress: impl FnMut(&str),
) -> crate::Result<PathBuf> {
    if let Some(existing) = find_tor_in_dir(dest_dir) {
        return Ok(existing);
    }
    if std::env::var("HBP_SKIP_TOR_DOWNLOAD").ok().as_deref() == Some("1") {
        return Err(crate::Error::msg("Tor download skipped (HBP_SKIP_TOR_DOWNLOAD)"));
    }
    fs::create_dir_all(dest_dir)?;
    let url = expert_bundle_url();
    progress("Descargando Tor (oficial, primera vez)…");
    let resp = ureq::get(&url)
        .timeout(std::time::Duration::from_secs(180))
        .call()
        .map_err(|e| crate::Error::msg(format!("no pude bajar Tor: {e}")))?;
    if resp.status() != 200 {
        return Err(crate::Error::msg(format!(
            "no pude bajar Tor (HTTP {})",
            resp.status()
        )));
    }
    let tgz = dest_dir.join(format!("expert-bundle-{TOR_BUNDLE_VERSION}.tar.gz"));
    let mut file = File::create(&tgz)?;
    let mut reader = resp.into_reader();
    std::io::copy(&mut reader, &mut file)?;
    file.flush()?;
    drop(file);
    progress("Desempaquetando Tor…");
    let unpacked = dest_dir.join("unpacked");
    extract_expert_bundle(&tgz, &unpacked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_windows_url() {
        let u = expert_bundle_url_for("windows", "x86_64");
        assert!(u.starts_with("https://dist.torproject.org/torbrowser/"));
        assert!(u.contains("tor-expert-bundle-windows-x86_64-"));
        assert!(u.ends_with(".tar.gz"));
        assert!(!u.contains("alpha"));
    }

    #[test]
    fn extract_tiny_tarball_finds_tor_exe() {
        let dir = std::env::temp_dir().join(format!("hbp-tgz-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let tgz = dir.join("mini.tar.gz");
        {
            let f = File::create(&tgz).unwrap();
            let enc = flate2::write::GzEncoder::new(f, flate2::Compression::fast());
            let mut b = tar::Builder::new(enc);
            let mut header = tar::Header::new_gnu();
            header.set_size(3);
            header.set_cksum();
            header.set_mode(0o755);
            b.append_data(&mut header, "tor/tor.exe", &b"tor"[..]).unwrap();
            b.finish().unwrap();
        }
        let unpacked = dir.join("out");
        let found = extract_expert_bundle(&tgz, &unpacked).unwrap();
        assert!(found.ends_with("tor.exe"));
        assert_eq!(fs::read(&found).unwrap(), b"tor");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_parent_dir_components() {
        assert!(path_has_parent(Path::new("../evil")));
        assert!(path_has_parent(Path::new("tor/../evil")));
        assert!(!path_has_parent(Path::new("tor/tor.exe")));
    }
}
