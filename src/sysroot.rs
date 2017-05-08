//! Download and manage sysroots.

use std::env;
use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ffi::OsStr;

use chrono::{TimeZone, UTC};
use flate2::bufread::GzDecoder;
use reqwest;
use tar::Archive;

use git::Commit;

use errors::{Result, ResultExt};

pub struct Sysroot {
    pub sha: String,
    pub rustc: PathBuf,
    pub cargo: PathBuf,
    pub triple: String,
    pub preserve: bool,
}

impl Sysroot {
    // if running with cargo run, we want to avoid things like CARGO_INCREMENTAL
    // sneaking into the command's environment, but we do need the PATH to
    // find linkers and other things that cargo and rust needs.
    pub fn command<P: AsRef<Path>>(&self, path: P) -> Command {
        let mut command = Command::new(path.as_ref().as_os_str());
        command
            .env_clear()
            .env("PATH", env::var("PATH").unwrap_or_default())
            .env("CARGO_HOME", env::var("CARGO_HOME").unwrap_or_default())
            .env("CARGO", &self.cargo)
            .env("CARGO_RELATIVE", &self.cargo.strip_prefix(&env::current_dir().unwrap()).unwrap())
            .env("RUSTC", &self.rustc)
            .env("RUSTC_RELATIVE", &self.rustc.strip_prefix(&env::current_dir().unwrap()).unwrap());
        command
    }

    pub fn install(commit: &Commit, triple: &str, preserve: bool) -> Result<Self> {
        let sha : &str = &commit.sha();
        let unpack_into = format!("rust-{}", sha);

        let cargo_sha = if commit.date < UTC.ymd(2017, 3, 20).and_hms(0, 0, 0) {
            // Versions of rustc older than Mar 20 have bugs in
            // their cargo. Use a known-good cargo for older rustcs
            // instead.
            warn!("using fallback cargo");
            "53eb08bedc8719844bb553dbe1a39d9010783ff5"
        } else {
            sha
        };

        if !Path::new(&unpack_into).exists() {
            get_and_extract("rustc", sha, triple, &unpack_into, false)?;
            get_and_extract("rust-std", sha, triple, &unpack_into, true)?;
            get_and_extract("cargo", cargo_sha, triple, &unpack_into, false)?;
        }

        let result = Sysroot {
            rustc: PathBuf::from(format!("rust-{}/rustc/bin/rustc", sha)).canonicalize()
                .chain_err(|| format!("failed to canonicalize rustc path for {}", sha))?,
            cargo: PathBuf::from(format!("rust-{}/cargo/bin/cargo", sha)).canonicalize()
                .chain_err(|| format!("failed to canonicalize cargo path for {}", sha))?,
            sha: sha.to_owned(),
            preserve: preserve,
            triple: triple.to_string(),
        };

        let version = result.command(&result.rustc).arg("--version").output()
            .chain_err(|| format!("{} --version", result.rustc.display()))?;
        let version = String::from_utf8(version.stdout).unwrap();
        debug!("version: {}", version.trim());

        Ok(result)
    }

    pub fn path(&self) -> String {
        format!("rust-{}", self.sha)
    }
}

impl Drop for Sysroot {
    fn drop(&mut self) {
        if self.preserve {
            return
        }

        fs::remove_dir_all(&self.path()).unwrap_or_else(|err| {
            info!("failed to remove {}, please do so manually: {:?}", self.path(), err);
        });
    }
}

const MODULE_URLS: &'static [&'static str] = &[
    "https://s3.amazonaws.com/rust-lang-ci/rustc-builds/@SHA@/@MODULE@-nightly-@TRIPLE@.tar.gz",
    "https://s3.amazonaws.com/rust-lang-ci/rustc-builds/@SHA@/dist/@MODULE@-nightly-@TRIPLE@.tar.gz",
    "https://s3.amazonaws.com/rust-lang-ci/rustc-builds/@SHA@/@MODULE@-1.16.0-dev-@TRIPLE@.tar.gz",
];

fn get_module(module: &str, sha: &str, triple: &str) -> Result<reqwest::Response> {
    for url in MODULE_URLS {
        let url = url.replace("@MODULE@", module).replace("@SHA@", sha).replace("@TRIPLE@", triple);
        debug!("requesting: {}", url);
        let resp = reqwest::get(&url)?;
        debug!("{}", resp.status());
        if resp.status().is_success() {
            return Ok(resp);
        }
    }
    bail!("unable to download sha {} triple {} module {}", sha, triple, module);
}

fn get_and_extract(module: &str, sha: &str, triple: &str, into: &str, is_std: bool) -> Result<()> {
    let resp = get_module(module, sha, triple)?;
    let mut resp = BufReader::new(resp);

    let decoder = GzDecoder::new(&mut resp)?;
    let mut archive = Archive::new(decoder);
    let std_prefix = format!("rust-std-{}/lib/rustlib", triple);

    let mut to_link = Vec::new();

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let mut components = path.components();
        assert!(components.next().is_some(), "strip container directory");
        let path = components.as_path();

        let path = if is_std {
            if let Ok(path) = path.strip_prefix(&std_prefix) {
                if path.extension() == Some(OsStr::new("dylib")) {
                    to_link.push(path.to_owned());
                    continue;
                } else {
                    Path::new("rustc/lib/rustlib").join(path)
                }
            } else {
                continue;
            }
        } else {
            path.into()
        };
        let path = Path::new(into).join(path);
        fs::create_dir_all(&path.parent().unwrap())
            .chain_err(|| format!("could not create intermediate directories for {}",
                    path.display()))?;
        entry.unpack(path)?;
    }

    let link_dst_prefix = format!("{}/rustc/lib/rustlib/{}/lib", into, triple);
    let link_src_prefix = format!("{}/lib", triple);
    for path in to_link {
        let src = Path::new(into).join("rustc/lib").join(path.strip_prefix(&link_src_prefix)
            .chain_err(|| format!("stripping prefix from: {:?}", path))?);
        let dst = Path::new(&link_dst_prefix).join(&path);
        fs::create_dir_all(&dst.parent().unwrap())
            .chain_err(|| format!("could not create intermediate directories for {}", dst.display()))?;
        debug!("linking {} to {}", src.display(), dst.display());
        fs::hard_link(src, dst)?;
    }

    Ok(())
}
