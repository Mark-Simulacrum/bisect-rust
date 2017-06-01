#![recursion_limit = "1024"]

extern crate serde;
extern crate serde_json;
#[macro_use] extern crate error_chain;
extern crate xz2;
extern crate flate2;
extern crate tar;
#[macro_use] extern crate log;
extern crate reqwest;
extern crate git2;
extern crate chrono;

pub mod errors {
    // Create the Error, ErrorKind, ResultExt, and Result types
    error_chain! {
        foreign_links {
            Git2(::git2::Error);
            Reqwest(::reqwest::Error);
            Io(::std::io::Error);
        }
    }
}

pub mod git;
pub mod sysroot;

use std::process::Command;

use errors::*;

pub fn get_host_triple() -> Result<String> {
    let output = Command::new("rustc")
        .arg("-v").arg("-V").output()
        .chain_err(|| format!("running rustc -vV to obtain host triple failed; try --triple"))?;
    let output = String::from_utf8_lossy(&output.stdout);
    Ok(output.lines().find(|l| l.starts_with("host: ")).unwrap()[6..].to_string())
}

pub fn get_commits() -> Result<Vec<git::Commit>> {
    const START: &str = "927c55d86b0be44337f37cf5b0a76fb8ba86e06c";
    const END: &str = "master";

    info!("Getting commits from the git checkout");
    let commits = git::get_commits_between(START, END)?;
    assert_eq!(commits.first().expect("at least one commit").sha, START);

    Ok(commits)
}
