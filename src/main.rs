mod archive;
mod data;
mod writer;

use crate::archive::{FileContent, PackageArchive};
use crossbeam::thread;
use std::fs;
use std::fs::File;
use std::io::{BufReader, Write};

use anyhow::Context;
use clap::Parser;
use git2::{
    Buf, ObjectType, Odb, RebaseOperationType, RebaseOptions, Repository, RepositoryInitOptions,
    Signature, Time,
};
use rayon::prelude::*;

use std::path::PathBuf;

use crate::writer::{consume_queue, TextFile};
use chrono::{DateTime, Utc};
use crossbeam::channel::bounded;
use log::{info, warn};
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
        work_path: PathBuf,

        #[arg()]
        finished_path: PathBuf,
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

impl JsonInput {
    pub fn package_filename(&self) -> &str {
        self.url.path_segments().unwrap().last().unwrap()
    }
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
        RunType::FromJson {
            input_file,
            work_path,
            finished_path,
        } => {
            fs::create_dir(&work_path).unwrap();
            let work_path = fs::canonicalize(&work_path).unwrap();

            let reader = BufReader::new(File::open(input_file).unwrap());
            let input: Vec<JsonInput> = serde_json::from_reader(reader).unwrap();
            run_multiple(&work_path, input)?;
            fs::create_dir(&finished_path).unwrap();
            fs::rename(&work_path, &finished_path).unwrap();
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
            let opts = RepositoryInitOptions::new();
            let repo = Repository::init_opts(base_repo, &opts).unwrap();
            let object_db = repo.odb().unwrap();
            let mempack_backend = object_db.add_new_mempack_backend(3).unwrap();
            let mut repo_idx = repo.index().unwrap();
            repo_idx.set_version(4).unwrap();

            warn!("Fetching...");
            let commits_to_pick = target_repos.iter().enumerate().filter_map(|(idx, target)| {
                let target = fs::canonicalize(target).unwrap();
                let remote_name = format!("import_{idx}");
                let _ = repo.remote_delete(&remote_name);
                let mut remote = repo
                    .remote(
                        &remote_name,
                        format!("file://{}", target.to_str().unwrap()).as_str(),
                    )
                    .unwrap();
                warn!("Fetching remote {}", remote.url().unwrap());
                if let Err(e) = remote.fetch(
                    &[format!(
                        "refs/heads/master:refs/remotes/{remote_name}/master"
                    )],
                    None,
                    None,
                ) {
                    warn!("Error fetching remote: {}", e);
                    return None;
                }
                let reference = repo
                    .find_reference(format!("refs/remotes/{remote_name}/master").as_str())
                    .unwrap();
                Some(repo.reference_to_annotated_commit(&reference).unwrap())
            });

            for (idx, reference) in commits_to_pick.enumerate() {
                warn!("Progress: {idx}/{}", target_repos.len() - 1);
                info!("Rebasing from {}", reference.refname().unwrap());
                let local_ref = match repo.head() {
                    //repo.find_branch("merge", BranchType::Local) {
                    Ok(v) => repo.reference_to_annotated_commit(&v).unwrap(),
                    Err(_) => {
                        repo.set_head_detached(reference.id()).unwrap();
                        continue;
                    }
                };
                info!("Rebasing commit onto: {}", local_ref.id());

                let mut opts = RebaseOptions::new();
                opts.inmemory(true);
                let mut rebase = repo
                    .rebase(
                        Some(&reference),
                        Option::from(&local_ref),
                        None,
                        Some(&mut opts),
                    )
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
                            panic!("unknown rebase kind {kind:?}");
                        }
                    }
                }
                rebase.finish(None).unwrap();
                let new_idx = rebase.inmemory_index().unwrap();
                for item in new_idx.iter() {
                    repo_idx.add(&item).unwrap();
                }
                let last_commit = repo.find_commit(last_commit.unwrap()).unwrap();

                repo.set_head_detached(last_commit.id()).unwrap();
            }

            warn!("Rebase done, resetting head");
            let head = repo.head().unwrap().peel_to_commit().unwrap();
            repo.branch("master", &head, true).unwrap();

            warn!("Dumping packfile");
            let mut buf = Buf::new();
            mempack_backend.dump(&repo, &mut buf).unwrap();

            let mut writer = object_db.packwriter().unwrap();
            writer.write_all(&buf).unwrap();
            writer.commit().unwrap();
            warn!("Writing index");
            repo_idx.write().unwrap();
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

    let (sender, recv) = bounded::<(JsonInput, Vec<TextFile>)>(20);

    let odb = repo.odb().unwrap();
    let mempack_backend = odb.add_new_mempack_backend(3).unwrap();

    thread::scope(|s| {
        s.spawn(|_| {
            let sender = sender;
            items.into_par_iter().for_each(|item| {
                let error_ctx = format!(
                    "Name: {}, version: {}, url: {}",
                    item.name, item.version, item.url
                );
                let idx = run(item, &odb).context(error_ctx).unwrap();
                if let Some(idx) = idx {
                    if sender.send(idx).is_err() {
                        // Ignore errors sending
                    }
                }
            });
            info!("Finished par iter");
        });

        consume_queue(&repo, &odb, mempack_backend, recv)
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

fn run(item: JsonInput, repo_odb: &Odb) -> anyhow::Result<Option<(JsonInput, Vec<TextFile>)>> {
    let package_filename = item.package_filename();
    // let odb = Odb::new().unwrap();
    // odb.add_new_mempack_backend(3).unwrap();
    let new_repo = Repository::from_odb(Odb::new().unwrap()).unwrap();
    let odb = new_repo.odb().unwrap();
    odb.add_new_mempack_backend(3).unwrap();

    let mut pack = new_repo.packbuilder().unwrap();
    pack.set_threads(1);

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
            let oid = odb.write(ObjectType::Blob, &content).unwrap();
            entries.push(TextFile {
                path: path.into_bytes(),
                oid,
                size: content.len(),
            });
            pack.insert_object(oid, None).unwrap();
            has_any_text_files = true;
        }
    }

    if !has_any_text_files {
        return Ok(None);
    }

    let mut buf = Buf::new();
    pack.write_buf(&mut buf).unwrap();

    let mut writer = repo_odb.packwriter().unwrap();
    writer.write_all(&buf).unwrap();
    writer.commit().unwrap();

    Ok(Some((item, entries)))
}
