use flate2::read::GzDecoder;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs::{DirEntry, File};
use std::io::BufReader;
use std::io::Write;
use std::{env, fs, io};
use tar::Archive;

fn main() -> anyhow::Result<()> {
    let dir = env::args().last().unwrap();
    let all_entries: Vec<DirEntry> = fs::read_dir(dir)?.flatten().collect();
    all_entries.into_par_iter().for_each(|entry| {
        let path = entry.path();
        let results: Vec<String> = match path.extension().and_then(OsStr::to_str) {
            Some(ext) => match ext {
                "gz" => {
                    let tar_gz = BufReader::new(File::open(path).unwrap());
                    let tar = GzDecoder::new(tar_gz);
                    let mut archive = Archive::new(tar);
                    archive
                        .entries()
                        .unwrap()
                        .flatten()
                        .map(|mut entry| {
                            let mut hasher = Sha256::new();
                            io::copy(&mut entry, &mut hasher).unwrap();
                            let result = hasher.finalize();
                            let entry_path = entry.path().unwrap();
                            let extension = entry_path
                                .extension()
                                .and_then(|v| v.to_str())
                                .unwrap_or("None");
                            format!("{result:X} {} {}", entry.size(), extension)
                        })
                        .collect()
                }
                "egg" | "zip" | "whl" => {
                    let zip_file = BufReader::new(File::open(path).unwrap());
                    match zip::ZipArchive::new(zip_file) {
                        Ok(mut archive) => (0..archive.len())
                            .map(|i| {
                                let mut file = archive.by_index(i).unwrap();
                                let mut hasher = Sha256::new();
                                io::copy(&mut file, &mut hasher).unwrap();
                                let result = hasher.finalize();
                                let extension = match file.enclosed_name() {
                                    Some(v) => {
                                        v.extension().and_then(|v| v.to_str()).unwrap_or("None")
                                    }
                                    None => "None",
                                };
                                format!("{result:X} {} {}", file.size(), extension)
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
        for line in results {
            writeln!(lock, "{line}").unwrap();
        }
    });
    Ok(())
}
