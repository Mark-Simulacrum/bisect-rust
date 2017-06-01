#![recursion_limit = "1024"]

#[macro_use] extern crate error_chain;
#[macro_use] extern crate clap;
extern crate env_logger;
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
use rust_sysroot::get_host_triple;

fn run() -> Result<i32> {
    env_logger::init().expect("logger initialization successful");

    let matches = clap_app!(install_sysroot =>
       (version: "0.1")
       (author: "The Rust Infrastructure Team")
       (about: "Install Rust from a given PR")
       (@arg commit: --commit +takes_value +required "SHA of sysroot")
       (@arg triple: +takes_value --triple "triple to use for downloads")
    ).get_matches();

    let commits = rust_sysroot::get_commits()?;

    let triple = match matches.value_of("triple") {
        Some(x) => x.to_string(),
        None => get_host_triple()?,
    };
    let commit = matches.value_of("commit").unwrap();
    let commit = commits.iter().find(|c| c.sha.starts_with(commit)).expect("commit passed to be bors commit");

    let _sysroot = Sysroot::install(commit, &triple, false, true)?;

    println!("Sysroot can be found in cache/{}", commit.sha);
    println!("Please delete it when finished.");

    Ok(0)
}
