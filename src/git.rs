//! Get git commits with help of the libgit2 library

const RUST_SRC_REPO: &str = env!("RUST_SRC_REPO");

use chrono::{DateTime, TimeZone, UTC};

use errors::Result;

use git2::{Repository, Oid, Commit as Git2Commit};

#[derive(Debug, Clone, PartialEq)]
pub struct Commit {
    pub id: Oid,
    pub date: DateTime<UTC>,
    pub summary: String,
}

impl Commit {
    // Takes &mut because libgit2 internally caches summaries
    fn from_git2_commit(commit: &mut Git2Commit) -> Self {
        Commit {
            id: commit.id(),
            date: UTC.timestamp(commit.time().seconds(), 0),
            summary: String::from(commit.summary().unwrap()),
        }
    }
    pub fn sha(&self) -> String {
        format!("{}", self.id)
    }
}

fn lookup_rev<'rev>(repo: &'rev Repository, rev: &str) -> Result<Git2Commit<'rev>> {
    if let Ok(c) = repo.revparse_single(rev)?.into_commit() {
        return Ok(c);
    }
    bail!("Could not find a commit for revision specifier '{}'", rev)
}

/// Returns the bors merge commits between the two specified boundaries
/// (boundaries inclusive).
pub fn get_commits_between(first_commit: &str, last_commit: &str) -> Result<Vec<Commit>> {
    let repo = Repository::open(RUST_SRC_REPO)?;
    let mut first = lookup_rev(&repo, first_commit)?;
    let last = lookup_rev(&repo, last_commit)?;

    // Sanity check -- our algorithm below only works reliably if the
    // two commits are merge commits made by bors
    let assert_by_bors = |c: &Git2Commit| -> Result<()> {
        match c.author().name() {
            Some("bors") => Ok(()),
            Some(author) => bail!("Expected author {} to be bors for {}", author, c.id()),
            None => bail!("No author for {}", c.id()),
        }
    };
    assert_by_bors(&first)?;
    assert_by_bors(&last)?;
    // Now find the commits
    // We search from the last and always take the first of its parents,
    // to only get merge commits.
    // This uses the fact that all bors merge commits have the earlier
    // merge commit as their first parent.
    let mut res = Vec::new();
    let mut current = last;
    loop {
        assert_by_bors(&current)?;
        res.push(Commit::from_git2_commit(&mut current));
        match current.parents().next() {
            Some(c) => {
                if c.author().name() != Some("bors") {
                    warn!("{:?} has non-bors author: {:?}, skipping", c.id(), c.author().name());
                    current = c.parents().next().unwrap();
                    continue;
                }
                current = c;
                if current.id() == first.id() {
                    // Reached the first commit, our end of the search.
                    break;
                }
            },
            None => bail!("reached end of repo without encountering the first commit"),
        }
    }
    res.push(Commit::from_git2_commit(&mut first));
    // Reverse in order to obtain chronological order
    res.reverse();
    Ok(res)
}
