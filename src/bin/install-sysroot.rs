#![recursion_limit = "1024"]

#[macro_use] extern crate error_chain;
#[macro_use] extern crate clap;
extern crate env_logger;
extern crate chrono;
extern crate rust_sysroot;

mod errors {
    error_chain! {
        links {
            Utils(::rust_sysroot::errors::Error, ::rust_sysroot::errors::ErrorKind);
        }
    }
}

use errors::*;

quick_main!(run);

use rust_sysroot::sysroot::Sysroot;
use rust_sysroot::git::Commit;
use chrono::{Utc, TimeZone};
use rust_sysroot::get_host_triple;

fn run() -> Result<i32> {
    env_logger::init().expect("logger initialization successful");

    let matches = clap_app!(install_sysroot =>
       (version: "0.1")
       (author: "The Rust Infrastructure Team")
       (about: "Install Rust from a given PR")
       (@arg commit: --commit +takes_value +required "SHA of sysroot")
       (@arg skip_validation: --("skip-validation") "skip validation of commit, useful for try builds")
       (@arg triple: +takes_value --triple "triple to use for downloads")
    ).get_matches();

    let triple = match matches.value_of("triple") {
        Some(x) => x.to_string(),
        None => get_host_triple()?,
    };
    let commit = matches.value_of("commit").unwrap();
    let commit = if !matches.is_present("skip_validation") {
        let commits = rust_sysroot::get_commits()?;
        commits.into_iter()
            .find(|c| c.sha.starts_with(commit))
            .expect("commit passed to be bors commit")
    } else {
        Commit {
            sha: commit.to_string(),
            date: Utc.ymd(2000, 1, 1).and_hms(0, 0, 0),
            summary: String::new(),
        }
    };

    let _sysroot = Sysroot::install(&commit, &triple, false, true)?;

    println!("Sysroot can be found in cache/{}", commit.sha);
    println!("Please delete it when finished.");

    Ok(0)
}
