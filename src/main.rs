use flate2::read::GzDecoder;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs::{DirEntry, File};
use std::io::{BufReader, Read};

use std::{fs};

use tar::Archive;
use indicatif::{ParallelProgressIterator, ProgressStyle};
use indicatif::ProgressBar;
use content_inspector::{ContentType, inspect};

use std::path::{PathBuf};

use clap::{Parser};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg()]
    input: PathBuf,

    #[arg()]
    output: PathBuf,
}

const BUF_SIZE: usize = 1024 * 1024 * 32;

fn main() -> anyhow::Result<()> {
    let args: Cli = Cli::parse();
    let all_entries: Vec<DirEntry> = fs::read_dir(args.input)?.flatten().collect();

    let size = all_entries.len();
    let pbar = ProgressBar::new(size as u64);

    // pbar.enable_steady_tick(Duration::from_secs(1));
    let style = ProgressStyle::with_template("{prefix:>12.cyan.bold} [{bar:57}] {pos}/{len} ({eta})").unwrap();
    pbar.set_style(style);
    all_entries.into_par_iter().progress_with(pbar).for_each(|entry| {
        let path = entry.path();
        match path.extension().and_then(OsStr::to_str) {
            Some(ext) => match ext {
                "gz" => {
                    let tar_gz = BufReader::with_capacity(BUF_SIZE, File::open(path).unwrap());
                    let tar = GzDecoder::new(tar_gz);
                    let mut archive = Archive::new(tar);
                    let mut hasher = Sha256::new();

                    for mut entry in archive.entries().unwrap().flatten().filter(|v| v.size() != 0) {
                        let mut buf = Vec::with_capacity(entry.size() as usize);
                        entry.read_to_end(&mut buf).unwrap();
                        let content_type = inspect(&buf);

                        if content_type == ContentType::BINARY {
                            continue;
                        }
                        hasher.update(&buf);
                        let result = hasher.finalize_reset();
                        let file_name = format!("{result:X}");
                        let output_path = args.output.join(&file_name[0..4]).join(file_name);
                        if output_path.exists() {
                            return;
                        }
                        fs::create_dir_all(output_path.parent().unwrap()).unwrap();
                        fs::write(output_path, buf).unwrap();
                    }
                },
                "egg" | "zip" | "whl" => {
                    let zip_file = BufReader::with_capacity(BUF_SIZE, File::open(path).unwrap());
                    let mut hasher = Sha256::new();
                    match zip::ZipArchive::new(zip_file) {
                        Ok(mut archive) => (0..archive.len())
                            .map(|i| {
                                let mut entry = archive.by_index(i).unwrap();
                                if !entry.is_file() || entry.size() == 0 {
                                    return;
                                }

                                let mut buf = Vec::with_capacity(entry.size() as usize);
                                entry.read_to_end(&mut buf).unwrap();
                                let content_type = inspect(&buf);

                                if content_type == ContentType::BINARY {
                                    return;
                                }
                                hasher.update(&buf);
                                let result = hasher.finalize_reset();
                                let file_name = format!("{result:X}");
                                let output_path = args.output.join(&file_name[0..4]).join(file_name);
                                if output_path.exists() {
                                    return;
                                }

                                fs::create_dir_all(output_path.parent().unwrap()).unwrap();
                                fs::write(output_path, buf).unwrap();
                            })
                            .collect(),
                        Err(_) => {}
                    }
                }
                _ => panic!("Unhandled extension {ext}"),
            },
            None => {}
        };
    });
    Ok(())
}
