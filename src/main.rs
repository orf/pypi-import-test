use flate2::read::GzDecoder;
use rayon::prelude::*;
use std::ffi::OsStr;
use std::fs::{DirEntry, File};
use std::io::{BufReader, BufWriter, Read, Write};

use std::{fs, io};

use content_inspector::{inspect, ContentType};
use indicatif::ProgressBar;
use indicatif::{ParallelProgressIterator, ProgressStyle};
use rand::seq::SliceRandom;
use rand::thread_rng;
use tar::Archive;

use std::path::PathBuf;

use clap::Parser;

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
    limit: usize,
}

const BUF_SIZE: usize = 1024 * 1024 * 2;

fn main() -> anyhow::Result<()> {
    let args: Cli = Cli::parse();
    let mut all_entries: Vec<DirEntry> = fs::read_dir(args.input)?.flatten().collect();
    all_entries.shuffle(&mut thread_rng());
    println!("Total: {}", all_entries.len());
    fs::create_dir_all(&args.done_dir).unwrap();
    fs::create_dir_all(&args.output).unwrap();
    let all_entries = &all_entries[0..args.limit];
    let size = all_entries.len();
    let pbar = ProgressBar::new(size as u64);

    let style =
        ProgressStyle::with_template("{prefix:>12.cyan.bold} [{bar:57}] {pos}/{len} ({eta})")
            .unwrap();
    pbar.set_style(style);
    all_entries
        .into_par_iter()
        .progress_with(pbar)
        .for_each(|entry| {
            let path = entry.path();
            let output_path = args.output.join(path.file_name().unwrap());
            let rename_path = path.clone();
            if let Some(ext) = path.extension().and_then(OsStr::to_str) {
                match ext {
                    "gz" => {
                        let output_txt_file = File::create(output_path).unwrap();
                        let mut writer = BufWriter::new(output_txt_file);
                        let tar = GzDecoder::new(File::open(path).unwrap());
                        let mut archive = Archive::new(tar);
                        for mut entry in archive
                            .entries()
                            .unwrap()
                            .flatten()
                            .filter(|v| v.size() != 0)
                        {
                            let mut first = [0; 1024];
                            let n = entry.read(&mut first[..]).unwrap();
                            let content_type = inspect(&first[..n]);

                            if content_type == ContentType::BINARY {
                                continue;
                            }
                            writer.write_all(&first).unwrap();
                            io::copy(&mut entry, &mut writer).unwrap();
                        }
                    }
                    "egg" | "zip" | "whl" => {
                        let zip_file =
                            BufReader::with_capacity(BUF_SIZE, File::open(path).unwrap());
                        if let Ok(mut archive) = zip::ZipArchive::new(zip_file) {
                            let output_txt_file = File::create(output_path).unwrap();
                            let mut writer = BufWriter::new(output_txt_file);
                            (0..archive.len()).for_each(|i| {
                                let mut entry = archive.by_index(i).unwrap();
                                if !entry.is_file() || entry.size() == 0 {
                                    return;
                                }
                                let mut first = [0; 1024];
                                let n = entry.read(&mut first[..]).unwrap();
                                let content_type = inspect(&first[..n]);
                                if content_type == ContentType::BINARY {
                                    return;
                                }
                                writer.write_all(&first).unwrap();
                                io::copy(&mut entry, &mut writer).unwrap();
                            })
                        }
                    }
                    _ => panic!("Unhandled extension {ext}"),
                }
            };

            let fname = rename_path.file_name().unwrap();
            let new = args.done_dir.join(fname);
            fs::rename(&rename_path, new).unwrap();
        });
    Ok(())
}
