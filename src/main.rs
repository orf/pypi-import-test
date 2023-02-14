mod archive;
mod combine;
mod data;
mod file_inspection;
mod inspect;
mod pusher;
mod writer;
mod downloader;

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
use crossbeam::channel::unbounded;
use data::PackageInfo;
use fs_extra::dir::CopyOptions;

use reqwest::blocking::Client;
use url::Url;
use writer::PackageResult;

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

        #[arg()]
        template: PathBuf,
    },
    Combine {
        #[arg()]
        job_idx: usize,
        #[arg()]
        base_repo: PathBuf,
        #[arg()]
        target_repos: Vec<PathBuf>,
    },
    RepoStats {
        #[arg()]
        base_repos: Vec<PathBuf>,
    },
    Push {
        #[arg()]
        strategy: String,
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
            writer::run_multiple(
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
                        sort_key: None,
                        uploaded_on: Default::default(),
                    }],
                },
            )?;
        }
        RunType::FromJson {
            input_file,
            work_path,
            finished_path,
            template,
        } => {
            let opts = CopyOptions::new();
            fs::create_dir(&work_path).unwrap();
            fs_extra::dir::copy(&template.join(".git/"), &work_path, &opts).unwrap();
            let work_path = fs::canonicalize(&work_path).unwrap();

            let reader = BufReader::new(File::open(input_file).unwrap());
            let input: DownloadJob = serde_json::from_reader(reader).unwrap();
            match writer::run_multiple(&work_path, input)? {
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
        RunType::RepoStats { base_repos } => {
            base_repos.into_par_iter().for_each(|base_repo| {
                let output = pusher::get_repo_statistics(base_repo);
                println!("{}", serde_json::to_string(&output).unwrap());
            });
        }
        RunType::Push { strategy } => {
            pusher::push(strategy);
        }
        RunType::ReadIndex { repo: _ } => {
            // let x = inspect::parse_index(repo);
            // println!("Total: {}", x);
        }
    }
    Ok(())
}
