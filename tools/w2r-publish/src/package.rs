//! Builds `<id>-<version>.zip` from the compiled DLL and the plugin folder. Only the files the
//! manager's archive rules accept are written: `plugin.json`, the entry DLL, `README.md`,
//! `LICENSE` (docs/index.md in the w2rpm repo). The zip is deterministic (fixed timestamps, fixed
//! order), so the same inputs give the same hash.

use crate::Result;
use crate::repo::Plugin;
use std::fs;
use std::io::Write;
use std::path::Path;
use zip::write::SimpleFileOptions;

pub fn build_all(plugins: &[Plugin], target_dir: &Path, out: &Path) -> Result<()> {
    fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    for p in plugins {
        let m = &p.manifest;
        let dll_path = target_dir.join(&m.entry);
        let dll = fs::read(&dll_path).map_err(|e| format!("{}: {e}", dll_path.display()))?;
        let mut files: Vec<(&str, Vec<u8>)> = vec![
            ("plugin.json", p.manifest_raw.clone()),
            (m.entry.as_str(), dll),
        ];
        for extra in ["README.md", "LICENSE"] {
            if let Ok(bytes) = fs::read(p.dir.join(extra)) {
                files.push((extra, bytes));
            }
        }
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default());
        for (name, bytes) in files {
            zip.start_file(name, opts).map_err(|e| e.to_string())?;
            zip.write_all(&bytes).map_err(|e| e.to_string())?;
        }
        let bytes = zip.finish().map_err(|e| e.to_string())?.into_inner();
        let file = out.join(format!("{}-{}.zip", m.id, m.version_text));
        fs::write(&file, &bytes).map_err(|e| format!("{}: {e}", file.display()))?;
        println!("{}  {} bytes", file.display(), bytes.len());
    }
    Ok(())
}
