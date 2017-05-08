#![recursion_limit = "1024"]

#[macro_use] extern crate clap;
extern crate serde;
extern crate serde_json;
#[macro_use] extern crate error_chain;
extern crate flate2;
extern crate tar;
extern crate env_logger;
#[macro_use] extern crate log;
extern crate reqwest;
extern crate git2;
extern crate hex;
extern crate chrono;

mod errors {
    // Create the Error, ErrorKind, ResultExt, and Result types
    error_chain! {
        foreign_links {
            Git2(::git2::Error);
            Reqwest(::reqwest::Error);
            Io(::std::io::Error);
        }
    }
}

use errors::*;

quick_main!(run);

use std::path::Path;
use std::process::Command;

mod git;
mod sysroot;

use git::Commit;
use sysroot::Sysroot;

// return true if commit is successfully broken
fn test_commit(commit: &Commit, test_case: &Path, triple: &str, preserve_sysroots: bool) -> Result<bool> {
    let sysroot = Sysroot::install(commit, triple, preserve_sysroots)?;

    let status = sysroot.command(test_case).status()?;
    info!("tested {} from {}: {}", commit.sha, commit.date.to_rfc2822(), status.success());
    Ok(status.success())
}

/// Finds the index of the least item in `slice` for which the `predicate` holds.
pub fn least_satisfying<T, P>(slice: &[T], mut predicate: P) -> usize
    where P: FnMut(&T) -> bool
{
    let mut base = 0usize;
    let mut s = slice;

    loop {
        let (head, tail) = s.split_at(s.len() >> 1);
        if tail.is_empty() {
            return base + head.len();
        }
        if predicate(&tail[0]) {
            s = head;
        } else {
            base += head.len() + 1;
            s = &tail[1..];
        }
    }
}

fn get_host_triple() -> Result<String> {
    let output = Command::new("rustc")
        .arg("-v").arg("-V").output()
        .chain_err(|| format!("running rustc -vV to obtain host triple failed; try --triple"))?;
    let output = String::from_utf8_lossy(&output.stdout);
    Ok(output.lines().find(|l| l.starts_with("host: ")).unwrap()[6..].to_string())
}

fn run() -> Result<i32> {
    env_logger::init().expect("logger initialization successful");

    let matches = clap_app!(bisect_rust =>
       (version: "0.1")
       (author: "The Rust Infrastructure Team")
       (about: "Find PRs introducing regressions into Rust")
       (@arg preserve_sysroots: -p --preserve "Don't delete sysroots after running.")
       (@arg test: +required +takes_value --test "File to run to test for regression")
       (@arg triple: +takes_value --triple "triple to use for downloads")
    ).get_matches();

    let preserve_sysroots = matches.is_present("preserve_sysroots");
    let test_case = Path::new(matches.value_of_os("test").unwrap()).canonicalize()?;
    let triple = match matches.value_of("triple") {
        Some(x) => x.to_string(),
        None => get_host_triple()?,
    };

    const START: &str = "927c55d86b0be44337f37cf5b0a76fb8ba86e06c";
    const END: &str = "master";

    println!("Getting commits from the git checkout");
    let commits = try!(git::get_commits_between(START, END));
    assert_eq!(commits.first().expect("at least one commit").sha, START);
    println!("Searching in {} commits; about {} steps",
        commits.len(),
        commits.len().next_power_of_two().trailing_zeros());

    let found = least_satisfying(&commits, |commit| {
        test_commit(commit, &test_case, &triple, preserve_sysroots).unwrap()
    });

    println!("searched commits {:?} through {:?}", commits.first().unwrap().sha, commits.last().unwrap().sha);
    println!("regression in {:?}; {:?}", found, commits.get(found));

    Ok(0)
}
