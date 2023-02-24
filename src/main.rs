mod archive;
mod combine;
mod create_urls;
mod downloader;
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

use fs_extra::dir::CopyOptions;

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
        work_path: PathBuf,

        #[arg()]
        finished_path: PathBuf,

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
            work_path,
            finished_path,
            template,
        } => {
            let opts = CopyOptions::new();
            fs::create_dir(&work_path).unwrap();
            fs_extra::dir::copy(template.join(".git/"), &work_path, &opts).unwrap();
            let work_path = fs::canonicalize(&work_path).unwrap();

            let reader = BufReader::new(File::open(&input_file).unwrap());
            let input: Vec<DownloadJob> = serde_json::from_reader(reader).unwrap();

            job::run_multiple(&work_path, input)
                .with_context(|| format!("Input file: {}", input_file.display()))?;
            if finished_path.exists() {
                fs::remove_dir_all(&finished_path).unwrap();
            }
            fs::create_dir(&finished_path).unwrap();
            fs::rename(&work_path, &finished_path).unwrap();
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
            combine::merge_all_branches(into, repos)?;
        }
    }
    Ok(())
}
