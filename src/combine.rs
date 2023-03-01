use git2::{Mempack, ObjectType, Oid, Repository};

use std::fs;
use std::io::Write;

use crate::job::CommitMessage;
use crate::utils::log_timer;
use anyhow::Context;


use indicatif::{ProgressBar, ProgressIterator};
use itertools::{Itertools};
use std::path::{PathBuf};
use std::time::Duration;

const FILE_MODE_TREE: i32 = 0o040000;

pub fn merge_all_branches(into: PathBuf, mut repos: Vec<PathBuf>) -> anyhow::Result<()> {
    git2::opts::strict_object_creation(false);
    git2::opts::strict_hash_verification(false);
    git2::opts::enable_caching(false);

    let target_repo = Repository::open(&into)?;
    let target_pack_dir = target_repo.path().join("objects").join("pack");

    // Ensure repos are sorted so that tags are somewhat deterministic
    repos.sort();


    // Step 1: Create a branch in each target repo and copy the data. this is faster than using a remote.
    for (_idx, repo_path) in repos.iter().enumerate() {
        // println!("{}", repo_path.display());
        let repo = match Repository::open(repo_path) {
            Ok(r) => r,
            Err(e) => {
                println!("Skipping {}: {e}", repo_path.display());
                continue;
            }
        };

        let pack_dir = repo.path().join("objects").join("pack");
        for item in fs::read_dir(pack_dir)? {
            let item = item?;
            let to_path = target_pack_dir.join(item.file_name());
            if !to_path.exists() {
                fs::copy(item.path(), &to_path).with_context(|| {
                    format!(
                        "Error copying {} to {}",
                        item.path().display(),
                        to_path.display()
                    )
                })?;
            }
        }
    }

    let odb = target_repo.odb()?;

    let mut commits = vec![];
    odb.foreach(|v| {
        let (_, obj_type) = odb.read_header(*v).unwrap();
        if obj_type == ObjectType::Commit {
            commits.push(*v);
        }
        true
    })?;

    let commits: Vec<_> = commits
        .into_iter()
        .map(|oid| target_repo.find_commit(oid).unwrap())
        .sorted_by(|c1, c2| c1.time().cmp(&c2.time()))
        .collect();

    let total_commits = commits.len();

    println!("reset refs/heads/import");

    for (idx, commit) in commits.into_iter().enumerate() {
        let idx = idx + 1;
        let commit_message = commit.message().unwrap();
        let message: CommitMessage = serde_json::from_str(commit_message)
            .with_context(|| format!("Message: {}", commit.message().unwrap()))?;

        let (root, package_name, upload_name) = message.path.components().map(|c| c.as_os_str().to_str().unwrap()).collect_tuple().unwrap();

        // commit refs/heads/main
        // mark :2
        // author Tom Forbes <tom@tomforb.es> 1673443596 +0000
        // committer Tom Forbes <tom@tomforb.es> 1673443635 +0000
        // data 17
        // Make it parallel
        // from :1
        // M 040000 da3115eb3ba108a778bb980214b2e657d977f1a0 bar

        let author = commit.author();

        println!("commit refs/heads/import");
        println!("mark :{idx}");
        println!("author {} <{}> {} +0000", author.name().unwrap(), author.email().unwrap(), commit.time().seconds());
        println!("committer {} <{}> {} +0000", author.name().unwrap(), author.email().unwrap(), commit.time().seconds());
        println!("data {}", commit_message.len());
        print!("{}\n", commit_message);
        if idx > 1 {
            println!("from :{}", idx - 1);
        }
        println!("M 040000 {} {}/{}", commit.tree_id(), package_name, upload_name);
        println!();

        if (idx % 10_000) == 0 {
            println!("progress {idx}/{total_commits}");
            println!();
        }
    }

    Ok(())
}
