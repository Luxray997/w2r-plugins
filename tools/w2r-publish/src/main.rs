//! Release tooling for the plugin repository (docs/PIPELINE.md).
//!
//! ```text
//! w2r-publish check
//! w2r-publish package --target-dir target/i686-pc-windows-msvc/release --out out/packages
//! w2r-publish index   --packages out/packages --url-template TEMPLATE --out out/index.json
//!                     (TEMPLATE has {name} for id-version)
//!                     [--previous index.json] [--days 90] [--next-key BODY] [--now UNIX]
//! w2r-publish verify  --index index.json --sig index.json.sig --key keys/a.pub [--key ...]
//! ```
//!
//! This tool never signs anything. Signing is an offline step done with the real `minisign`
//! (docs/SIGNING.md); `verify` is what the signer and CI run afterwards, with the same code the
//! manager uses.

mod index;
mod package;
mod repo;

use std::collections::HashMap;
use std::process::ExitCode;

pub type Result<T> = std::result::Result<T, String>;

/// `--name value` pairs; a name may repeat (`--key`).
struct Args(HashMap<String, Vec<String>>);

impl Args {
    fn parse(items: &[String]) -> Result<Args> {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        let mut it = items.iter();
        while let Some(name) = it.next() {
            let name = name
                .strip_prefix("--")
                .ok_or_else(|| format!("unexpected argument `{name}`"))?;
            let value = it
                .next()
                .ok_or_else(|| format!("--{name} needs a value"))?;
            map.entry(name.to_owned()).or_default().push(value.clone());
        }
        Ok(Args(map))
    }

    fn one(&self, name: &str) -> Result<&str> {
        match self.0.get(name).map(Vec::as_slice) {
            Some([v]) => Ok(v),
            Some(_) => Err(format!("--{name} given more than once")),
            None => Err(format!("--{name} is required")),
        }
    }

    fn opt(&self, name: &str) -> Result<Option<&str>> {
        if self.0.contains_key(name) {
            self.one(name).map(Some)
        } else {
            Ok(None)
        }
    }

    fn num<T: std::str::FromStr>(&self, name: &str) -> Result<Option<T>> {
        self.opt(name)?
            .map(|v| v.parse::<T>().map_err(|_| format!("bad --{name}")))
            .transpose()
    }

    fn all(&self, name: &str) -> &[String] {
        self.0.get(name).map_or(&[], Vec::as_slice)
    }

    fn known(&self, allowed: &[&str]) -> Result<()> {
        match self.0.keys().find(|k| !allowed.contains(&k.as_str())) {
            Some(k) => Err(format!("unknown option --{k}")),
            None => Ok(()),
        }
    }
}

fn run() -> Result<()> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, rest) = argv
        .split_first()
        .ok_or("usage: w2r-publish check|package|index|verify")?;
    let args = Args::parse(rest)?;
    match cmd.as_str() {
        "check" => {
            args.known(&["root"])?;
            let root = args.opt("root")?.unwrap_or(".");
            let plugins = repo::load(root.as_ref())?;
            for p in &plugins {
                println!("ok  {} {}", p.manifest.id, p.manifest.version_text);
                if p.manifest.capabilities.iter().any(|c| c == "hooks.raw") {
                    println!("    note: hooks.raw, needs the extra review in CONTRIBUTING.md");
                }
            }
            println!("{} plugin(s) checked", plugins.len());
            Ok(())
        }
        "package" => {
            args.known(&["root", "target-dir", "out"])?;
            let root = args.opt("root")?.unwrap_or(".");
            let plugins = repo::load(root.as_ref())?;
            package::build_all(
                &plugins,
                args.one("target-dir")?.as_ref(),
                args.one("out")?.as_ref(),
            )
        }
        "index" => {
            args.known(&[
                "packages", "url-template", "out", "previous", "days", "next-key", "now",
            ])?;
            index::assemble(&index::Options {
                packages: args.one("packages")?.into(),
                url_template: args.one("url-template")?.to_owned(),
                out: args.one("out")?.into(),
                previous: args.opt("previous")?.map(Into::into),
                days: args.num("days")?.unwrap_or(90),
                next_key: args.opt("next-key")?.map(str::to_owned),
                now: args.num("now")?,
            })
        }
        "verify" => {
            args.known(&["index", "sig", "key", "now", "min-serial"])?;
            index::verify_signed(
                args.one("index")?.as_ref(),
                args.one("sig")?.as_ref(),
                args.all("key"),
                args.num("now")?,
                args.num("min-serial")?.unwrap_or(0),
            )
        }
        other => Err(format!("unknown command `{other}`")),
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
