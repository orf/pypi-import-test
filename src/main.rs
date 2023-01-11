use flate2::read::GzDecoder;
use rayon::prelude::*;
use sha2::{Digest};
use std::ffi::OsStr;
use std::fs::{DirEntry, File};
use std::io::{BufReader, Read};
use std::io::Write;
use std::{env, fs};

use tar::Archive;
use indicatif::{ParallelProgressIterator, ProgressStyle};
use indicatif::ProgressBar;
use content_inspector::{inspect};

const BUF_SIZE: usize = 1024 * 1024 * 32;

fn main() -> anyhow::Result<()> {
    let dir = env::args().last().unwrap();
    let all_entries: Vec<DirEntry> = fs::read_dir(dir)?.flatten().collect();
    let size = all_entries.len();
    let pbar = ProgressBar::new(size as u64);
    // pbar.enable_steady_tick(Duration::from_secs(1));
    let style = ProgressStyle::with_template("{prefix:>12.cyan.bold} [{bar:57}] {pos}/{len} ({eta})").unwrap();
    pbar.set_style(style);
    all_entries.into_par_iter().progress_with(pbar).for_each(|entry| {
        let path = entry.path();
        let results: Vec<_> = match path.extension().and_then(OsStr::to_str) {
            Some(ext) => match ext {
                "gz" => {
                    let tar_gz = BufReader::with_capacity(BUF_SIZE, File::open(path).unwrap());
                    let tar = GzDecoder::new(tar_gz);
                    let mut archive = Archive::new(tar);
                    archive
                        .entries()
                        .unwrap()
                        .flatten()
                        .filter(|v| v.size() != 0)
                        .map(|entry| {
                            // io::copy(&mut entry, &mut hasher).unwrap();
                            // let result = hasher.finalize_reset();
                            let extension = {
                                let x = &entry.path().unwrap();
                                x
                                    .extension()
                                    .and_then(|v| v.to_str()).map(|v| v.to_string())
                                    .unwrap_or_else(||"None".to_string())
                            };
                            let size = entry.size();
                            let mut buf = Vec::with_capacity(1024);
                            let mut first_bytes = entry.take(1024);
                            first_bytes.read_to_end(&mut buf).unwrap();
                            let res = inspect(&buf);
                            Some(format!("{size} {extension} {res}"))
                        })
                        .collect()
                }
                "egg" | "zip" | "whl" => {
                    let zip_file = BufReader::with_capacity(BUF_SIZE, File::open(path).unwrap());
                    match zip::ZipArchive::new(zip_file) {
                        Ok(mut archive) => (0..archive.len())
                            .map(|i| {
                                let file = archive.by_index(i).unwrap();
                                if !file.is_file() || file.size() == 0 {
                                    return None;
                                }
                                let extension = match file.enclosed_name() {
                                    Some(v) => {
                                        v.extension().and_then(|v| v.to_str()).unwrap_or("None")
                                    }
                                    None => "None",
                                }.to_string();
                                let size = file.size();
                                let mut buf = Vec::with_capacity(1024);
                                let mut first_bytes = file.take(1024);
                                first_bytes.read_to_end(&mut buf).unwrap();
                                // io::copy(&mut file, &mut hasher).unwrap();
                                // let result = hasher.finalize_reset();
                                let res = inspect(&buf);
                                Some(format!("{size} {extension} {res}"))
                            })
                            .collect(),
                        Err(_) => vec![],
                    }
                }
                _ => panic!("Unhandled extension {ext}"),
            },
            None => vec![],
        };
        if results.is_empty() {
            return;
        }
        let stdout = std::io::stdout();
        let mut lock = stdout.lock();
        for line in results.iter().flatten() {
            writeln!(lock, "{line}").unwrap();
        }
    });
    Ok(())
}
