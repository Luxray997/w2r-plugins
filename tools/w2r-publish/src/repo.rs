//! Reads `plugins/*` and checks what CI can check before anything is built.

use crate::Result;
use std::fs;
use std::path::{Path, PathBuf};
use w2r_formats::manifest::{self, Manifest};

pub struct Plugin {
    pub dir: PathBuf,
    pub manifest: Manifest,
    /// The exact bytes of `plugin.json`, packaged unchanged.
    pub manifest_raw: Vec<u8>,
}

/// Every folder under `plugins/` must be a plugin: a valid `plugin.json` whose id is the folder
/// name and whose version is the crate version, so a release cannot ship a stale manifest.
pub fn load(root: &Path) -> Result<Vec<Plugin>> {
    let dir = root.join("plugins");
    let mut names: Vec<_> = fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut out = Vec::new();
    for name in names {
        let pdir = dir.join(&name);
        let raw =
            fs::read(pdir.join("plugin.json")).map_err(|e| format!("{name}: plugin.json: {e}"))?;
        let m = manifest::validate(&raw).map_err(|r| format!("{name}: plugin.json: {r}"))?;
        if m.id != name {
            return Err(format!(
                "{name}: manifest id `{}` must equal the folder name",
                m.id
            ));
        }
        let cargo = fs::read_to_string(pdir.join("Cargo.toml"))
            .map_err(|e| format!("{name}: Cargo.toml: {e}"))?;
        match package_version(&cargo) {
            Some(v) if v == m.version_text => {}
            Some(v) => {
                return Err(format!(
                    "{name}: Cargo.toml version {v} differs from plugin.json version {}",
                    m.version_text
                ));
            }
            None => return Err(format!("{name}: no version in Cargo.toml [package]")),
        }
        out.push(Plugin {
            dir: pdir,
            manifest: m,
            manifest_raw: raw,
        });
    }
    if out.is_empty() {
        return Err("no plugins found".into());
    }
    Ok(out)
}

/// `version = "x"` inside `[package]`; enough for a manifest we require to be plain.
fn package_version(cargo: &str) -> Option<String> {
    let mut in_package = false;
    for line in cargo.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package && let Some(rest) = line.strip_prefix("version") {
            let rest = rest.trim_start().strip_prefix('=')?.trim();
            return rest.strip_prefix('"')?.split('"').next().map(str::to_owned);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::package_version;

    #[test]
    fn reads_the_package_version_only() {
        let t = "[package]\nname = \"x\"\nversion = \"1.2.3\"\n[dependencies]\nfoo = { version = \"9\" }\n";
        assert_eq!(package_version(t).as_deref(), Some("1.2.3"));
        assert_eq!(package_version("[dependencies]\nversion = \"9\"\n"), None);
    }
}
