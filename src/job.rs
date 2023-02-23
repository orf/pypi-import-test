use crate::archive::PackageArchive;
use crate::downloader::download_multiple;
use crate::create_urls::DownloadJob;
use git2::{Buf, Index, IndexEntry, IndexTime, Mempack, Odb, Repository, Signature, Time};
use log::error;
use serde::{Deserialize, Serialize};
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use crate::utils::create_pbar;

#[derive(Debug, Deserialize, Serialize)]
pub struct CommitMessage<'a> {
    pub name: &'a str,
    pub version: &'a str,
    pub file: &'a str,
    pub path: String,
}

pub fn run_multiple(repo_path: &PathBuf, jobs: Vec<DownloadJob>) -> anyhow::Result<()> {
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
    let odb = repo.odb().unwrap();
    let mempack_backend = odb.add_new_mempack_backend(3).unwrap();
    let mut index = repo.index().unwrap();

    let downloaded = download_multiple(jobs);
    let total = downloaded.len();
    let pbar = create_pbar(total as u64, "Extracting");
    for (job, temp_dir, download_path) in downloaded.into_iter() {
        pbar.inc(1);
        let extract_start = Instant::now();
        let extract_result = extract(&job, download_path, &odb, &mut index);
        let extract_time = extract_start.elapsed().as_secs_f32();

        match extract_result {
            Ok(v) => match v {
                None => {}
                Some((path, total_files)) => {
                    let start = Instant::now();
                    commit(&repo, &mut index, &job, path);
                    let commit_time = start.elapsed().as_secs_f32();
                    if extract_time > 0.5 || commit_time > 0.5 {
                        let position = pbar.position();
                        println!("[{position} / {total}] Finished {} {}. Files: {total_files} / Extract time: {extract_time:.3} / Commit time: {commit_time:.3} / Index size: {}", job.name, job.version, index.len());
                    }
                }
            },
            Err(e) => {
                error!("Error running job: {e}");
            }
        }
        drop(temp_dir);
    }

    flush_repo(&repo, index, &odb, mempack_backend);

    Ok(())
}

pub fn extract(
    job: &DownloadJob,
    archive_path: PathBuf,
    odb: &Odb,
    index: &mut Index,
) -> anyhow::Result<Option<(String, usize)>> {
    let package_filename = job.package_filename();
    let package_extension = package_filename.rsplit('.').next().unwrap();
    let mut archive =
        match PackageArchive::new(package_extension, File::open(archive_path).unwrap()) {
            None => {
                return Ok(None);
            }
            Some(v) => v,
        };
    let tar_gz_first_segment = format!("{}-{}/", job.name, job.version).to_ascii_lowercase();
    let prefix = format!("packages/{package_filename}/");
    let index_time = IndexTime::new(job.uploaded_on.timestamp() as i32, 0);
    let mut file_count = 0;
    for v in archive.all_items(odb) {
        let (file_name, size, oid) = match v {
            Ok(v) => v,
            Err(e) => {
                error!("Error with package {}: {e}", job.url);
                continue;
            }
        };

        let file_name = if file_name.to_ascii_lowercase().starts_with(&tar_gz_first_segment) {
            &file_name[tar_gz_first_segment.len()..]
        } else {
            &*file_name
        };

        // println!("File name: {package_filename} '{tar_gz_first_segment}' - '{file_name}'");

        // Some paths are weird. A release in backports.ssl_match_hostname contains
        // files with double slashes: `src/backports/ssl_match_hostname//backports.ssl_match_hostname-3.4.0.1.tar.gz.asc`
        // This might be an issue with my code somewhere, but everything else seems to be fine.
        let path = format!("{}/{file_name}", prefix)
            .replace("/./", "/")
            .replace("/../", "/")
            .replace("//", "/");

        let entry = IndexEntry {
            ctime: index_time,
            mtime: index_time,
            dev: 0,
            ino: 0,
            mode: 0o100644,
            uid: 0,
            gid: 0,
            file_size: size as u32,
            id: oid,
            flags: 0,
            flags_extended: 0,
            path: path.into_bytes(),
        };
        index.add(&entry).unwrap();
        file_count += 1;
    }

    if file_count == 0 {
        Ok(None)
    } else {
        Ok(Some((prefix, file_count)))
    }
}

pub fn commit(repo: &Repository, index: &mut Index, info: &DownloadJob, code_path: String) {
    let filename = info.package_filename();
    // let index_time = IndexTime::new(i.uploaded_on.timestamp() as i32, 0);
    // let total_bytes = index.iter().map(|v| v.size).sum::<usize>();
    let signature = Signature::new(
        "Tom Forbes",
        "tom@tomforb.es",
        &Time::new(info.uploaded_on.timestamp(), 0),
    )
        .unwrap();
    let oid = index.write_tree_to(repo).unwrap();

    let tree = repo.find_tree(oid).unwrap();
    let parent = &repo.head().unwrap().peel_to_commit().unwrap();

    let commit_message = serde_json::to_string(&CommitMessage {
        name: &info.name,
        version: &info.version,
        file: filename,
        path: code_path,
    })
        .unwrap();
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        &commit_message,
        &tree,
        &[parent],
    )
        .unwrap();
}

pub fn flush_repo(
    repo: &Repository,
    mut repo_idx: Index,
    object_db: &Odb,
    mempack_backend: Mempack,
) {
    // match repo.head() {
    //     Ok(h) => {
    //         let commit = h.peel_to_commit().unwrap();
    //         repo.branch(&format!("{}-{}", info.name, info.chunk), &commit, true)
    //             .unwrap();
    //     }
    //     Err(e) => {
    //         panic!("Could not get repo head? {e}");
    //     }
    // }
    let mut buf = Buf::new();
    mempack_backend.dump(repo, &mut buf).unwrap();

    let mut writer = object_db.packwriter().unwrap();
    writer.write_all(&buf).unwrap();
    writer.commit().unwrap();
    repo_idx.write().unwrap();
}
