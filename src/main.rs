mod archive;
mod combine;
mod create_urls;
mod file_inspection;
mod inspect;
mod job;
mod utils;

use std::fs;
use std::fs::File;
use std::io::BufReader;

use clap::Parser;

use anyhow::Context;
use std::path::PathBuf;

use crate::create_urls::DownloadJob;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    run_type: RunType,
}

#[derive(clap::Subcommand)]
enum RunType {
    FromJson {
        #[arg()]
        input_file: PathBuf,

        #[arg()]
        work_dir: PathBuf,

        #[arg()]
        finished_dir: PathBuf,

        #[arg()]
        template: PathBuf,
    },
    CreateUrls {
        #[arg()]
        data: PathBuf,
        #[arg()]
        output_dir: PathBuf,
        #[arg(long, short)]
        limit: Option<usize>,
        #[arg(long, short)]
        find: Option<Vec<String>>,
        #[arg(long, short, default_value = "5000")]
        split: usize,
    },
    MergeBranches {
        #[arg()]
        into: PathBuf,
        #[arg()]
        repos: Vec<PathBuf>,
    },
    ParseFile {
        #[arg()]
        file: PathBuf,
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
        RunType::FromJson {
            input_file,
            work_dir,
            finished_dir,
            template: _,
        } => {
            let reader = BufReader::new(File::open(&input_file).unwrap());
            let input: Vec<DownloadJob> = serde_json::from_reader(reader).unwrap();

            let first_job_time = input.iter().map(|v| v.uploaded_on).min().unwrap();
            let repo_path = work_dir.join(format!("{first_job_time}"));
            let finished_path = finished_dir.join(format!("{first_job_time}"));

            // let opts = CopyOptions::new();
            // fs::create_dir(&repo_path).unwrap();
            // fs_extra::dir::copy(template.join(".git/"), &repo_path, &opts).unwrap();
            // let repo_path = fs::canonicalize(&repo_path).unwrap();

            job::run_multiple(&repo_path, input)
                .with_context(|| format!("Input file: {}", input_file.display()))
                .unwrap();
            if finished_path.exists() {
                fs::remove_dir_all(&finished_path).unwrap();
            }
            fs::create_dir(&finished_path).unwrap();
            fs::rename(&repo_path, &finished_path).unwrap();
        }
        RunType::CreateUrls {
            data,
            output_dir,
            limit,
            find,
            split,
        } => create_urls::extract_urls(data, output_dir, limit, find, split),
        RunType::ParseFile { .. } => {
            // inspect::parse_file(file)
        }
        RunType::ReadIndex { .. } => {
            // inspect::parse(repo);
            // let x = inspect::parse_index(repo);
            // println!("Total: {}", x);
        }
        RunType::MergeBranches { into, repos } => {
            // let into = fs::canonicalize(into)?;
            // To-do: handle errors here
            // let repos = repos.into_iter().map(|v| fs::canonicalize(v).unwrap()).collect();
            combine::merge_all_branches(into, repos)?;
        }
    }
    Ok(())
}
