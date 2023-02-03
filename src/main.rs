mod archive;
mod data;
mod writer;

use crate::archive::{FileContent, PackageArchive};
use crossbeam::thread;
use std::fs;
use std::fs::File;
use std::io::BufReader;

use anyhow::Context;
use clap::Parser;
use git2::{
    ObjectType, RebaseOperationType, RebaseOptions, Repository, ResetType, Signature,
    Time,
};
use rayon::prelude::*;

use std::path::PathBuf;

use crate::writer::{consume_queue, TextFile};
use chrono::{DateTime, Utc};
use crossbeam::channel::bounded;
use log::info;
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
    uploaded_on: DateTime<Utc>,
}

fn main() -> anyhow::Result<()> {
    let args: Cli = Cli::parse();
    env_logger::init();

    match args.run_type {
        RunType::FromArgs {
            name,
            version,
            url,
            repo,
        } => {
            run_multiple(
                &repo,
                vec![JsonInput {
                    name,
                    version,
                    url,
                    uploaded_on: Default::default(),
                }],
            )?;
        }
        RunType::FromJson { input_file, repo } => {
            let reader = BufReader::new(File::open(input_file).unwrap());
            let input: Vec<JsonInput> = serde_json::from_reader(reader).unwrap();
            run_multiple(&repo, input)?;
        }
        RunType::CreateUrls {
            data,
            output_dir,
            limit,
            find,
        } => data::extract_urls(data, output_dir, limit, find),
        RunType::Combine {
            base_repo,
            target_repos,
        } => {
            let repo = Repository::open(base_repo).unwrap();
            let object_db = repo.odb().unwrap();
            object_db.add_new_mempack_backend(3).unwrap();

            let commits_to_pick = target_repos.iter().enumerate().map(|(idx, target)| {
                let target = fs::canonicalize(target).unwrap();
                let remote_name = format!("import_{idx}");
                let _ = repo.remote_delete(&remote_name);
                let mut remote = repo
                    .remote(
                        &remote_name,
                        format!("file://{}", target.to_str().unwrap()).as_str(),
                    )
                    .unwrap();
                remote
                    .fetch(
                        &["refs/heads/master:refs/remotes/import/master".to_string()],
                        None,
                        None,
                    )
                    .unwrap();
                let reference = repo
                    .find_reference(format!("refs/remotes/{remote_name}/master").as_str())
                    .unwrap();
                repo.reference_to_annotated_commit(&reference).unwrap()
            });

            for reference in commits_to_pick {
                info!("Rebasing from {}", reference.refname().unwrap());
                let local_ref = repo
                    .reference_to_annotated_commit(&repo.head().unwrap())
                    .unwrap();

                let mut opts = RebaseOptions::new();
                opts.inmemory(true);
                let mut rebase = repo
                    .rebase(Some(&reference), Some(&local_ref), None, Some(&mut opts))
                    .unwrap();
                let signature =
                    Signature::new("Tom Forbes", "tom@tomforb.es", &Time::new(0, 0)).unwrap();

                let mut last_commit = None;
                while let Some(x) = rebase.next() {
                    let kind = x.unwrap().kind().unwrap();
                    match kind {
                        RebaseOperationType::Pick => {
                            last_commit = Some(rebase.commit(None, &signature, None).unwrap());
                        }
                        _ => {
                            panic!("unknown kind {kind:?}");
                        }
                    }
                }
                rebase.finish(Some(&signature)).unwrap();
                repo.reset(
                    &repo
                        .find_object(last_commit.unwrap(), Some(ObjectType::Commit))
                        .unwrap(),
                    ResetType::Soft,
                    None,
                )
                .unwrap();
                info!("Rebased and reset {}", reference.refname().unwrap());
            }
        }
    }
    Ok(())
}

fn run_multiple(repo_path: &PathBuf, items: Vec<JsonInput>) -> anyhow::Result<()> {
    let repo = match Repository::open(repo_path) {
        Ok(v) => v,
        Err(_) => {
            let _ = fs::create_dir(repo_path);
            let repo = Repository::init(repo_path).unwrap();
            let mut index = repo.index().unwrap();
            index.set_version(4).unwrap();
            repo
        }
    };

    let (sender, recv) = bounded::<(JsonInput, Vec<TextFile>, String)>(20);
    info!("Starting");

    thread::scope(|s| {
        s.spawn(|_| {
            let sender = sender;
            items.into_par_iter().for_each(|item| {
                let error_ctx = format!(
                    "Name: {}, version: {}, url: {}",
                    item.name, item.version, item.url
                );
                let idx = run(item).context(error_ctx).unwrap();
                if let Some(idx) = idx {
                    if sender.send(idx).is_err() {
                        // Ignore errors sending
                    }
                }
            });
        });

        consume_queue(&repo, recv)
    })
    .unwrap();

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

fn run(item: JsonInput) -> anyhow::Result<Option<(JsonInput, Vec<TextFile>, String)>> {
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
    let reduced_package_filename =
        &package_filename[(item.name.len() + 1 + item.version.len() + 1)..];

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
        if IGNORED_SUFFIXES.iter().any(|s| file_name.ends_with(s))
            || file_name.contains("/.git/")
            || file_name.ends_with("/.git")
        {
            continue;
        }
        let file_name = if file_name.starts_with(&tar_gz_first_segment) {
            &file_name[tar_gz_first_segment.len()..]
        } else {
            &*file_name
        };
        let path = format!(
            "code/{}/{}/{}/{file_name}",
            item.name, item.version, reduced_package_filename
        )
        .replace("/./", "/")
        .replace("/../", "/");
        if let FileContent::Text(content) = content {
            entries.push(TextFile {
                path,
                contents: content,
            });
            has_any_text_files = true;
        }
    }

    if !has_any_text_files {
        return Ok(None);
    }

    Ok(Some((item, entries, package_filename.to_string())))
}
