mod archive;

use crate::archive::{FileContent, PackageArchive};


use anyhow::Context;
use clap::Parser;
use git2::{
    Index, IndexEntry, IndexTime, ObjectType,
    Oid, Repository, Signature, Sort,
};

use std::path::{PathBuf};
use url::Url;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(long, short)]
    repo: PathBuf,

    #[command(subcommand)]
    run_type: RunType,
}

#[derive(clap::Subcommand)]
enum RunType {
    FromArgs {
        #[arg()]
        name: String,

        #[arg()]
        version: String,

        #[arg()]
        url: Url,
    },
    FromJson {
        #[arg()]
        inputs: Vec<String>,
    },
    Inspect {},
}

#[derive(serde::Deserialize, Debug)]
struct JsonInput {
    name: String,
    version: String,
    url: Url,
}

fn main() -> anyhow::Result<()> {
    let args: Cli = Cli::parse();

    match args.run_type {
        RunType::FromArgs { name, version, url } => {
            run_multiple(&args.repo, [JsonInput { name, version, url }].into_iter())?;
        }
        RunType::FromJson { inputs } => {
            let items = inputs.iter().map(|v| serde_json::from_str(v).unwrap());
            run_multiple(&args.repo, items)?;
        }
        RunType::Inspect {} => {
            let repo = Repository::open("/Users/tom/tmp/foo/test/test").unwrap();
            let mut remote = repo.find_remote("django").unwrap();

            for refspec in remote.fetch_refspecs().unwrap().iter() {
                println!("ref: {refspec:?}");
            }
            remote
                .fetch(
                    &["refs/heads/master:refs/remotes/django/master".to_string()],
                    None,
                    None,
                )
                .unwrap();
            let reference = repo.find_reference("refs/remotes/django/master").unwrap();

            let remote_ref = reference.peel_to_commit().unwrap();
            let mut local_commit = repo.head().unwrap().peel_to_commit().unwrap();

            let mut walk = repo.revwalk().unwrap();
            walk.push(remote_ref.id()).unwrap();
            walk.set_sorting(Sort::REVERSE).unwrap();

            for item in walk {
                let hash = item.unwrap();
                let to_commit = repo.find_commit(hash).unwrap();
                let mut idx2 = repo
                    .cherrypick_commit(&to_commit, &local_commit, 0, None)
                    .unwrap();
                let commit_tree_oid = idx2.write_tree_to(&repo).unwrap();
                let commit_tree = repo.find_tree(commit_tree_oid).unwrap();
                let rebased_commit_oid = repo
                    .commit(
                        Some("refs/heads/master"),
                        &to_commit.author(),
                        &to_commit.committer(),
                        to_commit.message().unwrap(),
                        &commit_tree,
                        &[&local_commit],
                    )
                    .unwrap();
                local_commit = repo.find_commit(rebased_commit_oid).unwrap();
            }
        }
    }
    Ok(())
}

fn run_multiple(repo_path: &PathBuf, items: impl Iterator<Item = JsonInput>) -> anyhow::Result<()> {
    let repo = match Repository::open(repo_path) {
        Ok(v) => v,
        Err(_) => {
            let repo = Repository::init(repo_path).unwrap();
            let mut index = repo.index().unwrap();
            index.set_version(4).unwrap();
            repo
        }
    };

    let mut index = repo.index().unwrap();
    for item in items {
        let error_ctx = format!(
            "Name: {}, version: {}, url: {}",
            item.name, item.version, item.url
        );
        run(&repo, &mut index, item.name, item.version, item.url).context(error_ctx)?;
    }
    Ok(())
}

fn run(
    repo: &Repository,
    index: &mut Index,
    name: String,
    version: String,
    url: Url,
) -> anyhow::Result<()> {
    let package_filename = url.path_segments().unwrap().last().unwrap();
    let package_extension = package_filename.rsplit('.').next().unwrap();

    let download_response = reqwest::blocking::get(url.clone())?;
    let mut archive = match PackageArchive::new(package_extension, download_response) {
        None => {
            // Skip unknown extensions?
            println!("Unknown extension {package_extension}");
            return Ok(());
        }
        Some(v) => v,
    };

    let mut has_any_text_files = false;

    for (file_name, content) in archive.all_items().flatten() {
        // Skip METADATA files. These can contain gigantic readme files which can bloat the repo?
        let path = format!("code/{name}/{version}/{file_name}").replace("/./", "/");
        if path.ends_with(".dist-info/METADATA") || path.contains("/.git/") {
            continue;
        }
        if let FileContent::Text(content) = content {
            let hash = Oid::hash_object(ObjectType::Blob, &content)?;
            let entry = IndexEntry {
                ctime: IndexTime::new(0, 0),
                mtime: IndexTime::new(0, 0),
                dev: 0,
                ino: 0,
                mode: 0o100644,
                uid: 0,
                gid: 0,
                file_size: content.len() as u32,
                id: hash,
                flags: 0,
                flags_extended: 0,
                path: path.into(),
            };
            index.add_frombuffer(&entry, &content)?;

            has_any_text_files = true;
        }
    }

    if !has_any_text_files {
        println!("No files! {url}");
        return Ok(());
    }

    let oid = index.write_tree()?;

    let signature = Signature::now("Tom Forbes", "tom@tomforb.es")?;
    let tree = repo.find_tree(oid)?;
    let parent = match &repo.head() {
        Ok(v) => Some(v.peel_to_commit().unwrap()),
        Err(_) => None,
    };
    let parent = match &parent {
        None => vec![],
        Some(p) => vec![p],
    };

    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        format!("{name} {version} ({package_filename})").as_str(),
        &tree,
        &parent,
    )?;

    Ok(())
}
