mod archive;
mod data;
mod writer;


use crossbeam::thread;
use std::fs;
use std::fs::File;
use std::io::{BufReader, Write};

use anyhow::Context;
use clap::Parser;
use git2::{
    Buf, RebaseOperationType, RebaseOptions, Repository, RepositoryInitOptions,
    Signature, Time,
};
use rayon::prelude::*;

use std::path::PathBuf;

use crate::data::{DownloadJob, JobInfo};
use crate::writer::{commit, flush_repo, TextFile};
use crossbeam::channel::bounded;
use data::PackageInfo;
use log::{info, warn};
use reqwest::blocking::Client;
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
        #[arg(long, short, default_value = "500")]
        split: usize,
    },
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
                DownloadJob {
                    info: JobInfo {
                        name,
                        total: 1,
                        chunk: 0,
                    },
                    packages: vec![PackageInfo {
                        version,
                        url,
                        index: 0,
                        uploaded_on: Default::default(),
                    }],
                },
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
            let input: DownloadJob = serde_json::from_reader(reader).unwrap();
            run_multiple(&work_path, input)?;
            fs::create_dir(&finished_path).unwrap();
            fs::rename(&work_path, &finished_path).unwrap();
        }
        RunType::CreateUrls {
            data,
            output_dir,
            limit,
            find,
            split,
        } => data::extract_urls(data, output_dir, limit, find, split),
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

fn run_multiple(repo_path: &PathBuf, job: DownloadJob) -> anyhow::Result<()> {
    git2::opts::strict_object_creation(false);
    git2::opts::strict_hash_verification(false);

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

    let (sender, recv) = bounded::<(&JobInfo, PackageInfo, Vec<TextFile>)>(20);

    let odb = repo.odb().unwrap();
    let mempack_backend = odb.add_new_mempack_backend(3).unwrap();
    let mut repo_idx = repo.index().unwrap();

    thread::scope(|s| {
        s.spawn(|_| {
            let sender = sender;
            job.packages
                .into_par_iter()
                .for_each_init(Client::new, |client, item| {
                    let error_ctx = format!(
                        "Name: {}, version: {}, url: {}",
                        job.info.name, item.version, item.url
                    );
                    let idx = writer::run(client, &job.info, item, &odb)
                        .context(error_ctx)
                        .unwrap();
                    if let Some(idx) = idx {
                        if sender.send(idx).is_err() {
                            // Ignore errors sending
                        }
                    }
                });
        });

        for (job_info, package_info, index) in recv {
            commit(&repo, &mut repo_idx, job_info, package_info, index);
        }
        flush_repo(&repo,  repo_idx, &odb, mempack_backend);
        // consume_queue(&repo, &odb, mempack_backend, recv)
    })
        .unwrap();

    Ok(())
}
