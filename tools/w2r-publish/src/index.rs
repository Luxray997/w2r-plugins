//! Assembling `index.json` and checking one.
//!
//! Published versions are immutable: a version already in the previous index keeps its published
//! entry (url, hash, size) even when a fresh build of the same version differs byte for byte.
//! Shipping a change therefore needs a version bump. The previous index must be one the caller
//! has already checked (the release workflow runs `verify` on it first).

use crate::Result;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use w2r_formats::semver::Version;
use w2r_loader_lib::index::{format_time, parse_index};

/// The oldest manager that can install what this tool publishes.
const MIN_MANAGER: &str = "0.1.0";

pub struct Options {
    pub packages: PathBuf,
    /// Package URL with `{name}` standing for `<id>-<version>`.
    pub url_template: String,
    pub out: PathBuf,
    pub previous: Option<PathBuf>,
    pub now: Option<i64>,
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn packaged_manifest(zip_bytes: &[u8], file: &str) -> Result<Value> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes))
        .map_err(|e| format!("{file}: {e}"))?;
    let mut entry = zip
        .by_name("plugin.json")
        .map_err(|_| format!("{file}: no plugin.json"))?;
    let mut raw = Vec::new();
    entry
        .by_ref()
        .take(65_537)
        .read_to_end(&mut raw)
        .map_err(|e| format!("{file}: {e}"))?;
    serde_json::from_slice(&raw).map_err(|e| format!("{file}: plugin.json: {e}"))
}

pub fn assemble(o: &Options) -> Result<()> {
    let now = o.now.unwrap_or_else(unix_now);

    // id -> version text -> entry
    let mut plugins: BTreeMap<String, BTreeMap<String, Value>> = BTreeMap::new();
    let mut serial = 1;
    if let Some(prev) = &o.previous {
        let doc: Value = serde_json::from_slice(
            &fs::read(prev).map_err(|e| format!("{}: {e}", prev.display()))?,
        )
        .map_err(|e| format!("previous index: {e}"))?;
        serial = doc["serial"].as_u64().ok_or("previous index: no serial")? + 1;
        for p in doc["plugins"].as_array().ok_or("previous index: no plugins")? {
            let id = p["id"].as_str().ok_or("previous index: bad id")?;
            for v in p["versions"].as_array().ok_or("previous index: bad versions")? {
                let ver = v["version"].as_str().ok_or("previous index: bad version")?;
                plugins
                    .entry(id.to_owned())
                    .or_default()
                    .insert(ver.to_owned(), v.clone());
            }
        }
    }

    let mut files: Vec<_> = fs::read_dir(&o.packages)
        .map_err(|e| format!("{}: {e}", o.packages.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "zip"))
        .collect();
    files.sort();
    for path in files {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = fs::read(&path).map_err(|e| format!("{file}: {e}"))?;
        let manifest = packaged_manifest(&bytes, &file)?;
        let id = manifest["id"].as_str().ok_or_else(|| format!("{file}: no id"))?;
        let ver = manifest["version"]
            .as_str()
            .ok_or_else(|| format!("{file}: no version"))?;
        if file != format!("{id}-{ver}.zip") {
            return Err(format!("{file}: file name does not match id and version"));
        }
        let versions = plugins.entry(id.to_owned()).or_default();
        if let Some(old) = versions.get(ver) {
            if old["sha256"].as_str() != Some(&sha256_hex(&bytes)) {
                println!("{id} {ver}: already published, keeping the published entry (bump the version to ship changes)");
            }
            continue;
        }
        println!("{id} {ver}: new");
        versions.insert(
            ver.to_owned(),
            json!({
                "version": ver,
                "manifest": manifest,
                "url": o.url_template.replace("{name}", file.trim_end_matches(".zip")),
                "sha256": sha256_hex(&bytes),
                "size": bytes.len(),
                "min_manager": MIN_MANAGER,
            }),
        );
    }

    let list: Vec<Value> = plugins
        .into_iter()
        .map(|(id, versions)| {
            let mut v: Vec<(Version, Value)> = versions
                .into_iter()
                .filter_map(|(k, e)| Version::parse(&k).map(|k| (k, e)))
                .collect();
            v.sort_by(|a, b| a.0.cmp(&b.0));
            json!({ "id": id, "versions": v.into_iter().map(|x| x.1).collect::<Vec<_>>() })
        })
        .collect();
    let doc = json!({
        "schema": 1,
        "serial": serial,
        "generated": format_time(now),
        "plugins": list,
    });
    let raw = serde_json::to_vec(&doc).map_err(|e| e.to_string())?;
    // Our own output must pass the manager's rules before it is published.
    parse_index(&raw, 0).map_err(|r| format!("assembled index fails the manager's rules: {r}"))?;
    fs::write(&o.out, &raw).map_err(|e| format!("{}: {e}", o.out.display()))?;
    println!("{}: serial {serial}, {} bytes", o.out.display(), raw.len());
    Ok(())
}

/// Parses `index` with the manager's rules; `min_serial` is the serial it must not fall below.
pub fn verify(index: &Path, min_serial: u64) -> Result<()> {
    let raw = fs::read(index).map_err(|e| format!("{}: {e}", index.display()))?;
    let idx = parse_index(&raw, min_serial).map_err(|r| format!("index refused: {r}"))?;
    let versions: usize = idx.plugins.iter().map(|p| p.versions.len()).sum();
    println!(
        "ok: serial {}, {} plugin(s), {versions} version(s), generated {}",
        idx.serial,
        idx.plugins.len(),
        format_time(idx.generated)
    );
    Ok(())
}
