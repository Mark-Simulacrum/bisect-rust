//! Get commits through the Github API.

use std::str;

use chrono::{DateTime, TimeZone, UTC};
use reqwest::{self, Client};
use reqwest::header::Authorization;
use serde_json::{self, Value};

use errors::{Result, ResultExt};

const GH_API_TOKEN: &'static str = env!("GH_API_TOKEN");

#[derive(Debug, Clone, PartialEq)]
pub struct Commit {
    pub sha: String,
    pub date: DateTime<UTC>,
}

fn parse_commit(commit: Value) -> Commit {
    if let Value::Object(mut map) = commit {
        return Commit {
            sha: map.remove("sha").expect("sha to be present").as_str().unwrap().to_string(),
            date: UTC.datetime_from_str(map.remove("commit")
                .and_then(|mut commit| commit.as_object_mut()
                .and_then(|commit| commit.remove("committer")))
                .and_then(|mut committer| committer.as_object_mut()
                .and_then(|committer| committer.remove("date")))
                .expect("commit.comitter.date to be present").as_str().unwrap(),
                "%+").expect("failed to parse date"),
        };
    } else {
        panic!("commit object {:?} is not an object?", commit)
    }
}

fn request_from_gh(client: &reqwest::Client, url: &str) -> Result<(Value, reqwest::Response)> {
    info!("Requesting: {}", url);
    let mut request_ = client.get(url);
    if !GH_API_TOKEN.is_empty() {
        request_ = request_.header(Authorization(format!("token {}", GH_API_TOKEN)));
    }
    let mut response = request_.send().chain_err(|| format!("API request to {}", url))?;
    let value = serde_json::from_reader(&mut response)
        .chain_err(|| format!("API request to {} deserialization", url))?;
    Ok((value, response))
}

pub fn get_commits_since(client: &Client, first_commit: &str) -> Result<Vec<Commit>> {
    fn request(client: &reqwest::Client, url: &str, first_commit: &str, commits: &mut Vec<Commit>) -> Result<()> {
        let (value, response) = request_from_gh(client, url)?;
        if let Value::Array(arr) = value {
            let new_commits = arr.into_iter().map(parse_commit);

            commits.extend(new_commits);

            if let Some(_) = commits.iter().find(|commit| commit.sha == first_commit) {
                return Ok(());
            }
        } else {
            bail!("{} returned non-array response: {}", url, value);
        }

        if let Some(headers) = response.headers().get_raw("Link") {
            if let Some(next) = headers.iter().map(|s| str::from_utf8(s).unwrap()).flat_map(|s| s.split(", ")).find(|s| s.contains(r#"rel="next"#)) {
                let url = &next[1..next.find(">").unwrap()];
                return request(&client, url, first_commit, commits);
            }
        }

        bail!("Couldn't find first commit")
    }

    let mut commits = Vec::new();
    request(
        &client,
        "https://api.github.com/repos/rust-lang/rust/commits?author=bors&per_page=100",
        &first_commit,
        &mut commits,
    )?;

    if let Some(pos) = commits.iter().position(|commit| commit.sha == first_commit) {
        {
            let _drop = commits.drain((pos + 1)..);
        }
        commits.reverse();

        return Ok(commits);
    }

    bail!("Couldn't find first commit")
}
