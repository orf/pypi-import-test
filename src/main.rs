mod archive;
mod data;

use crate::archive::{FileContent, PackageArchive};
use std::fs::File;
use std::io::BufReader;

use anyhow::Context;
use clap::Parser;
use git2::{Index, IndexEntry, IndexTime, ObjectType, Oid, Repository, Signature, Sort};
use rayon::prelude::*;


use std::path::PathBuf;


use std::thread;
use url::Url;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
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

        #[arg(long, short)]
        repo: PathBuf,
    },
    FromJson {
        #[arg()]
        input_file: PathBuf,

        #[arg()]
        repo: PathBuf,
    },
    Combine {
        #[arg()]
        base_repo: PathBuf,
        #[arg()]
        target_repos: Vec<PathBuf>,
    },
    CreateUrls {
        #[arg()]
        data: PathBuf,
        #[arg()]
        output_dir: PathBuf,
        #[arg(long, short)]
        limit: Option<usize>,
        #[arg(long, short)]
        find: Option<String>,
    },
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct JsonInput {
    name: String,
    version: String,
    url: Url,
}

fn main() -> anyhow::Result<()> {
    let args: Cli = Cli::parse();

    match args.run_type {
        RunType::FromArgs { name, version, url, repo } => {
            run_multiple(&repo, vec![JsonInput { name, version, url }])?;
        }
        RunType::FromJson { input_file, repo } => {
            let reader = BufReader::new(File::open(input_file).unwrap());
            let input: Vec<JsonInput> = serde_json::from_reader(reader).unwrap();
            run_multiple(&repo, input)?;
        }
        RunType::CreateUrls { data, output_dir,  limit, find} => data::extract_urls(data, output_dir, limit, find),
        RunType::Combine { base_repo, target_repos } => {
            let repo = Repository::open(base_repo).unwrap();
            let commits_to_pick= target_repos.iter().enumerate().flat_map(|(idx, target)| {
                let remote_name = format!("import_{}", idx);
                let _ = repo.remote_delete(&*remote_name);
                let mut remote = repo.remote(&*remote_name, format!("file://{}", target.to_str().unwrap()).as_str()).unwrap();
                remote
                    .fetch(
                        &["refs/heads/master:refs/remotes/import/master".to_string()],
                        None,
                        None,
                    )
                    .unwrap();
                let reference = repo.find_reference(format!("refs/remotes/{}/master", remote_name).as_str()).unwrap();
                let remote_ref = reference.peel_to_commit().unwrap();
                let mut walk = repo.revwalk().unwrap();
                walk.push(remote_ref.id()).unwrap();
                walk.set_sorting(Sort::REVERSE).unwrap();
                walk
            }).flatten();

            let mut local_commit = repo.head().unwrap().peel_to_commit().unwrap();

            for hash in commits_to_pick {
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

fn run_multiple(repo_path: &PathBuf, items: Vec<JsonInput>) -> anyhow::Result<()> {
    let repo = match Repository::open(repo_path) {
        Ok(v) => v,
        Err(_) => {
            let repo = Repository::init(repo_path).unwrap();
            let mut index = repo.index().unwrap();
            index.set_version(4).unwrap();
            repo
        }
    };

    use crossbeam::channel::bounded;
    let (s, r) = bounded::<(JsonInput, Vec<IndexEntry>, String)>(10);

    let inner_thread = thread::spawn(move || {
        let signature = Signature::now("Tom Forbes", "tom@tomforb.es").unwrap();
        let mut repo_idx = repo.index().unwrap();

        for (i, index, filename) in r {
            for entry in index.iter() {
                repo_idx.add(&entry).unwrap();
            }
            let oid = repo_idx.write_tree().unwrap_or_else(|_| panic!("Error writing {} {} {}", i.name, i.version, i.url));

            let tree = repo.find_tree(oid).unwrap();
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
                format!("{} {} ({})", i.name, i.version, filename).as_str(),
                &tree,
                &parent,
            )
                .unwrap();
        }
    });

    items.into_par_iter().for_each(|item| {
        let repo = Repository::open(repo_path).unwrap();
        // let index = repo.index().unwrap();
        let error_ctx = format!(
            "Name: {}, version: {}, url: {}",
            item.name, item.version, item.url
        );

        let idx = run(repo, item).context(error_ctx).unwrap();
        if let Some(idx) = idx {
            s.send(idx).unwrap();
        }
    });
    drop(s);
    inner_thread.join().unwrap();
    Ok(())
}

const IGNORED_SUFFIXES: &[&str] = &[
    // Skip METADATA files. These can contain gigantic readme files which can bloat the repo?
    ".dist-info/METADATA",
    // Same for license files
    ".dist-info/LICENSE",
    ".dist-info/RECORD",
    ".dist-info/TOP_LEVEL",
    ".dist-info/DESCRIPTION.rst",
];

fn run(repo: Repository, item: JsonInput) -> anyhow::Result<Option<(JsonInput, Vec<IndexEntry>, String)>> {
    let package_filename = item
        .url
        .path_segments()
        .unwrap()
        .last()
        .unwrap()
        .to_string();
    let package_extension = package_filename.rsplit('.').next().unwrap();
    // The package filename contains the package name and the version. We don't need this in the output, so just ignore it.
    // The format is `{name}-{version}-{rest}`, so we strip out `rest`
    let reduced_package_filename = &package_filename[(item.name.len() + 1 + item.version.len() + 1)..];

    // .tar.gz files unwrap all contents to paths like `Django-1.10rc1/...`. This isn't great,
    // so we detect this and strip the prefix.
    let tar_gz_first_segment = format!("{}-{}/", item.name, item.version);

    let download_response = reqwest::blocking::get(item.url.clone())?;
    let mut archive = match PackageArchive::new(package_extension, download_response) {
        None => {
            return Ok(None);
        }
        Some(v) => v,
    };

    let mut has_any_text_files = false;

    let mut entries = Vec::with_capacity(1024);

    for (file_name, content) in archive.all_items().flatten() {
        if IGNORED_SUFFIXES.iter().any(|s| file_name.ends_with(s)) || file_name.contains("/.git/") || file_name.ends_with("/.git") {
            continue;
        }
        let file_name = if file_name.starts_with(&tar_gz_first_segment) {
            &file_name[tar_gz_first_segment.len()..]
        } else {
            &*file_name
        };
        let path = format!("code/{}/{}/{}/{file_name}", item.name, item.version, reduced_package_filename).replace("/./", "/").replace("/../", "/");
        if let FileContent::Text(content) = content {
            let oid = repo.blob(&content).unwrap();
            let blob = repo.find_blob(oid).unwrap();
            let entry = IndexEntry {
                ctime: IndexTime::new(0, 0),
                mtime: IndexTime::new(0, 0),
                dev: 0,
                ino: 0,
                mode: 0o100644,
                uid: 0,
                gid: 0,
                file_size: blob.size() as u32,
                id: oid,
                flags: 0,
                flags_extended: 0,
                path: path.into(),
            };
            entries.push(entry);
            has_any_text_files = true;
        }
    }

    if !has_any_text_files {
        return Ok(None);
    }

    Ok(Some((item, entries, package_filename.to_string())))
}
