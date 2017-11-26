#![recursion_limit = "1024"]

#[macro_use] extern crate error_chain;
#[macro_use] extern crate clap;
#[macro_use] extern crate log;
extern crate env_logger;
extern crate rust_sysroot;

mod errors {
    error_chain! {
        links {
            Utils(::rust_sysroot::errors::Error, ::rust_sysroot::errors::ErrorKind);
        }

        foreign_links {
            Io(::std::io::Error);
        }
    }
}

use errors::*;

quick_main!(run);

use std::path::Path;

use rust_sysroot::git::Commit;
use rust_sysroot::sysroot::Sysroot;
use rust_sysroot::{get_host_triple, EPOCH_COMMIT};

// return true if commit is successfully broken
fn test_commit(commit: &Commit, test_case: &Path, triple: &str, preserve_sysroots: bool) -> Result<bool> {
    let sysroot = Sysroot::install(commit, triple, preserve_sysroots, false)?;

    let status = sysroot.command(test_case).status()?;
    info!("tested {:} from {}: test failed: {}", &commit.sha[0..9], commit.date.to_rfc2822(), status.success());
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

fn run() -> Result<i32> {
    env_logger::init().expect("logger initialization successful");

    let matches = clap_app!(bisect =>
       (version: "0.1")
       (author: "The Rust Infrastructure Team")
       (about: "Find PRs introducing regressions into Rust")
       (@arg preserve_sysroots: -p --preserve "Don't delete sysroots after running.")
       (@arg test: +required +takes_value --test "File to run to test for regression")
       (@arg triple: +takes_value --triple "triple to use for downloads")
       (@arg start: +takes_value default_value(EPOCH_COMMIT) --start "First commit to search from")
       (@arg end: +takes_value default_value[master] --end "Last commit to search until")
    ).get_matches();

    let preserve_sysroots = matches.is_present("preserve_sysroots");
    let test_case = Path::new(matches.value_of_os("test").expect("--test")).canonicalize()?;
    let triple = match matches.value_of("triple") {
        Some(x) => x.to_string(),
        None => get_host_triple()?,
    };

    let start = matches.value_of("start").unwrap();
    let end = matches.value_of("end").unwrap();
    let commits = rust_sysroot::get_commits(start, end)?;

    println!("Searching in {} commits; about {} steps",
        commits.len(),
        commits.len().next_power_of_two().trailing_zeros());

    let found = least_satisfying(&commits, |commit| {
        test_commit(commit, &test_case, &triple, preserve_sysroots).unwrap()
    });

    println!("searched commits {} through {}", commits.first().unwrap().sha, commits.last().unwrap().sha);
    println!("regression in {:?}; {:?}", found, commits.get(found));

    Ok(0)
}
