use git2::{Mempack, ObjectType, Oid, Repository};
use std::collections::hash_map::Entry;
use std::collections::HashMap;

use std::fs;
use std::io::Write;

use crate::job::CommitMessage;
use crate::utils::log_timer;
use anyhow::Context;

use chrono::prelude::*;
use indicatif::{ProgressBar, ProgressIterator};
use itertools::Itertools;
use log::warn;
use std::path::PathBuf;
use std::time::Duration;
use tinytemplate::TinyTemplate;
use url::Url;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct JsonIndexEntry {
    pub name: String,
    pub version: String,
    pub path: PathBuf,
    pub uploaded_on: DateTime<Utc>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct JsonIndex {
    pub url: Url,
    pub earliest_release: DateTime<Utc>,
    pub latest_release: DateTime<Utc>,
    pub entries: HashMap<String, Vec<JsonIndexEntry>>,
}

const FILE_MODE_TREE: i32 = 0o040000;

pub fn merge_all_branches(into: PathBuf, mut repos: Vec<PathBuf>) -> anyhow::Result<()> {
    let repository_partition_index = into.file_name().unwrap().to_str().unwrap();
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
                warn!("Skipping {}: {e}", repo_path.display());
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

    std::process::Command::new("git").current_dir(target_repo.path()).args(&["repack", "-k", "-a", "-d", "--window=5", "--depth=20", "--write-bitmap-index", "--threads=1"]).status().unwrap();

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

    let mut packages_index: HashMap<String, Vec<JsonIndexEntry>> = HashMap::new();

    println!("reset refs/heads/import");

    let mut current_mark = 0;

    for commit in commits.into_iter() {
        current_mark += 1;
        let commit_message = commit.message().unwrap();
        let mut message: CommitMessage = serde_json::from_str(commit_message)
            .with_context(|| format!("Message: {}", commit.message().unwrap()))?;
        let (root, package_name, upload_name) = message
            .path
            .components()
            .map(|c| c.as_os_str().to_str().unwrap())
            .collect_tuple()
            .unwrap();
        message.path = PathBuf::new().join(package_name).join(&upload_name);
        let commit_message = serde_json::to_string(&message).unwrap();
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
        println!("mark :{current_mark}");
        println!(
            "author {} <{}> {} +0000",
            author.name().unwrap(),
            author.email().unwrap(),
            commit.time().seconds()
        );
        println!(
            "committer {} <{}> {} +0000",
            author.name().unwrap(),
            author.email().unwrap(),
            commit.time().seconds()
        );
        println!("data {}", commit_message.len());
        print!("{}\n", commit_message);
        if current_mark > 1 {
            println!("from :{}", current_mark - 1);
        }
        println!("M 040000 {} {}", commit.tree_id(), message.path.display());
        println!();

        if (current_mark % 10_000) == 0 {
            println!("progress {current_mark}/{total_commits}");
            println!();
        }

        let time = DateTime::<Utc>::from_utc(
            NaiveDateTime::from_timestamp_opt(commit.time().seconds(), 0).unwrap(),
            Utc,
        );
        let entry = JsonIndexEntry {
            name: message.name,
            version: message.version,
            path: message.path,
            uploaded_on: time,
        };

        match packages_index.entry(entry.name.to_string()) {
            Entry::Occupied(e) => e.into_mut().push(entry),
            Entry::Vacant(e) => {
                e.insert(vec![entry]);
            }
        }
    }

    let total_projects = packages_index.len();
    let total_releases = packages_index.values().flatten().count();
    let (min_release_time, max_release_time) = packages_index
        .values()
        .flatten()
        .map(|e| e.uploaded_on)
        .minmax()
        .into_option()
        .unwrap();
    let top_projects_by_count = packages_index
        .iter()
        .map(|(name, items)| (name, items.len()))
        .sorted_by(|v1, v2| v1.1.cmp(&v2.1).reverse())
        .take(25)
        .collect();

    #[derive(serde::Serialize)]
    struct Context<'a> {
        first_release: NaiveDate,
        last_release: NaiveDate,
        total_projects: usize,
        total_releases: usize,
        table: Vec<(&'a String, usize)>,
        repo_url: Url,
    }

    let repo_url: Url = format!(
        "https://github.com/pypi-data/pypi-code-{repository_partition_index}"
    ).parse()?;

    let mut tt = TinyTemplate::new();
    tt.add_template("readme", include_str!("index_template.md"))?;
    let readme = tt.render(
        "readme",
        &Context {
            first_release: min_release_time.date_naive(),
            last_release: max_release_time.date_naive(),
            total_projects,
            total_releases,
            repo_url: repo_url.clone(),
            table: top_projects_by_count,
        },
    )?;
    let index_json = serde_json::to_string(&JsonIndex {
        url: repo_url,
        earliest_release: min_release_time,
        latest_release: max_release_time,
        entries: packages_index,
    }).unwrap();

    println!("reset refs/heads/main");

    // Add the README
    current_mark += 1;
    println!("blob");
    println!("mark :{current_mark}");
    println!("data {}", readme.len());
    println!("{readme}");
    let readme_mark = current_mark;

    // Add the index.json
    current_mark += 1;
    println!("blob");
    println!("mark :{current_mark}");
    println!("data {}", index_json.len());
    println!("{index_json}");
    let index_json_mark = current_mark;

    println!("commit refs/heads/main");
    println!(
        "author Tom Forbes <tom@tomforb.es> {} +0000",
        max_release_time.timestamp()
    );
    println!(
        "committer Tom Forbes <tom@tomforb.es> {} +0000",
        max_release_time.timestamp()
    );

    let commit_message = format!(
        "Import from {} to {}",
        min_release_time.date_naive(),
        max_release_time.date_naive()
    );

    println!("data {}", commit_message.len());
    print!("{}\n", commit_message);
    println!("M 100644 :{} {}", readme_mark, "README.md");
    println!("M 100644 :{} {}", index_json_mark, "index.json");
    println!();

    println!("done");

    Ok(())
}
