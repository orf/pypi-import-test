use crate::writer::{package_name_to_path, CommitMessage};
use git2::{Commit, Cred, Direction, PushOptions, RemoteCallbacks, Repository, TreeWalkMode};
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::str::FromStr;


#[derive(Debug, Serialize, Deserialize)]
struct PushStrategy {
    tag_name: String,
    total_files: usize,
    last_commit: String,
    push_order: Vec<String>,
    path: PathBuf,
}

pub fn push(strategy_info: String) {
    // let reader = io::BufReader::new(File::open(strategy_file).unwrap());
    // let strategy: PushStrategy = serde_json::from_reader(reader).unwrap();
    let mut strategy: PushStrategy = serde_json::from_str(&strategy_info).unwrap();
    // println!("[{}] Setting up push", strategy.tag_name);
    let repo = Repository::open(&strategy.path).unwrap();
    let mut remote = match repo.find_remote("origin") {
        Ok(r) => r,
        Err(_) => repo.remote("origin", "git@github.com:pypi-data/pypi-code.git").unwrap()
    };

    let new_remote_callbacks = || {
        let mut callbacks = RemoteCallbacks::new();
        callbacks.push_update_reference(|r, status| {
            if let Some(s) = status {
                panic!("Reference {r} in {:?} could not be pushed: {s}", strategy.path)
            };
            println!("Reference {r} in {:?} pushed", strategy.tag_name);
            Ok(())
        });
        callbacks.push_transfer_progress(|current, total, bytes| {
            if current != 0 {
                println!("current: {current} total: {total} bytes: {bytes}");
            }
        });
        callbacks.credentials(|_, _, _| {
            // let p = Path::new("/Users/tom/.ssh/id_ed25519");
            // let p2 = Path::new("/Users/tom/.ssh/id_ed25519.pub");
            let p = Path::new("/root/.ssh/id_rsa");
            let p2 = Path::new("/root/.ssh/id_rsa.pub");
            Cred::ssh_key(
                "git",
                Some(p2),
                p,
                None,
            )
        });

        callbacks
    };

    remote.connect_auth(
        Direction::Push,
        Some(new_remote_callbacks()),
        None,
    ).unwrap();

    let mut options = PushOptions::new();
    options.remote_callbacks(new_remote_callbacks());

    let push_order = match strategy.push_order.as_slice() {
        [.., last] if last == &strategy.last_commit => {
            strategy.push_order
        }
        _ => {
            strategy.push_order.push(strategy.last_commit);
            strategy.push_order
        }
    };

    println!("[{}] Starting pushing {} items", strategy.tag_name, push_order.len());
    for commit in push_order {
        println!("[{}] Setting head to {commit}", strategy.tag_name);
        repo.set_head_detached(commit.parse().unwrap()).unwrap();
        println!("[{}] Pushing {commit}", strategy.tag_name);
        let refspec = format!("+HEAD:refs/tags/{}", strategy.tag_name);
        remote.push(&[refspec], Some(&mut options)).unwrap();
        println!("[{}] Pushed {commit}", strategy.tag_name);
    }
}


pub fn compute_push_strategy(repo: PathBuf) {
    let mut tag_name = repo.file_name().unwrap().to_str().unwrap().rsplit_once('.').unwrap().0.to_string();
    if tag_name.ends_with("_0") {
        tag_name = tag_name.strip_suffix("_0").unwrap().to_string();
    }
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

    let output = serde_json::to_string(
        &ordered_commits(package_name, commits, &repo, tag_name),
    )
        .unwrap();
    println!("{output}");
}

lazy_static! {
    static ref INFO_REGEX: Regex =
        Regex::new("[^\\s]* (?P<version>.*) \\((?P<filename>.*)\\)$").unwrap();
}

const TOTAL_FILES_PER_PUSH: i32 = 1_000;

fn ordered_commits(
    package_name: String,
    mut commits: Vec<Commit>,
    repo: &Repository,
    tag_name: String,
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
        let p = tree.get_path(&path).unwrap_or_else(|_| panic!("Error unwrapping: {}", commit.message().unwrap()));
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
            running_total = 0;
        }
    }

    let path = repo.path().to_path_buf();
    PushStrategy {
        tag_name,
        total_files,
        last_commit,
        push_order,
        path,
    }
}
