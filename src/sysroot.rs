//! Download and manage sysroots.

use std::env;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufRead, Read, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ffi::OsStr;

use chrono::{TimeZone, Utc};
use flate2::bufread::GzDecoder;
use xz2::bufread::XzDecoder;
use reqwest;
use tar::Archive;

use git::Commit;

use errors::{Result, ResultExt};

pub struct Sysroot {
    pub sha: String,
    pub rustc: PathBuf,
    pub rustdoc: PathBuf,
    pub cargo: PathBuf,
    pub triple: String,
    pub preserve: bool,
    pub used_fallback_cargo: bool,
    pub is_saving_sysroot: bool,
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
            .env("RUSTC_RELATIVE", &self.rustc.strip_prefix(&env::current_dir().unwrap()).unwrap())
            .env("RUSTDOC", &self.rustdoc)
            .env("RUSTDOC_RELATIVE", &self.rustdoc.strip_prefix(&env::current_dir().unwrap()).unwrap());
        command
    }

    pub fn with_local_rustc(commit: &Commit, rustc: &str, triple: &str, preserve: bool, is_saving_sysroot: bool) -> Result<Self> {
        let sha: &str = &commit.sha;
        let unpack_into = format!("cache");
        let mut used_fallback_cargo = false;

        let cargo_sha = if commit.date < Utc.ymd(2017, 3, 20).and_hms(0, 0, 0) {
            // Versions of rustc older than Mar 20 have bugs in
            // their cargo. Use a known-good cargo for older rustcs
            // instead.
            used_fallback_cargo = true;
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

        download.get_and_extract("cargo")?;

        Ok(Sysroot {
            rustc: PathBuf::from(rustc).canonicalize()
                .chain_err(|| format!("failed to canonicalize rustc path: {}", rustc))?,
            rustdoc: PathBuf::from(rustc).canonicalize()
                .chain_err(|| format!("failed to canonicalize rustc path: {}", rustc))?
                .parent().unwrap().join("rustdoc"),
            cargo: download.directory.join(&download.rust_sha).join("cargo/bin/cargo").canonicalize()
                .chain_err(|| format!("failed to canonicalize cargo path for {}", download.cargo_sha))?,
            sha: download.rust_sha,
            preserve: download.save_download,
            triple: download.triple,
            used_fallback_cargo,
            is_saving_sysroot,
        })
    }

    pub fn install(commit: &Commit, triple: &str, preserve: bool, is_saving_sysroot: bool) -> Result<Self> {
        let sha: &str = &commit.sha;
        let unpack_into = format!("cache");
        let mut used_fallback_cargo = false;

        let cargo_sha = if commit.date < Utc.ymd(2017, 3, 20).and_hms(0, 0, 0) {
            // Versions of rustc older than Mar 20 have bugs in
            // their cargo. Use a known-good cargo for older rustcs
            // instead.
            used_fallback_cargo = true;
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

        download.get_and_extract("rustc")?;
        download.get_and_extract("rust-std")?;
        download.get_and_extract("cargo")?;

        download.into_sysroot(used_fallback_cargo, is_saving_sysroot)
    }
}

impl Drop for Sysroot {
    fn drop(&mut self) {
        if !self.is_saving_sysroot {
            fs::remove_dir_all(format!("cache/{}", self.sha)).unwrap_or_else(|err| {
                info!("failed to remove {:?}, please do so manually: {:?}",
                    format!("cache/{}", self.sha), err);
            });
        }
    }
}

#[derive(Debug, Clone)]
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
    "https://s3.amazonaws.com/rust-lang-ci/rustc-builds-try/@SHA@/@MODULE@-nightly-@TRIPLE@.tar.xz",
];

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum ModuleVariant {
    Cargo,
    Rustc,
    Std
}

impl fmt::Display for ModuleVariant {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ModuleVariant::Cargo => write!(f, "cargo"),
            ModuleVariant::Rustc => write!(f, "rustc"),
            ModuleVariant::Std => write!(f, "rust-std"),
        }
    }
}

#[derive(Debug, Copy, Clone)]
struct Module<'a> {
    variant: ModuleVariant,
    sysroot: &'a SysrootDownload,
}

impl<'a> Module<'a> {
    fn sha(&self) -> &str {
        match self.variant {
            ModuleVariant::Cargo => &self.sysroot.cargo_sha,
            _ => &self.sysroot.rust_sha,
        }
    }

