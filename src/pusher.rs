use crate::writer::package_name_to_path;
use git2::{Commit, Repository, TreeWalkMode};
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};

struct CurrentRun<'a> {
    name: String,
    items: Vec<Commit<'a>>,
}

pub fn push(repo: PathBuf) {
    let repo = Repository::open(repo).unwrap();
    let odb = repo.odb().unwrap();

    let mut current_run = CurrentRun {
        name: "".to_string(),
        items: vec![],
    };

    // let mut current_run: (String, Vec<Commit>) = ("".to_string(), Vec::with_capacity(150));
    // let mut pushes = Vec::with_capacity(600_000);
    let mut stdout = io::BufWriter::new(io::stdout().lock());

    odb.foreach(|v| {
        if let Ok(c) = repo.find_commit(*v) {
            let package_name = c.message().unwrap().trim().split(' ').next().unwrap();

            if package_name != current_run.name {
                let new_current_run = CurrentRun {
                    name: package_name.to_string(),
                    items: vec![],
                };
                let old_current_run = std::mem::replace(&mut current_run, new_current_run);
                if !old_current_run.items.is_empty() {
                    serde_json::to_writer(&mut stdout, &ordered_commits(old_current_run, &repo))
                        .unwrap();
                    stdout.write_all("\n".as_ref()).unwrap();
                }
            }
            current_run.items.push(c);
        }
        true
    })
    .unwrap();
}

#[derive(Debug, Serialize, Deserialize)]
struct PushStrategy {
    tag_name: String,
    total_files: usize,
    last_commit: String,
    push_order: Vec<String>,
}

lazy_static! {
    static ref INFO_REGEX: Regex =
        Regex::new("[^\\s]* (?P<version>[^\\s]*) \\((?P<filename>[^\\s]*)\\)").unwrap();
}

const TOTAL_FILES_PER_PUSH: i32 = 1_000;

fn ordered_commits(mut run: CurrentRun, repo: &Repository) -> PushStrategy {
    run.items.sort_by_cached_key(|v| v.time());
    let mut total_files = 0;
    let mut running_total = 0;
    let mut push_order = vec![];
    let last_commit = run.items.last().unwrap().id().to_string();

    for commit in run.items {
        let tree = commit.tree().unwrap();
        let res = INFO_REGEX.captures(commit.message().unwrap()).unwrap();
        let version = res.name("version").unwrap().as_str();
        let filename = res.name("filename").unwrap().as_str();
        let path = package_name_to_path(&run.name, version, filename);
        let p = tree
            .get_path(&Path::new("code").join(path.0).join(path.1).join(path.2))
            .unwrap();
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

    PushStrategy {
        tag_name: run.name,
        total_files,
        last_commit,
        push_order,
    }
}
