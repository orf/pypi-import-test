mod archive;
mod combine;
mod data;
mod file_inspection;
mod inspect;
mod writer;

use crossbeam::thread;
use std::fs;
use std::fs::File;
use std::io::BufReader;

use anyhow::Context;
use clap::Parser;
use git2::Repository;
use rayon::prelude::*;

use std::path::PathBuf;

use crate::data::{DownloadJob, JobInfo};
use crate::writer::{commit, flush_repo};
use crossbeam::channel::bounded;
use data::PackageInfo;

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
        job_idx: usize,
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
    ReadIndex {
        #[arg()]
        repo: PathBuf,
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
            match run_multiple(&work_path, input)? {
                PackageResult::Complete => {
                    if finished_path.exists() {
                        fs::remove_dir_all(&finished_path).unwrap();
                    }
                    fs::create_dir(&finished_path).unwrap();
                    fs::rename(&work_path, &finished_path).unwrap();
                }
                PackageResult::Empty | PackageResult::Excluded => {
                    // Delete the path
                    fs::remove_dir_all(&work_path).unwrap()
                }
            }
        }
        RunType::CreateUrls {
            data,
            output_dir,
            limit,
            find,
            split,
        } => data::extract_urls(data, output_dir, limit, find, split),
        RunType::Combine {
            job_idx,
            base_repo,
            target_repos,
        } => {
            combine::combine(job_idx, base_repo, target_repos);
        }
        RunType::ReadIndex { repo: _ } => {
            // let x = inspect::parse_index(repo);
            // println!("Total: {}", x);
        }
    }
    Ok(())
}

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

enum PackageResult {
    Complete,
    Empty,
    Excluded,
}

fn run_multiple(repo_path: &PathBuf, job: DownloadJob) -> anyhow::Result<PackageResult> {
    if file_inspection::is_excluded_package(&job.info.name) {
        return Ok(PackageResult::Excluded);
    }

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

    let (sender, recv) = bounded::<_>(20);

    let odb = repo.odb().unwrap();
    let mempack_backend = odb.add_new_mempack_backend(3).unwrap();
    let repo_idx = repo.index().unwrap();

    let mut should_copy_repo = false;

    thread::scope(|s| {
        s.spawn(|_| {
            let sender = sender;
            job.packages.into_par_iter().for_each_init(
                || {
                    Client::builder()
                        .http2_prior_knowledge()
                        .http2_adaptive_window(true)
                        .user_agent(APP_USER_AGENT)
                        .build()
                        .unwrap()
                },
                |client, item| {
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
                },
            );
        });

        for (job_info, package_info, index) in recv {
            commit(&repo, job_info, package_info, index);
            should_copy_repo = true;
        }
        if should_copy_repo {
            flush_repo(&repo, repo_idx, &odb, mempack_backend);
        }
    })
    .unwrap_or_else(|_| panic!("Error with job {}", job.info));

    if should_copy_repo {
        Ok(PackageResult::Complete)
    } else {
        Ok(PackageResult::Empty)
    }
}
