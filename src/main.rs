use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs::File;
use std::io::BufReader;
use std::ptr::hash;
use std::{env, fs, io};
use tar::Archive;
use zip::result::ZipResult;

fn main() -> anyhow::Result<()> {
    let dir = env::args().last().unwrap();
    let mut errors = 0;
    for entry in fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        if let Some(ext) = path.extension().and_then(OsStr::to_str) {
            match ext {
                "gz" => {
                    let tar_gz = BufReader::new(File::open(path)?);
                    let tar = GzDecoder::new(tar_gz);
                    let mut archive = Archive::new(tar);
                    for mut entry in archive.entries()?.flatten().into_iter() {
                        let mut hasher = Sha256::new();
                        io::copy(&mut entry, &mut hasher)?;
                        let result = hasher.finalize();
                        let entry_path = entry.path()?;
                        let extension = entry_path
                            .extension()
                            .and_then(|v| v.to_str())
                            .unwrap_or("None");
                        println!("{result:X} {} {}", entry.size(), extension);
                    }
                }
                "egg" | "zip" | "whl" => {
                    let zip_file = BufReader::new(File::open(path)?);
                    match zip::ZipArchive::new(zip_file) {
                        Ok(mut archive) => {
                            for i in 0..archive.len() {
                                let mut file = archive.by_index(i).unwrap();
                                let mut hasher = Sha256::new();
                                io::copy(&mut file, &mut hasher)?;
                                let result = hasher.finalize();
                                let extension = match file.enclosed_name() {
                                    Some(v) => {
                                        v.extension().and_then(|v| v.to_str()).unwrap_or("None")
                                    }
                                    None => "None",
                                };
                                println!("{result:X} {} {}", file.size(), extension);
                            }
                        }
                        Err(_) => continue,
                    }
                }
                _ => panic!("Unhandled extension {ext}"),
            }
        }
    }
    println!("Errors: {errors}");
    Ok(())
}
