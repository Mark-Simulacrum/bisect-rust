//! Download and manage sysroots.

use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, Read, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ffi::OsStr;

use chrono::{TimeZone, UTC};
use flate2::bufread::GzDecoder;
use xz2::bufread::XzDecoder;
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
            .env("CARGO", &self.cargo)
            .env("CARGO_RELATIVE", &self.cargo.strip_prefix(&env::current_dir().unwrap()).unwrap())
            .env("RUSTC", &self.rustc)
            .env("RUSTC_RELATIVE", &self.rustc.strip_prefix(&env::current_dir().unwrap()).unwrap());
        command
    }

    pub fn install(commit: &Commit, triple: &str, preserve: bool) -> Result<Self> {
        let sha: &str = &commit.sha;
        let unpack_into = format!("cache");

        let cargo_sha = if commit.date < UTC.ymd(2017, 3, 20).and_hms(0, 0, 0) {
            // Versions of rustc older than Mar 20 have bugs in
            // their cargo. Use a known-good cargo for older rustcs
            // instead.
            warn!("using fallback cargo");
            "53eb08bedc8719844bb553dbe1a39d9010783ff5"
        } else {
            sha
        };

        fs::create_dir_all(&unpack_into)?;

        let download = SysrootDownload {
            directory: unpack_into.into(),
            save_download: preserve,
            rust_sha: sha.to_string(),
            cargo_sha: cargo_sha.to_string(),
            triple: triple.to_string(),
        };

        download.get_and_extract("rustc", false)?;
        download.get_and_extract("rust-std", true)?;
        download.get_and_extract("cargo", false)?;

        download.into_sysroot()
    }
}

impl Drop for Sysroot {
    fn drop(&mut self) {
        fs::remove_dir_all(format!("cache/{}", self.sha)).unwrap_or_else(|err| {
            info!("failed to remove {:?}, please do so manually: {:?}",
                format!("cache/{}", self.sha), err);
        });
    }
}

struct SysrootDownload {
    directory: PathBuf,
    save_download: bool,
    rust_sha: String,
    cargo_sha: String,
    triple: String,
}

const MODULE_URLS: &[&str] = &[
    "https://s3.amazonaws.com/rust-lang-ci/rustc-builds/@SHA@/@MODULE@-nightly-@TRIPLE@.tar.xz",
    "https://s3.amazonaws.com/rust-lang-ci/rustc-builds/@SHA@/@MODULE@-nightly-@TRIPLE@.tar.gz",
    "https://s3.amazonaws.com/rust-lang-ci/rustc-builds/@SHA@/dist/@MODULE@-nightly-@TRIPLE@.tar.gz",
    "https://s3.amazonaws.com/rust-lang-ci/rustc-builds/@SHA@/@MODULE@-1.16.0-dev-@TRIPLE@.tar.gz",
];

impl SysrootDownload {
    fn into_sysroot(self) -> Result<Sysroot> {
        Ok(Sysroot {
            rustc: self.directory.join(&self.rust_sha).join("rustc/bin/rustc").canonicalize()
                .chain_err(|| format!("failed to canonicalize rustc path for {}", self.rust_sha))?,
            cargo: self.directory.join(&self.rust_sha).join("cargo/bin/cargo").canonicalize()
                .chain_err(|| format!("failed to canonicalize cargo path for {}", self.cargo_sha))?,
            sha: self.rust_sha,
            preserve: self.save_download,
            triple: self.triple,
        })
    }

    fn sha<'a>(&'a self, module: &str) -> &'a str {
        if module == "cargo" {
            &self.cargo_sha
        } else {
            &self.rust_sha
        }
    }

    fn get_module(&self, module: &str) -> Result<Box<Read>> {
        info!("Getting {} for {}", module, self.sha(module));
        for url in MODULE_URLS {
            let url = url
                .replace("@MODULE@", module)
                .replace("@SHA@", self.sha(module))
                .replace("@TRIPLE@", &self.triple);

            let extension = if url.ends_with("gz") { "gz" } else { "xz" };
            let archive_path = self.directory.join(format!("{}-{}.tar.{}",
                    self.sha(module), module, extension));

            let mut reader: Box<BufRead> = if archive_path.exists() {
                Box::new(BufReader::new(File::open(&archive_path)?))
            } else {
                debug!("requesting: {}", url);
                let resp = reqwest::get(&url)?;
                debug!("{}", resp.status());
                if resp.status().is_success() {
                    Box::new(BufReader::new(resp))
                } else {
                    continue;
                }
            };

            let reader: Box<BufRead> = if self.save_download && !archive_path.exists() {
                let mut file = File::create(&archive_path)?;
                io::copy(&mut reader, &mut file)?;
                Box::new(BufReader::new(File::open(&archive_path)?))
            } else {
                reader
            };

            let reader: Box<Read> = if extension == "gz" {
                Box::new(GzDecoder::new(reader)?)
            } else if extension == "xz" {
                Box::new(XzDecoder::new(reader))
            } else {
                bail!("unknown file extension on URL: {:?}", url);
            };
            return Ok(reader);
        }
        bail!("unable to download sha {} triple {} module {}", self.sha(module), self.triple, module);
    }

    fn get_and_extract(&self, module: &str, is_std: bool) -> Result<()> {
        let reader = self.get_module(module)?;
        let mut archive = Archive::new(reader);
        let std_prefix = format!("rust-std-{}/lib/rustlib", self.triple);

        let mut to_link = Vec::new();

        let unpack_into = self.directory.join(&self.rust_sha);

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
            let path = unpack_into.join(path);
            fs::create_dir_all(&path.parent().unwrap())
                .chain_err(|| format!("could not create intermediate directories for {}",
                        path.display()))?;
            entry.unpack(path)?;
        }

        let link_dst_prefix = unpack_into.join(format!("rustc/lib/rustlib/{}/lib", self.triple));
        let link_src_prefix = format!("{}/lib", self.triple);
        for path in to_link {
            let src = unpack_into.join("rustc/lib").join(path.strip_prefix(&link_src_prefix)
                .chain_err(|| format!("stripping prefix from: {:?}", path))?);
            let dst = link_dst_prefix.join(&path);
            fs::create_dir_all(&dst.parent().unwrap())
                .chain_err(|| format!("could not create intermediate directories for {}", dst.display()))?;
            debug!("linking {} to {}", src.display(), dst.display());
            fs::hard_link(src, dst)?;
        }

        Ok(())
    }
}
