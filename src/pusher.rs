use crate::writer::{package_name_to_path, CommitMessage};
use git2::{Commit, Repository, TreeWalkMode};
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::io;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub fn compute_push_strategy(repo: PathBuf) {
    let repo = Repository::open(repo).unwrap();
    let odb = repo.odb().unwrap();

    let mut commits = vec![];

    odb.foreach(|v| {
        if let Ok(c) = repo.find_commit(*v) {
            commits.push(c);
        }
        true
    })
    .unwrap();

    let msg = commits[0].message().unwrap();
    let package_name = match serde_json::from_str::<CommitMessage>(msg) {
        Ok(m) => m.name,
        Err(_) => msg.trim().split(' ').next().unwrap(),
    }
    .to_string();

    let mut locked_stdout = BufWriter::new(io::stdout().lock());
    serde_json::to_writer(
        &mut locked_stdout,
        &ordered_commits(package_name, commits, &repo),
    )
    .unwrap();
    locked_stdout.write_all("\n".as_ref()).unwrap();
}

#[derive(Debug, Serialize, Deserialize)]
struct PushStrategy {
    tag_name: String,
    total_files: usize,
    last_commit: String,
    push_order: Vec<String>,
    path: PathBuf,
}

lazy_static! {
    static ref INFO_REGEX: Regex =
        Regex::new("[^\\s]* (?P<version>[^\\s]*) \\((?P<filename>[^\\s]*)\\)").unwrap();
}

const TOTAL_FILES_PER_PUSH: i32 = 1_000;

fn ordered_commits(
    package_name: String,
    mut commits: Vec<Commit>,
    repo: &Repository,
) -> PushStrategy {
    commits.sort_by_cached_key(|v| v.time());
    let mut total_files = 0;
    let mut running_total = 0;
    let mut push_order = vec![];
    let last_commit = commits.last().unwrap().id().to_string();

    for commit in commits {
        let tree = commit.tree().unwrap();
        let path = match serde_json::from_str::<CommitMessage>(commit.message().unwrap()) {
            Ok(m) => PathBuf::from_str(&m.path).unwrap(),
            Err(_) => {
                let res = INFO_REGEX.captures(commit.message().unwrap()).unwrap();
                let version = res.name("version").unwrap().as_str();
                let filename = res.name("filename").unwrap().as_str();
                let path = package_name_to_path(&package_name, version, filename);
                Path::new("code").join(path.0).join(path.1).join(path.2)
            }
        };
        let p = tree.get_path(&path).unwrap();
        let sub_tree = p.to_object(repo).unwrap().into_tree().unwrap();
        sub_tree
            .walk(TreeWalkMode::PostOrder, |_, _| {
                running_total += 1;
                total_files += 1;
                0
            })
            .unwrap();
        if running_total >= TOTAL_FILES_PER_PUSH {
            push_order.push(commit.id().to_string());
        }
    }

    let path = repo.path().to_path_buf();
    PushStrategy {
        tag_name: package_name,
        total_files,
        last_commit,
        push_order,
        path,
    }
}
