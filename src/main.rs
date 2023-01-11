use flate2::read::GzDecoder;
use rayon::prelude::*;
use sha2::{Digest};
use std::ffi::OsStr;
use std::fs::{DirEntry, File};
use std::io::{BufReader, Read, Write};

use std::{fs};

use rand::thread_rng;
use rand::seq::SliceRandom;
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
    done_dir: PathBuf,

    #[arg()]
    output: PathBuf,

    #[arg()]
    limit: usize
}

const BUF_SIZE: usize = 1024 * 1024 * 32;

fn main() -> anyhow::Result<()> {
    let args: Cli = Cli::parse();
    let mut all_entries: Vec<DirEntry> = fs::read_dir(args.input)?.flatten().collect();
    all_entries.shuffle(&mut thread_rng());
    println!("Total: {}", all_entries.len());
    fs::create_dir(&args.done_dir).unwrap();
    let all_entries = &all_entries[0..args.limit];
    let size = all_entries.len();
    let pbar = ProgressBar::new(size as u64);

    let style = ProgressStyle::with_template("{prefix:>12.cyan.bold} [{bar:57}] {pos}/{len} ({eta})").unwrap();
    pbar.set_style(style);
    all_entries.into_par_iter().progress_with(pbar).for_each(|entry| {
        let path = entry.path();
        let output_path = args.output.join(path.file_name().unwrap());
        let rename_path = path.clone();
        match path.extension().and_then(OsStr::to_str) {
            Some(ext) => match ext {
                "gz" => {
                    let mut output_txt_file = File::open(output_path).unwrap();
                    let tar_gz = BufReader::with_capacity(BUF_SIZE, File::open(path).unwrap());
                    let tar = GzDecoder::new(tar_gz);
                    let mut archive = Archive::new(tar);
                    for mut entry in archive.entries().unwrap().flatten().filter(|v| v.size() != 0) {
                        let mut buf = Vec::with_capacity(entry.size() as usize);
                        entry.read_to_end(&mut buf).unwrap();
                        let content_type = inspect(&buf);

                        if content_type == ContentType::BINARY {
                            continue;
                        }
                        output_txt_file.write_all(&buf).unwrap();
                    }
                },
                "egg" | "zip" | "whl" => {
                    let zip_file = BufReader::with_capacity(BUF_SIZE, File::open(path).unwrap());
                    match zip::ZipArchive::new(zip_file) {
                        Ok(mut archive) => {
                            let mut output_txt_file = File::open(output_path).unwrap();
                            (0..archive.len())
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
                                    output_txt_file.write_all(&buf).unwrap();
                                })
                                .collect()
                        },
                        Err(_) => {}
                    }
                }
                _ => panic!("Unhandled extension {ext}"),
            },
            None => {}
        };

        let fname = &(*rename_path.file_name().unwrap());
        let new = args.done_dir.join(fname);
        fs::rename(&rename_path, new).unwrap();
    });
    Ok(())
}