    fn urls(&self) -> Vec<String> {
        MODULE_URLS.iter().map(|url| {
            url.replace("@MODULE@", &self.variant.to_string())
               .replace("@SHA@", self.sha())
               .replace("@TRIPLE@", &self.sysroot.triple)
        }).collect()
    }

    fn decompress<'b, R: BufRead + 'b>(&self, reader: R, extension: &str) -> Result<Box<Read + 'b>> {
        if extension == "gz" {
            Ok(Box::new(GzDecoder::new(reader)?))
        } else if extension == "xz" {
            Ok(Box::new(XzDecoder::new(reader)))
        } else {
            bail!("unknown extension {}", extension);
        }
    }

    fn get(&self) -> Result<()> {
        let archive_path = |extension| {
            self.sysroot.directory.join(format!("{}-{}-{}.tar.{}",
                self.sha(), self.sysroot.triple, self.variant, extension))
        };
        for &extension in &["xz", "gz"] {
            let archive_path = archive_path(extension);

            let reader = if archive_path.exists() {
                BufReader::new(File::open(&archive_path)?)
            } else {
                continue;
            };
            match self.decompress(reader, extension)
                .and_then(|reader| self.sysroot.extract(self, reader)) {
                Ok(()) => return Ok(()),
                Err(err) => {
                    warn!("extracting {} failed: {:?}", archive_path.display(), err);
                    fs::remove_file(archive_path)?;
                    continue;
                }
            }
        }

        for url in self.urls() {
            let extension = if url.ends_with("gz") { "gz" } else { "xz" };

            debug!("requesting: {}", url);
            let resp = reqwest::get(&url)?;
            debug!("{}", resp.status());
            let mut reader = if resp.status().is_success() {
                BufReader::new(resp)
            } else {
                continue;
            };
            let archive_path = archive_path(extension);

            let reader: Box<BufRead> = if self.sysroot.save_download && !archive_path.exists() {
                let mut file = File::create(&archive_path)?;
                io::copy(&mut reader, &mut file)?;
                Box::new(BufReader::new(File::open(&archive_path)?))
            } else {
                Box::new(reader)
            };

            match self.decompress(reader, extension)
                .and_then(|reader| self.sysroot.extract(self, reader)) {
                Ok(()) => return Ok(()),
                Err(err) => {
                    warn!("extracting {} failed: {:?}", url, err);
                    if self.sysroot.save_download {
                        fs::remove_file(archive_path)?;
                    }
                    continue;
                }
            }
        }

        bail!("unable to download sha {} triple {} module {}",
            self.sha(), self.sysroot.triple, self.variant);
    }
}

impl SysrootDownload {
    fn into_sysroot(self, used_fallback_cargo: bool, is_saving_sysroot: bool) -> Result<Sysroot> {
        Ok(Sysroot {
            rustc: self.directory.join(&self.rust_sha).join("rustc/bin/rustc").canonicalize()
                .chain_err(|| format!("failed to canonicalize rustc path for {}", self.rust_sha))?,
            rustdoc: self.directory.join(&self.rust_sha).join("rustc/bin/rustdoc").canonicalize()
                .chain_err(|| format!("failed to canonicalize rustdoc path for {}", self.rust_sha))?,
            cargo: self.directory.join(&self.rust_sha).join("cargo/bin/cargo").canonicalize()
                .chain_err(|| format!("failed to canonicalize cargo path for {}", self.cargo_sha))?,
            sha: self.rust_sha,
            preserve: self.save_download,
            triple: self.triple,
            used_fallback_cargo,
            is_saving_sysroot,
        })
    }

    fn get_module(&self, module: &str) -> Result<()> {
        Module {
            variant: match module {
                "cargo" => ModuleVariant::Cargo,
                "rustc" => ModuleVariant::Rustc,
                "rust-std" => ModuleVariant::Std,
                _ => panic!("unknown module variant: {}", module),
            },
            sysroot: self,
        }.get()
    }

    fn get_and_extract(&self, module: &str) -> Result<()> {
        self.get_module(module)
    }

    fn extract(&self, module: &Module, reader: Box<Read>) -> Result<()> {
        let is_std = module.variant == ModuleVariant::Std;
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
